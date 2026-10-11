//! Desk-led epic and ad-hoc Adams through the supported daemon operations
//! (ASMA-8452).
//!
//! A desk-led Team Definition places an epic's leadership on a subject node
//! below the `ADAM` desk workspace instead of on a per-epic control plane. The
//! definition is published and selected through the supported Admin surfaces,
//! never taken from a bundled revision. Every desk Adam native, epic and
//! ad-hoc alike, launches on the epic-free hosted-seat contract of ASMA-8451
//! in the desk's own project. Native evidence here is the fake runtime's:
//! what placement each launch asked for. The labels and project a real Paseo
//! derives from that placement are proved in the Paseo adapter's contract.

use super::*;
use kontor_core::spec::TeamDefinitionSpec;
use kontor_runtime::durable_seat::HostedSeatScope;

/// The desk and leadership-subject kinds a desk-led topology adds.
const DESK_LED_KINDS: &str = r#"[
  {"kind": "DESK", "allowed_parents": ["PSW"], "cardinality": {"minimum": 0},
   "projection_capabilities": ["native_root"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "Durable Desk", "meaning": "A desk.",
                 "category": "session_topology", "lifecycle": "current"}},
  {"kind": "DWS", "allowed_parents": ["DESK"], "cardinality": {"minimum": 1, "maximum": 1},
   "projection_capabilities": ["native_child", "session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "Desk Workspace", "meaning": "A desk's workspace.",
                 "category": "session_topology", "lifecycle": "current"}},
  {"kind": "ADE", "allowed_parents": ["DWS"], "cardinality": {"minimum": 0},
   "projection_capabilities": ["parent_session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "Epic Adam", "meaning": "An epic's desk leadership.",
                 "category": "session_topology", "lifecycle": "current"}},
  {"kind": "ADH", "allowed_parents": ["DWS"], "cardinality": {"minimum": 0},
   "projection_capabilities": ["parent_session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "Ad-hoc Adam", "meaning": "Ad-hoc desk leadership.",
                 "category": "session_topology", "lifecycle": "current"}}
]"#;

/// The containers a desk-led definition adds, with the role-led seat titles.
const DESK_LED_CONTAINERS: &str = r#"[
  {"kind": "DESK", "parent": null, "prefix": "DESK", "projection_capabilities": ["native_root"],
   "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"},
                                  {"kind": "token", "value": "DESK_NAME"}]}},
  {"kind": "DWS", "parent": "DESK", "prefix": "ADAM",
   "projection_capabilities": ["native_child", "session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"}]}},
  {"kind": "ADE", "parent": "DWS", "prefix": "ADE",
   "projection_capabilities": ["parent_session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "ROLE_CODE"},
                                       {"kind": "token", "value": "EPIC_JIRA_KEY"},
                                       {"kind": "token", "value": "TOPIC"}]},
   "slots": [{"slot_id": "lsa", "role_code": "LSA", "capability_profile": "epic-leadership"},
             {"slot_id": "tpm", "role_code": "TPM", "capability_profile": "epic-leadership"}]},
  {"kind": "ADH", "parent": "DWS", "prefix": "ADH",
   "projection_capabilities": ["parent_session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "ROLE_CODE"},
                                       {"kind": "literal", "value": "ADHOC"},
                                       {"kind": "token", "value": "TOPIC"}]},
   "slots": [{"slot_id": "lsa", "role_code": "LSA", "capability_profile": "epic-leadership"}]}
]"#;

const DESKS: &str = r#"[
  {"desk_key": "adam", "display_name": "ADAM", "kind": "DESK", "workspace_kind": "DWS"},
  {"desk_key": "pr-review", "display_name": "PR REVIEW", "kind": "DESK", "workspace_kind": "DWS"}
]"#;

const LEADERSHIP: &str = r#"{"desk_key": "adam", "epic_kind": "ADE", "ad_hoc_kind": "ADH"}"#;

/// A Claude route: the LSA plans on Anthropic.
fn claude_route() -> serde_json::Value {
    serde_json::json!({"provider": "claude", "model": "claude-opus-5", "effort": "high"})
}

/// A Codex route: the TPM coordinates on OpenAI.
fn codex_route() -> serde_json::Value {
    serde_json::json!({"provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"})
}

/// One project whose selected topology and Team Definition lead every new
/// epic from the `adam` desk, with a Core Team that persists only the LSA and
/// TPM.
struct DeskLed {
    world: World,
    project: String,
    project_id: ProjectId,
    category: String,
}

async fn desk_led_realm(slug: &str) -> DeskLed {
    realm_with(slug, true).await
}

/// The same realm, desk-led or under the definition every legacy epic runs on.
async fn realm_with(slug: &str, desk_led: bool) -> DeskLed {
    let world = World::open_empty_with_a_plane().await;
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let root = world.directory.path().join(format!("{slug}-repository"));
    // Keyed apart from every call key a test chooses, which the slug prefixes.
    let created = ensure_project(
        &world,
        &format!("{slug}-project-ensure"),
        "Kontor",
        root.to_str().expect("a path"),
    )
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    let project = created.json()["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    // Selected before any session node exists, so the project root and every
    // node an epic gets are created under the vocabulary that declares the
    // leadership subjects.
    if desk_led {
        select_desk_led_definition(&world, &project).await;
    }
    adopt_session_base(
        &world,
        &project,
        current_project_revision(&world, &project).await,
    )
    .await;
    publish_core_team(
        &world,
        &project,
        serde_json::json!([seat("SA", "on_demand", true)]),
    )
    .await;
    prepare_fake_provider_headroom(&world, &project).await;
    let category = first_category(&world).await;
    DeskLed {
        project_id: ProjectId::parse(&project).expect("a project id"),
        world,
        project,
        category,
    }
}

/// Publish a desk-led successor of the project's topology and Team Definition
/// through the Admin surfaces, and select both.
async fn select_desk_led_definition(world: &World, project: &str) -> TeamDefinitionSpec {
    let project_id = ProjectId::parse(project).expect("a project id");
    let selected = world.daemon.state().with_store(|store| {
        let selected = store
            .get_project_team_definition_default(project_id)
            .expect("the selection reads")
            .expect("the project has a selection");
        store
            .get_team_definition(
                project_id,
                selected.definition.definition_id,
                selected.definition.version,
            )
            .expect("the selected revision reads")
            .expect("the selected revision exists")
    });
    let lineage = selected.topology.spec_id;
    let base = selected.topology.version.get();
    let current = Call::get(format!(
        "/v1/projects/{project}/topology-specs/{lineage}/{base}"
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(current.status, 200, "{}", current.body);
    let mut node_kinds = current.json()["document"]["node_kinds"].clone();
    let missing: Vec<serde_json::Value> =
        serde_json::from_str::<Vec<serde_json::Value>>(DESK_LED_KINDS)
            .expect("kinds")
            .into_iter()
            .filter(|kind| {
                node_kinds
                    .as_array()
                    .expect("node kinds")
                    .iter()
                    .all(|declared| declared["kind"] != kind["kind"])
            })
            .collect();
    node_kinds
        .as_array_mut()
        .expect("node kinds")
        .extend(missing);
    let drafted = Call::post(
        format!("/v1/projects/{project}/topology-specs:draft"),
        &serde_json::json!({
            "base": {"id": lineage.to_string(), "version": base},
            "name": "Desk-led vocabulary",
            "root_kind": current.json()["document"]["root_kind"],
            "node_kinds": node_kinds,
            "historical_codes": current.json()["document"]["historical_codes"],
        }),
    )
    .signed_as(world, "admin")
    .with_key("desk-led-topology-draft")
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
    .with_key("desk-led-topology-publish")
    .send(world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);
    let topology = published.json()["spec"].clone();
    let preview = Call::post(
        format!("/v1/projects/{project}/topology-selection:preview"),
        &serde_json::json!({"target_spec": {"id": topology["id"], "version": topology["version"]}}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let chosen = Call::post(
        format!("/v1/projects/{project}/topology-selection:apply"),
        &serde_json::json!({
            "preview_hash": preview.json()["preview_hash"],
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "admin")
    .with_key("desk-led-topology-select")
    .send(world)
    .await;
    assert_eq!(chosen.status, 200, "{}", chosen.body);

    let catalog = Call::get(format!("/v1/projects/{project}/team-definitions"))
        .signed_as(world, "admin")
        .send(world)
        .await;
    assert_eq!(catalog.status, 200, "{}", catalog.body);
    let latest = catalog.json()["definitions"]
        .as_array()
        .expect("the Team Definition catalog")
        .iter()
        .filter(|entry| entry["definition"]["id"] == selected.definition_id.to_string())
        .filter_map(|entry| entry["definition"]["version"].as_u64())
        .max()
        .expect("the selected lineage is catalogued");
    let mut definition = serde_json::to_value(&selected).expect("serializes");
    let containers: Vec<serde_json::Value> =
        serde_json::from_str(DESK_LED_CONTAINERS).expect("containers");
    definition["containers"]
        .as_array_mut()
        .expect("containers")
        .extend(containers);
    definition["desks"] = serde_json::from_str(DESKS).expect("desks");
    definition["desk_leadership"] = serde_json::from_str(LEADERSHIP).expect("leadership");
    definition["version"] = serde_json::json!(latest + 1);
    definition["topology"] = serde_json::json!({
        "spec_id": topology["id"],
        "version": topology["version"],
        "canonical_hash": topology["canonical_hash"],
    });
    let validated = Call::post(
        format!("/v1/projects/{project}/team-definitions:validate"),
        &serde_json::json!({"candidate": definition}),
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
            "candidate": definition,
            "validation_hash": validated.json()["validation_hash"],
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "admin")
    .with_key("desk-led-definition-publish")
    .send(world)
    .await;
    assert_eq!(published.status, 200, "{}", published.body);
    let preview = Call::post(
        format!("/v1/projects/{project}/team-definition-selection:preview"),
        &serde_json::json!({"target_definition": {
            "id": selected.definition_id.to_string(),
            "version": latest + 1,
        }}),
    )
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let chosen = Call::post(
        format!("/v1/projects/{project}/team-definition-selection:apply"),
        &serde_json::json!({
            "preview_hash": preview.json()["preview_hash"],
            "expected_revision": current_project_revision(world, project).await,
        }),
    )
    .signed_as(world, "admin")
    .with_key("desk-led-definition-select")
    .send(world)
    .await;
    assert_eq!(chosen.status, 200, "{}", chosen.body);
    serde_json::from_value(definition).expect("the published desk-led definition")
}

/// Apply one single-task epic, optionally bound from an ad-hoc desk Adam.
async fn apply_epic(
    realm: &DeskLed,
    name: &str,
    key: &str,
    origin: Option<serde_json::Value>,
) -> Answer {
    let mut body = epic_body(
        current_project_revision(&realm.world, &realm.project).await,
        name,
        &realm.category,
        serde_json::json!([{"title": format!("{name} work")}]),
    );
    if let Some(origin) = origin {
        body["desk_adam_origin"] = origin;
    }
    Call::post(format!("/v1/projects/{}/epics:apply", realm.project), &body)
        .signed_as(&realm.world, "admin")
        .with_key(key)
        .send(&realm.world)
        .await
}

/// Judge the same epic as `apply_epic` would apply it, writing nothing.
async fn preview_epic(realm: &DeskLed, name: &str, origin: serde_json::Value) -> Answer {
    let mut body = epic_body(
        current_project_revision(&realm.world, &realm.project).await,
        name,
        &realm.category,
        serde_json::json!([{"title": format!("{name} work")}]),
    );
    body["desk_adam_origin"] = origin;
    Call::post(
        format!("/v1/projects/{}/epics:preview", realm.project),
        &body,
    )
    .signed_as(&realm.world, "admin")
    .send(&realm.world)
    .await
}

/// Apply one epic that must succeed, with its Jira identity confirmed.
async fn applied_epic(realm: &DeskLed, name: &str, key: &str) -> String {
    let applied = apply_epic(realm, name, key, None).await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    confirm_test_epic_identity(&realm.world, &realm.project, &epic);
    epic
}

fn epic_id(epic: &str) -> MiniProjectId {
    MiniProjectId::parse(epic).expect("an epic id")
}

fn seat_id(seat: &str) -> SeatBindingId {
    SeatBindingId::parse(seat).expect("a seat binding id")
}

/// The epic's desk Adam subject, as stored.
fn subject_of(realm: &DeskLed, epic: &str) -> kontor_core::repository::StoredDeskAdamSubject {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_epic_desk_adam_subject(realm.project_id, epic_id(epic)))
        .expect("the subject reads")
        .expect("the epic has a desk Adam subject")
}

fn node(realm: &DeskLed, id: TopologyNodeId) -> kontor_core::state::SessionTopologyNode {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_topology_node(realm.project_id, id))
        .expect("the node reads")
        .expect("the node exists")
}

fn seats_on(realm: &DeskLed, id: TopologyNodeId) -> Vec<kontor_core::state::SeatBinding> {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.list_seat_bindings(realm.project_id, id))
        .expect("the seats read")
}

fn hosted(
    realm: &DeskLed,
    seat: SeatBindingId,
) -> Option<kontor_core::repository::StoredHostedTopologySeat> {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_hosted_topology_seat(realm.project_id, seat))
        .expect("the hosted seat reads")
}

fn receipt_recorded(realm: &DeskLed, key: &str) -> bool {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_receipt_by_key(&IdempotencyKey::parse(key).expect("a key")))
        .expect("the receipt reads")
        .is_some()
}

/// Materialize exactly one Core Team seat of `epic` on `route`.
async fn launch_role(
    realm: &DeskLed,
    epic: &str,
    role: &str,
    route: serde_json::Value,
    key: &str,
) -> Answer {
    let revision = realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_mini_project(realm.project_id, epic_id(epic)))
        .expect("the epic reads")
        .expect("the epic exists")
        .revision
        .get();
    Call::post(
        format!(
            "/v1/projects/{}/epics/{epic}/core-team/seats:materialize",
            realm.project
        ),
        &serde_json::json!({
            "expected_revision": revision,
            "routes": [{"role_code": role, "model_route": route}],
        }),
    )
    .signed_as(&realm.world, "admin")
    .with_key(key)
    .send(&realm.world)
    .await
}

/// The epic's planning LSA, launched on Claude.
async fn planning_lsa(realm: &DeskLed, epic: &str, key: &str) -> (SeatBindingId, ExternalId, u64) {
    let launched = launch_role(realm, epic, "LSA", claude_route(), key).await;
    assert_eq!(launched.status, 200, "{}", launched.body);
    let lsa = seat_id(&core_seat(&launched, "LSA"));
    let live = hosted(realm, lsa).expect("the LSA is hosted");
    (
        lsa,
        live.native_identity.native_id.clone(),
        live.native_identity.generation,
    )
}

fn epic_revision(realm: &DeskLed, epic: &str) -> u64 {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_mini_project(realm.project_id, epic_id(epic)))
        .expect("the epic reads")
        .expect("the epic exists")
        .revision
        .get()
}

