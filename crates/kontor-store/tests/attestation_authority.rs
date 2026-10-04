//! Durable public metadata, CAS rollback and SQLite guard behavior only.
//! No fixture proves authentic live acquisition, issuance or native admission.

mod support;

use std::sync::{Arc, Barrier};

use kontor_core::id::{ContentHash, ExternalId, ExternalName, ProjectId, RealmId, Timestamp};
use kontor_core::repository::{
    AttestationAuthorityRepository, AttestationAuthorityScope, NewProject, ProjectRepository,
    RegisterAttestationKey, RepositoryError,
};
use kontor_store::SqliteStore;
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn id(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("id")
}

fn project(store: &SqliteStore) -> ProjectId {
    let project_id = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project_id,
            name: ExternalName::parse(&format!("Public ledger fixture {project_id}"))
                .expect("name"),
            root_path: ExternalName::parse(&format!("/tmp/public-ledger-fixture/{project_id}"))
                .expect("path"),
            created_at: Timestamp::now(),
        })
        .expect("project");
    project_id
}

fn fixture() -> (TempDir, SqliteStore, AttestationAuthorityScope) {
    let root = support::state_root();
    let store = SqliteStore::open(&root.path().join("kontor.db")).expect("store");
    let scope = AttestationAuthorityScope {
        realm_id: store.realm_id(),
        project_id: project(&store),
        application: id("asma.planning-pair.application.v1"),
    };
    (root, store, scope)
}

fn registration(
    scope: &AttestationAuthorityScope,
    head: Option<u64>,
    name: &str,
) -> RegisterAttestationKey {
    RegisterAttestationKey {
        scope: scope.clone(),
        expected_head_revision: head,
        issuer: id("issuer"),
        key_id: id(name),
        public_key_der: vec![1, 2, 3],
        not_before: 10,
        expires_at: 1000,
    }
}

fn sql(root: &TempDir) -> Connection {
    let connection = Connection::open(root.path().join("kontor.db")).expect("connection");
    connection
        .execute_batch("PRAGMA foreign_keys=ON")
        .expect("FK");
    connection
}

#[test]
fn first_registration_advances_once_and_read_projection_survives_reopen() {
    let (root, store, scope) = fixture();
    assert!(
        store
            .read_attestation_key_authority(&scope, &id("issuer"), &id("key-1"))
            .expect("read")
            .is_none()
    );
    let projection = store
        .register_attestation_key(&registration(&scope, None, "key-1"))
        .expect("register");
    assert_eq!(projection.head_revision, 1);
    let key = projection.selected_key.as_ref().expect("key");
    assert_eq!(key.public_key_der, [1, 2, 3]);
    assert_eq!(key.material_digest, ContentHash::of(&[1, 2, 3]));
    assert_eq!((key.registered_revision, key.revoked_revision), (1, None));
    let missing = store
        .read_attestation_key_authority(&scope, &id("issuer"), &id("missing"))
        .expect("read")
        .expect("head");
    assert_eq!(missing.head_revision, 1);
    assert!(missing.selected_key.is_none());
    drop(store);
    let reopened = SqliteStore::open(&root.path().join("kontor.db")).expect("reopen");
    assert_eq!(
        reopened
            .read_attestation_key_authority(&scope, &id("issuer"), &id("key-1"))
            .expect("read"),
        Some(projection)
    );
}

#[test]
fn exact_expected_head_is_required_and_failed_writes_leave_no_partial_mutation() {
    let (_root, store, scope) = fixture();
    assert!(matches!(
        store.register_attestation_key(&registration(&scope, Some(1), "key-1")),
        Err(RepositoryError::Conflict { .. })
    ));
    let before = store
        .register_attestation_key(&registration(&scope, None, "key-1"))
        .expect("register");
    for expected in [None, Some(2)] {
        assert!(matches!(
            store.register_attestation_key(&registration(&scope, expected, "key-2")),
            Err(RepositoryError::Conflict { .. })
        ));
        assert_eq!(
            store
                .read_attestation_key_authority(&scope, &id("issuer"), &id("key-1"))
                .expect("read"),
            Some(before.clone())
        );
        assert!(
            store
                .read_attestation_key_authority(&scope, &id("issuer"), &id("key-2"))
                .expect("read")
                .expect("head")
                .selected_key
                .is_none()
        );
    }
    assert!(matches!(
        store.revoke_attestation_key(&scope, &id("issuer"), &id("key-1"), 2),
        Err(RepositoryError::Conflict { .. })
    ));
    assert_eq!(
        store
            .read_attestation_key_authority(&scope, &id("issuer"), &id("key-1"))
            .expect("read"),
        Some(before)
    );
}

