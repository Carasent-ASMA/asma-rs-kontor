//! The CLI's answer shapes and exit classes, driven through the real command path
//! against a recording fake Realm (KON-MVP-16).
//!
//! Nothing here binds a socket, starts the daemon or spawns a child process
//! (TST-001). The path exercised is the one the binary takes after it has connected:
//! parse a real command line, gate it, plan it, dispatch it to a fake that records
//! what it received, and render the answer. Only `connect` is left out, and it is the
//! part that has nothing to do with the contract.
//!
//! The mutants this suite exists to kill:
//!
//! * a refusal that exits 0, which would make `set -e` useless against this control
//!   plane;
//! * a daemon code translated into the CLI's own vocabulary, losing the revision or
//!   the retained window the caller is owed;
//! * a mutation dispatched with no idempotency key, so a retry becomes a second
//!   effect;
//! * a caller's key regenerated, so a deliberate replay records a new command;
//! * a dry run that dispatches the write it describes;
//! * `--authority observer` on a write reaching the daemon at all;
//! * an arbitrary profile or gate key rejected because something enumerated the
//!   seeded ones;
//! * a run-lifecycle command sending the wrong kind.

use std::sync::Arc;

use clap::Parser;
use kontor_cli::args::Cli;
use kontor_cli::commands;
use kontor_cli::output::ExitClass;
use kontor_mcp::client::{CallerTier, Method, RealmClient, Request};
use kontor_mcp::fake::FakeTransport;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// One parsed command line, run against a fake Realm.
struct Run {
    class: ExitClass,
    fake: Arc<FakeTransport>,
}

impl Run {
    /// Parse `arguments`, run them at the operation's own authority, and record
    /// everything the fake was asked.
    async fn go(arguments: &[&str], scripted: &[(u16, Value)]) -> Self {
        let cli = Cli::try_parse_from(arguments).expect("the command line parses");
        let invocation = cli
            .invocation()
            .expect("the command names a catalogue operation");
        let tool = kontor_mcp::tools::find(invocation.operation)
            .expect("the catalogue serves the operation");
        let tier = invocation.authority.unwrap_or(tool.tier);

        let fake = Arc::new(FakeTransport::new(tier));
        for (status, body) in scripted {
            fake.push(*status, body.clone());
        }
        let client = RealmClient::new(Box::new(Arc::clone(&fake)));
        let class = commands::perform_with(&client, tier, &invocation).await;
        Self { class, fake }
    }

    /// Every request the fake was asked to make.
    fn dispatched(&self) -> Vec<Request> {
        self.fake.recorded()
    }

    /// The one write the fake was asked to make.
    fn write(&self) -> Request {
        self.dispatched()
            .into_iter()
            .find(|request| request.method == Method::Post)
            .expect("a write was dispatched")
    }
}

/// A canonical fixture identifier.
fn id(last: u8) -> String {
    format!("0192f0c0-0000-7000-8000-0000000000{last:02x}")
}

/// A refusal in the daemon's own envelope shape.
fn refusal(code: &str, extra: Value) -> Value {
    let mut body = json!({
        "realm_id": kontor_mcp::fake::FIXTURE_REALM,
        "code": code,
        "rule": "a static rule",
        "current_revision": Value::Null,
        "oldest_retained_cursor": Value::Null,
        "newest_cursor": Value::Null,
    });
    if let (Some(target), Some(source)) = (body.as_object_mut(), extra.as_object()) {
        for (name, value) in source {
            target.insert(name.clone(), value.clone());
        }
    }
    body
}

// ---------------------------------------------------------------------------
// Exit classes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_successful_read_exits_zero() {
    let run = Run::go(
        &["kontor", "run", "show", &id(2)],
        &[(
            200,
            json!({ "realm_id": kontor_mcp::fake::FIXTURE_REALM, "value": {} }),
        )],
    )
    .await;
    assert_eq!(run.class, ExitClass::Success);
    assert_eq!(run.class.code(), 0);
    assert_eq!(
        run.dispatched().len(),
        1,
        "a read is one request and nothing else"
    );
    assert_eq!(run.dispatched()[0].path, format!("/v1/runs/{}", id(2)));
}