fn tpm_of(realm: &DeskLed, epic: &str) -> SeatBindingId {
    seats_on(realm, subject_of(realm, epic).topology_node_id)
        .into_iter()
        .find(|seat| seat.role.role_code.as_str() == "TPM" && seat.is_non_terminal())
        .expect("the subject's TPM seat")
        .id
}

async fn hand_off(
    realm: &DeskLed,
    epic: &str,
    (lsa, native, generation): (SeatBindingId, &ExternalId, u64),
    tpm: SeatBindingId,
    route: serde_json::Value,
    key: &str,
) -> Answer {
    Call::post(
        format!(
            "/v1/projects/{}/epics/{epic}/core-team/desk-adam:handoff",
            realm.project
        ),
        &serde_json::json!({
            "expected_revision": epic_revision(realm, epic),
            "from_seat_binding_id": lsa,
            "expected_native_id": native,
            "expected_generation": generation,
            "to_seat_binding_id": tpm,
            "model_route": route,
        }),
    )
    .signed_as(&realm.world, "admin")
    .with_key(key)
    .send(&realm.world)
    .await
}

/// Present `seat`'s generation-scoped credential where only an active seat of
/// `epic` may act, and return the answer.
async fn act_as_seat(realm: &DeskLed, epic: &str, seat: SeatBindingId, generation: u64) -> Answer {
    let bearer = realm
        .world
        .daemon
        .state()
        .credentials()
        .seat_credential_for_generation(seat, generation);
    let question = kontor_core::id::OpenQuestionId::generate();
    Call::post(
        format!(
            "/v1/projects/{}/epics/{epic}/open-questions:record",
            realm.project
        ),
        &serde_json::json!({
            "question_id": question,
            "expected_revision": 0,
            "action": {
                "action": "raise",
                "subject": "Which desk owns this?",
                "scope": "architecture",
                "attachment": {"record": {"kind": "mini_project", "mini_project_id": epic}},
                "why_ambiguous": "Two leadership subjects share one workspace.",
                "options": ["The subject's own epic", "Any epic on the desk"],
            },
        }),
    )
    .with_token(bearer)
    .with_key(format!("desk-adam-act-{question}"))
    .send(&realm.world)
    .await
}

fn title(role: &str, name: &str) -> String {
    format!(
        "{role} • {} • {name}",
        test_jira_key(&format!("{name}-epic"))
    )
}

/// What the fake runtime was asked to do since `before`.
fn calls_since(realm: &DeskLed, before: usize) -> Vec<AdapterCall> {
    realm.world.fake.calls()[before..].to_vec()
}

/// The `adam` desk every desk Adam of these realms is led from.
fn adam_desk(realm: &DeskLed) -> kontor_core::repository::StoredDesk {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| {
            store.get_desk(
                realm.project_id,
                &kontor_core::id::DeskKey::parse("adam").expect("a desk key"),
            )
        })
        .expect("the desk reads")
        .expect("the adam desk is materialized")
}

/// Assert that `seat`'s last launch asked to be placed in the `adam` desk's
/// own native project, the desk workspace's exact persisted parent, and named
/// no epic.
fn assert_desk_placed(realm: &DeskLed, seat: SeatBindingId) {
    let desk = adam_desk(realm);
    let project = container(realm, desk.topology_node_id).identity;
    assert_eq!(
        container(realm, desk.workspace_node_id)
            .readback
            .expect("a complete readback")
            .native_parent,
        Some(project.clone()),
        "the desk workspace hangs under the desk's own project"
    );
    let scope = realm
        .world
        .fake
        .hosted_launch_scope(seat)
        .expect("the seat was launched");
    assert!(
        scope.epic().is_none(),
        "a desk Adam names no epic: {scope:?}"
    );
    assert_eq!(scope, HostedSeatScope::Desk { project });
}

#[tokio::test]
async fn a_desk_led_epic_places_its_lsa_on_the_desk_with_a_role_led_title_and_no_ecp() {
    let realm = desk_led_realm("desk-adam-placement").await;
    let epic = applied_epic(&realm, "Durable desks", "desk-adam-placement-epic").await;

    // The epic's leadership is a subject node directly below the desk's
    // workspace, scoped to the epic, and the epic has no ECP at all.
    let subject = subject_of(&realm, &epic);
    assert_eq!(subject.desk_key.as_str(), "adam");
    assert_eq!(subject.topic.as_str(), "Durable desks");
    assert_eq!(subject.origin_topology_node_id, None);
    let placed = node(&realm, subject.topology_node_id);
    assert_eq!(placed.kind.as_str(), "ADE");
    assert_eq!(placed.mini_project_id, Some(epic_id(&epic)));
    let workspace = node(&realm, placed.parent_id.expect("a parent"));
    assert_eq!(workspace.kind.as_str(), "DWS");
    assert_eq!(workspace.mini_project_id, None);
    let epic_nodes = realm.world.daemon.state().with_store(|store| {
        store
            .list_topology_nodes(realm.project_id, Some(epic_id(&epic)))
            .expect("the epic's nodes read")
    });
    assert!(
        epic_nodes.iter().all(|node| node.kind.as_str() != "ECP"),
        "a desk-led epic has no ECP: {epic_nodes:?}"
    );

    let before = realm.world.fake.calls().len();
    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-placement-lsa").await;
    assert_eq!(generation, 1);
    assert_eq!(
        realm.world.fake.seat_title(&native).as_deref(),
        Some(title("LSA", "Durable desks").as_str())
    );
    assert_desk_placed(&realm, lsa);
    // The session borrows the desk workspace's native container; the subject
    // node never gets one of its own.
    let calls = calls_since(&realm, before);
    assert!(
        calls.iter().all(|call| !matches!(
            call,
            AdapterCall::PrepareContainer(id) | AdapterCall::CreateNativeContainer(id)
                if *id == subject.topology_node_id
        )),
        "{calls:?}"
    );
    assert!(realm.world.fake.container_native_id(workspace.id).is_some());
    assert!(
        realm
            .world
            .fake
            .container_native_id(subject.topology_node_id)
            .is_none()
    );
    assert_eq!(
        seats_on(&realm, subject.topology_node_id)
            .iter()
            .find(|seat| seat.id == lsa)
            .expect("the LSA seat is the subject's")
            .role_slot_id
            .as_str(),
        "lsa"
    );

    // One phase owner at a time: routing the TPM beside the live LSA refuses
    // before any native effect.
    let before = realm.world.fake.calls().len();
    let second = launch_role(
        &realm,
        &epic,
        "TPM",
        codex_route(),
        "desk-adam-placement-tpm",
    )
    .await;
    assert_eq!(
        second.json()["code"],
        "placement_blocked",
        "{}",
        second.body
    );
    assert!(
        calls_since(&realm, before)
            .iter()
            .all(|call| !matches!(call, AdapterCall::LaunchHostedSeat(_)))
    );
    assert!(!receipt_recorded(&realm, "desk-adam-placement-tpm"));
}

