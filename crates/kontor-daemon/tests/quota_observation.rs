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

// This binary uses part of the shared harness; the rest is not dead code.
#[allow(dead_code)]
mod harness;

use harness::{Call, World, at};
use kontor_accounts::{QuotaBasis, QuotaSignal};
use kontor_core::id::AccountProfileId;
use kontor_core::repository::CapacityRepository;
use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};
use kontor_daemon::quota_observation::{QuotaClassification, QuotaObservationError, decide};
use kontor_runtime::refusal::{RefusalProvenance, TransientRefusal};

/// The text Codex actually produced on 2026-08-21, from the report Igor filed.
const CODEX_LIMIT: &str = "[System Error] You've hit your usage limit. Visit \
     https://chatgpt.com/codex/settings/usage to purchase more credits or try \
     again at Aug 23rd, 2026 9:35 AM.";

fn codex_signal_for(alias: &str) -> QuotaSignal {
    QuotaSignal {
        provider: alias.to_owned(),
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
    AccountProfileId::parse(
        created.json()["account_profile_id"]
            .as_str()
            .expect("an id"),
    )
    .expect("a canonical account id")
}

/// Decide, then perform the write the observation transaction would perform.
///
/// The production path hands the decided row to `record_observation` so the two
/// land atomically; these tests exercise the decision and its durable effect,
/// and the atomicity itself is proven at the store layer in `event_replay`.
fn classify_and_record(
    state: &kontor_api::state::ApiState,
    project: kontor_core::id::ProjectId,
    account_profile_id: AccountProfileId,
    signals: &[QuotaSignal],
    refusal: &TransientRefusal,
    now: kontor_core::id::Timestamp,
) -> Result<Option<QuotaClassification>, QuotaObservationError> {
    let Some(decided) = decide(state, project, account_profile_id, signals, refusal, now)? else {
        return Ok(None);
    };
    if let Some(request) = decided.request.as_ref() {
        state
            .with_store(|store| store.set_provider_quota_state(request))
            .map_err(QuotaObservationError::Repository)?;
    }
    Ok(Some(decided.classification))
}

fn where_from() -> RefusalProvenance {
    RefusalProvenance {
        agent_run_id: kontor_core::id::AgentRunId::parse("01a0306f-9398-7a51-a612-8c2b58251d58")
            .expect("a canonical run id"),
        binding_generation: 1,
        position: kontor_runtime::timeline::TimelinePosition {
            epoch: 1,
            sequence: 7,
        },
        sequence_end: 7,
        source_sequences: vec![(7, 7)],
        item_type: "assistant_message".to_owned(),
        observed_at: at("2026-08-21T09:00:00Z"),
    }
}

fn refusal(text: &str) -> TransientRefusal {
    TransientRefusal::parse(text, where_from()).expect("a non-sensitive refusal")
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
        &[codex_signal_for("codex-work")],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    )
    .expect("the write settles")
    .expect("a quota refusal");

    assert!(
        classified.proposes_write,
        "the first observation must write a row"
    );
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
            &[codex_signal_for("codex-work")],
            &refusal(CODEX_LIMIT),
            at(&format!("2026-08-21T10:0{minute}:00Z")),
        )
        .expect("the write settles")
        .expect("a quota refusal every time");
        if classification.proposes_write {
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
                &[codex_signal_for("codex-work")],
                &refusal(text),
                at("2026-08-21T10:00:00Z"),
            )
            .expect("no storage failure")
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
        .expect("no storage failure")
        .is_none(),
        "no configured signals means the poller stays the sole source of truth",
    );
    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    assert!(
        stored
            .iter()
            .all(|entry| entry.account_profile_id != profile)
    );
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
            &[codex_signal_for("codex-work")],
            &refusal(CODEX_LIMIT),
            at("2026-08-21T10:00:00Z"),
        )
        .expect("no storage failure")
        .is_none(),
        "a refusal may only block an account that can select that provider",
    );
    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    assert!(
        stored
            .iter()
            .all(|entry| entry.account_profile_id != profile)
    );
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
        &[codex_signal_for("codex-work")],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    )
    .expect("the write settles")
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
    assert!(
        TransientRefusal::parse("authorization: Bearer sk-livesecretvalue00", where_from())
            .is_none()
    );
    let carried = TransientRefusal::parse(CODEX_LIMIT, where_from()).expect("an ordinary refusal");
    assert!(!format!("{carried:?}").contains("usage limit"));
}

