//! Core Team, Quick-session and QSW-to-ESW promotion contract.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use kontor_context::{TestAttempt, TestResult, WorkspaceRef};
use kontor_core::id::{
    AgentRunId, BoundedText, CanonicalDocument, ContentHash, ContextPackId, ExternalId,
    ExternalName, IdempotencyKey, ProjectId, RealmId, RoleCatalogId, RoleCode, SCHEMA_VERSION,
    SeatBindingId, SpecVersion, Timestamp, TopologyKindKey, TopologySpecId,
};
use kontor_core::spec::{
    CodeLifecycle, RoleCatalogEntry, RoleCatalogRevision, RoleSegment, TopologySnapshot,
};
use kontor_core::{DomainError, DomainResult};
use kontor_teams::{
    CoreTeamRevision, CoreTeamSeatSelection, EpicPresence, OperationalEffects, OperationalKinds,
    OperationalWorkflow, PinnedConfiguration, ProjectSessionBaseBinding, PromotionPlan,
    PromotionTarget, QuickSession, QuickSessionRequest, QuickSourceEvidence, SourceDisposition,
};

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("valid external name")
}

fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("valid bounded text")
}

fn external(value: &str) -> ExternalId {
    ExternalId::parse(value).expect("valid external id")
}

fn at(value: &str) -> Timestamp {
    value.parse().expect("valid timestamp")
}

fn hash(value: &str) -> ContentHash {
    CanonicalDocument::from_serializable(
        &serde_json::json!({ "schema_version": SCHEMA_VERSION, "value": value }),
    )
    .expect("canonical document")
    .hash()
    .clone()
}

fn catalog() -> RoleCatalogRevision {
    let roles = [
        ("LSA", "Lead Software Architect", RoleSegment::Architecture),
        (
            "TPM",
            "Technical Program Manager",
            RoleSegment::ProductDelivery,
        ),
        ("SA", "Software Architect", RoleSegment::Architecture),
        (
            "QA",
            "Quality Assurance Engineer",
            RoleSegment::QualityTesting,
        ),
    ]
    .into_iter()
    .map(|(code, title, segment)| RoleCatalogEntry {
        role_code: RoleCode::parse(code).expect("valid role code"),
        standard_title: name(title),
        segment,
        responsibility_summary: text("A bounded standard responsibility."),
        lifecycle: CodeLifecycle::Current,
        capability_defaults: Vec::new(),
    })
    .collect();
    RoleCatalogRevision {
        schema_version: SCHEMA_VERSION,
        catalog_id: RoleCatalogId::generate(),
        version: SpecVersion::FIRST,
        name: name("Operational roles"),
        roles,
    }
}

fn selection(role: &str, presence: EpicPresence, ad_hoc_allowed: bool) -> CoreTeamSeatSelection {
    CoreTeamSeatSelection {
        role_code: RoleCode::parse(role).expect("valid role code"),
        custom_display_name: (role == "SA").then(|| name("Domain architect")),
        presence,
        ad_hoc_allowed,
    }
}

fn roster(version: SpecVersion) -> CoreTeamRevision {
    CoreTeamRevision::resolve(
        version,
        &catalog(),
        &[
            selection("SA", EpicPresence::Default, true),
            selection("QA", EpicPresence::OnDemand, false),
        ],
    )
    .expect("valid Core Team")
}

fn kinds() -> OperationalKinds {
    OperationalKinds {
        quick: TopologyKindKey::parse("QSW").expect("valid kind"),
        epic: TopologyKindKey::parse("ESW").expect("valid kind"),
        control: TopologyKindKey::parse("ECP").expect("valid kind"),
    }
}

fn source(realm_id: RealmId) -> QuickSourceEvidence {
    QuickSourceEvidence {
        realm_id,
        source_run_id: AgentRunId::generate(),
        context_pack_id: ContextPackId::generate(),
        context_pack_hash: hash("context-pack"),
        workspace: WorkspaceRef {
            root: text("/workspace/kontor"),
            branch: name("feat/quick"),
            baseline_commit: external("abc123"),
        },
        attempted_work: vec![text("Investigated the topology")],
        touched_files: vec![text("notes/topology.md")],
        commits: vec![external("def456")],
        tests: vec![TestAttempt {
            command: text("cargo test -p kontor-teams"),
            result: TestResult::Passed,
        }],
        decisions: vec![text("Use one ECP for both control seats")],
        evidence: vec![external("evidence-1")],
        remaining_work: vec![text("Implement the promoted epic")],
        risks: vec![text("Runtime placement may drift")],
        recommended_next_action: text("Review the immutable promotion handoff"),
    }
}

