//! TEST-004: the headless native census.
//!
//! Cold start, archive coverage, duplicate titles, bounded pagination,
//! reconnect, unavailable Paseo, incomplete walks that delete nothing, stale
//! page fencing and the fallback snapshot's observation time. Every runtime
//! here is a source, not a live session: the transport-level protocol is
//! exercised in `kontor-runtime-paseo`.

mod support;

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use kontor_catalog::native::{
    NativeCensus, NativeCensusSource, NativeCheckout, NativeInventoryFilter, NativePageCounts,
    NativeProject, NativeSession, NativeWorkspace,
};
use kontor_catalog::{Catalog, CollectionState, EntryState, ReadSource};
use support::Fixture;

const RUNTIME: &str = "paseo:ws://127.0.0.1:6767/ws";
const CLIENT: &str = "kontor-census-test";

#[derive(Debug)]
struct SlowSource {
    runtime: String,
    client: String,
}

#[async_trait]
impl NativeCensusSource for SlowSource {
    fn runtime_instance(&self) -> &str {
        &self.runtime
    }

    fn client_id(&self) -> &str {
        &self.client
    }

    async fn observe(&self) -> NativeCensus {
        tokio::time::sleep(Duration::from_secs(10)).await;
        NativeCensus::unavailable(&self.runtime, &self.client, "unreachable")
    }
}

#[derive(Debug)]
struct FakeSource {
    runtime: String,
    client: String,
    answers: Mutex<VecDeque<NativeCensus>>,
    observations: AtomicUsize,
}

impl FakeSource {
    fn new(answers: Vec<NativeCensus>) -> Arc<Self> {
        Arc::new(Self {
            runtime: RUNTIME.to_owned(),
            client: CLIENT.to_owned(),
            answers: Mutex::new(answers.into()),
            observations: AtomicUsize::new(0),
        })
    }

    fn with_runtime(runtime: &str, answers: Vec<NativeCensus>) -> Arc<Self> {
        Arc::new(Self {
            runtime: runtime.to_owned(),
            client: CLIENT.to_owned(),
            answers: Mutex::new(answers.into()),
            observations: AtomicUsize::new(0),
        })
    }

