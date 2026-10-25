//! TEST-001: disposable checkout discovery with no UI, no watch and no governed
//! project.
//!
//! Add, change, rename and remove a tracked plan; add a non-ignored untracked
//! draft and a `.github/plans/qnr` execution unit. The collector is polled by
//! hand, exactly as a daemon would poll it, and no plugin, UI or watch exists.

mod support;

use std::path::PathBuf;
use std::time::Instant;

use kontor_catalog::{ContentKind, EntryFilter, EntryKind, EntryState};
use support::{Fixture, Repo, plan, registry, sweep};

#[tokio::test]
async fn local_discovery_tracks_added_changed_renamed_and_removed_plans() {
    let repo = Repo::new();
    repo.write(
        "_docs/plans/alpha.md",
        &plan("Alpha plan", Some("ASMA-9001"), Some("kontor"), ""),
    );
    repo.write(
        "config/orchestration/workstreams.json",
        &registry(&["kontor"]),
    );
    repo.commit("initial");
    let registry_path = repo.root().join("config/orchestration/workstreams.json");
    let fixture = Fixture::open(
        &[(repo.root().to_path_buf(), true)],
        Some(registry_path),
        30,
    );
    assert!(
        !fixture.state.path().join("kontor.db").exists(),
        "the catalog must not require or create a governed database"
    );

    let started = Instant::now();
    let report = sweep(&fixture.catalog).await;
    let elapsed = started.elapsed();
    println!(
        "TEST-001 sweep latency: {} ms for {} files and {} entries",
        elapsed.as_millis(),
        report.files_seen,
        report.entries
    );
    assert!(
        elapsed.as_secs() < 30,
        "a tiny checkout must fit the 30 s scan budget; took {elapsed:?}"
    );
    assert!(
        report.finished,
        "one readable repository is a finished sweep"
    );

    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(entries.entries.len(), 1);
    let alpha = &entries.entries[0];
    assert_eq!(alpha.kind, EntryKind::Plan);
    assert_eq!(alpha.occurrences.len(), 1);
    assert_eq!(alpha.occurrences[0].content_kind, ContentKind::Tracked);
    assert_eq!(alpha.occurrences[0].relative_path, "_docs/plans/alpha.md");
    assert_eq!(
        alpha.occurrences[0].content.epic.as_deref(),
        Some("ASMA-9001")
    );
    let first_id = alpha.logical_plan_id.clone();
    let first_revision = alpha.occurrences[0].content.revision_id.clone();

    // Change: a new content revision under the same logical identity.
    repo.write(
        "_docs/plans/alpha.md",
        &plan("Alpha plan revised", Some("ASMA-9001"), Some("kontor"), ""),
    );
    repo.commit("revise");
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert_eq!(entries.entries.len(), 1);
    let alpha = &entries.entries[0];
    assert_eq!(alpha.logical_plan_id, first_id);
    assert_ne!(alpha.occurrences[0].content.revision_id, first_revision);

    // A non-ignored untracked draft is visible and visibly a draft.
    repo.write(
        "plans/draft.md",
        &plan("Draft plan", Some("ASMA-9002"), Some("kontor"), ""),
    );
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    let draft = entries
        .entries
        .iter()
        .find(|entry| entry.title == "Draft plan")
        .expect("the untracked draft is visible");
    assert_eq!(
        draft.occurrences[0].content_kind,
        ContentKind::UntrackedDraft
    );
    assert_eq!(
        draft.occurrences[0].revision_kind,
        kontor_catalog::RevisionKind::SavedDraft
    );

    // Ignoring the draft makes it absent; a complete sweep may now remove it.
    repo.write(".gitignore", "/plans/draft.md\n");
    repo.commit("ignore the draft");
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert!(
        entries
            .entries
            .iter()
            .all(|entry| entry.title != "Draft plan"),
        "an ignored draft is not catalogued"
    );
    let history = fixture
        .catalog
        .entries(&EntryFilter {
            include_removed: true,
            ..EntryFilter::default()
        })
        .unwrap();
    assert!(
        history
            .entries
            .iter()
            .any(|entry| entry.title == "Draft plan" && entry.state == EntryState::Removed),
        "the ignored draft remains as removed history"
    );

    // A QNR execution unit is included only on a root that permits it, and it is
    // never a plan entry.
    repo.write(
        ".github/plans/qnr/unit-1.md",
        &plan("QNR unit", Some("ASMA-9003"), Some("kontor"), ""),
    );
    repo.commit("qnr unit");
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    let unit = entries
        .entries
        .iter()
        .find(|entry| entry.title == "QNR unit")
        .expect("the permitted execution unit is visible");
    assert_eq!(unit.kind, EntryKind::ExecutionUnit);
    let filtered = fixture
        .catalog
        .entries(&EntryFilter {
            kind: Some(EntryKind::ExecutionUnit),
            ..EntryFilter::default()
        })
        .unwrap();
    assert_eq!(filtered.entries.len(), 1);
    assert_eq!(filtered.entries[0].kind, EntryKind::ExecutionUnit);

    // Rename: one absent predecessor with the same revision and epic retains the
    // logical identity; the old path survives as the alias.
    std::fs::rename(
        repo.root().join("_docs/plans/alpha.md"),
        repo.root().join("_docs/plans/alpha-renamed.md"),
    )
    .expect("the plan is renamed");
    repo.commit("rename");
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    let renamed = entries
        .entries
        .iter()
        .find(|entry| entry.logical_plan_id == first_id)
        .expect("the logical identity survives the rename");
    assert_eq!(
        renamed.occurrences[0].relative_path,
        "_docs/plans/alpha-renamed.md"
    );
    assert!(
        renamed
            .aliases
            .iter()
            .any(|alias| alias.value == "_docs/plans/alpha.md"),
        "the prior path is retained as a rename alias"
    );

    // Scope filtering reaches the workstream association without inventing one.
    let by_workstream = fixture
        .catalog
        .entries(&EntryFilter {
            scope: Some("kontor".to_owned()),
            ..EntryFilter::default()
        })
        .unwrap();
    assert!(
        by_workstream
            .entries
            .iter()
            .any(|entry| entry.title == "Alpha plan revised")
    );

    // Remove: gone from the default answer, retained as history.
    repo.remove("_docs/plans/alpha-renamed.md");
    repo.commit("remove");
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert!(
        entries
            .entries
            .iter()
            .all(|entry| entry.logical_plan_id != first_id)
    );
    let history = fixture
        .catalog
        .entries(&EntryFilter {
            include_removed: true,
            ..EntryFilter::default()
        })
        .unwrap();
    let removed = history
        .entries
        .iter()
        .find(|entry| entry.logical_plan_id == first_id)
        .expect("the removed plan is retained");
    assert_eq!(removed.state, EntryState::Removed);
    assert_eq!(removed.occurrences[0].state, EntryState::Removed);

    let status = fixture.catalog.status().unwrap();
    assert!(status.counts.tombstones >= 1);
    assert!(status.snapshot.present);
    assert!(status.snapshot.current);
}

