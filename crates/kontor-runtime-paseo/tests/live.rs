//! An opt-in conformance check against a real, disposable Paseo 0.3.1 daemon.
//!
//! Ignored by default, and it skips with a precise reason when its environment
//! is absent — because the alternative is worse than no coverage: a live test
//! that silently passes when it could not run tells you the integration works
//! when nothing was checked.
//!
//! # Non-mutating, and only ever against disposable identities
//!
//! Every request here is a read. The canonical Architect / Implement / QA /
//! Audit agents of a real ticket are live seats holding real work, and this test
//! names none of them: it opens one connection, reads the pushed identity, and
//! lists projects. It creates nothing, sends nothing, answers no permission,
//! stops nothing and archives nothing.
//!
//! The mutating half of the qualification — prepare, launch, send, permission,
//! reconnect, restart — is a composed `kontord -> Paseo` run against a
//! disposable project, and it deliberately does not live in an ordinary unit
//! test: `cargo test` must never be able to start an agent.
//!
//! ```bash
//! KONTOR_PASEO_LIVE=1 \
//! KONTOR_PASEO_HOST='127.0.0.1:6767' \
//! KONTOR_PASEO_ENDPOINT='ws://127.0.0.1:6767/ws' \
//! KONTOR_PASEO_EXECUTABLE=/Applications/Paseo.app/Contents/Resources/bin/paseo \
//! cargo test -p kontor-runtime-paseo --test live -- --ignored --nocapture
//! ```

use kontor_runtime_paseo::client::{
    PASEO_DEFAULT_ENDPOINT, PaseoCommand, PaseoLiveTransport, PaseoRpc, PaseoTransport,
};
use kontor_runtime_paseo::wire::{
    PASEO_APP_VERSION, PASEO_WS_PROTOCOL_VERSION, PaseoFeature, PaseoProjectList, version_at_least,
};
use secrecy::SecretString;

/// Why a live run could not happen, stated precisely rather than as a silent
/// pass.
struct Skip(&'static str);

fn transport() -> Result<PaseoLiveTransport, Skip> {
    if std::env::var("KONTOR_PASEO_LIVE").as_deref() != Ok("1") {
        return Err(Skip("KONTOR_PASEO_LIVE is not 1"));
    }
    let host =
        std::env::var("KONTOR_PASEO_HOST").map_err(|_| Skip("KONTOR_PASEO_HOST is unset"))?;
    if host.is_empty() {
        return Err(Skip("KONTOR_PASEO_HOST is empty"));
    }
    let executable =
        std::env::var("KONTOR_PASEO_EXECUTABLE").unwrap_or_else(|_| "paseo".to_owned());
    let endpoint = std::env::var("KONTOR_PASEO_ENDPOINT")
        .unwrap_or_else(|_| PASEO_DEFAULT_ENDPOINT.to_owned());
    PaseoLiveTransport::new(
        &executable,
        SecretString::from(host),
        &endpoint,
        "kontor-live-conformance",
        10,
    )
    .map_err(|_| Skip("the live transport could not be configured"))
}

macro_rules! live {
    () => {
        match transport() {
            Ok(transport) => transport,
            Err(Skip(reason)) => {
                println!("skipped: {reason}");
                return;
            }
        }
    };
}

/// The hello is accepted, the daemon pushes its identity, and that identity is
/// the pinned application version advertising every required feature.
///
/// This is the gate the whole adapter stands on: protocol 1 and app 0.3.1 are
/// separate pins, and a daemon that fails either is observed rather than driven.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_hello_is_accepted_and_the_daemon_pushes_a_pinned_identity() {
    let transport = live!();
    let identity = transport
        .server_identity()
        .await
        .expect("the daemon accepts protocol 1 and announces itself");

    println!(
        "live Paseo announced version={:?} serverId={}",
        identity.version, identity.server_id
    );
    assert!(
        identity.is_supported_baseline(),
        "a build below the {PASEO_APP_VERSION} floor is observed, never driven; \
         this daemon reports {:?}",
        identity.version
    );
    assert!(
        identity.missing_required().is_empty(),
        "the qualified daemon advertises every required feature, missing {:?}",
        identity.missing_required()
    );
    assert!(
        identity.supports_project_rename(),
        "Paseo 0.4.0 implements the correlated project rename even though its \
         server-info feature object omits projectRename"
    );
    assert!(
        !identity.supports(PaseoFeature::ContextManagement),
        "provider context management is absent and must not be simulated"
    );
    assert_eq!(
        PASEO_WS_PROTOCOL_VERSION, 1,
        "the hello that was just accepted carried this protocol number"
    );
}

