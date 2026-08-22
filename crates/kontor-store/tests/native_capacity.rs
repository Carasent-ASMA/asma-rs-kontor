//! OP-03 CP3 — raw capacity evidence is immutable, and an override stands
//! beside it rather than over it.
//!
//! These are the store-level halves of two required negative proofs: a refresh
//! that stored only derived state, and an override that rewrote what the
//! provider reported. Both are killed here structurally — the evidence row
//! cannot be updated at all — and again at the API boundary in the daemon's
//! black-box suite.

use kontor_core::id::{
    AccountProfileId, AggregateRevision, CanonicalDocument, CapacityObservationId, ContentHash,
    CredentialAlias, ExternalName, ProjectId, RuntimeKindKey, Timestamp, parse_utc_timestamp,
};
use kontor_core::id::{CurrencyCode, Money};
use kontor_core::quota::{CreditBalance, HeadroomThresholds, QuotaWindow, QuotaWindowKind};
use kontor_core::repository::{
    CapacityRepository, CredentialReference, CredentialReferenceKind, NewAccountProfile,
    NewAvailabilityOverride, NewCapacityObservation, NewProject, NewProviderQuotaState,
    ProjectRepository, RepositoryError,
};
use kontor_core::spec::{ProviderQuotaKind, ProviderQuotaSource};
use kontor_store::SqliteStore;
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn document(json: serde_json::Value) -> CanonicalDocument {
    CanonicalDocument::from_value(&json).expect("a canonical document")
}

/// A reading shaped like the collector's, without depending on that crate: the
/// store is not allowed to know what a reading means, so its tests must not
/// either.
fn reading(available: bool) -> CanonicalDocument {
    document(serde_json::json!({
        "schema_version": 1,
        "profile_enabled": true,
        "runtime_kind": "paseo",
        "probe": if available {
            serde_json::json!({ "outcome": "account_environment_supported" })
        } else {
            serde_json::json!({ "outcome": "refused", "refusal": "limit_exceeded" })
        },
    }))
}

struct Fixture {
    store: SqliteStore,
    project_id: ProjectId,
    account_profile_id: AccountProfileId,
    /// The realm file, so a test can reopen it and prove what survived a
    /// restart rather than what the open handle still remembers.
    path: std::path::PathBuf,
    _home: TempDir,
}

fn fixture() -> Fixture {
    let home = TempDir::new().expect("a temporary directory");
    let path = home.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the store opens");
    let project_id = ProjectId::generate();
    let account_profile_id = AccountProfileId::generate();
    let created_at = at("2026-08-17T09:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Capacity project"),
            root_path: name("/tmp/capacity-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_account_profile(&NewAccountProfile {
            id: account_profile_id,
            project_id,
            label: name("Primary account"),
            external_account_id: None,
            harness: RuntimeKindKey::parse("paseo").expect("a valid runtime kind"),
            credential_ref: CredentialReference {
                kind: CredentialReferenceKind::Keychain,
                alias: CredentialAlias::parse("primary").expect("a valid alias"),
            },
            environment: document(serde_json::json!({ "schema_version": 1 })),
            routing: document(serde_json::json!({ "schema_version": 1 })),
            capability: document(serde_json::json!({ "schema_version": 1 })),
            provider_identity: None,
            enabled: true,
            created_at,
        })
        .expect("the account profile is created");
    Fixture {
        store,
        project_id,
        account_profile_id,
        path,
        _home: home,
    }
}

#[test]
fn a_stored_observation_keeps_its_raw_reading_alongside_what_was_derived() {
    let fixture = fixture();
    let id = CapacityObservationId::generate();
    let stored = fixture
        .store
        .record_capacity_observation(&NewCapacityObservation {
            id,
            project_id: fixture.project_id,
            account_profile_id: fixture.account_profile_id,
            observed_at: at("2026-08-17T09:05:00Z"),
            reading: reading(false),
            available: false,
            pressure: true,
            cooling_until: Some(at("2026-08-17T09:10:00Z")),
        })
        .expect("the observation is recorded");

    assert_eq!(stored.reading, reading(false), "the raw evidence survives");
    assert!(stored.pressure);
    assert!(!stored.available);

    let read_back = fixture
        .store
        .get_capacity_observation(fixture.project_id, id)
        .expect("the read succeeds")
        .expect("the observation exists");
    assert_eq!(read_back, stored);
}

