//! Durable epic Jira reconciliation authority, conflicts and restart recovery.

use kontor_core::id::{
    AggregateRevision, CanonicalDocument, CommandReceiptId, ContentHash, ExternalId, ExternalName,
    IdempotencyKey, MiniProjectId, ProjectId, SemanticMilestoneKey, SpecVersion, StatusConflictId,
    Timestamp, parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CommandRepository, NewLocalCommand, NewMiniProject, NewProject, ProjectRepository,
    RepositoryError,
};
use kontor_core::ticket::{
    EpicStatusConflict, EpicStatusTransitionIntent, StatusConflictKind, StatusSelector,
};
use kontor_store::SqliteStore;
use kontor_store::backup::export_realm;
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn at(value: &str) -> Timestamp {
    parse_utc_timestamp(value).expect("a canonical timestamp")
}

fn external(value: &str) -> ExternalId {
    ExternalId::parse(value).expect("a valid external id")
}

fn name(value: &str) -> ExternalName {
    ExternalName::parse(value).expect("a valid external name")
}

fn status(id: &str, display: &str) -> StatusSelector {
    StatusSelector {
        status_id: external(id),
        status_name: name(display),
    }
}

fn document(marker: &str) -> CanonicalDocument {
    CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "marker": marker,
    }))
    .expect("a canonical document")
}

struct Fixture {
    _directory: TempDir,
    path: std::path::PathBuf,
    store: SqliteStore,
    project_id: ProjectId,
    epic_id: MiniProjectId,
}

fn fixture() -> Fixture {
    let directory = TempDir::new().expect("a temporary directory");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("the store opens");
    let project_id = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Epic Jira persistence"),
            root_path: name("/tmp/epic-jira-persistence"),
            created_at: at("2026-09-03T10:00:00Z"),
        })
        .expect("the project is created");
    let epic_id = MiniProjectId::generate();
    store
        .create_mini_project(&NewMiniProject {
            id: epic_id,
            project_id,
            name: name("Resident convergence"),
            created_at: at("2026-09-03T10:01:00Z"),
        })
        .expect("the epic is created");
    Fixture {
        _directory: directory,
        path,
        store,
        project_id,
        epic_id,
    }
}

#[test]
fn epic_conflict_schema_refuses_unknown_kinds_bad_timestamps_and_second_resolution() {
    let fixture = fixture();
    let connection = Connection::open(&fixture.path).expect("the database opens directly");
    let insert = |id: StatusConflictId, kind: &str, detected_at: &str| {
        connection.execute(
            "INSERT INTO epic_status_conflicts
                 (id, project_id, epic_id, kind, external_issue_key,
                  observed_status_id, observed_status_name, observed_at,
                  payload_hash, epic_revision, spec_version, milestone,
                  detected_at, resolved_at, resolution_receipt_id)
             VALUES (?1, ?2, ?3, ?4, 'ASMA-8200', '10237', 'DRAFT',
                     '2026-09-03T10:02:00Z', ?5, 1, 1, NULL, ?6, NULL, NULL)",
            params![
                id.to_string(),
                fixture.project_id.to_string(),
                fixture.epic_id.to_string(),
                kind,
                "a".repeat(64),
                detected_at,
            ],
        )
    };
    assert!(
        insert(
            StatusConflictId::generate(),
            "invented_conflict_kind",
            "2026-09-03T10:02:01Z"
        )
        .is_err(),
        "the persisted conflict vocabulary is closed"
    );
    assert!(
        insert(
            StatusConflictId::generate(),
            "no_live_transition",
            "not-a-timestamp"
        )
        .is_err(),
        "the evidence timestamp must be canonical UTC"
    );
    drop(connection);

    let conflict = EpicStatusConflict {
        id: StatusConflictId::generate(),
        epic_id: fixture.epic_id,
        kind: StatusConflictKind::NoLiveTransition,
        external_issue_key: external("ASMA-8200"),
        observed_status: status("10237", "DRAFT"),
        observed_at: at("2026-09-03T10:02:00Z"),
        payload_hash: ContentHash::of(b"immutable conflict"),
        epic_revision: AggregateRevision::INITIAL,
        spec_version: SpecVersion::FIRST,
        milestone: None,
        detected_at: at("2026-09-03T10:02:01Z"),
        resolved_at: None,
        resolution_receipt_id: None,
    };
    fixture
        .store
        .insert_epic_status_conflict(fixture.project_id, &conflict)
        .expect("the conflict is inserted");
    let command = NewLocalCommand {
        project_id: fixture.project_id,
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse("resolve-once-schema").expect("a valid key"),
        kind: CommandKind::ResolveStatusConflict,
        target: AggregateRef::MiniProject {
            mini_project_id: fixture.epic_id,
        },
        target_revision: AggregateRevision::INITIAL,
        intent: document("resolve exactly once"),
        created_at: at("2026-09-03T10:03:00Z"),
    };
    fixture
        .store
        .resolve_epic_jira_conflict_atomically(
            fixture.project_id,
            conflict.id,
            &command,
            at("2026-09-03T10:04:00Z"),
        )
        .expect("the first resolution commits");
    let connection = Connection::open(&fixture.path).expect("the database reopens directly");
    assert!(
        connection
            .execute(
                "UPDATE epic_status_conflicts SET resolved_at = ?1
                 WHERE project_id = ?2 AND id = ?3",
                params![
                    "2026-09-03T10:05:00Z",
                    fixture.project_id.to_string(),
                    conflict.id.to_string(),
                ],
            )
            .is_err(),
        "a resolved conflict cannot be rewritten through direct SQL"
    );
}