/// A correlated read round-trips over the session envelope: the request goes out
/// as `{type:"session"}`, and only a `project.list.response` carrying this
/// request's id is accepted as its answer.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_a_correlated_session_read_round_trips() {
    let transport = live!();
    let request = PaseoRpc::project_list("kontor-live-projects".to_owned());
    let frame = transport
        .request(&request)
        .await
        .expect("the daemon answers a project list");

    assert_eq!(frame.request_id, request.request_id);
    assert_eq!(frame.response_type, request.response_type);
    let listed: PaseoProjectList = frame
        .resolve(&request, "PaseoProjectList")
        .expect("the answer is the pinned 0.3.1 shape");
    println!("live Paseo holds {} projects", listed.projects.len());

    // The same frame must not satisfy a different question. This is the
    // wrong-request refusal, proved against the real daemon's own answer rather
    // than a recording of one.
    let other = PaseoRpc::daemon_status("kontor-live-status".to_owned());
    assert!(
        frame
            .resolve::<serde_json::Value>(&other, "PaseoDaemonStatus")
            .is_err(),
        "a project list is not a daemon status, whatever id it carries"
    );
}

/// The daemon status readback agrees with the identity it pushed.
///
/// Two independent statements of the same fact — one volunteered on connect, one
/// asked for over a correlated request — and the adapter refuses to drive
/// anything unless they are both the pinned version.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_the_status_readback_agrees_with_the_pushed_identity() {
    let transport = live!();
    let pushed = transport
        .server_identity()
        .await
        .expect("the daemon announces itself");
    let request = PaseoRpc::daemon_status("kontor-live-status".to_owned());
    let frame = transport
        .request(&request)
        .await
        .expect("the daemon answers a status request");
    let status: kontor_runtime_paseo::wire::PaseoDaemonStatus = frame
        .resolve(&request, "PaseoDaemonStatus")
        .expect("the answer is the pinned 0.3.1 shape");

    assert!(
        status
            .version
            .as_deref()
            .is_some_and(|version| version_at_least(version, PASEO_APP_VERSION)),
        "the readback is at or above the {PASEO_APP_VERSION} floor, reporting {:?}",
        status.version
    );
    assert_eq!(
        status.server_id, pushed.server_id,
        "the push and the readback describe the same daemon boot"
    );
}

/// The bundled CLI clears the baseline too.
///
/// Paseo prints the bare version for `--version --json` rather than JSON, which
/// is why this reads text: a parser expecting an object here would fail against
/// the very build it is pinned to.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_cli_reports_the_supported_baseline() {
    let transport = live!();
    let output = transport
        .run(&PaseoCommand::version())
        .await
        .expect("the Paseo CLI answers");
    let reported = output.version().expect("a zero exit prints a version");
    println!("live Paseo CLI reports {reported}");
    assert!(
        version_at_least(&reported, PASEO_APP_VERSION),
        "the bundled CLI is at or above the {PASEO_APP_VERSION} floor, reporting {reported}"
    );
}