#[tokio::test]
async fn a_cross_vendor_handoff_fences_the_lsa_before_the_tpm_launches_and_replays() {
    let realm = desk_led_realm("desk-adam-handoff").await;
    let epic = applied_epic(&realm, "Handoff epic", "desk-adam-handoff-epic").await;
    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-handoff-lsa").await;
    let tpm = tpm_of(&realm, &epic);
    let planning = act_as_seat(&realm, &epic, lsa, 1).await;
    assert_eq!(planning.status, 200, "{}", planning.body);

    let before = realm.world.fake.calls().len();
    let handed = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        codex_route(),
        "desk-adam-handoff",
    )
    .await;
    assert_eq!(handed.status, 200, "{}", handed.body);
    assert_eq!(handed.json()["fenced_native_id"], native.as_str());
    assert_eq!(handed.json()["from_seat_binding_id"], lsa.to_string());
    assert_eq!(handed.json()["to_seat_binding_id"], tpm.to_string());
    assert_eq!(handed.json()["receipt"]["applied"], "created");

    // The LSA's native is retired before the TPM's is launched.
    let calls = calls_since(&realm, before);
    let retired = calls
        .iter()
        .position(|call| *call == AdapterCall::RetireHostedSeat(lsa))
        .expect("the LSA was retired");
    let launched = calls
        .iter()
        .position(|call| *call == AdapterCall::LaunchHostedSeat(tpm))
        .expect("the TPM was launched");
    assert!(retired < launched, "{calls:?}");
    assert!(
        hosted(&realm, lsa).is_none(),
        "the LSA occupancy is in history"
    );
    let successor = hosted(&realm, tpm).expect("the TPM is hosted");
    assert_eq!(successor.model_rung.provider.0, "codex");
    assert_eq!(
        realm
            .world
            .fake
            .seat_title(&successor.native_identity.native_id)
            .as_deref(),
        Some(title("TPM", "Handoff epic").as_str())
    );
    assert_desk_placed(&realm, tpm);

    // The fenced LSA's credential names no live occupancy; the TPM's does.
    let fenced = act_as_seat(&realm, &epic, lsa, 1).await;
    assert_eq!(fenced.status, 409, "{}", fenced.body);
    assert_eq!(fenced.json()["code"], "stale_binding", "{}", fenced.body);
    let coordinating = act_as_seat(&realm, &epic, tpm, 1).await;
    assert_eq!(coordinating.status, 200, "{}", coordinating.body);

    // The same request replays its receipt and asks the runtime for nothing.
    let before = realm.world.fake.calls().len();
    let replayed = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        codex_route(),
        "desk-adam-handoff",
    )
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        replayed.json()["receipt"]["receipt_id"],
        handed.json()["receipt"]["receipt_id"]
    );
    let calls = calls_since(&realm, before);
    assert!(
        calls.iter().all(|call| !matches!(
            call,
            AdapterCall::RetireHostedSeat(_) | AdapterCall::LaunchHostedSeat(_)
        )),
        "{calls:?}"
    );
}

#[tokio::test]
async fn a_handoff_that_lost_its_answer_resumes_from_the_step_it_reached() {
    let realm = desk_led_realm("desk-adam-handoff-crash").await;
    let epic = applied_epic(&realm, "Crash epic", "desk-adam-crash-epic").await;
    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-crash-lsa").await;
    let tpm = tpm_of(&realm, &epic);

    // The fence lands and the TPM launch is refused: no owner, never two.
    realm.world.fake.refuse_next_hosted_launch("codex");
    let interrupted = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        codex_route(),
        "desk-adam-crash",
    )
    .await;
    assert_ne!(interrupted.status, 200, "{}", interrupted.body);
    assert!(hosted(&realm, lsa).is_none());
    assert!(hosted(&realm, tpm).is_none());
    assert!(!receipt_recorded(&realm, "desk-adam-crash"));

    // The same request finds the predecessor in history and launches the TPM.
    let before = realm.world.fake.calls().len();
    let resumed = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        codex_route(),
        "desk-adam-crash",
    )
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    let calls = calls_since(&realm, before);
    assert!(
        !calls.contains(&AdapterCall::RetireHostedSeat(lsa)),
        "{calls:?}"
    );
    assert!(
        calls.contains(&AdapterCall::LaunchHostedSeat(tpm)),
        "{calls:?}"
    );
    assert!(hosted(&realm, tpm).is_some());

    // A retire whose acknowledgement was lost is proved absent on retry.
    let realm = desk_led_realm("desk-adam-handoff-lost-ack").await;
    let epic = applied_epic(&realm, "Lost ack epic", "desk-adam-lost-epic").await;
    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-lost-lsa").await;
    let tpm = tpm_of(&realm, &epic);
    realm.world.fake.lose_next_hosted_retire_ack(lsa);
    let lost = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        codex_route(),
        "desk-adam-lost",
    )
    .await;
    assert_ne!(lost.status, 200, "{}", lost.body);
    assert!(
        hosted(&realm, tpm).is_none(),
        "nothing launched past a lost fence"
    );
    let resumed = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        codex_route(),
        "desk-adam-lost",
    )
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    assert!(hosted(&realm, lsa).is_none());
    assert!(hosted(&realm, tpm).is_some());
}

#[tokio::test]
async fn a_same_vendor_handoff_is_refused_before_any_effect() {
    let realm = desk_led_realm("desk-adam-same-vendor").await;
    let epic = applied_epic(&realm, "Same vendor epic", "desk-adam-same-epic").await;
    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-same-lsa").await;
    let tpm = tpm_of(&realm, &epic);
    let before = realm.world.fake.calls().len();
    let refused = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        serde_json::json!({"provider": "claude-work", "model": "claude-fable-5", "effort": "high"}),
        "desk-adam-same",
    )
    .await;
    assert_eq!(
        refused.json()["code"],
        "unsupported_capability",
        "{}",
        refused.body
    );
    assert!(
        refused.body.contains("seat-credential continuity"),
        "{}",
        refused.body
    );
    assert!(calls_since(&realm, before).is_empty());
    assert_eq!(
        hosted(&realm, lsa)
            .expect("the LSA is still live")
            .native_identity
            .native_id,
        native
    );
    assert!(hosted(&realm, tpm).is_none());
    assert!(!receipt_recorded(&realm, "desk-adam-same"));
}

#[tokio::test]
async fn a_handoff_naming_anything_but_the_exact_live_predecessor_refuses_before_effects() {
    let realm = desk_led_realm("desk-adam-exact").await;
    let epic = applied_epic(&realm, "Exact epic", "desk-adam-exact-epic").await;
    let other = applied_epic(&realm, "Other epic", "desk-adam-exact-other").await;
    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-exact-lsa").await;
    let (other_lsa, other_native, other_generation) =
        planning_lsa(&realm, &other, "desk-adam-exact-other-lsa").await;
    let tpm = tpm_of(&realm, &epic);
    let other_tpm = tpm_of(&realm, &other);
    let stranger = ExternalId::parse("native-hosted-seat-999").expect("an id");

    for (case, from, to, code) in [
        (
            "generation",
            (lsa, &native, generation + 1),
            tpm,
            "revision_conflict",
        ),
        (
            "native",
            (lsa, &stranger, generation),
            tpm,
            "revision_conflict",
        ),
        (
            "to-role",
            (lsa, &native, generation),
            lsa,
            "placement_blocked",
        ),
        // Another epic's seats, though they share the desk workspace.
        (
            "from-ancestry",
            (other_lsa, &other_native, other_generation),
            tpm,
            "placement_blocked",
        ),
        (
            "to-ancestry",
            (lsa, &native, generation),
            other_tpm,
            "placement_blocked",
        ),
    ] {
        let before = realm.world.fake.calls().len();
        let key = format!("desk-adam-exact-{case}");
        let refused = hand_off(&realm, &epic, from, to, codex_route(), &key).await;
        assert_eq!(refused.json()["code"], code, "{case}: {}", refused.body);
        assert!(calls_since(&realm, before).is_empty(), "{case}");
        assert!(!receipt_recorded(&realm, &key), "{case}");
    }

    // The desk workspace the session borrows no longer answers: refused at
    // the readback, before the runtime is asked to retire anything.
    let workspace = node(&realm, subject_of(&realm, &epic).topology_node_id)
        .parent_id
        .expect("the desk workspace");
    realm.world.fake.forget_container(workspace);
    let before = realm.world.fake.calls().len();
    let refused = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        codex_route(),
        "desk-adam-exact-host",
    )
    .await;
    assert_eq!(refused.json()["code"], "stale_binding", "{}", refused.body);
    assert!(!calls_since(&realm, before).contains(&AdapterCall::RetireHostedSeat(lsa)));
    assert!(hosted(&realm, lsa).is_some());
    assert!(hosted(&realm, other_lsa).is_some());
    assert!(!receipt_recorded(&realm, "desk-adam-exact-host"));
}

/// Seat a same-role LSA on the desk workspace itself, beside the epic
/// subjects: the shape a durable desk seat has, as a disposable fixture.
fn desk_level_lsa(
    realm: &DeskLed,
    workspace: TopologyNodeId,
    like: SeatBindingId,
) -> SeatBindingId {
    let role = seats_on(realm, subject_node_of_seat(realm, like))
        .into_iter()
        .find(|seat| seat.id == like)
        .expect("the template seat")
        .role;
    let now = kontor_api::now();
    realm
        .world
        .daemon
        .state()
        .with_store(|store| {
            store.create_seat_binding(&kontor_core::repository::NewSeatBinding {
                id: SeatBindingId::generate(),
                project_id: realm.project_id,
                topology_node_id: workspace,
                role_slot_id: kontor_core::id::RoleSlotId::parse("desk-lsa").expect("a slot"),
                role,
                task_id: None,
                team_run_id: None,
                attach_deadline: now,
                parent_seat_binding_id: None,
                created_at: now,
            })
        })
        .expect("the desk-level seat is recorded")
        .id
}

fn subject_node_of_seat(realm: &DeskLed, seat: SeatBindingId) -> TopologyNodeId {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_seat_binding(realm.project_id, seat))
        .expect("the seat reads")
        .expect("the seat exists")
        .topology_node_id
}

/// The TPM a completion run answers to, as the run stored it.
fn completion_tpm(realm: &DeskLed, epic: &str) -> String {
    let stored = realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_epic_completion(realm.project_id, epic_id(epic)))
        .expect("the completion reads")
        .expect("the completion exists");
    stored.state["tpm_seat_id"]
        .as_str()
        .expect("the run names its TPM")
        .to_owned()
}