#[test]
fn key_identity_is_never_reused_and_revocation_is_irreversible_under_replay() {
    let (_root, store, scope) = fixture();
    store
        .register_attestation_key(&registration(&scope, None, "key-1"))
        .expect("register");
    let mut changed = registration(&scope, Some(1), "key-1");
    changed.public_key_der = vec![4, 5];
    assert!(matches!(
        store.register_attestation_key(&changed),
        Err(RepositoryError::Conflict { .. })
    ));
    let revoked = store
        .revoke_attestation_key(&scope, &id("issuer"), &id("key-1"), 1)
        .expect("revoke");
    assert_eq!(revoked.head_revision, 2);
    assert_eq!(
        revoked.selected_key.as_ref().expect("key").revoked_revision,
        Some(2)
    );
    assert!(matches!(
        store.revoke_attestation_key(&scope, &id("issuer"), &id("key-1"), 1),
        Err(RepositoryError::Conflict { .. })
    ));
    assert_eq!(
        store
            .revoke_attestation_key(&scope, &id("issuer"), &id("key-1"), 2)
            .expect("unchanged revocation"),
        revoked
    );
    changed.expected_head_revision = Some(2);
    assert!(matches!(
        store.register_attestation_key(&changed),
        Err(RepositoryError::Conflict { .. })
    ));
    assert!(matches!(
        store.revoke_attestation_key(&scope, &id("issuer"), &id("missing"), 2),
        Err(RepositoryError::NotFound { .. })
    ));
    assert_eq!(
        store
            .read_attestation_key_authority(&scope, &id("issuer"), &id("key-1"))
            .expect("read"),
        Some(revoked)
    );
}

#[test]
fn realm_project_and_application_scopes_never_leak_or_create_unknown_rows() {
    let (_root, store, scope) = fixture();
    store
        .register_attestation_key(&registration(&scope, None, "key-1"))
        .expect("register");
    for foreign in [
        AttestationAuthorityScope {
            realm_id: RealmId::generate(),
            ..scope.clone()
        },
        AttestationAuthorityScope {
            project_id: ProjectId::generate(),
            ..scope.clone()
        },
    ] {
        assert!(
            store
                .read_attestation_key_authority(&foreign, &id("issuer"), &id("key-1"))
                .expect("read")
                .is_none()
        );
        assert!(matches!(
            store.register_attestation_key(&registration(&foreign, None, "key-2")),
            Err(RepositoryError::NotFound { .. })
        ));
        assert!(matches!(
            store.revoke_attestation_key(&foreign, &id("issuer"), &id("key-1"), 1),
            Err(RepositoryError::NotFound { .. })
        ));
    }
    let other_project = AttestationAuthorityScope {
        project_id: project(&store),
        ..scope.clone()
    };
    assert!(
        store
            .read_attestation_key_authority(&other_project, &id("issuer"), &id("key-1"))
            .expect("other project")
            .is_none()
    );
    let mut own = registration(&other_project, None, "key-1");
    own.public_key_der = vec![9];
    assert_eq!(
        store
            .register_attestation_key(&own)
            .expect("independent project")
            .head_revision,
        1
    );
    let other_application = AttestationAuthorityScope {
        application: id("other.application"),
        ..scope.clone()
    };
    assert!(
        store
            .read_attestation_key_authority(&other_application, &id("issuer"), &id("key-1"))
            .expect("other application")
            .is_none()
    );
    assert_eq!(
        store
            .read_attestation_key_authority(&scope, &id("issuer"), &id("key-1"))
            .expect("original")
            .expect("head")
            .selected_key
            .expect("key")
            .public_key_der,
        [1, 2, 3]
    );
}

