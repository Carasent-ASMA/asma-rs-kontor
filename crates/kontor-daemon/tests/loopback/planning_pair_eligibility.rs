//! ASMA-8282 B1a: the trusted daemon path of the frozen-caller eligibility
//! and frozen member context decisions the shared runtime core now makes.
//!
//! These authenticate real seat credentials against real stored state; the
//! runtime core's own tests use hypothetical facts instead. Each refusal here
//! is compared whole, as the exact body the service built before the move:
//! `ApiError::new(realm, code, rule)` with that code's status and action.

use super::*;
use kontor_api::error::{ApiError, ApiErrorCode};

const TICKET_ONLY_PROFILE: &str = "01991c00-0000-7000-8000-0000000000b8";
const FENCED: &str = "the caller credential belongs to a fenced native occupancy generation";
const NOT_FROZEN_CALLER: &str =
    "only the planning pair's frozen caller asks for clarification or records the disposition";

/// `answer` is exactly the refusal the service built as `code` and `rule`.
fn assert_refused(world: &World, answer: &Answer, code: ApiErrorCode, rule: &'static str) {
    let expected = ApiError::new(world.realm_id(), code, rule);
    assert_eq!(
        answer.status.as_u16(),
        code.status().as_u16(),
        "{rule}: {}",
        answer.body
    );
    assert_eq!(
        answer.json(),
        serde_json::to_value(expected.body()).expect("a refusal body"),
        "{rule}"
    );
}

fn execute(world: &World, statement: &str, params: impl rusqlite::Params) -> usize {
    rusqlite::Connection::open(world.directory.path().join("kontor.db"))
        .expect("the realm database opens")
        .execute(statement, params)
        .expect("the fixture state is written")
}

