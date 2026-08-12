//! Snapshots, corruption, retention and restore, against real files.
//!
//! Every test here works on a real database in a real directory, because the
//! properties under test are filesystem properties: what exists after a failure,
//! what was replaced, what was removed and what a second process would find.
//!
//! The mutants this suite exists to kill:
//!
//! * copying a database that failed its integrity check, or publishing a copy
//!   that failed one;
//! * a truncated, header-damaged or page-damaged file passing verification;
//! * a manifest that does not describe the file beside it being accepted;
//! * overwriting a published snapshot, or replacing a valid backup during a
//!   failed one;
//! * retention deleting the newest snapshot, a foreign Realm's snapshot, a
//!   partial or an unverifiable file;
//! * restoring into a *different* initialized Realm, or mutating the
//!   destination before finding out that it is one.

use std::path::{Path, PathBuf};

use kontor_core::id::{ExternalName, ProjectId, RealmId, Timestamp, parse_utc_timestamp};
use kontor_core::repository::{NewProject, ProjectRepository};
use kontor_store::backup::{
    BackupError, RETAINED_SNAPSHOTS, SnapshotManifest, create_snapshot, list_snapshots,
    prune_snapshots, restore_snapshot,
};
use kontor_store::{SCHEMA_VERSION, SqliteStore};
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid external name")
}

/// A migrated database with `projects` rows in it.
fn seeded(path: &Path, projects: usize) -> SqliteStore {
    let store = SqliteStore::open(path).expect("the database migrates");
    for index in 0..projects {
        store
            .create_project(&NewProject {
                id: ProjectId::generate(),
                name: name(&format!("Project {index}")),
                root_path: name(&format!("/tmp/kontor-{index}")),
                created_at: at("2026-08-10T09:00:00Z"),
            })
            .expect("a project is created");
    }
    store
}

/// Count the rows a snapshot carries, through a fresh read-only connection.
fn projects_in(database: &Path) -> i64 {
    let connection =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("the snapshot opens");
    connection
        .query_row("SELECT count(*) FROM projects", [], |row| row.get(0))
        .expect("the snapshot has a projects table")
}

/// Every file in a directory, sorted, as text.
fn listing(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .expect("the directory is readable")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_snapshot_taken_while_the_database_is_written_is_a_consistent_point() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = seeded(&database, 4);
    let before = projects_in(&database);

    // A second connection keeps committing throughout the copy, which is the
    // situation `VACUUM INTO` exists for: the daemon does not stop to be backed
    // up. The writer is a thread rather than an injected hook so the interleaving
    // is real rather than arranged.
    let writing = database.clone();
    let writer = std::thread::spawn(move || {
        let live = SqliteStore::open(&writing).expect("a second connection opens");
        for index in 0..64 {
            live.create_project(&NewProject {
                id: ProjectId::generate(),
                name: name(&format!("Concurrent {index}")),
                root_path: name(&format!("/tmp/kontor-concurrent-{index}")),
                created_at: at("2026-08-10T09:00:00Z"),
            })
            .expect("a concurrent project is created");
        }
    });

    let outcome = create_snapshot(
        &database,
        &home.path().join("backups"),
        at("2026-08-10T10:00:00Z"),
    )
    .expect("the snapshot is taken while the database is written");
    writer.join().expect("the writer finishes");

    // The copy is a whole database: it verifies, it carries this build's schema
    // version, and it holds a *committed* prefix of the writer's work — never a
    // half-written transaction.
    outcome
        .manifest
        .verify_file(&outcome.snapshot)
        .expect("the published snapshot matches its manifest");
    assert_eq!(outcome.manifest.database_schema_version, SCHEMA_VERSION);
    assert_eq!(outcome.manifest.realm_id, store.realm_id());
    let copied = projects_in(&outcome.snapshot);
    assert!(
        copied >= before && copied <= before + 64,
        "the snapshot holds a committed point, not a partial transaction"
    );

    let restored = SqliteStore::open(&outcome.snapshot).expect("the snapshot reopens as a store");
    restored
        .integrity_check()
        .expect("the snapshot passes an integrity check");
    restored
        .foreign_key_check()
        .expect("the snapshot has no dangling reference");
    assert_eq!(restored.realm_id(), store.realm_id());
}

