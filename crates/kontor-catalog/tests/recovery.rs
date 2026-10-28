//! TEST-005: crash boundaries, fencing, concurrent ownership, restart catch-up
//! and snapshot compatibility.
//!
//! The page/promote protocol is exercised through the same store the daemon
//! uses. A crash is modeled by dropping the connection without the final
//! transaction, which is exactly the state a killed process leaves.

mod support;

use kontor_catalog::contract::{RegistryState, RegistryStatus};
use kontor_catalog::store::{Database, ObservedEntry, ScannedRoot, SweepOutcome};
use kontor_catalog::{Catalog, CatalogError, CatalogSettings, EntryFilter};
use kontor_core::id::Timestamp;
use support::{Fixture, Repo, plan, sweep};
use tempfile::TempDir;

fn settings_of(state: &std::path::Path) -> CatalogSettings {
    serde_json::from_slice(
        &std::fs::read(Catalog::settings_path(state)).expect("the fixture settings"),
    )
    .expect("valid settings")
}

fn absent_registry() -> RegistryStatus {
    RegistryStatus {
        path: None,
        revision: None,
        state: RegistryState::Absent,
        workstreams: 0,
        diagnostics: Vec::new(),
    }
}

fn outcome(
    complete: bool,
    coverage: Vec<kontor_catalog::contract::RepositoryCoverage>,
    scanned: Vec<ScannedRoot>,
) -> SweepOutcome {
    let entries_seen = coverage
        .iter()
        .map(|coverage| coverage.entries_observed)
        .sum();
    let files_seen = coverage.iter().map(|coverage| coverage.files_seen).sum();
    SweepOutcome {
        complete,
        coverage,
        diagnostics: Vec::new(),
        registry: absent_registry(),
        files_seen,
        entries_seen,
        error: None,
        scanned_roots: scanned,
    }
}

#[tokio::test]
async fn a_competing_collector_is_refused_before_any_scan() {
    let repo = Repo::new();
    repo.write(
        "plans/owned.md",
        &plan("Owned plan", Some("ASMA-9700"), Some("kontor"), ""),
    );
    repo.commit("owned");
    let state = TempDir::new().expect("a state root");
    let settings = CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root: repo.root().to_path_buf(),
            include_github_plans: false,
        }],
        ..CatalogSettings::default()
    };
    std::fs::write(
        Catalog::settings_path(state.path()),
        serde_json::to_vec_pretty(&settings).expect("settings"),
    )
    .expect("settings written");
    let first = Catalog::open_with_owner(state.path(), "owner-a").expect("first catalog");
    let second = Catalog::open_with_owner(state.path(), "owner-b").expect("second catalog");
    first
        .collect_once()
        .await
        .expect("the first owner collects");
    match second.collect_once().await {
        Err(CatalogError::OwnerConflict { owner }) => assert_eq!(owner, "owner-a"),
        other => panic!("a competing owner must be refused, got {other:?}"),
    }
    let status = first.status().unwrap();
    assert!(status.collector.owner_here);
}

#[tokio::test]
async fn a_crash_before_or_after_a_page_never_advances_the_active_generation() {
    let repo = Repo::new();
    repo.write(
        "plans/crash.md",
        &plan("Crash plan", Some("ASMA-9800"), Some("kontor"), ""),
    );
    repo.commit("crash plan");
    let state = TempDir::new().expect("a state root");
    let settings = CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root: repo.root().to_path_buf(),
            include_github_plans: false,
        }],
        ..CatalogSettings::default()
    };
    std::fs::write(
        Catalog::settings_path(state.path()),
        serde_json::to_vec_pretty(&settings).expect("settings"),
    )
    .expect("settings written");
    let now = Timestamp::now();

    // Crash before any page commit: the sweep row exists, nothing else does.
    {
        let database = Database::open(&Catalog::database_path(state.path())).expect("database");
        database
            .acquire_owner("crash-owner", 120, &now)
            .expect("the lease");
        let _sweep = database
            .begin_sweep("crash-owner", &now)
            .expect("the sweep begins");
        // Dropped here: the process died before committing a page.
    }
    let catalog = Catalog::open_with_owner(state.path(), "recovery").expect("recovery catalog");
    assert_eq!(catalog.status().unwrap().generation, 0);

    // Crash after a committed page but before the final transaction.
    let discovered = kontor_catalog::discovery::discover(&settings_of(state.path()), now).await;
    assert_eq!(discovered.entries.len(), 1);
    {
        let database = Database::open(&Catalog::database_path(state.path())).expect("database");
        database
            .release_owner("crash-owner", &now)
            .expect("release the crashed lease");
        database
            .acquire_owner("crash-owner-2", 120, &now)
            .expect("the second lease");
        let sweep = database
            .begin_sweep("crash-owner-2", &now)
            .expect("the sweep begins");
        database
            .commit_page(&sweep, 0, &now, &discovered.entries, discovered.files_seen)
            .expect("the page commits");
        assert_eq!(
            database.read_status().unwrap().generation,
            0,
            "a page is evidence, not a complete generation"
        );
        // Dropped without finalize: the page remains staged.
    }
    assert_eq!(catalog.status().unwrap().generation, 0);

    // Restart catch-up: release the dead lease, then one complete sweep
    // observes the same file and promotes it.
    {
        let database = Database::open(&Catalog::database_path(state.path())).expect("database");
        database
            .release_owner("crash-owner-2", &now)
            .expect("release the dead lease");
    }
    let report = catalog.collect_once().await.expect("restart catch-up");
    assert!(report.finished);
    assert_eq!(catalog.status().unwrap().generation, report.generation);
    let entries = catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(entries.entries.len(), 1);
}