/// The caller's own eligibility, refusal by refusal in the service's order,
/// then the positive path at its current generation, then the fence: a
/// retired generation never replays even its own admitted invocation.
#[tokio::test]
async fn the_caller_is_held_to_scope_seat_node_generation_and_role_exactly_as_before() {
    let realm = pair_realm("/tmp/kontor-asma8282-b1a-caller").await;
    let world = &realm.world;
    let caller = realm.caller.to_string();
    let body = realm.invoke_body(&realm.profile, "Eligibility plan").await;

    let tpm = realm.seat_token(realm.tpm, realm.hosted_generation(realm.tpm));
    let refused = realm.invoke_with(&body, tpm.clone(), "b1a-tpm").await;
    assert_refused(
        world,
        &refused,
        ApiErrorCode::Forbidden,
        "the authenticated seat's role may not convene this planning pair",
    );

    let issued = realm.hosted_generation(realm.caller);
    let unissued = realm
        .invoke_with(
            &body,
            realm.seat_token(realm.caller, issued + 1),
            "b1a-unissued",
        )
        .await;
    assert_refused(world, &unissued, ApiErrorCode::StaleBinding, FENCED);
    // Two faults answer the earlier stage: the generation before the role.
    let tpm_unissued = realm.seat_token(realm.tpm, realm.hosted_generation(realm.tpm) + 1);
    let both = realm
        .invoke_with(&body, tpm_unissued.clone(), "b1a-tpm-unissued")
        .await;
    assert_refused(world, &both, ApiErrorCode::StaleBinding, FENCED);

    let mut ticket_only = pair_document(TICKET_ONLY_PROFILE, PAIR_KIND);
    ticket_only["allowed_scopes"] = serde_json::json!(["ticket"]);
    let ticket_profile =
        publish_pair_document(world, &realm.project, &ticket_only, "b1a-ticket-only").await;
    let epic_scoped = realm
        .invoke_with(
            &realm.invoke_body(&ticket_profile, "Eligibility plan").await,
            realm.caller_token(),
            "b1a-epic-scope",
        )
        .await;
    assert_refused(
        world,
        &epic_scoped,
        ApiErrorCode::Forbidden,
        "the pinned planning pair document does not permit epic-scoped invocation",
    );
    // The scope before the seat's generation and role.
    let scoped_first = realm
        .invoke_with(
            &realm.invoke_body(&ticket_profile, "Eligibility plan").await,
            tpm_unissued,
            "b1a-tpm-epic-scope",
        )
        .await;
    assert_refused(
        world,
        &scoped_first,
        ApiErrorCode::Forbidden,
        "the pinned planning pair document does not permit epic-scoped invocation",
    );

    let seat_lifecycle = "UPDATE seat_bindings SET lifecycle = ?1 WHERE id = ?2";
    assert_eq!(
        execute(world, seat_lifecycle, rusqlite::params!["retired", caller]),
        1
    );
    let inactive = realm
        .invoke_with(&body, realm.caller_token(), "b1a-seat-inactive")
        .await;
    assert_refused(
        world,
        &inactive,
        ApiErrorCode::StaleBinding,
        "the authenticated caller seat is not active",
    );
    assert_eq!(
        execute(world, seat_lifecycle, rusqlite::params!["active", caller]),
        1
    );

    let seat_node = "UPDATE seat_bindings SET topology_node_id = ?1 WHERE id = ?2";
    let own_node: String = rusqlite::Connection::open(world.directory.path().join("kontor.db"))
        .expect("the realm database opens")
        .query_row(
            "SELECT topology_node_id FROM seat_bindings WHERE id = ?1",
            [&caller],
            |row| row.get(0),
        )
        .expect("the caller node reads");
    assert_eq!(
        execute(
            world,
            "UPDATE seat_bindings SET topology_node_id =
                 (SELECT id FROM topology_nodes WHERE mini_project_id IS NULL AND parent_id IS NULL)
             WHERE id = ?1",
            [&caller],
        ),
        1
    );
    let foreign = realm
        .invoke_with(&body, realm.caller_token(), "b1a-seat-outside")
        .await;
    assert_refused(
        world,
        &foreign,
        ApiErrorCode::Forbidden,
        "the authenticated caller seat does not belong to this epic",
    );
    assert_eq!(
        execute(world, seat_node, rusqlite::params![own_node, caller]),
        1
    );

    let node_lifecycle = "UPDATE topology_nodes SET lifecycle = ?1 WHERE id = ?2";
    assert_eq!(
        execute(
            world,
            node_lifecycle,
            rusqlite::params!["retired", own_node]
        ),
        1
    );
    let retired_node = realm
        .invoke_with(&body, realm.caller_token(), "b1a-node-inactive")
        .await;
    assert_refused(
        world,
        &retired_node,
        ApiErrorCode::StaleBinding,
        "the authenticated caller seat's topology node is not active",
    );
    // The node before the generation, and the seat before the node.
    let node_first = realm
        .invoke_with(
            &body,
            realm.seat_token(realm.caller, issued + 1),
            "b1a-node-unissued",
        )
        .await;
    assert_refused(
        world,
        &node_first,
        ApiErrorCode::StaleBinding,
        "the authenticated caller seat's topology node is not active",
    );
    assert_eq!(
        execute(world, seat_lifecycle, rusqlite::params!["retired", caller]),
        1
    );
    let seat_first = realm
        .invoke_with(&body, realm.caller_token(), "b1a-seat-node")
        .await;
    assert_refused(
        world,
        &seat_first,
        ApiErrorCode::StaleBinding,
        "the authenticated caller seat is not active",
    );
    assert_eq!(
        execute(world, seat_lifecycle, rusqlite::params!["active", caller]),
        1
    );
    assert_eq!(
        execute(world, node_lifecycle, rusqlite::params!["active", own_node]),
        1
    );
    assert_eq!(realm.planning_pair_runs(), 0, "no refusal froze a run");

    // The positive trusted path: the frozen caller at its current generation,
    // and the exact replay of that same request.
    let body = realm.invoke_body(&realm.profile, "Eligibility plan").await;
    let admitted_token = realm.caller_token();
    let admitted = realm
        .invoke_with(&body, admitted_token.clone(), "b1a-invoke")
        .await;
    assert_eq!(admitted.status, 200, "{}", admitted.body);
    let replayed = realm
        .invoke_with(&body, admitted_token.clone(), "b1a-invoke")
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    let run = admitted.json();
    let pair = Pair {
        run: run["planning_pair_run_id"]
            .as_str()
            .expect("a planning pair run id")
            .to_owned(),
        seat_a: SeatBindingId::generate(),
        seat_b: SeatBindingId::generate(),
        invoked: run,
    };
    let ask = serde_json::json!({
        "question": "Which step reverts alone?",
        "addressed": ["seat-a"],
        "expected_revision": pair.invoked["revision"],
    });
    let dispose = serde_json::json!({
        "members": [], "rationale": "not the caller",
        "expected_revision": pair.invoked["revision"],
    });
    for (suffix, decision) in [
        ("/clarification:request", &ask),
        ("/disposition:record", &dispose),
    ] {
        let other = realm
            .write(&pair, suffix, decision, tpm.clone(), "b1a-tpm-decides")
            .await;
        assert_refused(world, &other, ApiErrorCode::Forbidden, NOT_FROZEN_CALLER);
    }

    // Authentication before replay: the fenced generation is refused on the
    // very key and body it was admitted under, and on a decision.
    retire_caller_generation(&realm);
    let fenced_replay = realm
        .invoke_with(&body, admitted_token.clone(), "b1a-invoke")
        .await;
    assert_refused(world, &fenced_replay, ApiErrorCode::StaleBinding, FENCED);
    let fenced_ask = realm
        .write(
            &pair,
            "/clarification:request",
            &ask,
            admitted_token,
            "b1a-fenced-ask",
        )
        .await;
    assert_refused(world, &fenced_ask, ApiErrorCode::StaleBinding, FENCED);
    let current = realm.read_with(&pair, Some(realm.caller_token())).await;
    assert_eq!(current.status, 200, "{}", current.body);
    assert_eq!(current.json()["viewer"], "caller");

    // Last, since it removes the caller's hosting: no hosted occupancy.
    let token = realm.caller_token();
    assert_eq!(
        execute(
            world,
            "DELETE FROM hosted_topology_seats WHERE seat_binding_id = ?1",
            [&caller],
        ),
        1
    );
    let unhosted = realm
        .write(&pair, "/clarification:request", &ask, token, "b1a-unhosted")
        .await;
    assert_refused(
        world,
        &unhosted,
        ApiErrorCode::StaleBinding,
        "the authenticated caller seat has no active hosted native occupancy",
    );
}