#[tokio::test]
async fn each_daemon_code_maps_to_the_exit_class_its_caller_needs() {
    for (code, status, expected) in [
        ("unauthenticated", 401, ExitClass::Refused),
        ("forbidden", 403, ExitClass::Refused),
        ("realm_mismatch", 409, ExitClass::Conflict),
        ("revision_conflict", 409, ExitClass::Conflict),
        ("idempotency_conflict", 409, ExitClass::Conflict),
        ("stale_binding", 409, ExitClass::Conflict),
        ("timeline_refetch_required", 409, ExitClass::Conflict),
        ("resnapshot_required", 410, ExitClass::Conflict),
        ("reconciliation_pending", 503, ExitClass::Unavailable),
        ("unavailable", 503, ExitClass::Unavailable),
        ("not_found", 404, ExitClass::Absent),
        ("unsupported_capability", 422, ExitClass::Absent),
        ("invalid_request", 400, ExitClass::Local),
    ] {
        let run = Run::go(
            &["kontor", "run", "show", &id(2)],
            &[(status, refusal(code, json!({})))],
        )
        .await;
        assert_eq!(run.class, expected, "{code} must exit {}", expected.code());
        assert_ne!(
            run.class.code(),
            0,
            "{code} is a refusal and must not exit 0"
        );
    }
}

#[tokio::test]
async fn a_code_outside_the_closed_vocabulary_exits_one() {
    let run = Run::go(
        &["kontor", "run", "show", &id(2)],
        &[(418, refusal("teapot", json!({})))],
    )
    .await;
    assert_eq!(
        run.class,
        ExitClass::Unexpected,
        "a code this contract does not have means the thing answering is not a realm of this \
         generation"
    );
}

#[tokio::test]
async fn a_realm_that_answers_for_another_realm_is_a_conflict() {
    let run = Run::go(
        &["kontor", "health"],
        &[
            (
                200,
                json!({ "realm_id": kontor_mcp::fake::FIXTURE_REALM, "live": true }),
            ),
            (200, json!({ "realm_id": id(0xee), "live": true })),
        ],
    )
    .await;
    // The first answer established the expectation and succeeded, so this run's one
    // call is the one that matched. The mismatch case is asserted over two calls in
    // the MCP protocol suite, where one client makes both.
    assert_eq!(run.class, ExitClass::Success);

    let client = RealmClient::expecting(
        Box::new(Arc::new(FakeTransport::in_realm(
            CallerTier::Observer,
            &id(0xee),
        ))),
        kontor_mcp::fake::FIXTURE_REALM.to_owned(),
    );
    let cli = Cli::try_parse_from(["kontor", "health"]).expect("the command line parses");
    let invocation = cli.invocation().expect("health names an operation");
    let class = commands::perform_with(&client, CallerTier::Observer, &invocation).await;
    assert_eq!(
        class,
        ExitClass::Conflict,
        "a client that established one realm and is answered by another is a realm_mismatch, \
         which is a conflict and not a transport failure"
    );
}

// ---------------------------------------------------------------------------
// The refusal body is the daemon's own
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_revision_conflict_preserves_the_revision_the_caller_is_owed() {
    // The number in that body is the whole point of the refusal. A CLI that
    // reshaped the envelope and dropped it would leave a caller with nothing to
    // retry with.
    let body = refusal("revision_conflict", json!({ "current_revision": 12 }));
    let failure = kontor_mcp::Failure::Refused(kontor_mcp::client::Refusal {
        status: 409,
        code: "revision_conflict".to_owned(),
        body: body.clone(),
    });
    assert_eq!(
        failure.body(None),
        body,
        "the daemon's body is relayed byte for byte"
    );
    assert_eq!(
        failure.code(),
        "revision_conflict",
        "and its code untouched"
    );
    assert_eq!(ExitClass::of(failure.code()), ExitClass::Conflict);
}