#[tokio::test]
async fn a_stale_page_write_is_fenced_after_the_sweep_is_finished() {
    let state = TempDir::new().expect("a state root");
    let now = Timestamp::now();
    let database = Database::open(&Catalog::database_path(state.path())).expect("database");
    database
        .acquire_owner("fenced-owner", 120, &now)
        .expect("the lease");
    let sweep = database
        .begin_sweep("fenced-owner", &now)
        .expect("the sweep begins");
    database
        .finalize_sweep(&sweep, &outcome(true, Vec::new(), Vec::new()), &now)
        .expect("the empty sweep finishes");
    let entries: Vec<ObservedEntry> = Vec::new();
    assert!(matches!(
        database.commit_page(&sweep, 0, &now, &entries, 0),
        Err(kontor_catalog::StoreError::Fenced)
    ));
}

#[tokio::test]
async fn an_incomplete_sweep_cannot_tombstone_an_absence_it_did_not_prove() {
    // MUT-001 boundary: this test fails if the `if complete` protection around
    // tombstoning is removed.
    let repo = Repo::new();
    repo.write(
        "plans/alpha.md",
        &plan("Alpha", Some("ASMA-9900"), Some("kontor"), ""),
    );
    repo.write(
        "plans/beta.md",
        &plan("Beta", Some("ASMA-9901"), Some("kontor"), ""),
    );
    repo.commit("two plans");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    assert_eq!(
        fixture
            .catalog
            .entries(&EntryFilter::default())
            .unwrap()
            .entries
            .len(),
        2
    );

    // Remove Beta from the index, then run a *partial* sweep through the store
    // directly. The scan saw only Alpha; it has not earned the right to say Beta
    // is gone.
    std::fs::remove_file(repo.root().join("plans/beta.md")).expect("beta is removed");
    let now = Timestamp::now();
    let settings = settings_of(fixture.state.path());
    fixture
        .catalog
        .release_owner()
        .expect("release the fixture lease");
    let database = Database::open(&Catalog::database_path(fixture.state.path())).expect("database");
    database
        .acquire_owner("partial-owner", 120, &now)
        .expect("the partial lease");
    let partial_sweep = database
        .begin_sweep("partial-owner", &now)
        .expect("the partial sweep begins");
    let discovered = kontor_catalog::discovery::discover(&settings, now).await;
    assert_eq!(discovered.entries.len(), 1, "only Alpha was observed");
    database
        .commit_page(
            &partial_sweep,
            0,
            &now,
            &discovered.entries,
            discovered.files_seen,
        )
        .expect("the partial page commits");
    database
        .finalize_sweep(
            &partial_sweep,
            &outcome(false, discovered.coverage, discovered.scanned_roots),
            &now,
        )
        .expect("the partial sweep records its checkpoint");

    let still_visible = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(
        still_visible.entries.len(),
        2,
        "an incomplete sweep may not remove a fact"
    );
    assert!(
        still_visible
            .entries
            .iter()
            .any(|entry| entry.title == "Beta"),
        "Beta remains until a complete sweep proves it absent"
    );

    // A subsequent complete sweep can prove it.
    database
        .release_owner("partial-owner", &now)
        .expect("release the partial lease");
    sweep(&fixture.catalog).await;
    let after = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(after.entries.len(), 1);
    assert!(after.entries.iter().all(|entry| entry.title == "Alpha"));
    let history = fixture
        .catalog
        .entries(&EntryFilter {
            include_removed: true,
            ..EntryFilter::default()
        })
        .unwrap();
    assert!(
        history.entries.iter().any(
            |entry| entry.title == "Beta" && entry.state == kontor_catalog::EntryState::Removed
        )
    );
}