/// Two logins of one vendor carry the *identical* sentence under different
/// aliases. Each must record its own exact `(account, provider)` tuple.
#[tokio::test]
async fn identical_wording_records_the_alias_the_seat_actually_runs_on() {
    let world = World::open().await;
    let work = account(&world, "alias-work", "codex-work").await;
    let personal = account(&world, "alias-personal", "codex-personal").await;
    let state = world.daemon.state();
    // The realm-wide configured sequence, work first.
    let configured = [
        codex_signal_for("codex-work"),
        codex_signal_for("codex-personal"),
    ];

    for (profile, expected) in [(work, "codex-work"), (personal, "codex-personal")] {
        let classified = classify_and_record(
            &state,
            world.project,
            profile,
            &configured,
            &refusal(CODEX_LIMIT),
            at("2026-08-21T10:00:00Z"),
        )
        .expect("the write settles")
        .unwrap_or_else(|| panic!("{expected} classifies its own wording"));
        assert_eq!(classified.provider, expected);
        assert!(classified.proposes_write);
    }

    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    for (profile, expected) in [(work, "codex-work"), (personal, "codex-personal")] {
        let row = stored
            .iter()
            .find(|entry| entry.account_profile_id == profile)
            .unwrap_or_else(|| panic!("{expected} has a row"));
        assert_eq!(row.provider, expected, "each login records its own alias");
    }
}

/// The masking case. An ineligible entry earlier in the configured sequence
/// must not consume the match and leave the eligible one unreached — which is
/// exactly what classifying first and filtering afterwards did.
#[tokio::test]
async fn an_ineligible_earlier_signal_cannot_mask_an_eligible_later_one() {
    let world = World::open().await;
    let personal = account(&world, "mask-personal", "codex-personal").await;
    let state = world.daemon.state();
    // `codex-work` is first and matches the same text, but this seat cannot
    // select it.
    let configured = [
        codex_signal_for("codex-work"),
        codex_signal_for("codex-personal"),
    ];

    let classified = classify_and_record(
        &state,
        world.project,
        personal,
        &configured,
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    )
    .expect("the write settles")
    .expect("the eligible signal is reached");
    assert_eq!(
        classified.provider, "codex-personal",
        "an ineligible alias must not stand in front of an eligible one",
    );
}

/// A deployment that names bare vendor families rather than catalog aliases
/// classifies nothing, and says so by writing nothing at all.
#[tokio::test]
async fn a_signal_naming_a_vendor_family_is_inert_for_an_alias_routed_account() {
    let world = World::open().await;
    let profile = account(&world, "family-only", "codex-work").await;
    let state = world.daemon.state();

    assert!(
        classify_and_record(
            &state,
            world.project,
            profile,
            &[codex_signal_for("codex")],
            &refusal(CODEX_LIMIT),
            at("2026-08-21T10:00:00Z"),
        )
        .expect("no storage failure")
        .is_none(),
        "`codex` is not an alias any account routes",
    );
    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    assert!(
        stored
            .iter()
            .all(|entry| entry.account_profile_id != profile)
    );
}

/// A reset the vendor states as already past must not be recorded as a live
/// `Exhausted`: `blocks_at` would stop blocking immediately, the walk would
/// re-admit the account that just refused, and the pair would spin.
#[tokio::test]
async fn a_reset_that_is_not_in_the_future_records_a_blocking_unknown_instead() {
    let world = World::open().await;
    let profile = account(&world, "past-reset", "codex-work").await;
    let state = world.daemon.state();

    // Observed well after the instant the message states.
    let classified = classify_and_record(
        &state,
        world.project,
        profile,
        &[codex_signal_for("codex-work")],
        &refusal(CODEX_LIMIT),
        at("2027-01-01T00:00:00Z"),
    )
    .expect("the write settles")
    .expect("a quota refusal");

    assert_eq!(
        classified.kind,
        ProviderQuotaKind::Unknown,
        "a stated reset that already passed cannot describe a live block",
    );
    assert_eq!(classified.resets_at, None);

    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    let row = stored
        .iter()
        .find(|entry| entry.account_profile_id == profile)
        .expect("a recorded row");
    assert!(
        row.blocks_at(at("2027-01-01T00:00:01Z")),
        "the row still blocks, so the walk cannot loop back onto the account",
    );
}