#[tokio::test]
async fn a_resnapshot_preserves_the_retained_window() {
    let body = refusal(
        "resnapshot_required",
        json!({ "oldest_retained_cursor": 900, "newest_cursor": 1200 }),
    );
    let failure = kontor_mcp::Failure::Refused(kontor_mcp::client::Refusal {
        status: 410,
        code: "resnapshot_required".to_owned(),
        body: body.clone(),
    });
    assert_eq!(failure.body(None)["oldest_retained_cursor"], json!(900));
    assert_eq!(failure.body(None)["newest_cursor"], json!(1200));
}

// ---------------------------------------------------------------------------
// Mutations
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_four_run_lifecycle_commands_send_four_different_kinds() {
    for (verb, kind) in [
        ("launch", "launch_run"),
        ("cancel", "cancel_run"),
        ("park", "park_run"),
        ("abandon", "abandon_run"),
    ] {
        let run = Run::go(
            &[
                "kontor",
                "run",
                verb,
                "--project",
                &id(1),
                &id(2),
                "--expected-revision",
                "3",
            ],
            &[(200, json!({ "realm_id": kontor_mcp::fake::FIXTURE_REALM, "value": {}, "replayed": false }))],
        )
        .await;
        assert_eq!(run.class, ExitClass::Success);
        assert_eq!(
            run.write().path,
            format!("/v1/commands/{kind}"),
            "`run {verb}` must send {kind} and nothing else"
        );
    }
}

#[tokio::test]
async fn every_mutation_carries_an_idempotency_key_and_the_callers_own_is_kept() {
    let generated = Run::go(
        &[
            "kontor",
            "task",
            "resume",
            "--project",
            &id(1),
            &id(2),
            "--expected-revision",
            "1",
        ],
        &[(
            200,
            json!({ "realm_id": kontor_mcp::fake::FIXTURE_REALM, "value": {}, "replayed": false }),
        )],
    )
    .await;
    let key = generated
        .write()
        .idempotency_key
        .expect("a mutation carries a key");
    assert_eq!(
        uuid::Uuid::parse_str(&key)
            .expect("a generated key is a uuid")
            .get_version_num(),
        7
    );

    let named = Run::go(
        &[
            "kontor",
            "task",
            "resume",
            "--project",
            &id(1),
            &id(2),
            "--expected-revision",
            "1",
            "--idempotency-key",
            "resume-once",
        ],
        &[(
            200,
            json!({ "realm_id": kontor_mcp::fake::FIXTURE_REALM, "value": {}, "replayed": false }),
        )],
    )
    .await;
    assert_eq!(
        named.write().idempotency_key.as_deref(),
        Some("resume-once"),
        "a caller's key is what makes a retry a replay, so it is never regenerated"
    );
}

#[tokio::test]
async fn a_replay_exits_zero_and_the_body_says_it_replayed() {
    // An idempotent replay is a success: the command the caller wanted is durable,
    // and it happens to have been durable already.
    let run = Run::go(
        &[
            "kontor",
            "task",
            "resume",
            "--project",
            &id(1),
            &id(2),
            "--expected-revision",
            "1",
            "--idempotency-key",
            "resume-once",
        ],
        &[(
            200,
            json!({
                "realm_id": kontor_mcp::fake::FIXTURE_REALM,
                "value": { "receipt_id": id(9), "state": "intent_persisted" },
                "replayed": true
            }),
        )],
    )
    .await;
    assert_eq!(run.class, ExitClass::Success);
    assert_eq!(run.class.code(), 0);
}