#[tokio::test]
async fn two_epic_adams_in_one_workspace_never_share_ownership_credentials_or_evidence() {
    let realm = desk_led_realm("desk-adam-isolation").await;
    let alpha = applied_epic(&realm, "Alpha desk", "desk-adam-isolation-alpha").await;
    let beta = applied_epic(&realm, "Beta desk", "desk-adam-isolation-beta").await;
    let alpha_subject = subject_of(&realm, &alpha);
    let beta_subject = subject_of(&realm, &beta);
    assert_ne!(
        alpha_subject.topology_node_id,
        beta_subject.topology_node_id
    );
    let workspace = node(&realm, alpha_subject.topology_node_id)
        .parent_id
        .expect("the desk workspace");
    assert_eq!(
        node(&realm, beta_subject.topology_node_id).parent_id,
        Some(workspace),
        "both subjects share the one ADAM workspace"
    );

    // Both planning sessions live at once, in the same native workspace.
    let (alpha_lsa, alpha_native, _) =
        planning_lsa(&realm, &alpha, "desk-adam-isolation-a-lsa").await;
    let (beta_lsa, beta_native, _) = planning_lsa(&realm, &beta, "desk-adam-isolation-b-lsa").await;
    assert_ne!(alpha_native, beta_native);
    // Both are placed in the one desk project with no epic; nothing below
    // tells them apart but their subjects, which is what every check reads.
    assert_desk_placed(&realm, alpha_lsa);
    assert_desk_placed(&realm, beta_lsa);
    assert_eq!(
        realm.world.fake.seat_title(&beta_native).as_deref(),
        Some(title("LSA", "Beta desk").as_str())
    );
    let durable = desk_level_lsa(&realm, workspace, alpha_lsa);

    // Credentials and evidence: each LSA acts for its own epic only, in both
    // directions, and the desk-level seat for neither.
    for (seat, own, foreign) in [(alpha_lsa, &alpha, &beta), (beta_lsa, &beta, &alpha)] {
        let own_answer = act_as_seat(&realm, own, seat, 1).await;
        assert_eq!(own_answer.status, 200, "{}", own_answer.body);
        let foreign_answer = act_as_seat(&realm, foreign, seat, 1).await;
        assert_eq!(foreign_answer.status, 403, "{}", foreign_answer.body);
        assert!(
            foreign_answer
                .body
                .contains("active seat of this exact epic"),
            "{}",
            foreign_answer.body
        );
    }
    for epic in [&alpha, &beta] {
        let desk_answer = act_as_seat(&realm, epic, durable, 1).await;
        assert_ne!(desk_answer.status, 200, "{}", desk_answer.body);
    }

    // Ownership: each epic's Core Team reads back only its own subject's seats.
    for (epic, subject, foreign) in [
        (&alpha, &alpha_subject, beta_lsa),
        (&beta, &beta_subject, alpha_lsa),
    ] {
        let read = Call::get(format!(
            "/v1/projects/{}/epics/{epic}/core-team",
            realm.project
        ))
        .signed_as(&realm.world, "observer")
        .send(&realm.world)
        .await;
        assert_eq!(read.status, 200, "{}", read.body);
        let body = read.json();
        let listed: BTreeSet<String> = body["seats"]
            .as_array()
            .or_else(|| body["core_team"]["seats"].as_array())
            .expect("the Core Team seats")
            .iter()
            .filter_map(|seat| seat["seat_binding_id"].as_str().map(str::to_owned))
            .collect();
        let own: BTreeSet<String> = seats_on(&realm, subject.topology_node_id)
            .iter()
            .map(|seat| seat.id.to_string())
            .collect();
        assert_eq!(listed, own, "{}", read.body);
        assert!(!listed.contains(&foreign.to_string()));
        assert!(!listed.contains(&durable.to_string()));
    }

    // Completion: each run answers to its own epic's TPM.
    for (epic, key) in [
        (&alpha, "desk-adam-isolation-a-done"),
        (&beta, "desk-adam-isolation-b-done"),
    ] {
        let started = Call::post(
            format!(
                "/v1/projects/{}/epics/{epic}/completion:advance",
                realm.project
            ),
            &serde_json::json!({"expected_revision": 1}),
        )
        .signed_as(&realm.world, "operator")
        .with_key(key)
        .send(&realm.world)
        .await;
        // The owner resolves from the epic's own subject before the run waits
        // on its open ticket; the stored owner is proved where the ticket closes.
        assert_eq!(
            started.json()["subject"],
            "completion ticket gate",
            "{}",
            started.body
        );
    }
    assert_ne!(tpm_of(&realm, &alpha), tpm_of(&realm, &beta));

    // Handing one subject off leaves the other's session untouched.
    let alpha_tpm = tpm_of(&realm, &alpha);
    let live = hosted(&realm, alpha_lsa).expect("alpha's LSA");
    let handed = hand_off(
        &realm,
        &alpha,
        (
            alpha_lsa,
            &live.native_identity.native_id,
            live.native_identity.generation,
        ),
        alpha_tpm,
        codex_route(),
        "desk-adam-isolation-a-handoff",
    )
    .await;
    assert_eq!(handed.status, 200, "{}", handed.body);
    assert_eq!(
        hosted(&realm, beta_lsa)
            .expect("beta's LSA is still live")
            .native_identity
            .native_id,
        beta_native
    );
    let beta_still = act_as_seat(&realm, &beta, beta_lsa, 1).await;
    assert_eq!(beta_still.status, 200, "{}", beta_still.body);
    let alpha_tpm_on_beta = act_as_seat(&realm, &beta, alpha_tpm, 1).await;
    assert_eq!(alpha_tpm_on_beta.status, 403, "{}", alpha_tpm_on_beta.body);
}

async fn open_ad_hoc(realm: &DeskLed, body: serde_json::Value, key: &str) -> Answer {
    Call::post(
        format!("/v1/projects/{}/desk-adams/ad-hoc:open", realm.project),
        &body,
    )
    .signed_as(&realm.world, "admin")
    .with_key(key)
    .send(&realm.world)
    .await
}

/// Open one ad-hoc desk Adam and return its subject node and LSA seat.
async fn ad_hoc(realm: &DeskLed, topic: &str, key: &str) -> (TopologyNodeId, SeatBindingId) {
    let opened = open_ad_hoc(realm, serde_json::json!({"topic": topic}), key).await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    (
        TopologyNodeId::parse(
            opened.json()["subject"]["topology_node_id"]
                .as_str()
                .expect("a subject node"),
        )
        .expect("a node id"),
        seat_id(
            opened.json()["lsa_seat_binding_id"]
                .as_str()
                .expect("an LSA seat"),
        ),
    )
}

/// Open one ad-hoc desk Adam with its LSA launched on Claude, and return its
/// subject node, LSA seat and live native.
async fn ad_hoc_live(
    realm: &DeskLed,
    topic: &str,
    key: &str,
) -> (TopologyNodeId, SeatBindingId, ExternalId) {
    let opened = open_ad_hoc(
        realm,
        serde_json::json!({"topic": topic, "model_route": claude_route()}),
        key,
    )
    .await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let lsa = seat_id(
        opened.json()["lsa_seat_binding_id"]
            .as_str()
            .expect("an LSA seat"),
    );
    let native = hosted(realm, lsa)
        .expect("the ad-hoc LSA is hosted")
        .native_identity
        .native_id;
    (
        TopologyNodeId::parse(
            opened.json()["subject"]["topology_node_id"]
                .as_str()
                .expect("a subject node"),
        )
        .expect("a node id"),
        lsa,
        native,
    )
}

fn seat_row(realm: &DeskLed, seat: SeatBindingId) -> kontor_core::state::SeatBinding {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_seat_binding(realm.project_id, seat))
        .expect("the seat reads")
        .expect("the seat exists")
}

fn epic_named(realm: &DeskLed, name: &str) -> bool {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.list_mini_projects(realm.project_id))
        .expect("the epics read")
        .iter()
        .any(|epic| epic.name.as_str() == name)
}

#[tokio::test]
async fn an_ad_hoc_desk_adam_opens_its_subject_and_lsa_with_a_frozen_topic() {
    let realm = desk_led_realm("desk-adam-ad-hoc").await;
    let before = realm.world.fake.calls().len();
    let opened = open_ad_hoc(
        &realm,
        serde_json::json!({"topic": "Kontor triage"}),
        "desk-adam-ad-hoc-open",
    )
    .await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    assert_eq!(opened.json()["lsa_title"], "LSA • ADHOC • Kontor triage");
    assert_eq!(opened.json()["subject"]["desk_key"], "adam");
    assert_eq!(opened.json()["subject"]["topic"], "Kontor triage");
    assert!(
        opened.json()["subject"]["epic_id"].is_null(),
        "{}",
        opened.body
    );
    assert_eq!(opened.json()["receipt"]["applied"], "created");
    let subject = TopologyNodeId::parse(
        opened.json()["subject"]["topology_node_id"]
            .as_str()
            .expect("a node"),
    )
    .expect("a node id");
    let lsa = seat_id(
        opened.json()["lsa_seat_binding_id"]
            .as_str()
            .expect("a seat"),
    );
    let placed = node(&realm, subject);
    assert_eq!(placed.kind.as_str(), "ADH");
    assert_eq!(placed.mini_project_id, None);
    assert_eq!(
        node(&realm, placed.parent_id.expect("a parent"))
            .kind
            .as_str(),
        "DWS"
    );
    let seat = seat_row(&realm, lsa);
    assert_eq!(seat.topology_node_id, subject);
    assert_eq!(seat.role.role_code.as_str(), "LSA");
    assert!(hosted(&realm, lsa).is_none(), "no native without a route");
    assert!(opened.json()["lsa_native_id"].is_null(), "{}", opened.body);
    assert!(
        calls_since(&realm, before)
            .iter()
            .all(|call| !matches!(call, AdapterCall::LaunchHostedSeat(_))),
    );

    // The same request replays; another key for the same topic finds the same
    // subject and seat rather than opening a second.
    let replayed = open_ad_hoc(
        &realm,
        serde_json::json!({"topic": "Kontor triage"}),
        "desk-adam-ad-hoc-open",
    )
    .await;
    assert_eq!(
        replayed.json()["receipt"]["applied"],
        "unchanged",
        "{}",
        replayed.body
    );
    assert_eq!(
        replayed.json()["receipt"]["receipt_id"],
        opened.json()["receipt"]["receipt_id"]
    );
    let again = open_ad_hoc(
        &realm,
        serde_json::json!({"topic": "Kontor triage"}),
        "desk-adam-ad-hoc-again",
    )
    .await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(
        again.json()["subject"]["topology_node_id"],
        subject.to_string()
    );
    assert_eq!(again.json()["lsa_seat_binding_id"], lsa.to_string());
    assert_eq!(seats_on(&realm, subject).len(), 1);

    // A separator glyph could forge a title segment, and a route that names
    // no catalog model cannot launch. Each refuses before anything is placed.
    let routed = open_ad_hoc(
        &realm,
        serde_json::json!({"topic": "Routed topic",
                           "model_route": {"provider": "claude", "model": ""}}),
        "desk-adam-ad-hoc-routed",
    )
    .await;
    assert_eq!(routed.status, 400, "{}", routed.body);
    assert!(!receipt_recorded(&realm, "desk-adam-ad-hoc-routed"));
    for (topic, key) in [
        ("Kontor • triage", "desk-adam-ad-hoc-bullet"),
        ("Kontor · triage", "desk-adam-ad-hoc-dot"),
    ] {
        let refused = open_ad_hoc(&realm, serde_json::json!({"topic": topic}), key).await;
        assert_eq!(
            refused.json()["code"],
            "invalid_request",
            "{}",
            refused.body
        );
        assert!(!receipt_recorded(&realm, key));
    }
    let listed = realm.world.daemon.state().with_store(|store| {
        store
            .list_desk_adam_subjects(
                realm.project_id,
                &kontor_core::id::DeskKey::parse("adam").expect("a desk key"),
            )
            .expect("the subjects read")
    });
    assert_eq!(listed.len(), 1, "{listed:?}");

    // A project whose selected definition leads from no desk opens none.
    let plain = ensure_project(
        &realm.world,
        "desk-adam-ad-hoc-plain-project",
        "Plain",
        realm
            .world
            .directory
            .path()
            .join("plain-repository")
            .to_str()
            .expect("a path"),
    )
    .await;
    assert_eq!(plain.status, 200, "{}", plain.body);
    let refused = Call::post(
        format!(
            "/v1/projects/{}/desk-adams/ad-hoc:open",
            plain.json()["project_id"].as_str().expect("a project")
        ),
        &serde_json::json!({"topic": "Kontor triage"}),
    )
    .signed_as(&realm.world, "admin")
    .with_key("desk-adam-ad-hoc-plain")
    .send(&realm.world)
    .await;
    assert_eq!(
        refused.json()["code"],
        "placement_blocked",
        "{}",
        refused.body
    );
}

