//! The headless Paseo census contract.
//!
//! The recorded daemon answers the same correlated frames the live socket
//! does, so these tests state what crossed the wire: the exact read request
//! types, the absent label filter, the archive scope, the followed cursors and
//! the refusal of an incomplete walk. The one ignored test is the live
//! protocol preflight against the installed daemon.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use kontor_runtime_paseo::census::{PaseoCensus, PaseoCensusConfig};
use kontor_runtime_paseo::client::{
    PaseoCommand, PaseoFrame, PaseoOutput, PaseoRpc, PaseoTransport,
};
use kontor_runtime_paseo::fixture::RecordedPaseo;
use kontor_runtime_paseo::wire::PaseoServerInfo;
use secrecy::SecretString;
use serde_json::json;

const RUNTIME: &str = "paseo:ws://fixture/ws";
const CLIENT: &str = "kontor-census-fixture";

fn agent(id: &str, title: &str, workspace: &str, archived: Option<&str>) -> serde_json::Value {
    json!({
        "id": id,
        "provider": "claude",
        "model": "opus",
        "workspaceId": workspace,
        "cwd": "/repo/task",
        "title": title,
        "status": if archived.is_some() { "closed" } else { "idle" },
        "archivedAt": archived,
        "createdAt": "2026-10-25T00:00:00Z",
        "updatedAt": "2026-10-25T01:00:00Z",
        "lastUserMessageAt": null,
        "labels": {},
        "currentModeId": "default",
        "pendingPermissions": [],
    })
}

fn placement(project: &str, name: &str) -> serde_json::Value {
    json!({
        "projectKey": project,
        "projectName": name,
        "workspaceName": "ADAM",
        "checkout": {
            "cwd": "/repo/task",
            "isGit": true,
            "worktreeRoot": "/repo/task",
            "currentBranch": "feat/test",
            "isPaseoOwnedWorktree": false,
            "mainRepoRoot": "/repo",
            "remoteUrl": "https://user:secret@example.invalid/repo.git"
        }
    })
}

fn project_list() -> serde_json::Value {
    json!({
        "projects": [
            {"projectId": "prj_desk", "projectDisplayName": "DESK • ADAM", "projectKey": "remote:desk",
             "projectRootPath": "/repo", "projectKind": "git"},
            {"projectId": "prj_empty", "projectDisplayName": "DESK • OPS", "projectKey": "remote:ops",
             "projectRootPath": "/repo/ops", "projectKind": "git"}
        ],
        "error": null
    })
}

fn workspace(id: &str, project: &str, name: &str, directory: &str) -> serde_json::Value {
    json!({
        "id": id,
        "projectId": project,
        "projectDisplayName": if project == "prj_desk" { "DESK • ADAM" } else { "DESK • OPS" },
        "projectRootPath": if project == "prj_desk" { "/repo" } else { "/repo/ops" },
        "projectKind": "git",
        "workspaceDirectory": directory,
        "workspaceKind": "worktree",
        "name": name,
        "title": null,
        "archivingAt": null,
        "status": "done",
        "activityAt": null,
        "scripts": []
    })
}