    fn observations(&self) -> usize {
        self.observations.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl NativeCensusSource for FakeSource {
    fn runtime_instance(&self) -> &str {
        &self.runtime
    }

    fn client_id(&self) -> &str {
        &self.client
    }

    async fn observe(&self) -> NativeCensus {
        self.observations.fetch_add(1, Ordering::SeqCst);
        self.answers
            .lock()
            .expect("the fixture queue is intact")
            .pop_front()
            .expect("the fixture was given an answer for this attempt")
    }
}

fn project(id: &str, name: &str, root: &str) -> NativeProject {
    NativeProject {
        runtime_instance: RUNTIME.to_owned(),
        project_id: id.to_owned(),
        display_name: name.to_owned(),
        custom_name: None,
        project_key: Some(format!("remote:{id}")),
        root_path: root.to_owned(),
        project_kind: Some("git".to_owned()),
    }
}

fn workspace(id: &str, project_id: &str, name: &str, directory: &str) -> NativeWorkspace {
    NativeWorkspace {
        runtime_instance: RUNTIME.to_owned(),
        workspace_id: id.to_owned(),
        project_id: project_id.to_owned(),
        name: name.to_owned(),
        title: None,
        directory: directory.to_owned(),
        kind: "worktree".to_owned(),
        archiving_at: None,
        worktree_slug: None,
        project_display_name: Some("DESK • ADAM".to_owned()),
        project_custom_name: None,
        project_root_path: Some("/repo".to_owned()),
        project_kind: Some("git".to_owned()),
    }
}

fn session(id: &str, title: &str, workspace_id: &str, archived_at: Option<&str>) -> NativeSession {
    NativeSession {
        runtime_instance: RUNTIME.to_owned(),
        session_id: id.to_owned(),
        workspace_id: Some(workspace_id.to_owned()),
        project_id: Some("prj_desk".to_owned()),
        project_name: Some("DESK • ADAM".to_owned()),
        workspace_name: Some("ADAM".to_owned()),
        title: Some(title.to_owned()),
        provider: "claude".to_owned(),
        model: Some("opus".to_owned()),
        status: "idle".to_owned(),
        archived_at: archived_at.map(str::to_owned),
        attention_reason: None,
        cwd: "/repo/adam".to_owned(),
        created_at: Some("2026-10-25T00:00:00Z".to_owned()),
        updated_at: Some("2026-10-25T01:00:00Z".to_owned()),
        last_user_message_at: None,
        parent_session_id: None,
        checkout: Some(NativeCheckout {
            cwd: Some("/repo/adam".to_owned()),
            branch: Some("main".to_owned()),
            worktree_root: None,
            is_git: Some(true),
            is_paseo_owned_worktree: None,
        }),
    }
}

fn census(
    projects: Vec<NativeProject>,
    workspaces: Vec<NativeWorkspace>,
    sessions: Vec<NativeSession>,
) -> NativeCensus {
    NativeCensus {
        runtime_instance: RUNTIME.to_owned(),
        client_id: CLIENT.to_owned(),
        server_id: Some("srv-test".to_owned()),
        server_version: Some("0.11.4".to_owned()),
        protocol_version: 1,
        permissions: Some(vec!["workspace.read".to_owned()]),
        features: BTreeMap::new(),
        include_archived: true,
        complete: true,
        error: None,
        diagnostics: Vec::new(),
        pages: NativePageCounts::default(),
        projects,
        workspaces,
        sessions,
    }
}

async fn collect(catalog: &Catalog, source: &Arc<FakeSource>) -> kontor_catalog::NativeSweepReport {
    let sources: Vec<Arc<dyn NativeCensusSource>> =
        vec![Arc::clone(source) as Arc<dyn NativeCensusSource>];
    catalog
        .collect_native_once(&sources)
        .await
        .expect("the native sweep completes")
}

#[tokio::test]
async fn cold_start_reads_archived_sessions_without_merging_duplicate_titles() {
    let fixture = Fixture::open(&[], None, 30);
    let source = FakeSource::new(vec![census(
        vec![
            project("prj_desk", "DESK • ADAM", "/repo"),
            project("prj_esw", "ESW • ASMA-9001", "/repo/esw"),
        ],
        vec![
            workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam"),
            workspace("wks_esw", "prj_esw", "ASMA-9001", "/repo/esw"),
        ],
        vec![
            session("agt_live", "Implement", "wks_adam", None),
            session(
                "agt_archived",
                "Implement",
                "wks_adam",
                Some("2026-10-24T00:00:00Z"),
            ),
            session("agt_esw", "Implement", "wks_esw", None),
        ],
    )]);

    let started = Instant::now();
    let report = collect(&fixture.catalog, &source).await;
    let elapsed = started.elapsed();
    println!(
        "TEST-004 catalog census latency: {} ms for {} projects, {} workspaces, {} sessions",
        elapsed.as_millis(),
        report.projects,
        report.workspaces,
        report.sessions
    );
    assert!(
        elapsed.as_secs() < 30,
        "a complete fixture census must fit the stated 30 s bound; took {elapsed:?}"
    );
    assert!(report.complete);
    assert_eq!(report.state, CollectionState::Complete);
    assert_eq!(report.projects, 2);
    assert_eq!(report.workspaces, 2);
    assert_eq!(report.sessions, 3);
    assert_eq!(report.archived_sessions, 1);
    assert!(report.snapshot_written);
    assert_eq!(source.observations(), 1);

    let status = fixture.catalog.native_status().expect("status");
    assert_eq!(status.source, ReadSource::Database);
    assert_eq!(status.generation, report.generation);
    assert_eq!(status.collection.state, CollectionState::Complete);
    assert_eq!(status.collection.runtimes_configured, 1);
    assert_eq!(status.collection.runtimes_complete, 1);
    assert_eq!(status.counts.projects, 2);
    assert_eq!(status.counts.workspaces, 2);
    assert_eq!(status.counts.sessions, 3);
    assert_eq!(status.counts.archived_sessions, 1);
    assert!(status.collection.include_archived);
    assert_eq!(
        status.collection.coverage[0].server_version.as_deref(),
        Some("0.11.4")
    );
    assert_eq!(
        status.collection.coverage[0].permissions.as_deref(),
        Some(["workspace.read".to_owned()].as_slice())
    );

    let inventory = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert!(inventory.archive_scope);
    assert_eq!(inventory.sessions.len(), 3);
    let same_title: Vec<_> = inventory
        .sessions
        .iter()
        .filter(|session| session.title.as_deref() == Some("Implement"))
        .collect();
    assert_eq!(same_title.len(), 3);
    let ids: std::collections::BTreeSet<_> = same_title
        .iter()
        .map(|session| session.session_id.as_str())
        .collect();
    assert_eq!(ids.len(), 3, "duplicate titles must never merge identities");
    let archived = inventory
        .sessions
        .iter()
        .find(|session| session.session_id == "agt_archived")
        .expect("the archived session is explicit");
    assert_eq!(
        archived.archived_at.as_deref(),
        Some("2026-10-24T00:00:00Z")
    );
    assert_eq!(
        archived.project_name.as_deref(),
        Some("DESK • ADAM"),
        "directory project metadata is preserved"
    );
    assert_eq!(
        archived.workspace_name.as_deref(),
        Some("ADAM"),
        "directory workspace metadata is preserved"
    );
    assert_eq!(
        archived
            .checkout
            .as_ref()
            .and_then(|checkout| checkout.branch.as_deref()),
        Some("main")
    );
}

#[tokio::test]
async fn three_hundred_sessions_are_committed_in_bounded_pages() {
    let fixture = Fixture::open(&[], None, 30);
    let sessions = (0..300)
        .map(|index| session(&format!("agt_{index:03}"), "Same title", "wks_adam", None))
        .collect();
    let source = FakeSource::new(vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        sessions,
    )]);
    let report = collect(&fixture.catalog, &source).await;
    assert_eq!(report.sessions, 300);
    assert!(
        report.pages >= 3,
        "300 sessions cross several atomic checkpoint pages"
    );
    let pages = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert_eq!(pages.sessions.len(), 300);
    assert_eq!(
        fixture
            .catalog
            .native_inventory(&NativeInventoryFilter::default())
            .expect("inventory")
            .sessions
            .len(),
        300
    );
}