/// A row that already exists on the pair is not our conclusion. It is only
/// skipped when it matches exactly; otherwise our conclusion is written --
/// provided ours is the more recent observation, which the store enforces.
#[tokio::test]
async fn a_foreign_older_row_is_never_mistaken_for_our_own() {
    use kontor_core::repository::CapacityRepository as _;

    let world = World::open().await;
    let profile = account(&world, "cas-foreign", "codex-work").await;
    let state = world.daemon.state();

    // Somebody else's conclusion about the same pair, from a different source
    // and observed *before* the item we are about to classify.
    state
        .with_store(|store| {
            store.set_provider_quota_state(&kontor_core::repository::NewProviderQuotaState {
                project_id: world.project,
                account_profile_id: profile,
                provider: "codex-work".to_owned(),
                state: ProviderQuotaKind::Available,
                resets_at: None,
                windows: Vec::new(),
                credit: None,
                evidence_hash: kontor_core::id::ContentHash::of(b"poller"),
                source: ProviderQuotaSource::ProviderReport,
                observed_at: at("2026-08-21T08:00:00Z"),
                expected_revision: kontor_core::id::AggregateRevision::INITIAL,
                updated_at: at("2026-08-21T08:00:00Z"),
            })
        })
        .expect("the foreign row is stored");

    // Our refusal's item is newer, so it must win.
    let classified = classify_and_record(
        &state,
        world.project,
        profile,
        &[codex_signal_for("codex-work")],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    )
    .expect("the write settles")
    .expect("a quota refusal");

    assert!(
        classified.proposes_write,
        "an existing foreign row is not our conclusion; ours must still be proposed",
    );
    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    let row = stored
        .iter()
        .find(|entry| entry.account_profile_id == profile)
        .expect("a recorded row");
    assert_eq!(
        row.source,
        ProviderQuotaSource::RuntimeObservation,
        "the newer refusal's conclusion is what is stored",
    );
    assert_eq!(row.state, ProviderQuotaKind::Exhausted);
    assert_eq!(
        stored
            .iter()
            .filter(|entry| entry.account_profile_id == profile)
            .count(),
        1,
        "one pair holds one row; no duplicate effect",
    );
}

/// The symmetric case, and the invariant the store now enforces: a refusal
/// whose *item* predates the current row must not regress it, however recently
/// Kontor happened to look.
#[tokio::test]
async fn a_refusal_older_than_the_current_row_does_not_regress_it() {
    use kontor_core::repository::CapacityRepository as _;

    let world = World::open().await;
    let profile = account(&world, "cas-stale", "codex-work").await;
    let state = world.daemon.state();

    state
        .with_store(|store| {
            store.set_provider_quota_state(&kontor_core::repository::NewProviderQuotaState {
                project_id: world.project,
                account_profile_id: profile,
                provider: "codex-work".to_owned(),
                state: ProviderQuotaKind::Available,
                resets_at: None,
                windows: Vec::new(),
                credit: None,
                evidence_hash: kontor_core::id::ContentHash::of(b"poller"),
                source: ProviderQuotaSource::ProviderReport,
                observed_at: at("2026-08-21T12:00:00Z"),
                expected_revision: kontor_core::id::AggregateRevision::INITIAL,
                updated_at: at("2026-08-21T12:00:00Z"),
            })
        })
        .expect("the newer report is stored");
    let before = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states")
        .into_iter()
        .find(|row| row.account_profile_id == profile)
        .expect("a row");

    // The item is dated 09:00 -- older than the report, however late the probe.
    let _ = classify_and_record(
        &state,
        world.project,
        profile,
        &[codex_signal_for("codex-work")],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T18:00:00Z"),
    )
    .expect("the write settles");

    let after = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states")
        .into_iter()
        .find(|row| row.account_profile_id == profile)
        .expect("a row");
    assert_eq!(
        after.state,
        ProviderQuotaKind::Available,
        "a stale refusal must not regress a newer report",
    );
    assert_eq!(after.revision, before.revision, "the row is untouched");
}

/// An account the run points at that no longer exists is an ordinary no-op, not
/// a storage failure — the boundary must keep the two apart.
#[tokio::test]
async fn a_vanished_account_is_a_no_op_and_not_an_error() {
    let world = World::open().await;
    let state = world.daemon.state();
    let absent = AccountProfileId::generate();
    let outcome = classify_and_record(
        &state,
        world.project,
        absent,
        &[codex_signal_for("codex-work")],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    );
    assert!(
        matches!(outcome, Ok(None)),
        "a missing account profile is Ok(None), never Err",
    );
}