#[tokio::test]
async fn an_epic_binds_its_ad_hoc_origin_after_fencing_it_and_launches_a_fresh_lsa_with_the_capsule()
 {
    let realm = desk_led_realm("desk-adam-bind").await;
    let (origin, ad_hoc_lsa, ad_hoc_native) =
        ad_hoc_live(&realm, "Desk planning", "desk-adam-bind-open").await;
    let ad_hoc_occupancy = hosted(&realm, ad_hoc_lsa).expect("the ad-hoc LSA is live");
    let before = realm.world.fake.calls().len();
    let applied = apply_epic(
        &realm,
        "Bound epic",
        "desk-adam-bind-apply",
        Some(serde_json::json!({"subject": origin})),
    )
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic")
        .to_owned();
    confirm_test_epic_identity(&realm.world, &realm.project, &epic);

    // The ad-hoc session was retired in the desk workspace and moved into its
    // seat's history, and the seat was retired, before the epic's subject was
    // written; the epic's subject names it as its origin and renders the
    // epic's own frozen name.
    let workspace = container(&realm, adam_desk(&realm).workspace_node_id);
    let placement = realm
        .world
        .fake
        .last_hosted_retire_placement(ad_hoc_lsa)
        .flatten()
        .expect("the ad-hoc session was retired in an exact placement");
    assert_eq!(placement.workspace_native_id, workspace.identity.native_id);
    assert_eq!(
        Some(placement.canonical_cwd.as_str()),
        workspace
            .canonical_cwd
            .as_ref()
            .map(kontor_core::id::ExternalName::as_str)
    );
    assert!(hosted(&realm, ad_hoc_lsa).is_none(), "no live occupancy");
    assert_eq!(
        realm
            .world
            .daemon
            .state()
            .with_store(|store| store.get_hosted_topology_seat_history(
                realm.project_id,
                ad_hoc_lsa,
                &ad_hoc_native
            ))
            .expect("the history reads")
            .expect("the ad-hoc session is in its seat's history")
            .native_identity,
        ad_hoc_occupancy.native_identity
    );
    assert_eq!(
        seat_row(&realm, ad_hoc_lsa).lifecycle,
        TopologyLifecycle::Retired
    );
    let source = realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_desk_adam_subject(realm.project_id, origin))
        .expect("the origin reads")
        .expect("the origin stands");
    assert_eq!(source.mini_project_id, None);
    assert_eq!(source.topic.as_str(), "Desk planning");
    let subject = subject_of(&realm, &epic);
    assert_eq!(subject.origin_topology_node_id, Some(origin));
    assert_eq!(subject.topic.as_str(), "Bound epic");
    assert_ne!(subject.topology_node_id, origin);
    assert_eq!(
        calls_since(&realm, before)
            .into_iter()
            .filter(|call| matches!(
                call,
                AdapterCall::LaunchHostedSeat(_) | AdapterCall::RetireHostedSeat(_)
            ))
            .collect::<Vec<_>>(),
        vec![AdapterCall::RetireHostedSeat(ad_hoc_lsa)],
        "binding fences the ad-hoc session and launches nothing"
    );
    let receipt = realm.world.daemon.state().with_store(|store| {
        store
            .get_receipt_by_key(&IdempotencyKey::parse("desk-adam-bind-apply").expect("a key"))
            .expect("the receipt reads")
            .expect("the apply is receipted")
    });
    assert!(receipt.intent.json().contains("\"desk_adam_origin\""));

    // The epic's LSA launches fresh, with its own seat and first-generation
    // credential, and the server-owned capsule naming where its work came from.
    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-bind-lsa").await;
    assert_ne!(lsa, ad_hoc_lsa);
    assert_ne!(native, ad_hoc_native, "the epic's LSA is a fresh native");
    assert_eq!(generation, 1);
    assert_desk_placed(&realm, lsa);
    assert_eq!(
        realm.world.fake.seat_title(&native).as_deref(),
        Some(title("LSA", "Bound epic").as_str())
    );
    let prompt = realm
        .world
        .fake
        .hosted_initial_prompt(lsa)
        .expect("the LSA launch prompt");
    let prompt = prompt.as_str();
    assert!(prompt.contains("Desk Adam binding handoff:"), "{prompt}");
    assert!(
        prompt.contains("\"continuation\":\"desk_adam_binding\""),
        "{prompt}"
    );
    assert!(prompt.contains(&origin.to_string()), "{prompt}");
    assert!(prompt.contains(&ad_hoc_lsa.to_string()), "{prompt}");
    assert!(prompt.contains("Desk planning"), "{prompt}");
    let planning = act_as_seat(&realm, &epic, lsa, 1).await;
    assert_eq!(planning.status, 200, "{}", planning.body);
    // The ad-hoc session's credential did not travel: it acts for nothing.
    let fenced = act_as_seat(&realm, &epic, ad_hoc_lsa, 1).await;
    assert_ne!(fenced.status, 200, "{}", fenced.body);

    // The same apply replays, and a fresh key naming the same origin
    // converges on the one placement.
    let replayed = apply_epic(
        &realm,
        "Bound epic",
        "desk-adam-bind-apply",
        Some(serde_json::json!({"subject": origin})),
    )
    .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    let converged = apply_epic(
        &realm,
        "Bound epic",
        "desk-adam-bind-again",
        Some(serde_json::json!({"subject": origin})),
    )
    .await;
    assert_eq!(converged.status, 200, "{}", converged.body);
    assert_eq!(
        subject_of(&realm, &epic).topology_node_id,
        subject.topology_node_id
    );

    // The bound ad-hoc topic seats and hosts no one again, routed or not.
    let before = realm.world.fake.calls().len();
    for (body, key) in [
        (
            serde_json::json!({"topic": "Desk planning"}),
            "desk-adam-bind-reopen",
        ),
        (
            serde_json::json!({"topic": "Desk planning", "model_route": claude_route()}),
            "desk-adam-bind-relaunch",
        ),
    ] {
        let reopened = open_ad_hoc(&realm, body, key).await;
        assert_eq!(
            reopened.json()["code"],
            "placement_blocked",
            "{}",
            reopened.body
        );
    }
    assert!(calls_since(&realm, before).is_empty());
    assert_eq!(seats_on(&realm, origin).len(), 1);
    assert!(hosted(&realm, ad_hoc_lsa).is_none());
}

/// An ad-hoc desk Adam opened with a route launches its LSA in the desk's own
/// native project: no epic, no container of its own, and one native that
/// every later open of the topic finds rather than relaunches.
#[tokio::test]
async fn an_ad_hoc_desk_adam_launches_its_lsa_in_the_desk_project_with_no_epic() {
    let realm = desk_led_realm("desk-adam-ad-hoc-live").await;
    let before = realm.world.fake.calls().len();
    let opened = open_ad_hoc(
        &realm,
        serde_json::json!({"topic": "Live triage", "model_route": claude_route()}),
        "desk-adam-ad-hoc-live-open",
    )
    .await;
    assert_eq!(opened.status, 200, "{}", opened.body);
    let subject = TopologyNodeId::parse(
        opened.json()["subject"]["topology_node_id"]
            .as_str()
            .expect("a node"),
    )
    .expect("a node id");
    let lsa = seat_id(
        opened.json()["lsa_seat_binding_id"]
            .as_str()
            .expect("a seat"),
    );
    let live = hosted(&realm, lsa).expect("the ad-hoc LSA is hosted");
    let native = live.native_identity.native_id.clone();
    assert_eq!(live.native_identity.generation, 1);
    assert_eq!(opened.json()["lsa_native_id"], native.as_str());
    assert_eq!(
        realm.world.fake.seat_title(&native).as_deref(),
        Some("LSA • ADHOC • Live triage")
    );
    assert_desk_placed(&realm, lsa);
    let prompt = realm
        .world
        .fake
        .hosted_initial_prompt(lsa)
        .expect("the ad-hoc launch prompt");
    assert!(prompt.as_str().contains("belongs to no epic"), "{prompt:?}");
    let calls = calls_since(&realm, before);
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, AdapterCall::LaunchHostedSeat(_)))
            .collect::<Vec<_>>(),
        vec![&AdapterCall::LaunchHostedSeat(lsa)],
        "{calls:?}"
    );
    assert!(
        calls.iter().all(|call| !matches!(
            call,
            AdapterCall::PrepareContainer(id) | AdapterCall::CreateNativeContainer(id)
                if *id == subject
        )),
        "{calls:?}"
    );
    assert!(
        realm
            .world
            .daemon
            .state()
            .with_store(|store| store.get_topology_node_container(realm.project_id, subject))
            .expect("the binding reads")
            .is_none(),
        "the subject borrows the desk workspace's container"
    );

    // A replay, another key with the same route, and a logical-only open all
    // find the one native; none launches another.
    let before = realm.world.fake.calls().len();
    let replayed = open_ad_hoc(
        &realm,
        serde_json::json!({"topic": "Live triage", "model_route": claude_route()}),
        "desk-adam-ad-hoc-live-open",
    )
    .await;
    assert_eq!(
        replayed.json()["receipt"]["applied"],
        "unchanged",
        "{}",
        replayed.body
    );
    for (body, key) in [
        (
            serde_json::json!({"topic": "Live triage", "model_route": claude_route()}),
            "desk-adam-ad-hoc-live-again",
        ),
        (
            serde_json::json!({"topic": "Live triage"}),
            "desk-adam-ad-hoc-live-logical",
        ),
    ] {
        let again = open_ad_hoc(&realm, body, key).await;
        assert_eq!(again.status, 200, "{}", again.body);
        assert_eq!(again.json()["lsa_seat_binding_id"], lsa.to_string());
        assert_eq!(again.json()["lsa_native_id"], native.as_str());
    }
    assert!(
        calls_since(&realm, before)
            .iter()
            .all(|call| !matches!(call, AdapterCall::LaunchHostedSeat(_))),
        "{:?}",
        calls_since(&realm, before)
    );

    // A live ad-hoc native is never re-routed: another route refuses before
    // any effect, and the session stands.
    let before = realm.world.fake.calls().len();
    let rerouted = open_ad_hoc(
        &realm,
        serde_json::json!({"topic": "Live triage", "model_route": codex_route()}),
        "desk-adam-ad-hoc-live-rerouted",
    )
    .await;
    assert_eq!(
        rerouted.json()["code"],
        "placement_blocked",
        "{}",
        rerouted.body
    );
    assert!(!receipt_recorded(&realm, "desk-adam-ad-hoc-live-rerouted"));
    assert!(calls_since(&realm, before).is_empty());
    let still = hosted(&realm, lsa).expect("the ad-hoc LSA stands");
    assert_eq!(still.native_identity, live.native_identity);
    assert_eq!(seats_on(&realm, subject).len(), 1);
}

