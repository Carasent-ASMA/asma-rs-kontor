//! PR reviews on a PR desk through the supported daemon operations
//! (ASMA-8453).
//!
//! A Team Definition that declares `pr_review` places one review per pull
//! request on its PR desk: an Adam tab in the desk's workspace and a review
//! room below the desk. The definition is published and selected through the
//! supported Admin surfaces. Every realm is disposable, on the fake runtime,
//! in the one project the ASMA forge mapping governs, so the confirmed Jira
//! key of an epic whose task owns `_tools/asma-rs-kontor` authorizes the root
//! repository and `Carasent-ASMA/asma-rs-kontor`, and nothing else.

use super::*;
use kontor_core::id::DeskKey;
use kontor_core::pull_request::{ForgeRepository, PullRequestRef};
use kontor_core::repository::{StoredDesk, StoredPrReviewSubject};
use kontor_core::spec::TeamDefinitionSpec;
use kontor_runtime::durable_seat::HostedSeatScope;

#[path = "qualification.rs"]
mod qualification;

const ROOT_REPOSITORY: &str = "Carasent-ASMA/asma-modules";
const MODULE_REPOSITORY: &str = "Carasent-ASMA/asma-rs-kontor";

/// The desk, desk workspace, PR Adam and review room kinds.
const REVIEW_KINDS: &str = r#"[
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
  {"kind": "PRA", "allowed_parents": ["DWS"], "cardinality": {"minimum": 0},
   "projection_capabilities": ["parent_session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "PR Adam", "meaning": "One pull request's Adam.",
                 "category": "session_topology", "lifecycle": "current"}},
  {"kind": "RSW", "allowed_parents": ["DESK"], "cardinality": {"minimum": 0},
   "projection_capabilities": ["native_child", "session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "Review Room", "meaning": "One pull request's room.",
                 "category": "session_topology", "lifecycle": "current"}}
]"#;

/// The containers: the desk and its workspace as ASMA-8450 places them, the
/// Adam titled by its role, key and pull request, and the room named by its
/// key and pull request with its three labelled seats.
const REVIEW_CONTAINERS: &str = r#"[
  {"kind": "DESK", "parent": null, "prefix": "DESK", "projection_capabilities": ["native_root"],
   "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"},
                                  {"kind": "token", "value": "DESK_NAME"}]}},
  {"kind": "DWS", "parent": "DESK", "prefix": "ADAM",
   "projection_capabilities": ["native_child", "session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"}]}},
  {"kind": "PRA", "parent": "DWS", "prefix": "PRA",
   "projection_capabilities": ["parent_session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "ROLE_CODE"},
                                       {"kind": "token", "value": "SCOPE_JIRA_KEY"},
                                       {"kind": "token", "value": "PULL_REQUEST"}]},
   "slots": [{"slot_id": "tpm", "role_code": "TPM", "capability_profile": "epic-coordination"}]},
  {"kind": "RSW", "parent": "DESK", "prefix": "RSW",
   "projection_capabilities": ["native_child", "session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"},
                                  {"kind": "token", "value": "SCOPE_JIRA_KEY"},
                                  {"kind": "token", "value": "PULL_REQUEST"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "SLOT_DISPLAY_NAME"}]},
   "slots": [{"slot_id": "rev-a", "display_name": "REV A",
              "capability_profile": "independent-reviewer"},
             {"slot_id": "rev-b", "display_name": "REV B",
              "capability_profile": "independent-reviewer"},
             {"slot_id": "judge", "display_name": "JUDGE",
              "capability_profile": "committee-judge"}]}
]"#;

const DESKS: &str = r#"[
  {"desk_key": "adam", "display_name": "ADAM", "kind": "DESK", "workspace_kind": "DWS"},
  {"desk_key": "pr-review", "display_name": "PR REVIEW", "kind": "DESK", "workspace_kind": "DWS"}
]"#;

const PR_REVIEW: &str = r#"{"desk_key": "pr-review", "adam_kind": "PRA", "room_kind": "RSW",
  "room_roles": [{"slot_id": "rev-a", "role_code": "AUD"},
                 {"slot_id": "rev-b", "role_code": "AUD"},
                 {"slot_id": "judge", "role_code": "AUD"}]}"#;

const ROOM_SLOTS: [(&str, &str); 3] = [("rev-a", "REV A"), ("rev-b", "REV B"), ("judge", "JUDGE")];

/// One governed realm with a confirmed epic whose task owns the Kontor
/// module, under a definition that does (or does not) declare `pr_review`.
struct Review {
    world: World,
    project: String,
    project_id: ProjectId,
    epic: String,
    epic_key: String,
    task_key: String,
}

async fn review_realm(slug: &str, review: Option<&str>) -> Review {
    let world = World::open_empty_with_a_plane().await;
    assert_eq!(world.daemon.reconcile().await, BarrierState::Open);
    let project = ensure_governed_project(&world, &format!("/tmp/kontor-{slug}"));
    select_review_definition(&world, &project, review).await;
    let category = first_category(&world).await;
    let name = format!("{slug} epic");
    let applied = Call::post(
        format!("/v1/projects/{project}/epics:apply"),
        &epic_body(
            current_project_revision(&world, &project).await,
            &name,
            &category,
            serde_json::json!([{
                "title": "Review it",
                "module": "_tools/asma-rs-kontor",
                "worktree": "/w/review-it"
            }]),
        ),
    )
    .signed_as(&world, "admin")
    .with_key(format!("{slug}-epic"))
    .send(&world)
    .await;
    assert_eq!(applied.status, 200, "{}", applied.body);
    let epic = applied.json()["epic_id"]
        .as_str()
        .expect("an epic id")
        .to_owned();
    confirm_test_epic_identity(&world, &project, &epic);
    activate_publication_policy(&world, &project);
    Review {
        project_id: ProjectId::parse(&project).expect("a project id"),
        world,
        project,
        epic,
        epic_key: test_jira_key(&format!("{name}-epic")),
        task_key: test_jira_key(&format!("{name}-task-0")),
    }
}

/// Publish a successor of the project's topology and Team Definition with
/// the review kinds, and select both, before any session node exists.
async fn select_review_definition(world: &World, project: &str, review: Option<&str>) {
    select_qualification_definition(world, project, review, false).await;
}

