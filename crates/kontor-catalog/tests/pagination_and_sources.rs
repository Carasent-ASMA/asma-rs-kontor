//! TASK-004: generation-consistent entry cursors and the per-source observer
//! status.
//!
//! The cursor is proved against a real catalog and real sweeps: a page never
//! mixes generations, a cursor from another filter is refused, and a source
//! status reports the generation's completion instant rather than the instant
//! the status was read.

mod support;

use std::os::unix::fs::PermissionsExt;

use kontor_catalog::{
    CatalogError, CatalogSource, CatalogSourceState, EntryFilter, EntryKind, EntryState, ReadSource,
};
use support::{Fixture, Repo, plan, sweep};

fn repository_with_plans(count: usize) -> (Repo, Fixture) {
    let repo = Repo::new();
    for index in 0..count {
        repo.write(
            &format!("_docs/plans/plan-{index:02}.md"),
            &plan(
                &format!("Plan {index:02}"),
                Some(&format!("ASMA-90{index:02}")),
                Some("kontor"),
                "",
            ),
        );
    }
    repo.commit("plans");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    (repo, fixture)
}

#[tokio::test]
async fn entry_pages_are_one_generation_and_end_exactly_once() {
    let (_repo, fixture) = repository_with_plans(5);
    let report = sweep(&fixture.catalog).await;
    assert!(report.finished);

    let mut seen = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let view = fixture
            .catalog
            .entries(&EntryFilter {
                limit: 2,
                cursor,
                ..EntryFilter::default()
            })
            .expect("a page");
        assert_eq!(view.source, ReadSource::Database);
        assert_eq!(view.generation, report.generation);
        assert!(view.as_of.is_some(), "the generation's instant is reported");
        assert!(view.entries.len() <= 2);
        seen.extend(
            view.entries
                .iter()
                .map(|entry| entry.logical_plan_id.clone()),
        );
        pages += 1;
        match view.next_cursor {
            Some(next) => {
                assert!(view.truncated);
                cursor = Some(next);
            }
            None => {
                assert!(!view.truncated);
                break;
            }
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(seen.len(), 5);
    let unique: std::collections::BTreeSet<_> = seen.iter().collect();
    assert_eq!(unique.len(), 5, "no entry appears on two pages");
}

#[tokio::test]
async fn a_cursor_from_a_replaced_generation_is_refused() {
    let (repo, fixture) = repository_with_plans(3);
    sweep(&fixture.catalog).await;
    let first = fixture
        .catalog
        .entries(&EntryFilter {
            limit: 1,
            ..EntryFilter::default()
        })
        .expect("the first page");
    let cursor = first.next_cursor.expect("a further page");

    repo.write(
        "_docs/plans/plan-00.md",
        &plan("Plan 00 revised", Some("ASMA-9000"), Some("kontor"), ""),
    );
    repo.commit("revise");
    let later = sweep(&fixture.catalog).await;
    assert_ne!(later.generation, first.generation);

    let stale = fixture.catalog.entries(&EntryFilter {
        limit: 1,
        cursor: Some(cursor),
        ..EntryFilter::default()
    });
    assert!(matches!(
        stale,
        Err(CatalogError::StaleCursor {
            generation,
            active,
        }) if generation == first.generation && active == later.generation
    ));
}

#[tokio::test]
async fn a_cursor_from_another_filter_is_refused() {
    let (_repo, fixture) = repository_with_plans(2);
    sweep(&fixture.catalog).await;
    let first = fixture
        .catalog
        .entries(&EntryFilter {
            limit: 1,
            ..EntryFilter::default()
        })
        .expect("the first page");
    let cursor = first.next_cursor.expect("a further page");
    let wrong = fixture.catalog.entries(&EntryFilter {
        kind: Some(EntryKind::ExecutionUnit),
        limit: 1,
        cursor: Some(cursor),
        ..EntryFilter::default()
    });
    assert!(matches!(wrong, Err(CatalogError::InvalidCursor { .. })));
}

#[tokio::test]
async fn one_unreadable_operator_source_does_not_freeze_the_catalog() {
    let repo = Repo::new();
    repo.write(
        "_docs/plans/plan.md",
        &plan("Plan", Some("ASMA-9010"), Some("kontor"), ""),
    );
    repo.commit("plan");
    let bad_source = tempfile::TempDir::new().expect("a directory source");
    let fixture = Fixture::open_settings(kontor_catalog::CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root: repo.root().to_path_buf(),
            include_github_plans: false,
        }],
        sources: vec![kontor_catalog::config::OperatorSource {
            id: "bad-source".to_owned(),
            title: None,
            kind: kontor_catalog::config::SourceKind::Plan,
            path: bad_source.path().to_path_buf(),
            workstream: None,
            epic: None,
            project_id: None,
            revision_kind: Default::default(),
            expected_sha256: None,
            logical_plan_id: None,
        }],
        ..kontor_catalog::CatalogSettings::default()
    });

    let first = sweep(&fixture.catalog).await;
    assert!(
        first.finished,
        "a bad source is a diagnostic, not a failed sweep"
    );
    assert_eq!(first.generation, 1);
    assert_eq!(
        fixture
            .catalog
            .entries(&EntryFilter::default())
            .unwrap()
            .entries
            .len(),
        1
    );

    repo.write(
        "_docs/plans/second.md",
        &plan("Second", Some("ASMA-9011"), Some("kontor"), ""),
    );
    repo.commit("second plan");
    let second = sweep(&fixture.catalog).await;
    assert!(second.finished);
    assert_eq!(second.generation, 2, "the catalog keeps advancing");
    assert_eq!(
        fixture
            .catalog
            .entries(&EntryFilter::default())
            .unwrap()
            .entries
            .len(),
        2,
        "a newly committed plan appears despite the bad source"
    );
}