/// A binding whose ad-hoc retire lost its acknowledgement leaves everything
/// the fence had not finished as it was, and the same apply finishes it.
#[tokio::test]
async fn a_binding_whose_fence_lost_its_answer_resumes_and_finishes_the_fence() {
    let realm = desk_led_realm("desk-adam-bind-resume").await;
    let (origin, ad_hoc_lsa, ad_hoc_native) =
        ad_hoc_live(&realm, "Resumed planning", "desk-adam-bind-resume-open").await;
    realm.world.fake.lose_next_hosted_retire_ack(ad_hoc_lsa);
    let before = realm.world.fake.calls().len();
    let lost = apply_epic(
        &realm,
        "Resumed epic",
        "desk-adam-bind-resume-apply",
        Some(serde_json::json!({"subject": origin})),
    )
    .await;
    assert_eq!(lost.json()["code"], "unavailable", "{}", lost.body);
    assert!(!receipt_recorded(&realm, "desk-adam-bind-resume-apply"));
    // Only a session the runtime proves absent counts as gone: the occupancy,
    // the seat and the topic's freedom all stand, and no epic subject exists.
    assert_eq!(
        hosted(&realm, ad_hoc_lsa)
            .expect("the occupancy stands")
            .native_identity
            .native_id,
        ad_hoc_native
    );
    assert_eq!(
        seat_row(&realm, ad_hoc_lsa).lifecycle,
        TopologyLifecycle::Active
    );
    assert!(
        realm
            .world
            .daemon
            .state()
            .with_store(|store| store.get_desk_adam_subject_bound_from(realm.project_id, origin))
            .expect("the binding reads")
            .is_none(),
        "the topic is bound to no epic yet"
    );

    let resumed = apply_epic(
        &realm,
        "Resumed epic",
        "desk-adam-bind-resume-apply",
        Some(serde_json::json!({"subject": origin})),
    )
    .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    let epic = resumed.json()["epic_id"]
        .as_str()
        .expect("an epic")
        .to_owned();
    assert_eq!(
        calls_since(&realm, before)
            .into_iter()
            .filter(|call| matches!(
                call,
                AdapterCall::LaunchHostedSeat(_) | AdapterCall::RetireHostedSeat(_)
            ))
            .collect::<Vec<_>>(),
        vec![AdapterCall::RetireHostedSeat(ad_hoc_lsa)],
        "the session is retired natively once, and nothing launches"
    );
    let workspace = container(&realm, adam_desk(&realm).workspace_node_id);
    assert_eq!(
        realm
            .world
            .fake
            .last_hosted_retire_placement(ad_hoc_lsa)
            .flatten()
            .expect("the retry asked in an exact placement")
            .workspace_native_id,
        workspace.identity.native_id
    );
    assert!(hosted(&realm, ad_hoc_lsa).is_none());
    assert!(
        realm
            .world
            .daemon
            .state()
            .with_store(|store| store.get_hosted_topology_seat_history(
                realm.project_id,
                ad_hoc_lsa,
                &ad_hoc_native
            ))
            .expect("the history reads")
            .is_some(),
        "the ad-hoc session is in its seat's history"
    );
    assert_eq!(
        seat_row(&realm, ad_hoc_lsa).lifecycle,
        TopologyLifecycle::Retired
    );
    assert_eq!(
        subject_of(&realm, &epic).origin_topology_node_id,
        Some(origin)
    );
}

#[tokio::test]
async fn a_desk_adam_origin_is_checked_whole_before_any_graph_write() {
    let realm = desk_led_realm("desk-adam-origin").await;
    let (origin, _) = ad_hoc(&realm, "First topic", "desk-adam-origin-first").await;
    let placed = apply_epic(
        &realm,
        "Placed epic",
        "desk-adam-origin-placed",
        Some(serde_json::json!({"subject": origin})),
    )
    .await;
    assert_eq!(placed.status, 200, "{}", placed.body);
    let (other, other_lsa) = ad_hoc(&realm, "Second topic", "desk-adam-origin-second").await;

    // Each refusal leaves the epic graph, the origin's seat and the receipt
    // ledger exactly as they were.
    let unplaced = apply_epic(
        &realm,
        "Unplaced epic",
        "desk-adam-origin-unplaced-plain",
        None,
    )
    .await;
    assert_eq!(unplaced.status, 200, "{}", unplaced.body);
    let mut cases = vec![
        // Bound to another epic already.
        (
            "conflict",
            "Thief epic",
            serde_json::json!({"subject": origin}),
            "placement_blocked",
        ),
        // A placed epic cannot change where its leadership came from.
        (
            "different",
            "Placed epic",
            serde_json::json!({"subject": other}),
            "placement_blocked",
        ),
        // An epic placed without an origin cannot acquire one.
        (
            "late",
            "Unplaced epic",
            serde_json::json!({"subject": other}),
            "placement_blocked",
        ),
        // Keeping the ad-hoc native waits on a pending decision.
        (
            "keep",
            "Keeping epic",
            serde_json::json!({"subject": other, "keep_native": true}),
            "unsupported_capability",
        ),
        // A node that is no ad-hoc subject at all.
        (
            "stranger",
            "Stranger epic",
            serde_json::json!({"subject": TopologyNodeId::generate()}),
            "placement_blocked",
        ),
    ];
    // An ad-hoc subject on another desk than the one the epic is led from.
    let ensured = Call::post(
        format!("/v1/projects/{}/topology:ensure", realm.project),
        &serde_json::json!({
            "target": {"scope": "desk", "desk_key": "pr-review"},
            "expected_revision": current_project_revision(&realm.world, &realm.project).await,
        }),
    )
    .signed_as(&realm.world, "operator")
    .with_key("desk-adam-origin-pr-review")
    .send(&realm.world)
    .await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);
    let elsewhere = TopologyNodeId::generate();
    let pinned =
        subject_of(&realm, placed.json()["epic_id"].as_str().expect("an epic")).team_definition;
    realm
        .world
        .daemon
        .state()
        .with_store(|store| {
            store.create_desk_adam_subject(&kontor_core::repository::StoredDeskAdamSubject {
                project_id: realm.project_id,
                topology_node_id: elsewhere,
                desk_key: kontor_core::id::DeskKey::parse("pr-review").expect("a desk key"),
                mini_project_id: None,
                topic: kontor_core::id::ExternalName::parse("Review topic").expect("a topic"),
                origin_topology_node_id: None,
                team_definition: pinned,
                created_at: kontor_api::now(),
            })
        })
        .expect("the other desk's ad-hoc subject is recorded");
    cases.push((
        "cross-desk",
        "Crossing epic",
        serde_json::json!({"subject": elsewhere}),
        "placement_blocked",
    ));

    for (case, name, origin_body, code) in cases {
        let key = format!("desk-adam-origin-{case}");
        let existed = epic_named(&realm, name);
        let before = realm.world.fake.calls().len();
        // Preview judges the origin exactly as apply does.
        let previewed = preview_epic(&realm, name, origin_body.clone()).await;
        assert_eq!(previewed.json()["code"], code, "{case}: {}", previewed.body);
        let refused = apply_epic(&realm, name, &key, Some(origin_body)).await;
        assert_eq!(refused.json()["code"], code, "{case}: {}", refused.body);
        assert_eq!(
            epic_named(&realm, name),
            existed,
            "{case}: no graph was written"
        );
        assert!(!receipt_recorded(&realm, &key), "{case}");
        assert!(calls_since(&realm, before).is_empty(), "{case}");
        assert_eq!(
            seat_row(&realm, other_lsa).lifecycle,
            TopologyLifecycle::Active,
            "{case}: the second topic's seat was not fenced"
        );
    }

    // A preview that would bind fences nothing: the live ad-hoc session and
    // its seat stand, and no epic or subject is written.
    let (live, live_lsa, live_native) =
        ad_hoc_live(&realm, "Live topic", "desk-adam-origin-live").await;
    let before = realm.world.fake.calls().len();
    let previewed = preview_epic(
        &realm,
        "Previewed epic",
        serde_json::json!({"subject": live}),
    )
    .await;
    assert_eq!(previewed.status, 200, "{}", previewed.body);
    assert!(calls_since(&realm, before).is_empty());
    assert!(!epic_named(&realm, "Previewed epic"));
    assert_eq!(
        hosted(&realm, live_lsa)
            .expect("the ad-hoc session stands")
            .native_identity
            .native_id,
        live_native
    );
    assert_eq!(
        seat_row(&realm, live_lsa).lifecycle,
        TopologyLifecycle::Active
    );

    // A project that leads from no desk accepts no origin at all.
    let plain = ensure_project(
        &realm.world,
        "desk-adam-origin-plain-project",
        "Plain",
        realm
            .world
            .directory
            .path()
            .join("plain-origin")
            .to_str()
            .expect("a path"),
    )
    .await;
    assert_eq!(plain.status, 200, "{}", plain.body);
    let plain_project = plain.json()["project_id"]
        .as_str()
        .expect("a project")
        .to_owned();
    let mut body = epic_body(
        current_project_revision(&realm.world, &plain_project).await,
        "Plain epic",
        &realm.category,
        serde_json::json!([{"title": "Plain work"}]),
    );
    body["desk_adam_origin"] = serde_json::json!({"subject": other});
    let refused = Call::post(format!("/v1/projects/{plain_project}/epics:apply"), &body)
        .signed_as(&realm.world, "admin")
        .with_key("desk-adam-origin-plain-apply")
        .send(&realm.world)
        .await;
    assert_eq!(
        refused.json()["code"],
        "placement_blocked",
        "{}",
        refused.body
    );
}