fn scripted_daemon() -> Arc<RecordedPaseo> {
    let daemon = Arc::new(RecordedPaseo::new());
    daemon.queue_answer_rpc("project.list.request", project_list());
    daemon.queue_answer_rpc(
        "fetch_workspaces_request",
        json!({
            "requestId": "ignored",
            "entries": [workspace("wks_adam", "prj_desk", "ADAM", "/repo/adam")],
            "emptyProjects": [{
                "projectId": "prj_empty", "projectDisplayName": "DESK • OPS",
                "projectKey": "remote:ops", "projectRootPath": "/repo/ops", "projectKind": "git"
            }],
            "pageInfo": {"nextCursor": "workspace-page-2", "prevCursor": null, "hasMore": true},
            "error": null
        }),
    );
    daemon.queue_answer_rpc(
        "fetch_workspaces_request",
        json!({
            "requestId": "ignored",
            "entries": [workspace("wks_ops", "prj_empty", "OPS", "/repo/ops")],
            "pageInfo": {"nextCursor": null, "prevCursor": null, "hasMore": false},
            "error": null
        }),
    );
    daemon.queue_answer_rpc(
        "fetch_agents_request",
        json!({
            "requestId": "ignored",
            "entries": [
                {"agent": agent("agt_live", "Implement", "wks_adam", None),
                 "project": placement("prj_desk", "DESK • ADAM")},
                {"agent": agent("agt_archived", "Implement", "wks_adam", Some("2026-10-24T00:00:00Z")),
                 "project": placement("prj_desk", "DESK • ADAM")}
            ],
            "pageInfo": {"nextCursor": "session-page-2", "prevCursor": null, "hasMore": true},
            "error": null
        }),
    );
    daemon.queue_answer_rpc(
        "fetch_agents_request",
        json!({
            "requestId": "ignored",
            "entries": [
                {"agent": agent("agt_ops", "Implement", "wks_ops", None),
                 "project": placement("prj_empty", "DESK • OPS")}
            ],
            "pageInfo": {"nextCursor": null, "prevCursor": null, "hasMore": false},
            "error": null
        }),
    );
    daemon
}

fn census_for(daemon: Arc<RecordedPaseo>, max_pages: usize) -> PaseoCensus {
    PaseoCensus::with_transport(RUNTIME, CLIENT, 200, max_pages, Box::new(daemon))
}

#[tokio::test]
async fn the_census_reads_every_page_and_preserves_native_identity_and_metadata() {
    let daemon = scripted_daemon();
    let census = census_for(Arc::clone(&daemon), 1_000).census().await;
    assert!(census.complete, "{:?}", census.error);
    assert!(census.include_archived);
    assert_eq!(census.server_version.as_deref(), Some("0.8.0"));
    assert_eq!(census.projects.len(), 2, "the empty project is explicit");
    assert!(
        census
            .projects
            .iter()
            .any(|project| project.project_id == "prj_empty")
    );
    assert_eq!(census.workspaces.len(), 2);
    assert_eq!(census.sessions.len(), 3);
    assert_eq!(census.archived_sessions(), 1);
    assert_eq!(census.pages.workspace_pages, 2);
    assert_eq!(census.pages.session_pages, 2);

    let titles: Vec<_> = census
        .sessions
        .iter()
        .filter(|session| session.title.as_deref() == Some("Implement"))
        .collect();
    assert_eq!(titles.len(), 3);
    let ids: std::collections::BTreeSet<_> = titles
        .iter()
        .map(|session| session.session_id.as_str())
        .collect();
    assert_eq!(ids.len(), 3, "duplicate titles never merge identities");
    let archived = census
        .sessions
        .iter()
        .find(|session| session.session_id == "agt_archived")
        .expect("the archived session is included");
    assert!(archived.archived_at.is_some());
    assert_eq!(archived.project_id.as_deref(), Some("prj_desk"));
    assert_eq!(archived.project_name.as_deref(), Some("DESK • ADAM"));
    assert_eq!(archived.workspace_name.as_deref(), Some("ADAM"));
    assert_eq!(
        archived
            .checkout
            .as_ref()
            .and_then(|checkout| checkout.branch.as_deref()),
        Some("feat/test")
    );

    let projects = daemon.sent_messages("fetch_agents_request");
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0]["filter"]["includeArchived"], true);
    assert!(
        projects[0]["filter"].get("labels").is_none(),
        "an empty label map is omitted rather than sent as a filter"
    );
    assert_eq!(projects[0]["page"]["limit"], 200);
    assert_eq!(projects[1]["page"]["cursor"], "session-page-2");
    let workspaces = daemon.sent_messages("fetch_workspaces_request");
    assert_eq!(workspaces.len(), 2);
    assert!(
        workspaces[0].get("filter").is_none(),
        "the host-wide workspace read carries no governed project filter"
    );
    assert_eq!(workspaces[1]["page"]["cursor"], "workspace-page-2");
    assert!(
        daemon.mutations().is_empty(),
        "the census must send only read requests"
    );
    assert_eq!(daemon.count("rpc project.list.request"), 1);
    assert_eq!(daemon.count("rpc fetch_workspaces_request"), 2);
    assert_eq!(daemon.count("rpc fetch_agents_request"), 2);
}