#[tokio::test]
async fn a_plan_that_grew_past_the_read_bound_is_not_removed() {
    let repo = Repo::new();
    let content = plan("Grows", Some("ASMA-9704"), Some("kontor"), "");
    repo.write("plans/grows.md", &content);
    repo.commit("grows");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;

    let mut huge = content;
    huge.push_str(&"x".repeat(3 * 1024 * 1024));
    repo.write("plans/grows.md", &huge);
    repo.commit("huge");
    sweep(&fixture.catalog).await;

    let status = fixture.catalog.status().unwrap();
    assert_eq!(
        status.collection.coverage[0].state,
        kontor_catalog::contract::RepositoryState::Partial
    );
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert!(
        entries.entries.iter().any(
            |entry| entry.title == "Grows" && entry.state == kontor_catalog::EntryState::Active
        ),
        "a plan that could not be read is not proven absent"
    );
}

#[tokio::test]
async fn a_partial_root_cannot_prove_a_sibling_plan_absent() {
    let repo = Repo::new();
    repo.write(
        "plans/gone.md",
        &plan("Gone", Some("ASMA-9705"), Some("kontor"), ""),
    );
    let mut huge = plan("Huge", Some("ASMA-9706"), Some("kontor"), "");
    huge.push_str(&"x".repeat(3 * 1024 * 1024));
    repo.write("plans/huge.md", &huge);
    repo.commit("two");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;

    repo.remove("plans/gone.md");
    repo.commit("remove gone");
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert!(
        entries.entries.iter().any(|entry| entry.title == "Gone"),
        "a partial root withholds its absence evidence"
    );
}

#[tokio::test]
async fn a_tombstone_survives_later_generations() {
    let repo = Repo::new();
    repo.write(
        "plans/keep.md",
        &plan("Keep", Some("ASMA-9707"), Some("kontor"), ""),
    );
    repo.write(
        "plans/drop.md",
        &plan("Drop", Some("ASMA-9708"), Some("kontor"), ""),
    );
    repo.commit("two");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;

    repo.remove("plans/drop.md");
    repo.commit("drop");
    for _ in 0..4 {
        sweep(&fixture.catalog).await;
    }
    let history = fixture
        .catalog
        .entries(&EntryFilter {
            include_removed: true,
            ..EntryFilter::default()
        })
        .unwrap();
    let dropped = history
        .entries
        .iter()
        .find(|entry| entry.title == "Drop")
        .expect("the tombstone remains readable four generations later");
    assert_eq!(dropped.state, kontor_catalog::EntryState::Removed);
    assert_eq!(dropped.occurrences.len(), 1);
    assert_eq!(
        dropped.occurrences[0].state,
        kontor_catalog::EntryState::Removed
    );
    assert!(fixture.catalog.status().unwrap().counts.tombstones >= 1);
}

#[tokio::test]
async fn observer_reads_do_not_write_configuration_provenance() {
    let repo = Repo::new();
    repo.write(
        "plans/a.md",
        &plan("A", Some("ASMA-9709"), Some("kontor"), ""),
    );
    repo.commit("a");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    let read = || -> String {
        let connection =
            rusqlite::Connection::open(Catalog::database_path(fixture.state.path())).unwrap();
        connection
            .query_row("SELECT imported_at FROM catalog_config", [], |row| {
                row.get(0)
            })
            .unwrap()
    };
    let before = read();
    std::thread::sleep(std::time::Duration::from_millis(20));
    fixture.catalog.status().unwrap();
    fixture.catalog.sources().unwrap();
    assert_eq!(read(), before, "a GET is not a configuration event");
}

