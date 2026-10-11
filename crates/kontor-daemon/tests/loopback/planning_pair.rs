//! ASMA-8282 D1–D3: governed `planning_pair@1` through the registered routes.
//!
//! Every call presents the credential a real actor holds: the caller's hosted
//! seat credential, a member's scoped consultation credential, or an ambient
//! tier. The fixture publishes everything planning-pair-specific explicitly —
//! a topology kind, a Team Definition container, a document and one activated
//! fleet policy — so nothing here leans on a default the realm invented.
//!
//! The mutants these tests exist to kill:
//!
//! * an ambient Admin or Operator, the caller, or the other member writing a
//!   member's contribution, or a member asking the caller's questions;
//! * a sealed finding or answer reaching the caller, an observer or the peer
//!   before the domain releases it;
//! * a retired generation regaining authority through an idempotent replay;
//! * a second clarification, a disposition that drops dissent, or a planning
//!   pair reaching a settled state or a verdict path;
//! * a native effect on a runtime that does not compose the member surface.

use super::*;
use kontor_core::id::{PlanningPairRunId, SpecVersion, TopologyKindKey};
use kontor_core::planning_pair::PlanningPairSlot;
use kontor_core::repository::{StoredConsultationRun, StoredPlanningPairContribution};
use kontor_core::spec::RoleCatalogRevision;

#[path = "planning_pair_eligibility.rs"]
mod eligibility;
#[path = "planning_pair_intents.rs"]
mod intents;
#[path = "planning_pair_invoke_parity.rs"]
mod invoke_parity;
#[path = "planning_pair_recovery.rs"]
mod recovery;

const PAIR_PROFILE: &str = "01991c00-0000-7000-8000-0000000000b1";

/// The topology kind and Team Definition container the fixture publishes.
const PAIR_KIND: &str = "PPW";

/// A writable kind whose container is otherwise a valid pair container: only
/// its `read_only` differs from [`PAIR_KIND`].
const WRITABLE_KIND: &str = "PWW";

/// A read-only kind whose container titles `seat-b` `SEAT C`: only that title
/// differs from [`PAIR_KIND`].
const MISTITLED_KIND: &str = "PMW";

/// A realm whose promoted epic is pinned to a Team Definition declaring the
/// read-only planning pair container, with a published document, an
/// activated fleet policy and materialized LSA/TPM control seats.
struct PairRealm {
    world: World,
    project: String,
    project_id: ProjectId,
    epic: String,
    caller: SeatBindingId,
    tpm: SeatBindingId,
    profile: serde_json::Value,
}

/// One planning pair document selecting `container_kind`, its members `SA`.
fn pair_document(profile_id: &str, container_kind: &str) -> serde_json::Value {
    pair_document_as(profile_id, container_kind, "SA")
}

/// One planning pair document selecting `container_kind`, both members under
/// the explicit `role_code`.
fn pair_document_as(profile_id: &str, container_kind: &str, role_code: &str) -> serde_json::Value {
    let member = |slot: &str| {
        serde_json::json!({
            "slot": slot,
            "role_code": role_code,
            "specialty": format!("The {slot} planning perspective"),
            "behavior": "Read the frozen plan and give one finding; change nothing.",
            "context": {"skills": [], "files": [], "memory": "none"},
        })
    };
    serde_json::json!({
        "schema_version": 1,
        "protocol": "planning_pair@1",
        "profile_id": profile_id,
        "version": 1,
        "name": "Planning pair",
        "charter": "Is this plan the smallest sound next step?",
        "container_kind": container_kind,
        "members": [member("seat-a"), member("seat-b")],
        "allowed_caller_roles": ["lsa"],
        "allowed_scopes": ["epic"],
        "budget": {
            "max_tokens": 200000,
            "max_commands": 40,
            "max_duration_seconds": 1800,
            "max_cost": {"minor_units": 5000, "currency": "NOK"}
        },
    })
}