#[tokio::test]
async fn an_incomplete_enumeration_never_removes_a_fact_and_a_later_complete_walk_can() {
    let fixture = Fixture::open(&[], None, 30);
    let first = FakeSource::new(vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![
            session("agt_a", "A", "wks_adam", None),
            session("agt_b", "B", "wks_adam", None),
        ],
    )]);
    let complete = collect(&fixture.catalog, &first).await;
    assert_eq!(complete.generation, 1);

    let mut partial = census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_a", "A", "wks_adam", None)],
    );
    partial.complete = false;
    partial.include_archived = false;
    partial.error =
        Some("the session directory did not end within the bounded page walk".to_owned());
    let second = FakeSource::new(vec![partial]);
    let incomplete = collect(&fixture.catalog, &second).await;
    assert!(!incomplete.complete);
    assert_eq!(incomplete.state, CollectionState::Incomplete);

    let status = fixture.catalog.native_status().expect("status");
    assert_eq!(status.generation, 1, "the complete generation is untouched");
    assert_eq!(status.attempted_generation, 2);
    assert_eq!(status.collection.state, CollectionState::Incomplete);
    assert!(status.collection.retry_at.is_some());
    let still_visible = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert_eq!(
        still_visible.sessions.len(),
        2,
        "an incomplete walk may not remove a fact"
    );
    assert!(
        still_visible
            .sessions
            .iter()
            .any(|session| session.session_id == "agt_b")
    );

    let third = FakeSource::new(vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_a", "A", "wks_adam", None)],
    )]);
    let repaired = collect(&fixture.catalog, &third).await;
    assert!(repaired.complete);
    assert_eq!(repaired.generation, 3);
    let active = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert_eq!(active.sessions.len(), 1);
    let history = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter {
            include_removed: true,
            ..NativeInventoryFilter::default()
        })
        .expect("history");
    let removed = history
        .sessions
        .iter()
        .find(|session| session.session_id == "agt_b")
        .expect("the removed session is retained");
    assert_eq!(removed.state, EntryState::Removed);
    let snapshot = fixture
        .catalog
        .read_native_snapshot()
        .expect("snapshot reads")
        .expect("snapshot exists");
    assert!(
        snapshot
            .sessions
            .iter()
            .all(|session| session.session_id != "agt_b"),
        "the fallback snapshot carries active rows only, not tombstone history"
    );
}