#[test]
fn the_same_collector_reading_cannot_be_recorded_twice() {
    let fixture = fixture();
    let id = CapacityObservationId::generate();
    let request = NewCapacityObservation {
        id,
        project_id: fixture.project_id,
        account_profile_id: fixture.account_profile_id,
        observed_at: at("2026-08-17T09:05:00Z"),
        reading: reading(true),
        available: true,
        pressure: false,
        cooling_until: None,
    };
    fixture
        .store
        .record_capacity_observation(&request)
        .expect("the first record succeeds");
    let replay = fixture.store.record_capacity_observation(&request);
    assert!(
        matches!(replay, Err(RepositoryError::Conflict { .. })),
        "a replayed collector reading is not a second fact: {replay:?}"
    );
}

#[test]
fn an_override_stands_beside_the_evidence_and_never_rewrites_it() {
    let fixture = fixture();
    let id = CapacityObservationId::generate();
    fixture
        .store
        .record_capacity_observation(&NewCapacityObservation {
            id,
            project_id: fixture.project_id,
            account_profile_id: fixture.account_profile_id,
            observed_at: at("2026-08-17T09:05:00Z"),
            reading: reading(false),
            available: false,
            pressure: true,
            cooling_until: Some(at("2026-08-17T09:10:00Z")),
        })
        .expect("the observation is recorded");

    let judgement = fixture
        .store
        .set_availability_override(&NewAvailabilityOverride {
            project_id: fixture.project_id,
            account_profile_id: fixture.account_profile_id,
            available: true,
            reason: name("provider confirmed the limit was ours"),
            expires_at: Some(at("2026-08-17T10:00:00Z")),
            expected_revision: AggregateRevision::INITIAL,
            updated_at: at("2026-08-17T09:06:00Z"),
        })
        .expect("the override is recorded");
    assert!(judgement.available);
    assert_eq!(judgement.revision, AggregateRevision::INITIAL);

    let evidence = fixture
        .store
        .get_capacity_observation(fixture.project_id, id)
        .expect("the read succeeds")
        .expect("the observation exists");
    assert!(
        !evidence.available && evidence.pressure,
        "the operator disagreed with the provider; the provider's word is unchanged"
    );
    assert_eq!(evidence.reading, reading(false));
}

#[test]
fn an_override_written_against_a_revision_that_has_moved_is_refused() {
    let fixture = fixture();
    let first = NewAvailabilityOverride {
        project_id: fixture.project_id,
        account_profile_id: fixture.account_profile_id,
        available: true,
        reason: name("first judgement"),
        expires_at: None,
        expected_revision: AggregateRevision::INITIAL,
        updated_at: at("2026-08-17T09:06:00Z"),
    };
    let stored = fixture
        .store
        .set_availability_override(&first)
        .expect("the first override is recorded");
    assert_eq!(stored.revision, AggregateRevision::INITIAL);

    // A caller that read revision one is current, and its write advances it.
    let advanced = fixture
        .store
        .set_availability_override(&NewAvailabilityOverride {
            reason: name("second judgement"),
            updated_at: at("2026-08-17T09:07:00Z"),
            ..first.clone()
        })
        .expect("a write under the current revision is accepted");
    assert!(advanced.revision > stored.revision);

    // A second caller still holding revision one is not.
    let stale = fixture
        .store
        .set_availability_override(&NewAvailabilityOverride {
            reason: name("third judgement"),
            updated_at: at("2026-08-17T09:08:00Z"),
            ..first
        });
    assert!(
        matches!(stale, Err(RepositoryError::Domain(_))),
        "a write under a revision that has moved is a conflict: {stale:?}"
    );
    assert_eq!(
        fixture
            .store
            .list_availability_overrides(fixture.project_id)
            .expect("the read succeeds"),
        vec![advanced],
        "the refused write left nothing behind"
    );
}

#[test]
fn a_lapsed_override_is_still_a_record_that_someone_made_it() {
    let fixture = fixture();
    let stored = fixture
        .store
        .set_availability_override(&NewAvailabilityOverride {
            project_id: fixture.project_id,
            account_profile_id: fixture.account_profile_id,
            available: true,
            reason: name("during the incident"),
            expires_at: Some(at("2026-08-17T10:00:00Z")),
            expected_revision: AggregateRevision::INITIAL,
            updated_at: at("2026-08-17T09:06:00Z"),
        })
        .expect("the override is recorded");

    assert!(stored.is_standing(at("2026-08-17T09:59:59Z")));
    assert!(!stored.is_standing(at("2026-08-17T10:00:00Z")));
    assert_eq!(
        fixture
            .store
            .list_availability_overrides(fixture.project_id)
            .expect("the read succeeds")
            .len(),
        1,
        "expiry is applied by the reader, not by deleting the row"
    );
}

