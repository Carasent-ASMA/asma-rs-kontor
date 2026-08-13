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
    PASEO_APP_VERSION, PASEO_WS_PROTOCOL_VERSION, PaseoFeature, PaseoProjectList,
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
    assert_eq!(
        identity.version.as_deref(),
        Some(PASEO_APP_VERSION),
        "this adapter's DTOs and argv evidence are pinned to {PASEO_APP_VERSION}"
    );
    assert!(
        identity.is_pinned_baseline(),
        "an unpinned build is observed, never driven"
    );
    assert!(
        identity.missing_required().is_empty(),
        "the qualified daemon advertises every required feature, missing {:?}",
        identity.missing_required()
    );
    for feature in [PaseoFeature::ProjectRename, PaseoFeature::Compaction] {
        assert!(
            !identity.supports(feature),
            "{feature:?} is not a supported 0.3.1 operation and must not be simulated"
        );
    }
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

    assert_eq!(status.version.as_deref(), Some(PASEO_APP_VERSION));
    assert_eq!(
        status.server_id, pushed.server_id,
        "the push and the readback describe the same daemon boot"
    );
}

/// The bundled CLI is the pinned baseline too.
///
/// 0.3.1 prints the bare version for `--version --json` rather than JSON, which
/// is why this reads text: a parser expecting an object here would fail against
/// the very build it is pinned to.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_cli_reports_the_pinned_baseline() {
    let transport = live!();
    let output = transport
        .run(&PaseoCommand::version())
        .await
        .expect("the Paseo CLI answers");
    let reported = output.version().expect("a zero exit prints a version");
    println!("live Paseo CLI reports {reported}");
    assert_eq!(reported, PASEO_APP_VERSION);
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