/// Preview and publish one planning pair document; the pinned reference.
async fn publish_pair_document(
    world: &World,
    project: &str,
    document: &serde_json::Value,
    key: &str,
) -> serde_json::Value {
    let catalog = Call::get(format!("/v1/projects/{project}/planning-pair-profiles"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    let previewed = Call::post(
        format!("/v1/projects/{project}/planning-pair-profiles:preview"),
        &serde_json::json!({"definition": document}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    assert_eq!(
        previewed.json()["violations"],
        serde_json::json!([]),
        "{}",
        previewed.body
    );
    let applied = Call::post(
        format!("/v1/projects/{project}/planning-pair-profiles:apply"),
        &serde_json::json!({
            "definition": document,
            "preview_hash": previewed.json()["preview_hash"],
            "expected_revision": catalog.json()["revision"],
        }),
    )
    .signed_as(world, "admin")
    .with_key(key)
    .send(world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let published = &applied.json()["published"];
    serde_json::json!({
        "id": published["id"],
        "version": published["version"],
        "definition_hash": published["definition_hash"],
    })
}

/// Publish the bundled topology's successor with one read-only planning pair
/// kind, and select it for future epics.
async fn select_pair_topology(world: &World, project: &str) -> serde_json::Value {
    // The topology the default Team Definition composes against, which the
    // harness publishes; a later bundled successor (ASMA-8450's desks) is not
    // the revision these tests extend.
    let domain =
        kontor_profiles::bundled_operational_domain().expect("the bundled domain validates");
    let default = domain
        .team_definitions
        .first()
        .expect("the bundled Team Definition");
    let bundled = domain
        .topology_specs
        .into_iter()
        .find(|topology| {
            topology.spec_id == default.topology.spec_id
                && topology.version == default.topology.version
        })
        .expect("the bundled topology");
    let spec_id = bundled.spec_id.to_string();
    let base = bundled.version.get();
    let current = Call::get(format!(
        "/v1/projects/{project}/topology-specs/{spec_id}/{base}"
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(current.status, 200, "{}", current.body);
    let mut node_kinds = current.json()["document"]["node_kinds"].clone();
    for (kind, read_only, name) in [
        (PAIR_KIND, true, "Planning Pair Workspace"),
        (WRITABLE_KIND, false, "Writable Pair Workspace"),
        (MISTITLED_KIND, true, "Mistitled Pair Workspace"),
    ] {
        node_kinds
            .as_array_mut()
            .expect("node kinds")
            .push(serde_json::json!({
                "kind": kind,
                "allowed_parents": ["ESW"],
                "cardinality": {"minimum": 0},
                "projection_capabilities": ["native_child", "session_host"],
                "read_only": read_only,
                "name_template": {"segments": [{"kind": "literal", "value": name}]},
                "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
                "code_help": {
                    "full_name": name,
                    "meaning": "A workspace for one planning pair and its two member seats.",
                    "category": "session_topology",
                    "lifecycle": "current"
                }
            }));
    }
    let drafted = Call::post(
        format!("/v1/projects/{project}/topology-specs:draft"),
        &serde_json::json!({
            "base": {"id": spec_id, "version": base},
            "name": "Planning pair vocabulary",
            "root_kind": "PSW",
            "node_kinds": node_kinds,
            "historical_codes": current.json()["document"]["historical_codes"],
        }),
    )
    .signed_as(world, "admin")
    .with_key("pp-topology-draft")
    .send(world)
    .await;
    assert_eq!(drafted.status, 200, "{}", drafted.body);
    let published = Call::post(
        format!("/v1/projects/{project}/topology-specs:publish"),
        &serde_json::json!({
            "candidate": drafted.json()["candidate"],
            "validation_hash": drafted.json()["candidate_hash"],
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pp-topology-publish")
    .send(world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);
    let spec = published.json()["spec"].clone();
    let preview = Call::post(
        format!("/v1/projects/{project}/topology-selection:preview"),
        &serde_json::json!({"target_spec": {"id": spec["id"], "version": spec["version"]}}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let selected = Call::post(
        format!("/v1/projects/{project}/topology-selection:apply"),
        &serde_json::json!({
            "preview_hash": preview.json()["preview_hash"],
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pp-topology-select")
    .send(world)
    .await;
    assert_eq!(selected.status, 200, "{}", selected.body);
    spec
}

/// Publish the bundled Team Definition's successor with one read-only
/// planning pair container whose two display-named slots are SEAT A and SEAT
/// B, validated by `topology`, and select it for future epics.
async fn select_pair_team_definition(world: &World, project: &str, topology: &serde_json::Value) {
    let mut definition = kontor_profiles::bundled_operational_domain()
        .expect("the bundled domain validates")
        .team_definitions
        .into_iter()
        .next()
        .expect("the bundled Team Definition");
    let catalog = Call::get(format!("/v1/projects/{project}/team-definitions"))
        .signed_as(world, "admin")
        .send(world)
        .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    let latest = catalog.json()["definitions"]
        .as_array()
        .expect("the Team Definition catalog")
        .iter()
        .filter(|entry| entry["definition"]["id"] == definition.definition_id.to_string())
        .filter_map(|entry| entry["definition"]["version"].as_u64())
        .max()
        .unwrap_or(1);
    definition.version =
        SpecVersion::parse(u32::try_from(latest + 1).expect("a small version")).expect("a version");
    definition.topology = serde_json::from_value(serde_json::json!({
        "spec_id": topology["id"],
        "version": topology["version"],
        "canonical_hash": topology["canonical_hash"],
    }))
    .expect("the published topology snapshot");
    let committee = definition
        .containers
        .iter()
        .find(|container| container.kind.as_str() == "CSW")
        .expect("the bundled Committee container")
        .clone();
    for (kind, read_only, seat_b_title) in [
        (PAIR_KIND, true, "SEAT B"),
        (WRITABLE_KIND, false, "SEAT B"),
        (MISTITLED_KIND, true, "SEAT C"),
    ] {
        let mut container = committee.clone();
        container.kind = TopologyKindKey::parse(kind).expect("a kind");
        container.prefix = ExternalName::parse(kind).expect("a prefix");
        container.read_only = read_only;
        container.slots = serde_json::from_value(serde_json::json!([
            {"slot_id": "seat-a", "display_name": "SEAT A", "capability_profile": "planning_pair_member"},
            {"slot_id": "seat-b", "display_name": seat_b_title, "capability_profile": "planning_pair_member"},
        ]))
        .expect("the member slots");
        definition.containers.push(container);
    }
    let candidate = serde_json::to_value(&definition).expect("the definition serializes");
    let validated = Call::post(
        format!("/v1/projects/{project}/team-definitions:validate"),
        &serde_json::json!({"candidate": candidate}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(validated.status, 200, "{}", validated.body);
    assert_eq!(
        validated.json()["violations"],
        serde_json::json!([]),
        "{}",
        validated.body
    );
    let published = Call::post(
        format!("/v1/projects/{project}/team-definitions:publish"),
        &serde_json::json!({
            "candidate": candidate,
            "validation_hash": validated.json()["validation_hash"],
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pp-team-definition-publish")
    .send(world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);
    let preview = Call::post(
        format!("/v1/projects/{project}/team-definition-selection:preview"),
        &serde_json::json!({"target_definition": {
            "id": definition.definition_id.to_string(),
            "version": definition.version.get(),
        }}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let selected = Call::post(
        format!("/v1/projects/{project}/team-definition-selection:apply"),
        &serde_json::json!({
            "preview_hash": preview.json()["preview_hash"],
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "admin")
    .with_key("pp-team-definition-select")
    .send(world)
    .await;
    assert_eq!(selected.status, 200, "{}", selected.body);
}

/// The shared fixture fleet: `seat-a` names an existing Committee reviewer key
/// on xAI, `seat-b` the fixture Advisor key on Anthropic, so the shared
/// allocator can place both on distinct actual vendors.
fn pair_fleet_yaml(seat_b_chain: &str) -> String {
    committee_fleet_yaml(&[
        (&committee_fleet_key("reviewer-a"), "cursor-grok"),
        (&advisor_fleet_key(), seat_b_chain),
    ])
}

/// A Quick session opened under `role_code` of catalog revision `version`,
/// ready to promote.
async fn quick_session_selecting(
    world: &World,
    project: &str,
    version: u32,
    role_code: &str,
) -> (String, String) {
    let opened = Call::post(
        format!("/v1/projects/{project}/quick-sessions:ensure"),
        &serde_json::json!({
            "role": {
                "catalog_revision": {"id": SEEDED_CATALOG, "version": version},
                "role_code": role_code,
            },
            "purpose": "Plan with a pair",
        }),
    )
    .signed_as(world, "operator")
    .with_key("pp-quick")
    .send(world)
    .await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let session = opened.json()["quick_session_id"]
        .as_str()
        .expect("a session id")
        .to_owned();
    let previewed = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{session}/promotion:preview"),
        &serde_json::json!({}),
    )
    .signed_as(world, "operator")
    .send(world)
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    let hash = previewed.json()["preview_hash"]
        .as_str()
        .expect("a preview hash")
        .to_owned();
    (session, hash)
}

async fn pair_realm(root: &str) -> PairRealm {
    pair_realm_with(root, None, "SA").await
}

/// As [`pair_realm`], with the epic's roster selecting `selected` — a role
/// catalog revision persisted beside the bundled one — when it is given, and
/// the fixture document's members under `role_code`.
async fn pair_realm_with(
    root: &str,
    selected: Option<&RoleCatalogRevision>,
    role_code: &str,
) -> PairRealm {
    let owned_world = World::open_empty_with_a_plane().await;
    let world = &owned_world;
    world.daemon.reconcile().await;
    let created = ensure_project(world, "planning-pair-project", "Kontor", root).await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    // Selected before any session node exists, so the project root and every
    // node of the epic are created under the vocabulary that declares the
    // planning pair kind.
    let topology = select_pair_topology(world, &project).await;
    select_pair_team_definition(world, &project, &topology).await;
    adopt_session_base(
        world,
        &project,
        current_project_revision(world, &project).await,
    )
    .await;
    let (quick, preview_hash) = match selected {
        None => {
            publish_core_team(
                world,
                &project,
                serde_json::json!([seat("SA", "default", true)]),
            )
            .await;
            quick_session_ready_to_promote(world, &project, "Plan with a pair", "pp-quick").await
        }
        Some(catalog) => {
            world.daemon.state().with_store(|store| {
                store
                    .publish_role_catalog(
                        catalog,
                        &kontor_core::spec::Shareability::default_for(
                            kontor_core::spec::ShareabilityTier::ProjectKnowledge,
                        )
                        .expect("a classified stamp"),
                        kontor_api::now(),
                    )
                    .expect("the selected catalog revision is persisted")
            });
            let version = catalog.version.get();
            publish_core_team(
                world,
                &project,
                serde_json::json!([{
                    "role": {
                        "catalog_revision": {"id": SEEDED_CATALOG, "version": version},
                        "role_code": "LSA",
                    },
                    "presence": "required",
                    "ad_hoc_allowed": true,
                }]),
            )
            .await;
            quick_session_selecting(world, &project, version, "LSA").await
        }
    };
    let promoted = Call::post(
        format!("/v1/projects/{project}/quick-sessions/{quick}/promotion:apply"),
        &promotion_apply_body(&preview_hash),
    )
    .signed_as(world, "operator")
    .with_key("pp-promote")
    .send(world)
    .await;
    assert_eq!(promoted.status, 200, "{}", promoted.body);
    let epic = promoted.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    confirm_promoted_epic_identity(world, &project, &epic, "PROMO", "ASMA-9001");
    let materialized = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/core-team/seats:materialize"),
        &control_seat_routes(1),
    )
    .signed_as(world, "admin")
    .with_key("pp-control-seats")
    .send(world)
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    let caller = SeatBindingId::parse(&core_seat(&materialized, "LSA")).expect("the LSA seat");
    let tpm = SeatBindingId::parse(&core_seat(&materialized, "TPM")).expect("the TPM seat");
    let project_id = ProjectId::parse(&project).expect("a project id");
    let pinned = world.daemon.state().with_store(|store| {
        store
            .get_mini_project_team_definition(
                project_id,
                MiniProjectId::parse(&epic).expect("an epic id"),
            )
            .expect("the pin reads")
            .expect("the promoted epic is pinned")
    });
    assert_ne!(
        pinned.definition.version.get(),
        1,
        "the promoted epic inherits the selected planning pair Team Definition"
    );
    activate_fleet_policy(world, &pair_fleet_yaml("claude-then-codex"));
    let profile = publish_pair_document(
        world,
        &project,
        &pair_document_as(PAIR_PROFILE, PAIR_KIND, role_code),
        "pp-document",
    )
    .await;
    PairRealm {
        world: owned_world,
        project,
        project_id,
        epic,
        caller,
        tpm,
        profile,
    }
}

impl PairRealm {
    /// The current hosted generation of one control seat.
    fn hosted_generation(&self, seat: SeatBindingId) -> u64 {
        self.world
            .daemon
            .state()
            .with_store(|store| {
                store.hosted_topology_seat_occupancy_generation(self.project_id, seat)
            })
            .expect("the hosted occupancy reads")
            .expect("the control seat is hosted")
    }

    /// A control seat's scoped credential at `generation`.
    fn seat_token(&self, seat: SeatBindingId, generation: u64) -> String {
        self.world
            .daemon
            .state()
            .credentials()
            .seat_credential_for_generation(seat, generation)
    }

    /// The caller's credential at its current hosted generation.
    fn caller_token(&self) -> String {
        self.seat_token(self.caller, self.hosted_generation(self.caller))
    }

    /// A member's scoped consultation credential at `generation`.
    fn member_token(&self, seat: SeatBindingId, generation: u64) -> String {
        self.world
            .daemon
            .state()
            .credentials()
            .consultation_seat_credential_for_generation(seat, generation)
    }

    async fn epic_revision(&self) -> serde_json::Value {
        let read = Call::get(format!("/v1/projects/{}/epics/{}", self.project, self.epic))
            .signed_as(&self.world, "observer")
            .send(&self.world)
            .await;
        assert_eq!(read.status, 200, "{}", read.body);
        read.json()["revision"].clone()
    }

    /// The invoke body for `topic` under `profile`, members on the fixture keys.
    async fn invoke_body(&self, profile: &serde_json::Value, topic: &str) -> serde_json::Value {
        serde_json::json!({
            "protocol": "planning_pair@1",
            "profile": profile,
            "topic": topic,
            "question": format!("Is the {topic} plan the smallest sound next step?"),
            "members": [
                {"slot": "seat-a", "binding_key": committee_fleet_key("reviewer-a")},
                {"slot": "seat-b", "binding_key": advisor_fleet_key()},
            ],
            "expected_revision": self.epic_revision().await,
        })
    }

    async fn invoke_with(&self, body: &serde_json::Value, token: String, key: &str) -> Answer {
        Call::post(
            format!(
                "/v1/projects/{}/epics/{}/planning-pair-runs:invoke",
                self.project, self.epic
            ),
            body,
        )
        .with_token(token)
        .with_key(key)
        .send(&self.world)
        .await
    }

    /// Invoke the fixture document as the caller; the admitted run.
    async fn invoke(&self, topic: &str, key: &str) -> Pair {
        let body = self.invoke_body(&self.profile, topic).await;
        let invoked = self.invoke_with(&body, self.caller_token(), key).await;
        assert_eq!(invoked.status, 200, "{}", invoked.body);
        let run = invoked.json();
        let member = |slot: &str| {
            let entry = run["members"]
                .as_array()
                .expect("two members")
                .iter()
                .find(|member| member["slot"] == slot)
                .unwrap_or_else(|| panic!("the {slot} member"))
                .clone();
            SeatBindingId::parse(entry["seat_binding_id"].as_str().expect("a member seat"))
                .expect("a member seat id")
        };
        Pair {
            run: run["planning_pair_run_id"]
                .as_str()
                .expect("a planning pair run id")
                .to_owned(),
            seat_a: member("seat-a"),
            seat_b: member("seat-b"),
            invoked: run,
        }
    }

    fn run_path(&self, pair: &Pair, suffix: &str) -> String {
        format!(
            "/v1/projects/{}/planning-pair-runs/{}{suffix}",
            self.project, pair.run
        )
    }

    async fn read_with(&self, pair: &Pair, token: Option<String>) -> Answer {
        let call = Call::get(self.run_path(pair, ""));
        match token {
            Some(token) => call.with_token(token),
            None => call.signed_as(&self.world, "observer"),
        }
        .send(&self.world)
        .await
    }

    async fn revision(&self, pair: &Pair) -> u64 {
        let read = self.read_with(pair, None).await;
        assert_eq!(read.status, 200, "{}", read.body);
        read.json()["revision"].as_u64().expect("the run revision")
    }

    async fn write(
        &self,
        pair: &Pair,
        suffix: &str,
        body: &serde_json::Value,
        token: String,
        key: &str,
    ) -> Answer {
        Call::post(self.run_path(pair, suffix), body)
            .with_token(token)
            .with_key(key)
            .send(&self.world)
            .await
    }

    async fn finding(&self, pair: &Pair, seat: SeatBindingId, advice: &str, key: &str) -> Answer {
        let body = serde_json::json!({
            "advice": advice,
            "expected_revision": self.revision(pair).await,
        });
        self.write(
            pair,
            "/findings:record",
            &body,
            self.member_token(seat, 1),
            key,
        )
        .await
    }

    /// Every stored contribution row of one run.
    fn contributions(&self, pair: &Pair) -> Vec<StoredPlanningPairContribution> {
        let run = ConsultationRunId::PlanningPair(
            PlanningPairRunId::parse(&pair.run).expect("a planning pair run id"),
        );
        self.world
            .daemon
            .state()
            .with_store(|store| store.planning_pair_contributions(self.project_id, run))
            .expect("the contributions read")
    }

    fn stored_run(&self, pair: &Pair) -> StoredConsultationRun {
        let run = ConsultationRunId::PlanningPair(
            PlanningPairRunId::parse(&pair.run).expect("a planning pair run id"),
        );
        self.world
            .daemon
            .state()
            .with_store(|store| store.get_consultation_run(self.project_id, run))
            .expect("the run reads")
            .expect("the run exists")
    }

    fn planning_pair_runs(&self) -> usize {
        let epic = MiniProjectId::parse(&self.epic).expect("an epic id");
        self.world
            .daemon
            .state()
            .with_store(|store| {
                store.list_consultation_runs(
                    self.project_id,
                    epic,
                    ConsultationFamily::PlanningPair,
                )
            })
            .expect("the runs read")
            .len()
    }
}

/// A list field the projection omits when it is empty.
fn listed(value: &serde_json::Value) -> Vec<serde_json::Value> {
    match value {
        serde_json::Value::Null => Vec::new(),
        serde_json::Value::Array(items) => items.clone(),
        other => panic!("not a list: {other}"),
    }
}

/// The slots of one projected contribution list.
fn slots(value: &serde_json::Value) -> Vec<String> {
    listed(value)
        .iter()
        .map(|entry| entry["slot"].as_str().expect("a slot").to_owned())
        .collect()
}

/// A write the domain transitions refuse: the run's state machine, not the
/// store, said no.
fn assert_transition_refused(answer: &Answer, why: &str) {
    assert_eq!(answer.status, 400, "{why}: {}", answer.body);
    assert_eq!(
        answer.json()["subject"],
        "PlanningPairRun",
        "{why}: {}",
        answer.body
    );
}

/// One admitted planning pair.
struct Pair {
    run: String,
    seat_a: SeatBindingId,
    seat_b: SeatBindingId,
    invoked: serde_json::Value,
}

/// The whole protocol through its own routes: sealed findings, their release
/// to the caller alone, one clarification to one member, and a disposition
/// that keeps the dissent and ends the pair without any verdict or settlement.
#[tokio::test]
async fn a_planning_pair_releases_sealed_findings_to_its_caller_and_ends_in_a_disposition() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-journey").await;
    let world = &realm.world;
    let pair = realm.invoke("Release plan", "pp-journey-invoke").await;
    let invoked = &pair.invoked;
    assert_eq!(invoked["state"], "running", "{invoked}");
    assert_eq!(invoked["phase"], "awaiting_findings", "{invoked}");
    assert_eq!(invoked["viewer"], "caller", "{invoked}");
    assert_eq!(invoked["caller_seat_binding_id"], realm.caller.to_string());
    let members = invoked["members"].as_array().expect("two members");
    assert_eq!(
        members
            .iter()
            .map(|member| member["label"].as_str().expect("a label"))
            .collect::<Vec<_>>(),
        ["SEAT A", "SEAT B"]
    );
    assert_ne!(
        members[0]["vendor"], members[1]["vendor"],
        "the shared allocator places the members on distinct actual vendors: {invoked}"
    );
    assert!(
        members
            .iter()
            .all(|member| member["observed_binding"]["native_id"].is_string()),
        "both members were launched through the runtime port and observed: {invoked}"
    );

    // Findings are sealed: each member sees its own, nobody else sees any.
    let first = realm
        .finding(
            &pair,
            pair.seat_a,
            "Split the migration first.",
            "pp-journey-a",
        )
        .await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["viewer"], "member");
    assert_eq!(slots(&first.json()["own_contributions"]), ["seat-a"]);
    assert!(first.json()["findings"].is_null(), "{}", first.body);
    for (token, viewer) in [
        (Some(realm.caller_token()), "caller"),
        (None, "observer"),
        (Some(realm.member_token(pair.seat_b, 1)), "member"),
    ] {
        let read = realm.read_with(&pair, token).await;
        assert_eq!(read.status, 200, "{}", read.body);
        assert_eq!(read.json()["viewer"], viewer);
        assert!(
            read.json()["findings"].is_null(),
            "sealed from the {viewer}: {}",
            read.body
        );
        assert!(
            slots(&read.json()["own_contributions"]).is_empty(),
            "nothing of seat-a reaches the {viewer}: {}",
            read.body
        );
    }

    let second = realm
        .finding(
            &pair,
            pair.seat_b,
            "Keep the migration whole.",
            "pp-journey-b",
        )
        .await;
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(second.json()["phase"], "findings_released");
    // Released to the caller alone; the peer still sees only its own words.
    let caller_read = realm.read_with(&pair, Some(realm.caller_token())).await;
    let findings = caller_read.json()["findings"].clone();
    assert_eq!(
        findings.as_array().map(Vec::len),
        Some(2),
        "{}",
        caller_read.body
    );
    let observer_read = realm.read_with(&pair, None).await;
    assert!(
        observer_read.json()["findings"].is_null(),
        "{}",
        observer_read.body
    );
    let seat_a_read = realm
        .read_with(&pair, Some(realm.member_token(pair.seat_a, 1)))
        .await;
    assert_eq!(
        slots(&seat_a_read.json()["own_contributions"]),
        ["seat-a"],
        "{}",
        seat_a_read.body
    );
    assert!(seat_a_read.json()["findings"].is_null());

    // One clarification, to one member.
    let ask = |key: &'static str, addressed: serde_json::Value| {
        let realm = &realm;
        let pair = &pair;
        async move {
            let body = serde_json::json!({
                "question": "Which step can be reverted alone?",
                "addressed": addressed,
                "expected_revision": realm.revision(pair).await,
            });
            realm
                .write(
                    pair,
                    "/clarification:request",
                    &body,
                    realm.caller_token(),
                    key,
                )
                .await
        }
    };
    let asked = ask("pp-journey-ask", serde_json::json!(["seat-a"])).await;
    assert_eq!(asked.status, 200, "{}", asked.body);
    assert_eq!(asked.json()["phase"], "awaiting_answers");
    let again = ask("pp-journey-ask-again", serde_json::json!(["seat-b"])).await;
    assert_transition_refused(&again, "one clarification only");

    let answer = |seat: SeatBindingId, key: &'static str| {
        let realm = &realm;
        let pair = &pair;
        async move {
            let body = serde_json::json!({
                "advice": "The schema step reverts alone.",
                "expected_revision": realm.revision(pair).await,
            });
            realm
                .write(
                    pair,
                    "/answers:record",
                    &body,
                    realm.member_token(seat, 1),
                    key,
                )
                .await
        }
    };
    let unaddressed = answer(pair.seat_b, "pp-journey-answer-b").await;
    assert_transition_refused(&unaddressed, "seat-b was not asked");
    let answered = answer(pair.seat_a, "pp-journey-answer-a").await;
    assert_eq!(answered.status, 200, "{}", answered.body);
    assert_eq!(answered.json()["phase"], "answers_released");

    // The disposition cites the exact hashes and keeps seat-b's dissent.
    let caller_read = realm.read_with(&pair, Some(realm.caller_token())).await;
    let released = caller_read.json();
    let finding_hash = |slot: &str| {
        released["findings"]
            .as_array()
            .expect("released findings")
            .iter()
            .find(|entry| entry["slot"] == slot)
            .expect("a released finding")["document_hash"]
            .clone()
    };
    let answer_hash = released["clarification"]["answers"][0]["document_hash"].clone();
    let disposed = realm
        .write(
            &pair,
            "/disposition:record",
            &serde_json::json!({
                "members": [
                    {"slot": "seat-a", "finding": finding_hash("seat-a"), "answer": answer_hash,
                     "disposition": "accepted"},
                    {"slot": "seat-b", "finding": finding_hash("seat-b"),
                     "disposition": "rejected"},
                ],
                "rationale": "Split first; the reversible schema step lands alone.",
                "expected_revision": realm.revision(&pair).await,
            }),
            realm.caller_token(),
            "pp-journey-dispose",
        )
        .await;
    assert_eq!(disposed.status, 200, "{}", disposed.body);
    assert_eq!(disposed.json()["state"], "disposed");
    assert_eq!(disposed.json()["phase"], "disposed");
    assert_eq!(
        slots(&disposed.json()["retained_dissent"]),
        ["seat-b"],
        "the rejected member's finding is kept verbatim: {}",
        disposed.body
    );
    // Disposed is terminal, and never a settlement or a verdict.
    let late = realm
        .finding(&pair, pair.seat_b, "One more thought.", "pp-journey-late")
        .await;
    assert_eq!(
        late.status, 409,
        "a disposed pair takes no contribution: {}",
        late.body
    );
    assert_eq!(
        late.json()["rule"],
        "the aggregate is terminal and immutable"
    );
    let stored = realm.stored_run(&pair);
    assert_eq!(stored.state, ConsultationRunState::Disposed);
    assert!(stored.result.is_none() && stored.settled_at.is_none());
    assert_eq!(realm.contributions(&pair).len(), 3);
    // No settling family reads it: it is neither a Committee nor an Advisor run.
    for family in ["committee-runs", "advisor-runs"] {
        let read = Call::get(format!(
            "/v1/projects/{}/{family}/{}",
            realm.project, pair.run
        ))
        .signed_as(world, "observer")
        .send(world)
        .await;
        assert_eq!(
            read.status, 404,
            "a planning pair is not in {family}: {}",
            read.body
        );
    }
}

/// Fence one member's occupancy generation, as a prepared seat recovery does
/// before its successor launches: the same logical SeatBinding, now held at
/// the next generation, so every credential of the old one is retired.
///
/// Planning pair member recovery is not an operation yet, so the fence is
/// placed directly on the realm database rather than through one.
fn retire_member_generation(realm: &PairRealm, seat: SeatBindingId) {
    let database = realm.world.directory.path().join("kontor.db");
    let connection = rusqlite::Connection::open(database).expect("the realm database opens");
    assert_eq!(
        connection
            .execute(
                "UPDATE consultation_seats SET occupancy_generation = occupancy_generation + 1
                 WHERE seat_binding_id = ?1",
                rusqlite::params![seat.to_string()],
            )
            .expect("the member generation is fenced"),
        1
    );
}

/// Fence the caller's hosted occupancy generation with a successor native.
fn retire_caller_generation(realm: &PairRealm) {
    realm.world.daemon.state().with_store(|store| {
        let predecessor = store
            .get_hosted_topology_seat(realm.project_id, realm.caller)
            .expect("the caller occupancy reads")
            .expect("the caller is hosted");
        let mut successor = predecessor.clone();
        successor.native_identity.native_id =
            ExternalId::parse("pp-caller-successor").expect("a native id");
        successor.provider_session_id =
            Some(ExternalId::parse("pp-caller-successor-session").expect("a session"));
        successor.observed_at = kontor_api::now();
        store
            .replace_hosted_topology_seat_route(
                &predecessor,
                &successor,
                kontor_api::now(),
                "test the planning pair caller generation fence",
                None, // Fixture-only route replacement; no Core Team succession receipt.
            )
            .expect("the caller occupancy is replaced");
    });
}

/// D2: the registry tier is a floor, never the authentication. Only the
/// exact frozen caller at its current generation invokes, asks and decides;
/// only the exact frozen member at its current generation contributes, and
/// only in its own slot. No ambient tier, peer, other pair or retired
/// generation stands in for either, not even through an idempotent replay.
#[tokio::test]
async fn only_the_frozen_caller_and_members_hold_authority_and_a_retired_generation_never_replays()
{
    let realm = pair_realm("/tmp/kontor-asma8282-pair-authority").await;
    let world = &realm.world;
    let body = realm.invoke_body(&realm.profile, "Authority plan").await;
    for tier in ["admin", "operator"] {
        let ambient = Call::post(
            format!(
                "/v1/projects/{}/epics/{}/planning-pair-runs:invoke",
                realm.project, realm.epic
            ),
            &body,
        )
        .signed_as(world, tier)
        .with_key(format!("pp-auth-ambient-{tier}"))
        .send(world)
        .await;
        assert_eq!(
            ambient.status, 403,
            "an ambient {tier} is no caller: {}",
            ambient.body
        );
    }
    let tpm = realm
        .invoke_with(
            &body,
            realm.seat_token(realm.tpm, realm.hosted_generation(realm.tpm)),
            "pp-auth-tpm",
        )
        .await;
    assert_eq!(
        tpm.status, 403,
        "the document admits LSA callers only: {}",
        tpm.body
    );
    assert_eq!(
        realm.planning_pair_runs(),
        0,
        "no refused invocation froze a run"
    );

    let pair = realm.invoke("Authority plan", "pp-auth-invoke").await;
    let other = realm.invoke("Another plan", "pp-auth-other").await;
    let finding = |advice: &str| serde_json::json!({"advice": advice, "expected_revision": pair.invoked["revision"]});
    for tier in ["admin", "operator"] {
        let ambient = Call::post(
            realm.run_path(&pair, "/findings:record"),
            &finding("ambient"),
        )
        .signed_as(world, tier)
        .with_key(format!("pp-auth-ambient-finding-{tier}"))
        .send(world)
        .await;
        assert_eq!(
            ambient.status, 403,
            "an ambient {tier} is no member: {}",
            ambient.body
        );
    }
    for (token, why) in [
        (realm.caller_token(), "the caller is no member"),
        (
            realm.member_token(other.seat_a, 1),
            "another pair's member is no member here",
        ),
    ] {
        let refused = realm
            .write(
                &pair,
                "/findings:record",
                &finding("not mine"),
                token,
                "pp-auth-not-member",
            )
            .await;
        assert_eq!(refused.status, 403, "{why}: {}", refused.body);
    }
    let stale = realm
        .write(
            &pair,
            "/findings:record",
            &finding("a generation never issued"),
            realm.member_token(pair.seat_a, 2),
            "pp-auth-future-generation",
        )
        .await;
    assert_eq!(stale.code(), "stale_binding", "{}", stale.body);
    let ask = serde_json::json!({
        "question": "Which step reverts alone?",
        "addressed": ["seat-a"],
        "expected_revision": pair.invoked["revision"],
    });
    for (suffix, body) in [
        ("/clarification:request", ask.clone()),
        (
            "/disposition:record",
            serde_json::json!({
                "members": [], "rationale": "not the caller",
                "expected_revision": pair.invoked["revision"],
            }),
        ),
    ] {
        let member = realm
            .write(
                &pair,
                suffix,
                &body,
                realm.member_token(pair.seat_a, 1),
                "pp-auth-member-asks",
            )
            .await;
        assert_eq!(
            member.status, 403,
            "a member is not the caller: {}",
            member.body
        );
        let ambient = Call::post(realm.run_path(&pair, suffix), &body)
            .signed_as(world, "admin")
            .with_key("pp-auth-admin-asks")
            .send(world)
            .await;
        assert_eq!(
            ambient.status, 403,
            "an ambient Admin is not the caller: {}",
            ambient.body
        );
    }
    assert!(
        realm.contributions(&pair).is_empty(),
        "no refused write left a row"
    );

    // The slot is the authenticated member's own, whatever it writes.
    let recorded = realm
        .finding(&pair, pair.seat_b, "Keep it whole.", "pp-auth-seat-b")
        .await;
    assert_eq!(recorded.status, 200, "{}", recorded.body);
    let rows = realm.contributions(&pair);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].slot, PlanningPairSlot::SeatB);
    assert_eq!(rows[0].seat_binding_id, pair.seat_b);
    assert_eq!(rows[0].occupancy_generation, 1);
    let seat_a_read = realm
        .read_with(&pair, Some(realm.member_token(pair.seat_a, 1)))
        .await;
    assert!(slots(&seat_a_read.json()["own_contributions"]).is_empty());
    let foreign_read = realm
        .read_with(&pair, Some(realm.member_token(other.seat_a, 1)))
        .await;
    assert_eq!(foreign_read.status, 403, "{}", foreign_read.body);

    // A retired member generation regains nothing through its own key.
    let replay_body = serde_json::json!({
        "advice": "Keep it whole.",
        "expected_revision": pair.invoked["revision"],
    });
    let replayed = realm
        .write(
            &pair,
            "/findings:record",
            &replay_body,
            realm.member_token(pair.seat_b, 1),
            "pp-auth-seat-b",
        )
        .await;
    assert_eq!(
        replayed.status, 200,
        "the current generation replays: {}",
        replayed.body
    );
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    retire_member_generation(&realm, pair.seat_b);
    let fenced = realm
        .write(
            &pair,
            "/findings:record",
            &replay_body,
            realm.member_token(pair.seat_b, 1),
            "pp-auth-seat-b",
        )
        .await;
    assert_eq!(fenced.code(), "stale_binding", "{}", fenced.body);
    let fenced_read = realm
        .read_with(&pair, Some(realm.member_token(pair.seat_b, 1)))
        .await;
    assert_eq!(fenced_read.code(), "stale_binding", "{}", fenced_read.body);

    // And so does a retired caller generation, for its invocation replay too.
    let caller_before = realm.caller_token();
    retire_caller_generation(&realm);
    let fenced_ask = realm
        .write(
            &pair,
            "/clarification:request",
            &ask,
            caller_before.clone(),
            "pp-auth-fenced-ask",
        )
        .await;
    assert_eq!(fenced_ask.code(), "stale_binding", "{}", fenced_ask.body);
    let fenced_invoke = realm
        .invoke_with(
            &realm.invoke_body(&realm.profile, "Authority plan").await,
            caller_before,
            "pp-auth-invoke",
        )
        .await;
    assert_eq!(
        fenced_invoke.code(),
        "stale_binding",
        "{}",
        fenced_invoke.body
    );
    let current = realm.read_with(&pair, Some(realm.caller_token())).await;
    assert_eq!(current.status, 200, "{}", current.body);
    assert_eq!(current.json()["viewer"], "caller");
}

/// D2: one key is one command. The same key and payload replay the original
/// receipt; the same key with another payload, a stale revision and a second
/// pair on the same scope and topic are each refused, and a race between the
/// two members on one revision admits exactly one.
#[tokio::test]
async fn planning_pair_commands_replay_exactly_and_refuse_reuse_staleness_and_duplicates() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-idempotency").await;
    let body = realm.invoke_body(&realm.profile, "Replay plan").await;
    let created = realm
        .invoke_with(&body, realm.caller_token(), "pp-idem-invoke")
        .await;
    assert_eq!(created.status, 200, "{}", created.body);
    assert_eq!(created.json()["receipt"]["applied"], "created");
    let replayed = realm
        .invoke_with(&body, realm.caller_token(), "pp-idem-invoke")
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        replayed.json()["planning_pair_run_id"],
        created.json()["planning_pair_run_id"]
    );
    let mut reused = body.clone();
    reused["topic"] = serde_json::json!("Another topic");
    let reused = realm
        .invoke_with(&reused, realm.caller_token(), "pp-idem-invoke")
        .await;
    assert_eq!(reused.code(), "idempotency_conflict", "{}", reused.body);
    let duplicate = realm
        .invoke_with(
            &realm.invoke_body(&realm.profile, "Replay plan").await,
            realm.caller_token(),
            "pp-idem-duplicate",
        )
        .await;
    assert_eq!(
        duplicate.code(),
        "idempotency_conflict",
        "{}",
        duplicate.body
    );
    assert!(
        duplicate.json()["rule"]
            .as_str()
            .is_some_and(|rule| rule.starts_with("consultation_semantic_duplicate")),
        "the shared semantic index refuses a second pair on one scope and topic: {}",
        duplicate.body
    );
    assert_eq!(realm.planning_pair_runs(), 1);

    let pair = Pair {
        run: created.json()["planning_pair_run_id"]
            .as_str()
            .expect("a run")
            .to_owned(),
        seat_a: SeatBindingId::parse(
            created.json()["members"][0]["seat_binding_id"]
                .as_str()
                .expect("seat-a"),
        )
        .expect("a seat"),
        seat_b: SeatBindingId::parse(
            created.json()["members"][1]["seat_binding_id"]
                .as_str()
                .expect("seat-b"),
        )
        .expect("a seat"),
        invoked: created.json(),
    };
    let at_invocation = pair.invoked["revision"].clone();
    let write = |advice: &str, seat: SeatBindingId, key: &'static str| {
        let realm = &realm;
        let pair = &pair;
        let body = serde_json::json!({"advice": advice, "expected_revision": at_invocation});
        async move {
            realm
                .write(
                    pair,
                    "/findings:record",
                    &body,
                    realm.member_token(seat, 1),
                    key,
                )
                .await
        }
    };
    // Both members race on the one revision they read: exactly one lands.
    let (a, b) = tokio::join!(
        write("Split it.", pair.seat_a, "pp-idem-a"),
        write("Keep it whole.", pair.seat_b, "pp-idem-b"),
    );
    let mut statuses = [a.status, b.status];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 409], "{} / {}", a.body, b.body);
    let (won, lost, lost_key, lost_advice, lost_seat) = if a.status == 200 {
        (a, b, "pp-idem-b", "Keep it whole.", pair.seat_b)
    } else {
        (b, a, "pp-idem-a", "Split it.", pair.seat_a)
    };
    assert_eq!(lost.code(), "revision_conflict", "{}", lost.body);
    assert_eq!(lost.json()["current_revision"], won.json()["revision"]);
    assert_eq!(realm.contributions(&pair).len(), 1);
    // The winner's key replays, and refuses another payload.
    let winner_seat = if lost_seat == pair.seat_a {
        pair.seat_b
    } else {
        pair.seat_a
    };
    let winner_key = if lost_key == "pp-idem-a" {
        "pp-idem-b"
    } else {
        "pp-idem-a"
    };
    let winner_advice = if lost_advice == "Split it." {
        "Keep it whole."
    } else {
        "Split it."
    };
    let replay = write(winner_advice, winner_seat, winner_key).await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    let rewrite = write("Something else.", winner_seat, winner_key).await;
    assert_eq!(rewrite.code(), "idempotency_conflict", "{}", rewrite.body);
    // A second finding from the same member is the domain's refusal.
    let second = realm
        .finding(&pair, winner_seat, "And another.", "pp-idem-second")
        .await;
    assert_transition_refused(&second, "one finding per member");
    // The loser, re-reading, lands at the current revision.
    let retried = realm
        .finding(&pair, lost_seat, lost_advice, "pp-idem-retry")
        .await;
    assert_eq!(retried.status, 200, "{}", retried.body);
    assert_eq!(realm.contributions(&pair).len(), 2);
}

/// D1/D3: placement is explicit and whole. A document that selects a
/// container the pinned Team Definition does not declare as the read-only
/// pair container, or members the activated policy cannot put on distinct
/// actual vendors, freezes nothing and launches nothing.
#[tokio::test]
async fn a_planning_pair_freezes_nothing_without_its_explicit_container_or_distinct_vendors() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-placement").await;
    let world = &realm.world;
    world.fake.take_calls();
    for (profile_id, kind, key) in [
        (
            "01991c00-0000-7000-8000-0000000000b2",
            "CSW",
            "pp-place-committee-kind",
        ),
        (
            "01991c00-0000-7000-8000-0000000000b3",
            "XPW",
            "pp-place-absent-kind",
        ),
    ] {
        let profile =
            publish_pair_document(world, &realm.project, &pair_document(profile_id, kind), key)
                .await;
        let body = realm.invoke_body(&profile, "Placement plan").await;
        let refused = realm
            .invoke_with(&body, realm.caller_token(), &format!("{key}-invoke"))
            .await;
        assert_eq!(
            refused.code(),
            "placement_blocked",
            "{kind}: {}",
            refused.body
        );
    }
    activate_fleet_policy(world, &pair_fleet_yaml("cursor-grok"));
    let body = realm.invoke_body(&realm.profile, "Placement plan").await;
    let same_vendor = realm
        .invoke_with(&body, realm.caller_token(), "pp-place-same-vendor")
        .await;
    assert_eq!(
        same_vendor.code(),
        "placement_blocked",
        "{}",
        same_vendor.body
    );
    assert!(
        same_vendor.json()["rule"]
            .as_str()
            .is_some_and(|rule| rule.contains("distinct actual vendors")),
        "{}",
        same_vendor.body
    );
    assert_eq!(realm.planning_pair_runs(), 0);
    assert!(
        world.fake.take_calls().iter().all(|call| !matches!(
            call,
            AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
        )),
        "a blocked placement has no native effect"
    );
}

/// D3: a runtime that does not compose the member surface is the defined
/// capability gap. The invocation says so before anything is frozen,
/// prepared or launched, and nothing reports a launch that did not happen.
#[tokio::test]
async fn a_runtime_without_the_member_surface_is_a_capability_gap_with_no_effect() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-unsupported").await;
    let world = &realm.world;
    world.fake.withholding_planning_pair_members();
    world.fake.take_calls();
    let body = realm.invoke_body(&realm.profile, "Unsupported plan").await;
    let refused = realm
        .invoke_with(&body, realm.caller_token(), "pp-unsupported")
        .await;
    assert_eq!(refused.code(), "unsupported_capability", "{}", refused.body);
    assert_eq!(realm.planning_pair_runs(), 0, "nothing was frozen");
    assert!(
        world.fake.take_calls().iter().all(|call| !matches!(
            call,
            AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
        )),
        "no container was prepared and no member launched"
    );
}

/// D1/D2 under concurrency: one key raced against itself admits one pair and
/// answers both requests with it; two keys raced on one scope and topic
/// admit exactly one pair through the shared semantic identity.
#[tokio::test]
async fn concurrent_invocations_of_one_key_or_one_topic_admit_exactly_one_pair() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-concurrency").await;
    let body = realm.invoke_body(&realm.profile, "Raced plan").await;
    let (first, second) = tokio::join!(
        realm.invoke_with(&body, realm.caller_token(), "pp-race-key"),
        realm.invoke_with(&body, realm.caller_token(), "pp-race-key"),
    );
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(
        first.json()["planning_pair_run_id"],
        second.json()["planning_pair_run_id"],
        "one key is one pair"
    );
    let mut applied = [
        first.json()["receipt"]["applied"].clone(),
        second.json()["receipt"]["applied"].clone(),
    ];
    applied.sort_by_key(ToString::to_string);
    assert_eq!(
        applied,
        [serde_json::json!("created"), serde_json::json!("unchanged")]
    );

    let other = realm.invoke_body(&realm.profile, "Contested plan").await;
    let (left, right) = tokio::join!(
        realm.invoke_with(&other, realm.caller_token(), "pp-race-left"),
        realm.invoke_with(&other, realm.caller_token(), "pp-race-right"),
    );
    let mut statuses = [left.status, right.status];
    statuses.sort_unstable();
    assert_eq!(statuses, [200, 409], "{} / {}", left.body, right.body);
    let refused = if left.status == 409 { &left } else { &right };
    assert_eq!(refused.code(), "idempotency_conflict", "{}", refused.body);
    assert_eq!(realm.planning_pair_runs(), 2);
}

/// D1 recovery: nothing of a planning pair lives in process memory. A daemon
/// reopened on the same state root restores the sealed pair from its durable
/// records through the domain transitions, keeps the seal, and continues it.
#[tokio::test]
async fn a_reopened_realm_restores_a_sealed_planning_pair_and_continues_it() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-restart").await;
    let pair = realm.invoke("Restart plan", "pp-restart-invoke").await;
    let sealed = realm
        .finding(&pair, pair.seat_a, "Split it.", "pp-restart-a")
        .await;
    assert_eq!(sealed.status, 200, "{}", sealed.body);
    let caller_generation = realm.hosted_generation(realm.caller);
    let revision = realm.revision(&pair).await;
    let PairRealm {
        world,
        project,
        caller,
        ..
    } = realm;
    let World {
        directory, daemon, ..
    } = world;
    let state_root = directory.path().to_owned();
    drop(daemon);
    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("the same state root reopens");
    restarted.state().signals().stop();
    let router = restarted.router();
    let state = restarted.state();
    let credentials = state.credentials();
    let path = format!("/v1/projects/{project}/planning-pair-runs/{}", pair.run);
    let read = |token: String| {
        let router = &router;
        let path = path.clone();
        async move { Call::get(path).with_token(token).send_to(router).await }
    };
    let as_caller =
        read(credentials.seat_credential_for_generation(caller, caller_generation)).await;
    assert_eq!(as_caller.status, 200, "{}", as_caller.body);
    assert_eq!(as_caller.json()["phase"], "awaiting_findings");
    assert!(
        as_caller.json()["findings"].is_null(),
        "still sealed: {}",
        as_caller.body
    );
    let as_seat_a =
        read(credentials.consultation_seat_credential_for_generation(pair.seat_a, 1)).await;
    assert_eq!(slots(&as_seat_a.json()["own_contributions"]), ["seat-a"]);
    let as_seat_b =
        read(credentials.consultation_seat_credential_for_generation(pair.seat_b, 1)).await;
    assert!(slots(&as_seat_b.json()["own_contributions"]).is_empty());

    let released = Call::post(
        format!("{path}/findings:record"),
        &serde_json::json!({"advice": "Keep it whole.", "expected_revision": revision}),
    )
    .with_token(credentials.consultation_seat_credential_for_generation(pair.seat_b, 1))
    .with_key("pp-restart-b")
    .send_to(&router)
    .await;
    assert_eq!(released.status, 200, "{}", released.body);
    assert_eq!(released.json()["phase"], "findings_released");
    let as_caller =
        read(credentials.seat_credential_for_generation(caller, caller_generation)).await;
    assert_eq!(slots(&as_caller.json()["findings"]), ["seat-a", "seat-b"]);
    drop(directory);
}

/// Audit 6f residual: two requests for one key interleave inside
/// materialization. Both are held at the member launch, so the second resumes
/// the run the first froze while the first is still launching. The run's
/// compare-and-swap admits one `running` advance, both requests answer with
/// the one pair on its one frozen node, and no member is launched twice.
///
/// On this current-thread runtime the first released request then runs to
/// its receipt before the second resumes, so this test does not line the two
/// up at the receipt write. That interleaving is
/// `two_requests_held_at_the_receipt_write_classify_one_created_and_one_unchanged`.
#[tokio::test]
async fn a_resumed_invocation_interleaved_with_its_original_launches_nothing_twice() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-resume").await;
    let world = &realm.world;
    let body = realm.invoke_body(&realm.profile, "Resumed plan").await;
    let token = realm.caller_token();
    let gate = world.fake.holding_consultation_launches();
    let open = async {
        tokio::time::timeout(Duration::from_secs(30), async {
            while gate.waiting() < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both requests reach the member launch before either proceeds");
        gate.release();
    };
    let (first, second, ()) = tokio::join!(
        realm.invoke_with(&body, token.clone(), "pp-resume"),
        realm.invoke_with(&body, token.clone(), "pp-resume"),
        open,
    );
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(
        second.status, 200,
        "the resumed request answers too: {}",
        second.body
    );
    assert_eq!(
        first.json()["planning_pair_run_id"],
        second.json()["planning_pair_run_id"]
    );
    let mut applied = [
        first.json()["receipt"]["applied"].clone(),
        second.json()["receipt"]["applied"].clone(),
    ];
    applied.sort_by_key(ToString::to_string);
    assert_eq!(
        applied,
        [serde_json::json!("created"), serde_json::json!("unchanged")]
    );
    let natives = |answer: &Answer| {
        answer.json()["members"]
            .as_array()
            .expect("two members")
            .iter()
            .map(|member| {
                member["observed_binding"]["native_id"]
                    .as_str()
                    .expect("an observed member")
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(natives(&first), natives(&second), "one native per member");
    assert_eq!(
        natives(&first).iter().collect::<BTreeSet<_>>().len(),
        2,
        "two members, two natives"
    );
    assert_eq!(
        first.json()["topology_node_id"],
        second.json()["topology_node_id"],
        "one frozen node, so one pair container"
    );
    assert_eq!(
        first.json()["container_name"],
        second.json()["container_name"]
    );
    assert_eq!(realm.planning_pair_runs(), 1);
    let pair = Pair {
        run: first.json()["planning_pair_run_id"]
            .as_str()
            .expect("a run")
            .to_owned(),
        seat_a: SeatBindingId::generate(),
        seat_b: SeatBindingId::generate(),
        invoked: first.json(),
    };
    assert_eq!(realm.stored_run(&pair).state, ConsultationRunState::Running);
}

/// The bundled role catalog at its next revision, edited by `edit`: what a
/// realm that selected a later catalog revision would hold beside it.
fn successor_catalog(edit: impl FnOnce(&mut RoleCatalogRevision)) -> RoleCatalogRevision {
    let mut catalog = kontor_profiles::bundled_operational_domain()
        .expect("the bundled domain validates")
        .role_catalogs
        .remove(0);
    catalog.version = SpecVersion::parse(catalog.version.get() + 1).expect("the next revision");
    edit(&mut catalog);
    catalog.validate().expect("the successor catalog is valid");
    catalog
}

/// The stored SeatBinding role of every member of `pair`.
fn member_roles(realm: &PairRealm, pair: &Pair) -> Vec<kontor_core::spec::CatalogRoleRef> {
    [pair.seat_a, pair.seat_b]
        .into_iter()
        .map(|seat| {
            realm
                .world
                .daemon
                .state()
                .with_store(|store| store.get_seat_binding(realm.project_id, seat))
                .expect("the member binding reads")
                .expect("the member binding exists")
                .role
        })
        .collect()
}

/// Audit 6f: a member's role comes only from the catalog the epic selected.
/// A role code that only the selected revision declares is frozen from that
/// revision, never refused for being absent from the build's first catalog.
#[tokio::test]
async fn a_member_role_only_the_epics_selected_catalog_declares_is_frozen_from_it() {
    let selected = successor_catalog(|catalog| {
        let mut member = catalog
            .roles
            .iter()
            .find(|role| role.role_code.as_str() == "SA")
            .expect("the bundled SA entry")
            .clone();
        member.role_code = kontor_core::id::RoleCode::parse("PPM").expect("a role code");
        member.standard_title = ExternalName::parse("Planning Pair Member").expect("a title");
        catalog.roles.push(member);
    });
    let realm = pair_realm_with(
        "/tmp/kontor-asma8282-pair-selected-catalog",
        Some(&selected),
        "PPM",
    )
    .await;
    let pair = realm
        .invoke("Selected catalog plan", "pp-catalog-selected")
        .await;
    for role in member_roles(&realm, &pair) {
        assert_eq!(role.catalog_id, selected.catalog_id);
        assert_eq!(role.catalog_revision, selected.version, "{role:?}");
        assert_eq!(role.role_code.as_str(), "PPM");
        assert_eq!(role.standard_title.as_str(), "Planning Pair Member");
        role.validate_against(&selected)
            .expect("the frozen role is the selected catalog's exact projection");
    }
}

/// Audit 6f: a published but unselected catalog revision is no authority. A
/// member role the epic's selected revision does not declare, or declares
/// only for compatibility, freezes nothing even though the build's first
/// catalog declares it as current.
#[tokio::test]
async fn a_member_role_outside_the_epics_selected_catalog_freezes_nothing() {
    let selected = successor_catalog(|catalog| {
        catalog.roles.retain(|role| role.role_code.as_str() != "SA");
        catalog
            .roles
            .iter_mut()
            .find(|role| role.role_code.as_str() == "AUD")
            .expect("the bundled AUD entry")
            .lifecycle = kontor_core::spec::CodeLifecycle::Compatibility;
    });
    let realm = pair_realm_with(
        "/tmp/kontor-asma8282-pair-unselected-catalog",
        Some(&selected),
        "SA",
    )
    .await;
    let world = &realm.world;
    world.fake.take_calls();
    let body = realm
        .invoke_body(&realm.profile, "Unselected catalog plan")
        .await;
    let absent = realm
        .invoke_with(&body, realm.caller_token(), "pp-catalog-absent")
        .await;
    assert_eq!(absent.code(), "placement_blocked", "{}", absent.body);
    assert_eq!(
        absent.json()["rule"],
        "the member's role code is absent from the epic's selected role catalog"
    );
    let compatibility = publish_pair_document(
        world,
        &realm.project,
        &pair_document_as("01991c00-0000-7000-8000-0000000000b4", PAIR_KIND, "AUD"),
        "pp-catalog-compatibility-document",
    )
    .await;
    let body = realm
        .invoke_body(&compatibility, "Unselected catalog plan")
        .await;
    let retired = realm
        .invoke_with(&body, realm.caller_token(), "pp-catalog-compatibility")
        .await;
    assert_eq!(retired.code(), "placement_blocked", "{}", retired.body);
    assert_eq!(
        retired.json()["rule"],
        "the member's role cannot open new seats in the epic's selected role catalog"
    );
    assert_eq!(realm.planning_pair_runs(), 0);
    assert!(
        world.fake.take_calls().iter().all(|call| !matches!(
            call,
            AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
        )),
        "a refused role has no native effect"
    );
}

/// Audit 6f: the selected catalog is held to its exact persisted bytes, and a
/// frozen member role to the member's explicit code, both before any native
/// effect. A roster pin the persisted catalog does not hash to freezes
/// nothing; a stored member role that no longer corresponds to the document
/// is refused when the frozen run resumes, before its members launch.
#[tokio::test]
async fn a_mismatched_catalog_pin_or_member_role_fails_closed_before_any_native_effect() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-catalog-pin").await;
    let world = &realm.world;
    let database = world.directory.path().join("kontor.db");
    let connection = rusqlite::Connection::open(&database).expect("the realm database opens");
    let epic = realm.epic.clone();
    let pinned: String = connection
        .query_row(
            "SELECT catalog_hash FROM epic_rosters WHERE mini_project_id = ?1",
            [&epic],
            |row| row.get(0),
        )
        .expect("the roster pin reads");
    let set_pin = |hash: &str| {
        assert_eq!(
            connection
                .execute(
                    "UPDATE epic_rosters SET catalog_hash = ?1 WHERE mini_project_id = ?2",
                    rusqlite::params![hash, epic],
                )
                .expect("the roster pin is written"),
            1
        );
    };
    set_pin(ContentHash::of(b"another catalog").as_str());
    world.fake.take_calls();
    let body = realm
        .invoke_body(&realm.profile, "Pinned catalog plan")
        .await;
    let mismatched = realm
        .invoke_with(&body, realm.caller_token(), "pp-catalog-mismatch")
        .await;
    assert_eq!(
        mismatched.code(),
        "placement_blocked",
        "{}",
        mismatched.body
    );
    assert_eq!(
        mismatched.json()["rule"],
        "the persisted role catalog does not hash to the epic's frozen catalog pin"
    );
    assert_eq!(realm.planning_pair_runs(), 0);
    set_pin(&pinned);

    // Freeze a pair whose first launch fails, so it stays materializing.
    let seat_a = kontor_core::id::RoleSlotId::parse("seat-a").expect("a slot");
    world.fake.refusing_launch_of(&seat_a);
    let interrupted = realm
        .invoke_with(&body, realm.caller_token(), "pp-catalog-resume")
        .await;
    assert_ne!(interrupted.status, 200, "{}", interrupted.body);
    assert_eq!(
        realm.planning_pair_runs(),
        1,
        "the pair froze before its launch failed"
    );
    world.fake.allowing_launch_of(&seat_a);
    // The stored seat-a role is rewritten to another exact catalog role.
    let auditor_title = kontor_profiles::bundled_operational_domain()
        .expect("the bundled domain validates")
        .role_catalogs
        .remove(0)
        .role(&kontor_core::id::RoleCode::parse("AUD").expect("a code"))
        .expect("the bundled AUD entry")
        .standard_title
        .as_str()
        .to_owned();
    assert_eq!(
        connection
            .execute(
                "UPDATE seat_bindings SET role_code = 'AUD', standard_title = ?1
                 WHERE role_slot_id = 'seat-a'
                   AND topology_node_id IN (
                       SELECT topology_node_id FROM consultation_runs WHERE family = 'planning_pair')",
                [&auditor_title],
            )
            .expect("the frozen member role is rewritten"),
        1
    );
    world.fake.take_calls();
    let resumed = realm
        .invoke_with(&body, realm.caller_token(), "pp-catalog-resume")
        .await;
    assert_eq!(resumed.code(), "placement_blocked", "{}", resumed.body);
    assert_eq!(
        resumed.json()["rule"],
        "a frozen member role does not correspond to the document's member role code"
    );
    assert!(
        world.fake.take_calls().iter().all(|call| !matches!(
            call,
            AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
        )),
        "the refusal came before any native effect"
    );
}

/// The read-only predicate on the selected container: `PWW` is a valid pair
/// container in every respect but `read_only`, so only that predicate
/// refuses it.
#[tokio::test]
async fn a_writable_selected_container_freezes_nothing() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-writable").await;
    let world = &realm.world;
    let profile = publish_pair_document(
        world,
        &realm.project,
        &pair_document("01991c00-0000-7000-8000-0000000000b5", WRITABLE_KIND),
        "pp-writable-document",
    )
    .await;
    world.fake.take_calls();
    let body = realm.invoke_body(&profile, "Writable plan").await;
    let refused = realm
        .invoke_with(&body, realm.caller_token(), "pp-writable")
        .await;
    assert_eq!(refused.code(), "placement_blocked", "{}", refused.body);
    assert_eq!(
        refused.json()["rule"],
        "a planning pair container must be read-only"
    );
    assert_eq!(realm.planning_pair_runs(), 0);
    assert!(world.fake.take_calls().iter().all(|call| !matches!(
        call,
        AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
    )));
}

/// The slot-title predicate on the selected container: `PMW` is a valid
/// read-only pair container in every respect but `seat-b`'s title, so only
/// that predicate refuses it.
#[tokio::test]
async fn a_mistitled_member_seat_freezes_nothing() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-mistitled").await;
    let world = &realm.world;
    let profile = publish_pair_document(
        world,
        &realm.project,
        &pair_document("01991c00-0000-7000-8000-0000000000b6", MISTITLED_KIND),
        "pp-mistitled-document",
    )
    .await;
    world.fake.take_calls();
    let body = realm.invoke_body(&profile, "Mistitled plan").await;
    let refused = realm
        .invoke_with(&body, realm.caller_token(), "pp-mistitled")
        .await;
    assert_eq!(refused.code(), "placement_blocked", "{}", refused.body);
    assert_eq!(
        refused.json()["rule"],
        "a planning pair slot must be titled SEAT A or SEAT B"
    );
    assert_eq!(realm.planning_pair_runs(), 0);
    assert!(world.fake.take_calls().iter().all(|call| !matches!(
        call,
        AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
    )));
}

/// Audit 6f (turn 5): two requests for one key, both past the run's
/// compare-and-swap and both held immediately before the receipt write, so
/// neither has seen a receipt when the other writes. The write itself decides
/// which request recorded the command: exactly one answers `created`, the
/// other `unchanged`, both with the one receipt, the one pair on its one node
/// and container, and the same two natives.
#[tokio::test]
async fn two_requests_held_at_the_receipt_write_classify_one_created_and_one_unchanged() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-receipt").await;
    let world = &realm.world;
    let body = realm.invoke_body(&realm.profile, "Receipt plan").await;
    let token = realm.caller_token();
    let hold = world.daemon.hold_planning_pair_invocation_receipts();
    let open = async {
        tokio::time::timeout(Duration::from_secs(30), async {
            while hold.waiting() < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both requests reach the receipt write before either writes");
        hold.release();
    };
    let (first, second, ()) = tokio::join!(
        realm.invoke_with(&body, token.clone(), "pp-receipt"),
        realm.invoke_with(&body, token.clone(), "pp-receipt"),
        open,
    );
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(second.status, 200, "{}", second.body);
    let mut applied = [
        first.json()["receipt"]["applied"].clone(),
        second.json()["receipt"]["applied"].clone(),
    ];
    applied.sort_by_key(ToString::to_string);
    assert_eq!(
        applied,
        [serde_json::json!("created"), serde_json::json!("unchanged")],
        "exactly one request wrote the receipt: {} / {}",
        first.body,
        second.body
    );
    assert_eq!(
        first.json()["receipt"]["receipt_id"],
        second.json()["receipt"]["receipt_id"],
        "one receipt"
    );
    for field in ["planning_pair_run_id", "topology_node_id", "container_name"] {
        assert_eq!(first.json()[field], second.json()[field], "{field}");
    }
    let natives = |answer: &Answer| {
        answer.json()["members"]
            .as_array()
            .expect("two members")
            .iter()
            .map(|member| {
                member["observed_binding"]["native_id"]
                    .as_str()
                    .expect("an observed member")
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(natives(&first), natives(&second));
    assert_eq!(natives(&first).iter().collect::<BTreeSet<_>>().len(), 2);
    assert_eq!(realm.planning_pair_runs(), 1);
    let receipts: i64 = rusqlite::Connection::open(world.directory.path().join("kontor.db"))
        .expect("the realm database opens")
        .query_row(
            "SELECT count(*) FROM command_receipts WHERE idempotency_key = 'pp-receipt'",
            [],
            |row| row.get(0),
        )
        .expect("the receipts count");
    assert_eq!(receipts, 1, "one key, one stored receipt");
}

/// Audit 6f (turn 5): the roster's catalog selection is one exact persisted
/// revision. A roster whose seats name two revisions, or one revision this
/// realm never persisted, freezes nothing; the same roster restored invokes.
#[tokio::test]
async fn a_mixed_or_unpersisted_roster_catalog_freezes_nothing() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-roster-catalog").await;
    let world = &realm.world;
    let connection = rusqlite::Connection::open(world.directory.path().join("kontor.db"))
        .expect("the realm database opens");
    let epic = realm.epic.clone();
    let original: String = connection
        .query_row(
            "SELECT seats FROM epic_rosters WHERE mini_project_id = ?1",
            [&epic],
            |row| row.get(0),
        )
        .expect("the roster seats read");
    let write_seats = |seats: &serde_json::Value| {
        assert_eq!(
            connection
                .execute(
                    "UPDATE epic_rosters SET seats = ?1 WHERE mini_project_id = ?2",
                    rusqlite::params![seats.to_string(), epic],
                )
                .expect("the roster seats are written"),
            1
        );
    };
    let unpersisted = 7;
    let seats: serde_json::Value = serde_json::from_str(&original).expect("roster seats JSON");
    assert!(
        seats.as_array().is_some_and(|seats| seats.len() >= 2),
        "the roster has seats to disagree: {seats}"
    );
    let mut mixed = seats.clone();
    let last = mixed.as_array().expect("seats").len() - 1;
    mixed[last]["role"]["catalog_revision"] = serde_json::json!(unpersisted);
    let mut moved = seats.clone();
    for seat in moved.as_array_mut().expect("seats") {
        seat["role"]["catalog_revision"] = serde_json::json!(unpersisted);
    }
    world.fake.take_calls();
    let body = realm
        .invoke_body(&realm.profile, "Roster catalog plan")
        .await;
    for (roster, key, rule) in [
        (
            &mixed,
            "pp-roster-mixed",
            "the epic's frozen roster names more than one role catalog revision",
        ),
        (
            &moved,
            "pp-roster-unpersisted",
            "the epic's selected role catalog revision is not persisted in this realm",
        ),
    ] {
        write_seats(roster);
        let refused = realm.invoke_with(&body, realm.caller_token(), key).await;
        assert_eq!(
            refused.code(),
            "placement_blocked",
            "{key}: {}",
            refused.body
        );
        assert_eq!(refused.json()["rule"], rule, "{key}");
    }
    assert_eq!(realm.planning_pair_runs(), 0);
    assert!(world.fake.take_calls().iter().all(|call| !matches!(
        call,
        AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
    )));
    write_seats(&seats);
    let restored = realm
        .invoke_with(&body, realm.caller_token(), "pp-roster-restored")
        .await;
    assert_eq!(
        restored.status, 200,
        "the restored roster invokes: {}",
        restored.body
    );
}

/// D-3: the two actual placed routes are asked about before anything is
/// frozen. A route whose provider the runtime cannot compose the closed
/// member surface for is refused with that provider named: nothing is
/// frozen, prepared or launched, and no other route is substituted.
#[tokio::test]
async fn a_member_route_the_runtime_cannot_compose_freezes_nothing_and_names_its_provider() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-route-surface").await;
    let world = &realm.world;
    world.fake.withholding_planning_pair_members_on("cursor");
    world.fake.take_calls();
    let body = realm.invoke_body(&realm.profile, "Route plan").await;
    let refused = realm
        .invoke_with(&body, realm.caller_token(), "pp-route-surface")
        .await;
    assert_eq!(refused.code(), "unsupported_capability", "{}", refused.body);
    assert_eq!(refused.json()["subject"], "planning pair member route");
    assert_eq!(
        refused.json()["at"],
        "providers/cursor/not_composed",
        "the refusal names the provider and its gap: {}",
        refused.body
    );
    assert_eq!(realm.planning_pair_runs(), 0, "nothing was frozen");
    assert!(world.fake.take_calls().iter().all(|call| !matches!(
        call,
        AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
    )));
}

/// D-3: every member launch carries the frozen context the service derived
/// from durable state: the seat's actual occupancy generation, the pinned
/// document, topology, Team Definition and role catalog, the frozen route and
/// actual vendor, and the placement. A resumed member whose generation was
/// fenced launches at the generation it now holds, never a default.
#[tokio::test]
async fn member_launches_carry_the_seats_current_generation_and_frozen_pins() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-context").await;
    let world = &realm.world;
    let seat_a = kontor_core::id::RoleSlotId::parse("seat-a").expect("a slot");
    world.fake.refusing_launch_of(&seat_a);
    let body = realm.invoke_body(&realm.profile, "Context plan").await;
    let interrupted = realm
        .invoke_with(&body, realm.caller_token(), "pp-context")
        .await;
    assert_ne!(interrupted.status, 200, "{}", interrupted.body);
    let run = world.daemon.state().with_store(|store| {
        store
            .list_consultation_runs(
                realm.project_id,
                MiniProjectId::parse(&realm.epic).expect("an epic id"),
                ConsultationFamily::PlanningPair,
            )
            .expect("the runs read")
            .pop()
            .expect("the frozen pair")
    });
    let seats = world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    let seat_a_binding = seats
        .iter()
        .find(|seat| seat.role_slot_id.as_str() == "seat-a")
        .expect("seat A")
        .seat_binding_id;
    // Fence seat A's occupancy to generation 2 before it ever launches.
    let connection = rusqlite::Connection::open(world.directory.path().join("kontor.db"))
        .expect("the realm database opens");
    assert_eq!(
        connection
            .execute(
                "UPDATE consultation_seats SET occupancy_generation = 2 WHERE seat_binding_id = ?1",
                [seat_a_binding.to_string()],
            )
            .expect("the generation is fenced"),
        1
    );
    world.fake.allowing_launch_of(&seat_a);
    let resumed = realm
        .invoke_with(&body, realm.caller_token(), "pp-context")
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);

    let contexts = world.fake.planning_pair_launch_contexts();
    assert_eq!(contexts.len(), 2, "both members launched with a context");
    let kontor_core::consultation::ConsultationRunId::PlanningPair(run_id) = run.id else {
        panic!("a planning pair run")
    };
    let placement = world.daemon.state().with_store(|store| {
        store
            .planning_pair_placement(realm.project_id, run.id)
            .expect("the placement reads")
            .expect("the placement exists")
    });
    let roster_pin: String = connection
        .query_row(
            "SELECT catalog_hash FROM epic_rosters WHERE mini_project_id = ?1",
            [&realm.epic],
            |row| row.get(0),
        )
        .expect("the roster pin reads");
    let team_definition = world.daemon.state().with_store(|store| {
        store
            .get_mini_project_team_definition(
                realm.project_id,
                MiniProjectId::parse(&realm.epic).expect("an epic id"),
            )
            .expect("the pin reads")
            .expect("the epic is pinned")
            .definition
    });
    for seat in &seats {
        let context = &contexts[&seat.seat_binding_id];
        let expected_generation = if seat.seat_binding_id == seat_a_binding {
            2
        } else {
            1
        };
        assert_eq!(
            context.occupancy_generation, expected_generation,
            "the seat's actual generation, not a default: {context:?}"
        );
        assert_eq!(context.run_id, run_id);
        assert_eq!(context.slot.as_str(), seat.role_slot_id.as_str());
        assert_eq!(context.route, seat.model_rung);
        assert_eq!(context.profile.definition_hash, run.definition_hash);
        assert_eq!(context.placement_hash, *placement.placement.hash());
        assert_eq!(context.role_catalog.canonical_hash.as_str(), roster_pin);
        assert_eq!(context.team_definition, team_definition);
        assert_eq!(context.topology_node_id, run.topology_node_id);
        assert_eq!(context.vendor, context.requested_fleet_provenance.vendor);
    }
}

/// D-3: a member qualifies — is bound, and so may contribute — only on a
/// readback that observed exactly its requested provenance. One whose labels
/// carry none is kept unbound and named, the pair stays materializing, its
/// credential cannot contribute, and a replay meets the same native session
/// rather than creating or replacing one.
#[tokio::test]
async fn a_member_without_its_provenance_readback_is_kept_unqualified_and_never_relaunched() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-readback").await;
    let world = &realm.world;
    world.fake.dropping_planning_pair_provenance_labels();
    let body = realm.invoke_body(&realm.profile, "Readback plan").await;
    let first = realm
        .invoke_with(&body, realm.caller_token(), "pp-readback")
        .await;
    assert_eq!(first.code(), "unavailable", "{}", first.body);
    assert_eq!(first.json()["subject"], "planning pair member readback");
    let native = first.json()["at"].clone();
    assert!(
        native.as_str().is_some_and(|at| at.starts_with("native/")),
        "the kept native session is named: {}",
        first.body
    );
    let run = world.daemon.state().with_store(|store| {
        store
            .list_consultation_runs(
                realm.project_id,
                MiniProjectId::parse(&realm.epic).expect("an epic id"),
                ConsultationFamily::PlanningPair,
            )
            .expect("the runs read")
            .pop()
            .expect("the frozen pair")
    });
    assert_eq!(run.state, ConsultationRunState::Materializing);
    let seats = world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    assert!(
        seats.iter().all(|seat| seat.native_identity.is_none()),
        "no member is bound"
    );
    let kontor_core::consultation::ConsultationRunId::PlanningPair(run_id) = run.id else {
        panic!("a planning pair run")
    };
    let pair = Pair {
        run: run_id.to_string(),
        seat_a: seats[0].seat_binding_id,
        seat_b: seats[1].seat_binding_id,
        invoked: serde_json::json!({}),
    };
    let contribution = realm
        .finding(
            &pair,
            pair.seat_a,
            "Not yet qualified.",
            "pp-readback-finding",
        )
        .await;
    assert_eq!(
        contribution.code(),
        "stale_binding",
        "{}",
        contribution.body
    );
    let replayed = realm
        .invoke_with(&body, realm.caller_token(), "pp-readback")
        .await;
    assert_eq!(replayed.code(), "unavailable", "{}", replayed.body);
    assert_eq!(
        replayed.json()["at"],
        native,
        "the replay met the same native session"
    );
}

/// One invocation whose member launch the fake reports as `withheld`
/// describes, against a runtime that claimed the whole member surface before
/// freeze: the member is kept unqualified under `rule`.
///
/// It proves, together: a typed no-observation refusal naming the kept native
/// session and that confirmation is unknown; no member bound and no member
/// credential able to contribute; the pair still materializing with no
/// receipt for its key; and a replay meeting the same native session, with no
/// second create, replacement or destruction.
async fn assert_member_kept_unqualified(root: &str, withhold: impl Fn(&World), rule: &str) {
    let realm = pair_realm(root).await;
    let world = &realm.world;
    withhold(world);
    world.fake.take_calls();
    let body = realm.invoke_body(&realm.profile, "Unobserved plan").await;
    let first = realm
        .invoke_with(&body, realm.caller_token(), "pp-unobserved")
        .await;
    assert_eq!(first.code(), "unavailable", "{}", first.body);
    assert_eq!(first.json()["subject"], "planning pair member readback");
    assert_eq!(first.json()["rule"], rule, "{}", first.body);
    assert!(
        first.body.contains("confirmation unknown"),
        "the refusal says confirmation is unknown: {}",
        first.body
    );
    let native = first.json()["at"].clone();
    assert!(
        native.as_str().is_some_and(|at| at.starts_with("native/")),
        "the kept native session is named: {}",
        first.body
    );
    let run = world.daemon.state().with_store(|store| {
        store
            .list_consultation_runs(
                realm.project_id,
                MiniProjectId::parse(&realm.epic).expect("an epic id"),
                ConsultationFamily::PlanningPair,
            )
            .expect("the runs read")
            .pop()
            .expect("the frozen pair")
    });
    assert_eq!(run.state, ConsultationRunState::Materializing);
    let seats = world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    assert!(
        seats.iter().all(|seat| seat.native_identity.is_none()),
        "no member is bound"
    );
    let receipts: i64 = rusqlite::Connection::open(world.directory.path().join("kontor.db"))
        .expect("the realm database opens")
        .query_row(
            "SELECT count(*) FROM command_receipts WHERE idempotency_key = 'pp-unobserved'",
            [],
            |row| row.get(0),
        )
        .expect("the receipts count");
    assert_eq!(receipts, 0, "no qualified running receipt");
    let kontor_core::consultation::ConsultationRunId::PlanningPair(run_id) = run.id else {
        panic!("a planning pair run")
    };
    let pair = Pair {
        run: run_id.to_string(),
        seat_a: seats[0].seat_binding_id,
        seat_b: seats[1].seat_binding_id,
        invoked: serde_json::json!({}),
    };
    for seat in [pair.seat_a, pair.seat_b] {
        let contribution = realm
            .finding(
                &pair,
                seat,
                "Not qualified.",
                &format!("pp-unobserved-{seat}"),
            )
            .await;
        assert_eq!(
            contribution.code(),
            "stale_binding",
            "{}",
            contribution.body
        );
    }
    let replayed = realm
        .invoke_with(&body, realm.caller_token(), "pp-unobserved")
        .await;
    assert_eq!(replayed.json()["rule"], rule, "{}", replayed.body);
    assert_eq!(
        replayed.json()["at"],
        native,
        "the replay met the same native session"
    );
    assert!(
        world.fake.take_calls().iter().all(|call| !matches!(
            call,
            AdapterCall::RetireConsultation(_) | AdapterCall::ArchiveContainer(_)
        )),
        "nothing was replaced or destroyed"
    );
}

/// No waiver: a member whose correlation was not observed is not qualified.
#[tokio::test]
async fn a_member_with_an_unobserved_correlation_is_kept_unqualified() {
    assert_member_kept_unqualified(
        "/tmp/kontor-asma8282-pair-unobserved-correlation",
        |world| {
            world.fake.observing_planning_pair_member_field_unsupported(
                kontor_runtime::planning_pair::MandatoryMemberField::Correlation,
            );
        },
        "the planning pair member's readback did not observe its correlation",
    )
    .await;
}

/// No waiver: a member whose route was not observed is not qualified.
#[tokio::test]
async fn a_member_with_an_unobserved_route_is_kept_unqualified() {
    assert_member_kept_unqualified(
        "/tmp/kontor-asma8282-pair-unobserved-route",
        |world| {
            world.fake.observing_planning_pair_member_field_unsupported(
                kontor_runtime::planning_pair::MandatoryMemberField::Route,
            );
        },
        "the planning pair member's readback did not observe its route",
    )
    .await;
}

/// No waiver: a member whose closed tool restriction was not observed is not
/// qualified, even with its correlation, route and provenance all observed.
#[tokio::test]
async fn a_member_with_an_unobserved_tool_restriction_is_kept_unqualified() {
    assert_member_kept_unqualified(
        "/tmp/kontor-asma8282-pair-unobserved-restriction",
        |world| {
            world.fake.observing_planning_pair_member_field_unsupported(
                kontor_runtime::planning_pair::MandatoryMemberField::ToolRestrictions,
            );
        },
        "the planning pair member's readback did not observe its closed tool restriction",
    )
    .await;
}

/// No waiver: a member launch that reports no member-surface observation at
/// all is not qualified.
#[tokio::test]
async fn a_member_without_a_member_surface_observation_is_kept_unqualified() {
    assert_member_kept_unqualified(
        "/tmp/kontor-asma8282-pair-unobserved-surface",
        |world| world.fake.omitting_planning_pair_member_observation(),
        "the planning pair member's launch reported no member-surface observation",
    )
    .await;
}

/// Audit 6f's untested limit, now a fixture. Qualification is per member, not
/// one atomic step: when only seat B's readback fails, seat A, launched first
/// and fully observed, stays bound and qualified. Seat B stays unbound and
/// named, the pair stays materializing with no receipt for its key, and a
/// replay meets seat B's durable known-native claim with no launch at all.
///
/// Seat A's credential is therefore qualified while the pair is still
/// materializing: its finding is recorded, sealed from the caller, and seat
/// B's is refused. This pins the behaviour as it is. No contract unbinds both
/// members, and none gates a bound member on the pair's state.
#[tokio::test]
async fn a_second_seat_readback_failure_keeps_the_first_member_bound_and_the_pair_materializing() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-second-seat").await;
    let world = &realm.world;
    world
        .fake
        .observing_planning_pair_member_field_unsupported_in(
            PlanningPairSlot::SeatB,
            kontor_runtime::planning_pair::MandatoryMemberField::Route,
        );
    world.fake.take_calls();
    let body = realm.invoke_body(&realm.profile, "Second seat plan").await;
    let first = realm
        .invoke_with(&body, realm.caller_token(), "pp-second-seat")
        .await;
    assert_eq!(first.code(), "unavailable", "{}", first.body);
    assert_eq!(first.json()["subject"], "planning pair member readback");
    assert_eq!(
        first.json()["rule"],
        "the planning pair member's readback did not observe its route",
        "{}",
        first.body
    );
    let run = world.daemon.state().with_store(|store| {
        store
            .list_consultation_runs(
                realm.project_id,
                MiniProjectId::parse(&realm.epic).expect("an epic id"),
                ConsultationFamily::PlanningPair,
            )
            .expect("the runs read")
            .pop()
            .expect("the frozen pair")
    });
    assert_eq!(run.state, ConsultationRunState::Materializing);
    let seats = world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    let seat = |slot: &str| {
        seats
            .iter()
            .find(|seat| seat.role_slot_id.as_str() == slot)
            .unwrap_or_else(|| panic!("the {slot} seat"))
            .clone()
    };
    let (seat_a, seat_b) = (seat("seat-a"), seat("seat-b"));
    let bound_a = seat_a
        .native_identity
        .clone()
        .expect("seat A stays bound to its observed native");
    assert!(seat_b.native_identity.is_none(), "seat B is not bound");
    let native_b = first.json()["at"].clone();
    assert!(
        native_b
            .as_str()
            .is_some_and(|at| at.starts_with("native/")),
        "seat B's kept native session is named: {}",
        first.body
    );
    assert_ne!(
        native_b,
        serde_json::json!(format!("native/{}", bound_a.native_id.as_str())),
        "the refusal names seat B's native, not seat A's"
    );
    let launched = |calls: Vec<AdapterCall>| -> Vec<SeatBindingId> {
        calls
            .into_iter()
            .filter_map(|call| match call {
                AdapterCall::LaunchConsultation(seat) => Some(seat),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        launched(world.fake.take_calls()),
        vec![seat_a.seat_binding_id, seat_b.seat_binding_id],
        "seat A launched and was bound before seat B's readback failed"
    );
    let receipts: i64 = rusqlite::Connection::open(world.directory.path().join("kontor.db"))
        .expect("the realm database opens")
        .query_row(
            "SELECT count(*) FROM command_receipts WHERE idempotency_key = 'pp-second-seat'",
            [],
            |row| row.get(0),
        )
        .expect("the receipts count");
    assert_eq!(receipts, 0, "no qualified pair receipt");

    let replayed = realm
        .invoke_with(&body, realm.caller_token(), "pp-second-seat")
        .await;
    assert_eq!(
        replayed.json()["rule"],
        first.json()["rule"],
        "{}",
        replayed.body
    );
    assert_eq!(
        replayed.json()["at"],
        native_b,
        "the replay met seat B's same native session"
    );
    let calls = world.fake.take_calls();
    assert!(
        calls.iter().all(|call| !matches!(
            call,
            AdapterCall::RetireConsultation(_) | AdapterCall::ArchiveContainer(_)
        )),
        "nothing was replaced or destroyed"
    );
    assert_eq!(
        launched(calls),
        Vec::<SeatBindingId>::new(),
        "the replay skipped bound seat A and met seat B's kept claim with no launch"
    );
    let after = world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    assert_eq!(
        after
            .iter()
            .find(|seat| seat.seat_binding_id == seat_a.seat_binding_id)
            .and_then(|seat| seat.native_identity.as_ref()),
        Some(&bound_a),
        "seat A is still bound to the same native"
    );
    assert!(
        after
            .iter()
            .find(|seat| seat.seat_binding_id == seat_b.seat_binding_id)
            .is_some_and(|seat| seat.native_identity.is_none()),
        "seat B is still unbound"
    );
    let kontor_core::consultation::ConsultationRunId::PlanningPair(run_id) = run.id else {
        panic!("a planning pair run")
    };
    let pair = Pair {
        run: run_id.to_string(),
        seat_a: seat_a.seat_binding_id,
        seat_b: seat_b.seat_binding_id,
        invoked: serde_json::json!({}),
    };
    let refused_b = realm
        .finding(&pair, pair.seat_b, "Not qualified.", "pp-second-seat-b")
        .await;
    assert_eq!(refused_b.code(), "stale_binding", "{}", refused_b.body);
    let recorded_a = realm
        .finding(&pair, pair.seat_a, "Qualified alone.", "pp-second-seat-a")
        .await;
    assert_eq!(recorded_a.status, 200, "{}", recorded_a.body);
    assert_eq!(recorded_a.json()["state"], "materializing");
    let caller = realm.read_with(&pair, Some(realm.caller_token())).await;
    assert_eq!(caller.status, 200, "{}", caller.body);
    assert!(
        caller.json().get("findings").is_none(),
        "seat A's finding is sealed from the caller: {}",
        caller.body
    );
    assert_eq!(
        realm.stored_run(&pair).state,
        ConsultationRunState::Materializing,
        "the pair is still materializing"
    );
}

/// The route refusal's diagnostic limit, pinned. Seat B's route alone is
/// refused under its own provider; once seat A's route is refused too, the
/// refusal names only the first blocker, seat A's provider and gap, and never
/// seat B's. Each is still a refusal before anything is frozen.
#[tokio::test]
async fn a_pair_whose_two_routes_are_both_refused_names_only_the_first_blocker() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-both-routes").await;
    let world = &realm.world;
    world.fake.take_calls();
    let body = realm.invoke_body(&realm.profile, "Both routes plan").await;
    world
        .fake
        .withholding_planning_pair_members_on("claude-personal");
    let seat_b_only = realm
        .invoke_with(&body, realm.caller_token(), "pp-seat-b-route")
        .await;
    assert_eq!(
        seat_b_only.code(),
        "unsupported_capability",
        "{}",
        seat_b_only.body
    );
    assert_eq!(
        seat_b_only.json()["at"],
        "providers/claude-personal/not_composed",
        "seat B's route is refused on its own: {}",
        seat_b_only.body
    );
    world.fake.withholding_planning_pair_members_on("cursor");
    let refused = realm
        .invoke_with(&body, realm.caller_token(), "pp-both-routes")
        .await;
    assert_eq!(refused.code(), "unsupported_capability", "{}", refused.body);
    assert_eq!(refused.json()["subject"], "planning pair member route");
    assert_eq!(
        refused.json()["at"],
        "providers/cursor/not_composed",
        "only seat A's provider and gap are named: {}",
        refused.body
    );
    assert!(
        !refused.body.contains("claude-personal"),
        "seat B's refused provider is not named: {}",
        refused.body
    );
    assert_eq!(realm.planning_pair_runs(), 0, "nothing was frozen");
    assert!(world.fake.take_calls().iter().all(|call| !matches!(
        call,
        AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
    )));
}

/// Frontier A's runtime seam on the hypothetical fake, which observes every
/// field (source contract, not live proof). Each member's exact known native
/// is reconciled in place against the frozen context the daemon derived for
/// it: the same session, never a create, read back now rather than cached.
/// Another session, another frozen field, an absent session or a withheld
/// route is refused, and nothing is created, retired or archived. The
/// caller's member recovery is the one daemon operation that calls it.
#[tokio::test]
async fn the_hypothetical_fake_reconciles_only_the_exact_known_member_native_in_place() {
    use kontor_runtime::planning_pair::{
        MandatoryMemberField, MemberSurfaceField, PlanningPairMemberReconcileRequest,
    };
    let realm = pair_realm("/tmp/kontor-asma8282-pair-reconcile-seam").await;
    let world = &realm.world;
    let pair = realm.invoke("Reconcile plan", "pp-reconcile-seam").await;
    let run = realm.stored_run(&pair);
    let seats = world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    let contexts = world.fake.planning_pair_launch_contexts();
    let known = |seat: SeatBindingId| {
        seats
            .iter()
            .find(|stored| stored.seat_binding_id == seat)
            .and_then(|stored| stored.native_identity.clone())
            .expect("a bound member native")
    };
    let request = |seat: SeatBindingId| PlanningPairMemberReconcileRequest {
        context: contexts
            .get(&seat)
            .cloned()
            .expect("the frozen member context"),
        identity: known(seat),
        requested_at: kontor_api::now(),
    };
    world.fake.take_calls();

    for seat in [pair.seat_a, pair.seat_b] {
        let exact = request(seat);
        let outcome = world
            .fake
            .reconcile_planning_pair_member(&exact)
            .await
            .expect("the exact known native reconciles");
        assert_eq!(exact.require_same_native(&outcome), Ok(()));
        assert!(!outcome.created, "never a create");
        assert_eq!(outcome.identity, known(seat), "the same native session");
        assert!(matches!(
            &outcome.fleet_provenance,
            kontor_runtime::FleetProvenanceObservation::Observed { provenance, .. }
                if *provenance == exact.context.requested_fleet_provenance
        ));
        let observed = outcome.planning_pair.expect("a member-surface readback");
        assert_eq!(observed.unmatched_mandatory(), None, "every field matched");
        assert!(
            !format!("{exact:?}").contains(&realm.member_token(seat, 1)),
            "no member credential travels"
        );
    }

    let wrong = PlanningPairMemberReconcileRequest {
        identity: known(pair.seat_b),
        ..request(pair.seat_a)
    };
    let drifted: Vec<(&str, PlanningPairMemberReconcileRequest)> = vec![
        ("another member's native", wrong),
        ("another generation", {
            let mut drift = request(pair.seat_a);
            drift.context.occupancy_generation += 1;
            drift
        }),
        ("another route", {
            let mut drift = request(pair.seat_a);
            drift.context.route.model.0.push_str("-next");
            drift
        }),
        ("another vendor", {
            let mut drift = request(pair.seat_a);
            drift.context.vendor = "openai".to_owned();
            drift.context.requested_fleet_provenance.vendor = "openai".to_owned();
            drift
        }),
        ("another placement", {
            let mut drift = request(pair.seat_a);
            drift.context.placement_hash = kontor_core::id::ContentHash::of(b"another placement");
            drift
        }),
        ("another document", {
            let mut drift = request(pair.seat_a);
            drift.context.profile.definition_hash =
                kontor_core::id::ContentHash::of(b"another document");
            drift
        }),
        ("another slot", {
            let mut drift = request(pair.seat_a);
            drift.context.slot = PlanningPairSlot::SeatB;
            drift
        }),
    ];
    for (why, drift) in drifted {
        assert_eq!(
            world.fake.reconcile_planning_pair_member(&drift).await,
            Err(kontor_runtime::RuntimeError::CorrelationFailed),
            "{why}"
        );
    }

    // The readback is taken now: a field the runtime can no longer observe
    // is reported unsupported, never the launch's cached match.
    world
        .fake
        .observing_planning_pair_member_field_unsupported_in(
            PlanningPairSlot::SeatB,
            MandatoryMemberField::ToolRestrictions,
        );
    let unobserved = world
        .fake
        .reconcile_planning_pair_member(&request(pair.seat_b))
        .await
        .expect("the same native is read back");
    assert_eq!(
        unobserved
            .planning_pair
            .expect("a member-surface readback")
            .tool_restrictions,
        MemberSurfaceField::Unsupported
    );

    world.fake.losing_consultation_native(pair.seat_a);
    assert!(matches!(
        world
            .fake
            .reconcile_planning_pair_member(&request(pair.seat_a))
            .await,
        Err(kontor_runtime::RuntimeError::StaleBinding { .. })
    ));
    world
        .fake
        .withholding_planning_pair_members_on("claude-personal");
    assert!(matches!(
        world
            .fake
            .reconcile_planning_pair_member(&request(pair.seat_b))
            .await,
        Err(kontor_runtime::RuntimeError::PlanningPairMemberSurfaceUnsupported { .. })
    ));

    let calls = world.fake.take_calls();
    assert!(
        calls
            .iter()
            .all(|call| matches!(call, AdapterCall::ReconcilePlanningPairMember(_))),
        "nothing but reconciles: no launch, retire or archive: {calls:?}"
    );
    assert_eq!(
        calls.len(),
        2 + 7 + 1 + 1,
        "the withheld route is refused before the runtime is asked"
    );
}

/// The existing Committee seat recovery, which replaces a native, never
/// reaches a planning pair member: its run id names no Committee, so even an
/// Admin's exact request finds nothing and no native is retired or launched.
#[tokio::test]
async fn the_committee_seat_recovery_route_never_reaches_a_planning_pair_member() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-committee-recover").await;
    let world = &realm.world;
    let pair = realm.invoke("Recover plan", "pp-committee-recover").await;
    let run = realm.stored_run(&pair);
    let seats = world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    let native = seats
        .iter()
        .find(|seat| seat.seat_binding_id == pair.seat_a)
        .and_then(|seat| seat.native_identity.clone())
        .expect("seat A is bound");
    world.fake.take_calls();
    let refused = Call::post(
        format!(
            "/v1/projects/{}/committee-runs/{}/seats/{}/recover",
            realm.project, pair.run, pair.seat_a
        ),
        &serde_json::json!({
            "expected_revision": realm.revision(&pair).await,
            "expected_native_id": native.native_id.as_str(),
            "reason": "credential_propagation",
        }),
    )
    .signed_as(world, "admin")
    .with_key("pp-committee-recover-seat")
    .send(world)
    .await;
    assert_eq!(refused.code(), "not_found", "{}", refused.body);
    assert_eq!(
        refused.json()["rule"],
        "no such consultation run exists in this project",
        "{}",
        refused.body
    );
    assert!(
        world.fake.take_calls().iter().all(|call| !matches!(
            call,
            AdapterCall::RetireConsultation(_) | AdapterCall::LaunchConsultation(_)
        )),
        "no native was retired or launched"
    );
    let after = world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    assert_eq!(after, seats, "both member seats are unchanged");
}

/// A `kontor-mcp` transport over this realm's own router, presenting one
/// bearer: a seat's scoped credential or an ambient tier secret.
///
/// No socket and no process: the production dispatcher — registry, serve
/// profile, gate, schema validation, one request — runs unchanged above this
/// seam, and the daemon's real authentication runs below it.
struct SeatTransport {
    router: axum::Router,
    tier: kontor_mcp::CallerTier,
    bearer: String,
    requests: std::sync::atomic::AtomicUsize,
}

impl std::fmt::Debug for SeatTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SeatTransport")
            .field("tier", &self.tier)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl kontor_mcp::Transport for SeatTransport {
    fn tier(&self) -> kontor_mcp::CallerTier {
        self.tier
    }

    fn base_url(&self) -> String {
        "http://127.0.0.1:7717".to_owned()
    }

    async fn call(
        &self,
        request: &kontor_mcp::Request,
    ) -> Result<kontor_mcp::Reply, kontor_mcp::TransportFailure> {
        use tower::ServiceExt as _;
        self.requests
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut builder = axum::http::Request::builder()
            .method(match request.method {
                kontor_mcp::Method::Get => axum::http::Method::GET,
                kontor_mcp::Method::Post => axum::http::Method::POST,
            })
            .uri(&request.path)
            .header("host", "127.0.0.1:7717")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", self.bearer));
        if let Some(key) = &request.idempotency_key {
            builder = builder.header("idempotency-key", key);
        }
        let body = request
            .body
            .as_ref()
            .map_or_else(axum::body::Body::empty, |document| {
                axum::body::Body::from(serde_json::to_vec(document).expect("a JSON body"))
            });
        let response = self
            .router
            .clone()
            .oneshot(builder.body(body).expect("a well-formed request"))
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("the whole body");
        Ok(kontor_mcp::Reply {
            status,
            body: serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        })
    }

    async fn frames(
        &self,
        request: &kontor_mcp::Request,
        _budget: kontor_mcp::FrameBudget,
    ) -> Result<kontor_mcp::Reply, kontor_mcp::TransportFailure> {
        Err(kontor_mcp::TransportFailure::Protocol {
            path: request.path.clone(),
            status: None,
            detail: "no streamed read is part of the planning pair surface",
        })
    }
}

/// One `kontor-mcp` dispatcher presenting `bearer` at `tier` under `profile`.
fn seat_dispatcher(
    world: &World,
    tier: kontor_mcp::CallerTier,
    bearer: String,
    profile: &str,
) -> (kontor_mcp::Dispatcher, std::sync::Arc<SeatTransport>) {
    let transport = std::sync::Arc::new(SeatTransport {
        router: world.router.clone(),
        tier,
        bearer,
        requests: std::sync::atomic::AtomicUsize::new(0),
    });
    let dispatcher = kontor_mcp::Dispatcher::new(Box::new(std::sync::Arc::clone(&transport)))
        .with_profile(kontor_mcp::ServeProfile::find(profile).expect("a declared profile"));
    (dispatcher, transport)
}

/// Read one planning pair through a dispatcher.
async fn mcp_read(
    dispatcher: &kontor_mcp::Dispatcher,
    project: &str,
    run: &str,
) -> kontor_mcp::Envelope {
    dispatcher
        .call(
            "kontor_planning_pair_run_get",
            &serde_json::json!({"project_id": project, "planning_pair_run_id": run}),
        )
        .await
        .expect("the read is dispatched")
}

/// Record one finding through a dispatcher, at the revision it reads first.
async fn mcp_finding(
    dispatcher: &kontor_mcp::Dispatcher,
    project: &str,
    run: &str,
    advice: &str,
    key: &str,
) -> kontor_mcp::Envelope {
    let current = mcp_read(dispatcher, project, run).await;
    dispatcher
        .call(
            "kontor_planning_pair_findings_record",
            &serde_json::json!({
                "project_id": project,
                "planning_pair_run_id": run,
                "advice": advice,
                "expected_revision": current.body["revision"],
                "idempotency_key": key,
            }),
        )
        .await
        .expect("the finding is dispatched")
}

fn excluded_by_profile(result: &Result<kontor_mcp::Envelope, kontor_mcp::Failure>) -> bool {
    matches!(
        result,
        Err(kontor_mcp::Failure::Denied(
            kontor_mcp::Denied::ProfileExcluded { .. }
        ))
    )
}

/// ASMA-8282 frontier B: the opt-in `planning_pair_caller` serve profile,
/// driven end to end through the production `kontor-mcp` dispatcher.
///
/// It serves exactly the four caller tools and grants nothing: the caller is
/// still authenticated by the daemon as the exact frozen caller seat at its
/// current hosted generation, under the document's allowed roles. A member,
/// a TPM seat and an ambient Admin are refused under it; the caller cannot
/// contribute under the member profile; a member tool is excluded by the
/// caller profile before any request; and a retired caller generation never
/// replays its invocation through it. The protocol holds through it: sealed
/// reads, one clarification, a disposition that keeps dissent, no verdict.
#[tokio::test]
async fn the_opt_in_caller_profile_drives_a_pair_through_its_four_tools_and_grants_nothing() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-caller-profile").await;
    let world = &realm.world;
    let operator = kontor_mcp::CallerTier::Operator;
    let (caller, caller_requests) = seat_dispatcher(
        world,
        operator,
        realm.caller_token(),
        "planning_pair_caller",
    );
    assert_eq!(
        caller
            .tools()
            .map(|tool| tool.name)
            .collect::<BTreeSet<_>>(),
        kontor_core::planning_pair::CALLER_MCP_TOOLS
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "exactly the four caller tools are served"
    );
    let mut invoke = realm
        .invoke_body(&realm.profile, "Caller profile plan")
        .await;
    invoke["project_id"] = serde_json::json!(realm.project);
    invoke["epic_id"] = serde_json::json!(realm.epic);
    invoke["idempotency_key"] = serde_json::json!("pp-caller-invoke");
    let invoked = caller
        .call("kontor_planning_pair_run_invoke", &invoke)
        .await
        .expect("the invocation is dispatched");
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.body["planning_pair_run_id"]
        .as_str()
        .expect("a planning pair run")
        .to_owned();
    let seat = |slot: &str| {
        SeatBindingId::parse(
            invoked.body["members"]
                .as_array()
                .expect("two members")
                .iter()
                .find(|member| member["slot"] == slot)
                .expect("a member")["seat_binding_id"]
                .as_str()
                .expect("a seat"),
        )
        .expect("a seat id")
    };
    let (seat_a, seat_b) = (seat("seat-a"), seat("seat-b"));
    let at =
        |run: &str| serde_json::json!({"project_id": realm.project, "planning_pair_run_id": run});
    let revision = |envelope: &kontor_mcp::Envelope| envelope.body["revision"].clone();
    let project = realm.project.clone();

    // The caller profile excludes a member's write before any request.
    let before = caller_requests
        .requests
        .load(std::sync::atomic::Ordering::SeqCst);
    let mut contribution = at(&run);
    contribution["advice"] = serde_json::json!("A caller pretending to be a member.");
    contribution["expected_revision"] = revision(&invoked);
    contribution["idempotency_key"] = serde_json::json!("pp-caller-as-member");
    for tool in [
        "kontor_planning_pair_findings_record",
        "kontor_planning_pair_answer_record",
        "kontor_planning_pair_profile_apply",
        "kontor_committee_run_settle",
    ] {
        assert!(
            excluded_by_profile(&caller.call(tool, &contribution).await),
            "{tool} is not on the caller profile"
        );
    }
    assert_eq!(
        caller_requests
            .requests
            .load(std::sync::atomic::Ordering::SeqCst),
        before,
        "an excluded tool makes no request"
    );

    // Members contribute under their own profile; the caller's read stays
    // sealed until both have.
    let member = |seat| {
        seat_dispatcher(
            world,
            operator,
            realm.member_token(seat, 1),
            "planning_pair_member",
        )
        .0
    };
    let (member_a, member_b) = (member(seat_a), member(seat_b));
    assert!(
        excluded_by_profile(
            &member_a
                .call("kontor_planning_pair_run_invoke", &invoke)
                .await
        ),
        "a member profile serves no caller tool"
    );
    let first = mcp_finding(
        &member_a,
        &project,
        &run,
        "Split the migration.",
        "pp-caller-a",
    )
    .await;
    assert_eq!(first.status, 200, "{}", first.body);
    let sealed = mcp_read(&caller, &project, &run).await;
    assert_eq!(sealed.body["viewer"], "caller");
    assert!(sealed.body["findings"].is_null(), "sealed: {}", sealed.body);
    // The caller cannot contribute even under the member profile.
    let (caller_as_member, _) = seat_dispatcher(
        world,
        operator,
        realm.caller_token(),
        "planning_pair_member",
    );
    let refused = mcp_finding(
        &caller_as_member,
        &project,
        &run,
        "Not a member.",
        "pp-caller-member-profile",
    )
    .await;
    assert_eq!(refused.status, 403, "{}", refused.body);
    let second = mcp_finding(&member_b, &project, &run, "Keep it whole.", "pp-caller-b").await;
    assert_eq!(second.status, 200, "{}", second.body);
    let released = mcp_read(&caller, &project, &run).await;
    assert_eq!(slots(&released.body["findings"]), ["seat-a", "seat-b"]);

    // One clarification, through the caller profile.
    let mut ask = at(&run);
    ask["question"] = serde_json::json!("Which step reverts alone?");
    ask["addressed"] = serde_json::json!(["seat-a"]);
    ask["expected_revision"] = revision(&released);
    ask["idempotency_key"] = serde_json::json!("pp-caller-ask");
    let asked = caller
        .call("kontor_planning_pair_clarification_request", &ask)
        .await
        .expect("the clarification is dispatched");
    assert_eq!(asked.status, 200, "{}", asked.body);
    ask["expected_revision"] = revision(&asked);
    ask["idempotency_key"] = serde_json::json!("pp-caller-ask-again");
    let again = caller
        .call("kontor_planning_pair_clarification_request", &ask)
        .await
        .expect("the second clarification is dispatched");
    assert_eq!(again.status, 400, "one clarification only: {}", again.body);
    let mut answer = at(&run);
    answer["advice"] = serde_json::json!("The schema step reverts alone.");
    answer["expected_revision"] = revision(&asked);
    answer["idempotency_key"] = serde_json::json!("pp-caller-answer");
    let answered = member_a
        .call("kontor_planning_pair_answer_record", &answer)
        .await
        .expect("the answer is dispatched");
    assert_eq!(answered.status, 200, "{}", answered.body);

    // The disposition keeps dissent and is no verdict.
    let current = mcp_read(&caller, &project, &run).await;
    let finding_hash = |slot: &str| {
        current.body["findings"]
            .as_array()
            .expect("released findings")
            .iter()
            .find(|entry| entry["slot"] == slot)
            .expect("a finding")["document_hash"]
            .clone()
    };
    let mut decide = at(&run);
    decide["members"] = serde_json::json!([
        {"slot": "seat-a", "finding": finding_hash("seat-a"),
         "answer": current.body["clarification"]["answers"][0]["document_hash"],
         "disposition": "accepted"},
        {"slot": "seat-b", "finding": finding_hash("seat-b"), "disposition": "rejected"},
    ]);
    decide["rationale"] = serde_json::json!("Split first.");
    decide["expected_revision"] = revision(&current);
    decide["idempotency_key"] = serde_json::json!("pp-caller-dispose");
    let disposed = caller
        .call("kontor_planning_pair_disposition_record", &decide)
        .await
        .expect("the disposition is dispatched");
    assert_eq!(disposed.status, 200, "{}", disposed.body);
    assert_eq!(disposed.body["state"], "disposed");
    assert_eq!(slots(&disposed.body["retained_dissent"]), ["seat-b"]);
    for absent in ["verdict", "settled_at", "result", "judge"] {
        assert!(
            disposed.body.get(absent).is_none(),
            "no {absent} on a planning pair: {}",
            disposed.body
        );
    }

    // The profile grants nothing to anyone else.
    let mut other = invoke.clone();
    other["topic"] = serde_json::json!("Another caller plan");
    other["idempotency_key"] = serde_json::json!("pp-caller-other");
    for (bearer, tier, status, code, why) in [
        (
            realm.member_token(seat_a, 1),
            operator,
            409,
            "stale_binding",
            "a member seat holds no hosted caller occupancy",
        ),
        (
            realm.seat_token(realm.tpm, realm.hosted_generation(realm.tpm)),
            operator,
            403,
            "forbidden",
            "a TPM seat is not an allowed caller role",
        ),
        (
            secret(world, "admin"),
            kontor_mcp::CallerTier::Admin,
            403,
            "forbidden",
            "an ambient Admin is no caller seat",
        ),
        (
            secret(world, "operator"),
            operator,
            403,
            "forbidden",
            "an ambient Operator is no caller seat",
        ),
    ] {
        let (dispatcher, _) = seat_dispatcher(world, tier, bearer, "planning_pair_caller");
        let refused = dispatcher
            .call("kontor_planning_pair_run_invoke", &other)
            .await
            .expect("the invocation is dispatched");
        assert_eq!(refused.status, status, "{why}: {}", refused.body);
        assert_eq!(refused.body["code"], code, "{why}: {}", refused.body);
    }
    assert_eq!(
        realm.planning_pair_runs(),
        1,
        "no refused caller froze a pair"
    );
    // A retired caller generation never replays its invocation through it.
    let retired = realm.caller_token();
    retire_caller_generation(&realm);
    let (fenced, _) = seat_dispatcher(world, operator, retired, "planning_pair_caller");
    let replay = fenced
        .call("kontor_planning_pair_run_invoke", &invoke)
        .await
        .expect("the replay is dispatched");
    assert_eq!(replay.status, 409, "{}", replay.body);
    assert_eq!(replay.body["code"], "stale_binding", "{}", replay.body);
}

// ---------------------------------------------------------------------------
// Frontier A: the frozen caller's same-native member recovery.
// ---------------------------------------------------------------------------

/// One member of `slot` as a planning pair projection renders it.
fn member_of_slot(run: &serde_json::Value, slot: &str) -> serde_json::Value {
    run["members"]
        .as_array()
        .expect("two members")
        .iter()
        .find(|member| member["slot"] == slot)
        .cloned()
        .unwrap_or_else(|| panic!("the {slot} member in {run}"))
}

/// One member as a planning pair projection renders it.
fn member_of(run: &serde_json::Value, seat: SeatBindingId) -> serde_json::Value {
    run["members"]
        .as_array()
        .expect("two members")
        .iter()
        .find(|member| member["seat_binding_id"] == seat.to_string())
        .cloned()
        .unwrap_or_else(|| panic!("member {seat} in {run}"))
}

impl PairRealm {
    async fn recover_with(
        &self,
        pair: &Pair,
        seat: SeatBindingId,
        body: &serde_json::Value,
        token: String,
        key: &str,
    ) -> Answer {
        self.write(pair, &format!("/seats/{seat}/recover"), body, token, key)
            .await
    }

    /// The body a caller builds from what it reads: the run revision, the
    /// member's generation and its known native session. Nothing here names a
    /// session the caller did not read.
    async fn recover_body(&self, pair: &Pair, seat: SeatBindingId) -> serde_json::Value {
        let read = self.read_with(pair, Some(self.caller_token())).await;
        assert_eq!(read.status, 200, "{}", read.body);
        let run = read.json();
        let member = member_of(&run, seat);
        let known = &member["known_native"];
        assert!(
            known.is_object(),
            "the caller reads the member's known native: {}",
            read.body
        );
        serde_json::json!({
            "expected_run_revision": run["revision"],
            "expected_member_occupancy_generation": member["occupancy_generation"],
            "expected_native_identity": known["native_identity"],
            "expected_provider_session_id": known["provider_session_id"],
        })
    }

    /// How many receipts of `kind` the realm holds.
    fn receipts_of(&self, kind: &str) -> i64 {
        rusqlite::Connection::open(self.world.directory.path().join("kontor.db"))
            .expect("the realm database opens")
            .query_row(
                "SELECT count(*) FROM command_receipts WHERE kind = ?1",
                [kind],
                |row| row.get(0),
            )
            .expect("the receipts count")
    }

    /// One member seat as storage keeps it.
    fn member_seat(
        &self,
        pair: &Pair,
        seat: SeatBindingId,
    ) -> kontor_core::repository::StoredConsultationSeat {
        let run = self.stored_run(pair);
        self.world
            .daemon
            .state()
            .with_store(|store| store.list_consultation_seats(self.project_id, run.id))
            .expect("the seats read")
            .into_iter()
            .find(|stored| stored.seat_binding_id == seat)
            .expect("the member seat")
    }

    /// One member's kept known native claim at its current generation.
    fn known_claim(
        &self,
        pair: &Pair,
        seat: SeatBindingId,
    ) -> Option<kontor_core::repository::StoredPlanningPairKnownNative> {
        let run = self.stored_run(pair);
        let generation = self.member_seat(pair, seat).occupancy_generation;
        self.world
            .daemon
            .state()
            .with_store(|store| {
                store.planning_pair_known_native(self.project_id, run.id, seat, generation)
            })
            .expect("the claim reads")
    }
}

/// The calls a fake runtime received that could create, replace, archive or
/// reconcile a member.
fn member_effects(calls: &[AdapterCall]) -> Vec<AdapterCall> {
    calls
        .iter()
        .filter(|call| {
            matches!(
                call,
                AdapterCall::LaunchConsultation(_)
                    | AdapterCall::RetireConsultation(_)
                    | AdapterCall::ArchiveContainer(_)
                    | AdapterCall::ReconcilePlanningPairMember(_)
            )
        })
        .cloned()
        .collect()
}

/// A pair whose `slot` member's launch readback did not observe `field`: that
/// member's known native is kept and it is unqualified; the other is bound.
/// Answers the pair and its invoke body.
async fn unqualified_pair(
    realm: &PairRealm,
    slot: PlanningPairSlot,
    field: kontor_runtime::planning_pair::MandatoryMemberField,
    topic: &str,
    key: &str,
) -> (Pair, serde_json::Value) {
    realm
        .world
        .fake
        .observing_planning_pair_member_field_unsupported_in(slot, field);
    let body = realm.invoke_body(&realm.profile, topic).await;
    let first = realm.invoke_with(&body, realm.caller_token(), key).await;
    assert_eq!(first.code(), "unavailable", "{}", first.body);
    let run = realm.world.daemon.state().with_store(|store| {
        store
            .list_consultation_runs(
                realm.project_id,
                MiniProjectId::parse(&realm.epic).expect("an epic id"),
                ConsultationFamily::PlanningPair,
            )
            .expect("the runs read")
            .into_iter()
            .find(|run| run.invoke_key.as_str() == key)
            .expect("the frozen pair")
    });
    let seats = realm.world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    let kontor_core::consultation::ConsultationRunId::PlanningPair(run_id) = run.id else {
        panic!("a planning pair run")
    };
    let seat = |slot: &str| {
        seats
            .iter()
            .find(|seat| seat.role_slot_id.as_str() == slot)
            .expect("the member seat")
            .seat_binding_id
    };
    (
        Pair {
            run: run_id.to_string(),
            seat_a: seat("seat-a"),
            seat_b: seat("seat-b"),
            invoked: serde_json::json!({}),
        },
        body,
    )
}

/// Frontier A, the positive path on the hypothetical fake: a second-seat
/// readback failure is recovered per member. The caller reads seat B's known
/// native, requalifies that same session (the same SeatBinding, generation and
/// native, read back again) and its invocation replay then runs the pair.
/// Nothing is launched, retired or archived; an exact replay of the recovery
/// answers its receipt with no runtime call; a different intent conflicts.
#[tokio::test]
async fn the_caller_requalifies_a_second_seat_on_its_same_native_and_the_replay_runs_the_pair() {
    use kontor_runtime::planning_pair::MandatoryMemberField;
    let realm = pair_realm("/tmp/kontor-asma8282-pair-recover").await;
    let world = &realm.world;
    let (pair, invoke) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::Route,
        "Recover plan",
        "pp-recover-invoke",
    )
    .await;
    let claim = realm
        .known_claim(&pair, pair.seat_b)
        .expect("seat B's claim");
    assert_eq!(
        claim.readback_refusal,
        Some(kontor_core::planning_pair::PlanningPairReadbackRefusal::RouteUnobserved)
    );
    let read = realm.read_with(&pair, Some(realm.caller_token())).await;
    let seat_b = member_of(&read.json(), pair.seat_b);
    assert!(
        seat_b["observed_binding"].is_null(),
        "seat B is not qualified"
    );
    assert_eq!(
        seat_b["known_native"]["readback_refusal"],
        "route_unobserved"
    );
    assert!(member_of(&read.json(), pair.seat_a)["observed_binding"].is_object());
    world
        .fake
        .clearing_planning_pair_member_observation_faults();
    let body = realm.recover_body(&pair, pair.seat_b).await;
    let revision = realm.revision(&pair).await;
    world.fake.take_calls();

    let recovered = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "pp-recover-b",
        )
        .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    let answer = recovered.json();
    assert_eq!(answer["receipt"]["applied"], "created");
    assert_eq!(answer["native_identity"], body["expected_native_identity"]);
    assert_eq!(
        answer["provider_session_id"],
        body["expected_provider_session_id"]
    );
    assert_eq!(answer["member_occupancy_generation"], 1);
    assert_eq!(answer["recovered_revision"], revision + 1);
    assert_eq!(
        answer["planning_pair"]["state"], "materializing",
        "materializing stays"
    );
    assert!(
        member_of(&answer["planning_pair"], pair.seat_b)["observed_binding"].is_object(),
        "seat B is bound to its same native: {}",
        recovered.body
    );
    assert_eq!(
        member_effects(&world.fake.take_calls()),
        vec![AdapterCall::ReconcilePlanningPairMember(pair.seat_b)],
        "one in-place readback, no launch, retirement or archive"
    );
    assert_eq!(
        realm.known_claim(&pair, pair.seat_b),
        Some(claim),
        "the claim is kept unchanged"
    );
    assert_eq!(
        realm
            .member_seat(&pair, pair.seat_b)
            .native_identity
            .map(|identity| identity.native_id),
        Some(
            ExternalId::parse(
                body["expected_native_identity"]["native_id"]
                    .as_str()
                    .expect("an id")
            )
            .expect("a native id")
        )
    );

    let replayed = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "pp-recover-b",
        )
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        replayed.json()["receipt"]["receipt_id"],
        answer["receipt"]["receipt_id"]
    );
    let mut other = body.clone();
    other["expected_run_revision"] = serde_json::json!(revision + 1);
    let conflicting = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &other,
            realm.caller_token(),
            "pp-recover-b",
        )
        .await;
    assert_eq!(
        conflicting.code(),
        "idempotency_conflict",
        "{}",
        conflicting.body
    );
    assert!(
        member_effects(&world.fake.take_calls()).is_empty(),
        "a replay or a conflict reaches no runtime"
    );
    assert_eq!(realm.receipts_of("recover_planning_pair_seat"), 1);

    let invoked = realm
        .invoke_with(&invoke, realm.caller_token(), "pp-recover-invoke")
        .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    assert_eq!(invoked.json()["state"], "running");
    assert_eq!(invoked.json()["receipt"]["applied"], "created");
    assert!(
        member_effects(&world.fake.take_calls()).is_empty(),
        "the invocation resumes on both bound members with no launch"
    );
}