fn transition_intent(fixture: &Fixture) -> EpicStatusTransitionIntent {
    EpicStatusTransitionIntent {
        id: CommandReceiptId::generate(),
        epic_id: fixture.epic_id,
        external_issue_key: external("ASMA-8200"),
        idempotency_key: IdempotencyKey::parse("jira-auto-epic-test").expect("a valid key"),
        intent_hash: ContentHash::of(b"epic transition intent"),
        epic_revision: AggregateRevision::parse(1).expect("an initial revision"),
        spec_version: SpecVersion::parse(1).expect("an initial version"),
        milestone: SemanticMilestoneKey::parse("terminal_done").expect("a valid milestone"),
        target: status("10228", "Closed"),
        destination: status("10213", "READY FOR DEVELOPMENT"),
        prior_payload_hash: ContentHash::of(b"prior Jira observation"),
        planned_at: at("2026-09-03T10:02:00Z"),
        confirmed_at: None,
        confirmation_payload_hash: None,
    }
}

#[test]
fn epic_transition_authority_replays_and_recovers_from_confirmed_readback() {
    let fixture = fixture();
    let intent = transition_intent(&fixture);
    let first = fixture
        .store
        .insert_epic_transition_intent(fixture.project_id, &intent)
        .expect("the authority is persisted before dispatch");
    assert_eq!(first, intent.id);

    let mut replay = intent.clone();
    replay.id = CommandReceiptId::generate();
    replay.planned_at = at("2026-09-03T10:03:00Z");
    assert_eq!(
        fixture
            .store
            .insert_epic_transition_intent(fixture.project_id, &replay)
            .expect("the same logical attempt replays"),
        intent.id
    );

    let confirmation = ContentHash::of(b"confirmed Jira observation");
    assert_eq!(
        fixture
            .store
            .confirm_matching_epic_transition_intents(
                fixture.project_id,
                fixture.epic_id,
                &intent.external_issue_key,
                &intent.destination.status_id,
                &confirmation,
                at("2026-09-03T10:04:00Z"),
            )
            .expect("a restart recovers the matching readback"),
        1
    );
    assert_eq!(
        fixture
            .store
            .confirm_matching_epic_transition_intents(
                fixture.project_id,
                fixture.epic_id,
                &intent.external_issue_key,
                &intent.destination.status_id,
                &confirmation,
                at("2026-09-03T10:04:00Z"),
            )
            .expect("the recovery is idempotent"),
        0
    );

    let export = export_realm(&fixture.store, at("2026-09-03T10:05:00Z"))
        .expect("the durable authority is exportable");
    assert_eq!(export.records.epic_jira_transition_intents.len(), 1);
    assert_eq!(
        export.records.epic_jira_transition_intents[0]
            .confirmation_payload_hash
            .as_deref(),
        Some(confirmation.as_str())
    );
}

#[test]
fn epic_transition_key_cannot_authorize_different_intent() {
    let fixture = fixture();
    let intent = transition_intent(&fixture);
    fixture
        .store
        .insert_epic_transition_intent(fixture.project_id, &intent)
        .expect("the first authority is persisted");
    let mut changed = intent;
    changed.intent_hash = ContentHash::of(b"different transition intent");
    assert!(matches!(
        fixture
            .store
            .insert_epic_transition_intent(fixture.project_id, &changed),
        Err(RepositoryError::Conflict { .. })
    ));
}