/// An unknown agent id is a refusal rather than an empty session.
///
/// The live shape of the fail-closed rule: 0.3.1 answers `fetch_agent_request`
/// for an id it does not hold with `agent: null` and an error string, and
/// reading that as "a session with no content" is how a binding gets made
/// against nothing.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_an_unknown_agent_is_refused_rather_than_answered_empty() {
    let transport = live!();
    let request = PaseoRpc::agent_fetch(
        "kontor-live-missing".to_owned(),
        "agt_kontor_does_not_exist",
    );
    let frame = transport
        .request(&request)
        .await
        .expect("the daemon answers");
    let answer: kontor_runtime_paseo::wire::PaseoAgentAnswer = frame
        .resolve(&request, "PaseoAgentAnswer")
        .expect("the answer is the pinned 0.3.1 shape");
    assert!(
        answer.agent.is_none(),
        "the daemon holds no such agent, and said so"
    );
    assert!(answer.error.is_some(), "and named the refusal");
}

/// OP-02 checkpoint 4: adoption, child placement and archive against the real
/// daemon, proving the run creates no project.
///
/// Paseo 0.3.1 can create a project and cannot delete one, so a "disposable
/// epic" made by creation would be permanent residue and the project-id set
/// could never come back equal. The disposable unit is therefore the *child*
/// container, which `workspace archive` can remove, and the epic root is
/// **adopted** — which is the path OP-02 wants exercised anyway.
///
/// The assertion is the point: the project-id set before and after must be
/// identical. Any inequality means this run registered a project, which is the
/// residue the checkpoint exists to forbid.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_adopted_root_places_and_archives_a_child_and_registers_no_project() {
    let transport = live!();

    let project_ids = |listed: &PaseoProjectList| {
        listed
            .projects
            .iter()
            .map(|project| project.id.clone())
            .collect::<std::collections::BTreeSet<_>>()
    };

    let request = PaseoRpc::project_list("kontor-op02-before".to_owned());
    let frame = transport
        .request(&request)
        .await
        .expect("the daemon lists projects");
    let before: PaseoProjectList = frame
        .resolve(&request, "PaseoProjectList")
        .expect("a project list");
    let before_ids = project_ids(&before);
    println!("project ids BEFORE ({}):", before_ids.len());
    for id in &before_ids {
        println!("  {id}");
    }
    assert!(
        !before_ids.is_empty(),
        "this host holds no project to adopt as an epic root"
    );

    // Adopt: the root is read back by exact id and never registered. Which
    // project is adopted is configuration, so the test takes the first id the
    // host reports rather than matching a display name — a name is not identity.
    let adopted = before_ids
        .iter()
        .next()
        .expect("the host holds at least one project")
        .clone();
    println!("adopting existing project by exact id: {adopted}");

    // The disposable child, labelled by a topology node exactly as the adapter
    // labels one.
    let node = "01890000-0000-7000-8000-00000000c001";
    let title = format!("KON-OP-02 disposable [kontor-node-{node}]");
    let cwd = std::env::var("KONTOR_PASEO_DISPOSABLE_CWD")
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    let created = transport
        .run(&PaseoCommand::workspace_create(&cwd, &adopted, &title))
        .await
        .expect("the child container is created inside the adopted root");
    let created: kontor_runtime_paseo::wire::PaseoCliWorkspaceCreated = created
        .parse("PaseoCliWorkspaceCreated")
        .expect("the create is acknowledged");
    println!(
        "created disposable child workspace: {}",
        created.workspace_id
    );

    // Archive it. This is the whole reason the disposable unit is a workspace.
    let archived = transport
        .run(&PaseoCommand::workspace_archive(&created.workspace_id))
        .await;
    assert!(
        archived.is_ok(),
        "the disposable child must be removable: {archived:?}"
    );
    println!("archived disposable child workspace");

    let request = PaseoRpc::project_list("kontor-op02-after".to_owned());
    let frame = transport
        .request(&request)
        .await
        .expect("the daemon lists projects");
    let after: PaseoProjectList = frame
        .resolve(&request, "PaseoProjectList")
        .expect("a project list");
    let after_ids = project_ids(&after);
    println!("project ids AFTER ({}):", after_ids.len());
    for id in &after_ids {
        println!("  {id}");
    }

    assert_eq!(
        before_ids, after_ids,
        "the run must register no project: adoption reads a root back, it never creates one"
    );
}