#[tokio::test]
async fn a_hidden_occurrence_does_not_pin_a_removed_plan_active() {
    let repo = Repo::new();
    repo.write(
        "plans/a.md",
        &plan(
            "Pinned",
            Some("ASMA-9712"),
            Some("kontor"),
            "catalog_id: pinned-id\n",
        ),
    );
    repo.commit("a");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;

    repo.write(
        "plans/b.md",
        &plan(
            "Pinned",
            Some("ASMA-9712"),
            Some("kontor"),
            "catalog_id: pinned-id\n",
        ),
    );
    repo.commit("b");
    let settings = settings_of(fixture.state.path());
    let now = Timestamp::now();
    fixture.catalog.release_owner().unwrap();
    let database = Database::open(&Catalog::database_path(fixture.state.path())).unwrap();
    database.acquire_owner("partial-owner", 120, &now).unwrap();
    let partial = database.begin_sweep("partial-owner", &now).unwrap();
    let discovered = kontor_catalog::discovery::discover(&settings, now).await;
    database
        .commit_page(
            &partial,
            0,
            &now,
            &discovered.entries,
            discovered.files_seen,
        )
        .unwrap();
    database
        .finalize_sweep(
            &partial,
            &SweepOutcome {
                complete: false,
                coverage: discovered.coverage,
                diagnostics: Vec::new(),
                registry: discovered.registry,
                files_seen: discovered.files_seen,
                entries_seen: discovered.entries.len(),
                error: None,
                scanned_roots: discovered.scanned_roots,
            },
            &now,
        )
        .unwrap();
    database.release_owner("partial-owner", &now).unwrap();

    repo.remove("plans/a.md");
    repo.remove("plans/b.md");
    repo.commit("remove both");
    let report = sweep(&fixture.catalog).await;
    assert!(report.finished);
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert!(
        entries
            .entries
            .iter()
            .all(|entry| entry.logical_plan_id != "pinned-id"),
        "a plan with no visible occurrence in a fully read root is removed"
    );
}

#[tokio::test]
async fn the_snapshot_is_generation_consistent_and_version_checked() {
    let repo = Repo::new();
    repo.write(
        "plans/snapshot.md",
        &plan("Snapshot plan", Some("ASMA-9950"), Some("kontor"), ""),
    );
    repo.commit("snapshot");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    let status = fixture.catalog.status().unwrap();
    let snapshot = fixture
        .catalog
        .read_snapshot()
        .expect("the snapshot reads")
        .expect("the snapshot exists");
    assert_eq!(snapshot.generation, status.generation);
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(
        snapshot.schema_version,
        kontor_catalog::CATALOG_CONTRACT_VERSION
    );
    assert_eq!(snapshot.status.as_of, status.as_of);

    let path = Catalog::snapshot_path(fixture.state.path());
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("snapshot bytes")).expect("JSON");
    document["schema_version"] = serde_json::json!(99);
    std::fs::write(&path, serde_json::to_vec(&document).expect("JSON")).expect("rewrite");
    assert!(
        fixture.catalog.read_snapshot().is_err(),
        "a newer snapshot contract is refused rather than misread"
    );
    assert!(!fixture.catalog.status().unwrap().snapshot.current);
}

#[tokio::test]
async fn a_root_removed_from_configuration_keeps_its_last_facts_visible() {
    let repo = Repo::new();
    repo.write(
        "plans/retained.md",
        &plan("Retained plan", Some("ASMA-9800"), Some("kontor"), ""),
    );
    repo.commit("retained");
    let settings = CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root: repo.root().to_path_buf(),
            include_github_plans: false,
        }],
        ..CatalogSettings::default()
    };
    let fixture = Fixture::open_settings(settings);
    sweep(&fixture.catalog).await;
    let before = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(before.entries.len(), 1);

    fixture.rewrite_settings(&CatalogSettings {
        schema_version: 1,
        repositories: Vec::new(),
        ..CatalogSettings::default()
    });
    sweep(&fixture.catalog).await;
    let after = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(
        after.entries.len(),
        1,
        "a root removed from configuration is unobserved, not removed"
    );
    assert_eq!(after.entries[0].title, "Retained plan");
}