/// Reuse the publication journey for the combined TASK-005 fixture.
async fn select_qualification_definition(
    world: &World,
    project: &str,
    review: Option<&str>,
    leadership: bool,
) {
    let prefix = if leadership {
        "qualification"
    } else {
        "pr-review"
    };
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
    let mut newest = selected.topology.version.get();
    let mut current = Call::get(format!(
        "/v1/projects/{project}/topology-specs/{lineage}/{newest}"
    ))
    .signed_as(world, "admin")
    .send(world)
    .await;
    assert_eq!(current.status, 200, "{}", current.body);
    loop {
        let next = Call::get(format!(
            "/v1/projects/{project}/topology-specs/{lineage}/{}",
            newest + 1
        ))
        .signed_as(world, "admin")
        .send(world)
        .await;
        if next.status != 200 {
            break;
        }
        newest += 1;
        current = next;
    }
    let mut node_kinds = current.json()["document"]["node_kinds"].clone();
    let mut kinds = serde_json::from_str::<Vec<serde_json::Value>>(REVIEW_KINDS).expect("kinds");
    if leadership {
        kinds.extend(
            serde_json::from_str::<Vec<serde_json::Value>>(super::desk_adams::DESK_LED_KINDS)
                .expect("leadership kinds")
                .into_iter()
                .filter(|kind| ["ADE", "ADH"].contains(&kind["kind"].as_str().unwrap_or_default())),
        );
    }
    let missing: Vec<serde_json::Value> = kinds
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
            "base": {"id": lineage.to_string(), "version": newest},
            "name": "PR review vocabulary",
            "root_kind": current.json()["document"]["root_kind"],
            "node_kinds": node_kinds,
            "historical_codes": current.json()["document"]["historical_codes"],
        }),
    )
    .signed_as(world, "admin")
    .with_key(format!("{prefix}-topology-draft"))
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
    .with_key(format!("{prefix}-topology-publish"))
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
    .with_key(format!("{prefix}-topology-select"))
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
    // Only a declared review may render a pull request, so an undeclared
    // definition carries the desks alone.
    let mut containers: Vec<serde_json::Value> =
        serde_json::from_str::<Vec<serde_json::Value>>(REVIEW_CONTAINERS)
            .expect("containers")
            .into_iter()
            .filter(|container| {
                review.is_some()
                    || ["DESK", "DWS"].contains(&container["kind"].as_str().unwrap_or_default())
            })
            .collect();
    if leadership {
        containers.extend(
            serde_json::from_str::<Vec<serde_json::Value>>(super::desk_adams::DESK_LED_CONTAINERS)
                .expect("leadership containers")
                .into_iter()
                .filter(|kind| ["ADE", "ADH"].contains(&kind["kind"].as_str().unwrap_or_default())),
        );
        definition["desk_leadership"] =
            serde_json::from_str(super::desk_adams::LEADERSHIP).expect("leadership");
    }
    let existing = definition["containers"].as_array_mut().expect("containers");
    for container in containers {
        if existing
            .iter()
            .all(|declared| declared["kind"] != container["kind"])
        {
            existing.push(container);
        }
    }
    definition["desks"] = serde_json::from_str(DESKS).expect("desks");
    if let Some(review) = review {
        definition["pr_review"] = serde_json::from_str(review).expect("pr_review");
    }
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
    .with_key(format!("{prefix}-definition-publish"))
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
    .with_key(format!("{prefix}-definition-select"))
    .send(world)
    .await;
    assert_eq!(chosen.status, 200, "{}", chosen.body);
    let _: TeamDefinitionSpec =
        serde_json::from_value(definition).expect("the published review definition");
}

fn claude_route() -> serde_json::Value {
    serde_json::json!({"provider": "claude", "model": "claude-opus-5", "effort": "high"})
}

fn codex_route() -> serde_json::Value {
    serde_json::json!({"provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"})
}

/// One exact route per seat: the Adam's and every room slot's.
fn routes() -> serde_json::Value {
    serde_json::json!({
        "tpm": codex_route(),
        "rev-a": claude_route(),
        "rev-b": codex_route(),
        "judge": claude_route(),
    })
}

fn open_body(
    jira_key: &str,
    repository: &str,
    number: u64,
    routes: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "external_key": jira_key,
        "pull_request": {"repository": repository, "number": number},
    });
    if let Some(routes) = routes {
        body["model_routes"] = routes;
    }
    body
}

async fn open_as(
    realm: &Review,
    tier: &str,
    desk: &str,
    body: &serde_json::Value,
    key: &str,
) -> Answer {
    let mut body = body.clone();
    if body.get("expected_revision").is_none() {
        body["expected_revision"] =
            serde_json::json!(current_project_revision(&realm.world, &realm.project).await);
    }
    Call::post(
        format!(
            "/v1/projects/{}/desks/{desk}/pr-reviews:open",
            realm.project
        ),
        &body,
    )
    .signed_as(&realm.world, tier)
    .with_key(key)
    .send(&realm.world)
    .await
}

async fn open(realm: &Review, body: &serde_json::Value, key: &str) -> Answer {
    open_as(realm, "operator", "pr-review", body, key).await
}

/// Open one review and return its readback.
async fn opened(realm: &Review, body: &serde_json::Value, key: &str) -> serde_json::Value {
    let answer = open(realm, body, key).await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    answer.json()["review"].clone()
}

async fn close(realm: &Review, repository: &str, number: u64, key: &str) -> Answer {
    Call::post(
        format!(
            "/v1/projects/{}/desks/pr-review/pr-reviews:close",
            realm.project
        ),
        &serde_json::json!({
            "expected_revision": current_project_revision(&realm.world, &realm.project).await,
            "pull_request": {"repository": repository, "number": number},
        }),
    )
    .signed_as(&realm.world, "operator")
    .with_key(key)
    .send(&realm.world)
    .await
}

async fn list(realm: &Review) -> serde_json::Value {
    let read = Call::get(format!(
        "/v1/projects/{}/desks/pr-review/pr-reviews",
        realm.project
    ))
    .signed_as(&realm.world, "observer")
    .send(&realm.world)
    .await;
    assert_eq!(read.status, 200, "{}", read.body);
    read.json()
}

/// One review of a report, by repository and number.
fn review_in(report: &serde_json::Value, repository: &str, number: u64) -> serde_json::Value {
    report["reviews"]
        .as_array()
        .expect("reviews")
        .iter()
        .find(|review| review["repository"] == repository && review["number"] == number)
        .unwrap_or_else(|| panic!("{repository}#{number} is not in {report}"))
        .clone()
}

fn pr_desk(realm: &Review) -> Option<StoredDesk> {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| {
            store.get_desk(
                realm.project_id,
                &DeskKey::parse("pr-review").expect("a desk key"),
            )
        })
        .expect("the desk reads")
}

fn subjects(realm: &Review) -> Vec<StoredPrReviewSubject> {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| {
            store.list_pr_review_subjects(
                realm.project_id,
                &DeskKey::parse("pr-review").expect("a desk key"),
            )
        })
        .expect("the subjects read")
}

fn container(realm: &Review, id: TopologyNodeId) -> kontor_core::state::NativeContainerBinding {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_topology_node_container(realm.project_id, id))
        .expect("the binding reads")
        .expect("the node holds a native container")
}

