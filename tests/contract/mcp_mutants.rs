//! The forbidden-schema mutant suite: nine families of change that must not be
//! possible to make quietly.
//!
//! # How to read this file
//!
//! Each test states the property one mutant family would break. The mutant itself
//! is the edit somebody might make — adding a `database_path` argument, letting a
//! tool name a runtime endpoint, following a redirect, retrying a 5xx, caching a
//! receipt — and the test is what turns that edit red.
//!
//! The audits are table-driven over the *whole* registry rather than over a list of
//! tools somebody remembered, so a tool added later is audited by construction. A
//! mutant that adds a forbidden property to a new tool fails here without anyone
//! extending this file.

use std::collections::BTreeSet;

use kontor_mcp::{ArgType, Place, REGISTRY, ToolSpec};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Every argument name in the vocabulary, with the tool that declares it.
fn every_argument() -> Vec<(&'static str, &'static str)> {
    REGISTRY
        .iter()
        .flat_map(|tool| tool.args.iter().map(move |arg| (tool.name, arg.name)))
        .collect()
}

/// Every identifier a caller can see: tool names, argument names, enum values.
fn every_identifier() -> Vec<(&'static str, String)> {
    let mut names: Vec<(&'static str, String)> = REGISTRY
        .iter()
        .map(|tool| (tool.name, tool.name.to_owned()))
        .collect();
    for tool in REGISTRY {
        for arg in tool.args {
            names.push((tool.name, arg.name.to_owned()));
            if let ArgType::Enum(allowed) = arg.ty {
                names.extend(allowed.iter().map(|value| (tool.name, (*value).to_owned())));
            }
        }
    }
    names
}

/// Refuse every identifier that contains one of `forbidden`, except the exact
/// spellings in `allowed`.
fn audit(family: &str, forbidden: &[&str], allowed: &[&str]) {
    let allowed: BTreeSet<&str> = allowed.iter().copied().collect();
    let mut found = Vec::new();
    for (tool, name) in every_identifier() {
        if allowed.contains(name.as_str()) {
            continue;
        }
        let lowered = name.to_lowercase();
        for needle in forbidden {
            if lowered.contains(needle) {
                found.push(format!("{tool}.{name} contains `{needle}`"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{family}: the tool vocabulary names something it must not reach: {found:#?}"
    );
}

// --- Mutant 1: direct persistence -----------------------------------------------

#[test]
fn no_tool_names_a_store_a_database_or_a_migration() {
    audit(
        "direct persistence",
        &[
            "sqlite",
            "database",
            "db_path",
            "migration",
            "repository",
            "rusqlite",
            "vacuum",
        ],
        &[],
    );
    // `sql` and `store` are checked separately because they are substrings of
    // ordinary words; the rule is a whole-segment match rather than a blind
    // `contains`, so a future `restore_point` is not a false positive while a
    // `sql_filter` still fails.
    for (tool, name) in every_argument() {
        for segment in name.split('_') {
            assert!(
                !matches!(segment, "sql" | "store" | "db"),
                "direct persistence: {tool}.{name} names persistence directly"
            );
        }
    }
}

// --- Mutant 2: direct runtime or Paseo ------------------------------------------

#[test]
fn no_tool_names_a_runtime_endpoint_or_a_provider() {
    audit(
        "direct runtime",
        &[
            "paseo",
            "endpoint",
            "workspace",
            "codex_session",
            "native_session",
            "pid",
            "spawn",
        ],
        &[],
    );
    // Runtime *operations* stay daemon routes. The only runtime words a tool may
    // carry are the family key it selects and the capability list it reads, and
    // neither creates, kills or archives anything.
    let runtime_arguments: BTreeSet<&str> = every_argument()
        .into_iter()
        .filter(|(_, name)| name.contains("runtime"))
        .map(|(_, name)| name)
        .collect();
    assert_eq!(
        runtime_arguments,
        BTreeSet::from(["runtime_family"]),
        "a tool grew a runtime argument beyond the family it selects"
    );
    for verb in ["create", "kill", "archive", "attach", "detach", "reload"] {
        assert!(
            !REGISTRY.iter().any(|tool| tool.name.contains(verb)),
            "direct runtime: a tool named `{verb}` would be driving a runtime directly"
        );
    }
}

// --- Mutant 3: direct external tracker ------------------------------------------

#[test]
fn no_tool_names_a_tracker_field_a_status_or_an_assignee() {
    audit(
        "direct tracker",
        &[
            "jira",
            "agentsroom",
            "assignee",
            "transition",
            "comment",
            "issue_key",
            "issue_status",
            "external_status",
            "connector_url",
        ],
        &[
            // Kontor's *own* lifecycle transition, which moves internal task and
            // epic state through the domain's transition table. The needle stays
            // broad so a `transition_id` or an external status transition still
            // fails; this one exact identifier is the internal concept it collides
            // with.
            "kontor_lifecycle_transition",
            // The inbound comment mirror's own two tool names. A blunt `comment`
            // ban would forbid naming the mirror at all while proving nothing about
            // the thing that matters — whether prose can travel — so the allowance
            // is a closed list of exact names and the real property is asserted
            // directly in `the_comment_mirror_can_carry_no_prose_and_sends_nothing`.
            "kontor_ticket_comments_pull",
            "kontor_ticket_comments_list",
        ],
    );
    // The ticket tools accept the daemon's closed reconcile DTOs and nothing else:
    // a plan takes only the task it is computed for, and an apply takes only the
    // hash of the plan the daemon produced.
    let plan = ToolSpec::find("kontor_ticket_reconcile_plan").expect("the plan tool");
    assert_eq!(
        plan.args_in(Place::Body).count(),
        0,
        "a reconcile plan is computed by the daemon, not described by the caller"
    );
    let apply = ToolSpec::find("kontor_ticket_reconcile_apply").expect("the apply tool");
    let body: Vec<_> = apply.args_in(Place::Body).map(|arg| arg.name).collect();
    assert_eq!(
        body,
        vec!["projection_hash"],
        "applying a reconciliation must name a plan, never a status, assignee or comment"
    );
}

#[test]
fn the_comment_mirror_can_carry_no_prose_and_sends_nothing() {
    // The narrow claim the broad needle cannot make: the two mirror tools take no
    // body at all, so there is no argument through which a comment's text could be
    // sent, mirrored back, or pushed outbound. `pull` carries only its scope and
    // the caller's key; `list` carries only its scope.
    for name in ["kontor_ticket_comments_pull", "kontor_ticket_comments_list"] {
        let tool = ToolSpec::find(name).expect("a declared mirror tool");
        assert_eq!(
            tool.args_in(Place::Body).count(),
            0,
            "{name} grew a body property; a comment mirror that accepts one is a push"
        );
    }

    // And nowhere in the vocabulary may a *body* property be prose-shaped. This is
    // the outbound-comment ban stated where it belongs: on what can be sent, not on
    // what can be named.
    for tool in REGISTRY {
        for arg in tool.args_in(Place::Body) {
            for prose in ["comment", "prose", "rendered", "markdown", "html"] {
                assert!(
                    !arg.name.to_lowercase().contains(prose),
                    "{}.{} could carry ticket prose outbound",
                    tool.name,
                    arg.name
                );
            }
        }
    }
}

// --- Mutant 4: credential leakage and arbitrary network -------------------------

#[test]
fn no_tool_can_name_a_credential_an_address_or_a_proxy() {
    audit(
        "credential and network",
        &[
            "token",
            "secret",
            "password",
            "bearer",
            "api_key",
            "authorization",
            "base_url",
            "host",
            "port",
            "proxy",
            "url",
            "redirect",
        ],
        &[
            // The daemon's own DTO field: an approved credential *reference*,
            // which is a name for a secret this process never sees and never
            // resolves.
            "credential_alias",
            // The bounded *execution* authorization an arm grants and a disarm
            // revokes — a domain object, not an HTTP credential. The needle stays
            // broad so an `authorization` header field would still fail.
            "authorization_id",
        ],
    );
}

#[test]
fn a_non_loopback_endpoint_is_refused_before_a_client_exists() {
    for base in [
        "http://evil.example",
        "http://10.0.0.4:7717",
        "http://0.0.0.0:7717",
        "https://kontor.example.com",
        "http://127.0.0.1.evil.com",
    ] {
        assert!(
            kontor_mcp::Endpoint::parse(base).is_err(),
            "{base} must not be addressable"
        );
    }
}

#[tokio::test]
async fn a_redirect_off_loopback_is_not_followed_and_carries_no_credential() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "http://evil.example/v1/realm")
                .set_body_json(serde_json::json!({ "code": "not_found" })),
        )
        .mount(&server)
        .await;

    let root = tempfile::tempdir().expect("a state root");
    std::fs::write(
        root.path().join("credentials.json"),
        serde_json::json!({
            "schema_version": 1,
            "observer": "observer-secret",
            "operator": "operator-secret",
            "admin": "admin-secret",
        })
        .to_string(),
    )
    .expect("credentials");

    let dispatcher = kontor_mcp::connect(
        root.path(),
        Some(&server.uri()),
        kontor_mcp::CallerTier::Observer,
    )
    .expect("a loopback dispatcher");
    let envelope = dispatcher
        .call("kontor_realm_get", &serde_json::json!({}))
        .await
        .expect("the redirect is an answer, not a transport failure");

    assert_eq!(
        envelope.status, 302,
        "the redirect is relayed, not followed: following it would carry the bearer \
         to whatever the location named"
    );
    assert_eq!(
        server
            .received_requests()
            .await
            .expect("recorded requests")
            .len(),
        1,
        "exactly one request, and no second one to the redirect target"
    );
}

#[tokio::test]
async fn a_hostile_path_argument_cannot_move_the_route() {
    // The one path argument that is deliberately free text is a runtime's own
    // opaque permission-request id. If a value could carry a separator it would
    // address a route the tool does not declare — reaching a *different* operation
    // with the seat's credential attached.
    //
    // Two outcomes are safe and both are accepted: the value is refused before a
    // request exists, or it is encoded into exactly one segment. What must never
    // happen is a dispatched request whose route the argument changed.
    const RUN: &str = "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70";
    let prefix = format!("/v1/sessions/{RUN}/permissions/");

    for hostile in [
        "../../../v1/projects",
        "a/b",
        "x?override=1",
        "y#fragment",
        "..",
        ".",
        "%2e%2e/escaped",
        "with space",
    ] {
        let recorder = std::sync::Arc::new(kontor_mcp::fake::RecordingTransport::new(
            kontor_mcp::CallerTier::Operator,
        ));
        let dispatcher = kontor_mcp::Dispatcher::new(Box::new(std::sync::Arc::clone(&recorder)));
        let outcome = dispatcher
            .call(
                "kontor_session_permission_respond",
                &serde_json::json!({
                    "agent_run_id": RUN,
                    "request_id": hostile,
                    "idempotency_key": "k-1",
                    "decision": "deny",
                }),
            )
            .await;

        if outcome.is_err() {
            assert_eq!(
                recorder.count(),
                0,
                "{hostile} was refused, so nothing may have reached the wire"
            );
            continue;
        }

        assert_eq!(recorder.count(), 1, "one request per invocation");
        let request = recorder.only_request();
        assert!(
            request.path.starts_with(&prefix),
            "{hostile} moved the route to {}",
            request.path
        );
        let segment = &request.path[prefix.len()..];
        assert!(
            !segment.is_empty(),
            "{hostile} produced an empty segment, which addresses another route"
        );
        for forbidden in ['/', '?', '#'] {
            assert!(
                !segment.contains(forbidden),
                "{hostile} smuggled {forbidden:?} into the route: {segment}"
            );
        }
        assert_eq!(
            request.path.matches("/v1/").count(),
            1,
            "{hostile} produced a path naming /v1/ twice: {}",
            request.path
        );
    }
}

#[test]
fn no_secret_can_appear_in_a_tool_schema() {
    for tool in REGISTRY {
        let rendered = tool.input_schema().to_string().to_lowercase();
        for needle in ["bearer ", "secret-value", "authorization:"] {
            assert!(
                !rendered.contains(needle),
                "{} advertises something credential-shaped",
                tool.name
            );
        }
    }
}

// --- Mutant 9: schema bypass -----------------------------------------------------

#[tokio::test]
async fn every_schema_bypass_is_refused_with_nothing_dispatched() {
    let recorder = std::sync::Arc::new(kontor_mcp::fake::RecordingTransport::new(
        kontor_mcp::CallerTier::Admin,
    ));
    let dispatcher = kontor_mcp::Dispatcher::new(Box::new(std::sync::Arc::clone(&recorder)));
    const UUID: &str = "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70";

    let bypasses: Vec<(&str, serde_json::Value, &str)> = vec![
        (
            "kontor_run_get",
            serde_json::json!({ "agent_run_id": UUID, "sqlite_path": "/tmp/x.db" }),
            "an unknown property",
        ),
        (
            "kontor_run_get",
            serde_json::json!({ "agent_run_id": "not-a-uuid" }),
            "a malformed identifier",
        ),
        (
            "kontor_task_get",
            serde_json::json!({ "project_id": UUID }),
            "a missing path argument",
        ),
        (
            "kontor_project_ensure",
            serde_json::json!({ "name": "Pilot", "root_path": "/tmp/pilot" }),
            "a write with no idempotency key",
        ),
        (
            "kontor_lifecycle_transition",
            serde_json::json!({
                "project_id": UUID, "epic_id": UUID, "idempotency_key": "k",
                "action": "delete_the_realm", "expected_revision": 1, "reason": "no",
            }),
            "an action outside the closed set",
        ),
        (
            "kontor_runtime_settle",
            serde_json::json!({
                "project_id": UUID, "agent_run_id": UUID,
                "idempotency_key": "k", "outcome": "succeeded",
            }),
            "a caller-supplied settlement outcome",
        ),
        (
            "kontor_runtime_settle",
            serde_json::json!({
                "project_id": UUID, "agent_run_id": UUID,
                "idempotency_key": "k", "evidence": ["fabricated"],
            }),
            "caller-supplied settlement evidence",
        ),
        (
            "kontor_ticket_reconcile_apply",
            serde_json::json!({
                "project_id": UUID, "task_id": UUID, "idempotency_key": "k",
                "projection_hash": "abc", "assignee": "someone",
            }),
            "a smuggled assignee",
        ),
        (
            "kontor_session_permission_respond",
            serde_json::json!({
                "agent_run_id": UUID, "request_id": "r-1",
                "idempotency_key": "k", "decision": "maybe",
            }),
            "a decision outside the closed set",
        ),
        (
            "kontor_epic_apply",
            serde_json::json!({
                "project_id": UUID, "idempotency_key": "k", "expected_revision": 0,
                "name": "E", "work_profile_category": "coding",
                "runtime_family": "codex", "tasks": [],
            }),
            "a revision below the domain minimum",
        ),
    ];

    for (tool, arguments, why) in bypasses {
        dispatcher
            .call(tool, &arguments)
            .await
            .err()
            .unwrap_or_else(|| panic!("{why} must be refused by {tool}"));
    }
    assert_eq!(
        recorder.count(),
        0,
        "a call that failed its schema must leave nothing on the wire"
    );
}

// --- The dependency audit ---------------------------------------------------------

#[test]
fn kontor_mcp_has_no_dependency_path_to_a_forbidden_crate() {
    // The brief's own check, run as the brief states it. `cargo tree` resolves the
    // real graph, which is the only thing that can answer this — a manifest reads
    // one level and would miss a transitive edge.
    let output = std::process::Command::new(env!("CARGO"))
        .args(["tree", "-p", "kontor-mcp"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree runs");
    assert!(
        output.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8_lossy(&output.stdout);

    for forbidden in [
        "kontor-store",
        "kontor-scheduler",
        "kontor-workflows",
        "kontor-profiles",
        "kontor-teams",
        "kontor-integrations-asma",
        "kontor-runtime",
        "kontor-api",
        "kontor-daemon",
        // The store's driver. Reaching it would mean reaching SQLite whatever the
        // crate in between was called.
        "rusqlite",
    ] {
        assert!(
            !tree.contains(forbidden),
            "kontor-mcp reaches {forbidden}:\n{tree}"
        );
    }
    // And the one Kontor crate it may link is present, so this test cannot pass by
    // resolving nothing at all.
    assert!(
        tree.contains("kontor-core"),
        "the audit resolved no Kontor dependency at all, so it proved nothing:\n{tree}"
    );
    assert!(
        tree.contains("kontor-mcp"),
        "the audit named the wrong crate"
    );
}
