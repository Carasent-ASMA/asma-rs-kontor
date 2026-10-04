//! ASMA-8282 B1b: the trusted daemon path of the recovery plan and outcome
//! decisions the shared runtime core now makes.
//!
//! Real caller credentials meet real stored pairs, on the hypothetical fake
//! runtime. Each refusal is compared whole, as the exact body the service
//! built before the move. The runtime core's own tests use hypothetical facts.

use super::*;
use kontor_api::error::{ApiError, ApiErrorCode};
use kontor_core::id::AggregateRevision;
use kontor_runtime::planning_pair::MandatoryMemberField;

const GENERATION: &str = "the member's occupancy generation is not the one the caller read";
const NATIVE: &str = "the expected native session is not the member's known one";
const CONVERSATION: &str =
    "the expected provider conversation is not the one recorded for the member's known session";
const CONTEXT: &str =
    "the member's frozen context differs from the one its known session was launched under";
const STOPPED: &str = "the planning pair member's native session is stopped and this runtime does not resume it in place";

/// `answer` is exactly `expected`, status and whole body.
fn assert_answered(answer: &Answer, expected: &ApiError, why: &str) {
    assert_eq!(
        answer.status.as_u16(),
        expected.code.status().as_u16(),
        "{why}: {}",
        answer.body
    );
    assert_eq!(
        answer.json(),
        serde_json::to_value(expected.body()).expect("a refusal body"),
        "{why}"
    );
}

/// An adverse readback's refusal, exactly as the service built it.
fn withdrawn(world: &World, rule: &'static str, native_id: &str) -> ApiError {
    ApiError::new(world.realm_id(), ApiErrorCode::Unavailable, rule)
        .about("planning pair member readback")
        .located_at(format!("native/{native_id}"))
        .advising("confirmation unknown: the member's known native session is kept and is not qualified now; nothing was created, replaced or archived")
}