fn seat_row(realm: &Review, seat: SeatBindingId) -> kontor_core::state::SeatBinding {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_seat_binding(realm.project_id, seat))
        .expect("the seat reads")
        .expect("the seat exists")
}

fn hosted(
    realm: &Review,
    seat: SeatBindingId,
) -> Option<kontor_core::repository::StoredHostedTopologySeat> {
    realm
        .world
        .daemon
        .state()
        .with_store(|store| store.get_hosted_topology_seat(realm.project_id, seat))
        .expect("the hosted seat reads")
}

fn node_id(value: &serde_json::Value) -> TopologyNodeId {
    TopologyNodeId::parse(value.as_str().expect("a node id")).expect("a node id")
}

fn seat_id(value: &serde_json::Value) -> SeatBindingId {
    SeatBindingId::parse(value.as_str().expect("a seat id")).expect("a seat id")
}

/// Every seat of one review's readback: the Adam's, then the room's.
fn review_seats(review: &serde_json::Value) -> Vec<serde_json::Value> {
    ["adam", "room"]
        .iter()
        .flat_map(|node| {
            review[*node]["seats"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

fn seat_natives(review: &serde_json::Value) -> Vec<String> {
    review_seats(review)
        .iter()
        .map(|seat| seat["native_id"].as_str().expect("a native").to_owned())
        .collect()
}

fn calls_since(realm: &Review, before: usize) -> Vec<AdapterCall> {
    realm.world.fake.calls()[before..].to_vec()
}

fn launches(calls: &[AdapterCall]) -> Vec<SeatBindingId> {
    calls
        .iter()
        .filter_map(|call| match call {
            AdapterCall::LaunchHostedSeat(seat) => Some(*seat),
            _ => None,
        })
        .collect()
}

fn retirements(calls: &[AdapterCall]) -> Vec<SeatBindingId> {
    calls
        .iter()
        .filter_map(|call| match call {
            AdapterCall::RetireHostedSeat(seat) => Some(*seat),
            _ => None,
        })
        .collect()
}

/// Assert one launched review's exact placement, titles and seats.
fn assert_review_placed(realm: &Review, review: &serde_json::Value, key: &str, pr: &str) {
    let desk = pr_desk(realm).expect("the PR desk exists");
    let desk_project = container(realm, desk.topology_node_id);
    let desk_workspace = container(realm, desk.workspace_node_id);
    let scope = HostedSeatScope::Desk {
        project: desk_project.identity.clone(),
    };

    // The Adam: a tab below the desk workspace, in the workspace's container.
    let adam = &review["adam"];
    assert_eq!(adam["kind"], "PRA");
    assert_eq!(adam["parent_id"], desk.workspace_node_id.to_string());
    assert_eq!(
        adam["container"]["topology_node_id"],
        desk.workspace_node_id.to_string()
    );
    assert_eq!(
        adam["container"]["native_id"],
        desk_workspace.identity.native_id.as_str()
    );
    let adam_seats = adam["seats"].as_array().expect("seats");
    assert_eq!(adam_seats.len(), 1, "{adam}");
    let tpm = &adam_seats[0];
    let tpm_title = format!("TPM • {key} • {pr}");
    assert_eq!(tpm["slot_id"], "tpm");
    assert_eq!(tpm["role_code"], "TPM");
    assert_eq!(tpm["title"], tpm_title.as_str());
    assert_eq!(tpm["occupancy_generation"], 1);
    let native = ExternalId::parse(tpm["native_id"].as_str().expect("a native")).expect("an id");
    assert_eq!(realm.world.fake.seat_title(&native), Some(tpm_title));
    let tpm_seat = seat_id(&tpm["seat_binding_id"]);
    assert_eq!(
        realm.world.fake.hosted_launch_scope(tpm_seat),
        Some(scope.clone()),
        "the Adam launches in the desk's own project and names no epic"
    );
    assert_eq!(
        seat_row(realm, tpm_seat).topology_node_id,
        node_id(&adam["topology_node_id"])
    );

    // The room: a native workspace of the desk's project, directly below it.
    let room = &review["room"];
    let room_id = node_id(&room["topology_node_id"]);
    let room_title = format!("RSW • {key} • {pr}");
    assert_eq!(room["kind"], "RSW");
    assert_eq!(room["parent_id"], desk.topology_node_id.to_string());
    assert_eq!(room["container"]["topology_node_id"], room_id.to_string());
    assert_eq!(room["container"]["observed_kind"], "workspace");
    assert_eq!(room["container"]["title"], room_title.as_str());
    assert_eq!(
        room["container"]["parent_native_id"],
        desk_project.identity.native_id.as_str()
    );
    assert_eq!(
        realm.world.fake.container_title(room_id),
        Some(room_title),
        "the runtime holds the room under its exact title"
    );
    let cwd = std::path::PathBuf::from(room["container"]["cwd"].as_str().expect("a cwd"));
    assert_eq!(
        cwd.file_name().and_then(|name| name.to_str()),
        Some(format!("desk-{}-room-{room_id}", desk.topology_node_id).as_str()),
        "each room works in a directory of its own"
    );
    assert_eq!(
        cwd.parent(),
        std::path::Path::new(
            desk_workspace
                .canonical_cwd
                .as_ref()
                .expect("the desk has a directory")
                .as_str()
        )
        .parent(),
        "a room's directory sits beside its desk's"
    );
    let room_seats = room["seats"].as_array().expect("seats");
    assert_eq!(room_seats.len(), ROOM_SLOTS.len(), "{room}");
    for (seat, (slot, label)) in room_seats.iter().zip(ROOM_SLOTS) {
        assert_eq!(seat["slot_id"], slot);
        assert_eq!(seat["role_code"], "AUD");
        assert_eq!(seat["title"], label);
        assert_eq!(seat["occupancy_generation"], 1);
        let native =
            ExternalId::parse(seat["native_id"].as_str().expect("a native")).expect("an id");
        assert_eq!(realm.world.fake.seat_title(&native), Some(label.to_owned()));
        let binding = seat_id(&seat["seat_binding_id"]);
        assert_eq!(
            realm.world.fake.hosted_launch_scope(binding),
            Some(scope.clone())
        );
        assert_eq!(seat_row(realm, binding).topology_node_id, room_id);
    }
}

#[tokio::test]
async fn two_pull_requests_of_one_key_get_distinct_adam_tabs_and_rooms_and_replay_reuses_them() {
    let realm = review_realm("prr-distinct", Some(PR_REVIEW)).await;
    let first_body = open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes()));
    let first = open(&realm, &first_body, "prr-distinct-101").await;
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(first.json()["receipt"]["applied"], "created");
    let second = opened(
        &realm,
        &open_body(&realm.epic_key, MODULE_REPOSITORY, 102, Some(routes())),
        "prr-distinct-102",
    )
    .await;
    let first = first.json()["review"].clone();

    for (review, number) in [(&first, 101), (&second, 102)] {
        assert_eq!(review["desk_key"], "pr-review");
        assert_eq!(review["repository"], MODULE_REPOSITORY);
        assert_eq!(review["number"], number);
        assert_eq!(review["external_key"], realm.epic_key.as_str());
        assert_eq!(review["epic_id"], realm.epic.as_str());
        assert!(review.get("task_id").is_none(), "{review}");
        assert_eq!(review["open"], true);
        assert_review_placed(
            &realm,
            review,
            &realm.epic_key,
            &format!("asma-rs-kontor#{number}"),
        );
    }
    // Repository and number are the identity: two numbers are two of
    // everything, and no native serves both.
    for node in ["adam", "room"] {
        assert_ne!(
            first[node]["topology_node_id"],
            second[node]["topology_node_id"]
        );
    }
    assert_ne!(
        first["room"]["container"]["native_id"],
        second["room"]["container"]["native_id"]
    );
    let mut natives = seat_natives(&first);
    natives.extend(seat_natives(&second));
    let distinct: BTreeSet<_> = natives.iter().collect();
    assert_eq!(distinct.len(), 8, "{natives:?}");

    // A replay returns the review as it is, and asks the runtime nothing.
    let before = realm.world.fake.calls().len();
    let replay = open(&realm, &first_body, "prr-distinct-101").await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    assert_eq!(replay.json()["review"], first);
    assert_eq!(calls_since(&realm, before), Vec::new());

    // A fresh open of the same pull request reuses each binding and native.
    let again = opened(&realm, &first_body, "prr-distinct-101-again").await;
    assert_eq!(again, first);
    assert_eq!(launches(&calls_since(&realm, before)), Vec::new());

    // The key names exactly one request.
    let reused = open(
        &realm,
        &open_body(&realm.epic_key, MODULE_REPOSITORY, 103, None),
        "prr-distinct-101",
    )
    .await;
    assert_eq!(reused.status, 409, "{}", reused.body);
    assert_eq!(reused.json()["code"], "idempotency_conflict");

    let report = list(&realm).await;
    assert_eq!(report["desk_key"], "pr-review");
    let numbers: Vec<_> = report["reviews"]
        .as_array()
        .expect("reviews")
        .iter()
        .map(|review| review["number"].as_u64().expect("a number"))
        .collect();
    assert_eq!(numbers, vec![101, 102]);
    assert_eq!(review_in(&report, MODULE_REPOSITORY, 101), first);
    assert_eq!(review_in(&report, MODULE_REPOSITORY, 102), second);

    // The record survives a daemon restart byte for byte.
    let realm = restarted(realm).await;
    assert_eq!(list(&realm).await, report);
}

#[tokio::test]
async fn the_same_number_in_two_repositories_is_two_reviews_and_a_task_key_names_its_task() {
    let realm = review_realm("prr-repositories", Some(PR_REVIEW)).await;
    let before = realm.world.fake.calls().len();
    let root = opened(
        &realm,
        &open_body(&realm.epic_key, ROOT_REPOSITORY, 7, None),
        "prr-repositories-root",
    )
    .await;
    let module = opened(
        &realm,
        &open_body(&realm.task_key, MODULE_REPOSITORY, 7, None),
        "prr-repositories-module",
    )
    .await;
    // Without routes a review is placed and seated, and nothing is launched.
    let calls = calls_since(&realm, before);
    assert!(
        calls.iter().all(|call| !matches!(
            call,
            AdapterCall::LaunchHostedSeat(_) | AdapterCall::PrepareContainer(_)
        )),
        "{calls:?}"
    );
    assert_eq!(root["external_key"], realm.epic_key.as_str());
    assert!(root.get("task_id").is_none(), "{root}");
    assert_eq!(module["external_key"], realm.task_key.as_str());
    let task = realm.world.daemon.state().with_store(|store| {
        store
            .list_epic_tasks(
                realm.project_id,
                MiniProjectId::parse(&realm.epic).expect("an epic id"),
            )
            .expect("the tasks read")
            .into_iter()
            .next()
            .expect("the epic has its task")
    });
    assert_eq!(module["task_id"], task.id.to_string());
    assert_eq!(module["epic_id"], realm.epic.as_str());
    for node in ["adam", "room"] {
        assert_ne!(
            root[node]["topology_node_id"],
            module[node]["topology_node_id"]
        );
    }
    for (review, key, pr) in [
        (&root, &realm.epic_key, "asma-modules#7"),
        (&module, &realm.task_key, "asma-rs-kontor#7"),
    ] {
        assert_eq!(review["open"], true);
        assert!(review["room"].get("container").is_none(), "{review}");
        let tpm = &review["adam"]["seats"][0];
        assert_eq!(tpm["title"], format!("TPM • {key} • {pr}").as_str());
        assert!(tpm.get("native_id").is_none(), "{tpm}");
        let labels: Vec<_> = review["room"]["seats"]
            .as_array()
            .expect("seats")
            .iter()
            .map(|seat| seat["title"].as_str().expect("a title").to_owned())
            .collect();
        assert_eq!(labels, vec!["REV A", "REV B", "JUDGE"]);
    }
    assert_eq!(subjects(&realm).len(), 2);
}

#[tokio::test]
async fn closing_one_review_keeps_the_desk_its_durable_seat_and_the_other_review() {
    let realm = review_realm("prr-close", Some(PR_REVIEW)).await;
    let first = opened(
        &realm,
        &open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes())),
        "prr-close-101",
    )
    .await;
    let second = opened(
        &realm,
        &open_body(&realm.epic_key, MODULE_REPOSITORY, 102, Some(routes())),
        "prr-close-102",
    )
    .await;
    // A daemon-owned durable seat on the same desk, launched by ASMA-8451.
    let declared = Call::post(
        format!(
            "/v1/projects/{}/desks/pr-review/seats:declare",
            realm.project
        ),
        &serde_json::json!({
            "expected_revision": current_project_revision(&realm.world, &realm.project).await,
            "seat_key": "keeper",
            "role_code": "LSA",
            "seat_label": "KEEPER",
            "model_route": codex_route(),
            "recovery_owner": "daemon",
            "decided_by": "Test Owner",
            "decided_on": "2026-10-11",
        }),
    )
    .signed_as(&realm.world, "operator")
    .with_key("prr-close-declare")
    .send(&realm.world)
    .await;
    assert_eq!(declared.status, 200, "{}", declared.body);
    let keeper = seat_id(&declared.json()["seat"]["seat_binding_id"]);
    let reconciled = Call::post(
        format!(
            "/v1/projects/{}/desks/pr-review/seats:reconcile",
            realm.project
        ),
        &serde_json::json!({
            "expected_revision": current_project_revision(&realm.world, &realm.project).await,
            "seat_keys": ["keeper"],
            "restart_missing": true,
        }),
    )
    .signed_as(&realm.world, "operator")
    .with_key("prr-close-reconcile")
    .send(&realm.world)
    .await;
    assert_eq!(reconciled.status, 200, "{}", reconciled.body);
    let keeper_native = hosted(&realm, keeper)
        .expect("the durable seat is live")
        .native_identity;
    let desk = pr_desk(&realm).expect("the PR desk exists");
    let desk_project = container(&realm, desk.topology_node_id);
    let desk_workspace = container(&realm, desk.workspace_node_id);
    let room_id = node_id(&first["room"]["topology_node_id"]);
    let room_native = container(&realm, room_id).identity;

    let before = realm.world.fake.calls().len();
    let closed = close(&realm, MODULE_REPOSITORY, 101, "prr-close-101-close").await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    assert_eq!(closed.json()["receipt"]["applied"], "created");
    let after = &closed.json()["review"];
    assert_eq!(after["open"], false);
    assert_eq!(after["adam"]["seats"], serde_json::json!([]));
    assert_eq!(after["room"]["seats"], serde_json::json!([]));
    // The room's workspace stays bound: the same pull request reopens onto it.
    assert_eq!(after["room"]["container"], first["room"]["container"]);

    // Exactly this review's four sessions were retired, and nothing else
    // was asked of the runtime but the fence's readback.
    let closed_seats: BTreeSet<SeatBindingId> = review_seats(&first)
        .iter()
        .map(|seat| seat_id(&seat["seat_binding_id"]))
        .collect();
    let calls = calls_since(&realm, before);
    assert_eq!(
        retirements(&calls).into_iter().collect::<BTreeSet<_>>(),
        closed_seats,
        "{calls:?}"
    );
    assert_eq!(retirements(&calls).len(), 4, "{calls:?}");
    assert!(
        calls.iter().all(|call| !matches!(
            call,
            AdapterCall::ArchiveContainer(_) | AdapterCall::LaunchHostedSeat(_)
        )),
        "{calls:?}"
    );
    for seat in &closed_seats {
        assert!(!seat_row(&realm, *seat).is_non_terminal());
        assert!(hosted(&realm, *seat).is_none());
    }

    // The other review, the desk and its durable seat are exactly as they were.
    assert_eq!(
        review_in(&list(&realm).await, MODULE_REPOSITORY, 102),
        second
    );
    for seat in review_seats(&second) {
        assert!(seat_row(&realm, seat_id(&seat["seat_binding_id"])).is_non_terminal());
    }
    assert_eq!(pr_desk(&realm), Some(desk.clone()));
    assert_eq!(container(&realm, desk.topology_node_id), desk_project);
    assert_eq!(container(&realm, desk.workspace_node_id), desk_workspace);
    assert!(seat_row(&realm, keeper).is_non_terminal());
    assert_eq!(
        hosted(&realm, keeper).map(|seat| seat.native_identity),
        Some(keeper_native)
    );

    // A replayed close changes nothing and asks the runtime nothing.
    let before = realm.world.fake.calls().len();
    let replay = close(&realm, MODULE_REPOSITORY, 101, "prr-close-101-close").await;
    assert_eq!(replay.status, 200, "{}", replay.body);
    assert_eq!(replay.json()["receipt"]["applied"], "unchanged");
    assert_eq!(calls_since(&realm, before), Vec::new());

    // Reopening places fresh seats on the same nodes and the same room.
    let reopened = opened(
        &realm,
        &open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes())),
        "prr-close-101-reopen",
    )
    .await;
    for node in ["adam", "room"] {
        assert_eq!(
            reopened[node]["topology_node_id"],
            first[node]["topology_node_id"]
        );
    }
    assert_eq!(container(&realm, room_id).identity, room_native);
    assert_review_placed(&realm, &reopened, &realm.epic_key, "asma-rs-kontor#101");
    let reopened_seats: BTreeSet<SeatBindingId> = review_seats(&reopened)
        .iter()
        .map(|seat| seat_id(&seat["seat_binding_id"]))
        .collect();
    assert!(reopened_seats.is_disjoint(&closed_seats));
    let old: BTreeSet<_> = seat_natives(&first).into_iter().collect();
    assert!(
        seat_natives(&reopened)
            .iter()
            .all(|native| !old.contains(native))
    );
}