#[test]
fn the_latest_reading_per_account_is_the_one_a_projection_reports() {
    let fixture = fixture();
    for (observed_at, available) in [
        ("2026-08-17T09:05:00Z", false),
        ("2026-08-17T09:06:00Z", true),
    ] {
        fixture
            .store
            .record_capacity_observation(&NewCapacityObservation {
                id: CapacityObservationId::generate(),
                project_id: fixture.project_id,
                account_profile_id: fixture.account_profile_id,
                observed_at: at(observed_at),
                reading: reading(available),
                available,
                pressure: !available,
                cooling_until: None,
            })
            .expect("the observation is recorded");
    }

    let latest = fixture
        .store
        .latest_capacity_observations(fixture.project_id)
        .expect("the read succeeds");
    assert_eq!(latest.len(), 1, "one row per account, not one per reading");
    assert!(latest[0].available);
    assert_eq!(latest[0].observed_at, at("2026-08-17T09:06:00Z"));
}

// ---------------------------------------------------------------------------
// Per-provider quota state
//
// The mutants this section exists to kill:
//
// * letting a drained balance carry a reset instant, which is what turns a dead
//   credit key into an endless requeue;
// * letting an exhausted allowance omit one, which parks work forever;
// * keying the state on the account alone, so "Codex is out, Claude is fine"
//   cannot be said.
// ---------------------------------------------------------------------------

fn quota(
    fixture: &Fixture,
    provider: &str,
    state: ProviderQuotaKind,
    resets_at: Option<Timestamp>,
) -> NewProviderQuotaState {
    NewProviderQuotaState {
        project_id: fixture.project_id,
        account_profile_id: fixture.account_profile_id,
        provider: provider.to_owned(),
        state,
        resets_at,
        windows: Vec::new(),
        credit: None,
        evidence_hash: ContentHash::of(b"the provider said so"),
        source: ProviderQuotaSource::RuntimeObservation,
        observed_at: at("2026-08-21T09:35:00Z"),
        expected_revision: AggregateRevision::INITIAL,
        updated_at: at("2026-08-21T09:35:00Z"),
    }
}

#[test]
fn one_account_holds_a_separate_quota_state_per_provider() {
    let fixture = fixture();
    // Exactly the 2026-08-21 shape: one account profile, Codex exhausted until a
    // known instant, Claude untouched. Account-scoped availability cannot express
    // this, which is the whole reason this table exists.
    fixture
        .store
        .set_provider_quota_state(&quota(
            &fixture,
            "codex",
            ProviderQuotaKind::Exhausted,
            Some(at("2026-08-23T09:35:00Z")),
        ))
        .expect("the codex state is recorded");
    fixture
        .store
        .set_provider_quota_state(&quota(
            &fixture,
            "claude",
            ProviderQuotaKind::Available,
            None,
        ))
        .expect("the claude state is recorded");

    let states = fixture
        .store
        .list_provider_quota_states(fixture.project_id)
        .expect("the read succeeds");
    assert_eq!(states.len(), 2);
    let codex = states
        .iter()
        .find(|entry| entry.provider == "codex")
        .expect("the codex state is listed");
    assert_eq!(codex.state, ProviderQuotaKind::Exhausted);
    assert_eq!(codex.resets_at, Some(at("2026-08-23T09:35:00Z")));
    assert!(codex.blocks_at(at("2026-08-22T00:00:00Z")));
    assert!(!codex.blocks_at(at("2026-08-23T10:00:00Z")));
    let claude = states
        .iter()
        .find(|entry| entry.provider == "claude")
        .expect("the claude state is listed");
    assert!(!claude.blocks_at(at("2026-08-22T00:00:00Z")));
}