#[tokio::test]
async fn a_missing_continuation_cursor_makes_the_census_incomplete() {
    let daemon = Arc::new(RecordedPaseo::new());
    daemon.queue_answer_rpc(
        "project.list.request",
        json!({"projects": [], "error": null}),
    );
    daemon.queue_answer_rpc(
        "fetch_workspaces_request",
        json!({
            "requestId": "ignored",
            "entries": [],
            "pageInfo": {"nextCursor": null, "prevCursor": null, "hasMore": true},
            "error": null
        }),
    );
    let census = census_for(daemon, 1_000).census().await;
    assert!(!census.complete);
    assert!(
        census
            .error
            .as_deref()
            .is_some_and(|error| error.contains("continuation cursor"))
    );
    assert!(!census.include_archived);
}

#[tokio::test]
async fn a_repeated_cursor_makes_the_census_incomplete() {
    let daemon = Arc::new(RecordedPaseo::new());
    daemon.queue_answer_rpc(
        "project.list.request",
        json!({"projects": [], "error": null}),
    );
    for _ in 0..2 {
        daemon.queue_answer_rpc(
            "fetch_workspaces_request",
            json!({
                "requestId": "ignored",
                "entries": [],
                "pageInfo": {"nextCursor": "same-cursor", "prevCursor": null, "hasMore": true},
                "error": null
            }),
        );
    }
    let census = census_for(daemon, 1_000).census().await;
    assert!(!census.complete);
    assert!(
        census
            .error
            .as_deref()
            .is_some_and(|error| error.contains("repeated"))
    );
}

#[tokio::test]
async fn the_bounded_page_walk_refuses_to_call_a_truncated_list_complete() {
    let daemon = Arc::new(RecordedPaseo::new());
    daemon.queue_answer_rpc(
        "project.list.request",
        json!({"projects": [], "error": null}),
    );
    daemon.queue_answer_rpc(
        "fetch_workspaces_request",
        json!({
            "requestId": "ignored",
            "entries": [],
            "pageInfo": {"nextCursor": "more", "prevCursor": null, "hasMore": true},
            "error": null
        }),
    );
    let census = census_for(daemon, 1).census().await;
    assert!(!census.complete);
    assert!(
        census
            .error
            .as_deref()
            .is_some_and(|error| error.contains("bounded page walk"))
    );
}

#[tokio::test]
async fn a_connection_without_workspace_read_is_incomplete_not_empty() {
    let daemon = Arc::new(RecordedPaseo::new());
    let mut identity = RecordedPaseo::baseline_identity();
    identity.permissions = Some(std::collections::BTreeSet::new());
    daemon.set_identity(&serde_json::to_value(&identity).expect("identity JSON"));
    let census = census_for(Arc::clone(&daemon), 1_000).census().await;
    assert!(!census.complete);
    assert!(
        census
            .error
            .as_deref()
            .is_some_and(|error| error.contains("workspace.read"))
    );
    assert!(
        daemon.calls().is_empty(),
        "a denied connection reads nothing"
    );
}

#[tokio::test]
async fn a_runtime_below_the_protocol_baseline_is_refused() {
    let daemon = Arc::new(RecordedPaseo::new());
    let mut identity = RecordedPaseo::baseline_identity();
    identity.version = Some("0.7.0".to_owned());
    daemon.set_identity(&serde_json::to_value(&identity).expect("identity JSON"));
    let census = census_for(Arc::clone(&daemon), 1_000).census().await;
    assert!(!census.complete);
    assert!(
        census
            .error
            .as_deref()
            .is_some_and(|error| error.contains("baseline"))
    );
    assert!(daemon.calls().is_empty());
}