fn quick_request(project_id: ProjectId, realm_id: RealmId, role: &str) -> QuickSessionRequest {
    QuickSessionRequest {
        project_id,
        role_code: RoleCode::parse(role).expect("valid role code"),
        custom_display_name: Some(name("Discovery architect")),
        purpose: text("Design the Operational bootstrap"),
        source: source(realm_id),
        requested_at: at("2026-08-17T11:00:00Z"),
    }
}

fn topology() -> TopologySnapshot {
    TopologySnapshot {
        spec_id: TopologySpecId::generate(),
        version: SpecVersion::FIRST,
        canonical_hash: hash("topology"),
    }
}

#[derive(Default)]
struct FakeEffects {
    quick_sessions: BTreeSet<String>,
    mini_projects: BTreeSet<String>,
    nodes: BTreeSet<String>,
    seats: BTreeSet<String>,
    handoffs: BTreeMap<String, ContentHash>,
    archived: BTreeSet<String>,
    fail_delivery_once: bool,
    materialize_epic_calls: usize,
}

#[async_trait]
impl OperationalEffects for FakeEffects {
    async fn materialize_quick(
        &mut self,
        base: &ProjectSessionBaseBinding,
        session: &QuickSession,
    ) -> DomainResult<()> {
        if base.topology_node_id != session.psw_topology_node_id {
            return Err(DomainError::invalid(
                "FakeEffects",
                "Quick session has the wrong PSW parent",
            ));
        }
        self.quick_sessions.insert(session.id.to_string());
        Ok(())
    }

    async fn materialize_epic(&mut self, plan: &PromotionPlan) -> DomainResult<()> {
        self.materialize_epic_calls += 1;
        self.mini_projects.insert(plan.mini_project_id.to_string());
        self.nodes
            .extend(plan.nodes.iter().map(|node| node.id.to_string()));
        self.seats
            .extend(plan.seats.iter().map(|seat| seat.id.to_string()));
        Ok(())
    }

    async fn deliver_handoff(
        &mut self,
        lsa_seat_binding_id: SeatBindingId,
        handoff: &CanonicalDocument,
    ) -> DomainResult<()> {
        if std::mem::take(&mut self.fail_delivery_once) {
            return Err(DomainError::invalid(
                "FakeEffects",
                "simulated lost delivery acknowledgement",
            ));
        }
        self.handoffs
            .insert(lsa_seat_binding_id.to_string(), handoff.hash().clone());
        Ok(())
    }

    async fn archive_quick(&mut self, session: &QuickSession) -> DomainResult<()> {
        self.archived.insert(session.id.to_string());
        Ok(())
    }
}

fn configure(workflow: &mut OperationalWorkflow, project_id: ProjectId) -> CoreTeamRevision {
    let core_team = roster(SpecVersion::FIRST);
    let (_, preview_hash) = workflow
        .preview_core_team(project_id, core_team.clone())
        .expect("Core Team preview");
    workflow
        .apply_core_team(
            &IdempotencyKey::parse("core-team-v1").expect("valid key"),
            project_id,
            core_team.clone(),
            &preview_hash,
        )
        .expect("Core Team apply");
    workflow
        .bind_project_base(
            project_id,
            ProjectSessionBaseBinding {
                topology_node_id: kontor_core::id::TopologyNodeId::generate(),
                configured_native_project_id: external("project-native-1"),
                observed_native_project_id: external("project-native-1"),
            },
        )
        .expect("matching PSW readback");
    core_team
}