#[test]
fn a_reset_instant_belongs_to_an_exhausted_allowance_and_to_nothing_else() {
    let fixture = fixture();
    // A drained credit balance recovers when someone pays, not on a clock. A row
    // that claimed otherwise would be requeued forever against a dead key.
    let drained_with_clock = quota(
        &fixture,
        "openrouter",
        ProviderQuotaKind::Drained,
        Some(at("2026-08-23T09:35:00Z")),
    );
    assert!(matches!(
        fixture
            .store
            .set_provider_quota_state(&drained_with_clock)
            .expect_err("a drained balance may not carry a reset instant"),
        RepositoryError::Domain(_)
    ));

    // And the converse: an allowance that ran out knows when it returns.
    let exhausted_without_clock = quota(&fixture, "codex", ProviderQuotaKind::Exhausted, None);
    assert!(matches!(
        fixture
            .store
            .set_provider_quota_state(&exhausted_without_clock)
            .expect_err("an exhausted allowance must carry a reset instant"),
        RepositoryError::Domain(_)
    ));

    // Neither refusal wrote anything.
    assert!(
        fixture
            .store
            .list_provider_quota_states(fixture.project_id)
            .expect("the read succeeds")
            .is_empty()
    );
}

#[test]
fn a_quota_state_written_against_a_revision_that_has_moved_is_refused() {
    let fixture = fixture();
    let first = quota(&fixture, "codex", ProviderQuotaKind::Available, None);
    let stored = fixture
        .store
        .set_provider_quota_state(&first)
        .expect("the first state is recorded");
    assert_eq!(stored.revision, AggregateRevision::INITIAL);

    // A caller that read revision one is current, and its write advances it.
    let advanced = fixture
        .store
        .set_provider_quota_state(&NewProviderQuotaState {
            state: ProviderQuotaKind::Exhausted,
            resets_at: Some(at("2026-08-23T09:35:00Z")),
            updated_at: at("2026-08-21T09:36:00Z"),
            ..first.clone()
        })
        .expect("a write under the current revision is accepted");
    assert!(advanced.revision > stored.revision);
    assert_eq!(advanced.state, ProviderQuotaKind::Exhausted);

    // A second caller still holding revision one is not. Two collectors racing
    // on one provider is the ordinary case, not the exotic one.
    let stale = fixture
        .store
        .set_provider_quota_state(&NewProviderQuotaState {
            state: ProviderQuotaKind::Drained,
            resets_at: None,
            updated_at: at("2026-08-21T09:37:00Z"),
            ..first
        });
    assert!(
        matches!(stale, Err(RepositoryError::Domain(_))),
        "a write under a revision that has moved is a conflict: {stale:?}"
    );
    assert_eq!(
        fixture
            .store
            .list_provider_quota_states(fixture.project_id)
            .expect("the read succeeds"),
        vec![advanced],
        "the refused write left nothing behind"
    );
}

// ---------------------------------------------------------------------------
// Concurrent windows and the credit balance
//
// The mutants this section exists to kill:
//
// * losing a window across a restart, so the latest-reset derivation has less to
//   derive from than the collector reported;
// * merging a new reading into the old set instead of replacing it, which keeps
//   a window the provider has withdrawn and routes on it forever;
// * storing a balance and its reserve in two currencies, which is the comparison
//   nothing in this system is allowed to make.
// ---------------------------------------------------------------------------

fn window(kind: QuotaWindowKind, used_percent: u8, resets_at: &str) -> QuotaWindow {
    QuotaWindow {
        kind,
        resets_at: at(resets_at),
        used_percent,
    }
}

fn credit(remaining: u64, reserve: u64, currency: &str) -> CreditBalance {
    let code = CurrencyCode::parse(currency).expect("a valid currency");
    CreditBalance {
        remaining: Money {
            minor_units: remaining,
            currency: code,
        },
        reserve: Money {
            minor_units: reserve,
            currency: code,
        },
    }
}

#[test]
fn every_window_balance_and_reserve_survives_a_restart() {
    let fixture = fixture();
    let recorded = fixture
        .store
        .set_provider_quota_state(&NewProviderQuotaState {
            windows: vec![
                window(QuotaWindowKind::Session, 28, "2026-08-21T14:00:00Z"),
                window(QuotaWindowKind::Weekly, 62, "2026-08-23T09:35:00Z"),
            ],
            credit: Some(credit(40_000, 10_000, "EUR")),
            ..quota(&fixture, "codex", ProviderQuotaKind::Available, None)
        })
        .expect("the state is recorded");
    assert_eq!(recorded.windows().len(), 2);

    // Reopened from the same file: the point is the durable row, not the cache.
    let reopened = SqliteStore::open(&fixture.path).expect("the realm reopens");
    let restored = reopened
        .list_provider_quota_states(fixture.project_id)
        .expect("the read succeeds");
    assert_eq!(
        restored,
        vec![recorded],
        "a restart must preserve every window, the balance and its reserve"
    );

    // And the derivation still holds over the restored row: the latest reset
    // among the spent windows, not the earliest.
    let thresholds = HeadroomThresholds {
        session_percent: 90,
        daily_percent: 85,
        weekly_percent: 50,
        monthly_percent: 80,
    };
    let outlook = restored[0].headroom(&thresholds, at("2026-08-21T10:00:00Z"));
    assert_eq!(
        outlook,
        kontor_core::repository::ProviderHeadroom::Blocked {
            blocked_until: at("2026-08-23T09:35:00Z")
        },
        "the weekly window is spent at this threshold and returns last"
    );
}