/// Frontier A authority: only the exact frozen caller, at its current hosted
/// generation, recovers. An ambient Admin or Operator, either member, the TPM
/// and a retired caller credential are refused before any replay or runtime
/// call, and a member of another pair is not this pair's to recover.
#[tokio::test]
async fn only_the_frozen_caller_recovers_and_a_retired_caller_never_replays() {
    use kontor_runtime::planning_pair::MandatoryMemberField;
    let realm = pair_realm("/tmp/kontor-asma8282-pair-recover-authority").await;
    let world = &realm.world;
    let (pair, _) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::ToolRestrictions,
        "Authority plan",
        "pp-authority-invoke",
    )
    .await;
    world
        .fake
        .clearing_planning_pair_member_observation_faults();
    let body = realm.recover_body(&pair, pair.seat_b).await;
    world.fake.take_calls();
    let path = realm.run_path(&pair, &format!("/seats/{}/recover", pair.seat_b));
    for tier in ["admin", "operator"] {
        let ambient = Call::post(path.clone(), &body)
            .signed_as(world, tier)
            .with_key(format!("pp-authority-{tier}"))
            .send(world)
            .await;
        assert_eq!(ambient.code(), "forbidden", "{tier}: {}", ambient.body);
    }
    for (who, token) in [
        ("seat A", realm.member_token(pair.seat_a, 1)),
        ("seat B", realm.member_token(pair.seat_b, 1)),
        (
            "the TPM",
            realm.seat_token(realm.tpm, realm.hosted_generation(realm.tpm)),
        ),
    ] {
        let refused = realm
            .recover_with(
                &pair,
                pair.seat_b,
                &body,
                token,
                &format!("pp-authority-{who}"),
            )
            .await;
        assert_eq!(refused.code(), "forbidden", "{who}: {}", refused.body);
    }
    let other = realm.invoke("Another plan", "pp-authority-other").await;
    let foreign = realm
        .recover_with(
            &pair,
            other.seat_b,
            &body,
            realm.caller_token(),
            "pp-authority-foreign",
        )
        .await;
    assert_eq!(foreign.code(), "not_found", "{}", foreign.body);
    // A seat the pinned document admits by role is still not this pair's
    // frozen caller: under a document that also allows the TPM to convene a
    // pair, the TPM cannot recover the LSA's.
    let mut widened = pair_document("01991c00-0000-7000-8000-0000000000b2", PAIR_KIND);
    widened["allowed_caller_roles"] = serde_json::json!(["lsa", "tpm"]);
    let widened = publish_pair_document(world, &realm.project, &widened, "pp-authority-doc").await;
    let invoked = realm
        .invoke_with(
            &realm.invoke_body(&widened, "Widened plan").await,
            realm.caller_token(),
            "pp-authority-widened",
        )
        .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let seat_of = |slot: &str| {
        SeatBindingId::parse(
            member_of_slot(&invoked.json(), slot)["seat_binding_id"]
                .as_str()
                .expect("a seat"),
        )
        .expect("a seat id")
    };
    let lsa_pair = Pair {
        run: invoked.json()["planning_pair_run_id"]
            .as_str()
            .expect("a run id")
            .to_owned(),
        seat_a: seat_of("seat-a"),
        seat_b: seat_of("seat-b"),
        invoked: invoked.json(),
    };
    let lsa_body = realm.recover_body(&lsa_pair, lsa_pair.seat_a).await;
    world.fake.take_calls();
    let by_role = realm
        .recover_with(
            &lsa_pair,
            lsa_pair.seat_a,
            &lsa_body,
            realm.seat_token(realm.tpm, realm.hosted_generation(realm.tpm)),
            "pp-authority-by-role",
        )
        .await;
    assert_eq!(by_role.code(), "forbidden", "{}", by_role.body);
    assert_eq!(
        by_role.json()["rule"],
        "only the planning pair's frozen caller recovers one of its members"
    );
    world.fake.take_calls();

    let recovered = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "pp-authority-recover",
        )
        .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    let retired = realm.caller_token();
    retire_caller_generation(&realm);
    world.fake.take_calls();
    let replay = realm
        .recover_with(&pair, pair.seat_b, &body, retired, "pp-authority-recover")
        .await;
    assert_eq!(replay.code(), "stale_binding", "{}", replay.body);
    let successor = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "pp-authority-recover",
        )
        .await;
    assert_eq!(
        successor.code(),
        "idempotency_conflict",
        "the successor generation is another caller intent: {}",
        successor.body
    );
    assert!(
        member_effects(&world.fake.take_calls()).is_empty(),
        "no refused or replayed request reached the runtime"
    );
    assert_eq!(realm.receipts_of("recover_planning_pair_seat"), 1);
}

