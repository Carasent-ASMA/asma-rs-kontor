//! The generated contract artefacts are the ones this crate serves.
//!
//! The operator console does not hand-write wire interfaces. It generates its
//! TypeScript types from `contract/openapi.json`, and its transcript renderer is
//! held to `contract/session-kinds.json` (KON-MVP-17). That is only safe while
//! both files *are* what this crate serves, so this suite pins them.
//!
//! A DTO that gains, loses or renames a field fails here until the document is
//! regenerated and the console's types are regenerated with it — which is what
//! stops a console from compiling against a contract the realm stopped serving.
//!
//! Regenerate with:
//!
//! ```text
//! KONTOR_UPDATE_CONTRACT=1 cargo test -p kontor-api --test openapi_contract
//! pnpm --filter kontor-console generate:api
//! ```

use std::path::PathBuf;

use kontor_runtime::timeline::SessionEventKind;

/// The environment variable that turns these assertions into a regeneration.
const UPDATE: &str = "KONTOR_UPDATE_CONTRACT";

/// Where the generated contract document is committed.
fn contract_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("contract")
        .join(file)
}

/// Where the console's generated test fixtures are committed.
///
/// The session vocabulary is a fixture of the console's own suite, so it is
/// generated straight into it: a fixture the console imports directly needs no
/// path out of its package and no Node type definitions to read.
fn console_fixture_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/console/src/test")
        .join(file)
}

/// Assert one generated artefact is unchanged, or regenerate it.
fn pin(path: PathBuf, rendered: &str) {
    if std::env::var_os(UPDATE).is_some() {
        std::fs::write(&path, rendered).expect("the contract artefact can be written");
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "{} is missing; regenerate it with {UPDATE}=1",
            path.display()
        )
    });
    assert_eq!(
        committed,
        rendered,
        "{} no longer matches what this crate serves; regenerate it with {UPDATE}=1 \
         and regenerate the console's types with it",
        path.display()
    );
}

/// The document as it is written to disk: pretty, newline-terminated, stable.
fn rendered_document() -> String {
    let document = kontor_api::openapi::document();
    let mut text =
        serde_json::to_string_pretty(&document).expect("the contract document serializes");
    text.push('\n');
    text
}

/// Every kind of thing that can happen inside a session.
///
/// The match is exhaustive on purpose: a new variant fails to compile here
/// rather than quietly going missing from the console's transcript, which is
/// exactly the failure a subscription that filters is not allowed to have.
fn every_session_kind() -> Vec<SessionEventKind> {
    let all = [
        SessionEventKind::Message,
        SessionEventKind::ToolCall,
        SessionEventKind::PermissionRequest,
        SessionEventKind::PermissionResolved,
        SessionEventKind::StateChange,
        SessionEventKind::Log,
    ];
    for kind in all {
        match kind {
            SessionEventKind::Message
            | SessionEventKind::ToolCall
            | SessionEventKind::PermissionRequest
            | SessionEventKind::PermissionResolved
            | SessionEventKind::StateChange
            | SessionEventKind::Log => {}
        }
    }
    all.to_vec()
}

#[test]
fn the_committed_contract_document_is_the_one_this_crate_serves() {
    pin(contract_path("openapi.json"), &rendered_document());
}

#[test]
fn the_committed_session_vocabulary_is_the_one_this_crate_subscribes_to() {
    let mut text = serde_json::to_string_pretty(&every_session_kind())
        .expect("the session vocabulary serializes");
    text.push('\n');
    pin(console_fixture_path("session-kinds.json"), &text);
}

#[test]
fn the_contract_document_names_every_route_the_router_exposes() {
    // The console's client is written against these paths. utoipa derives them
    // from the handlers, so a route that loses its `#[utoipa::path]` — and would
    // therefore vanish from the console's generated types without any compile
    // error — is caught here instead of at runtime.
    let document = kontor_api::openapi::document();
    let paths: Vec<&str> = document.paths.paths.keys().map(String::as_str).collect();
    for expected in [
        "/v1/health",
        "/v1/realm",
        "/v1/runs/{agent_run_id}",
        "/v1/projects/{project_id}/tasks/{task_id}",
        "/v1/events",
        "/v1/sessions/{agent_run_id}/timeline",
        "/v1/sessions/{agent_run_id}/stream",
        "/v1/sessions/{agent_run_id}/messages",
        "/v1/sessions/{agent_run_id}/permissions/{request_id}",
    ] {
        assert!(
            paths.contains(&expected),
            "the contract document no longer describes {expected}, which the console calls"
        );
    }
}