/// The exact-concurrent-winner boundary. The row already holds precisely the
/// conclusion this call would write, so the call must report success with
/// `recorded: false` -- the effect happened once, and this call is not what did
/// it. Reporting `recorded: true` would claim a write that never happened.
#[tokio::test]
async fn an_exact_row_already_present_succeeds_without_writing_again() {
    let world = World::open().await;
    let profile = account(&world, "cas-exact", "codex-work").await;
    let state = world.daemon.state();
    let signals = [codex_signal_for("codex-work")];

    // First call writes.
    let first = classify_and_record(
        &state,
        world.project,
        profile,
        &signals,
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    )
    .expect("the write settles")
    .expect("a quota refusal");
    assert!(first.proposes_write, "the first call performs the write");

    let revision_after_first = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states")
        .into_iter()
        .find(|row| row.account_profile_id == profile)
        .expect("a row")
        .revision;

    // Second call finds the exact row already there -- the same shape a
    // concurrent winner leaves behind.
    let second = classify_and_record(
        &state,
        world.project,
        profile,
        &signals,
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:05:00Z"),
    )
    .expect("the write settles")
    .expect("a quota refusal");
    assert!(
        !second.proposes_write,
        "an exact row already present is not a write this call performed",
    );

    let revision_after_second = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states")
        .into_iter()
        .find(|row| row.account_profile_id == profile)
        .expect("a row")
        .revision;
    assert_eq!(
        revision_after_first, revision_after_second,
        "no duplicate effect: the row is untouched",
    );
}

/// The same boundary, from the other side: a row that differs is not ours, so
/// the call must not silently accept it as already-recorded.
#[tokio::test]
async fn a_row_that_differs_is_never_accepted_as_already_recorded() {
    let world = World::open().await;
    let profile = account(&world, "cas-differs", "codex-work").await;
    let state = world.daemon.state();

    let first = classify_and_record(
        &state,
        world.project,
        profile,
        &[codex_signal_for("codex-work")],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    )
    .expect("the write settles")
    .expect("a quota refusal");
    assert!(first.proposes_write);

    // A *different* refusal on the same pair carries a different digest, so it
    // is a new conclusion and must be written.
    let other = TransientRefusal::parse(
        "[System Error] You've hit your usage limit. Visit \
         https://chatgpt.com/codex/settings/usage to purchase more credits or try \
         again at Aug 24th, 2026 9:35 AM.",
        where_from(),
    )
    .expect("a refusal");
    let second = classify_and_record(
        &state,
        world.project,
        profile,
        &[codex_signal_for("codex-work")],
        &other,
        at("2026-08-21T10:05:00Z"),
    )
    .expect("the write settles")
    .expect("a quota refusal");
    assert!(
        second.proposes_write,
        "a different conclusion on the same pair is a new write, not a replay",
    );
}

/// A probe runs on inspection, which may be long after the turn ended. The
/// conclusion must be dated by the *item*, not by the read: dating it now would
/// make a stale refusal look freshly observed and let it overwrite a newer
/// poller report.
#[tokio::test]
async fn a_delayed_inspection_dates_the_conclusion_by_the_item_not_the_read() {
    let world = World::open().await;
    let profile = account(&world, "late-probe", "codex-work").await;
    let state = world.daemon.state();

    // The item was emitted at 09:00; Kontor inspects at 15:00.
    let decided = decide(
        &state,
        world.project,
        profile,
        &[codex_signal_for("codex-work")],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T15:00:00Z"),
    )
    .expect("the decision settles")
    .expect("a quota refusal");
    let request = decided.request.expect("a proposed write");

    assert_eq!(
        request.observed_at,
        at("2026-08-21T09:00:00Z"),
        "the conclusion is dated by the item, not by the inspection",
    );
    assert_ne!(
        request.observed_at,
        at("2026-08-21T15:00:00Z"),
        "wall-clock now is not source authority",
    );
    // And the reset the vendor stated is still read from the message.
    assert_eq!(
        decided.classification.resets_at,
        Some(at("2026-08-23T07:35:00Z")),
        "delayed inspection preserves the item's own reset instant",
    );
}

/// The pre-write claim must never be reported as a write. `decide` runs before
/// the store knows whether the event is even reducible.
#[tokio::test]
async fn a_decision_reports_a_proposal_and_never_a_completed_write() {
    let world = World::open().await;
    let profile = account(&world, "proposal-only", "codex-work").await;
    let state = world.daemon.state();

    let decided = decide(
        &state,
        world.project,
        profile,
        &[codex_signal_for("codex-work")],
        &refusal(CODEX_LIMIT),
        at("2026-08-21T10:00:00Z"),
    )
    .expect("the decision settles")
    .expect("a quota refusal");

    assert!(decided.proposes_write_matches_request());
    // Nothing was written: `decide` does not touch the store.
    let stored = state
        .with_store(|store| store.list_provider_quota_states(world.project))
        .expect("quota states");
    assert!(
        stored.iter().all(|row| row.account_profile_id != profile),
        "deciding proposes; it does not write",
    );
}