#[test]
fn a_new_reading_replaces_the_window_set_rather_than_merging_into_it() {
    let fixture = fixture();
    let first = NewProviderQuotaState {
        windows: vec![
            window(QuotaWindowKind::Session, 10, "2026-08-21T14:00:00Z"),
            window(QuotaWindowKind::Weekly, 20, "2026-08-23T09:35:00Z"),
        ],
        ..quota(&fixture, "codex", ProviderQuotaKind::Available, None)
    };
    let stored = fixture
        .store
        .set_provider_quota_state(&first)
        .expect("the first reading is recorded");

    // The provider has stopped offering a session window.
    let second = fixture
        .store
        .set_provider_quota_state(&NewProviderQuotaState {
            windows: vec![window(QuotaWindowKind::Weekly, 35, "2026-08-23T09:35:00Z")],
            expected_revision: stored.revision,
            ..first
        })
        .expect("the second reading is recorded");
    assert_eq!(
        second
            .windows()
            .iter()
            .map(|entry| entry.kind)
            .collect::<Vec<_>>(),
        vec![QuotaWindowKind::Weekly],
        "a merge would keep a window the provider has withdrawn and route on it forever"
    );
    assert_eq!(second.windows()[0].used_percent, 35);
}

#[test]
fn a_balance_and_a_reserve_in_two_currencies_cannot_be_stored_at_all() {
    let fixture = fixture();
    let refused = fixture
        .store
        .set_provider_quota_state(&NewProviderQuotaState {
            credit: Some(CreditBalance {
                remaining: Money {
                    minor_units: 50_000,
                    currency: CurrencyCode::parse("EUR").expect("a currency"),
                },
                reserve: Money {
                    minor_units: 100,
                    currency: CurrencyCode::parse("USD").expect("a currency"),
                },
            }),
            ..quota(&fixture, "openrouter", ProviderQuotaKind::Available, None)
        });
    assert!(
        matches!(refused, Err(RepositoryError::Domain(_))),
        "one currency, two amounts — the comparison is refused before it can be made: {refused:?}"
    );
}

#[test]
fn one_window_kind_cannot_be_observed_twice_on_one_pair() {
    let fixture = fixture();
    let refused = fixture
        .store
        .set_provider_quota_state(&NewProviderQuotaState {
            windows: vec![
                window(QuotaWindowKind::Weekly, 10, "2026-08-23T09:35:00Z"),
                window(QuotaWindowKind::Weekly, 90, "2026-08-24T09:35:00Z"),
            ],
            ..quota(&fixture, "codex", ProviderQuotaKind::Available, None)
        });
    assert!(
        matches!(refused, Err(RepositoryError::Domain(_))),
        "two readings of one kind is not a richer observation, it is one stale reading: {refused:?}"
    );
}

#[test]
fn a_provider_that_cannot_report_headroom_is_storable_and_does_not_block() {
    let fixture = fixture();
    let stored = fixture
        .store
        .set_provider_quota_state(&quota(
            &fixture,
            "openrouter",
            ProviderQuotaKind::CannotReport,
            None,
        ))
        .expect("cannot-report is a recordable state");
    assert!(
        !stored.blocks_at(at("2099-01-01T00:00:00Z")),
        "failing closed on a figure this provider was never going to give retires it"
    );
    // And it is distinguishable from `unknown` after a restart, which is the
    // whole reason it is a fifth state rather than a reuse of the fourth.
    let reopened = SqliteStore::open(&fixture.path).expect("the realm reopens");
    assert_eq!(
        reopened
            .list_provider_quota_states(fixture.project_id)
            .expect("the read succeeds")[0]
            .state,
        ProviderQuotaKind::CannotReport
    );
}