/// Frontier A compare-and-swap: the body only asserts. Another native id,
/// runtime kind, host, runtime generation or provider conversation, another
/// member generation and a stale run revision are each refused before any
/// runtime call; an unknown field is refused by the closed body; a member with
/// no known native is refused with nothing discovered or created. The exact
/// assertion then recovers.
#[tokio::test]
async fn a_recovery_asserts_the_exact_known_session_under_compare_and_swap() {
    use kontor_runtime::planning_pair::MandatoryMemberField;
    let realm = pair_realm("/tmp/kontor-asma8282-pair-recover-cas").await;
    let world = &realm.world;
    let (pair, _) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::Correlation,
        "CAS plan",
        "pp-cas-invoke",
    )
    .await;
    world
        .fake
        .clearing_planning_pair_member_observation_faults();
    let body = realm.recover_body(&pair, pair.seat_b).await;
    world.fake.take_calls();
    let mutate = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut changed = body.clone();
        edit(&mut changed);
        changed
    };
    let revision = body["expected_run_revision"].as_u64().expect("a revision");
    for (why, changed, code) in [
        (
            "another native id",
            mutate(&|body| body["expected_native_identity"]["native_id"] = "native-other".into()),
            "stale_binding",
        ),
        (
            "another runtime kind",
            mutate(&|body| body["expected_native_identity"]["runtime_kind"] = "paseo".into()),
            "stale_binding",
        ),
        (
            "another host",
            mutate(&|body| body["expected_native_identity"]["host"] = "another-host".into()),
            "stale_binding",
        ),
        (
            "another runtime generation",
            mutate(&|body| {
                let generation = body["expected_native_identity"]["generation"]
                    .as_u64()
                    .expect("a generation");
                body["expected_native_identity"]["generation"] = (generation + 1).into();
            }),
            "stale_binding",
        ),
        (
            "another provider conversation",
            mutate(&|body| body["expected_provider_session_id"] = "provider-other".into()),
            "stale_binding",
        ),
        (
            "no provider conversation",
            mutate(&|body| body["expected_provider_session_id"] = serde_json::Value::Null),
            "stale_binding",
        ),
        (
            "another member generation",
            mutate(&|body| body["expected_member_occupancy_generation"] = 2.into()),
            "stale_binding",
        ),
        (
            "a stale run revision",
            mutate(&|body| body["expected_run_revision"] = (revision + 1).into()),
            "revision_conflict",
        ),
    ] {
        let refused = realm
            .recover_with(
                &pair,
                pair.seat_b,
                &changed,
                realm.caller_token(),
                &format!("pp-cas-{why}"),
            )
            .await;
        assert_eq!(refused.code(), code, "{why}: {}", refused.body);
    }
    let widened = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &mutate(&|body| body["model_route"] = serde_json::json!({"provider": "codex"})),
            realm.caller_token(),
            "pp-cas-widened",
        )
        .await;
    assert!(
        widened.status.is_client_error() && widened.status != 409,
        "a closed body refuses a route field: {} {}",
        widened.status,
        widened.body
    );
    assert!(
        member_effects(&world.fake.take_calls()).is_empty(),
        "no assertion that failed reached the runtime"
    );
    assert!(
        realm
            .member_seat(&pair, pair.seat_b)
            .native_identity
            .is_none(),
        "nothing was bound"
    );
    let recovered = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "pp-cas-exact",
        )
        .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);

    // A member whose launch never reported a session has no known native:
    // nothing is discovered, created or substituted for it.
    let lost = pair_realm("/tmp/kontor-asma8282-pair-recover-unknown").await;
    lost.world
        .fake
        .refusing_launch_of(&kontor_core::id::RoleSlotId::parse("seat-b").expect("a slot"));
    let lost_body = lost.invoke_body(&lost.profile, "Unknown plan").await;
    let refused = lost
        .invoke_with(&lost_body, lost.caller_token(), "pp-unknown-invoke")
        .await;
    assert_ne!(refused.status, 200, "{}", refused.body);
    let lost_run = lost.world.daemon.state().with_store(|store| {
        store
            .list_consultation_runs(
                lost.project_id,
                MiniProjectId::parse(&lost.epic).expect("an epic id"),
                ConsultationFamily::PlanningPair,
            )
            .expect("the runs read")
            .pop()
            .expect("the frozen pair")
    });
    let lost_seats = lost.world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(lost.project_id, lost_run.id)
            .expect("the seats read")
    });
    let kontor_core::consultation::ConsultationRunId::PlanningPair(lost_id) = lost_run.id else {
        panic!("a planning pair run")
    };
    let lost_pair = Pair {
        run: lost_id.to_string(),
        seat_a: lost_seats[0].seat_binding_id,
        seat_b: lost_seats[1].seat_binding_id,
        invoked: serde_json::json!({}),
    };
    assert!(lost.known_claim(&lost_pair, lost_pair.seat_b).is_none());
    lost.world.fake.take_calls();
    let unknown = lost
        .recover_with(
            &lost_pair,
            lost_pair.seat_b,
            &serde_json::json!({
                "expected_run_revision": lost.revision(&lost_pair).await,
                "expected_member_occupancy_generation": 1,
                "expected_native_identity": body["expected_native_identity"],
            }),
            lost.caller_token(),
            "pp-unknown-recover",
        )
        .await;
    assert_eq!(unknown.code(), "unavailable", "{}", unknown.body);
    assert!(
        unknown.body.contains("no known native session"),
        "{}",
        unknown.body
    );
    assert!(member_effects(&lost.world.fake.take_calls()).is_empty());
}