/// A retirement whose acknowledgement is lost has already taken effect, and
/// any session fenced before it is gone too. The refusal must name a partial
/// close and the convergent retry, never "nothing was changed".
#[tokio::test]
async fn a_close_that_lost_a_retirement_acknowledgement_reports_a_partial_close() {
    let realm = review_realm("prr-partial-close", Some(PR_REVIEW)).await;
    let first = opened(
        &realm,
        &open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes())),
        "prr-partial-close-open",
    )
    .await;
    let lost = seat_id(&first["room"]["seats"][0]["seat_binding_id"]);
    realm.world.fake.lose_next_hosted_retire_ack(lost);

    let refused = close(&realm, MODULE_REPOSITORY, 101, "prr-partial-close-close").await;
    assert_eq!(refused.status, 503, "{}", refused.body);
    assert_eq!(
        refused.json()["code"],
        "delivery_unconfirmed",
        "{}",
        refused.body
    );
    let refusal = refused.json();
    let action = refusal["action"].as_str().expect("an action");
    assert!(
        action.contains("retry this same close"),
        "the partial close names its convergent retry: {}",
        refused.body
    );

    let retried = close(&realm, MODULE_REPOSITORY, 101, "prr-partial-close-close").await;
    assert_eq!(retried.status, 200, "{}", retried.body);
    assert_eq!(retried.json()["review"]["open"], false);
    for seat in review_seats(&first) {
        assert!(
            !seat_row(&realm, seat_id(&seat["seat_binding_id"])).is_non_terminal(),
            "every seat is released after the convergent retry"
        );
    }
}

