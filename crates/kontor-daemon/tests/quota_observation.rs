//! Reading a seat's own refusal as a provider quota state, against a real Realm.
//!
//! The mutants this suite exists to kill:
//!
//! * classifying a refusal but never writing the row, or writing it without the
//!   instant the vendor stated;
//! * writing a second row for a limit the Realm already recorded, so one outage
//!   becomes a pile of identical states;
//! * treating an ordinary error, or a seat that simply finished, as a quota
//!   refusal;
//! * attributing one vendor's wording to an account that cannot select that
//!   provider;
//! * letting the refusal sentence — or a credential inside it — reach the store,
//!   a stored event row, or a `Debug` line.

mod harness;

use harness::{Call, World, at};
use kontor_accounts::{QuotaBasis, QuotaSignal};
use kontor_core::id::AccountProfileId;
use kontor_core::repository::CapacityRepository;
use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};
use kontor_daemon::quota_observation::classify_and_record;
use kontor_runtime::refusal::TransientRefusal;

/// The text Codex actually produced on 2026-08-21, from the report Igor filed.
const CODEX_LIMIT: &str = "[System Error] You've hit your usage limit. Visit \
     https://chatgpt.com/codex/settings/usage to purchase more credits or try \
     again at Aug 23rd, 2026 9:35 AM.";

fn codex_signal() -> QuotaSignal {
    QuotaSignal {
        provider: "codex-work".to_owned(),
        basis: QuotaBasis::PlanAllowance,
        markers: vec!["usage limit".to_owned()],
        reset_prefix: Some("try again at ".to_owned()),
        reset_zone: Some("Europe/Oslo".to_owned()),
    }
}

/// Create an account addressable as `codex-work`.
async fn account(world: &World, label: &str, alias: &str) -> AccountProfileId {
    let project = world.project;
    let created = Call::post(
        format!("/v1/projects/{project}/provider-account-profiles:ensure"),
        &serde_json::json!({
            "label": label,
            "harness": "fake.runtime",
            "credential_alias": label,
            "selectable_providers": [alias],
            "enabled": true
        }),
    )
    .signed_as(world, "admin")
    .with_key(label)
    .send(world)
    .await;
    assert_eq!(created.status, 200, "{}", created.body);
    AccountProfileId::parse(created.json()["account_profile_id"].as_str().expect("an id"))
        .expect("a canonical account id")
}

fn refusal(text: &str) -> TransientRefusal {
    TransientRefusal::parse(text).expect("a non-sensitive refusal")
}

#[tokio::test]
async fn a_usage_limit_refusal_records_the_instant_the_vendor_stated() {
    let world = World::open().await;
    let profile = account(&world, "codex-work-a", "codex-work").await;
    let state = world.daemon.state();

    let classified = classify_and_record(
        &state,
        world.project,
        profile,
        &[codex_signal()],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    )
    .expect("a quota refusal");

    assert!(classified.recorded, "the first observation must write a row");
    assert_eq!(classified.kind, ProviderQuotaKind::Exhausted);
    assert_eq!(classified.provider, "codex-work");
    // 09:35 in Oslo during August is CEST, two hours ahead of UTC.
    assert_eq!(classified.resets_at, Some(at("2026-08-23T07:35:00Z")));

    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    let row = stored
        .iter()
        .find(|entry| entry.account_profile_id == profile)
        .expect("a recorded row");
    assert_eq!(row.source, ProviderQuotaSource::RuntimeObservation);
    assert_eq!(row.state, ProviderQuotaKind::Exhausted);
    assert_eq!(row.resets_at, Some(at("2026-08-23T07:35:00Z")));
}