/// Frontier A durability: an unqualified member's known native is durable. A
/// restarted realm's invocation replay meets the same claim, answers the same
/// typed refusal with no launch at all, and the caller then requalifies that
/// same session.
#[tokio::test]
async fn a_durable_unqualified_member_survives_a_restart_without_a_second_create() {
    use kontor_runtime::planning_pair::MandatoryMemberField;
    let realm = pair_realm("/tmp/kontor-asma8282-pair-recover-restart").await;
    let (pair, invoke) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::Route,
        "Restart recover plan",
        "pp-restart-recover-invoke",
    )
    .await;
    let claim = realm
        .known_claim(&pair, pair.seat_b)
        .expect("seat B's claim");
    let caller_generation = realm.hosted_generation(realm.caller);
    let PairRealm {
        world,
        project,
        project_id,
        epic,
        caller,
        ..
    } = realm;
    let World {
        directory,
        daemon,
        fake,
        ..
    } = world;
    let state_root = directory.path().to_owned();
    drop(daemon);
    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");
    restarted.state().signals().stop();
    let router = restarted.router();
    let token = restarted
        .state()
        .credentials()
        .seat_credential_for_generation(caller, caller_generation);
    fake.take_calls();
    let replayed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/planning-pair-runs:invoke"),
        &invoke,
    )
    .with_token(token.clone())
    .with_key("pp-restart-recover-invoke")
    .send_to(&router)
    .await;
    assert_eq!(replayed.code(), "unavailable", "{}", replayed.body);
    assert_eq!(
        replayed.json()["rule"],
        "the planning pair member's readback did not observe its route"
    );
    assert_eq!(
        replayed.json()["at"],
        format!("native/{}", claim.identity.native_id.as_str())
    );
    assert!(
        member_effects(&fake.take_calls()).is_empty(),
        "the restarted replay met the kept claim: no launch, no second create"
    );
    let kept = restarted
        .state()
        .with_store(|store| {
            store.planning_pair_known_native(project_id, claim.run_id, pair.seat_b, 1)
        })
        .expect("the claim reads");
    assert_eq!(kept, Some(claim.clone()), "the claim survived the restart");

    fake.clearing_planning_pair_member_observation_faults();
    let read = Call::get(format!(
        "/v1/projects/{project}/planning-pair-runs/{}",
        pair.run
    ))
    .with_token(token.clone())
    .send_to(&router)
    .await;
    let member = member_of(&read.json(), pair.seat_b);
    let recovered = Call::post(
        format!(
            "/v1/projects/{project}/planning-pair-runs/{}/seats/{}/recover",
            pair.run, pair.seat_b
        ),
        &serde_json::json!({
            "expected_run_revision": read.json()["revision"],
            "expected_member_occupancy_generation": member["occupancy_generation"],
            "expected_native_identity": member["known_native"]["native_identity"],
            "expected_provider_session_id": member["known_native"]["provider_session_id"],
        }),
    )
    .with_token(token.clone())
    .with_key("pp-restart-recover-b")
    .send_to(&router)
    .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(
        member_effects(&fake.take_calls()),
        vec![AdapterCall::ReconcilePlanningPairMember(pair.seat_b)]
    );
    let resumed = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/planning-pair-runs:invoke"),
        &invoke,
    )
    .with_token(token)
    .with_key("pp-restart-recover-invoke")
    .send_to(&router)
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert_eq!(resumed.json()["state"], "running");
    assert!(member_effects(&fake.take_calls()).is_empty());
}

