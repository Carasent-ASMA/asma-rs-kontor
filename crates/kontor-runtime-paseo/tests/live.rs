//! An opt-in smoke test against a real, disposable Paseo daemon.
//!
//! Ignored by default, and it skips with a precise reason when its environment
//! is absent — because the alternative is worse than no coverage: a live test
//! that silently passes when it could not run tells you the integration works
//! when nothing was checked.
//!
//! # Only ever against disposable identities
//!
//! The canonical Architect / Implement / QA / Audit agents of a real ticket are
//! live seats holding real work. This test never names one: it reads the CLI's
//! own version, and it inspects only an agent id the operator supplied
//! explicitly. It creates nothing, stops nothing and archives nothing.
//!
//! ```bash
//! KONTOR_PASEO_LIVE=1 \
//! KONTOR_PASEO_HOST='<complete --host target, password and all>' \
//! KONTOR_PASEO_EXECUTABLE=paseo \
//! KONTOR_PASEO_DISPOSABLE_AGENT=agt_scratch \
//! cargo test -p kontor-runtime-paseo --test live -- --ignored --nocapture
//! ```
//!
//! # What the WebSocket gate leaves out
//!
//! The plan's live criterion also asks for canonical history, a same-agent
//! follow-up, message-id reconciliation and a permission round trip. Those ride
//! the daemon protocol socket, and this adapter declares no WebSocket client —
//! that needs an exact workspace-pinned dependency the root manifest does not
//! carry, and hand-rolling frames to avoid that gate is rejected. So this smoke
//! covers the CLI half plus the *honest degradation* the missing half produces,
//! and the protocol half stays deferred with the dependency. The frame protocol
//! itself is proved against recordings in `contract.rs`.

use kontor_core::id::{ExternalId, ExternalName, RuntimeKindKey};
use kontor_runtime::adapter::RuntimeAdapter;
use kontor_runtime::capability::{RuntimeCapability, TrustGrade};
use kontor_runtime::workspace::{WorkspacePrepareRequest, WorkspaceRoot};
use kontor_runtime_paseo::adapter::{
    PaseoAdapter, PaseoCheckpoint, PaseoConfig, PaseoExecutionScope,
};
use kontor_runtime_paseo::client::{PaseoCommand, PaseoLiveTransport, PaseoTransport};
use kontor_runtime_paseo::wire::PASEO_VERSION;
use secrecy::SecretString;

/// Why a live run could not happen, stated precisely rather than as a silent
/// pass.
struct Skip(&'static str);

fn gate() -> Result<(String, SecretString), Skip> {
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
    Ok((executable, SecretString::from(host)))
}

fn transport() -> Result<PaseoLiveTransport, Skip> {
    let (executable, host) = gate()?;
    PaseoLiveTransport::new(&executable, host, 30)
        .map_err(|_| Skip("the live transport could not be configured"))
}

fn config() -> PaseoConfig {
    PaseoConfig {
        runtime_kind: RuntimeKindKey::parse("paseo.agent").expect("a valid runtime key"),
        host_key: ExternalName::parse("paseo-live").expect("a valid host key"),
        mini_project_id: ExternalId::parse("kon-live-scratch").expect("a valid id"),
        scope: PaseoExecutionScope {
            jira_epic_key: ExternalId::parse("ASMA-0000").expect("a valid id"),
            mini_project_short_title: ExternalName::parse("Live scratch").expect("a valid name"),
            plan_item_key: ExternalId::parse("KON-LIVE-0").expect("a valid id"),
            task_short_title: ExternalName::parse("smoke").expect("a valid name"),
            canonical_worktree_cwd: WorkspaceRoot::parse("/tmp/kontor-paseo-live-scratch")
                .expect("an absolute path"),
            orchestrator_agent_id: ExternalId::parse("agt_live_orchestrator").expect("a valid id"),
        },
        max_concurrent_sessions: 1,
    }
}

/// The pinned CLI is present and is the baseline this adapter's argv evidence
/// was recorded against.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_cli_answers_as_the_pinned_baseline() {
    let transport = match transport() {
        Ok(transport) => transport,
        Err(Skip(reason)) => {
            println!("skipped: {reason}");
            return;
        }
    };
    let output = transport
        .run(&PaseoCommand::version())
        .await
        .expect("the Paseo CLI answers");
    let version: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("`--version --json` prints JSON");
    let reported = version["version"].as_str().unwrap_or_default();
    println!("live Paseo CLI reports {reported}");
    assert_eq!(
        reported, PASEO_VERSION,
        "this adapter's DTOs and argv evidence are pinned to {PASEO_VERSION}"
    );
}

/// A read-only inspect of an agent the operator nominated. Nothing else is
/// touched, and no canonical seat is ever named here.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_inspect_of_a_disposable_agent_round_trips() {
    let transport = match transport() {
        Ok(transport) => transport,
        Err(Skip(reason)) => {
            println!("skipped: {reason}");
            return;
        }
    };
    let Ok(agent_id) = std::env::var("KONTOR_PASEO_DISPOSABLE_AGENT") else {
        println!("skipped: KONTOR_PASEO_DISPOSABLE_AGENT is unset");
        return;
    };
    let output = transport
        .run(&PaseoCommand::agent_inspect(&agent_id))
        .await
        .expect("the daemon answers an inspect");
    let inspected: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("`agent inspect --json` prints JSON");
    assert_eq!(
        inspected["id"].as_str(),
        Some(agent_id.as_str()),
        "the daemon answered about the agent that was asked for"
    );
}

/// Without the daemon protocol socket, the adapter degrades honestly: it is
/// observed, never driven, and every runtime-changing operation is refused as
/// exactly the capability it is.
///
/// This is the live half of the WebSocket deferral. It is a real assertion
/// rather than a note, because "we left it out" and "we left it out and it
/// fails safe" are different claims.
#[tokio::test]
#[ignore = "requires a live Paseo daemon; see the module docs"]
async fn live_without_the_protocol_socket_the_plane_is_observed_but_not_driven() {
    let transport = match transport() {
        Ok(transport) => transport,
        Err(Skip(reason)) => {
            println!("skipped: {reason}");
            return;
        }
    };
    let host_key = config().host_key.clone();
    let adapter = PaseoAdapter::new(
        config(),
        Box::new(transport),
        PaseoCheckpoint::fresh(1, host_key),
    )
    .expect("a fresh checkpoint restores");

    let declared = adapter
        .discover_capabilities()
        .await
        .expect("the CLI probe still proves the runtime is there");
    assert_eq!(
        declared.trust_grade,
        TrustGrade::C,
        "a daemon whose features cannot be read is advisory, not trusted"
    );
    assert!(declared.supports(RuntimeCapability::Inspect));
    assert!(!declared.supports(RuntimeCapability::Launch));

    let refused = adapter
        .prepare_workspace(&WorkspacePrepareRequest {
            team_run_id: kontor_core::id::TeamRunId::generate(),
            task_id: kontor_core::id::TaskId::generate(),
            workspace_binding_id: kontor_runtime::workspace::WorkspaceBindingId::generate(),
            root: WorkspaceRoot::parse("/tmp/kontor-paseo-live-scratch").expect("absolute"),
            requested_at: kontor_core::id::Timestamp::now(),
        })
        .await
        .expect_err("an undeclared capability produces no effect");
    assert_eq!(
        refused,
        kontor_runtime::adapter::RuntimeError::UnsupportedCapability {
            capability: RuntimeCapability::PrepareWorkspace
        }
    );
}
