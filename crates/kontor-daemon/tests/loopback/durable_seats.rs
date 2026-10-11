//! Durable desk seats through the supported daemon operations (ASMA-8451).
//!
//! Every seat here lives in a disposable realm on the fake runtime. The two
//! ASMA-8419 PR REVIEW seats are declared as the Paseo-direct seats they are
//! and are never censused, adopted or restarted. Daemon-owned fixture seats
//! carry the launch, replay, concurrency, lost-answer and restart cases: only
//! a selected seat an authoritative census proved missing is ever launched,
//! and it is launched once.

use super::durable_desks::{desk_call, desk_world, select_desk_definition};
use super::*;
use kontor_core::durable_seat::{DurableSeatActivity, DurableSeatCarrier};
use kontor_core::receipt::CommandKind;
use kontor_runtime::adapter::RuntimeError;

#[path = "qualification_seats.rs"]
mod qualification_seats;

const ROUTE: &str = r#"{"provider": "codex", "model": "gpt-5.6-sol", "effort": "xhigh"}"#;

/// A realm whose `pr-review` desk is materialized, with no epic anywhere.
async fn seat_world(slug: &str) -> (World, String) {
    let (world, project) = desk_world(slug).await;
    select_desk_definition(&world, &project).await;
    let materialized = desk_call(
        &world,
        &project,
        "materialize",
        "pr-review",
        &format!("{slug}-materialize"),
    )
    .await;
    assert_eq!(materialized.status, 200, "{}", materialized.body);
    (world, project)
}

fn seats_path(project: &str, desk: &str) -> String {
    format!("/v1/projects/{project}/desks/{desk}/seats")
}

/// One of the two ASMA-8419 seats, declared exactly as it is run today.
fn paseo_direct_seat(seat_key: &str, label: &str, procedure: &str) -> serde_json::Value {
    serde_json::json!({
        "seat_key": seat_key,
        "role_code": "LSA",
        "seat_label": label,
        "procedure": procedure,
        "recovery_owner": "paseo_direct",
        "decided_by": "Igor",
        "decided_on": "2026-10-08",
    })
}

/// A disposable fixture seat Kontor may restart.
fn daemon_seat(seat_key: &str, label: &str) -> serde_json::Value {
    serde_json::json!({
        "seat_key": seat_key,
        "role_code": "LSA",
        "seat_label": label,
        "model_route": serde_json::from_str::<serde_json::Value>(ROUTE).expect("route"),
        "recovery_owner": "daemon",
        "decided_by": "Igor",
        "decided_on": "2026-10-11",
    })
}

async fn declare_on(
    world: &World,
    project: &str,
    desk: &str,
    seat: &serde_json::Value,
    key: &str,
) -> Answer {
    let mut body = seat.clone();
    body["expected_revision"] = serde_json::json!(current_project_revision(world, project).await);
    Call::post(format!("{}:declare", seats_path(project, desk)), &body)
        .signed_as(world, "operator")
        .with_key(key)
        .send(world)
        .await
}

async fn declare(world: &World, project: &str, seat: &serde_json::Value, key: &str) -> Answer {
    declare_on(world, project, "pr-review", seat, key).await
}

/// Declare one seat and return its planned SeatBinding, when it has one.
async fn declared(world: &World, project: &str, seat: &serde_json::Value) -> Option<SeatBindingId> {
    let key = format!("declare-{}", seat["seat_key"].as_str().expect("a key"));
    let answer = declare(world, project, seat, &key).await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    answer.json()["seat"]["seat_binding_id"]
        .as_str()
        .map(|id| SeatBindingId::parse(id).expect("a binding id"))
}

async fn reconcile(
    world: &World,
    project: &str,
    seats: &[&str],
    restart_missing: bool,
    key: &str,
) -> Answer {
    Call::post(
        format!("{}:reconcile", seats_path(project, "pr-review")),
        &serde_json::json!({
            "expected_revision": current_project_revision(world, project).await,
            "seat_keys": seats,
            "restart_missing": restart_missing,
        }),
    )
    .signed_as(world, "operator")
    .with_key(key)
    .send(world)
    .await
}

