//! TASK-004: generation-consistent entry cursors and the per-source observer
//! status.
//!
//! The cursor is proved against a real catalog and real sweeps: a page never
//! mixes generations, a cursor from another filter is refused, and a source
//! status reports the generation's completion instant rather than the instant
//! the status was read.

mod support;

use kontor_catalog::{
    CatalogError, CatalogSource, CatalogSourceState, EntryFilter, EntryKind, ReadSource,
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
    assert_eq!(sources.sources[2].state, CatalogSourceState::NeverCollected);
    // The status call does not move a source's freshness.
    assert_eq!(
        fixture.catalog.sources().expect("a second status").sources[0].as_of,
        plan_source.as_of
    );
}