#[derive(Debug)]
struct UnavailableTransport;

#[async_trait]
impl PaseoTransport for UnavailableTransport {
    async fn server_identity(&self) -> RuntimeResult<PaseoServerInfo> {
        Err(RuntimeError::Transport {
            rule: "the daemon protocol socket could not be opened",
        })
    }

    async fn run(&self, _command: &PaseoCommand) -> RuntimeResult<PaseoOutput> {
        unreachable!("a census never runs the CLI")
    }

    async fn request(&self, _request: &PaseoRpc) -> RuntimeResult<PaseoFrame> {
        unreachable!("a census never writes after a failed gate")
    }

    async fn drain_stream(&self, _agent_id: &str) -> RuntimeResult<Vec<serde_json::Value>> {
        unreachable!("a census never reads a stream")
    }
}

#[tokio::test]
async fn an_unreachable_runtime_is_incomplete_with_an_error_and_no_fabricated_rows() {
    let census =
        PaseoCensus::with_transport(RUNTIME, CLIENT, 200, 1_000, Box::new(UnavailableTransport))
            .census()
            .await;
    assert!(!census.complete);
    assert!(census.server_id.is_none());
    assert!(census.projects.is_empty());
    assert!(census.workspaces.is_empty());
    assert!(census.sessions.is_empty());
    assert!(
        census
            .error
            .as_deref()
            .is_some_and(|error| error.contains("socket"))
    );
    assert!(!census.include_archived);
}

#[derive(Debug)]
struct CountingReset {
    inner: Arc<RecordedPaseo>,
    resets: Arc<AtomicUsize>,
}

#[async_trait]
impl PaseoTransport for CountingReset {
    async fn server_identity(&self) -> RuntimeResult<PaseoServerInfo> {
        self.inner.server_identity().await
    }

    async fn run(&self, command: &PaseoCommand) -> RuntimeResult<PaseoOutput> {
        self.inner.run(command).await
    }

    async fn request(&self, request: &PaseoRpc) -> RuntimeResult<PaseoFrame> {
        self.inner.request(request).await
    }

    async fn drain_stream(&self, agent_id: &str) -> RuntimeResult<Vec<serde_json::Value>> {
        self.inner.drain_stream(agent_id).await
    }