#[tokio::test]
async fn a_dry_run_dispatches_no_write_and_still_exits_zero() {
    let run = Run::go(
        &[
            "kontor",
            "run",
            "launch",
            "--project",
            &id(1),
            &id(2),
            "--expected-revision",
            "3",
            "--dry-run",
        ],
        &[],
    )
    .await;
    assert_eq!(
        run.class,
        ExitClass::Success,
        "a valid dry run is a success, because what it promised to do is describe the request"
    );
    assert_eq!(
        run.fake.writes(),
        0,
        "a dry run must not dispatch the write it describes"
    );
    assert!(
        run.dispatched()
            .iter()
            .all(|request| request.method == Method::Get && request.path == "/v1/realm"),
        "a dry run reads the realm identity so its answer is realm-qualified, and nothing else"
    );
}

#[tokio::test]
async fn an_insisted_lower_authority_refuses_a_write_before_dispatch() {
    // `--authority observer` on a write is how a cautious operator proves a command
    // is read-only. It has to be refused locally, or the proof is worthless.
    let cli = Cli::try_parse_from([
        "kontor",
        "--authority",
        "observer",
        "run",
        "launch",
        "--project",
        &id(1),
        &id(2),
        "--expected-revision",
        "3",
    ])
    .expect("the command line parses");
    let invocation = cli.invocation().expect("a launch names an operation");
    assert_eq!(invocation.authority, Some(CallerTier::Observer));

    let fake = Arc::new(FakeTransport::new(CallerTier::Observer));
    let client = RealmClient::new(Box::new(Arc::clone(&fake)));
    let class = commands::perform_with(&client, CallerTier::Observer, &invocation).await;
    assert_eq!(class, ExitClass::Refused);
    assert_eq!(
        fake.dispatched(),
        0,
        "nothing was dispatched, not even to be refused by the daemon"
    );
}

// ---------------------------------------------------------------------------
// Open keys
// ---------------------------------------------------------------------------

#[tokio::test]
async fn an_arbitrary_profile_key_reaches_the_route_unchanged() {
    for key in ["delivery", "qa.sign-off", "team-b_flow", "0-bootstrap"] {
        let run = Run::go(
            &["kontor", "profile", "show", "--project", &id(1), key, "2"],
            &[(
                200,
                json!({ "realm_id": kontor_mcp::fake::FIXTURE_REALM, "value": {} }),
            )],
        )
        .await;
        assert_eq!(run.class, ExitClass::Success, "{key} is a legal open key");
        assert_eq!(
            run.dispatched()[0].path,
            format!("/v1/projects/{}/profiles/{key}/2", id(1)),
            "the deployment's own key reaches the route unchanged"
        );
    }
}

#[tokio::test]
async fn an_arbitrary_gate_key_reaches_the_intent_unchanged() {
    for gate in ["review", "qa.sign-off", "security_check", "gate-7"] {
        let run = Run::go(
            &[
                "kontor",
                "gate",
                "verdict",
                "--project",
                &id(1),
                "--task",
                &id(2),
                "--gate",
                gate,
                "--verdict",
                "passed",
                "--expected-revision",
                "1",
            ],
            &[(200, json!({ "realm_id": kontor_mcp::fake::FIXTURE_REALM, "value": {}, "replayed": false }))],
        )
        .await;
        assert_eq!(run.class, ExitClass::Success, "{gate} is a legal open key");
        let body = run.write().body.expect("a command body");
        assert_eq!(body["intent"]["gate"], json!(gate));
    }
}

#[tokio::test]
async fn a_key_outside_the_open_key_rule_is_refused_locally() {
    // Arbitrary *keys*, not arbitrary strings: the domain's lexical rule still
    // applies, and it is applied here rather than by the daemon.
    for key in ["Delivery", "has space", "trailing/slash", ""] {
        let cli = Cli::try_parse_from(["kontor", "profile", "show", "--project", &id(1), key, "2"])
            .expect("the command line parses");
        let invocation = cli.invocation().expect("a profile read names an operation");
        let fake = Arc::new(FakeTransport::new(CallerTier::Observer));
        let client = RealmClient::new(Box::new(Arc::clone(&fake)));
        let class = commands::perform_with(&client, CallerTier::Observer, &invocation).await;
        assert_eq!(
            class,
            ExitClass::Local,
            "`{key}` is not an open key and is refused as a caller error"
        );
        assert_eq!(fake.dispatched(), 0, "and never becomes a request");
    }
}