#[test]
fn a_damaged_database_is_never_copied_and_replaces_nothing() {
    // Three shapes of damage, and each one has to fail closed on its own: a
    // truncation, a destroyed header, and a corrupted interior page.
    for (label, damage) in [
        ("truncated", Damage::Truncate),
        ("invalid header", Damage::Header),
        ("page corruption", Damage::Page),
    ] {
        let home = TempDir::new().expect("a temporary directory");
        let database = home.path().join("kontor.db");
        let backups = home.path().join("backups");
        let store = seeded(&database, 32);
        let realm = store.realm_id();
        drop(store);

        // One good snapshot first. It is the evidence the failed run must not
        // touch.
        let good = create_snapshot(&database, &backups, at("2026-08-10T10:00:00Z"))
            .expect("the healthy database is copied");
        let published = listing(&backups);

        damage.apply(&database);

        let refused = create_snapshot(&database, &backups, at("2026-08-10T11:00:00Z"))
            .expect_err(&format!("a {label} database must not be copied"));
        assert!(
            matches!(refused, BackupError::Verification { .. }),
            "a {label} database fails verification, not something else: {refused:?}"
        );

        assert_eq!(
            listing(&backups),
            published,
            "a {label} database left the backup directory changed"
        );
        good.manifest
            .verify_file(&good.snapshot)
            .expect("the previous snapshot is untouched");
        // And retention still refuses to remove the one good backup there is.
        assert!(
            prune_snapshots(&backups, realm)
                .expect("retention runs")
                .is_empty(),
            "a {label} database must not cause a valid backup to be pruned"
        );
    }
}

/// The ways a database file gets damaged in the field.
enum Damage {
    /// Cut short — the classic half-copied file.
    Truncate,
    /// The first sixteen bytes are SQLite's magic string.
    Header,
    /// One interior page rewritten with rubbish.
    Page,
}

impl Damage {
    fn apply(&self, database: &Path) {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(database)
            .expect("the database is writable");
        let length = file.metadata().expect("the file has metadata").len();
        match self {
            Self::Truncate => file.set_len(length / 2).expect("the file is truncated"),
            Self::Header => {
                file.seek(SeekFrom::Start(0))
                    .expect("the header is seekable");
                file.write_all(b"not a database!!")
                    .expect("the header is overwritten");
            }
            Self::Page => {
                // Page 1 is the schema; damaging a later page is the case that a
                // header check alone would miss.
                file.seek(SeekFrom::Start(4096 * 3 + 32))
                    .expect("an interior page is seekable");
                file.write_all(&[0x5a; 512])
                    .expect("an interior page is overwritten");
            }
        }
        file.sync_all().expect("the damage is on the device");
    }
}

#[test]
fn a_published_snapshot_is_never_overwritten() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let backups = home.path().join("backups");
    drop(seeded(&database, 2));

    let instant = at("2026-08-10T10:00:00Z");
    let first = create_snapshot(&database, &backups, instant).expect("the first snapshot");
    let refused = create_snapshot(&database, &backups, instant)
        .expect_err("a second snapshot at the same instant collides");
    assert!(matches!(refused, BackupError::AlreadyExists { .. }));
    first
        .manifest
        .verify_file(&first.snapshot)
        .expect("the published snapshot is exactly as it was");
}