#[tokio::test]
async fn the_same_limit_observed_three_times_writes_one_row() {
    let world = World::open().await;
    let profile = account(&world, "codex-work-b", "codex-work").await;
    let state = world.daemon.state();

    let mut recorded = 0;
    for minute in 0..3 {
        let classification = classify_and_record(
            &state,
            world.project,
            profile,
            &[codex_signal()],
            &refusal(CODEX_LIMIT),
            at(&format!("2026-08-21T10:0{minute}:00Z")),
        )
        .expect("a quota refusal every time");
        if classification.recorded {
            recorded += 1;
        }
    }

    assert_eq!(recorded, 1, "only the first observation may write");
    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    assert_eq!(
        stored
            .iter()
            .filter(|entry| entry.account_profile_id == profile)
            .count(),
        1,
        "one limit is one row, not three",
    );
}

#[tokio::test]
async fn an_ordinary_error_and_a_finished_seat_record_nothing() {
    let world = World::open().await;
    let profile = account(&world, "codex-work-c", "codex-work").await;
    let state = world.daemon.state();

    for text in [
        "connection reset by peer",
        "Done. I've implemented the change and the tests pass.",
        "The task is complete.",
    ] {
        assert!(
            classify_and_record(
                &state,
                world.project,
                profile,
                &[codex_signal()],
                &refusal(text),
                at("2026-08-21T10:00:00Z"),
            )
            .is_none(),
            "{text:?} is not a quota refusal",
        );
    }

    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    assert!(
        !stored
            .iter()
            .any(|entry| entry.account_profile_id == profile),
        "a seat that simply finished must not block its account",
    );
}

#[tokio::test]
async fn a_realm_with_no_signals_document_stays_inert() {
    let world = World::open().await;
    let profile = account(&world, "codex-work-d", "codex-work").await;
    let state = world.daemon.state();

    assert!(
        classify_and_record(
            &state,
            world.project,
            profile,
            &[],
            &refusal(CODEX_LIMIT),
            at("2026-08-21T10:00:00Z"),
        )
        .is_none(),
        "no configured signals means the poller stays the sole source of truth",
    );
    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    assert!(stored.iter().all(|entry| entry.account_profile_id != profile));
}

#[tokio::test]
async fn a_wording_this_account_cannot_select_is_not_attributed_to_it() {
    let world = World::open().await;
    // The account is addressable as `codex-personal`; the signal names
    // `codex-work`. One vendor's sentence must not block the other login.
    let profile = account(&world, "codex-personal-a", "codex-personal").await;
    let state = world.daemon.state();

    assert!(
        classify_and_record(
            &state,
            world.project,
            profile,
            &[codex_signal()],
            &refusal(CODEX_LIMIT),
            at("2026-08-21T10:00:00Z"),
        )
        .is_none(),
        "a refusal may only block an account that can select that provider",
    );
    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    assert!(stored.iter().all(|entry| entry.account_profile_id != profile));
}

#[tokio::test]
async fn the_refusal_sentence_never_reaches_the_store() {
    let world = World::open().await;
    let profile = account(&world, "codex-work-e", "codex-work").await;
    let state = world.daemon.state();

    let classified = classify_and_record(
        &state,
        world.project,
        profile,
        &[codex_signal()],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    )
    .expect("a quota refusal");

    // Only the digest crosses the boundary.
    assert_eq!(
        classified.evidence_hash,
        refusal(CODEX_LIMIT).digest(),
        "the recorded evidence is the digest of the sentence",
    );

    let database = std::fs::read(world.directory.path().join("kontor.db"))
        .expect("the realm database is readable");
    let haystack = String::from_utf8_lossy(&database);
    for fragment in [
        "You've hit your usage limit",
        "chatgpt.com/codex/settings/usage",
        "try again at Aug 23rd",
    ] {
        assert!(
            !haystack.contains(fragment),
            "the database must not contain {fragment:?}",
        );
    }
}

#[tokio::test]
async fn a_credential_shaped_refusal_is_refused_before_it_is_ever_classified() {
    // The guard is in construction, so there is no path that classifies one.
    assert!(TransientRefusal::parse("authorization: Bearer sk-livesecretvalue00").is_none());
    let carried = TransientRefusal::parse(CODEX_LIMIT).expect("an ordinary refusal");
    assert!(!format!("{carried:?}").contains("usage limit"));
}