#[tokio::test]
async fn an_unreadable_root_keeps_plan_associations_diagnostics_and_conflicts() {
    let repo = Repo::new();
    let source_path = repo.write(
        "plans/retained.md",
        &plan(
            "Retained plan",
            Some("not-a-valid-epic"),
            Some("kontor"),
            "",
        ),
    );
    repo.commit("retained");
    let settings = CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root: repo.root().to_path_buf(),
            include_github_plans: false,
        }],
        sources: vec![kontor_catalog::config::OperatorSource {
            id: "retained-source".to_owned(),
            title: None,
            kind: kontor_catalog::config::SourceKind::Plan,
            path: source_path,
            workstream: Some("operator".to_owned()),
            epic: None,
            project_id: None,
            revision_kind: Default::default(),
            expected_sha256: None,
            logical_plan_id: None,
        }],
        ..CatalogSettings::default()
    };
    let fixture = Fixture::open_settings(settings);
    sweep(&fixture.catalog).await;
    let before = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(before.entries.len(), 1);
    let before_entry = &before.entries[0];
    let associations = before_entry.associations.len();
    let diagnostics = before_entry.diagnostics.len();
    let conflicts = before_entry.conflicts.len();
    assert!(
        associations >= 2,
        "the declaration and operator associations"
    );
    assert!(diagnostics > 0, "the registry diagnostic is retained");
    assert_eq!(conflicts, 1, "the workstream disagreement is retained");
    let before_sources = fixture.catalog.sources().unwrap();
    let pending = before_sources.sources[0].pending_verification;
    assert_eq!(pending, associations);

    std::fs::rename(repo.root(), fixture.state.path().join("moved-repository"))
        .expect("the configured root becomes unreadable");
    for _ in 0..2 {
        sweep(&fixture.catalog).await;
    }

    let after = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(after.entries.len(), 1);
    let after_entry = &after.entries[0];
    assert_eq!(after_entry.associations.len(), associations);
    assert_eq!(after_entry.diagnostics.len(), diagnostics);
    assert_eq!(after_entry.conflicts.len(), conflicts);
    let scoped = fixture
        .catalog
        .entries(&EntryFilter {
            scope: Some("kontor".to_owned()),
            ..EntryFilter::default()
        })
        .unwrap();
    assert_eq!(
        scoped.entries.len(),
        1,
        "the carried association still scopes"
    );
    assert_eq!(
        fixture.catalog.sources().unwrap().sources[0].pending_verification,
        pending
    );
}

#[tokio::test]
async fn an_invalid_catalog_json_cannot_advance_generation_or_replace_the_snapshot() {
    let repo = Repo::new();
    repo.write(
        "plans/stable.md",
        &plan("Stable plan", Some("ASMA-9801"), Some("kontor"), ""),
    );
    repo.commit("stable");
    let settings = CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root: repo.root().to_path_buf(),
            include_github_plans: false,
        }],
        ..CatalogSettings::default()
    };
    let fixture = Fixture::open_settings(settings);
    sweep(&fixture.catalog).await;
    let before = fixture.catalog.status().unwrap();
    assert!(before.snapshot.present);

    std::fs::write(Catalog::settings_path(fixture.state.path()), b"{not json")
        .expect("the broken settings document");
    let report = fixture
        .catalog
        .collect_once()
        .await
        .expect("a failed sweep");
    assert!(!report.finished);
    let after = fixture.catalog.status().unwrap();
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.attempted_generation, before.attempted_generation + 1);
    assert_eq!(after.snapshot.generation, before.snapshot.generation);
    assert_eq!(
        fixture
            .catalog
            .entries(&EntryFilter::default())
            .unwrap()
            .entries
            .len(),
        1
    );
}

#[tokio::test]
async fn plan_sweep_pages_and_old_checkpoints_are_pruned() {
    let repo = Repo::new();
    repo.write(
        "plans/bounded.md",
        &plan("Bounded plan", Some("ASMA-9802"), Some("kontor"), ""),
    );
    repo.commit("bounded");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    for _ in 0..20 {
        sweep(&fixture.catalog).await;
    }
    let connection = rusqlite::Connection::open(Catalog::database_path(fixture.state.path()))
        .expect("the catalog database");
    let pages: i64 = connection
        .query_row("SELECT COUNT(*) FROM catalog_sweep_pages", [], |row| {
            row.get(0)
        })
        .expect("the page count");
    let sweeps: i64 = connection
        .query_row("SELECT COUNT(*) FROM catalog_sweeps", [], |row| row.get(0))
        .expect("the sweep count");
    assert_eq!(pages, 1, "only the current generation's plan page remains");
    assert!(sweeps <= 17, "old checkpoints are bounded history");
}