    async fn reset_connection(&self) -> RuntimeResult<()> {
        self.resets.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn every_census_starts_on_a_fresh_connection_and_failure_discards_it() {
    let working = scripted_daemon();
    let complete_resets = Arc::new(AtomicUsize::new(0));
    let complete = CountingReset {
        inner: Arc::clone(&working),
        resets: Arc::clone(&complete_resets),
    };
    let census = PaseoCensus::with_transport(RUNTIME, CLIENT, 200, 1_000, Box::new(complete))
        .census()
        .await;
    assert!(census.complete);
    assert_eq!(
        complete_resets.load(Ordering::SeqCst),
        1,
        "a complete census starts on a fresh connection"
    );

    let broken = Arc::new(RecordedPaseo::new());
    let failing_resets = Arc::new(AtomicUsize::new(0));
    let failing = CountingReset {
        inner: Arc::clone(&broken),
        resets: Arc::clone(&failing_resets),
    };
    let census = PaseoCensus::with_transport(RUNTIME, CLIENT, 200, 1_000, Box::new(failing))
        .census()
        .await;
    assert!(!census.complete);
    assert_eq!(
        failing_resets.load(Ordering::SeqCst),
        2,
        "a failed census discards its connection so the retry re-gates"
    );
}

#[derive(Debug)]
struct ChangingIdentity {
    inner: Arc<RecordedPaseo>,
    reads: AtomicUsize,
}

#[async_trait]
impl PaseoTransport for ChangingIdentity {
    async fn server_identity(&self) -> RuntimeResult<PaseoServerInfo> {
        let read = self.reads.fetch_add(1, Ordering::SeqCst);
        let mut identity = self.inner.server_identity().await?;
        if read >= 1 {
            identity.server_id = "srv_after_restart".to_owned();
        }
        Ok(identity)
    }

    async fn run(&self, command: &PaseoCommand) -> RuntimeResult<PaseoOutput> {
        self.inner.run(command).await
    }

    async fn request(&self, request: &PaseoRpc) -> RuntimeResult<PaseoFrame> {
        self.inner.request(request).await
    }

    async fn drain_stream(&self, agent_id: &str) -> RuntimeResult<Vec<serde_json::Value>> {
        self.inner.drain_stream(agent_id).await
    }
}

#[tokio::test]
async fn a_boot_identity_change_during_the_walk_discards_the_census() {
    let daemon = scripted_daemon();
    let changing = ChangingIdentity {
        inner: Arc::clone(&daemon),
        reads: AtomicUsize::new(0),
    };
    let census = PaseoCensus::with_transport(RUNTIME, CLIENT, 200, 1_000, Box::new(changing))
        .census()
        .await;
    assert!(!census.complete);
    assert!(
        census
            .error
            .as_deref()
            .is_some_and(|error| error.contains("boot identity changed"))
    );
}

#[test]
fn a_census_config_never_prints_its_host_target() {
    let config = PaseoCensusConfig::new(
        RUNTIME,
        "paseo",
        SecretString::from("https://user:hunter2@paseo.invalid".to_owned()),
        "ws://127.0.0.1:6767/ws",
        "kontor-census",
        30,
    );
    let printed = format!("{config:?}");
    assert!(!printed.contains("hunter2"));
    assert!(!printed.contains("paseo.invalid"));
}

/// The live protocol preflight.
///
/// Read-only by construction: identity, projects, workspaces and sessions. It
/// creates, prompts, stops and archives nothing. Run it explicitly:
///
/// `cargo test -p kontor-runtime-paseo --test census the_installed_daemon -- --ignored --nocapture`
#[tokio::test]
#[ignore = "qualifies the installed daemon at ws://127.0.0.1:6767/ws"]
async fn the_installed_daemon_answers_the_census_contract() {
    let endpoint = std::env::var("PASEO_CENSUS_ENDPOINT")
        .unwrap_or_else(|_| "ws://127.0.0.1:6767/ws".to_owned());
    let started = std::time::Instant::now();
    let census = PaseoCensus::new(PaseoCensusConfig::new(
        format!("paseo:{endpoint}"),
        "paseo",
        SecretString::from("127.0.0.1:6767".to_owned()),
        &endpoint,
        "kontor-census-preflight",
        20,
    ))
    .expect("the preflight transport builds")
    .census()
    .await;
    let elapsed = started.elapsed();
    println!(
        "LIVE census: complete={} version={:?} projects={} workspaces={} sessions={} archived={} pages={:?} latency={}ms error={:?} diagnostics={:?}",
        census.complete,
        census.server_version,
        census.projects.len(),
        census.workspaces.len(),
        census.sessions.len(),
        census.archived_sessions(),
        census.pages,
        elapsed.as_millis(),
        census.error,
        census.diagnostics
    );
    for session in census.sessions.iter().take(5) {
        println!(
            "LIVE session id={} title={:?} project={:?} workspace={:?} archived={:?} status={}",
            session.session_id,
            session.title,
            session.project_id,
            session.workspace_id,
            session.archived_at,
            session.status
        );
    }
    assert!(
        census.complete,
        "the installed daemon must answer a complete read-only census: {:?}",
        census.error
    );
    assert!(census.server_version.is_some());
    assert!(census.include_archived);
    assert!(census.pages.project_pages >= 1);
    assert!(census.pages.workspace_pages >= 1);
    assert!(census.pages.session_pages >= 1);
}
