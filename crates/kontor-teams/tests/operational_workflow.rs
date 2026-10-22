//! Core Team, Quick-session and QSW-to-ESW promotion contract.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use kontor_context::{TestAttempt, TestResult, WorkspaceRef};
use kontor_core::id::{
    AgentRunId, BoundedText, CanonicalDocument, ContentHash, ContextPackId, ExternalId,
    ExternalName, IdempotencyKey, MiniProjectId, ProjectId, RealmId, RoleCatalogId, RoleCode,
    SCHEMA_VERSION, SeatBindingId, SpecVersion, Timestamp, TopologyKindKey, TopologySpecId,
};
use kontor_core::spec::{
    CodeLifecycle, RoleCatalogEntry, RoleCatalogRevision, RoleSegment, TopologySnapshot,
};
use kontor_core::{DomainError, DomainResult};
use kontor_teams::{
    CoreTeamRevision, CoreTeamSeatSelection, EpicPresence, OperationalEffects, OperationalKinds,
    OperationalWorkflow, PinnedConfiguration, ProjectSessionBaseBinding, PromotionPlan,
    PromotionSeat, PromotionTarget, QuickSession, QuickSessionRequest, QuickSourceEvidence,
    SourceDisposition,
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

fn purpose_roles() -> kontor_teams::CoreTeamPurposeRoles<'static> {
    static LEAD: std::sync::OnceLock<RoleCode> = std::sync::OnceLock::new();
    static PROCESS: std::sync::OnceLock<RoleCode> = std::sync::OnceLock::new();
    kontor_teams::CoreTeamPurposeRoles {
        architecture_closer: LEAD.get_or_init(|| RoleCode::parse("LSA").unwrap()),
        process_closer: PROCESS.get_or_init(|| RoleCode::parse("TPM").unwrap()),
        control: PROCESS.get_or_init(|| RoleCode::parse("TPM").unwrap()),
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
        purpose_roles(),
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
    roster_seats: BTreeSet<String>,
    handoffs: BTreeMap<String, ContentHash>,
    archived: BTreeSet<String>,
    fail_delivery_once: bool,
    materialize_epic_calls: usize,
    materialize_roster_calls: usize,
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

    async fn materialize_roster(
        &mut self,
        mini_project_id: MiniProjectId,
        seats: &[PromotionSeat],
    ) -> DomainResult<()> {
        assert!(self.mini_projects.contains(&mini_project_id.to_string()));
        self.materialize_roster_calls += 1;
        self.roster_seats
            .extend(seats.iter().map(|seat| seat.id.to_string()));
        self.seats
            .extend(seats.iter().map(|seat| seat.id.to_string()));
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
            purpose_roles()
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
        purpose_roles(),
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

    let roster_preview = workflow
        .preview_roster_upgrade(quick.id)
        .expect("explicit roster-upgrade preview");
    assert_eq!(roster_preview.plan.from_version, SpecVersion::FIRST);
    assert_eq!(
        roster_preview
            .plan
            .new_seats
            .iter()
            .map(|seat| seat.role.role_code.as_str())
            .collect::<Vec<_>>(),
        ["QA"],
        "only the newly default role is materialized"
    );
    let roster_key = IdempotencyKey::parse("upgrade-roster-v2").expect("valid key");
    let upgraded = workflow
        .apply_roster_upgrade(&roster_key, &roster_preview, &mut effects)
        .await
        .expect("roster upgrade applies");
    let roster_calls_after_success = effects.materialize_roster_calls;
    let roster_replay = workflow
        .apply_roster_upgrade(&roster_key, &roster_preview, &mut effects)
        .await
        .expect("roster upgrade acknowledgement replays");
    assert_eq!(upgraded, roster_replay);
    assert_eq!(effects.materialize_roster_calls, roster_calls_after_success);
    assert_eq!(effects.roster_seats.len(), 1);
    assert_eq!(
        workflow
            .epic_roster(quick.id)
            .expect("upgraded epic roster")
            .version,
        SpecVersion::parse(2).expect("version two")
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

#[test]
fn resolved_purposes_are_ordered_and_process_control_share_one_required_seat() {
    use kontor_teams::CoreTeamSeatPurpose::*;
    let roster = roster(SpecVersion::FIRST);
    let architecture = roster.seat_for_purpose(ArchitectureCloser).unwrap();
    assert_eq!(architecture.purposes, [ArchitectureCloser]);
    let process = roster.seat_for_purpose(ProcessCloser).unwrap();
    assert_eq!(process.purposes, [ProcessCloser, Control]);
    assert_eq!(process, roster.seat_for_purpose(Control).unwrap());
    assert_eq!(
        roster
            .seats
            .iter()
            .filter(|seat| seat.role.role_code.as_str() == "TPM")
            .count(),
        1
    );
}

#[test]
fn legacy_purposes_are_readable_omitted_on_serialization_and_never_inferred() {
    use kontor_teams::CoreTeamSeatPurpose;
    let mut value = serde_json::to_value(roster(SpecVersion::FIRST)).unwrap();
    for seat in value["seats"].as_array_mut().unwrap() {
        seat.as_object_mut().unwrap().remove("purposes");
    }
    let legacy: CoreTeamRevision = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&legacy).unwrap(), value);
    assert!(legacy.canonicalize().is_ok());
    for purpose in CoreTeamSeatPurpose::ALL {
        assert!(matches!(
            legacy.seat_for_purpose(*purpose),
            Err(DomainError::MissingAuthority {
                rule: "role_purpose_unpinned",
                ..
            })
        ));
    }
}

#[test]
fn malformed_duplicate_or_ambiguous_markers_and_weakened_presence_refuse() {
    use kontor_teams::CoreTeamSeatPurpose::*;
    let base = roster(SpecVersion::FIRST);
    for markers in [
        serde_json::json!(["unknown"]),
        serde_json::json!(["control", "control"]),
        serde_json::json!(["control", "process_closer"]),
    ] {
        let mut value = serde_json::to_value(&base).unwrap();
        value["seats"][0]["purposes"] = markers;
        assert!(serde_json::from_value::<CoreTeamRevision>(value).is_err());
    }
    let mut invalid = base.clone();
    invalid.seats[1].purposes.insert(0, ArchitectureCloser);
    assert!(invalid.canonicalize().is_err());
    let mut invalid = base.clone();
    invalid.seats[0].presence = EpicPresence::OnDemand;
    assert!(invalid.canonicalize().is_err());
    let mut invalid = base.clone();
    let mut duplicate = invalid.seats[0].clone();
    duplicate.purposes.clear();
    invalid.seats.push(duplicate);
    assert!(invalid.canonicalize().is_err());
    let mut invalid = base;
    invalid.seats[1].purposes.clear();
    assert!(invalid.canonicalize().is_err());
}

#[test]
fn exact_delivery_selectors_alone_assign_purposes_and_edits_do_not_reinterpret_snapshots() {
    use kontor_teams::{CoreTeamPurposeRoles, CoreTeamSeatPurpose::*};
    let catalog = catalog();
    let selections = [
        selection("LSA", EpicPresence::Required, true),
        selection("TPM", EpicPresence::Required, false),
        selection("SA", EpicPresence::Required, true),
        selection("QA", EpicPresence::Required, true),
    ];
    let roles = purpose_roles();
    let first =
        CoreTeamRevision::resolve(SpecVersion::FIRST, &catalog, &selections, roles).unwrap();
    let alternative = RoleCode::parse("SA").unwrap();
    let next = CoreTeamRevision::resolve(
        SpecVersion::FIRST,
        &catalog,
        &selections,
        CoreTeamPurposeRoles {
            control: &alternative,
            ..roles
        },
    )
    .unwrap();
    let mut stripped_first = serde_json::to_value(&first).unwrap();
    let mut stripped_next = serde_json::to_value(&next).unwrap();
    for value in [&mut stripped_first, &mut stripped_next] {
        for seat in value["seats"].as_array_mut().unwrap() {
            seat.as_object_mut().unwrap().remove("purposes");
        }
    }
    assert_eq!(
        stripped_first, stripped_next,
        "only purpose markers changed"
    );
    assert_ne!(
        first.canonicalize().unwrap().hash(),
        next.canonicalize().unwrap().hash()
    );
    assert_eq!(
        first
            .seat_for_purpose(Control)
            .unwrap()
            .role
            .role_code
            .as_str(),
        "TPM"
    );
    assert_eq!(
        next.seat_for_purpose(Control)
            .unwrap()
            .role
            .role_code
            .as_str(),
        "SA"
    );
    assert!(
        first
            .seats
            .iter()
            .find(|seat| seat.role.role_code.as_str() == "QA")
            .unwrap()
            .purposes
            .is_empty()
    );
    assert!(
        CoreTeamRevision::resolve(
            SpecVersion::FIRST,
            &catalog,
            &selections,
            CoreTeamPurposeRoles {
                architecture_closer: roles.process_closer,
                ..roles
            }
        )
        .is_err()
    );
    let mut retired = catalog.clone();
    retired.roles[0].lifecycle = CodeLifecycle::Retired;
    assert!(CoreTeamRevision::resolve(SpecVersion::FIRST, &retired, &selections, roles).is_err());
    let unknown = RoleCode::parse("UNKNOWN").unwrap();
    assert!(
        CoreTeamRevision::resolve(
            SpecVersion::FIRST,
            &catalog,
            &selections,
            CoreTeamPurposeRoles {
                control: &unknown,
                ..roles
            }
        )
        .is_err()
    );
}
