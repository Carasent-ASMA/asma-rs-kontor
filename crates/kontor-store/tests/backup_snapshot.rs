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

use kontor_core::authority::{AuthoritySubject, SubjectOrigin};
use kontor_core::id::{
    AggregateRevision, CommandReceiptId, ExternalName, IdempotencyKey, ProjectId, RealmId,
    SpecVersion, TaskId, TaskWorkflowId, Timestamp, parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    NewLocalCommand, NewProject, NewTask, NewTaskWorkflow, ProjectRepository,
};
use kontor_core::spec::ResolvedWorkProfileSnapshot;
use kontor_core::state::TaskState;
use kontor_store::authority::SubjectOrigins;
use kontor_store::backup::{
    BackupError, RETAINED_SNAPSHOTS, SnapshotManifest, create_snapshot, list_snapshots,
    prune_snapshots, restore_snapshot,
};
use kontor_store::memory::{AgentsRoomExport, LegacyMemoryEntry, MemoryProvenance};
use kontor_store::{
    ProfileSelection, ProjectEnsure, SCHEMA_VERSION, SqliteStore, StoredProfileSelectionOutcome,
    TeamTemplateSource,
};
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

fn memory_document(text: &str) -> kontor_core::id::CanonicalDocument {
    kontor_core::id::CanonicalDocument::from_value(
        &serde_json::json!({"schema_version": 1, "text": text}),
    )
    .expect("canonical memory")
}

fn selection_snapshot_fixture(
    store: &SqliteStore,
    project: ProjectId,
) -> [StoredProfileSelectionOutcome; 2] {
    let task = TaskId::generate();
    store
        .create_task(&NewTask {
            id: task,
            project_id: project,
            mini_project_id: None,
            title: name("Snapshot selection task"),
            module: None,
            state: TaskState::Ready,
            created_at: at("2026-08-10T09:00:00Z"),
        })
        .expect("the task is created");
    let pack = kontor_profiles::seeds::bundled_pack().expect("the bundled pack loads");
    let entry = pack
        .manifest
        .iter()
        .find(|entry| entry.availability == kontor_profiles::pack::PackAvailability::Seeded)
        .expect("the pack seeds at least one category");
    let bundle =
        kontor_profiles::pack::resolve_profile(&pack, &entry.category, at("2026-08-10T09:01:00Z"))
            .expect("the profile resolves");
    let mut first_definition = bundle.profile.definition;
    first_definition.team_template = None;

    let apply = |key: &str,
                 marker: &str,
                 definition: &kontor_core::spec::WorkProfileSpec,
                 instant: Timestamp|
     -> StoredProfileSelectionOutcome {
        let snapshot = ResolvedWorkProfileSnapshot::resolve(definition, instant)
            .expect("the profile resolves");
        let command = NewLocalCommand {
            project_id: project,
            receipt_id: CommandReceiptId::generate(),
            idempotency_key: IdempotencyKey::parse(key).expect("a valid key"),
            kind: CommandKind::SelectTaskProfile,
            target: AggregateRef::Task { task_id: task },
            target_revision: AggregateRevision::INITIAL,
            intent: kontor_core::id::CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "marker": marker,
            }))
            .expect("a canonical intent"),
            created_at: instant,
        };
        let workflow = NewTaskWorkflow {
            id: TaskWorkflowId::generate(),
            project_id: project,
            task_id: task,
            current_phase: definition.entry_phase.clone(),
            snapshot,
            created_at: instant,
        };
        store
            .apply_profile_selection(&ProfileSelection {
                command: &command,
                workflow: &workflow,
                definition,
                team: None,
                team_source: TeamTemplateSource::Registered,
            })
            .expect("the selection is stored atomically")
    };
    let first = apply(
        "snapshot-profile-selection-k",
        "snapshot-profile-selection-p1",
        &first_definition,
        at("2026-08-10T09:01:00Z"),
    );
    let mut second_definition = first_definition;
    second_definition.version =
        SpecVersion::parse(second_definition.version.get() + 1).expect("the next version");
    let second = apply(
        "snapshot-profile-selection-k2",
        "snapshot-profile-selection-p2",
        &second_definition,
        at("2026-08-10T09:02:00Z"),
    );
    [first, second]
}