#[test]
fn a_manifest_that_does_not_describe_its_snapshot_is_refused() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let backups = home.path().join("backups");
    drop(seeded(&database, 2));
    let snapshot = create_snapshot(&database, &backups, at("2026-08-10T10:00:00Z"))
        .expect("the snapshot is taken");

    // A snapshot whose bytes changed after it was published: the manifest is the
    // only thing that notices.
    let mut bytes = std::fs::read(&snapshot.snapshot).expect("the snapshot is readable");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&snapshot.snapshot, &bytes).expect("the snapshot is rewritten");
    let refused = snapshot
        .manifest
        .verify_file(&snapshot.snapshot)
        .expect_err("a rewritten snapshot fails its manifest");
    assert!(matches!(refused, BackupError::Verification { .. }));

    // And a manifest this build did not write is refused rather than guessed at.
    let manifest_path = SnapshotManifest::path_for(&snapshot.snapshot);
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).expect("the manifest is readable"))
            .expect("the manifest is JSON");
    document["format_version"] = serde_json::json!(2);
    std::fs::write(&manifest_path, document.to_string()).expect("the manifest is rewritten");
    let refused = SnapshotManifest::read(&manifest_path)
        .expect_err("a future manifest generation is refused");
    assert!(matches!(refused, BackupError::Manifest { .. }));

    // A restore of that snapshot is refused before it can touch anything.
    let destination = home.path().join("restored").join("kontor.db");
    let refused = restore_snapshot(&snapshot.snapshot, &destination, at("2026-08-10T12:00:00Z"))
        .expect_err("a snapshot with a malformed manifest is not restored");
    assert!(matches!(refused, BackupError::Manifest { .. }));
    assert!(
        !destination.exists(),
        "a refused restore must not have created the destination"
    );
}

#[test]
fn retention_keeps_the_newest_seven_of_this_realm_and_touches_nothing_else() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let backups = home.path().join("backups");
    let realm = seeded(&database, 3).realm_id();

    let mut published = Vec::new();
    for hour in 0..10 {
        published.push(
            create_snapshot(
                &database,
                &backups,
                at(&format!("2026-08-10T{hour:02}:00:00Z")),
            )
            .expect("a snapshot is published"),
        );
    }

    // Three files retention must be blind to: another Realm's snapshot, a
    // partial, and a file with no manifest at all.
    let foreign_realm = RealmId::generate();
    let foreign = backups.join(format!(
        "{}.db",
        SnapshotManifest::file_stem(foreign_realm, at("2026-08-09T00:00:00Z"))
    ));
    std::fs::copy(&published[0].snapshot, &foreign).expect("a foreign snapshot is planted");
    let foreign_manifest = SnapshotManifest::describe(
        &foreign,
        foreign_realm,
        SCHEMA_VERSION,
        at("2026-08-09T00:00:00Z"),
    )
    .expect("the foreign manifest is described");
    std::fs::write(
        SnapshotManifest::path_for(&foreign),
        foreign_manifest.to_bytes().expect("manifest bytes"),
    )
    .expect("the foreign manifest is written");
    let partial = backups.join("kontor-in-flight.partial");
    std::fs::write(&partial, b"half a copy").expect("a partial is planted");
    let orphan = backups.join(format!(
        "{}.db",
        SnapshotManifest::file_stem(realm, at("2026-08-08T00:00:00Z"))
    ));
    std::fs::write(&orphan, b"no manifest here").expect("an orphan is planted");

    let removed = prune_snapshots(&backups, realm).expect("retention runs");
    assert_eq!(removed.len(), 10 - RETAINED_SNAPSHOTS);

    let retained = list_snapshots(&backups, realm).expect("the listing is readable");
    assert_eq!(retained.len(), RETAINED_SNAPSHOTS);
    let newest: Vec<&PathBuf> = retained.iter().map(|entry| &entry.snapshot).collect();
    assert!(
        newest.contains(&&published[9].snapshot),
        "the newest verified snapshot is never pruned"
    );
    assert!(
        !newest.contains(&&published[0].snapshot),
        "the oldest snapshots are the ones that go"
    );
    assert!(
        foreign.exists(),
        "another realm's snapshot is not ours to delete"
    );
    assert!(
        partial.exists(),
        "a partial is not a snapshot and is not deleted"
    );
    assert!(orphan.exists(), "a file with no manifest is not deleted");
}

