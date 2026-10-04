//! The migrated-realm template every store fixture is cloned from.
//!
//! Opening a realm replays the complete migration history — eleven dozen
//! generations of DDL — and measures about two and a half seconds of SQLite
//! work per fresh database, paid again by every test in this suite. The
//! daemon's test harness solved the same problem by migrating once per process
//! and cloning the file (`crates/kontor-daemon/tests/harness/mod.rs`), and this
//! is the same mechanism for the store's own suite. The template is a
//! genuinely migrated realm, so nothing about the open path is faked.
//!
//! The Realm identity inside it is therefore *shared* by every clone. That is
//! not a shortcut around the schema but a consequence of it: a realm row may
//! not be updated or deleted, so a copy cannot be re-identified. A test that
//! compares the identity of two independently created realms — the backup and
//! export suites above all — must open at least one database of its own with
//! [`open_created_realm`].

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use kontor_store::SqliteStore;
use tempfile::TempDir;

/// The database file every fixture and world uses.
const DATABASE_FILE: &str = "kontor.db";

/// A state root already at the current schema version, migrated once per
/// process and cloned by every database after that.
fn migrated_state_root() -> &'static TempDir {
    static TEMPLATE: OnceLock<TempDir> = OnceLock::new();
    TEMPLATE.get_or_init(|| {
        let root = TempDir::new().expect("a template state root");
        SqliteStore::open(&root.path().join(DATABASE_FILE)).expect("the template realm migrates");
        // Closing the store above checkpoints the write-ahead log and truncates
        // it, so the database file is complete on its own — which is the whole
        // reason a plain copy of it is a whole realm. Frames left in the log
        // would mean the copy silently lost committed schema, so this is a
        // failure and not a warning.
        let log = root.path().join(format!("{DATABASE_FILE}-wal"));
        assert!(
            log.metadata().map_or(true, |metadata| metadata.len() == 0),
            "the template's write-ahead log still holds frames"
        );
        root
    })
}

/// A temporary state root whose database is already at the current schema.
///
/// For a test that opens the database with `SqliteStore::open` itself — and
/// reopens it, or holds two handles — pre-installing the migrated database
/// leaves every open the call under test: on an already-current file, `open` is
/// the idempotent path (load the Realm row) rather than the full migration
/// replay. A test that is *about* creation must build its root with
/// `TempDir::new` instead.
pub(crate) fn state_root() -> TempDir {
    let root = TempDir::new().expect("a temporary directory");
    std::fs::copy(template_database(), root.path().join(DATABASE_FILE))
        .expect("the migrated template is installed");
    root
}

/// Copy the migrated template to `path` and open the clone.
///
/// For a fixture that builds one database: one file copy plus the idempotent
/// open of an already-current database, instead of the full migration chain.
pub(crate) fn store_from_template(path: &Path) -> SqliteStore {
    std::fs::copy(template_database(), path).expect("the migrated template is cloned");
    SqliteStore::open(path).expect("the cloned store opens")
}

/// The template database file.
fn template_database() -> PathBuf {
    migrated_state_root().path().join(DATABASE_FILE)
}

/// Open a database of its own, paying the full migration chain.
///
/// For a test that is about creation, identity or the schema itself.
pub(crate) fn open_created_realm(path: &Path) -> SqliteStore {
    SqliteStore::open(path).expect("the store opens")
}

/// A state root holding a realm of its own, created by the full migration
/// chain — for a test that must tell two realms apart.
///
/// An export imported into a clone of its own realm is refused as a
/// same-realm import, so a test about import lineage needs a destination the
/// template cannot provide.
pub(crate) fn created_state_root() -> TempDir {
    let root = TempDir::new().expect("a temporary directory");
    SqliteStore::open(&root.path().join(DATABASE_FILE)).expect("the realm is created");
    root
}
