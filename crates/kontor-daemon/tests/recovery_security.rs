//! Credential rotation, restore-then-reconcile, and what a log line may contain.
//!
//! These are process properties rather than store properties, so every test here
//! starts a real Realm in a real state root and drives the real router. Nothing
//! binds a socket or spawns the binary (TST-001).
//!
//! The mutants this suite exists to kill:
//!
//! * a rotated Realm still answering to a previously issued token;
//! * a rotation that changes only one tier, or that changes memory without the
//!   file (or the file without memory);
//! * a restored Realm opening scheduling before it has reconciled;
//! * an operator command mutating a state root a daemon still owns;
//! * a log line carrying a header, a body, a credential path or a token —
//!   including under a field name that is otherwise allowed.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use kontor_api::state::{BarrierState, RuntimeRegistry};
use kontor_core::id::Timestamp;
use kontor_daemon::{DATABASE_FILE, Daemon, DaemonConfig, credentials, logging, recovery};
use tempfile::TempDir;
use tower::ServiceExt;

/// The tiers a Realm mints, in the order the credential file names them.
const TIERS: [&str; 3] = ["observer", "operator", "admin"];

/// Read one tier's secret out of the state root's credential file.
fn secret(state_root: &Path, tier: &str) -> String {
    let bytes =
        std::fs::read(credentials::path_in(state_root)).expect("the credential file exists");
    let document: serde_json::Value =
        serde_json::from_slice(&bytes).expect("the credential file is JSON");
    document
        .get(tier)
        .and_then(serde_json::Value::as_str)
        .expect("the credential file names every tier")
        .to_owned()
}

/// A loopback-shaped, authenticated `GET /v1`.
async fn identity(router: &Router, token: &str) -> StatusCode {
    let request = Request::builder()
        .method("GET")
        .uri("/v1/realm")
        .header("host", "127.0.0.1:7717")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("a well-formed request");
    router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers")
        .status()
}