#[test]
fn one_open_epic_conflict_is_kept_until_operator_resolution() {
    let fixture = fixture();
    let conflict = EpicStatusConflict {
        id: StatusConflictId::generate(),
        epic_id: fixture.epic_id,
        kind: StatusConflictKind::NoLiveTransition,
        external_issue_key: external("ASMA-8200"),
        observed_status: status("10237", "DRAFT"),
        observed_at: at("2026-09-03T10:02:00Z"),
        payload_hash: ContentHash::of(b"first conflicting observation"),
        epic_revision: AggregateRevision::parse(1).expect("an initial revision"),
        spec_version: SpecVersion::parse(1).expect("an initial version"),
        milestone: Some(SemanticMilestoneKey::parse("terminal_done").expect("a valid milestone")),
        detected_at: at("2026-09-03T10:02:01Z"),
        resolved_at: None,
        resolution_receipt_id: None,
    };
    assert!(
        fixture
            .store
            .insert_epic_status_conflict(fixture.project_id, &conflict)
            .expect("the first conflict is recorded")
    );
    let mut repeated = conflict.clone();
    repeated.id = StatusConflictId::generate();
    assert!(
        !fixture
            .store
            .insert_epic_status_conflict(fixture.project_id, &repeated)
            .expect("the exact observation de-duplicates")
    );
    let mut contradictory = repeated;
    contradictory.payload_hash = ContentHash::of(b"different evidence under the same kind");
    assert!(matches!(
        fixture
            .store
            .insert_epic_status_conflict(fixture.project_id, &contradictory),
        Err(RepositoryError::Conflict { .. })
    ));

    let stored = fixture
        .store
        .list_epic_status_conflicts(fixture.project_id, fixture.epic_id)
        .expect("the conflict ledger is readable");
    assert_eq!(stored, vec![conflict.clone()]);

    let receipt = CommandReceiptId::generate();
    let command = NewLocalCommand {
        project_id: fixture.project_id,
        receipt_id: receipt,
        idempotency_key: IdempotencyKey::parse("resolve-epic-jira-conflict").expect("a valid key"),
        kind: CommandKind::ResolveStatusConflict,
        target: AggregateRef::MiniProject {
            mini_project_id: fixture.epic_id,
        },
        target_revision: AggregateRevision::INITIAL,
        intent: document("resolve epic conflict"),
        created_at: at("2026-09-03T10:03:00Z"),
    };
    let outcome = fixture
        .store
        .resolve_epic_jira_conflict_atomically(
            fixture.project_id,
            conflict.id,
            &command,
            at("2026-09-03T10:04:00Z"),
        )
        .expect("the exact epic conflict is resolved");
    assert!(outcome.is_fresh());
    let resolved = fixture
        .store
        .list_epic_status_conflicts(fixture.project_id, fixture.epic_id)
        .expect("the conflict reads back")
        .into_iter()
        .find(|candidate| candidate.id == conflict.id)
        .expect("the conflict remains durable");
    assert_eq!(resolved.resolution_receipt_id, Some(receipt));
    assert!(resolved.resolved_at.is_some());
    let replay = fixture
        .store
        .resolve_epic_jira_conflict_atomically(
            fixture.project_id,
            conflict.id,
            &command,
            at("2026-09-03T10:05:00Z"),
        )
        .expect("the same key replays its own resolution");
    assert!(!replay.is_fresh());

    let competing = NewLocalCommand {
        receipt_id: CommandReceiptId::generate(),
        idempotency_key: IdempotencyKey::parse("competing-epic-conflict-close")
            .expect("a valid key"),
        ..command
    };
    assert!(
        fixture
            .store
            .resolve_epic_jira_conflict_atomically(
                fixture.project_id,
                conflict.id,
                &competing,
                at("2026-09-03T10:06:00Z"),
            )
            .is_err(),
        "a second key cannot claim another key's resolution"
    );
    assert!(
        fixture
            .store
            .get_receipt_by_key(&competing.idempotency_key)
            .expect("the receipt ledger reads")
            .is_none(),
        "the losing key leaves no receipt for an effect it did not perform"
    );
}