#[tokio::test]
async fn an_open_is_refused_whole_before_any_effect() {
    let realm = review_realm("prr-refusals", Some(PR_REVIEW)).await;
    let before = realm.world.fake.calls().len();
    // Each refusal is pinned to its rule, so none passes on another's ground.
    let refused = |answer: &Answer, status: u16, code: &str, rule: &str| {
        assert_eq!(answer.status, status, "{}", answer.body);
        assert_eq!(answer.json()["code"], code, "{}", answer.body);
        assert_eq!(answer.json()["rule"], rule, "{}", answer.body);
    };
    let epic = realm.epic_key.clone();

    // A key nobody confirmed, and the confirmed key in another spelling.
    for (case, key) in [
        ("forged", "ASMA-7".to_owned()),
        ("respelled", epic.to_lowercase()),
    ] {
        let forged = open(
            &realm,
            &open_body(&key, MODULE_REPOSITORY, 101, Some(routes())),
            &format!("prr-refusals-{case}"),
        )
        .await;
        refused(
            &forged,
            409,
            "placement_blocked",
            "the Jira key names no confirmed epic or task binding in this project",
        );
    }
    // A repository the key's epic does not own.
    let foreign = open(
        &realm,
        &open_body(&epic, "Carasent-ASMA/asma-console", 101, Some(routes())),
        "prr-refusals-foreign",
    )
    .await;
    refused(
        &foreign,
        403,
        "forbidden",
        "the Jira key does not authorize the pull request's repository",
    );
    let zero = open(
        &realm,
        &open_body(&epic, MODULE_REPOSITORY, 0, None),
        "prr-refusals-zero",
    )
    .await;
    refused(
        &zero,
        400,
        "invalid_request",
        "a pull request number is positive and fits the stored integer",
    );
    // A repository that is not exact `owner/name`, and a field no route reads.
    let mut unknown = open_body(&epic, MODULE_REPOSITORY, 101, None);
    unknown["pull_request"]["title"] = serde_json::json!("asma-rs-kontor#101");
    for (case, body) in [
        ("malformed", open_body(&epic, "asma-rs-kontor", 101, None)),
        ("unknown", unknown),
    ] {
        refused(
            &open(&realm, &body, &format!("prr-refusals-{case}")).await,
            400,
            "invalid_request",
            "the request body is valid JSON but does not match this route's schema",
        );
    }
    let mut stale = open_body(&epic, MODULE_REPOSITORY, 101, None);
    stale["expected_revision"] =
        serde_json::json!(current_project_revision(&realm.world, &realm.project).await + 1);
    refused(
        &open(&realm, &stale, "prr-refusals-stale").await,
        409,
        "revision_conflict",
        "the project moved since the caller read it",
    );
    let observer = open_as(
        &realm,
        "observer",
        "pr-review",
        &open_body(&epic, MODULE_REPOSITORY, 101, None),
        "prr-refusals-observer",
    )
    .await;
    refused(
        &observer,
        403,
        "forbidden",
        "this route requires a higher realm authority than the presented credential",
    );
    for desk in ["adam", "nope"] {
        let wrong = open_as(
            &realm,
            "operator",
            desk,
            &open_body(&epic, MODULE_REPOSITORY, 101, None),
            &format!("prr-refusals-desk-{desk}"),
        )
        .await;
        refused(
            &wrong,
            409,
            "placement_blocked",
            "the Team Definition declares no PR review on this desk",
        );
    }
    let mut short = routes();
    short.as_object_mut().expect("routes").remove("judge");
    let mut long = routes();
    long["rev-c"] = claude_route();
    for (case, routes) in [("short", short), ("long", long)] {
        let mismatched = open(
            &realm,
            &open_body(&epic, MODULE_REPOSITORY, 101, Some(routes)),
            &format!("prr-refusals-routes-{case}"),
        )
        .await;
        refused(
            &mismatched,
            400,
            "invalid_request",
            "model_routes must name the Adam's slot and every room slot exactly once",
        );
    }
    // Nothing above wrote anything or asked the runtime anything.
    assert_eq!(pr_desk(&realm), None);
    assert!(subjects(&realm).is_empty());
    assert_eq!(calls_since(&realm, before), Vec::new());

    // One review is open under the epic's key; the same pull request under
    // another key is refused, and so is a pull request no review names.
    opened(
        &realm,
        &open_body(&epic, MODULE_REPOSITORY, 101, None),
        "prr-refusals-101",
    )
    .await;
    let rekeyed = open(
        &realm,
        &open_body(&realm.task_key, MODULE_REPOSITORY, 101, None),
        "prr-refusals-101-task",
    )
    .await;
    refused(
        &rekeyed,
        409,
        "ensure_mismatch",
        "this pull request's review was opened under another Jira key",
    );
    refused(
        &close(&realm, MODULE_REPOSITORY, 999, "prr-refusals-close-999").await,
        404,
        "not_found",
        "this pull request has no review on this desk",
    );

    // Titles render the repository's name alone, so a same-named repository
    // of another owner may never share a number on the desk.
    let placed = subjects(&realm).pop().expect("the review is recorded");
    realm.world.daemon.state().with_store(|store| {
        store
            .create_pr_review_subject(&StoredPrReviewSubject {
                pull_request: PullRequestRef::new(
                    ForgeRepository::parse("Elsewhere/asma-rs-kontor").expect("a repository"),
                    55,
                )
                .expect("a pull request"),
                adam_node_id: TopologyNodeId::generate(),
                room_node_id: TopologyNodeId::generate(),
                ..placed.clone()
            })
            .expect("the foreign review is recorded")
    });
    let collision = open(
        &realm,
        &open_body(&epic, MODULE_REPOSITORY, 55, None),
        "prr-refusals-collision",
    )
    .await;
    refused(
        &collision,
        409,
        "placement_blocked",
        "another pull request on this desk renders the same title",
    );

    // A live review is never rerouted in place.
    opened(
        &realm,
        &open_body(&epic, MODULE_REPOSITORY, 102, Some(routes())),
        "prr-refusals-102",
    )
    .await;
    let before = realm.world.fake.calls().len();
    let mut rerouted = routes();
    rerouted["tpm"] = claude_route();
    let moved = open(
        &realm,
        &open_body(&epic, MODULE_REPOSITORY, 102, Some(rerouted)),
        "prr-refusals-102-rerouted",
    )
    .await;
    refused(
        &moved,
        409,
        "placement_blocked",
        "a seat of this review is already live under a different route",
    );
    assert_eq!(calls_since(&realm, before), Vec::new());
    assert_eq!(subjects(&realm).len(), 3);
}