#[tokio::test]
async fn a_permission_denied_source_inside_a_root_keeps_its_plan() {
    let repo = Repo::new();
    repo.write(
        "_docs/plans/plan.md",
        &plan("Plan", Some("ASMA-9010"), Some("kontor"), ""),
    );
    let source_path = std::fs::canonicalize(repo.write(
        "notes/operator.md",
        &plan("Operator plan", Some("ASMA-9015"), Some("kontor"), ""),
    ))
    .expect("the source path canonicalizes");
    repo.commit("plans");
    let root = std::fs::canonicalize(repo.root()).expect("the repository root canonicalizes");
    let fixture = Fixture::open_settings(kontor_catalog::CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root,
            include_github_plans: false,
        }],
        sources: vec![kontor_catalog::config::OperatorSource {
            id: "denied-source".to_owned(),
            title: None,
            kind: kontor_catalog::config::SourceKind::Plan,
            path: source_path.clone(),
            workstream: None,
            epic: None,
            project_id: None,
            revision_kind: Default::default(),
            expected_sha256: None,
            logical_plan_id: None,
        }],
        ..kontor_catalog::CatalogSettings::default()
    });
    sweep(&fixture.catalog).await;
    let before = fixture
        .catalog
        .entries(&EntryFilter {
            include_removed: true,
            ..EntryFilter::default()
        })
        .unwrap();
    assert_eq!(
        before
            .entries
            .iter()
            .find(|entry| entry.title == "Operator plan")
            .unwrap()
            .state,
        EntryState::Active
    );

    repo.write(
        "_docs/plans/second.md",
        &plan("Second", Some("ASMA-9011"), Some("kontor"), ""),
    );
    repo.commit("second plan");
    std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o000))
        .expect("the source becomes unreadable");
    let second = sweep(&fixture.catalog).await;
    let after = fixture
        .catalog
        .entries(&EntryFilter {
            include_removed: true,
            ..EntryFilter::default()
        })
        .unwrap();
    std::fs::set_permissions(&source_path, std::fs::Permissions::from_mode(0o644))
        .expect("the source is restored");
    assert!(second.finished);
    assert_eq!(second.generation, 2);
    assert!(
        after.entries.iter().any(|entry| entry.title == "Second"),
        "the sweep still advances"
    );
    assert_eq!(
        after
            .entries
            .iter()
            .find(|entry| entry.title == "Operator plan")
            .unwrap()
            .state,
        EntryState::Active,
        "a permission-denied source keeps its last rows instead of being tombstoned"
    );

    std::fs::remove_file(&source_path).expect("the source is deleted");
    sweep(&fixture.catalog).await;
    let removed = fixture
        .catalog
        .entries(&EntryFilter {
            include_removed: true,
            ..EntryFilter::default()
        })
        .unwrap();
    assert_eq!(
        removed
            .entries
            .iter()
            .find(|entry| entry.title == "Operator plan")
            .unwrap()
            .state,
        EntryState::Removed,
        "a genuinely deleted source is still tombstoned"
    );
}

