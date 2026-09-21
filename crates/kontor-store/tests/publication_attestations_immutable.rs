//! Publication attestation rows refuse update and delete after recording (ASMA-8102).

use kontor_core::id::{
    ExternalName, IdempotencyKey, ProjectId, PublicationAttestationId, parse_utc_timestamp,
};
use kontor_core::publication::{CommitSha, PublicationDecision};
use kontor_core::repository::{NewProject, ProjectRepository};
use kontor_store::SqliteStore;
use kontor_store::publication::{AttestationRecord, NewPublicationAttestation};
use rusqlite::params;
use tempfile::TempDir;

#[test]
fn recorded_publication_attestations_are_immutable_at_the_database_boundary() {
    let directory = TempDir::new().expect("temp dir");
    let path = directory.path().join("kontor.db");
    let store = SqliteStore::open(&path).expect("store opens at current schema");
    assert_eq!(store.schema_version().expect("schema version"), 119);

    let project = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: ExternalName::parse("Publication immutability").expect("name"),
            root_path: ExternalName::parse("/tmp/publication-immutable").expect("path"),
            created_at: parse_utc_timestamp("2026-09-22T00:00:00Z").expect("ts"),
        })
        .expect("project");

    let attestation_id = PublicationAttestationId::generate();
    let key = IdempotencyKey::parse("asma-8102-immutability-probe").expect("key");
    let head = CommitSha::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").expect("sha");
    let recorded = store
        .record_publication_attestation(&NewPublicationAttestation {
            id: attestation_id,
            project_id: project,
            mini_project_id: None,
            task_id: None,
            repository: ExternalName::parse("Carasent-ASMA/asma-rs-kontor").expect("repo"),
            base_branch: ExternalName::parse("master").expect("base"),
            head_branch: ExternalName::parse("feat/ASMA-8102-probe").expect("head"),
            head_sha: head,
            pull_request: None,
            title: None,
            decision: PublicationDecision::accepted(),
            idempotency_key: key,
            recorded_at: parse_utc_timestamp("2026-09-22T00:01:00Z").expect("ts"),
        })
        .expect("record");
    assert!(matches!(recorded, AttestationRecord::Recorded(_)));

    let connection = rusqlite::Connection::open(&path).expect("raw connection");
    assert!(
        connection
            .execute(
                "UPDATE publication_attestations SET accepted = 0, reasons = '[\"forged\"]'
                 WHERE id = ?1",
                params![attestation_id.to_string()],
            )
            .is_err(),
        "update must be refused"
    );
    assert!(
        connection
            .execute(
                "DELETE FROM publication_attestations WHERE id = ?1",
                params![attestation_id.to_string()],
            )
            .is_err(),
        "delete must be refused"
    );

    let reread = store
        .get_publication_attestation(project, attestation_id)
        .expect("read")
        .expect("row remains");
    assert!(reread.decision.accepted);
}