#[tokio::test]
async fn a_review_session_that_died_is_recovered_by_closing_and_opening_again() {
    let realm = review_realm("prr-died", Some(PR_REVIEW)).await;
    let body = open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes()));
    let first = opened(&realm, &body, "prr-died-open").await;
    // REV A's session ends outside Kontor.
    let rev_a = &first["room"]["seats"][0];
    assert_eq!(rev_a["slot_id"], "rev-a");
    realm.world.fake.archive_hosted_seat(
        &ExternalId::parse(rev_a["native_id"].as_str().expect("a native")).expect("an id"),
    );

    // Opening again neither relaunches it nor names a path a review lacks.
    let before = realm.world.fake.calls().len();
    let stale = open(&realm, &body, "prr-died-reopen").await;
    assert_eq!(stale.status, 409, "{}", stale.body);
    assert_eq!(stale.json()["code"], "stale_binding");
    assert_eq!(
        stale.json()["rule"],
        "a session of this review is no longer live"
    );
    assert_eq!(
        stale.json()["action"],
        "close this review and open it again; nothing was relaunched"
    );
    assert_eq!(launches(&calls_since(&realm, before)), Vec::new());

    // Closing fences what is left and counts the dead session as gone.
    let closed = close(&realm, MODULE_REPOSITORY, 101, "prr-died-close").await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    for seat in review_seats(&first) {
        let seat = seat_id(&seat["seat_binding_id"]);
        assert!(!seat_row(&realm, seat).is_non_terminal());
        assert!(hosted(&realm, seat).is_none());
    }
    let reopened = opened(&realm, &body, "prr-died-open-again").await;
    assert_review_placed(&realm, &reopened, &realm.epic_key, "asma-rs-kontor#101");
}