#[test]
fn retention_never_empties_a_directory_that_holds_one_backup() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let backups = home.path().join("backups");
    let realm = seeded(&database, 1).realm_id();
    let only = create_snapshot(&database, &backups, at("2026-08-10T10:00:00Z"))
        .expect("the only snapshot");

    for _ in 0..3 {
        assert!(
            prune_snapshots(&backups, realm)
                .expect("retention runs")
                .is_empty()
        );
    }
    only.manifest
        .verify_file(&only.snapshot)
        .expect("the only backup is still the backup");
}

#[test]
fn a_restore_reinstates_the_same_realm_and_refuses_a_different_one() {
    let home = TempDir::new().expect("a temporary directory");
    let source_root = home.path().join("source");
    std::fs::create_dir_all(&source_root).expect("the source root");
    let source = source_root.join("kontor.db");
    let store = seeded(&source, 5);
    let realm = store.realm_id();
    let snapshot = create_snapshot(
        &source,
        &home.path().join("backups"),
        at("2026-08-10T10:00:00Z"),
    )
    .expect("the snapshot is taken");

    // 1. Into an uninitialized destination.
    let fresh = home.path().join("fresh").join("kontor.db");
    let plan = restore_snapshot(&snapshot.snapshot, &fresh, at("2026-08-10T12:00:00Z"))
        .expect("an uninitialized destination is restored into");
    assert_eq!(plan.realm_id, realm);
    assert!(plan.superseded.is_none());
    assert!(
        plan.reconciliation_required,
        "a restored realm knows nothing about what its runtimes did meanwhile"
    );
    assert_eq!(projects_in(&fresh), 5);

    // 2. Back over the same Realm, which is what disaster recovery is.
    store
        .create_project(&NewProject {
            id: ProjectId::generate(),
            name: name("Written after the snapshot"),
            root_path: name("/tmp/kontor-after"),
            created_at: at("2026-08-10T11:00:00Z"),
        })
        .expect("the source moves on");
    drop(store);
    let plan = restore_snapshot(&snapshot.snapshot, &source, at("2026-08-10T12:30:00Z"))
        .expect("the same realm is restored over itself");
    assert_eq!(
        projects_in(&source),
        5,
        "the snapshot's state is what is back"
    );
    let superseded = plan.superseded.expect("the replaced database is kept");
    assert!(
        superseded.exists(),
        "the replaced database is evidence, not rubbish"
    );
    assert_eq!(
        projects_in(&superseded),
        6,
        "the superseded file is whole, WAL folded in"
    );

    // 3. Into a *different* initialized Realm: refused, and refused before the
    //    destination changes.
    let other_root = home.path().join("other");
    std::fs::create_dir_all(&other_root).expect("the other root");
    let other = other_root.join("kontor.db");
    let other_realm = seeded(&other, 2).realm_id();
    assert_ne!(other_realm, realm);
    let before = std::fs::read(&other).expect("the other database is readable");

    let refused = restore_snapshot(&snapshot.snapshot, &other, at("2026-08-10T13:00:00Z"))
        .expect_err("a foreign initialized realm is never overwritten");
    match refused {
        BackupError::DestinationInitialized { found } => assert_eq!(found, other_realm),
        other => panic!("the refusal must name the destination realm: {other:?}"),
    }
    assert_eq!(
        std::fs::read(&other).expect("the other database is still readable"),
        before,
        "the destination was mutated before the cross-realm check"
    );
    assert_eq!(
        SqliteStore::open(&other)
            .expect("the other realm reopens")
            .realm_id(),
        other_realm
    );
    assert_eq!(
        listing(&other_root),
        vec!["kontor.db".to_owned()],
        "a refused restore leaves no partial, no superseded copy and no residue"
    );
}