/// Frontier A adverse readbacks on a running pair. A stopped session, which
/// this runtime cannot resume in place, and a lost session are each refused
/// as unavailable, and nothing is created, replaced or archived. Only the
/// affected member loses its current qualification; its claim, its peer and
/// every finding are kept, and the running pair needs a human. Requalifying
/// both members of the same sessions restores it to running; a lost session
/// can never be requalified.
#[tokio::test]
async fn an_adverse_readback_withdraws_only_that_members_qualification_and_never_replaces() {
    let realm = pair_realm("/tmp/kontor-asma8282-pair-recover-adverse").await;
    let world = &realm.world;
    let pair = realm.invoke("Adverse plan", "pp-adverse-invoke").await;
    let finding = realm
        .finding(&pair, pair.seat_a, "Keep it small.", "pp-adverse-a")
        .await;
    assert_eq!(finding.status, 200, "{}", finding.body);
    let claim_a = realm
        .known_claim(&pair, pair.seat_a)
        .expect("seat A's claim");
    let body_a = realm.recover_body(&pair, pair.seat_a).await;
    world.fake.stopping_consultation_native(pair.seat_a);
    world.fake.take_calls();

    let stopped = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &body_a,
            realm.caller_token(),
            "pp-adverse-stopped",
        )
        .await;
    assert_eq!(stopped.code(), "unavailable", "{}", stopped.body);
    assert_eq!(
        stopped.json()["rule"],
        "the planning pair member's native session is stopped and this runtime does not resume it in place"
    );
    assert!(
        realm
            .member_seat(&pair, pair.seat_a)
            .native_identity
            .is_none(),
        "seat A lost its qualification"
    );
    assert!(
        realm
            .member_seat(&pair, pair.seat_b)
            .native_identity
            .is_some(),
        "seat B kept its own"
    );
    assert_eq!(
        realm.known_claim(&pair, pair.seat_a),
        Some(claim_a.clone()),
        "the claim is kept"
    );
    assert_eq!(
        realm.stored_run(&pair).state,
        ConsultationRunState::NeedsHuman
    );
    assert_eq!(
        realm.contributions(&pair).len(),
        1,
        "seat A's finding is kept"
    );
    assert_eq!(
        realm.receipts_of("recover_planning_pair_seat"),
        0,
        "a refusal writes no receipt"
    );
    let rewrite = realm
        .finding(&pair, pair.seat_a, "Another word.", "pp-adverse-a-again")
        .await;
    assert_eq!(
        rewrite.code(),
        "stale_binding",
        "an unqualified member writes nothing: {}",
        rewrite.body
    );

    let body_b = realm.recover_body(&pair, pair.seat_b).await;
    world.fake.losing_consultation_native(pair.seat_b);
    let lost = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body_b,
            realm.caller_token(),
            "pp-adverse-lost",
        )
        .await;
    assert_eq!(lost.code(), "unavailable", "{}", lost.body);
    assert!(
        realm
            .member_seat(&pair, pair.seat_b)
            .native_identity
            .is_none()
    );
    assert_eq!(
        realm.stored_run(&pair).state,
        ConsultationRunState::NeedsHuman
    );
    let calls = world.fake.take_calls();
    assert_eq!(
        member_effects(&calls),
        vec![
            AdapterCall::ReconcilePlanningPairMember(pair.seat_a),
            AdapterCall::ReconcilePlanningPairMember(pair.seat_b),
        ],
        "only readbacks: no launch, retirement or archive"
    );

    // Seat A's same session resumes; seat B's is gone and is never replaced.
    let body_a = realm.recover_body(&pair, pair.seat_a).await;
    world.fake.running_consultation_native_again(pair.seat_a);
    let requalified = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &body_a,
            realm.caller_token(),
            "pp-adverse-a-back",
        )
        .await;
    assert_eq!(requalified.status, 200, "{}", requalified.body);
    assert_eq!(
        requalified.json()["planning_pair"]["state"],
        "needs_human",
        "one qualified member does not run the pair"
    );
    let body_b = realm.recover_body(&pair, pair.seat_b).await;
    let never = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body_b,
            realm.caller_token(),
            "pp-adverse-b-back",
        )
        .await;
    assert_eq!(never.code(), "unavailable", "{}", never.body);
    assert!(
        member_effects(&world.fake.take_calls())
            .iter()
            .all(|call| matches!(call, AdapterCall::ReconcilePlanningPairMember(_))),
        "a lost native is never relaunched"
    );
    assert_eq!(
        realm.member_seat(&pair, pair.seat_a).occupancy_generation,
        1,
        "no generation moved"
    );
}