#[tokio::test]
async fn an_unavailable_runtime_is_failed_and_retains_prior_facts() {
    let fixture = Fixture::open(&[], None, 30);
    let first = FakeSource::new(vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_a", "A", "wks_adam", None)],
    )]);
    let complete = collect(&fixture.catalog, &first).await;
    assert_eq!(complete.generation, 1);

    let second = FakeSource::new(vec![NativeCensus::unavailable(
        RUNTIME,
        CLIENT,
        "the daemon protocol socket could not be opened",
    )]);
    let failed = collect(&fixture.catalog, &second).await;
    assert!(!failed.complete);
    assert_eq!(failed.state, CollectionState::Failed);
    assert_eq!(failed.projects, 0);
    assert_eq!(failed.sessions, 0);

    let status = fixture.catalog.native_status().expect("status");
    assert_eq!(status.generation, 1);
    assert_eq!(status.attempted_generation, 2);
    assert_eq!(status.collection.state, CollectionState::Failed);
    assert_eq!(
        status.collection.coverage[0].state,
        kontor_catalog::native::NativeRuntimeState::Failed
    );
    assert_eq!(
        status.collection.coverage[0].error.as_deref(),
        Some("the daemon protocol socket could not be opened")
    );
    assert!(status.collection.retry_at.is_some());
    let retained = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert_eq!(retained.sessions.len(), 1);
    assert_eq!(retained.sessions[0].session_id, "agt_a");
}

#[tokio::test]
async fn a_complete_walk_without_archive_coverage_cannot_tombstone_or_change_archive_scope() {
    let fixture = Fixture::open(&[], None, 30);
    let first = FakeSource::new(vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![
            session("agt_active", "Active", "wks_adam", None),
            session(
                "agt_archived",
                "Archived",
                "wks_adam",
                Some("2026-10-24T00:00:00Z"),
            ),
        ],
    )]);
    let complete = collect(&fixture.catalog, &first).await;
    assert!(complete.complete);

    let mut active_only = census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_active", "Active", "wks_adam", None)],
    );
    active_only.complete = true;
    active_only.include_archived = false;
    let second = FakeSource::new(vec![active_only]);
    let incomplete = collect(&fixture.catalog, &second).await;
    assert!(
        !incomplete.complete,
        "a walk that did not cover the archive cannot prove archived sessions absent"
    );
    assert_eq!(incomplete.generation, 2);
    let status = fixture.catalog.native_status().expect("status");
    assert_eq!(status.generation, 1, "the complete generation is untouched");
    assert!(
        status.collection.include_archived,
        "archive scope still describes the served generation"
    );
    let inventory = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert!(inventory.archive_scope);
    assert!(
        inventory
            .sessions
            .iter()
            .any(|session| session.session_id == "agt_archived"),
        "an archived session is not tombstoned by a non-archive walk"
    );
}

#[tokio::test]
async fn a_source_over_the_scan_budget_is_unavailable_not_empty() {
    let fixture = Fixture::open(&[], None, 1);
    let source: Arc<dyn NativeCensusSource> = Arc::new(SlowSource {
        runtime: RUNTIME.to_owned(),
        client: CLIENT.to_owned(),
    });
    let started = Instant::now();
    let report = fixture
        .catalog
        .collect_native_once(&[source])
        .await
        .expect("the bounded census records a failed attempt");
    assert!(!report.complete);
    assert_eq!(report.state, CollectionState::Failed);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the scan budget, not the source, bounds the attempt"
    );
    let status = fixture.catalog.native_status().expect("status");
    assert_eq!(status.generation, 0);
    assert_eq!(status.collection.state, CollectionState::Failed);
    assert!(
        status.collection.coverage[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("scan budget"))
    );
}