#[tokio::test]
async fn an_unsearchable_source_parent_inside_a_root_keeps_its_plan() {
    let repo = Repo::new();
    repo.write(
        "_docs/plans/plan.md",
        &plan("Plan", Some("ASMA-9010"), Some("kontor"), ""),
    );
    let source_path = repo.write(
        "outer/locked/operator.md",
        &plan("Operator plan", Some("ASMA-9015"), Some("kontor"), ""),
    );
    repo.commit("plans");
    let root = std::fs::canonicalize(repo.root()).expect("the repository root canonicalizes");
    let source_path = std::fs::canonicalize(source_path).expect("the source path canonicalizes");
    let parent = source_path
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the source grandparent");
    let fixture = Fixture::open_settings(kontor_catalog::CatalogSettings {
        schema_version: 1,
        repositories: vec![kontor_catalog::RepositoryConfig {
            root,
            include_github_plans: false,
        }],
        sources: vec![kontor_catalog::config::OperatorSource {
            id: "locked-source".to_owned(),
            title: None,
            kind: kontor_catalog::config::SourceKind::Plan,
            path: source_path.clone(),
            workstream: None,
            epic: None,
            project_id: None,
            revision_kind: Default::default(),
            expected_sha256: None,
            logical_plan_id: None,
        }],
        ..kontor_catalog::CatalogSettings::default()
    });
    sweep(&fixture.catalog).await;
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o000))
        .expect("the source grandparent becomes unsearchable");
    let next = sweep(&fixture.catalog).await;
    let entries = fixture
        .catalog
        .entries(&EntryFilter {
            include_removed: true,
            ..EntryFilter::default()
        })
        .unwrap();
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755))
        .expect("the source grandparent is restored");
    assert!(next.finished);
    assert_eq!(
        entries
            .entries
            .iter()
            .find(|entry| entry.title == "Operator plan")
            .unwrap()
            .state,
        EntryState::Active,
        "an unsearchable source parent keeps the source's plan instead of tombstoning it"
    );
}

#[tokio::test]
async fn source_coverage_is_bounded_and_keeps_its_summaries() {
    // The settings document admits at most 16 roots; the source view must stay
    // inside the plugin's 64-line public bound and keep its summary rows.
    let repos: Vec<Repo> = (0..16).map(|_| Repo::new()).collect();
    for (index, repo) in repos.iter().enumerate() {
        repo.write(
            &format!("_docs/plans/plan-{index:02}.md"),
            &plan(
                &format!("Plan {index:02}"),
                Some(&format!("ASMA-{}", 1_000 + index)),
                Some("kontor"),
                "",
            ),
        );
        repo.commit("plan");
    }
    let configured: Vec<_> = repos
        .iter()
        .map(|repo| (repo.root().to_path_buf(), false))
        .collect();
    let fixture = Fixture::open(&configured, None, 30);
    sweep(&fixture.catalog).await;
    let sources = fixture.catalog.sources().expect("the source status");
    for source in &sources.sources {
        assert!(
            source.coverage.len() <= 64,
            "a source stays inside the public coverage bound"
        );
    }
    let plan_source = &sources.sources[0];
    assert_eq!(plan_source.coverage.len(), 16 + 3);
    assert!(
        plan_source
            .coverage
            .iter()
            .any(|line| line.subject == "collection")
    );
    assert!(
        plan_source
            .coverage
            .iter()
            .any(|line| line.subject == "registry")
    );
    assert!(
        plan_source
            .coverage
            .iter()
            .any(|line| line.subject == "snapshot")
    );
}

#[tokio::test]
async fn source_status_reports_each_source_and_its_completion_instant() {
    let (_repo, fixture) = repository_with_plans(2);
    sweep(&fixture.catalog).await;
    let status = fixture.catalog.status().expect("the plan status");
    let sources = fixture.catalog.sources().expect("the source status");
    assert_eq!(sources.sources.len(), 3);
    assert_eq!(
        sources
            .sources
            .iter()
            .map(|source| source.source)
            .collect::<Vec<_>>(),
        [
            CatalogSource::Plan,
            CatalogSource::Jira,
            CatalogSource::Native
        ]
    );
    let plan_source = &sources.sources[0];
    assert_eq!(plan_source.state, CatalogSourceState::Complete);
    assert_eq!(plan_source.generation, status.generation);
    assert_eq!(plan_source.as_of, status.as_of);
    assert!(
        plan_source
            .coverage
            .iter()
            .any(|line| line.subject == "collection"),
        "the plan coverage line is present"
    );
    assert!(
        plan_source.pending_verification >= 2,
        "the plan's declared associations are pending verification"
    );
    assert_eq!(sources.sources[1].state, CatalogSourceState::NotConfigured);
    assert_eq!(sources.sources[1].pending_verification, 0);
    assert_eq!(sources.sources[2].state, CatalogSourceState::NeverCollected);
    assert_eq!(sources.sources[2].pending_verification, 0);
    // The status call does not move a source's freshness.
    assert_eq!(
        fixture.catalog.sources().expect("a second status").sources[0].as_of,
        plan_source.as_of
    );
}