/// Every refusal before the readback, each with the earlier of two faults
/// where two are present, then the frozen context, then the adverse readbacks
/// of an unbound member, then the exact recovery and its replay.
#[tokio::test]
async fn every_recovery_refusal_answers_its_baseline_body() {
    let realm = pair_realm("/tmp/kontor-asma8282-b1b-refusals").await;
    let world = &realm.world;
    let (pair, _) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::Correlation,
        "Recovery plan",
        "b1b-invoke",
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
    let deny = |code, rule| ApiError::new(world.realm_id(), code, rule);
    let cases = [
        (
            "a stale run revision before another generation",
            mutate(&|body| {
                body["expected_run_revision"] = (revision + 1).into();
                body["expected_member_occupancy_generation"] = 2.into();
            }),
            deny(
                ApiErrorCode::RevisionConflict,
                "the planning pair moved since the write was prepared",
            )
            .with_revision(Some(
                AggregateRevision::parse(revision).expect("a revision"),
            )),
        ),
        (
            "another generation before another session",
            mutate(&|body| {
                body["expected_member_occupancy_generation"] = 2.into();
                body["expected_native_identity"]["native_id"] = "native-other".into();
            }),
            deny(ApiErrorCode::StaleBinding, GENERATION),
        ),
        (
            "another session before another conversation",
            mutate(&|body| {
                body["expected_native_identity"]["native_id"] = "native-other".into();
                body["expected_provider_session_id"] = "provider-other".into();
            }),
            deny(ApiErrorCode::StaleBinding, NATIVE),
        ),
        (
            "another runtime kind",
            mutate(&|body| body["expected_native_identity"]["runtime_kind"] = "paseo".into()),
            deny(ApiErrorCode::StaleBinding, NATIVE),
        ),
        (
            "another host",
            mutate(&|body| body["expected_native_identity"]["host"] = "another-host".into()),
            deny(ApiErrorCode::StaleBinding, NATIVE),
        ),
        (
            "another runtime generation",
            mutate(&|body| {
                let generation = body["expected_native_identity"]["generation"]
                    .as_u64()
                    .expect("a generation");
                body["expected_native_identity"]["generation"] = (generation + 1).into();
            }),
            deny(ApiErrorCode::StaleBinding, NATIVE),
        ),
        (
            "another conversation",
            mutate(&|body| body["expected_provider_session_id"] = "provider-other".into()),
            deny(ApiErrorCode::StaleBinding, CONVERSATION),
        ),
        (
            "no conversation",
            mutate(&|body| body["expected_provider_session_id"] = serde_json::Value::Null),
            deny(ApiErrorCode::StaleBinding, CONVERSATION),
        ),
    ];
    for (why, changed, expected) in &cases {
        let refused = realm
            .recover_with(
                &pair,
                pair.seat_b,
                changed,
                realm.caller_token(),
                &format!("b1b-{why}"),
            )
            .await;
        assert_answered(&refused, expected, why);
    }
    let stranger = realm
        .recover_with(
            &pair,
            SeatBindingId::generate(),
            &body,
            realm.caller_token(),
            "b1b-stranger",
        )
        .await;
    assert_answered(
        &stranger,
        &deny(
            ApiErrorCode::NotFound,
            "the planning pair has no such member seat",
        ),
        "a seat of no member",
    );

    // The claim was launched under the frozen context; a cwd derived from a
    // moved project root is another context.
    let database = world.directory.path().join("kontor.db");
    let root: String = rusqlite::Connection::open(&database)
        .expect("the realm database opens")
        .query_row(
            "SELECT root_path FROM projects WHERE id = ?1",
            [&realm.project],
            |row| row.get(0),
        )
        .expect("the project root reads");
    let set_root = |path: &str| {
        rusqlite::Connection::open(&database)
            .expect("the realm database opens")
            .execute(
                "UPDATE projects SET root_path = ?1 WHERE id = ?2",
                [path, realm.project.as_str()],
            )
            .expect("the project root is written")
    };
    assert_eq!(set_root(&format!("{root}-moved")), 1);
    let drifted = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "b1b-context",
        )
        .await;
    assert_answered(
        &drifted,
        &deny(ApiErrorCode::PlacementBlocked, CONTEXT),
        "another context",
    );
    assert_eq!(set_root(&root), 1);
    assert!(
        member_effects(&world.fake.take_calls()).is_empty(),
        "no refusal before the readback reached the runtime"
    );

    // Adverse readbacks of an unbound member: refused, nothing withdrawn.
    let native = body["expected_native_identity"]["native_id"]
        .as_str()
        .expect("a native id")
        .to_owned();
    world
        .fake
        .observing_planning_pair_member_field_unsupported_in(
            PlanningPairSlot::SeatB,
            MandatoryMemberField::Correlation,
        );
    let unqualified = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "b1b-unqualified",
        )
        .await;
    assert_answered(
        &unqualified,
        &withdrawn(
            world,
            "the planning pair member's readback did not observe its correlation",
            &native,
        ),
        "an unobserved correlation",
    );
    world
        .fake
        .clearing_planning_pair_member_observation_faults();
    world.fake.stopping_consultation_native(pair.seat_b);
    let stopped = realm
        .recover_with(
            &pair,
            pair.seat_b,
            &body,
            realm.caller_token(),
            "b1b-stopped",
        )
        .await;
    assert_answered(&stopped, &withdrawn(world, STOPPED, &native), "stopped");
    world.fake.running_consultation_native_again(pair.seat_b);
    assert_eq!(
        member_effects(&world.fake.take_calls()),
        vec![
            AdapterCall::ReconcilePlanningPairMember(pair.seat_b),
            AdapterCall::ReconcilePlanningPairMember(pair.seat_b),
        ],
        "only readbacks"
    );
    assert!(
        realm
            .member_seat(&pair, pair.seat_b)
            .native_identity
            .is_none()
    );

    // The exact recovery binds the session; its replay calls nothing.
    let recovered = realm
        .recover_with(&pair, pair.seat_b, &body, realm.caller_token(), "b1b-exact")
        .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    assert_eq!(recovered.json()["receipt"]["applied"], "created");
    assert!(
        realm
            .member_seat(&pair, pair.seat_b)
            .native_identity
            .is_some()
    );
    world.fake.take_calls();
    let replayed = realm
        .recover_with(&pair, pair.seat_b, &body, realm.caller_token(), "b1b-exact")
        .await;
    assert_eq!(replayed.status, 200, "{}", replayed.body);
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    assert!(member_effects(&world.fake.take_calls()).is_empty());
    assert_eq!(realm.receipts_of("recover_planning_pair_seat"), 1);
}