#[tokio::test]
async fn a_fresh_failed_census_never_claims_archive_scope_or_a_generation() {
    let fixture = Fixture::open(&[], None, 30);
    let source = FakeSource::new(vec![NativeCensus::unavailable(
        RUNTIME,
        CLIENT,
        "the daemon protocol socket could not be opened",
    )]);
    let report = collect(&fixture.catalog, &source).await;
    assert!(!report.complete);
    let inventory = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert_eq!(inventory.generation, 0);
    assert!(!inventory.archive_scope);
    assert!(inventory.sessions.is_empty());
}

#[tokio::test]
async fn a_runtime_removed_from_configuration_keeps_its_visible_facts() {
    let fixture = Fixture::open(&[], None, 30);
    let other = "paseo:ws://127.0.0.1:9999/ws";
    let mut other_census = census(
        vec![project("prj_other", "OTHER", "/other")],
        vec![workspace("wks_other", "prj_other", "OTHER", "/other")],
        vec![session("agt_other", "Other", "wks_other", None)],
    );
    other_census.runtime_instance = other.to_owned();
    other_census.projects[0].runtime_instance = other.to_owned();
    other_census.workspaces[0].runtime_instance = other.to_owned();
    other_census.sessions[0].runtime_instance = other.to_owned();
    let source = FakeSource::with_runtime(other, vec![other_census]);
    let first = collect(&fixture.catalog, &source).await;
    assert!(first.complete);

    let only_remaining = FakeSource::new(vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_desk", "Desk", "wks_adam", None)],
    )]);
    let second = collect(&fixture.catalog, &only_remaining).await;
    assert!(second.complete);
    let inventory = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert!(
        inventory
            .sessions
            .iter()
            .any(|session| session.session_id == "agt_other"),
        "a runtime removed from configuration is unobserved, not removed"
    );
    assert!(
        inventory
            .sessions
            .iter()
            .any(|session| session.session_id == "agt_desk")
    );
}

#[tokio::test]
async fn a_reconnect_after_an_incomplete_walk_performs_a_complete_reconciliation() {
    let fixture = Fixture::open(&[], None, 30);
    let mut first = census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_old", "Old", "wks_adam", None)],
    );
    first.complete = false;
    first.include_archived = false;
    first.error = Some("the channel failed before the runtime answered".to_owned());
    let second = census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![
            session("agt_old", "Old", "wks_adam", None),
            session("agt_new", "New", "wks_adam", None),
        ],
    );
    let source = FakeSource::new(vec![first, second]);
    let partial = collect(&fixture.catalog, &source).await;
    assert!(!partial.complete);
    assert!(!partial.snapshot_written);
    let complete = collect(&fixture.catalog, &source).await;
    assert!(complete.complete);
    assert!(complete.snapshot_written);
    assert_eq!(complete.generation, 2);
    assert_eq!(source.observations(), 2);

    let status = fixture.catalog.native_status().expect("status");
    assert_eq!(status.generation, 2);
    assert_eq!(status.collection.state, CollectionState::Complete);
    assert_eq!(status.counts.sessions, 2);
    assert!(status.snapshot.current);
    assert_eq!(
        status.as_of.as_deref(),
        status.snapshot.written_at.as_deref()
    );
}

#[tokio::test]
async fn a_source_answering_as_another_runtime_is_never_promoted() {
    let fixture = Fixture::open(&[], None, 30);
    let good = census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_good", "Good", "wks_adam", None)],
    );
    let mut impostor = census(
        vec![project("prj_evil", "EVIL", "/elsewhere")],
        vec![workspace("wks_evil", "prj_evil", "EVIL", "/elsewhere")],
        vec![session("agt_evil", "Evil", "wks_evil", None)],
    );
    impostor.runtime_instance = "paseo:ws://127.0.0.1:9999/ws".to_owned();
    let source = FakeSource::new(vec![good, impostor]);

    let first = collect(&fixture.catalog, &source).await;
    assert!(first.complete);
    let second = collect(&fixture.catalog, &source).await;
    assert!(!second.complete, "a wrong identity is not a complete walk");
    assert_eq!(second.state, CollectionState::Failed);
    assert!(!second.snapshot_written);
    assert_eq!(second.projects, 0);
    assert_eq!(second.workspaces, 0);
    assert_eq!(second.sessions, 0);

    let status = fixture.catalog.native_status().expect("status");
    let coverage = &status.collection.coverage[0];
    assert_eq!(coverage.runtime_instance, RUNTIME);
    assert_eq!(coverage.client_id, CLIENT);
    assert_eq!(
        coverage.state,
        kontor_catalog::native::NativeRuntimeState::Failed
    );
    assert_eq!(coverage.projects, 0);
    assert_eq!(coverage.sessions, 0);

    let inventory = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert_eq!(inventory.sessions.len(), 1);
    assert_eq!(inventory.sessions[0].session_id, "agt_good");
    assert!(
        inventory
            .projects
            .iter()
            .all(|project| project.project_id != "prj_evil"),
        "an observation from another runtime identity must never be promoted"
    );
}