#[test]
fn invalid_bounds_intervals_and_sqlite_integer_overflow_do_not_create_a_head() {
    let (_root, store, scope) = fixture();
    let edits: &[fn(&mut RegisterAttestationKey)] = &[
        |r| r.public_key_der.clear(),
        |r| r.public_key_der.resize(2049, 0),
        |r| r.expires_at = r.not_before,
        |r| r.expires_at = r.not_before - 1,
        |r| r.expires_at = u64::MAX,
        |r| {
            r.not_before = i64::MAX as u64 + 1;
            r.expires_at = u64::MAX;
        },
        |r| r.expected_head_revision = Some(0),
        |r| r.expected_head_revision = Some(u64::MAX),
        |r| r.issuer = id(&"é".repeat(129)),
        |r| r.key_id = id(&"é".repeat(129)),
        |r| r.scope.application = id(&"é".repeat(129)),
    ];
    for edit in edits {
        let mut request = registration(&scope, None, "key-1");
        edit(&mut request);
        assert!(store.register_attestation_key(&request).is_err());
        assert!(
            store
                .read_attestation_key_authority(&scope, &id("issuer"), &id("key-1"))
                .expect("read")
                .is_none()
        );
    }
    let mut boundary = registration(&scope, None, "key-1");
    boundary.issuer = id(&"é".repeat(128));
    boundary.key_id = id(&"é".repeat(128));
    boundary.public_key_der = vec![0; 2048];
    boundary.not_before = 0;
    boundary.expires_at = i64::MAX as u64;
    assert_eq!(
        store
            .register_attestation_key(&boundary)
            .expect("exact boundaries")
            .head_revision,
        1
    );
}

#[test]
fn historical_ledger_is_not_limited_to_the_transport_key_count() {
    let (root, store, scope) = fixture();
    for index in 0..70 {
        let expected = if index == 0 { None } else { Some(index) };
        assert_eq!(
            store
                .register_attestation_key(&registration(&scope, expected, &format!("key-{index}")))
                .expect("rotation metadata")
                .head_revision,
            index + 1
        );
    }
    let first = store
        .read_attestation_key_authority(&scope, &id("issuer"), &id("key-0"))
        .expect("read")
        .expect("head");
    assert_eq!(first.head_revision, 70);
    assert_eq!(first.selected_key.expect("retained").registered_revision, 1);
    assert_eq!(
        sql(&root)
            .query_row(
                "SELECT count(*) FROM attestation_authority_keys",
                [],
                |row| row.get::<_, i64>(0)
            )
            .expect("count"),
        70
    );
}

#[test]
fn checked_revision_overflow_refuses_registration_and_revocation_atomically() {
    let (root, store, scope) = fixture();
    store
        .register_attestation_key(&registration(&scope, None, "key-1"))
        .expect("register");
    let connection = sql(&root);
    // Disposable fixture simulates an exhausted head without billions of writes.
    connection
        .execute_batch("DROP TRIGGER attestation_heads_monotonic")
        .expect("fixture only");
    connection
        .execute(
            "UPDATE attestation_authority_heads SET revision=?1",
            [i64::MAX],
        )
        .expect("exhausted fixture");
    assert!(matches!(
        store.register_attestation_key(&registration(&scope, Some(i64::MAX as u64), "key-2")),
        Err(RepositoryError::Conflict { .. })
    ));
    assert!(matches!(
        store.revoke_attestation_key(&scope, &id("issuer"), &id("key-1"), i64::MAX as u64),
        Err(RepositoryError::Conflict { .. })
    ));
    let projection = store
        .read_attestation_key_authority(&scope, &id("issuer"), &id("key-1"))
        .expect("read")
        .expect("head");
    assert_eq!(projection.head_revision, i64::MAX as u64);
    assert_eq!(projection.selected_key.expect("key").revoked_revision, None);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM attestation_authority_keys", [], |r| r
                .get::<_, i64>(0))
            .expect("count"),
        1
    );
}