#[tokio::test]
async fn a_room_role_the_catalog_lacks_refuses_before_anything_is_written() {
    let review = PR_REVIEW.replace(
        r#"{"slot_id": "judge", "role_code": "AUD"}"#,
        r#"{"slot_id": "judge", "role_code": "ZZZ"}"#,
    );
    assert_ne!(review, PR_REVIEW);
    let realm = review_realm("prr-uncatalogued", Some(&review)).await;
    let before = realm.world.fake.calls().len();
    let refused = open(
        &realm,
        &open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes())),
        "prr-uncatalogued-101",
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(
        refused.json()["code"],
        "placement_blocked",
        "{}",
        refused.body
    );
    assert_eq!(
        refused.json()["rule"],
        "the reviewing Team Definition names a role the catalog does not declare"
    );
    assert_eq!(pr_desk(&realm), None);
    assert!(subjects(&realm).is_empty());
    assert_eq!(calls_since(&realm, before), Vec::new());
}

#[tokio::test]
async fn a_definition_without_pr_review_places_nothing() {
    let realm = review_realm("prr-undeclared", None).await;
    let before = realm.world.fake.calls().len();
    let refused = open(
        &realm,
        &open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes())),
        "prr-undeclared-101",
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.json()["code"], "placement_blocked");
    assert_eq!(
        refused.json()["rule"],
        "the Team Definition declares no PR review on this desk"
    );
    assert_eq!(pr_desk(&realm), None);
    assert!(subjects(&realm).is_empty());
    assert_eq!(calls_since(&realm, before), Vec::new());
}

/// One way a review's proved native placement is later found wrong.
#[derive(Clone, Copy, Debug)]
enum Misplaced {
    /// The room's native workspace now works in another directory.
    RoomCwd,
    /// Another native workspace now occupies the room's node.
    RoomNative,
    /// The room's native workspace no longer sits below the desk's project.
    RoomParent,
    /// The desk workspace the Adam runs in now works in another directory.
    DeskWorkspaceCwd,
}

/// Misplace one review's native placement in the runtime, and return the
/// drift that puts it back. A lost parent cannot be put back.
fn misplace(
    realm: &Review,
    desk: &StoredDesk,
    room: TopologyNodeId,
    how: Misplaced,
) -> Option<(TopologyNodeId, kontor_runtime::fake::FakeContainerDrift)> {
    use kontor_runtime::container::{ContainerBindingId, ContainerWorkspaceKind};
    use kontor_runtime::fake::FakeContainerDrift;
    use kontor_runtime::workspace::WorkspaceRoot;
    let fake = &realm.world.fake;
    let root = |node| {
        let bound = container(realm, node);
        WorkspaceRoot::parse(bound.canonical_cwd.expect("a cwd").as_str()).expect("a root")
    };
    match how {
        Misplaced::RoomCwd => {
            let restore = FakeContainerDrift::Root(root(room));
            fake.drift_container(
                room,
                FakeContainerDrift::Root(
                    WorkspaceRoot::parse("/tmp/prr-drifted-room").expect("a root"),
                ),
            );
            Some((room, restore))
        }
        Misplaced::RoomNative => {
            let restore = FakeContainerDrift::NativeId(container(realm, room).identity.native_id);
            fake.drift_container(
                room,
                FakeContainerDrift::NativeId(
                    ExternalId::parse("native-replaced-room").expect("an id"),
                ),
            );
            Some((room, restore))
        }
        Misplaced::RoomParent => {
            // The same native, directory, title and shape, with no parent: the
            // runtime's room is no longer below the desk's project.
            let bound = container(realm, room);
            let title = fake.container_title(room).expect("the room has a title");
            fake.forget_container(room);
            fake.seed_container(
                room,
                ContainerBindingId::parse(bound.container_binding_id.as_str()).expect("an id"),
                bound.identity.native_id.clone(),
                root(room),
                &title,
                bound.bound_at,
            );
            fake.seed_container_kind(room, ContainerWorkspaceKind::Directory);
            None
        }
        Misplaced::DeskWorkspaceCwd => {
            let restore = FakeContainerDrift::Root(root(desk.workspace_node_id));
            fake.drift_container(
                desk.workspace_node_id,
                FakeContainerDrift::Root(
                    WorkspaceRoot::parse("/tmp/prr-drifted-desk-workspace").expect("a root"),
                ),
            );
            Some((desk.workspace_node_id, restore))
        }
    }
}