#[tokio::test]
async fn the_native_snapshot_falls_back_with_its_observation_time_not_the_read_time() {
    let fixture = Fixture::open(&[], None, 30);
    let source = FakeSource::new(vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_a", "A", "wks_adam", None)],
    )]);
    let report = collect(&fixture.catalog, &source).await;
    assert!(report.complete);
    let written_at = fixture
        .catalog
        .native_status()
        .expect("status")
        .snapshot
        .written_at
        .expect("the snapshot records its completion instant");
    let snapshot = fixture
        .catalog
        .read_native_snapshot()
        .expect("snapshot reads")
        .expect("snapshot exists");
    assert_eq!(
        snapshot.schema_version,
        kontor_catalog::native::NATIVE_CENSUS_VERSION
    );
    assert_eq!(snapshot.written_at, written_at);
    assert_eq!(
        snapshot.status.as_of.as_deref(),
        Some(snapshot.written_at.as_str())
    );
    assert_eq!(snapshot.sessions.len(), 1);

    // Break the database read without breaking the open connection: drop the
    // table the status count and the inventory query use.
    let connection = rusqlite::Connection::open(Catalog::database_path(fixture.state.path()))
        .expect("a second connection");
    connection
        .execute_batch("DROP TABLE catalog_native_sessions;")
        .expect("the native table is removed");

    let status = fixture
        .catalog
        .native_status()
        .expect("the snapshot answers when the database does not");
    assert_eq!(status.source, ReadSource::Snapshot);
    assert_eq!(status.generation, report.generation);
    assert_eq!(status.as_of.as_deref(), Some(snapshot.written_at.as_str()));
    assert!(status.snapshot.present);

    let inventory = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("the snapshot inventory answers");
    assert_eq!(inventory.source, ReadSource::Snapshot);
    assert_eq!(
        inventory.as_of.as_deref(),
        Some(snapshot.written_at.as_str()),
        "a fallback must not present the read instant as source freshness"
    );
    assert!(inventory.archive_scope);
    assert_eq!(inventory.sessions.len(), 1);
    assert_eq!(inventory.sessions[0].session_id, "agt_a");

    let path = Catalog::native_snapshot_path(fixture.state.path());
    let mut document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("snapshot bytes")).expect("JSON");
    document["schema_version"] = serde_json::json!(99);
    std::fs::write(&path, serde_json::to_vec(&document).expect("JSON")).expect("rewrite");
    assert!(
        fixture.catalog.read_native_snapshot().is_err(),
        "an incompatible native snapshot is refused rather than misread"
    );
}

#[tokio::test]
async fn native_collection_does_not_advance_plan_freshness() {
    let repo = support::Repo::new();
    repo.write(
        "plans/separate.md",
        &support::plan("Separate", Some("ASMA-9999"), Some("kontor"), ""),
    );
    repo.commit("separate");
    let fixture = Fixture::open(&[(repo.root().to_path_buf(), false)], None, 30);
    support::sweep(&fixture.catalog).await;
    let plan_status = fixture.catalog.status().expect("plan status");
    let plan_entries = fixture
        .catalog
        .entries(&kontor_catalog::EntryFilter::default())
        .expect("plan entries");

    let source = FakeSource::new(vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_a", "A", "wks_adam", None)],
    )]);
    collect(&fixture.catalog, &source).await;

    let after = fixture
        .catalog
        .status()
        .expect("plan status after the census");
    assert_eq!(after.generation, plan_status.generation);
    assert_eq!(after.as_of, plan_status.as_of);
    assert_eq!(
        fixture
            .catalog
            .entries(&kontor_catalog::EntryFilter::default())
            .expect("plan entries after the census")
            .entries,
        plan_entries.entries
    );
}