async fn list(world: &World, project: &str) -> serde_json::Value {
    let read = Call::get(seats_path(project, "pr-review"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(read.status, 200, "{}", read.body);
    read.json()
}

/// One seat of a report or a reconciliation, by key.
fn seat<'a>(answer: &'a serde_json::Value, seat_key: &str) -> &'a serde_json::Value {
    answer["seats"]
        .as_array()
        .expect("seats")
        .iter()
        .find(|seat| seat["seat_key"] == seat_key)
        .unwrap_or_else(|| panic!("{seat_key} is not in {answer}"))
}

fn observed(answer: &serde_json::Value, seat_key: &str) -> (String, String, String) {
    let seat = seat(answer, seat_key);
    (
        seat["status"].as_str().expect("a status").to_owned(),
        seat["reason"].as_str().expect("a reason").to_owned(),
        seat["decision"].as_str().expect("a decision").to_owned(),
    )
}

fn expect(status: &str, reason: &str, decision: &str) -> (String, String, String) {
    (status.to_owned(), reason.to_owned(), decision.to_owned())
}

fn launches_since(world: &World, before: usize) -> usize {
    world.fake.calls()[before..]
        .iter()
        .filter(|call| matches!(call, AdapterCall::LaunchHostedSeat(_)))
        .count()
}

fn native(answer: &serde_json::Value, seat_key: &str) -> ExternalId {
    ExternalId::parse(
        seat(answer, seat_key)["native_id"]
            .as_str()
            .expect("a native id"),
    )
    .expect("an external id")
}

fn receipt_kind(world: &World, key: &str) -> Option<CommandKind> {
    world.daemon.state().with_store(|store| {
        store
            .get_receipt_by_key(&IdempotencyKey::parse(key).expect("a key"))
            .expect("the receipt reads")
            .map(|receipt| receipt.kind)
    })
}

#[tokio::test]
async fn the_asma_8419_seats_are_declared_paseo_direct_and_never_touched() {
    let (world, project) = seat_world("seats-paseo-direct").await;
    let general = paseo_direct_seat(
        "general",
        "GENERAL",
        ".github/skills/asma-pr-review/SKILL.md",
    );
    let checker = paseo_direct_seat(
        "pr-drafts-status-checker",
        "PR DRAFTS STATUS CHECKER",
        ".github/skills/pr-draft-status-checker/SKILL.md",
    );
    let calls_before = world.fake.calls().len();
    for (body, title) in [
        (&general, "LSA • GENERAL"),
        (&checker, "LSA • PR DRAFTS STATUS CHECKER"),
    ] {
        let key = format!("declare-{}", body["seat_key"].as_str().expect("key"));
        let answer = declare(&world, &project, body, &key).await;
        assert_eq!(answer.status, 200, "{}", answer.body);
        assert_eq!(answer.json()["seat"]["title"], title);
        assert_eq!(answer.json()["seat"]["recovery_owner"], "paseo_direct");
        assert!(answer.json()["seat"]["seat_binding_id"].is_null());
        assert_eq!(answer.json()["receipt"]["applied"], "created");
        assert_eq!(
            receipt_kind(&world, &key),
            Some(CommandKind::DeclareDurableSeat)
        );
        let replayed = declare(&world, &project, body, &key).await;
        assert_eq!(replayed.status, 200, "{}", replayed.body);
        assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    }

    // The report reads the declarations alone. The runtime is not asked.
    let report = list(&world, &project).await;
    for seat_key in ["general", "pr-drafts-status-checker"] {
        assert_eq!(
            observed(&report, seat_key),
            expect("unchecked", "paseo_direct_owner", "none")
        );
    }
    let reported = reconcile(
        &world,
        &project,
        &["general", "pr-drafts-status-checker"],
        false,
        "paseo-direct-report",
    )
    .await;
    assert_eq!(reported.status, 200, "{}", reported.body);
    assert_eq!(
        observed(&reported.json(), "general"),
        expect("unchecked", "paseo_direct_owner", "none")
    );

    // Selecting one for a restart is a request to adopt it: refused before
    // the runtime is consulted and before anything is recorded.
    let refused = reconcile(&world, &project, &["general"], true, "paseo-direct-restart").await;
    assert_eq!(refused.json()["code"], "forbidden", "{}", refused.body);
    assert_eq!(receipt_kind(&world, "paseo-direct-restart"), None);
    assert!(
        world.fake.calls()[calls_before..].is_empty(),
        "a Paseo-direct seat reached the runtime: {:?}",
        &world.fake.calls()[calls_before..]
    );
}