// ---------------------------------------------------------------------------
// Stream reads
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_stream_read_is_bounded_and_asks_for_the_route_it_named() {
    let run = Run::go(
        &["kontor", "events", "--after", "6", "--max-frames", "2"],
        &[(
            200,
            json!({
                "realm_id": kontor_mcp::fake::FIXTURE_REALM,
                "frames": [
                    kontor_mcp::fake::frame("control", "7", json!({ "cursor": 7 })),
                    kontor_mcp::fake::frame("control", "8", json!({ "cursor": 8 })),
                    kontor_mcp::fake::frame("control", "9", json!({ "cursor": 9 })),
                ]
            }),
        )],
    )
    .await;
    assert_eq!(run.class, ExitClass::Success);
    let request = &run.dispatched()[0];
    assert_eq!(request.path, "/v1/events");
    assert_eq!(
        request.query,
        vec![("after".to_owned(), "6".to_owned())],
        "a control-plane cursor is carried as the integer this realm allocated"
    );
}

#[tokio::test]
async fn a_session_stream_requires_the_anchor_a_timeline_read_returned() {
    // Without one there is nothing for delivery to be strictly after, so the
    // operand is required rather than defaulted.
    assert!(
        Cli::try_parse_from(["kontor", "session", "stream", &id(2)]).is_err(),
        "a live read without an anchor is a command-line error"
    );
    let run = Run::go(
        &["kontor", "session", "stream", &id(2), "--after", "3:118"],
        &[(
            200,
            json!({ "realm_id": kontor_mcp::fake::FIXTURE_REALM, "frames": [] }),
        )],
    )
    .await;
    assert_eq!(run.class, ExitClass::Success);
    assert_eq!(
        run.dispatched()[0].query,
        vec![("after".to_owned(), "3:118".to_owned())],
        "a content anchor is relayed as the runtime's own text"
    );
}

// ---------------------------------------------------------------------------
// What the CLI never does
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_request_the_cli_makes_addresses_the_versioned_contract() {
    // Stated over the whole surface: a route outside `/v1` would mean this CLI had
    // learned about something other than the daemon's contract.
    let fake = Arc::new(FakeTransport::new(CallerTier::Admin));
    let client = RealmClient::new(Box::new(Arc::clone(&fake)));
    for tool in kontor_mcp::tools::catalogue() {
        let mut operands = serde_json::Map::new();
        let identifier = id(3);
        for property in tool.properties {
            if !property.required {
                continue;
            }
            let value = match property.kind {
                kontor_mcp::tools::PropertyKind::Integer => json!(1),
                kontor_mcp::tools::PropertyKind::Boolean => json!(false),
                kontor_mcp::tools::PropertyKind::TextArray => json!([]),
                kontor_mcp::tools::PropertyKind::Choice(values) => json!(values[0]),
                kontor_mcp::tools::PropertyKind::Text => json!(match property.name {
                    "profile_key" => "delivery",
                    "gate" => "review",
                    "body" => "go",
                    "permission_request_id" => "perm-1",
                    "after" => "1:1",
                    _ => &identifier,
                }),
            };
            operands.insert(property.name.to_owned(), value);
        }
        let invocation = kontor_cli::args::Invocation {
            operation: tool.name,
            operands,
            authority: None,
        };
        commands::perform_with(&client, CallerTier::Admin, &invocation).await;
    }
    for request in fake.recorded() {
        assert!(
            request.path.starts_with("/v1/"),
            "{} is outside the versioned contract",
            request.path
        );
    }
}