#[test]
fn core_team_forces_distinct_control_roles_and_derives_quick_and_initial_seats() {
    let roster = roster(SpecVersion::FIRST);
    let codes: Vec<&str> = roster
        .seats
        .iter()
        .map(|seat| seat.role.role_code.as_str())
        .collect();
    assert_eq!(codes, ["LSA", "TPM", "SA", "QA"]);
    assert_eq!(
        roster
            .quick_roles()
            .iter()
            .map(|role| role.role_code.as_str())
            .collect::<Vec<_>>(),
        ["LSA", "SA"]
    );
    assert_eq!(
        roster
            .initial_epic_seats()
            .iter()
            .map(|seat| seat.role.role_code.as_str())
            .collect::<Vec<_>>(),
        ["LSA", "TPM", "SA"],
        "on-demand QA stays absent"
    );
    let architect = roster
        .seats
        .iter()
        .find(|seat| seat.role.role_code.as_str() == "SA")
        .expect("SA remains present");
    assert_eq!(architect.role.standard_title.as_str(), "Software Architect");
    assert_eq!(architect.role.display_name().as_str(), "Domain architect");
    assert_eq!(architect.role_slot_id.as_str(), "sa");
    assert!(
        roster
            .seats
            .iter()
            .any(|seat| seat.role.role_code.as_str() == "LSA"),
        "SA never satisfies the distinct mandatory LSA slot"
    );
    assert!(
        CoreTeamRevision::resolve(
            SpecVersion::FIRST,
            &catalog(),
            &[selection("LSA", EpicPresence::OnDemand, true)],
        )
        .is_err(),
        "a project cannot weaken the mandatory LSA seat"
    );
}

#[tokio::test]
async fn qsw_to_esw_promotion_is_idempotent_and_hands_off_to_the_lsa() {
    let realm_id = RealmId::generate();
    let project_id = ProjectId::generate();
    let mut workflow = OperationalWorkflow::new(realm_id, kinds());
    let original_roster = configure(&mut workflow, project_id);
    let mut effects = FakeEffects::default();
    let quick_key = IdempotencyKey::parse("quick-design").expect("valid key");
    let request = quick_request(project_id, realm_id, "SA");
    let quick = workflow
        .ensure_quick_session(&quick_key, &request, &mut effects)
        .await
        .expect("eligible Quick session");
    let replay = workflow
        .ensure_quick_session(&quick_key, &request, &mut effects)
        .await
        .expect("lost Quick acknowledgement replay");
    assert_eq!(quick.id, replay.id);
    assert_eq!(effects.quick_sessions.len(), 1);
    assert_eq!(quick.role.role_code.as_str(), "SA");
    assert_eq!(quick.role.standard_title.as_str(), "Software Architect");
    assert_eq!(quick.role.display_name().as_str(), "Discovery architect");
    assert_eq!(quick.kind.as_str(), "QSW");

    let preview = workflow
        .preview_promotion(
            quick.id,
            PromotionTarget {
                name: name("Operational bootstrap"),
                activate_asma_epic: true,
                confirmed_jira_epic_id: Some(external("ASMA-9000")),
            },
            topology(),
            vec![PinnedConfiguration {
                id: name("operational-default"),
                version: SpecVersion::FIRST,
                hash: hash("operational-default-v1"),
            }],
            SourceDisposition::Idle,
        )
        .expect("promotion preview");

    // A later project edit cannot reach the already-owned preview snapshot.
    let next_roster = CoreTeamRevision::resolve(
        SpecVersion::parse(2).expect("version two"),
        &catalog(),
        &[selection("QA", EpicPresence::Default, false)],
    )
    .expect("next Core Team");
    let (_, next_hash) = workflow
        .preview_core_team(project_id, next_roster.clone())
        .expect("next preview");
    workflow
        .apply_core_team(
            &IdempotencyKey::parse("core-team-v2").expect("valid key"),
            project_id,
            next_roster,
            &next_hash,
        )
        .expect("next Core Team applies");
    assert_eq!(preview.plan.core_team, original_roster);
    assert_eq!(
        preview
            .plan
            .seats
            .iter()
            .map(|seat| seat.role.role_code.as_str())
            .collect::<Vec<_>>(),
        ["LSA", "TPM", "SA"]
    );
    assert_eq!(preview.plan.nodes[1].parent_id, preview.plan.nodes[0].id);
    assert!(
        preview
            .plan
            .seats
            .iter()
            .all(|seat| seat.topology_node_id == preview.plan.nodes[1].id),
        "every stable epic role shares the one ECP"
    );

    effects.fail_delivery_once = true;
    let apply_key = IdempotencyKey::parse("promote-design").expect("valid key");
    assert!(
        workflow
            .apply_promotion(
                &apply_key,
                quick.id,
                &preview.preview_hash,
                quick.revision,
                &mut effects,
            )
            .await
            .is_err(),
        "a missing handoff acknowledgement cannot report promotion success"
    );
    let encoded = serde_json::to_vec(&workflow).expect("workflow is durable data");
    workflow = serde_json::from_slice(&encoded).expect("workflow restarts from durable data");
    let outcome = workflow
        .apply_promotion(
            &apply_key,
            quick.id,
            &preview.preview_hash,
            quick.revision,
            &mut effects,
        )
        .await
        .expect("same durable plan resumes after a partial effect");
    let calls_after_success = effects.materialize_epic_calls;
    let replay = workflow
        .apply_promotion(
            &apply_key,
            quick.id,
            &preview.preview_hash,
            quick.revision,
            &mut effects,
        )
        .await
        .expect("lost apply acknowledgement replays the outcome");

    assert_eq!(outcome, replay);
    assert_eq!(effects.materialize_epic_calls, calls_after_success);
    assert_eq!(effects.mini_projects.len(), 1);
    assert_eq!(effects.nodes.len(), 2, "one ESW and one ECP");
    assert_eq!(effects.seats.len(), 3, "required/default seats once");
    assert_eq!(effects.handoffs.len(), 1);
    assert_eq!(effects.archived.len(), 0, "idle is the default disposition");
    assert_eq!(
        effects.handoffs[&outcome.lsa_seat_binding_id.to_string()],
        outcome.handoff_hash
    );
}