#[tokio::test]
async fn a_declaration_or_selection_that_is_not_one_exact_decision_is_refused() {
    let (world, project) = seat_world("seats-refusals").await;
    let general = paseo_direct_seat(
        "general",
        "GENERAL",
        ".github/skills/asma-pr-review/SKILL.md",
    );
    assert!(declared(&world, &project, &general).await.is_none());

    // The same decision under a fresh key is the same seat.
    let again = declare(&world, &project, &general, "declare-general-again").await;
    assert_eq!(again.status, 200, "{}", again.body);
    assert_eq!(again.json()["receipt"]["applied"], "unchanged");

    let mut amended = general.clone();
    amended["decided_by"] = serde_json::json!("Adam");
    let conflict = declare(&world, &project, &amended, "declare-general-amended").await;
    assert_eq!(conflict.status, 409, "{}", conflict.body);

    let same_title = daemon_seat("general-again", "GENERAL");
    let conflict = declare(&world, &project, &same_title, "declare-same-title").await;
    assert_eq!(
        conflict.status, 409,
        "one title names one seat: {}",
        conflict.body
    );

    let mut routeless = daemon_seat("routeless", "ROUTELESS");
    routeless
        .as_object_mut()
        .expect("object")
        .remove("model_route");
    let mut unknown_role = daemon_seat("unknown-role", "UNKNOWN ROLE");
    unknown_role["role_code"] = serde_json::json!("ZZZ");
    let mut future = daemon_seat("future", "FUTURE");
    future["decided_on"] = serde_json::json!("2099-01-01");
    let mut escaping = paseo_direct_seat("escaping", "ESCAPING", "../outside/SKILL.md");
    escaping["decided_on"] = serde_json::json!("2026-10-08");
    let mut chain = daemon_seat("chain", "CHAIN");
    chain["model_route"] = serde_json::json!({"provider": "premium", "model": "premium"});
    for (body, why) in [
        (&routeless, "a daemon-owned seat declares its exact route"),
        (&unknown_role, "the role is a catalog role"),
        (&future, "a decision is not dated in the future"),
        (&escaping, "a procedure is a plain repository path"),
        (&chain, "a named chain is not an exact route"),
    ] {
        let key = format!("declare-{}", body["seat_key"].as_str().expect("key"));
        let refused = declare(&world, &project, body, &key).await;
        assert_eq!(refused.status, 400, "{why}: {}", refused.body);
        assert_eq!(receipt_kind(&world, &key), None, "{why}");
    }
    let deskless = declare_on(&world, &project, "nowhere", &general, "declare-deskless").await;
    assert_eq!(deskless.status, 404, "{}", deskless.body);
    assert_eq!(
        list(&world, &project).await["seats"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    for (seats, status) in [
        (&[][..], 400),
        (&["general", "general"][..], 400),
        (&["undeclared"][..], 404),
    ] {
        let refused = reconcile(&world, &project, seats, false, "bad-selection").await;
        assert_eq!(refused.status, status, "{seats:?}: {}", refused.body);
    }
    let stale = Call::post(
        format!("{}:reconcile", seats_path(&project, "pr-review")),
        &serde_json::json!({
            "expected_revision": current_project_revision(&world, &project).await + 1,
            "seat_keys": ["general"],
            "restart_missing": false,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("stale-selection")
    .send(&world)
    .await;
    assert_eq!(stale.json()["code"], "revision_conflict", "{}", stale.body);
    assert_eq!(receipt_kind(&world, "bad-selection"), None);
}

#[tokio::test]
async fn a_missing_daemon_seat_launches_once_and_a_replay_never_launches_again() {
    let (world, project) = seat_world("seats-launch").await;
    let binding = declared(&world, &project, &daemon_seat("triage", "TRIAGE"))
        .await
        .expect("a daemon-owned seat plans its binding");

    // Proved missing, and only reported.
    let calls_before = world.fake.calls().len();
    assert_eq!(
        observed(&list(&world, &project).await, "triage"),
        expect("missing", "never_launched", "none")
    );
    let reported = reconcile(&world, &project, &["triage"], false, "triage-report").await;
    assert_eq!(reported.status, 200, "{}", reported.body);
    assert_eq!(
        observed(&reported.json(), "triage"),
        expect("missing", "never_launched", "none")
    );
    assert_eq!(
        receipt_kind(&world, "triage-report"),
        Some(CommandKind::ReconcileDurableSeats)
    );
    assert_eq!(launches_since(&world, calls_before), 0, "report only");
    world.daemon.state().with_store(|store| {
        assert!(
            store
                .get_seat_binding(ProjectId::parse(&project).expect("id"), binding)
                .expect("reads")
                .is_none(),
            "reporting creates no binding"
        );
    });

    let minted = world.fake.minted_natives();
    let launched = reconcile(&world, &project, &["triage"], true, "triage-launch").await;
    assert_eq!(launched.status, 200, "{}", launched.body);
    assert_eq!(launched.json()["receipt"]["applied"], "created");
    assert_eq!(
        observed(&launched.json(), "triage"),
        expect("missing", "never_launched", "launch")
    );
    assert_eq!(seat(&launched.json(), "triage")["occupancy_generation"], 1);
    let first = native(&launched.json(), "triage");
    assert_eq!(launches_since(&world, calls_before), 1);
    assert_eq!(world.fake.minted_natives(), minted + 1);
    assert_eq!(
        world.fake.hosted_seat_native_id(binding),
        Some(first.clone())
    );
    assert_eq!(
        world.fake.seat_title(&first).as_deref(),
        Some("LSA • TRIAGE")
    );
    assert_eq!(
        observed(&list(&world, &project).await, "triage"),
        expect("healthy", "idle", "none")
    );

    // A replay is the recorded reconciliation and no second launch; a fresh
    // key finds the seat healthy and launches nothing either.
    let replayed = reconcile(&world, &project, &["triage"], true, "triage-launch").await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        observed(&replayed.json(), "triage"),
        expect("healthy", "idle", "none")
    );
    let fresh = reconcile(&world, &project, &["triage"], true, "triage-again").await;
    assert_eq!(
        observed(&fresh.json(), "triage"),
        expect("healthy", "idle", "none")
    );
    assert_eq!(launches_since(&world, calls_before), 1);

    // The seat vanishes. Replaying the recorded reconciliation still acts on
    // nothing: what happened since is a question for a new key.
    world.fake.forget_seat(&first);
    let replayed = reconcile(&world, &project, &["triage"], true, "triage-launch").await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert_eq!(
        observed(&replayed.json(), "triage"),
        expect("missing", "native_absent", "none")
    );
    assert_eq!(
        launches_since(&world, calls_before),
        1,
        "the replay fence held"
    );
    assert_eq!(world.fake.minted_natives(), minted + 1);

    // A new key restarts it as the next occupancy, with the absent native
    // fenced in the seat's history.
    let restarted = reconcile(&world, &project, &["triage"], true, "triage-restart").await;
    assert_eq!(restarted.status, 200, "{}", restarted.body);
    assert_eq!(
        observed(&restarted.json(), "triage"),
        expect("missing", "native_absent", "launch")
    );
    assert_eq!(seat(&restarted.json(), "triage")["occupancy_generation"], 2);
    let second = native(&restarted.json(), "triage");
    assert_ne!(second, first);
    assert_eq!(
        world.fake.hosted_launch_fence(binding),
        Some(vec![first.clone()]),
        "the restart named the absent native as its fenced predecessor"
    );
    assert_eq!(launches_since(&world, calls_before), 2);
    assert_eq!(world.fake.minted_natives(), minted + 2);
    world.daemon.state().with_store(|store| {
        assert_eq!(
            store
                .list_hosted_topology_seat_history_native_ids(
                    ProjectId::parse(&project).expect("id"),
                    binding
                )
                .expect("reads"),
            vec![first.clone()]
        );
    });
    assert_eq!(
        observed(&list(&world, &project).await, "triage"),
        expect("healthy", "idle", "none")
    );

    // The current occupant vanishes and the fenced one comes back. That is a
    // retired native resurfacing, not a vacancy, and nothing is launched.
    world.fake.forget_seat(&second);
    world.fake.add_hosted_seat_carrier(
        binding,
        DurableSeatCarrier {
            native_id: first.clone(),
            archived: false,
            activity: DurableSeatActivity::Idle,
        },
    );
    let resurfaced = reconcile(&world, &project, &["triage"], true, "triage-resurfaced").await;
    assert_eq!(resurfaced.status, 200, "{}", resurfaced.body);
    assert_eq!(
        observed(&resurfaced.json(), "triage"),
        expect("mismatched", "predecessor_live", "none")
    );
    assert_eq!(launches_since(&world, calls_before), 2);
}

#[tokio::test]
async fn two_concurrent_reconciliations_launch_a_missing_seat_once() {
    let (world, project) = seat_world("seats-concurrent").await;
    let binding = declared(&world, &project, &daemon_seat("triage", "TRIAGE"))
        .await
        .expect("a binding");
    let calls_before = world.fake.calls().len();
    let minted = world.fake.minted_natives();
    let pause = world.fake.pause_next_hosted_launch();

    // The first call is held at the runtime boundary. The second starts while
    // it is held and gets the whole hold to reach the runtime too.
    let first = reconcile(&world, &project, &["triage"], true, "concurrent-first");
    let second = async {
        pause.entered().await;
        reconcile(&world, &project, &["triage"], true, "concurrent-second").await
    };
    let release = async {
        pause.entered().await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        pause.release();
    };
    let (first, second, ()) = tokio::join!(first, second, release);
    assert_eq!(first.status, 200, "{}", first.body);
    assert_eq!(second.status, 200, "{}", second.body);
    assert_eq!(
        launches_since(&world, calls_before),
        1,
        "two reconciliations reached the runtime"
    );
    assert_eq!(world.fake.minted_natives(), minted + 1);
    assert_eq!(
        observed(&first.json(), "triage"),
        expect("missing", "never_launched", "launch")
    );
    assert_eq!(
        observed(&second.json(), "triage"),
        expect("healthy", "idle", "none")
    );
    assert_eq!(
        native(&first.json(), "triage"),
        native(&second.json(), "triage")
    );
    assert_eq!(
        world.fake.hosted_seat_native_id(binding),
        Some(native(&first.json(), "triage"))
    );
}

#[tokio::test]
async fn a_lost_launch_answer_is_recovered_by_the_same_native_after_a_restart() {
    let (world, project) = seat_world("seats-lost-answer").await;
    let project_id = ProjectId::parse(&project).expect("a project id");
    let binding = declared(&world, &project, &daemon_seat("triage", "TRIAGE"))
        .await
        .expect("a binding");
    let minted = world.fake.minted_natives();
    world.fake.lose_next_hosted_launch_ack();
    let lost = reconcile(&world, &project, &["triage"], true, "lost-launch").await;
    assert_ne!(lost.status, 200, "{}", lost.body);
    assert_eq!(receipt_kind(&world, "lost-launch"), None);
    assert_eq!(world.fake.minted_natives(), minted + 1, "the native exists");
    let created = world
        .fake
        .hosted_seat_native_id(binding)
        .expect("the runtime holds the native");
    world.daemon.state().with_store(|store| {
        assert!(
            store
                .get_hosted_topology_seat(project_id, binding)
                .expect("reads")
                .is_none(),
            "nothing was bound"
        );
        assert!(
            store
                .get_hosted_seat_launch_intent(project_id, binding, 1)
                .expect("reads")
                .is_some(),
            "the prepared intent survived"
        );
    });
    assert_eq!(
        observed(&list(&world, &project).await, "triage"),
        expect("unchecked", "launch_unconfirmed", "none")
    );

    // The daemon restarts. The runtime still holds the native it never heard
    // about.
    let operator = secret(&world, "operator");
    let observer = secret(&world, "observer");
    let World {
        directory,
        daemon,
        router,
        fake,
        ..
    } = world;
    let state_root = directory.path().to_owned();
    daemon.state().signals().stop();
    drop(router);
    drop(daemon);
    fake.rebuild_adapter_state();
    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new().with(
            fake_family(),
            Arc::clone(&fake) as Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
        ),
    )
    .expect("the same state root reopens");
    assert_eq!(restarted.reconcile().await, BarrierState::Open);
    let router = restarted.router();
    let revision = Call::get(format!("/v1/projects/{project}"))
        .with_token(&observer)
        .send_to(&router)
        .await
        .json()["revision"]
        .clone();
    let reconcile_after = |key: &'static str| {
        Call::post(
            format!("{}:reconcile", seats_path(&project, "pr-review")),
            &serde_json::json!({
                "expected_revision": revision,
                "seat_keys": ["triage"],
                "restart_missing": true,
            }),
        )
        .with_token(&operator)
        .with_key(key)
        .send_to(&router)
    };
    let calls_before = fake.calls().len();
    let recovered = reconcile_after("lost-launch").await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(
        observed(&recovered.json(), "triage"),
        expect("unchecked", "launch_unconfirmed", "recover")
    );
    assert_eq!(native(&recovered.json(), "triage"), created);
    assert_eq!(seat(&recovered.json(), "triage")["occupancy_generation"], 1);
    assert_eq!(
        fake.minted_natives(),
        minted + 1,
        "recovery bound the native the lost launch created"
    );
    let again = reconcile_after("lost-launch-again").await;
    assert_eq!(
        observed(&again.json(), "triage"),
        expect("healthy", "idle", "none")
    );
    assert_eq!(
        fake.calls()[calls_before..]
            .iter()
            .filter(|call| matches!(call, AdapterCall::LaunchHostedSeat(_)))
            .count(),
        1,
        "only the recovery replayed the launch"
    );
    restarted.state().signals().stop();
    drop(directory);
}

#[tokio::test]
async fn a_recovery_binds_only_the_native_the_census_saw() {
    let (world, project) = seat_world("seats-recovery-mismatch").await;
    let project_id = ProjectId::parse(&project).expect("a project id");
    let binding = declared(&world, &project, &daemon_seat("triage", "TRIAGE"))
        .await
        .expect("a binding");
    world.fake.lose_next_hosted_launch_ack();
    let lost = reconcile(&world, &project, &["triage"], true, "lost-launch").await;
    assert_ne!(lost.status, 200, "{}", lost.body);

    // The runtime now carries the binding on a native the lost launch did not
    // create, and a relaunch would create yet another.
    let created = world.fake.hosted_seat_native_id(binding).expect("a native");
    world.fake.forget_seat(&created);
    world.fake.add_hosted_seat_carrier(
        binding,
        DurableSeatCarrier {
            native_id: ExternalId::parse("native-seen-by-the-census").expect("an id"),
            archived: false,
            activity: DurableSeatActivity::Idle,
        },
    );
    let refused = reconcile(&world, &project, &["triage"], true, "lost-launch").await;
    assert_eq!(refused.json()["code"], "stale_binding", "{}", refused.body);
    assert_eq!(receipt_kind(&world, "lost-launch"), None);
    world.daemon.state().with_store(|store| {
        assert!(
            store
                .get_hosted_topology_seat(project_id, binding)
                .expect("reads")
                .is_none(),
            "a native the census never saw was bound"
        );
    });
}

#[tokio::test]
async fn a_seat_that_is_not_proved_missing_is_never_restarted() {
    let (world, project) = seat_world("seats-not-missing").await;
    let mut bindings = BTreeMap::new();
    for (seat_key, label) in [
        ("busy", "BUSY"),
        ("archived", "ARCHIVED"),
        ("duplicate", "DUPLICATE"),
        ("waiting", "WAITING"),
    ] {
        let binding = declared(&world, &project, &daemon_seat(seat_key, label))
            .await
            .expect("a binding");
        bindings.insert(seat_key, binding);
    }
    let launched = reconcile(
        &world,
        &project,
        &["archived", "busy", "duplicate", "waiting"],
        true,
        "launch-all",
    )
    .await;
    assert_eq!(launched.status, 200, "{}", launched.body);
    let unrecorded = declared(&world, &project, &daemon_seat("unrecorded", "UNRECORDED"))
        .await
        .expect("a binding");

    world.fake.set_hosted_seat_activity(
        &native(&launched.json(), "busy"),
        DurableSeatActivity::Working,
    );
    world.fake.set_hosted_seat_activity(
        &native(&launched.json(), "waiting"),
        DurableSeatActivity::AwaitingPermission,
    );
    world
        .fake
        .archive_hosted_seat(&native(&launched.json(), "archived"));
    let live = |id: &str| DurableSeatCarrier {
        native_id: ExternalId::parse(id).expect("an id"),
        archived: false,
        activity: DurableSeatActivity::Idle,
    };
    world
        .fake
        .add_hosted_seat_carrier(bindings["duplicate"], live("native-outside-kontor"));
    world
        .fake
        .add_hosted_seat_carrier(unrecorded, live("native-never-recorded"));

    let calls_before = world.fake.calls().len();
    let seats = ["archived", "busy", "duplicate", "unrecorded", "waiting"];
    let answer = reconcile(&world, &project, &seats, true, "not-missing").await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    for (seat_key, status, reason) in [
        ("busy", "busy", "working"),
        ("waiting", "busy", "awaiting_permission"),
        ("archived", "archived", "native_archived"),
        ("duplicate", "duplicate", "several_live_carriers"),
        ("unrecorded", "mismatched", "unrecorded_carrier"),
    ] {
        assert_eq!(
            observed(&answer.json(), seat_key),
            expect(status, reason, "none"),
            "{seat_key}"
        );
    }
    assert_eq!(launches_since(&world, calls_before), 0);

    // An unavailable or failing runtime is not evidence of absence, and a
    // census that does not correlate is a mismatch, not a vacancy.
    world.fake.forget_seat(&native(&launched.json(), "busy"));
    for (error, status, reason) in [
        (
            RuntimeError::Transport {
                rule: "the runtime did not answer",
            },
            "unchecked",
            "census_failed",
        ),
        (
            RuntimeError::UnsupportedCapability {
                capability: kontor_runtime::capability::RuntimeCapability::Inspect,
            },
            "unchecked",
            "runtime_unavailable",
        ),
        (
            RuntimeError::CorrelationFailed,
            "mismatched",
            "uncorrelated",
        ),
    ] {
        world.fake.refuse_hosted_census(error);
        let key = format!("census-{reason}");
        let answer = reconcile(&world, &project, &["busy"], true, &key).await;
        assert_eq!(answer.status, 200, "{}", answer.body);
        assert_eq!(
            observed(&answer.json(), "busy"),
            expect(status, reason, "none")
        );
    }
    assert_eq!(launches_since(&world, calls_before), 0);
}

#[tokio::test]
async fn a_seat_of_an_unmaterialized_desk_is_refused_before_any_binding() {
    let (world, project) = seat_world("seats-unmaterialized").await;
    let ensured = desk_call(&world, &project, "ensure", "adam", "adam-ensure").await;
    assert_eq!(ensured.status, 200, "{}", ensured.body);
    let answer = declare_on(
        &world,
        &project,
        "adam",
        &daemon_seat("helper", "HELPER"),
        "declare-adam-helper",
    )
    .await;
    assert_eq!(answer.status, 200, "{}", answer.body);
    let binding = SeatBindingId::parse(
        answer.json()["seat"]["seat_binding_id"]
            .as_str()
            .expect("a binding"),
    )
    .expect("an id");
    // The seat is observed first, census included, so the runtime is asked.
    // The refusal comes after that and before the binding, the launch intent
    // and any launch, which is what this counts.
    let calls_before = world.fake.calls().len();
    let refused = Call::post(
        format!("{}:reconcile", seats_path(&project, "adam")),
        &serde_json::json!({
            "expected_revision": current_project_revision(&world, &project).await,
            "seat_keys": ["helper"],
            "restart_missing": true,
        }),
    )
    .signed_as(&world, "operator")
    .with_key("adam-helper-restart")
    .send(&world)
    .await;
    assert_eq!(
        refused.json()["code"],
        "placement_blocked",
        "{}",
        refused.body
    );
    assert_eq!(launches_since(&world, calls_before), 0);
    world.daemon.state().with_store(|store| {
        let project_id = ProjectId::parse(&project).expect("id");
        assert!(
            store
                .get_seat_binding(project_id, binding)
                .expect("reads")
                .is_none()
        );
        assert!(
            store
                .get_hosted_seat_launch_intent(project_id, binding, 1)
                .expect("reads")
                .is_none()
        );
    });
    assert_eq!(receipt_kind(&world, "adam-helper-restart"), None);
}