#[tokio::test]
async fn rotation_refuses_every_previous_token_and_leaves_the_realm_alone() {
    let home = TempDir::new().expect("a temporary directory");
    let daemon = Daemon::start(
        DaemonConfig::at(home.path()).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("the realm starts");
    let router = daemon.router();
    let realm = daemon.realm_id();

    let before: Vec<String> = TIERS.iter().map(|tier| secret(home.path(), tier)).collect();
    for token in &before {
        assert_eq!(
            identity(&router, token).await,
            StatusCode::OK,
            "every minted tier authorizes before the rotation"
        );
    }

    daemon
        .rotate_credentials()
        .expect("the realm rotates its credentials");

    let after: Vec<String> = TIERS.iter().map(|tier| secret(home.path(), tier)).collect();
    for (tier, (old, new)) in TIERS.iter().zip(before.iter().zip(after.iter())) {
        assert_ne!(old, new, "the {tier} secret must be regenerated too");
        assert_eq!(
            new.len(),
            64,
            "a secret is 32 bytes of entropy, hex encoded"
        );
    }

    for (tier, token) in TIERS.iter().zip(before.iter()) {
        assert_eq!(
            identity(&router, token).await,
            StatusCode::UNAUTHORIZED,
            "the previous {tier} token must be refused from the next request onwards"
        );
    }
    for token in &after {
        assert_eq!(
            identity(&router, token).await,
            StatusCode::OK,
            "the new tokens are this realm's credentials"
        );
    }

    // What rotation is *not* allowed to change: which Realm this is, and the
    // sessions this process is holding. A credential is how a client proves who
    // it is, not how a runtime session is identified.
    assert_eq!(daemon.realm_id(), realm);
    assert!(daemon.state().sessions().is_empty());
    assert_eq!(
        daemon
            .state()
            .with_store(kontor_store::SqliteStore::realm_id),
        realm
    );
}

#[tokio::test]
async fn a_restored_realm_starts_with_scheduling_shut_until_it_reconciles() {
    let home = TempDir::new().expect("a temporary directory");
    let state_root = home.path().join("realm");
    let daemon = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("the realm starts");
    let realm = daemon.realm_id();

    // A snapshot of a *serving* realm, which is the only kind an operator gets
    // to take without stopping work.
    let outcome = daemon
        .snapshot(None)
        .expect("a snapshot is taken while the realm serves");
    assert_eq!(outcome.manifest.realm_id, realm);

    // While the daemon owns the state root, an offline operation refuses rather
    // than racing it.
    let refused = recovery::restore(&state_root, &outcome.snapshot, Timestamp::now())
        .expect_err("a restore never runs under a live daemon");
    assert_eq!(refused.category(), "state_root_locked");

    daemon.shutdown();

    let plan = recovery::restore(&state_root, &outcome.snapshot, Timestamp::now())
        .expect("the stopped realm is restored");
    assert_eq!(plan.realm_id, realm);
    assert!(plan.reconciliation_required);

    let restarted = Daemon::start(
        DaemonConfig::at(&state_root).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("the restored realm starts");
    assert_eq!(
        restarted.realm_id(),
        realm,
        "a restore reinstates the same realm"
    );
    assert_eq!(
        restarted.state().barrier().state(),
        BarrierState::Pending,
        "a restored realm may not schedule before it has taken its inventory"
    );
    assert_eq!(restarted.reconcile().await, BarrierState::Open);
    assert!(restarted.state().barrier().state().is_open());
}

#[tokio::test]
async fn an_operator_command_refuses_a_state_root_a_daemon_owns() {
    let home = TempDir::new().expect("a temporary directory");
    let daemon = Daemon::start(
        DaemonConfig::at(home.path()).with_port(0),
        RuntimeRegistry::new(),
    )
    .expect("the realm starts");
    let before = std::fs::read(credentials::path_in(home.path())).expect("the credential file");

    let refused = recovery::rotate_credentials(home.path())
        .expect_err("the stopped-realm rotation never runs under a live daemon");
    assert_eq!(refused.category(), "state_root_locked");
    assert_eq!(
        std::fs::read(credentials::path_in(home.path())).expect("the credential file"),
        before,
        "a refused rotation must not have replaced the credentials"
    );

    // An export, by contrast, is a read and is allowed while the realm serves.
    let export = recovery::export(home.path(), Timestamp::now()).expect("a live realm exports");
    assert_eq!(export.source_realm_id, daemon.realm_id());
}

/// A writer the test can read back, so the assertion is over the bytes the sink
/// really produced rather than over what a formatter was asked to do.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .expect("the buffer is not poisoned")
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for Captured {
    type Writer = Self;

    fn make_writer(&'writer self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn a_log_line_carries_allowlisted_fields_only_and_never_a_credential() {
    let captured = Captured::default();
    let subscriber = logging::subscriber(captured.clone());

    tracing::subscriber::with_default(subscriber, || {
        // The shape of a failure path: an error category, an opaque id, and a
        // handful of fields nobody should ever have added.
        tracing::error!(
            realm_id = "0193f000-0000-7000-8000-0000000000a1",
            category = "verification",
            authorization = "Bearer 0123456789abcdef0123456789abcdef",
            payload = r#"{"messages":[{"role":"user","content":"a transcript"}]}"#,
            credential_path = "/home/operator/.kontor/credentials.json",
            token = "sk-0123456789abcdef0123456789",
            detail = "Bearer 0123456789abcdef0123456789abcdef",
            "the snapshot could not be verified"
        );
    });

    let written = String::from_utf8(
        captured
            .0
            .lock()
            .expect("the buffer is not poisoned")
            .clone(),
    )
    .expect("the log is UTF-8");

    for canary in [
        "0123456789abcdef0123456789abcdef",
        "sk-0123456789abcdef0123456789",
        "credentials.json",
        "a transcript",
        "authorization",
        "payload",
        "credential_path",
    ] {
        assert!(
            !written.contains(canary),
            "the sink wrote `{canary}`:\n{written}"
        );
    }
    assert!(written.contains("the snapshot could not be verified"));
    assert!(written.contains("realm_id=0193f000-0000-7000-8000-0000000000a1"));
    assert!(written.contains("category=verification"));
    assert!(
        written.contains(&format!("detail={}", logging::REDACTED)),
        "an allowed field carrying a token is written as the marker, not dropped silently:\n{written}"
    );
}

#[test]
fn a_sensitive_span_field_is_redacted_on_the_failure_path_too() {
    // A span is the other half of the sink, and the half a formatter-only
    // redaction misses: its fields are recorded when the span is created, by the
    // *field* formatter, and then written verbatim into every event inside it.
    // So the canary here lives on the span, not on the event, and the event is a
    // failure — the path where an operator is most likely to widen the context
    // "just this once".
    let captured = Captured::default();
    let subscriber = logging::subscriber(captured.clone());

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!(
            "snapshot",
            realm_id = "0193f000-0000-7000-8000-0000000000a1",
            authorization = "Bearer 0123456789abcdef0123456789abcdef",
            token = "sk-0123456789abcdef0123456789",
            credential_path = "/home/operator/.kontor/credentials.json",
            payload = r#"{"messages":[{"role":"user","content":"a transcript"}]}"#,
            detail = "Bearer 0123456789abcdef0123456789abcdef",
        );
        let entered = span.enter();
        tracing::error!(
            category = "verification",
            "the snapshot could not be verified"
        );
        drop(entered);
    });

    let written = String::from_utf8(
        captured
            .0
            .lock()
            .expect("the buffer is not poisoned")
            .clone(),
    )
    .expect("the log is UTF-8");

    for canary in [
        "0123456789abcdef0123456789abcdef",
        "sk-0123456789abcdef0123456789",
        "credentials.json",
        "a transcript",
        "authorization",
        "credential_path",
    ] {
        assert!(
            !written.contains(canary),
            "a span field carried `{canary}` to the sink:\n{written}"
        );
    }
    // The line is still useful: the span's allowed field survived, the value that
    // looked like a credential under an allowed name became the marker, and the
    // event itself was written.
    assert!(
        written.contains("realm_id=0193f000-0000-7000-8000-0000000000a1"),
        "an allowed span field must still be logged:\n{written}"
    );
    assert!(
        written.contains(&format!("detail={}", logging::REDACTED)),
        "a span field that looks like a credential is the marker, not the value:\n{written}"
    );
    assert!(written.contains("the snapshot could not be verified"));
    assert!(written.contains("category=verification"));
}

#[test]
fn a_failed_recovery_command_says_what_failed_without_saying_where_it_looked() {
    let home = TempDir::new().expect("a temporary directory");
    let missing = home.path().join("nowhere.db");
    let refused = recovery::restore(home.path(), &missing, Timestamp::now())
        .expect_err("a snapshot that is not there cannot be restored");

    // The category is stable vocabulary and the rendering carries no stored
    // value; both are what a structured log line is allowed to say.
    assert_eq!(refused.category(), "io");
    let rendered = refused.to_string();
    assert!(!rendered.contains("Bearer"));
    assert!(
        !home.path().join(DATABASE_FILE).exists(),
        "a refused restore creates no database"
    );
}