#[test]
fn direct_sql_cannot_change_material_unrevoke_delete_or_jump_the_head() {
    let (root, store, scope) = fixture();
    store
        .register_attestation_key(&registration(&scope, None, "key-1"))
        .expect("register");
    let connection = sql(&root);
    // SQLite REPLACE can delete a conflicting row without firing DELETE
    // triggers. Registration guards must refuse identity reuse themselves.
    assert!(connection.execute("INSERT OR REPLACE INTO attestation_authority_keys (project_id,application,issuer,key_id,public_key_der,material_digest,not_before,expires_at,registered_revision,revoked_revision) SELECT project_id,application,issuer,key_id,?1,?2,not_before,expires_at,2,NULL FROM attestation_authority_keys", params![vec![4_u8, 5], ContentHash::of(&[4, 5]).as_str()]).is_err());
    assert!(connection.execute("INSERT OR REPLACE INTO attestation_authority_heads SELECT project_id,application,revision FROM attestation_authority_heads", []).is_err());
    let material_change = connection.execute("UPDATE attestation_authority_keys SET public_key_der=?1, material_digest=?2, revoked_revision=2", params![vec![4_u8, 5], ContentHash::of(&[4, 5]).as_str()]);
    assert!(
        material_change.is_err(),
        "even a valid next revocation cannot replace immutable material"
    );
    assert!(
        connection
            .execute("UPDATE attestation_authority_heads SET revision=99", [])
            .is_err()
    );
    store
        .revoke_attestation_key(&scope, &id("issuer"), &id("key-1"), 1)
        .expect("revoke");
    for statement in [
        "UPDATE attestation_authority_keys SET revoked_revision=NULL",
        "UPDATE attestation_authority_keys SET revoked_revision=3",
        "DELETE FROM attestation_authority_keys",
        "DELETE FROM attestation_authority_heads",
        "UPDATE attestation_authority_heads SET revision=1",
    ] {
        assert!(connection.execute(statement, []).is_err(), "{statement}");
    }
    let projection = store
        .read_attestation_key_authority(&scope, &id("issuer"), &id("key-1"))
        .expect("read")
        .expect("head");
    assert_eq!(projection.head_revision, 2);
    assert_eq!(
        projection.selected_key.expect("key").public_key_der,
        [1, 2, 3]
    );
}

#[test]
fn failed_migration_127_rolls_back_its_new_head_and_version() {
    let (root, store, _scope) = fixture();
    drop(store);
    let connection = sql(&root);
    connection
        .execute_batch(
            "DROP TABLE attestation_authority_keys; DROP TABLE attestation_authority_heads;
        CREATE TABLE attestation_authority_keys (collision INTEGER); PRAGMA user_version=126;",
        )
        .expect("pre-v127 collision fixture");
    drop(connection);
    assert!(SqliteStore::open(&root.path().join("kontor.db")).is_err());
    let connection = sql(&root);
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .expect("version"),
        126
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='attestation_authority_heads'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .expect("count"),
        0
    );
}

#[test]
fn two_store_writers_cannot_both_satisfy_the_same_absent_head_expectation() {
    let (root, store, scope) = fixture();
    drop(store);
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for name in ["key-a", "key-b"] {
        let path = root.path().join("kontor.db");
        let scope = scope.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(std::thread::spawn(move || {
            let store = SqliteStore::open(&path).expect("store");
            barrier.wait();
            store.register_attestation_key(&registration(&scope, None, name))
        }));
    }
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("thread"))
        .collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(RepositoryError::Conflict { .. })))
            .count(),
        1
    );
    let connection = sql(&root);
    assert_eq!(
        connection
            .query_row(
                "SELECT revision FROM attestation_authority_heads",
                [],
                |r| r.get::<_, i64>(0)
            )
            .expect("head"),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM attestation_authority_keys", [], |r| r
                .get::<_, i64>(0))
            .expect("count"),
        1
    );
}