#[tokio::test]
async fn a_desk_adam_title_freezes_the_epic_name_and_refuses_a_forged_segment() {
    let realm = desk_led_realm("desk-adam-naming").await;
    for (name, key) in [
        ("Desk • forged", "desk-adam-naming-bullet"),
        ("Desk · forged", "desk-adam-naming-dot"),
    ] {
        let refused = apply_epic(&realm, name, key, None).await;
        assert_eq!(
            refused.json()["code"],
            "invalid_request",
            "{}",
            refused.body
        );
        assert!(!epic_named(&realm, name), "no graph for {name}");
    }
    for (name, key) in [
        (String::new(), "desk-adam-naming-empty"),
        ("x".repeat(513), "desk-adam-naming-long"),
    ] {
        // The fixture derives task worktrees from the name, so the body is built
        // under a placeholder and only the posted name is the refused one.
        let mut body = epic_body(
            current_project_revision(&realm.world, &realm.project).await,
            "Desk naming refused",
            &realm.category,
            serde_json::json!([{"title": "refused work"}]),
        );
        body["name"] = serde_json::json!(name);
        let refused = Call::post(format!("/v1/projects/{}/epics:apply", realm.project), &body)
            .signed_as(&realm.world, "admin")
            .with_key(key)
            .send(&realm.world)
            .await;
        assert_eq!(refused.status, 400, "{}", refused.body);
        assert!(!receipt_recorded(&realm, key));
        assert!(!epic_named(&realm, &name), "no graph for the refused name");
    }

    let epic = applied_epic(&realm, "Frozen name", "desk-adam-naming-epic").await;
    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-naming-lsa").await;
    assert_eq!(
        realm.world.fake.seat_title(&native).as_deref(),
        Some(title("LSA", "Frozen name").as_str())
    );

    // Whatever later renames the epic's row, the subject keeps the name it
    // froze, and the next seat it launches still renders it.
    let connection = rusqlite::Connection::open(
        realm
            .world
            .directory
            .path()
            .join(kontor_daemon::DATABASE_FILE),
    )
    .expect("the Realm database opens");
    let renamed = connection
        .execute(
            "UPDATE mini_projects SET name = 'Renamed epic' WHERE id = ?1",
            [&epic],
        )
        .expect("the fixture rename applies");
    assert_eq!(renamed, 1);
    drop(connection);
    let handed = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm_of(&realm, &epic),
        codex_route(),
        "desk-adam-naming-handoff",
    )
    .await;
    assert_eq!(handed.status, 200, "{}", handed.body);
    let tpm = hosted(&realm, tpm_of(&realm, &epic)).expect("the TPM is hosted");
    assert_eq!(
        realm
            .world
            .fake
            .seat_title(&tpm.native_identity.native_id)
            .as_deref(),
        Some(title("TPM", "Frozen name").as_str())
    );
    assert_eq!(subject_of(&realm, &epic).topic.as_str(), "Frozen name");
}

/// Restart the daemon over the same state root with the fake adapter's own
/// ledger rebuilt, and hand back the same realm.
async fn restarted(realm: DeskLed) -> DeskLed {
    let DeskLed {
        world,
        project,
        project_id,
        category,
    } = realm;
    let World {
        directory,
        daemon,
        router,
        fake,
        project: world_project,
        task,
        team_run,
    } = world;
    daemon.state().signals().stop();
    drop(router);
    drop(daemon);
    fake.rebuild_adapter_state();
    let daemon = Daemon::start(
        DaemonConfig::at(directory.path()).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");
    assert_eq!(daemon.reconcile().await, BarrierState::Open);
    let router = daemon.router();
    DeskLed {
        world: World {
            directory,
            daemon,
            router,
            fake,
            project: world_project,
            task,
            team_run,
        },
        project,
        project_id,
        category,
    }
}

/// The one node of `kind` an epic owns.
fn epic_node(
    realm: &DeskLed,
    epic: &str,
    kind: &str,
) -> Option<kontor_core::state::SessionTopologyNode> {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.list_topology_nodes(realm.project_id, Some(epic_id(epic))))
        .expect("the epic's nodes read")
        .into_iter()
        .find(|node| node.kind.as_str() == kind)
}

fn container(realm: &DeskLed, id: TopologyNodeId) -> kontor_core::state::NativeContainerBinding {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_topology_node_container(realm.project_id, id))
        .expect("the binding reads")
        .expect("the node holds a native container")
}

/// A desk-led epic delivers a task end to end with its leadership on the desk
/// and no ECP. Its task workspace hangs off the epic's own ESW, never off the
/// desk's workspace, also when the ESW was placed before an adapter restart.
/// Closing it releases only its own owner seat: the desk, its workspace and
/// the neighbouring epic Adam stay exactly as they were.
#[tokio::test]
async fn a_desk_led_epic_delivers_and_closes_without_an_ecp_or_touching_the_desk() {
    let realm = desk_led_realm("desk-adam-delivery").await;
    realm.world.script(HISTORY_LIVE);
    let account = Call::post(
        format!(
            "/v1/projects/{}/provider-account-profiles:ensure",
            realm.project
        ),
        &serde_json::json!({
            "label": "Desk delivery", "harness": "fake.runtime",
            "credential_alias": "desk-delivery", "enabled": true
        }),
    )
    .signed_as(&realm.world, "admin")
    .with_key("desk-adam-delivery-account")
    .send(&realm.world)
    .await;
    assert_eq!(account.status, 200, "{}", account.body);

    // A neighbour whose planning LSA stays live in the same desk workspace.
    let beta = applied_epic(&realm, "Neighbour epic", "desk-adam-delivery-beta").await;
    let (beta_lsa, beta_native, _) =
        planning_lsa(&realm, &beta, "desk-adam-delivery-beta-lsa").await;
    assert_desk_placed(&realm, beta_lsa);
    let beta_subject = subject_of(&realm, &beta).topology_node_id;
    let beta_seats = seats_on(&realm, beta_subject);

    let applied = apply_epic(&realm, "Delivered epic", "desk-adam-delivery-alpha", None).await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let alpha = applied.json()["epic_id"]
        .as_str()
        .expect("an epic")
        .to_owned();
    let task = applied.json()["tasks"][0]["task_id"]
        .as_str()
        .expect("a task")
        .to_owned();
    confirm_test_epic_identity(&realm.world, &realm.project, &alpha);
    let alpha_subject = subject_of(&realm, &alpha).topology_node_id;
    let workspace = node(&realm, alpha_subject).parent_id.expect("a parent");
    assert_eq!(node(&realm, beta_subject).parent_id, Some(workspace));
    let desk = node(&realm, workspace).parent_id.expect("the desk");
    let desk_bindings = (container(&realm, desk), container(&realm, workspace));

    // The epic's own ESW is placed before the adapter forgets everything.
    let placed = Call::post(
        format!("/v1/projects/{}/topology:materialize", realm.project),
        &serde_json::json!({
            "target": {"scope": "epic", "epic_id": alpha},
            "expected_revision": current_project_revision(&realm.world, &realm.project).await,
        }),
    )
    .signed_as(&realm.world, "operator")
    .with_key("desk-adam-delivery-esw")
    .send(&realm.world)
    .await;
    assert_eq!(placed.status, 200, "{}", placed.body);
    let esw = epic_node(&realm, &alpha, "ESW").expect("the epic's ESW");
    let esw_native = container(&realm, esw.id).identity;
    assert_ne!(esw_native.native_id, desk_bindings.0.identity.native_id);
    assert_ne!(esw_native.native_id, desk_bindings.1.identity.native_id);

    let realm = restarted(realm).await;
    let seed = Bootstrapped {
        project: realm.project.clone(),
        project_revision: current_project_revision(&realm.world, &realm.project).await,
        epic: alpha.clone(),
        task,
        task_revision: applied.json()["tasks"][0]["revision"]
            .as_u64()
            .expect("a task revision"),
        account: account.json()["account_profile_id"]
            .as_str()
            .expect("an account")
            .to_owned(),
    };
    let before = realm.world.fake.calls().len();
    let runs = seat_existing(&realm.world, &seed, "desk-adam-delivery").await;
    let tsw = epic_node(&realm, &alpha, "TSW").expect("the task's TSW");
    assert_eq!(tsw.parent_id, Some(esw.id));
    let tsw_parent = container(&realm, tsw.id)
        .readback
        .expect("a complete readback")
        .native_parent
        .expect("a native child has a parent");
    assert_eq!(
        tsw_parent, esw_native,
        "the TSW lives in the epic's own ESW project"
    );
    assert_eq!(container(&realm, esw.id).identity, esw_native);
    assert!(
        calls_since(&realm, before)
            .iter()
            .all(|call| !matches!(call, AdapterCall::CreateNativeContainer(id) if *id == esw.id)),
        "the restarted adapter resolves the ESW it already placed"
    );
    assert!(
        epic_node(&realm, &alpha, "ECP").is_none(),
        "a desk-led epic has no ECP"
    );

    // Admission's owner is the subject's own TPM seat on the desk.
    let owner = tpm_of(&realm, &alpha);
    assert_eq!(subject_node_of_seat(&realm, owner), alpha_subject);

    settle_every_seat(&realm.world, &seed, &runs, "desk-adam-delivery").await;
    let completed =
        discharge_the_profile_and_complete(&realm.world, &seed, "desk-adam-delivery").await;
    assert_eq!(completed.status, 200, "{}", completed.body);
    let started = Call::post(
        format!(
            "/v1/projects/{}/epics/{alpha}/completion:advance",
            realm.project
        ),
        &serde_json::json!({"expected_revision": 1}),
    )
    .signed_as(&realm.world, "operator")
    .with_key("desk-adam-delivery-completion")
    .send(&realm.world)
    .await;
    assert_eq!(started.status, 200, "{}", started.body);
    assert_eq!(started.json()["state"]["phase"]["phase"], "integration");
    assert_eq!(completion_tpm(&realm, &alpha), owner.to_string());
    assert_ne!(
        owner,
        tpm_of(&realm, &beta),
        "the neighbour's TPM is not the owner"
    );
    let closed = Call::post(
        format!("/v1/projects/{}/epics/{alpha}/lifecycle", realm.project),
        &serde_json::json!({
            "action": "close_epic",
            "expected_revision": epic_revision(&realm, &alpha),
            "reason": "Delivered from the desk"
        }),
    )
    .signed_as(&realm.world, "operator")
    .with_key("desk-adam-delivery-close")
    .send(&realm.world)
    .await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(closed.json()["state"], "closed");

    // Only the closed epic's owner was released.
    assert!(seat_row(&realm, owner).released_at.is_some());
    assert_eq!(
        seats_on(&realm, beta_subject),
        beta_seats,
        "the neighbour's seats"
    );
    assert_eq!(
        hosted(&realm, beta_lsa)
            .expect("the neighbour's LSA")
            .native_identity
            .native_id,
        beta_native
    );
    let beta_acts = act_as_seat(&realm, &beta, beta_lsa, 1).await;
    assert_eq!(beta_acts.status, 200, "{}", beta_acts.body);
    assert_eq!(
        (container(&realm, desk), container(&realm, workspace)),
        desk_bindings
    );
    for id in [desk, workspace, beta_subject] {
        assert_eq!(
            node(&realm, id).lifecycle,
            kontor_core::state::TopologyLifecycle::Active,
            "{id} stays active"
        );
    }
    assert!(epic_node(&realm, &alpha, "ECP").is_none());
}