/// The LSA's second decision, and the protocol carried through recovery. A
/// qualified seat A records its own first finding while seat B is unqualified
/// and the pair is still materializing: the finding is sealed from the
/// caller, the peer and an observer, and only seat A reads it back. Seat B
/// cannot write; seat A cannot rewrite, ask or decide; there is no invocation,
/// running state or recovery receipt, only seat A's own contribution receipt.
///
/// After seat B is requalified the pair runs and both findings release through
/// the original domain. A member that later loses its qualification keeps its
/// finding, cannot write again until requalified, and moves the running pair
/// to `needs_human`; requalifying it restores `running`. The disposition keeps
/// dissent, and a disposed pair is immutable to a new recovery while an exact
/// replay of a prior one still answers.
#[tokio::test]
async fn a_qualified_member_contributes_alone_sealed_and_the_pair_survives_requalification() {
    use kontor_runtime::planning_pair::MandatoryMemberField;
    let realm = pair_realm("/tmp/kontor-asma8282-pair-recover-decision").await;
    let world = &realm.world;
    let (pair, invoke) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::ToolRestrictions,
        "Decision plan",
        "pp-decision-invoke",
    )
    .await;
    const SEALED: &str = "Seat A alone: ship the schema step first.";
    let alone = realm
        .finding(&pair, pair.seat_a, SEALED, "pp-decision-a")
        .await;
    assert_eq!(alone.status, 200, "{}", alone.body);
    assert_eq!(alone.json()["state"], "materializing");
    assert_eq!(alone.json()["receipt"]["applied"], "created");
    let caller = realm.read_with(&pair, Some(realm.caller_token())).await;
    let observer = realm.read_with(&pair, None).await;
    let peer = realm
        .read_with(&pair, Some(realm.member_token(pair.seat_b, 1)))
        .await;
    for (who, read) in [
        ("caller", &caller),
        ("observer", &observer),
        ("peer", &peer),
    ] {
        assert_eq!(read.status, 200, "{who}: {}", read.body);
        assert!(
            !read.body.contains(SEALED),
            "{who} reads nothing sealed: {}",
            read.body
        );
    }
    let own = realm
        .read_with(&pair, Some(realm.member_token(pair.seat_a, 1)))
        .await;
    assert!(
        own.body.contains(SEALED),
        "seat A reads its own: {}",
        own.body
    );
    let rewrite = realm
        .finding(&pair, pair.seat_a, "A second word.", "pp-decision-a-again")
        .await;
    assert_ne!(rewrite.status, 200, "no rewrite: {}", rewrite.body);
    for (route, body) in [
        (
            "/clarification:request",
            serde_json::json!({"question": "Which?", "addressed": ["seat-a"],
                               "expected_revision": realm.revision(&pair).await}),
        ),
        (
            "/disposition:record",
            serde_json::json!({"members": [], "rationale": "No.",
                               "expected_revision": realm.revision(&pair).await}),
        ),
    ] {
        let refused = realm
            .write(
                &pair,
                route,
                &body,
                realm.member_token(pair.seat_a, 1),
                "pp-decision-a-acts",
            )
            .await;
        assert_eq!(refused.code(), "forbidden", "{route}: {}", refused.body);
    }
    let peer_write = realm
        .finding(&pair, pair.seat_b, "Not qualified.", "pp-decision-b")
        .await;
    assert_eq!(peer_write.code(), "stale_binding", "{}", peer_write.body);
    assert_eq!(realm.receipts_of("record_planning_pair_finding"), 1);
    assert_eq!(realm.receipts_of("invoke_planning_pair_run"), 0);
    assert_eq!(realm.receipts_of("recover_planning_pair_seat"), 0);
    assert_eq!(
        realm.stored_run(&pair).state,
        ConsultationRunState::Materializing
    );

    // Seat B is requalified, the pair runs, and both findings release.
    world
        .fake
        .clearing_planning_pair_member_observation_faults();
    let body_b = realm.recover_body(&pair, pair.seat_b).await;
    let recovered = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body_b,
            realm.caller_token(),
            "pp-decision-b-back",
        )
        .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    let invoked = realm
        .invoke_with(&invoke, realm.caller_token(), "pp-decision-invoke")
        .await;
    assert_eq!(invoked.json()["state"], "running", "{}", invoked.body);
    let second = realm
        .finding(
            &pair,
            pair.seat_b,
            "Seat B: split the migration.",
            "pp-decision-b-finding",
        )
        .await;
    assert_eq!(second.status, 200, "{}", second.body);
    let released = realm.read_with(&pair, Some(realm.caller_token())).await;
    assert_eq!(slots(&released.json()["findings"]), ["seat-a", "seat-b"]);
    assert!(
        released.body.contains(SEALED),
        "released through the domain"
    );
    let asked = realm
        .write(
            &pair,
            "/clarification:request",
            &serde_json::json!({"question": "Which step reverts alone?", "addressed": ["seat-a"],
                                "expected_revision": realm.revision(&pair).await}),
            realm.caller_token(),
            "pp-decision-ask",
        )
        .await;
    assert_eq!(asked.status, 200, "{}", asked.body);

    // Seat A loses its qualification: its finding stays, it cannot answer,
    // and the running pair needs a human until it is requalified.
    world
        .fake
        .observing_planning_pair_member_field_unsupported_in(
            PlanningPairSlot::SeatA,
            MandatoryMemberField::Correlation,
        );
    let body_a = realm.recover_body(&pair, pair.seat_a).await;
    let lost = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &body_a,
            realm.caller_token(),
            "pp-decision-a-lost",
        )
        .await;
    assert_eq!(lost.code(), "unavailable", "{}", lost.body);
    assert_eq!(
        realm.stored_run(&pair).state,
        ConsultationRunState::NeedsHuman
    );
    let answer = |key: &'static str| {
        let realm = &realm;
        let pair = &pair;
        async move {
            let body = serde_json::json!({
                "advice": "The schema step reverts alone.",
                "expected_revision": realm.revision(pair).await,
            });
            realm
                .write(
                    pair,
                    "/answers:record",
                    &body,
                    realm.member_token(pair.seat_a, 1),
                    key,
                )
                .await
        }
    };
    let unqualified = answer("pp-decision-answer-early").await;
    assert_eq!(unqualified.code(), "stale_binding", "{}", unqualified.body);
    let still = realm.read_with(&pair, Some(realm.caller_token())).await;
    assert_eq!(
        slots(&still.json()["findings"]),
        ["seat-a", "seat-b"],
        "nothing is erased"
    );
    world
        .fake
        .clearing_planning_pair_member_observation_faults();
    let body_a = realm.recover_body(&pair, pair.seat_a).await;
    let back = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &body_a,
            realm.caller_token(),
            "pp-decision-a-back",
        )
        .await;
    assert_eq!(back.status, 200, "{}", back.body);
    assert_eq!(
        back.json()["planning_pair"]["state"],
        "running",
        "both qualified again"
    );
    let answered = answer("pp-decision-answer").await;
    assert_eq!(answered.status, 200, "{}", answered.body);

    // The disposition keeps dissent; a disposed pair is immutable.
    let read = realm.read_with(&pair, Some(realm.caller_token())).await;
    let released = read.json();
    let finding_hash = |slot: &str| {
        released["findings"]
            .as_array()
            .expect("released findings")
            .iter()
            .find(|entry| entry["slot"] == slot)
            .expect("a released finding")["document_hash"]
            .clone()
    };
    let disposed = realm
        .write(
            &pair,
            "/disposition:record",
            &serde_json::json!({
                "members": [
                    {"slot": "seat-a", "finding": finding_hash("seat-a"),
                     "answer": released["clarification"]["answers"][0]["document_hash"],
                     "disposition": "accepted"},
                    {"slot": "seat-b", "finding": finding_hash("seat-b"), "disposition": "rejected"},
                ],
                "rationale": "The schema step first.",
                "expected_revision": realm.revision(&pair).await,
            }),
            realm.caller_token(),
            "pp-decision-dispose",
        )
        .await;
    assert_eq!(disposed.json()["state"], "disposed", "{}", disposed.body);
    assert_eq!(slots(&disposed.json()["retained_dissent"]), ["seat-b"]);
    world.fake.take_calls();
    let fresh = realm.recover_body(&pair, pair.seat_a).await;
    let terminal = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &fresh,
            realm.caller_token(),
            "pp-decision-after",
        )
        .await;
    assert_eq!(terminal.code(), "revision_conflict", "{}", terminal.body);
    assert_eq!(
        terminal.json()["rule"],
        "the aggregate is terminal and immutable",
        "a disposed pair is immutable to a new recovery: {}",
        terminal.body
    );
    let replay = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &body_a,
            realm.caller_token(),
            "pp-decision-a-back",
        )
        .await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    assert_eq!(replay.json()["planning_pair"]["state"], "disposed");
    assert!(member_effects(&world.fake.take_calls()).is_empty());
    let after = realm.read_with(&pair, Some(realm.caller_token())).await;
    assert_eq!(
        slots(&after.json()["retained_dissent"]),
        ["seat-b"],
        "dissent is intact"
    );
    let rounds: Vec<(String, String, u64)> = realm
        .contributions(&pair)
        .into_iter()
        .map(|row| {
            (
                row.round.as_str().to_owned(),
                row.slot.as_str().to_owned(),
                row.occupancy_generation,
            )
        })
        .collect();
    assert_eq!(
        rounds,
        [
            ("findings".to_owned(), "seat-a".to_owned(), 1),
            ("findings".to_owned(), "seat-b".to_owned(), 1),
            ("clarification".to_owned(), "seat-a".to_owned(), 1),
        ],
        "one findings round and one answer, no synthetic round or generation"
    );
}

