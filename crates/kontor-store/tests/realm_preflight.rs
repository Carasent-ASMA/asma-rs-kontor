//! Operator preflight validates existing identity without initialization/migration.
use kontor_store::SqliteStore;
#[test]
fn realm_preflight_reads_existing_identity_without_changing_database_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kontor.sqlite3");
    let store = SqliteStore::open(&path).unwrap();
    let expected = store.realm_metadata().clone();
    drop(store);
    let before = std::fs::read(&path).unwrap();
    assert_eq!(SqliteStore::read_existing_realm(&path).unwrap(), expected);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
#[test]
fn realm_preflight_refuses_absent_empty_or_unsupported_databases_without_repair() {
    let root = tempfile::tempdir().unwrap();
    let absent = root.path().join("absent.sqlite3");
    assert!(SqliteStore::read_existing_realm(&absent).is_err());
    assert!(!absent.exists());
    let empty = root.path().join("empty.sqlite3");
    drop(rusqlite::Connection::open(&empty).unwrap());
    let before = std::fs::read(&empty).unwrap();
    assert!(SqliteStore::read_existing_realm(&empty).is_err());
    assert_eq!(std::fs::read(&empty).unwrap(), before);
    let path = root.path().join("existing.sqlite3");
    drop(SqliteStore::open(&path).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 0).unwrap();
    drop(connection);
    let before = std::fs::read(&path).unwrap();
    assert!(SqliteStore::read_existing_realm(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