#[tokio::test]
async fn native_inventory_pages_are_generation_fenced() {
    let fixture = Fixture::open(&[], None, 30);
    let source = FakeSource::new(vec![
        census(
            vec![project("prj_desk", "DESK • ADAM", "/repo")],
            vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
            vec![
                session("agt_a", "A", "wks_adam", None),
                session("agt_b", "B", "wks_adam", None),
                session("agt_c", "C", "wks_adam", None),
            ],
        ),
        census(
            vec![project("prj_desk", "DESK • ADAM", "/repo")],
            vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
            vec![
                session("agt_a", "A", "wks_adam", None),
                session("agt_b", "B", "wks_adam", None),
            ],
        ),
    ]);
    let first_sweep = collect(&fixture.catalog, &source).await;
    let mut seen = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = fixture
            .catalog
            .native_inventory(&NativeInventoryFilter {
                limit: 2,
                cursor,
                ..NativeInventoryFilter::default()
            })
            .expect("a native page");
        assert_eq!(page.generation, first_sweep.generation);
        seen.extend(
            page.sessions
                .iter()
                .map(|session| session.session_id.clone()),
        );
        pages += 1;
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(pages, 2);
    seen.sort();
    assert_eq!(seen, ["agt_a", "agt_b", "agt_c"]);

    let stale = fixture.catalog.native_inventory(&NativeInventoryFilter {
        limit: 1,
        cursor: Some(format!(
            "n1:{}:0:0:1:0000000000000000",
            first_sweep.generation
        )),
        ..NativeInventoryFilter::default()
    });
    assert!(matches!(
        stale,
        Err(kontor_catalog::CatalogError::InvalidCursor { .. })
    ));

    let second_sweep = collect(&fixture.catalog, &source).await;
    assert_ne!(second_sweep.generation, first_sweep.generation);
    // An incomplete or complete later generation still fences the old cursor.
    let stale = fixture.catalog.native_inventory(&NativeInventoryFilter {
        limit: 1,
        cursor: Some(format!("n1:{}:0:0:1:{}", first_sweep.generation, {
            // The fingerprint is opaque; a cursor from another generation is
            // refused by its generation only after the filter matches.
            let view = fixture
                .catalog
                .native_inventory(&NativeInventoryFilter {
                    limit: 1,
                    ..NativeInventoryFilter::default()
                })
                .expect("a page");
            view.next_cursor
                .as_deref()
                .and_then(|cursor| cursor.rsplit(':').next())
                .unwrap_or_default()
                .to_owned()
        })),
        ..NativeInventoryFilter::default()
    });
    assert!(matches!(
        stale,
        Err(kontor_catalog::CatalogError::StaleCursor { generation, active })
            if generation == first_sweep.generation && active == second_sweep.generation
    ));
}

#[tokio::test]
async fn native_sweep_pages_and_old_checkpoints_are_pruned() {
    let fixture = Fixture::open(&[], None, 30);
    let answers = (0..20)
        .map(|_| {
            census(
                vec![project("prj_desk", "DESK • ADAM", "/repo")],
                vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
                vec![session("agt_a", "A", "wks_adam", None)],
            )
        })
        .collect();
    let source = FakeSource::new(answers);
    for _ in 0..20 {
        assert!(collect(&fixture.catalog, &source).await.complete);
    }
    let connection = rusqlite::Connection::open(Catalog::database_path(fixture.state.path()))
        .expect("the catalog database");
    let pages: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM catalog_native_sweep_pages",
            [],
            |row| row.get(0),
        )
        .expect("the page count");
    let sweeps: i64 = connection
        .query_row("SELECT COUNT(*) FROM catalog_native_sweeps", [], |row| {
            row.get(0)
        })
        .expect("the sweep count");
    assert_eq!(pages, 3, "only the current generation's pages remain");
    assert!(sweeps <= 17, "old checkpoints are bounded history");
}