/// A review whose room or desk workspace no longer proves is refused whole:
/// an open launches nothing and rebinds nothing, and a close fences nothing,
/// not even the Adam's session in a workspace that still proves.
#[tokio::test]
async fn a_misplaced_room_or_desk_workspace_refuses_open_and_close_before_any_effect() {
    const STALE: &str = "the binding no longer names a session this runtime will act on";
    const MOVED: &str = "this topology node already has a native container at another root";
    const ORPHANED: &str = "the inspected container does not have its exact persisted parent";
    for (case, how, reopen, close_refusal) in [
        (
            "room-cwd",
            Misplaced::RoomCwd,
            (422, "unsupported_capability", MOVED),
            (409, "stale_binding", STALE),
        ),
        (
            "room-native",
            Misplaced::RoomNative,
            (409, "stale_binding", STALE),
            (409, "stale_binding", STALE),
        ),
        (
            "room-parent",
            Misplaced::RoomParent,
            (422, "unsupported_capability", ORPHANED),
            (422, "unsupported_capability", ORPHANED),
        ),
        (
            "desk-cwd",
            Misplaced::DeskWorkspaceCwd,
            (422, "unsupported_capability", MOVED),
            (409, "stale_binding", STALE),
        ),
    ] {
        let realm = review_realm(&format!("prr-{case}"), Some(PR_REVIEW)).await;
        let body = open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes()));
        let first = opened(&realm, &body, &format!("prr-{case}-open")).await;
        let desk = pr_desk(&realm).expect("the PR desk exists");
        let room = node_id(&first["room"]["topology_node_id"]);
        let seats: Vec<SeatBindingId> = review_seats(&first)
            .iter()
            .map(|seat| seat_id(&seat["seat_binding_id"]))
            .collect();
        let held = |realm: &Review| {
            (
                // What a binding places, not when it was last read back: a
                // healthy container proved on the way refreshes only that.
                [desk.topology_node_id, desk.workspace_node_id, room].map(|node| {
                    let bound = container(realm, node);
                    (
                        bound.container_binding_id,
                        bound.identity,
                        bound.canonical_cwd,
                        bound.readback,
                        bound.bound_at,
                    )
                }),
                seats
                    .iter()
                    .map(|seat| (seat_row(realm, *seat), hosted(realm, *seat)))
                    .collect::<Vec<_>>(),
                subjects(realm),
            )
        };
        let snapshot = held(&realm);
        let restore = misplace(&realm, &desk, room, how);

        let before = realm.world.fake.calls().len();
        let reopen_key = format!("prr-{case}-reopen");
        let refused = open(&realm, &body, &reopen_key).await;
        let (status, code, rule) = reopen;
        assert_eq!(refused.status, status, "{case}: {}", refused.body);
        assert_eq!(refused.json()["code"], code, "{case}: {}", refused.body);
        assert_eq!(refused.json()["rule"], rule, "{case}: {}", refused.body);
        let close_key = format!("prr-{case}-close");
        let refused = close(&realm, MODULE_REPOSITORY, 101, &close_key).await;
        let (status, code, rule) = close_refusal;
        assert_eq!(refused.status, status, "{case}: {}", refused.body);
        assert_eq!(refused.json()["code"], code, "{case}: {}", refused.body);
        assert_eq!(refused.json()["rule"], rule, "{case}: {}", refused.body);
        let calls = calls_since(&realm, before);
        assert_eq!(launches(&calls), Vec::new(), "{case}: {calls:?}");
        assert_eq!(retirements(&calls), Vec::new(), "{case}: {calls:?}");
        assert_eq!(held(&realm), snapshot, "{case}");

        // Put back, the refused keys recorded nothing: the reopen finds the
        // same sessions, and the close fences exactly this review's four.
        let Some((node, drift)) = restore else {
            continue;
        };
        realm.world.fake.drift_container(node, drift);
        let before = realm.world.fake.calls().len();
        let reopened = opened(&realm, &body, &reopen_key).await;
        assert_eq!(seat_natives(&reopened), seat_natives(&first), "{case}");
        assert_eq!(launches(&calls_since(&realm, before)), Vec::new(), "{case}");
        let closed = close(&realm, MODULE_REPOSITORY, 101, &close_key).await;
        assert_eq!(closed.status, 200, "{case}: {}", closed.body);
        let mut retired = retirements(&calls_since(&realm, before));
        retired.sort();
        let mut expected = seats.clone();
        expected.sort();
        assert_eq!(retired, expected, "{case}");
    }
}

/// A seat whose launch may have created its session and lost the answer
/// refuses a close before any session is fenced, including the review's
/// other, confirmed one; retrying the open recovers it, and the same close
/// then fences them all.
#[tokio::test]
async fn a_launch_that_lost_its_answer_refuses_close_before_anything_is_fenced() {
    let realm = review_realm("prr-lost-ack", Some(PR_REVIEW)).await;
    let body = open_body(&realm.epic_key, MODULE_REPOSITORY, 101, Some(routes()));
    // The first launch is confirmed; the second creates its session and its
    // answer is lost, so the open stops there.
    let first_launch = realm.world.fake.pause_next_hosted_launch();
    let lost = open(&realm, &body, "prr-lost-ack-open");
    let arrange = async {
        first_launch.entered().await;
        let second_launch = realm.world.fake.pause_next_hosted_launch();
        first_launch.release();
        second_launch.entered().await;
        realm.world.fake.lose_next_hosted_launch_ack();
        second_launch.release();
    };
    let (lost, ()) = tokio::join!(lost, arrange);
    assert_eq!(lost.status, 503, "{}", lost.body);
    assert_eq!(lost.json()["code"], "unavailable", "{}", lost.body);
    let subject = subjects(&realm).pop().expect("the subject was written");
    let seats: Vec<kontor_core::state::SeatBinding> = [subject.adam_node_id, subject.room_node_id]
        .iter()
        .flat_map(|node| {
            realm
                .world
                .daemon
                .state()
                .with_store(|store| store.list_seat_bindings(realm.project_id, *node))
                .expect("the seats read")
        })
        .collect();
    assert_eq!(seats.len(), 1 + ROOM_SLOTS.len());
    let confirmed: Vec<_> = seats
        .iter()
        .filter_map(|seat| hosted(&realm, seat.id))
        .collect();
    assert_eq!(confirmed.len(), 1, "exactly one session was confirmed");

    let before = realm.world.fake.calls().len();
    let refused = close(&realm, MODULE_REPOSITORY, 101, "prr-lost-ack-close").await;
    assert_eq!(refused.status, 409, "{}", refused.body);
    assert_eq!(refused.json()["code"], "placement_blocked");
    assert_eq!(
        refused.json()["rule"],
        "a launch of this review's seat may have created its session and lost the answer"
    );
    assert_eq!(
        refused.json()["action"],
        "retry the open that launched it with the same idempotency key, then close; nothing was released"
    );
    assert_eq!(calls_since(&realm, before), Vec::new());
    for seat in &seats {
        assert_eq!(&seat_row(&realm, seat.id), seat);
    }
    assert_eq!(
        seats
            .iter()
            .filter_map(|seat| hosted(&realm, seat.id))
            .collect::<Vec<_>>(),
        confirmed
    );

    let retried = open(&realm, &body, "prr-lost-ack-open").await;
    assert_eq!(retried.status, 200, "{}", retried.body);
    assert_review_placed(
        &realm,
        &retried.json()["review"],
        &realm.epic_key,
        "asma-rs-kontor#101",
    );
    let before = realm.world.fake.calls().len();
    let closed = close(&realm, MODULE_REPOSITORY, 101, "prr-lost-ack-close").await;
    assert_eq!(closed.status, 200, "{}", closed.body);
    let mut retired = retirements(&calls_since(&realm, before));
    retired.sort();
    let mut expected: Vec<_> = seats.iter().map(|seat| seat.id).collect();
    expected.sort();
    assert_eq!(retired, expected);
    for seat in &seats {
        assert!(!seat_row(&realm, seat.id).is_non_terminal());
    }
}

/// The same realm after a daemon restart on the same state root.
async fn restarted(realm: Review) -> Review {
    let Review {
        world,
        project,
        project_id,
        epic,
        epic_key,
        task_key,
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
    Review {
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
        epic,
        epic_key,
        task_key,
    }
}