#[tokio::test]
#[ignore = "measures one configured host checkout; set CATALOG_MEASURE_ROOT"]
async fn measure_one_configured_checkout() {
    let root = std::env::var("CATALOG_MEASURE_ROOT")
        .expect("set CATALOG_MEASURE_ROOT to an absolute git checkout");
    let fixture = Fixture::open(&[(PathBuf::from(root), true)], None, 30);
    let started = Instant::now();
    let report = sweep(&fixture.catalog).await;
    println!(
        "MEASURE: {} ms, {} files, {} entries, {} repositories scanned",
        started.elapsed().as_millis(),
        report.files_seen,
        report.entries,
        fixture
            .catalog
            .status()
            .unwrap()
            .collection
            .repositories_scanned
    );
    assert!(report.finished);
    assert!(started.elapsed().as_secs() < 30);
}

#[tokio::test]
async fn zero_repositories_is_a_valid_complete_catalog() {
    let fixture = Fixture::open(&[], None, 30);
    let report = sweep(&fixture.catalog).await;
    assert!(report.finished);
    assert_eq!(report.entries, 0);
    let status = fixture.catalog.status().unwrap();
    assert_eq!(
        status.collection.state,
        kontor_catalog::CollectionState::Complete
    );
    assert_eq!(status.counts.logical_plans, 0);
    assert_eq!(status.attempted_generation, 1);
    assert!(
        fixture
            .catalog
            .entries(&EntryFilter::default())
            .unwrap()
            .entries
            .is_empty()
    );
    assert!(!fixture.state.path().join("kontor.db").exists());
}

#[tokio::test]
async fn a_repository_that_does_not_permit_github_plans_never_reads_them() {
    let repo = Repo::new();
    repo.write(
        ".github/plans/qnr/unit-1.md",
        &plan("Private QNR", Some("ASMA-9004"), Some("kontor"), ""),
    );
    repo.commit("qnr unit");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    sweep(&fixture.catalog).await;
    let entries = fixture.catalog.entries(&EntryFilter::default()).unwrap();
    assert!(entries.entries.is_empty());
}