/// Only the frozen caller recovers a member; another seat of the epic, at
/// its own current generation, is refused before any member is read.
#[tokio::test]
async fn only_the_frozen_caller_recovers_exactly_as_before() {
    use kontor_runtime::planning_pair::MandatoryMemberField;
    let realm = pair_realm("/tmp/kontor-asma8282-b1a-recover").await;
    let world = &realm.world;
    let (pair, _) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::ToolRestrictions,
        "Recovery plan",
        "b1a-recover-invoke",
    )
    .await;
    let body = realm.recover_body(&pair, pair.seat_b).await;
    world.fake.take_calls();
    let tpm = realm.seat_token(realm.tpm, realm.hosted_generation(realm.tpm));
    let refused = realm
        .recover_with(&pair, pair.seat_b, &body, tpm, "b1a-tpm-recovers")
        .await;
    assert_refused(
        world,
        &refused,
        ApiErrorCode::Forbidden,
        "only the planning pair's frozen caller recovers one of its members",
    );
    assert!(member_effects(&world.fake.take_calls()).is_empty());
}

/// The daemon's `member_context_hash` document at `be9537aa`, copied
/// verbatim, over a context the runtime actually received.
fn baseline_context_hash(
    context: &kontor_runtime::planning_pair::PlanningPairLaunchContext,
) -> ContentHash {
    kontor_core::id::CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "planning_pair_run_id": context.run_id.to_string(),
        "seat_binding_id": context.seat_binding_id.to_string(),
        "slot": context.slot.as_str(),
        "occupancy_generation": context.occupancy_generation,
        "profile": [
            context.profile.profile_id.to_string(),
            context.profile.version.get(),
            context.profile.definition_hash.as_str(),
        ],
        "topology": [
            context.topology.spec_id.to_string(),
            context.topology.version.get(),
            context.topology.canonical_hash.as_str(),
        ],
        "team_definition": [
            context.team_definition.definition_id.to_string(),
            context.team_definition.version.get(),
            context.team_definition.canonical_hash.as_str(),
        ],
        "role_catalog": [
            context.role_catalog.catalog_id.to_string(),
            context.role_catalog.version.get(),
            context.role_catalog.canonical_hash.as_str(),
        ],
        "topology_node_id": context.topology_node_id.to_string(),
        "cwd": context.cwd.as_str(),
        "route": context.route,
        "vendor": context.vendor,
        "placement_hash": context.placement_hash.as_str(),
        "requested_fleet_provenance": context.requested_fleet_provenance,
    }))
    .expect("a canonical document")
    .hash()
    .clone()
}