/// A bound member: an adverse readback withdraws only its qualification and
/// keeps its claim; a disposed pair refuses a new recovery; a runtime that
/// answers with a create is never the known session.
#[tokio::test]
async fn a_bound_member_withdraws_and_a_disposed_pair_refuses_exactly_as_before() {
    let realm = pair_realm("/tmp/kontor-asma8282-b1b-bound").await;
    let world = &realm.world;
    let pair = realm.invoke("Bound plan", "b1b-bound-invoke").await;
    let claim = realm
        .known_claim(&pair, pair.seat_a)
        .expect("seat A's claim");
    let native = claim.identity.native_id.as_str().to_owned();
    let body = realm.recover_body(&pair, pair.seat_a).await;
    world.fake.stopping_consultation_native(pair.seat_a);
    let stopped = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &body,
            realm.caller_token(),
            "b1b-bound-stopped",
        )
        .await;
    assert_answered(&stopped, &withdrawn(world, STOPPED, &native), "stopped");
    assert!(
        realm
            .member_seat(&pair, pair.seat_a)
            .native_identity
            .is_none(),
        "only a bound member is withdrawn"
    );
    assert_eq!(realm.known_claim(&pair, pair.seat_a), Some(claim));
    assert_eq!(
        realm.stored_run(&pair).state,
        ConsultationRunState::NeedsHuman
    );
    world.fake.running_consultation_native_again(pair.seat_a);
    let body = realm.recover_body(&pair, pair.seat_a).await;
    let back = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &body,
            realm.caller_token(),
            "b1b-bound-back",
        )
        .await;
    assert_eq!(back.status, 200, "{}", back.body);

    for (seat, key) in [
        (pair.seat_a, "b1b-finding-a"),
        (pair.seat_b, "b1b-finding-b"),
    ] {
        let recorded = realm.finding(&pair, seat, "Keep it small.", key).await;
        assert_eq!(recorded.status, 200, "{}", recorded.body);
    }
    let released = realm
        .read_with(&pair, Some(realm.caller_token()))
        .await
        .json();
    let finding = |slot: &str| {
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
                    {"slot": "seat-a", "finding": finding("seat-a"), "disposition": "accepted"},
                    {"slot": "seat-b", "finding": finding("seat-b"), "disposition": "rejected"},
                ],
                "rationale": "Keep the dissent.",
                "expected_revision": realm.revision(&pair).await,
            }),
            realm.caller_token(),
            "b1b-dispose",
        )
        .await;
    assert_eq!(disposed.status, 200, "{}", disposed.body);
    let body = realm.recover_body(&pair, pair.seat_a).await;
    let terminal = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &body,
            realm.caller_token(),
            "b1b-disposed",
        )
        .await;
    assert_answered(
        &terminal,
        &ApiError::from_domain(
            world.realm_id(),
            &kontor_core::DomainError::Terminal {
                subject: "PlanningPairRun",
            },
        ),
        "a disposed pair",
    );
    let mut stale = body.clone();
    stale["expected_run_revision"] =
        (body["expected_run_revision"].as_u64().expect("a revision") + 1).into();
    let terminal_first = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &stale,
            realm.caller_token(),
            "b1b-disposed-stale",
        )
        .await;
    assert_answered(
        &terminal_first,
        &ApiError::from_domain(
            world.realm_id(),
            &kontor_core::DomainError::Terminal {
                subject: "PlanningPairRun",
            },
        ),
        "a disposed pair before a stale revision",
    );

    // Last, since the fault is sticky: a create is never the known session.
    let other = realm.invoke("Another plan", "b1b-other-invoke").await;
    let other_native = realm
        .known_claim(&other, other.seat_a)
        .expect("the other pair's claim")
        .identity
        .native_id
        .as_str()
        .to_owned();
    let body = realm.recover_body(&other, other.seat_a).await;
    world.fake.misreporting_planning_pair_reconcile_as_created();
    let created = realm
        .recover_with(
            &other,
            other.seat_a,
            &body,
            realm.caller_token(),
            "b1b-created",
        )
        .await;
    assert_answered(
        &created,
        &withdrawn(
            world,
            "the runtime did not answer with the member's known native session",
            &other_native,
        ),
        "a create",
    );
    assert!(
        realm
            .member_seat(&other, other.seat_a)
            .native_identity
            .is_none()
    );
}
