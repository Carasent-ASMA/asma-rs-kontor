//! OP-03 CP3 — raw capacity evidence is immutable, and an override stands
//! beside it rather than over it.
//!
//! These are the store-level halves of two required negative proofs: a refresh
//! that stored only derived state, and an override that rewrote what the
//! provider reported. Both are killed here structurally — the evidence row
//! cannot be updated at all — and again at the API boundary in the daemon's
//! black-box suite.

use kontor_core::id::{
    AccountProfileId, AggregateRevision, CanonicalDocument, CapacityObservationId, CredentialAlias,
    ExternalName, ProjectId, RuntimeKindKey, Timestamp, parse_utc_timestamp,
};
use kontor_core::repository::{
    CapacityRepository, CredentialReference, CredentialReferenceKind, NewAccountProfile,
    NewAvailabilityOverride, NewCapacityObservation, NewProject, ProjectRepository,
    RepositoryError,
};
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
    _home: TempDir,
}

fn fixture() -> Fixture {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
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