/// Frontier A on real routes and faulty runtimes. A member route the runtime
/// cannot compose is refused before any readback, with no state change, and a
/// runtime that misreports its readback as a create is refused and binds
/// nothing.
#[tokio::test]
async fn a_withheld_route_or_a_misreported_create_never_requalifies() {
    use kontor_runtime::planning_pair::MandatoryMemberField;
    let realm = pair_realm("/tmp/kontor-asma8282-pair-recover-routes").await;
    let world = &realm.world;
    let (pair, _) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::Route,
        "Route recover plan",
        "pp-routes-invoke",
    )
    .await;
    world
        .fake
        .clearing_planning_pair_member_observation_faults();
    let body = realm.recover_body(&pair, pair.seat_b).await;
    world.fake.misreporting_planning_pair_reconcile_as_created();
    world.fake.take_calls();
    let created = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "pp-routes-created",
        )
        .await;
    assert_eq!(created.code(), "unavailable", "{}", created.body);
    assert_eq!(
        created.json()["rule"],
        "the runtime did not answer with the member's known native session"
    );
    assert!(
        realm
            .member_seat(&pair, pair.seat_b)
            .native_identity
            .is_none()
    );
    world.fake.withholding_planning_pair_members_on("cursor");
    let withheld = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "pp-routes-withheld",
        )
        .await;
    assert_eq!(
        withheld.code(),
        "unsupported_capability",
        "{}",
        withheld.body
    );
    assert_eq!(
        member_effects(&world.fake.take_calls()),
        vec![AdapterCall::ReconcilePlanningPairMember(pair.seat_b)],
        "the withheld route never reached a readback"
    );
    assert!(
        realm
            .member_seat(&pair, pair.seat_b)
            .native_identity
            .is_none()
    );
    assert_eq!(realm.receipts_of("recover_planning_pair_seat"), 0);
}

/// Frontier A atomicity, at deterministic barriers. Two recoveries of one key
/// held after their readback and before their write classify exactly one
/// `created`, with one receipt and one bind. A recovery held there while a
/// member's finding moves the run refuses, binding nothing and writing no
/// receipt. A recovery that runs while an invocation is held after its own
/// compare-and-swap and before its receipt moves the run once more, and both
/// receipts are written once.
#[tokio::test]
async fn recoveries_invocations_and_contributions_classify_atomically_at_their_barriers() {
    use kontor_runtime::planning_pair::MandatoryMemberField;
    let realm = pair_realm("/tmp/kontor-asma8282-pair-recover-atomic").await;
    let world = &realm.world;
    let (pair, invoke) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::Route,
        "Atomic plan",
        "pp-atomic-invoke",
    )
    .await;
    world
        .fake
        .clearing_planning_pair_member_observation_faults();
    let body = realm.recover_body(&pair, pair.seat_b).await;
    let hold = world.daemon.hold_planning_pair_recovery_writes();

    // A recovery held at its write while seat A's finding moves the run.
    let held = async {
        realm
            .recover_with(
                &pair,
                pair.seat_b,
                &body,
                realm.caller_token(),
                "pp-atomic-raced",
            )
            .await
    };
    let race = async {
        tokio::time::timeout(Duration::from_secs(30), async {
            while hold.waiting() < 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the recovery reaches its write");
        let finding = realm
            .finding(&pair, pair.seat_a, "Moves the run.", "pp-atomic-a")
            .await;
        assert_eq!(finding.status, 200, "{}", finding.body);
        hold.release();
    };
    let (raced, ()) = tokio::join!(held, race);
    assert_eq!(raced.code(), "revision_conflict", "{}", raced.body);
    assert!(
        realm
            .member_seat(&pair, pair.seat_b)
            .native_identity
            .is_none()
    );
    assert_eq!(realm.receipts_of("recover_planning_pair_seat"), 0);

    // Two recoveries of one key held at their write: one created, one receipt.
    let body = realm.recover_body(&pair, pair.seat_b).await;
    let hold = world.daemon.hold_planning_pair_recovery_writes();
    let open = async {
        tokio::time::timeout(Duration::from_secs(30), async {
            while hold.waiting() < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("both recoveries reach their write before either writes");
        hold.release();
    };
    let (first, second, ()) = tokio::join!(
        realm.recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "pp-atomic-b"
        ),
        realm.recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "pp-atomic-b"
        ),
        open,
    );
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(second.status, 200, "{}", second.body);
    let mut applied = [
        first.json()["receipt"]["applied"].clone(),
        second.json()["receipt"]["applied"].clone(),
    ];
    applied.sort_by_key(ToString::to_string);
    assert_eq!(
        applied,
        [serde_json::json!("created"), serde_json::json!("unchanged")]
    );
    assert_eq!(
        first.json()["receipt"]["receipt_id"],
        second.json()["receipt"]["receipt_id"]
    );
    assert_eq!(realm.receipts_of("recover_planning_pair_seat"), 1);
    let revision_after = realm.revision(&pair).await;
    assert_eq!(
        revision_after,
        body["expected_run_revision"].as_u64().expect("a revision") + 1,
        "the run moved once"
    );

    // An invocation held after its compare-and-swap; a recovery runs between.
    let stale = realm.recover_body(&pair, pair.seat_a).await;
    let invoke_hold = world.daemon.hold_planning_pair_invocation_receipts();
    let invoking = async {
        realm
            .invoke_with(&invoke, realm.caller_token(), "pp-atomic-invoke")
            .await
    };
    let between = async {
        tokio::time::timeout(Duration::from_secs(30), async {
            while invoke_hold.waiting() < 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the invocation reaches its receipt write");
        let refused = realm
            .recover_with(
                &pair,
                pair.seat_a,
                &stale,
                realm.caller_token(),
                "pp-atomic-a-stale",
            )
            .await;
        let current = realm.recover_body(&pair, pair.seat_a).await;
        let recovered = realm
            .recover_with(
                &pair,
                pair.seat_a,
                &current,
                realm.caller_token(),
                "pp-atomic-a-current",
            )
            .await;
        invoke_hold.release();
        (refused, recovered)
    };
    let (invoked, (refused, recovered)) = tokio::join!(invoking, between);
    assert_eq!(
        refused.code(),
        "revision_conflict",
        "the invocation's compare-and-swap moved the run: {}",
        refused.body
    );
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(recovered.json()["receipt"]["applied"], "created");
    assert_eq!(recovered.json()["planning_pair"]["state"], "running");
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    assert_eq!(invoked.json()["receipt"]["applied"], "created");
    assert_eq!(realm.receipts_of("invoke_planning_pair_run"), 1);
    assert_eq!(realm.receipts_of("recover_planning_pair_seat"), 2);
    assert_eq!(realm.revision(&pair).await, revision_after + 2);
}

/// Frontier C, C-M: the readiness seam on one real frozen pair, as a
/// diagnostic consumer reads it.
///
/// It reads only immutable existing facts: the frozen placement from the
/// pair's own record, the daemon-derived member contexts, and the known
/// claims from the claim boundary. It adds hypothetical readbacks from the
/// fake runtime. It keeps the three planes apart:
/// - the selected policy is the frozen placement;
/// - the caller plane is unsupported, although this realm holds a caller
///   credential, because nothing reaches the seam as an authenticated
///   current generation;
/// - seat A is matched on the fake's surface and seat B is unqualified by its
///   unobserved route.
///
/// Nothing is authorized. It writes nothing: no run revision, receipt, claim
/// or binding moves, and the runtime is asked only for readbacks. A changed
/// activation is never read again: the readiness of the frozen pair names
/// the same placement after another fleet policy is activated.
#[tokio::test]
async fn a_frozen_pairs_readiness_keeps_its_three_planes_apart_and_writes_nothing() {
    use kontor_runtime::planning_pair::{
        CallerGap, CallerPlane, MemberEvidence, MemberPlane, MemberReadinessGap,
        PlanningPairMemberReconcileRequest, PlanningPairMemberRoute, PlanningPairReadiness,
        PolicyPlane,
    };
    let realm = pair_realm("/tmp/kontor-asma8282-pair-readiness").await;
    let world = &realm.world;
    let (pair, _) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        kontor_runtime::planning_pair::MandatoryMemberField::Route,
        "Readiness plan",
        "pp-readiness-invoke",
    )
    .await;
    let run = realm.stored_run(&pair);
    let durable = || {
        let seats = world.daemon.state().with_store(|store| {
            store
                .list_consultation_seats(realm.project_id, run.id)
                .expect("the seats read")
        });
        let receipts: i64 = rusqlite::Connection::open(world.directory.path().join("kontor.db"))
            .expect("the realm database opens")
            .query_row("SELECT count(*) FROM command_receipts", [], |row| {
                row.get(0)
            })
            .expect("the receipts count");
        (
            realm.stored_run(&pair).revision,
            receipts,
            seats,
            realm.known_claim(&pair, pair.seat_a),
            realm.known_claim(&pair, pair.seat_b),
        )
    };
    let before = durable();
    let frozen = || {
        let record: kontor_core::planning_pair::PlanningPairRecord = world
            .daemon
            .state()
            .with_store(|store| store.latest_planning_pair_record(realm.project_id, run.id))
            .expect("the record reads")
            .expect("the pair's record")
            .record
            .deserialize()
            .expect("a planning pair record");
        kontor_core::planning_pair::PlanningPairMembers::freeze(
            record.placement_hash,
            record.members,
        )
        .expect("the frozen members")
    };
    let placement = frozen();
    let contexts = world.fake.planning_pair_launch_contexts();
    let context = |seat: SeatBindingId| contexts.get(&seat).cloned().expect("a frozen context");
    let (context_a, context_b) = (context(pair.seat_a), context(pair.seat_b));
    let claim_a = realm
        .known_claim(&pair, pair.seat_a)
        .expect("seat A's claim");
    let claim_b = realm
        .known_claim(&pair, pair.seat_b)
        .expect("seat B's claim");
    world.fake.take_calls();

    let policy_only = PlanningPairReadiness::policy_only(Some(&placement));
    assert!(
        policy_only
            .members
            .iter()
            .all(|member| member.plane == MemberPlane::Unobserved)
    );
    let routes: Vec<PlanningPairMemberRoute> = placement
        .members()
        .iter()
        .map(|member| PlanningPairMemberRoute {
            slot: member.slot,
            model_rung: member.route.clone(),
        })
        .collect();
    let answer = world.fake.validate_planning_pair_member_surface(&routes);
    let readback =
        |context: &kontor_runtime::planning_pair::PlanningPairLaunchContext,
         claim: &kontor_core::repository::StoredPlanningPairKnownNative| {
            PlanningPairMemberReconcileRequest {
                context: context.clone(),
                identity: claim.identity.clone(),
                requested_at: kontor_api::now(),
            }
        };
    let readback_a = world
        .fake
        .reconcile_planning_pair_member(&readback(&context_a, &claim_a))
        .await
        .expect("seat A is read back");
    let readback_b = world
        .fake
        .reconcile_planning_pair_member(&readback(&context_b, &claim_b))
        .await
        .expect("seat B is read back");
    let readiness = PlanningPairReadiness::assess(
        Some(&placement),
        &answer,
        [
            Some(MemberEvidence {
                context: &context_a,
                known: Some(&claim_a),
                readback: Some(&readback_a),
            }),
            Some(MemberEvidence {
                context: &context_b,
                known: Some(&claim_b),
                readback: Some(&readback_b),
            }),
        ],
    );
    assert_eq!(
        readiness.policy,
        PolicyPlane::Selected {
            placement_hash: placement.placement_hash().clone(),
            members: placement.members().to_vec(),
        }
    );
    assert_eq!(
        readiness.caller,
        CallerPlane::Unsupported {
            gap: CallerGap::NoAuthenticatedCallerGeneration,
        },
        "a realm caller credential is not an authenticated generation at this seam"
    );
    assert_eq!(
        readiness.members[0].plane,
        MemberPlane::Matched {
            surface: "fake.runtime".to_owned(),
        }
    );
    assert_eq!(
        readiness.members[1].plane,
        MemberPlane::Unqualified {
            reason: MemberReadinessGap::RouteUnobserved,
        }
    );
    assert!(!readiness.every_plane_established());
    assert!(!readiness.native_actuation_authorized());
    assert_eq!(readiness.document()["native_actuation_authorized"], false);
    let calls = world.fake.take_calls();
    assert_eq!(
        member_effects(&calls),
        vec![
            AdapterCall::ReconcilePlanningPairMember(pair.seat_a),
            AdapterCall::ReconcilePlanningPairMember(pair.seat_b),
        ],
        "only readbacks: no launch, retirement or archive"
    );
    assert_eq!(durable(), before, "the diagnostic wrote nothing");

    activate_fleet_policy(world, &pair_fleet_yaml("cursor-grok"));
    let after = PlanningPairReadiness::policy_only(Some(&frozen()));
    assert_eq!(
        after.policy, policy_only.policy,
        "the frozen pair's policy is its own placement, never a second allocation"
    );
}