/// A frozen pair resumes its member launches through the one derivation:
/// each drift of a fact it reads is refused before any member effect, and
/// once restored, every member's known claim names the baseline hash of the
/// exact context its launch carried. A zero member generation is not staged:
/// storage's own `occupancy_generation >= 1` check refuses it first.
#[tokio::test]
async fn a_resumed_launch_refuses_each_context_drift_and_claims_the_baseline_hash() {
    let realm = pair_realm("/tmp/kontor-asma8282-b1a-context").await;
    let world = &realm.world;
    let seat_a_slot = kontor_core::id::RoleSlotId::parse("seat-a").expect("a slot");
    world.fake.refusing_launch_of(&seat_a_slot);
    let body = realm.invoke_body(&realm.profile, "Context plan").await;
    let interrupted = realm
        .invoke_with(&body, realm.caller_token(), "b1a-context")
        .await;
    assert_ne!(interrupted.status, 200, "{}", interrupted.body);
    world.fake.allowing_launch_of(&seat_a_slot);
    let run = realm.world.daemon.state().with_store(|store| {
        store
            .list_consultation_runs(
                realm.project_id,
                MiniProjectId::parse(&realm.epic).expect("an epic id"),
                ConsultationFamily::PlanningPair,
            )
            .expect("the runs read")
            .into_iter()
            .find(|run| run.invoke_key.as_str() == "b1a-context")
            .expect("the frozen pair")
    });
    let seats = world.daemon.state().with_store(|store| {
        store
            .list_consultation_seats(realm.project_id, run.id)
            .expect("the seats read")
    });
    let seat = |slot: &str| {
        seats
            .iter()
            .find(|seat| seat.role_slot_id.as_str() == slot)
            .expect("the member seat")
            .seat_binding_id
            .to_string()
    };
    let (seat_a, seat_b) = (seat("seat-a"), seat("seat-b"));
    let container = run.topology_node_id.to_string();
    let read = |query: &str, key: &str| -> String {
        rusqlite::Connection::open(world.directory.path().join("kontor.db"))
            .expect("the realm database opens")
            .query_row(query, [key], |row| row.get(0))
            .expect("the fixture state reads")
    };
    let route_a = read(
        "SELECT model_rung FROM consultation_seats WHERE seat_binding_id = ?1",
        &seat_a,
    );
    let route_b = read(
        "SELECT model_rung FROM consultation_seats WHERE seat_binding_id = ?1",
        &seat_b,
    );
    let team_hash = read(
        "SELECT canonical_hash FROM mini_project_team_definition_snapshots
         WHERE mini_project_id = ?1",
        &realm.epic,
    );
    // A real successor revision of the pinned Team Definition, so the epic's
    // pin still names published bytes, only not the ones the pair froze.
    let successor_hash = world.daemon.state().with_store(|store| {
        let pinned = store
            .get_mini_project_team_definition(realm.project_id, run.mini_project_id)
            .expect("the pin reads")
            .expect("the epic is pinned")
            .definition;
        let mut successor = store
            .get_team_definition(realm.project_id, pinned.definition_id, pinned.version)
            .expect("the pinned revision reads")
            .expect("the pinned revision exists");
        successor.version = pinned.version.next().expect("a successor version");
        store
            .publish_team_definition(realm.project_id, &successor, kontor_api::now())
            .expect("the successor revision is published")
            .as_str()
            .to_owned()
    });
    let other_hash = ContentHash::of(b"drifted").as_str().to_owned();
    let seat_route = "UPDATE consultation_seats SET model_rung = ?1 WHERE seat_binding_id = ?2";
    let repin = "UPDATE mini_project_team_definition_snapshots
                 SET version = version + 1, canonical_hash = ?1 WHERE mini_project_id = ?2";
    let unpin = "UPDATE mini_project_team_definition_snapshots
                 SET version = version - 1, canonical_hash = ?1 WHERE mini_project_id = ?2";
    let container_hash = read(
        "SELECT spec_hash FROM topology_nodes WHERE id = ?1",
        &container,
    );
    let container_topology = "UPDATE topology_nodes SET spec_hash = ?1 WHERE id = ?2";

    type Write<'a> = (&'a str, [&'a str; 2]);
    let drifts: [(Write, Write, &'static str); 3] = [
        (
            (seat_route, [&route_b, &seat_a]),
            (seat_route, [&route_a, &seat_a]),
            "the member seat's route differs from the route its placement froze",
        ),
        (
            (container_topology, [&other_hash, &container]),
            (container_topology, [&container_hash, &container]),
            "the member container's topology differs from the epic's pinned topology",
        ),
        (
            (repin, [&successor_hash, &realm.epic]),
            (unpin, [&team_hash, &realm.epic]),
            "the epic's pinned Team Definition differs from the one the pair was frozen under",
        ),
    ];
    for ((drift, drift_params), (restore, restore_params), rule) in drifts {
        assert_eq!(execute(world, drift, drift_params), 1, "{rule}");
        world.fake.take_calls();
        let resumed = realm
            .invoke_with(&body, realm.caller_token(), "b1a-context")
            .await;
        assert_refused(world, &resumed, ApiErrorCode::PlacementBlocked, rule);
        let calls = world.fake.take_calls();
        assert!(
            member_effects(&calls).is_empty(),
            "{rule}: refused before any member effect: {calls:?}"
        );
        assert_eq!(execute(world, restore, restore_params), 1, "{rule}");
    }

    let resumed = realm
        .invoke_with(&body, realm.caller_token(), "b1a-context")
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    let pair = Pair {
        run: resumed.json()["planning_pair_run_id"]
            .as_str()
            .expect("a planning pair run id")
            .to_owned(),
        seat_a: SeatBindingId::parse(&seat_a).expect("a seat"),
        seat_b: SeatBindingId::parse(&seat_b).expect("a seat"),
        invoked: resumed.json(),
    };
    let launched = world.fake.planning_pair_launch_contexts();
    for seat in [pair.seat_a, pair.seat_b] {
        let context = launched.get(&seat).expect("the member launched");
        let claim = realm.known_claim(&pair, seat).expect("a known claim");
        assert_eq!(claim.context_hash, baseline_context_hash(context), "{seat}");
    }
}
