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

const PAIR_PROFILE: &str = "01991c00-0000-7000-8000-0000000000b1";

/// The topology kind and Team Definition container the fixture publishes.
const PAIR_KIND: &str = "PPW";

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

/// One planning pair document selecting `container_kind`.
fn pair_document(profile_id: &str, container_kind: &str) -> serde_json::Value {
    let member = |slot: &str| {
        serde_json::json!({
            "slot": slot,
            "role_code": "SA",
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
    let bundled = kontor_profiles::bundled_operational_domain()
        .expect("the bundled domain validates")
        .topology_specs
        .pop()
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
    node_kinds
        .as_array_mut()
        .expect("node kinds")
        .push(serde_json::json!({
            "kind": PAIR_KIND,
            "allowed_parents": ["ESW"],
            "cardinality": {"minimum": 0},
            "projection_capabilities": ["native_child", "session_host"],
            "read_only": true,
            "name_template": {"segments": [
                {"kind": "literal", "value": "Planning Pair Workspace"}
            ]},
            "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
            "code_help": {
                "full_name": "Planning Pair Workspace",
                "meaning": "Read-only workspace for one planning pair and its two member seats.",
                "category": "session_topology",
                "lifecycle": "current"
            }
        }));
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
    let mut container = definition
        .containers
        .iter()
        .find(|container| container.kind.as_str() == "CSW")
        .expect("the bundled Committee container")
        .clone();
    container.kind = TopologyKindKey::parse(PAIR_KIND).expect("a kind");
    container.prefix = ExternalName::parse(PAIR_KIND).expect("a prefix");
    container.slots = serde_json::from_value(serde_json::json!([
        {"slot_id": "seat-a", "display_name": "SEAT A", "capability_profile": "planning_pair_member"},
        {"slot_id": "seat-b", "display_name": "SEAT B", "capability_profile": "planning_pair_member"},
    ]))
    .expect("the member slots");
    definition.containers.push(container);
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

async fn pair_realm(root: &str) -> PairRealm {
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
    publish_core_team(
        world,
        &project,
        serde_json::json!([seat("SA", "default", true)]),
    )
    .await;
    let (quick, preview_hash) =
        quick_session_ready_to_promote(world, &project, "Plan with a pair", "pp-quick").await;
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
        &pair_document(PAIR_PROFILE, PAIR_KIND),
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