/// Launch ruling (a): an epic's desk Adam is placed in the desk's own native
/// project and names no epic, while its subject, and the authority its seats
/// derive from it, stay the epic's. Nothing of the desk is recorded under the
/// epic, across a restart, the handoff and a route correction.
#[tokio::test]
async fn an_epic_desk_adam_is_placed_in_the_desk_project_while_its_subject_stays_the_epics() {
    let realm = desk_led_realm("desk-adam-scope").await;
    let epic = applied_epic(&realm, "Scoped epic", "desk-adam-scope-epic").await;
    let placed = Call::post(
        format!("/v1/projects/{}/topology:materialize", realm.project),
        &serde_json::json!({
            "target": {"scope": "epic", "epic_id": epic},
            "expected_revision": current_project_revision(&realm.world, &realm.project).await,
        }),
    )
    .signed_as(&realm.world, "operator")
    .with_key("desk-adam-scope-esw")
    .send(&realm.world)
    .await;
    assert_eq!(placed.status, 200, "{}", placed.body);
    let esw = epic_node(&realm, &epic, "ESW").expect("the epic's ESW");
    let esw_binding = container(&realm, esw.id);

    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-scope-lsa").await;
    assert_desk_placed(&realm, lsa);
    let desk = adam_desk(&realm);
    let desk_natives = [
        container(&realm, desk.topology_node_id).identity,
        container(&realm, desk.workspace_node_id).identity,
    ];
    assert!(!desk_natives.contains(&esw_binding.identity));
    assert_eq!(
        realm.world.fake.seat_title(&native).as_deref(),
        Some(title("LSA", "Scoped epic").as_str())
    );
    let subject = subject_of(&realm, &epic).topology_node_id;
    assert_eq!(subject_node_of_seat(&realm, lsa), subject);
    assert_eq!(node(&realm, subject).mini_project_id, Some(epic_id(&epic)));
    let acts = act_as_seat(&realm, &epic, lsa, 1).await;
    assert_eq!(acts.status, 200, "{}", acts.body);

    // The epic's own bindings, read by node: the subject has none, the ESW's
    // is unchanged, and none of them is the desk's or its workspace's.
    let epic_ledger = |realm: &DeskLed| {
        realm.world.daemon.state().with_store(|store| {
            let nodes = store
                .list_topology_nodes(realm.project_id, Some(epic_id(&epic)))
                .expect("the epic's nodes read");
            nodes
                .iter()
                .filter_map(|node| {
                    store
                        .get_topology_node_container(realm.project_id, node.id)
                        .expect("the binding reads")
                        .map(|binding| (node.id, binding))
                })
                .collect::<Vec<_>>()
        })
    };
    let assert_nothing_of_the_desk_under_the_epic = |realm: &DeskLed| {
        let ledger = epic_ledger(realm);
        assert!(
            ledger
                .iter()
                .all(|(id, binding)| *id != subject && !desk_natives.contains(&binding.identity)),
            "{ledger:?}"
        );
        assert_eq!(container(realm, esw.id), esw_binding);
    };
    assert_nothing_of_the_desk_under_the_epic(&realm);

    // After a restart the handoff fences the LSA in the desk workspace and
    // places the TPM in the desk project too.
    let realm = restarted(realm).await;
    let tpm = tpm_of(&realm, &epic);
    let handed = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        codex_route(),
        "desk-adam-scope-handoff",
    )
    .await;
    assert_eq!(handed.status, 200, "{}", handed.body);
    assert_desk_placed(&realm, tpm);
    assert_eq!(subject_node_of_seat(&realm, tpm), subject);
    let coordinating = act_as_seat(&realm, &epic, tpm, 1).await;
    assert_eq!(coordinating.status, 200, "{}", coordinating.body);
    assert_nothing_of_the_desk_under_the_epic(&realm);

    // A route correction relaunches the TPM in the same desk placement, on
    // the same seat of the same subject, at its next occupancy.
    let predecessor = hosted(&realm, tpm).expect("the TPM is hosted");
    let correction = serde_json::json!({
        "expected_revision": epic_revision(&realm, &epic),
        "seat_binding_id": tpm,
        "expected_native_id": predecessor.native_identity.native_id,
        "expected_generation": predecessor.native_identity.generation,
        "desired_model_route": claude_route(),
    });
    let preview = Call::post(
        format!(
            "/v1/projects/{}/epics/{epic}/core-team/routes:preview",
            realm.project
        ),
        &correction,
    )
    .signed_as(&realm.world, "admin")
    .send(&realm.world)
    .await;
    assert_eq!(preview.status, 200, "{}", preview.body);
    let mut apply = correction.clone();
    apply["preview_hash"] = preview.json()["preview_hash"].clone();
    let routed = Call::post(
        format!(
            "/v1/projects/{}/epics/{epic}/core-team/routes:apply",
            realm.project
        ),
        &apply,
    )
    .signed_as(&realm.world, "admin")
    .with_key("desk-adam-scope-route")
    .send(&realm.world)
    .await;
    assert_eq!(routed.status, 200, "{}", routed.body);
    let workspace = container(&realm, desk.workspace_node_id);
    let retired = realm
        .world
        .fake
        .last_hosted_retire_placement(tpm)
        .flatten()
        .expect("the predecessor was retired in an exact placement");
    assert_eq!(retired.workspace_native_id, workspace.identity.native_id);
    assert_eq!(
        Some(retired.canonical_cwd.as_str()),
        workspace
            .canonical_cwd
            .as_ref()
            .map(kontor_core::id::ExternalName::as_str)
    );
    let successor = hosted(&realm, tpm).expect("the TPM is hosted again");
    assert_ne!(
        successor.native_identity.native_id,
        predecessor.native_identity.native_id
    );
    assert_eq!(
        realm
            .world
            .daemon
            .state()
            .with_store(
                |store| store.hosted_topology_seat_occupancy_generation(realm.project_id, tpm)
            )
            .expect("the occupancy reads"),
        Some(2),
        "the seat's second occupancy"
    );
    let corrected = act_as_seat(&realm, &epic, tpm, 2).await;
    assert_eq!(corrected.status, 200, "{}", corrected.body);
    assert_eq!(
        routed.json()["successor_native_id"],
        successor.native_identity.native_id.as_str()
    );
    assert_desk_placed(&realm, tpm);
    assert_eq!(subject_node_of_seat(&realm, tpm), subject);
    assert_eq!(
        realm
            .world
            .fake
            .seat_title(&successor.native_identity.native_id)
            .as_deref(),
        Some(title("TPM", "Scoped epic").as_str())
    );
    assert_nothing_of_the_desk_under_the_epic(&realm);
}

/// An epic under a definition that declares no desk leadership keeps the
/// legacy path whole: its leadership lives on its own ECP, no subject is
/// written, its apply intent carries no origin, and the desk Adam routes
/// refuse before any effect.
#[tokio::test]
async fn a_legacy_epic_keeps_its_ecp_and_refuses_the_desk_adam_routes() {
    let realm = realm_with("desk-adam-legacy", false).await;
    let epic = applied_epic(&realm, "Legacy epic", "desk-adam-legacy-epic").await;
    let intent = realm
        .world
        .daemon
        .state()
        .with_store(|store| {
            store
                .get_receipt_by_key(&IdempotencyKey::parse("desk-adam-legacy-epic").expect("a key"))
        })
        .expect("the receipt reads")
        .expect("the apply was recorded")
        .intent;
    assert!(
        !intent.json().contains("desk_adam_origin"),
        "{}",
        intent.json()
    );

    let (lsa, native, generation) = planning_lsa(&realm, &epic, "desk-adam-legacy-lsa").await;
    // The historical ECP launch keeps its epic placement.
    let scope = realm
        .world
        .fake
        .hosted_launch_scope(lsa)
        .expect("the LSA was launched");
    assert!(
        scope
            .epic()
            .is_some_and(|scope| scope.is_epic(epic_id(&epic))),
        "{scope:?}"
    );
    let control = node(&realm, subject_node_of_seat(&realm, lsa));
    assert_eq!(control.kind.as_str(), "ECP");
    assert_eq!(control.mini_project_id, Some(epic_id(&epic)));
    assert!(
        realm
            .world
            .daemon
            .state()
            .with_store(|store| store.get_epic_desk_adam_subject(realm.project_id, epic_id(&epic)))
            .expect("the subject reads")
            .is_none(),
        "a legacy epic has no desk Adam subject"
    );
    let tpm = seats_on(&realm, control.id)
        .into_iter()
        .find(|seat| seat.role.role_code.as_str() == "TPM" && seat.is_non_terminal())
        .expect("the ECP's TPM seat")
        .id;

    let before = realm.world.fake.calls().len();
    let handed = hand_off(
        &realm,
        &epic,
        (lsa, &native, generation),
        tpm,
        codex_route(),
        "desk-adam-legacy-handoff",
    )
    .await;
    assert_eq!(
        handed.json()["code"],
        "placement_blocked",
        "{}",
        handed.body
    );
    assert!(!receipt_recorded(&realm, "desk-adam-legacy-handoff"));
    let opened = open_ad_hoc(
        &realm,
        serde_json::json!({"topic": "Legacy topic"}),
        "desk-adam-legacy-ad-hoc",
    )
    .await;
    assert_eq!(
        opened.json()["code"],
        "placement_blocked",
        "{}",
        opened.body
    );
    assert!(!receipt_recorded(&realm, "desk-adam-legacy-ad-hoc"));
    assert!(calls_since(&realm, before).is_empty());
    let live = hosted(&realm, lsa).expect("the LSA is still hosted");
    assert_eq!(
        (
            live.native_identity.native_id,
            live.native_identity.generation
        ),
        (native, generation)
    );
}