#[tokio::test]
async fn invalid_role_base_and_asma_activation_refuse_before_effects() {
    let realm_id = RealmId::generate();
    let project_id = ProjectId::generate();
    let mut workflow = OperationalWorkflow::new(realm_id, kinds());
    let core_team = roster(SpecVersion::FIRST);
    let (_, hash) = workflow
        .preview_core_team(project_id, core_team.clone())
        .expect("Core Team preview");
    workflow
        .apply_core_team(
            &IdempotencyKey::parse("core-only").expect("valid key"),
            project_id,
            core_team,
            &hash,
        )
        .expect("Core Team apply");
    let mut effects = FakeEffects::default();
    assert!(
        workflow
            .ensure_quick_session(
                &IdempotencyKey::parse("unbound").expect("valid key"),
                &quick_request(project_id, realm_id, "SA"),
                &mut effects,
            )
            .await
            .is_err()
    );
    assert!(effects.quick_sessions.is_empty());

    assert!(
        workflow
            .bind_project_base(
                project_id,
                ProjectSessionBaseBinding {
                    topology_node_id: kontor_core::id::TopologyNodeId::generate(),
                    configured_native_project_id: external("configured"),
                    observed_native_project_id: external("different"),
                },
            )
            .is_err(),
        "a PSW mismatch is never accepted as a new fallback project"
    );
    workflow
        .bind_project_base(
            project_id,
            ProjectSessionBaseBinding {
                topology_node_id: kontor_core::id::TopologyNodeId::generate(),
                configured_native_project_id: external("configured"),
                observed_native_project_id: external("configured"),
            },
        )
        .expect("matching base");
    assert!(
        workflow
            .ensure_quick_session(
                &IdempotencyKey::parse("ineligible").expect("valid key"),
                &quick_request(project_id, realm_id, "QA"),
                &mut effects,
            )
            .await
            .is_err()
    );
    let quick = workflow
        .ensure_quick_session(
            &IdempotencyKey::parse("eligible").expect("valid key"),
            &quick_request(project_id, realm_id, "SA"),
            &mut effects,
        )
        .await
        .expect("eligible Quick session");
    assert!(
        workflow
            .preview_promotion(
                quick.id,
                PromotionTarget {
                    name: name("Unbound ASMA epic"),
                    activate_asma_epic: true,
                    confirmed_jira_epic_id: None,
                },
                topology(),
                Vec::new(),
                SourceDisposition::Idle,
            )
            .is_err(),
        "ASMA policy cannot activate without a confirmed Jira Epic binding"
    );

    let preview = workflow
        .preview_promotion(
            quick.id,
            PromotionTarget {
                name: name("Tracker-neutral work"),
                activate_asma_epic: false,
                confirmed_jira_epic_id: None,
            },
            topology(),
            Vec::new(),
            SourceDisposition::Archive,
        )
        .expect("explicit archive preview");
    workflow
        .apply_promotion(
            &IdempotencyKey::parse("archive-source").expect("valid key"),
            quick.id,
            &preview.preview_hash,
            quick.revision,
            &mut effects,
        )
        .await
        .expect("explicit archive applies after handoff");
    assert_eq!(effects.archived, BTreeSet::from([quick.id.to_string()]));
}