#[test]
fn snapshot_preserves_exact_k_and_k2_profile_selection_bindings() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the database migrates");
    let project = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project,
            name: name("Selection snapshot project"),
            root_path: name("/tmp/kontor-selection-snapshot"),
            created_at: at("2026-08-10T09:00:00Z"),
        })
        .expect("the project is created");
    let expected = selection_snapshot_fixture(&store, project);

    let outcome = create_snapshot(
        &database,
        &home.path().join("backups"),
        at("2026-08-10T10:00:00Z"),
    )
    .expect("the snapshot is published");
    assert_eq!(outcome.manifest.database_schema_version, 63);
    let restored = SqliteStore::open(&outcome.snapshot).expect("the snapshot reopens");
    for expected in expected {
        let actual = restored
            .get_profile_selection_outcome(project, expected.receipt_id)
            .expect("the outcome reads")
            .expect("the exact binding survives");
        assert_eq!(actual, expected);
    }
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
fn two_simultaneous_publishers_of_one_name_produce_one_snapshot() {
    // The published name is a function of the realm and the instant, so two
    // callers asking for a snapshot of the same realm at the same instant race
    // for the same file. Exactly one may win, and the loser must be refused
    // rather than replace what the winner published — a check followed by a
    // rename would let the loser win *after* the check and silently overwrite it.
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let backups = home.path().join("backups");
    let realm = seeded(&database, 24).realm_id();
    std::fs::create_dir_all(&backups).expect("the backup directory");

    let instant = at("2026-08-10T10:00:00Z");
    let start = std::sync::Arc::new(std::sync::Barrier::new(8));
    let publishers: Vec<_> = (0..8)
        .map(|_| {
            let database = database.clone();
            let backups = backups.clone();
            let start = std::sync::Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                create_snapshot(&database, &backups, instant)
            })
        })
        .collect();

    let mut published = Vec::new();
    let mut refused = 0;
    for publisher in publishers {
        match publisher.join().expect("the publisher finishes") {
            Ok(outcome) => published.push(outcome),
            Err(BackupError::AlreadyExists { .. }) => refused += 1,
            Err(other) => panic!("a loser must be refused as a collision: {other:?}"),
        }
    }
    assert_eq!(published.len(), 1, "exactly one publisher may win the name");
    assert_eq!(refused, 7, "every other publisher is refused");

    // The winner's snapshot is intact: a loser that had overwritten it would
    // leave a file whose bytes no longer match the manifest that was published
    // with it.
    let winner = &published[0];
    winner
        .manifest
        .verify_file(&winner.snapshot)
        .expect("the published snapshot is exactly the one that was verified");
    // Read-only, and deliberately not through `SqliteStore::open`: opening a
    // store migrates the file and switches it to WAL, which would *write* to the
    // snapshot this test is asserting nobody wrote to.
    let reader = rusqlite::Connection::open_with_flags(
        &winner.snapshot,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("the published snapshot opens read-only");
    let report: String = reader
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("the snapshot is checkable");
    assert_eq!(report, "ok", "the published snapshot is whole");
    assert_eq!(projects_in(&winner.snapshot), 24);

    // And the directory holds exactly one snapshot with one manifest — no
    // second copy, and no partial left behind by a refused publisher.
    let names = listing(&backups);
    assert_eq!(
        names.iter().filter(|name| name.ends_with(".db")).count(),
        1,
        "a refused publisher must not leave a second snapshot: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.ends_with(".partial")),
        "a refused publisher must not leave its partial behind: {names:?}"
    );
    assert_eq!(
        list_snapshots(&backups, realm).expect("the listing").len(),
        1
    );
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
fn a_restore_refuses_a_taken_superseded_name_and_leaves_the_destination_intact() {
    // The superseded name is derived from the restore's own instant, so two
    // restores in the same second — or an operator who already parked a file
    // under that name — collide on it. The database that name refers to is a
    // *previous* restore's evidence, and overwriting it to make room for this
    // one would destroy exactly the thing the superseded copy exists to keep.
    let home = TempDir::new().expect("a temporary directory");
    let state_root = home.path().join("realm");
    std::fs::create_dir_all(&state_root).expect("the state root");
    let destination = state_root.join("kontor.db");
    let store = seeded(&destination, 6);
    let realm = store.realm_id();
    let snapshot = create_snapshot(
        &destination,
        &home.path().join("backups"),
        at("2026-08-10T10:00:00Z"),
    )
    .expect("the snapshot is taken");

    // The realm moves on, so the destination is provably *not* the snapshot.
    store
        .create_project(&NewProject {
            id: ProjectId::generate(),
            name: name("Written after the snapshot"),
            root_path: name("/tmp/kontor-after"),
            created_at: at("2026-08-10T11:00:00Z"),
        })
        .expect("the source moves on");
    drop(store);

    let instant = at("2026-08-10T12:00:00Z");
    let taken = state_root.join("kontor.db.superseded-20260810T120000Z");
    let earlier = b"a database an earlier restore set aside";
    std::fs::write(&taken, earlier).expect("the superseded name is already taken");

    let refused = restore_snapshot(&snapshot.snapshot, &destination, instant)
        .expect_err("a taken superseded name is never overwritten");
    match refused {
        BackupError::AlreadyExists { path } => assert_eq!(path, taken),
        other => panic!("the refusal must name the collision: {other:?}"),
    }

    assert_eq!(
        std::fs::read(&taken).expect("the earlier file is still there"),
        earlier,
        "an earlier restore's evidence must survive a refused restore"
    );
    // The destination was not displaced: it is still the realm's live database,
    // with the row the snapshot does not have.
    assert_eq!(
        projects_in(&destination),
        7,
        "a refused restore must leave the destination exactly as it was"
    );
    let reopened = SqliteStore::open(&destination).expect("the destination is still a database");
    assert_eq!(reopened.realm_id(), realm);
    reopened
        .integrity_check()
        .expect("and it is whole after the refusal");
    drop(reopened);
    assert!(
        !listing(&state_root)
            .iter()
            .any(|entry| entry.ends_with(".partial")),
        "a refused restore leaves no copy behind: {:?}",
        listing(&state_root)
    );

    // With the collision cleared, the same restore succeeds and sets the
    // occupant aside under a name of its own.
    std::fs::remove_file(&taken).expect("the collision is cleared");
    let plan = restore_snapshot(&snapshot.snapshot, &destination, instant)
        .expect("the restore runs once the name is free");
    assert_eq!(plan.superseded.as_deref(), Some(taken.as_path()));
    assert_eq!(projects_in(&destination), 6, "the snapshot's state is back");
    assert_eq!(
        projects_in(&taken),
        7,
        "and the database it replaced is whole, under the superseded name"
    );
    assert!(
        !listing(&state_root)
            .iter()
            .any(|entry| entry.ends_with(".partial")),
        "a successful restore leaves no copy behind: {:?}",
        listing(&state_root)
    );
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

#[test]
fn memory_ledger_and_import_evidence_restore_while_fts_is_rebuilt() {
    let home = TempDir::new().expect("a temporary directory");
    let source = home.path().join("source.db");
    let store = seeded(&source, 0);
    let project = ProjectId::generate();
    // Declared legacy on the memory side, because this test is about import
    // evidence surviving a snapshot: a project created native has none.
    store
        .ensure_project(&ProjectEnsure {
            id: project,
            name: name("Memory project"),
            root_path: name("/tmp/memory-project"),
            origins: SubjectOrigins {
                memory: SubjectOrigin::LegacyPending,
                backlog: SubjectOrigin::KontorNative,
            },
            created_at: at("2026-08-10T09:00:00Z"),
        })
        .expect("the project is created");
    let mut export = AgentsRoomExport {
        schema_version: 1,
        source: "agentsroom".to_owned(),
        project_id: project,
        entries: vec![LegacyMemoryEntry {
            item_id: "legacy".to_owned(),
            document: memory_document("legacy provenance"),
            source_id: Some("legacy-1".to_owned()),
        }],
        export_hash: kontor_core::id::ContentHash::of(b"pending"),
    };
    export.export_hash = export.calculate_hash().expect("the export hashes");
    store
        .apply_agentsroom_import(&export)
        .expect("the export imports");
    let (attested, _) = store
        .attest_subject_source_frozen(
            project,
            AuthoritySubject::Memory,
            AggregateRevision::INITIAL,
            "agentsroom-cursor-1",
            &kontor_core::id::ContentHash::of(b"frozen source"),
        )
        .expect("the legacy source is attested frozen");
    store
        .switch_project_memory_authority(
            project,
            "agentsroom",
            &export.export_hash,
            attested.revision,
        )
        .expect("authority switches");
    let (proposal, propose_receipt) = store
        .propose_memory_revision(
            project,
            "native",
            0,
            &memory_document("native searchable needle"),
            &MemoryProvenance {
                source: "operator".to_owned(),
                source_id: Some("proposal-1".to_owned()),
                legacy_last_write_wins: false,
                history_unavailable: false,
            },
            "author",
        )
        .expect("a native revision is proposed");
    let approve_receipt = store
        .approve_memory_revision(project, "native", &proposal.revision_id, 1, "reviewer")
        .expect("the revision is approved");
    rusqlite::Connection::open(&source)
        .expect("a maintenance connection opens")
        .execute("DELETE FROM memory_fts", [])
        .expect("the derived index is absent from the backup");

    let snapshot = create_snapshot(
        &source,
        &home.path().join("backups"),
        at("2026-08-10T10:00:00Z"),
    )
    .expect("the realm is snapshotted");
    drop(store);
    let restored_path = home.path().join("restored.db");
    restore_snapshot(
        &snapshot.snapshot,
        &restored_path,
        at("2026-08-10T11:00:00Z"),
    )
    .expect("the snapshot restores");
    let restored = SqliteStore::open(&restored_path).expect("the restored realm opens");

    let listed = restored.list_memory(project).expect("current memory lists");
    assert_eq!(listed.len(), 2, "both imported and native pointers survive");
    assert_eq!(
        restored
            .search_memory(project, "needle", 10)
            .expect("FTS searches")
            .len(),
        1,
        "restore rebuilds the deliberately absent derived FTS projection"
    );
    let legacy = restored
        .memory_history(project, "legacy")
        .expect("legacy history");
    assert!(legacy[0].provenance.history_unavailable);
    assert!(legacy[0].provenance.legacy_last_write_wins);
    assert!(
        restored
            .preview_agentsroom_import(&export)
            .expect("manifest reads")
            .already_imported,
        "the import manifest survives"
    );
    let connection = rusqlite::Connection::open(&restored_path).expect("evidence connection");
    for receipt in [propose_receipt.receipt_id, approve_receipt.receipt_id] {
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM memory_receipts WHERE id=?1",
                [receipt],
                |row| row.get(0),
            )
            .expect("receipt count");
        assert_eq!(count, 1, "the native receipt survives");
    }
}