#[tokio::test]
async fn failed_attempts_keep_the_served_native_checkpoint() {
    let fixture = Fixture::open(&[], None, 30);
    let mut answers = vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
        vec![session("agt_served", "Served", "wks_adam", None)],
    )];
    for _ in 0..25 {
        answers.push(NativeCensus::unavailable(
            RUNTIME,
            CLIENT,
            "the runtime is asleep",
        ));
    }
    let source = FakeSource::new(answers);
    assert!(collect(&fixture.catalog, &source).await.complete);
    for _ in 0..25 {
        assert!(!collect(&fixture.catalog, &source).await.complete);
    }

    let status = fixture.catalog.native_status().expect("status");
    assert_eq!(status.generation, 1, "the served checkpoint is retained");
    assert_eq!(status.attempted_generation, 26);
    assert!(
        status.as_of.is_some(),
        "the active checkpoint is not pruned after 17 failed attempts"
    );
    assert_eq!(status.counts.sessions, 1);
    let inventory = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter::default())
        .expect("inventory");
    assert_eq!(inventory.generation, 1);
    assert_eq!(inventory.sessions.len(), 1);
}

#[tokio::test]
async fn a_native_tombstone_is_carried_across_later_generations() {
    let fixture = Fixture::open(&[], None, 30);
    let source = FakeSource::new(vec![
        census(
            vec![project("prj_desk", "DESK • ADAM", "/repo")],
            vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
            vec![
                session("agt_keep", "Keep", "wks_adam", None),
                session("agt_drop", "Drop", "wks_adam", None),
            ],
        ),
        census(
            vec![project("prj_desk", "DESK • ADAM", "/repo")],
            vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
            vec![session("agt_keep", "Keep", "wks_adam", None)],
        ),
        census(
            vec![project("prj_desk", "DESK • ADAM", "/repo")],
            vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
            vec![session("agt_keep", "Keep", "wks_adam", None)],
        ),
        census(
            vec![project("prj_desk", "DESK • ADAM", "/repo")],
            vec![workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
            vec![session("agt_keep", "Keep", "wks_adam", None)],
        ),
    ]);
    for _ in 0..4 {
        assert!(collect(&fixture.catalog, &source).await.complete);
    }

    let history = fixture
        .catalog
        .native_inventory(&NativeInventoryFilter {
            include_removed: true,
            ..NativeInventoryFilter::default()
        })
        .expect("inventory");
    let dropped = history
        .sessions
        .iter()
        .find(|session| session.session_id == "agt_drop")
        .expect("the tombstone remains readable after later generations");
    assert_eq!(dropped.state, EntryState::Removed);
    assert!(
        !history
            .sessions
            .iter()
            .any(|session| session.session_id == "agt_drop" && session.state == EntryState::Active),
        "the removed session is not resurrected"
    );
}

#[tokio::test]
async fn native_pages_serve_active_sessions_before_archived_sessions() {
    let fixture = Fixture::open(&[], None, 30);
    let source = FakeSource::new(vec![census(
        vec![project("prj_desk", "DESK • ADAM", "/repo")],
        vec![
            workspace("wks_a", "prj_desk", "ADAM", "/repo/a"),
            workspace("wks_b", "prj_desk", "ADAM", "/repo/b"),
        ],
        vec![
            session(
                "agt_0_archived_a",
                "Archived A",
                "wks_a",
                Some("2026-10-24T00:00:00Z"),
            ),
            session(
                "agt_1_archived_b",
                "Archived B",
                "wks_a",
                Some("2026-10-24T01:00:00Z"),
            ),
            session("agt_8_live_b", "Active B", "wks_b", None),
            session("agt_9_live_a", "Active A", "wks_b", None),
        ],
    )]);
    assert!(collect(&fixture.catalog, &source).await.complete);

    let mut cursor = None;
    let mut sessions = Vec::new();
    loop {
        let page = fixture
            .catalog
            .native_inventory(&NativeInventoryFilter {
                limit: 1,
                cursor,
                ..NativeInventoryFilter::default()
            })
            .expect("a native page");
        sessions.extend(page.sessions);
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    assert_eq!(sessions.len(), 4);
    assert!(
        sessions[..2]
            .iter()
            .all(|session| session.archived_at.is_none()),
        "active sessions occupy the first pages before any archived session"
    );
    assert!(
        sessions[2..]
            .iter()
            .all(|session| session.archived_at.is_some()),
        "archived sessions follow the active ones"
    );
}
