//! The MCP wire, driven end to end over an in-memory pair (KON-MVP-16).
//!
//! No socket, no child process and no daemon binary (TST-001). The server runs the
//! same `rmcp` handshake and the same dispatch the `kontor mcp` binary runs; the
//! only difference is that the two pipes are a `tokio::io::duplex` pair instead of
//! this process's standard streams, and the Realm behind it is a recording fake.
//!
//! The transcripts are asserted as whole JSON values rather than as substrings, so a
//! renamed field or a moved envelope shows up as a failure rather than as a test that
//! still passes for the wrong reason.
//!
//! The mutants this suite exists to kill:
//!
//! * a tool result that loses the daemon's own document, or renames a field on the
//!   way through;
//! * a mutation whose receipt lands in `data`, or a read whose document lands in
//!   `receipt`;
//! * a refusal reported as a JSON-RPC protocol error, which would strip the code and
//!   the rule a caller branches on;
//! * an envelope that does not name its Realm;
//! * `tools/list` advertising a tool the configured authority cannot perform;
//! * the credential file spelling drifting from the one the daemon writes.

use std::sync::Arc;
use std::time::Duration;

use kontor_mcp::client::{CallerTier, RealmClient};
use kontor_mcp::fake::{FakeTransport, frame};
use kontor_mcp::server::KontorMcp;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// One running server, and the client half of the pipe it speaks over.
struct Wire {
    stream: BufReader<DuplexStream>,
    next_id: i64,
    fake: Arc<FakeTransport>,
}

impl Wire {
    /// Start a server at `tier` over an in-memory pair and complete the handshake.
    async fn open(tier: CallerTier) -> Self {
        let fake = Arc::new(FakeTransport::new(tier));
        let client = RealmClient::new(Box::new(Arc::clone(&fake)));
        let (mine, theirs) = tokio::io::duplex(256 * 1024);
        tokio::spawn(async move {
            // The service ends when the client half is dropped, which is what the
            // end of a test does.
            let _ = kontor_mcp::server::serve(KontorMcp::new(client), theirs).await;
        });
        let mut wire = Self {
            stream: BufReader::new(mine),
            next_id: 0,
            fake,
        };
        wire.handshake().await;
        wire
    }

    /// Complete `initialize` and send `notifications/initialized`.
    async fn handshake(&mut self) -> Value {
        let initialized = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "kontor-tests", "version": "0" }
                }),
            )
            .await;
        self.notify("notifications/initialized", json!({})).await;
        initialized
    }

    /// Send one request and read its response.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))
        .await;
        loop {
            let message = self.receive().await;
            // Skip anything that is not this request's answer — a notification, or a
            // log the server chose to emit.
            if message.get("id") == Some(&json!(id)) {
                return message;
            }
        }
    }

    /// Send one notification, which has no answer.
    async fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await;
    }

    /// Write one JSON-RPC line.
    async fn send(&mut self, message: Value) {
        let line = format!("{message}\n");
        self.stream
            .get_mut()
            .write_all(line.as_bytes())
            .await
            .expect("the protocol pipe accepts a message");
        self.stream
            .get_mut()
            .flush()
            .await
            .expect("the protocol pipe flushes");
    }

    /// Read one JSON-RPC line.
    async fn receive(&mut self) -> Value {
        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(5), self.stream.read_line(&mut line))
            .await
            .expect("the server answers within five seconds")
            .expect("the protocol pipe is readable");
        assert!(read > 0, "the server closed the connection unexpectedly");
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("the server writes JSON-RPC lines: {error} in {line}"))
    }

    /// Call one tool and return its `result`.
    async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let response = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await;
        assert!(
            response.get("error").is_none(),
            "a tool call must answer with a result, not a protocol error: {response}"
        );
        response["result"].clone()
    }

    /// The single text block of one tool result, parsed as the JSON it carries.
    fn document(result: &Value) -> Value {
        let content = result["content"]
            .as_array()
            .unwrap_or_else(|| panic!("a tool result carries content: {result}"));
        assert_eq!(content.len(), 1, "one document per answer: {result}");
        let text = content[0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("the content block is text: {result}"));
        serde_json::from_str(text)
            .unwrap_or_else(|error| panic!("the document is JSON: {error} in {text}"))
    }
}

/// A canonical fixture identifier.
fn id(last: u8) -> String {
    format!("0192f0c0-0000-7000-8000-0000000000{last:02x}")
}

// ---------------------------------------------------------------------------
// Handshake and listing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_handshake_names_the_server_and_its_authority() {
    let mut wire = Wire::open(CallerTier::Operator).await;
    let initialized = wire.handshake().await;
    let result = &initialized["result"];
    assert_eq!(result["serverInfo"]["name"], json!("kontor"));
    assert!(
        result["capabilities"]["tools"].is_object(),
        "a server that serves tools declares the capability: {result}"
    );
    let instructions = result["instructions"]
        .as_str()
        .expect("the server instructs its caller");
    assert!(
        instructions.contains("operator"),
        "the configured authority is stated, so a caller is not left guessing why a tool is \
         missing: {instructions}"
    );
}

#[tokio::test]
async fn tools_list_advertises_exactly_what_the_authority_can_perform() {
    for tier in CallerTier::ALL.iter().copied() {
        let mut wire = Wire::open(tier).await;
        let listed = wire.request("tools/list", json!({})).await;
        let tools = listed["result"]["tools"]
            .as_array()
            .expect("a listing is an array")
            .clone();
        let names: Vec<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("a tool has a name"))
            .collect();

        let expected: Vec<&str> = kontor_mcp::tools::catalogue()
            .iter()
            .filter(|tool| tier.at_least(tool.tier))
            .map(|tool| tool.name)
            .collect();
        assert_eq!(
            names, expected,
            "a {tier} server must advertise exactly the tools it can perform, in name order"
        );
        for tool in &tools {
            assert_eq!(
                tool["inputSchema"]["additionalProperties"],
                json!(false),
                "every advertised schema is closed: {tool}"
            );
            assert!(
                tool["description"]
                    .as_str()
                    .is_some_and(|text| text.len() > 40),
                "every advertised tool explains itself: {tool}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_read_puts_the_daemons_own_document_in_data_and_nothing_in_receipt() {
    let mut wire = Wire::open(CallerTier::Observer).await;
    // The exact body `GET /v1/runs/{id}` answers with, abbreviated but not reshaped.
    wire.fake.push_ok(json!({
        "snapshot_cursor": 41,
        "value": {
            "agent_run_id": id(2),
            "projection": { "lifecycle": "running", "freshness": "fresh" },
            "revision": 7
        }
    }));

    let result = wire
        .call("run_show", json!({ "agent_run_id": id(2) }))
        .await;
    assert_ne!(
        result["isError"],
        json!(true),
        "a successful read is not an error: {result}"
    );
    let envelope = Wire::document(&result);
    assert_eq!(
        envelope,
        json!({
            "schema_version": 1,
            "realm_id": kontor_mcp::fake::FIXTURE_REALM,
            "command": "run_show",
            "data": {
                "realm_id": kontor_mcp::fake::FIXTURE_REALM,
                "snapshot_cursor": 41,
                "value": {
                    "agent_run_id": id(2),
                    "projection": { "lifecycle": "running", "freshness": "fresh" },
                    "revision": 7
                }
            },
            "receipt": Value::Null
        }),
        "a read nests the daemon's document under `data` unchanged, and records no receipt"
    );
}

#[tokio::test]
async fn a_stream_read_returns_its_frames_with_the_ids_a_caller_resumes_from() {
    let mut wire = Wire::open(CallerTier::Observer).await;
    wire.fake.push_ok(json!({
        "frames": [
            frame("control", "7", json!({ "cursor": 7, "agent_run_id": id(2) })),
            frame("control", "8", json!({ "cursor": 8, "agent_run_id": id(2) })),
        ]
    }));

    let result = wire.call("events_replay", json!({ "after": 6 })).await;
    let envelope = Wire::document(&result);
    let frames = envelope["data"]["frames"]
        .as_array()
        .expect("a stream read returns frames");
    assert_eq!(frames.len(), 2);
    assert_eq!(
        frames
            .iter()
            .map(|frame| frame["id"].clone())
            .collect::<Vec<_>>(),
        vec![json!("7"), json!("8")],
        "a frame id is relayed as text: it is a position this realm allocated, and adding it to a \
         content position would be mixing two cursor spaces"
    );
    assert_eq!(envelope["receipt"], Value::Null);
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_mutation_puts_the_whole_receipt_envelope_in_receipt_and_renames_nothing() {
    let mut wire = Wire::open(CallerTier::Operator).await;
    // The exact shape `POST /v1/commands/{kind}` answers with: a flattened
    // `ReceiptEnvelope<ReceiptDto>` plus `replayed`.
    wire.fake.push_ok(json!({
        "value": {
            "receipt_id": id(9),
            "project_id": id(1),
            "idempotency_key": "launch-once",
            "kind": "launch_run",
            "target": { "kind": "agent_run", "agent_run_id": id(2) },
            "target_revision": 3,
            "state": "intent_persisted",
            "attempts": 0,
            "created_at": "2026-08-12T09:00:00Z",
            "updated_at": "2026-08-12T09:00:00Z"
        },
        "replayed": false
    }));

    let result = wire
        .call(
            "run_launch",
            json!({
                "project_id": id(1),
                "agent_run_id": id(2),
                "expected_revision": 3,
                "idempotency_key": "launch-once"
            }),
        )
        .await;
    let envelope = Wire::document(&result);
    assert_eq!(
        envelope["data"],
        Value::Null,
        "a mutation's answer is a receipt, so `data` is empty"
    );
    assert_eq!(
        envelope["receipt"]["value"]["state"],
        json!("intent_persisted"),
        "the daemon's own field names survive: `state` is where kontor-api put it"
    );
    assert_eq!(
        envelope["receipt"]["replayed"],
        json!(false),
        "and `replayed` is not flattened away"
    );
    assert_eq!(envelope["receipt"]["value"]["receipt_id"], json!(id(9)));
    assert_eq!(
        envelope["realm_id"],
        json!(kontor_mcp::fake::FIXTURE_REALM),
        "every answer names its realm"
    );
}

#[tokio::test]
async fn a_replayed_mutation_returns_the_same_receipt_under_the_same_key() {
    let mut wire = Wire::open(CallerTier::Operator).await;
    let receipt = json!({
        "value": {
            "receipt_id": id(9),
            "idempotency_key": "resume-once",
            "kind": "resume_task",
            "state": "intent_persisted",
            "attempts": 0
        },
        "replayed": false
    });
    let mut replayed = receipt.clone();
    replayed["replayed"] = json!(true);
    wire.fake.push_ok(receipt);
    wire.fake.push_ok(replayed);

    let arguments = json!({
        "project_id": id(1),
        "task_id": id(2),
        "expected_revision": 1,
        "idempotency_key": "resume-once"
    });
    let first = Wire::document(&wire.call("task_resume", arguments.clone()).await);
    let second = Wire::document(&wire.call("task_resume", arguments).await);

    assert_eq!(
        first["receipt"]["value"]["receipt_id"], second["receipt"]["value"]["receipt_id"],
        "a replay returns the receipt that was already durable"
    );
    assert_eq!(
        second["receipt"]["replayed"],
        json!(true),
        "and says that is what happened"
    );
    // The caller's key is what makes the two the same command, so it must have
    // reached the daemon unchanged both times.
    let keys: Vec<Option<String>> = wire
        .fake
        .recorded()
        .iter()
        .map(|request| request.idempotency_key.clone())
        .collect();
    assert_eq!(
        keys,
        vec![
            Some("resume-once".to_owned()),
            Some("resume-once".to_owned())
        ],
        "the key a caller chose is never regenerated"
    );
}

// ---------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_daemon_refusal_is_relayed_as_a_tool_error_carrying_the_code_and_the_rule() {
    let mut wire = Wire::open(CallerTier::Operator).await;
    wire.fake.push(
        409,
        json!({
            "realm_id": kontor_mcp::fake::FIXTURE_REALM,
            "code": "revision_conflict",
            "rule": "the target aggregate moved since the caller read it",
            "current_revision": 12,
            "oldest_retained_cursor": Value::Null,
            "newest_cursor": Value::Null
        }),
    );

    let result = wire
        .call(
            "task_resume",
            json!({ "project_id": id(1), "task_id": id(2), "expected_revision": 4 }),
        )
        .await;
    assert_eq!(
        result["isError"],
        json!(true),
        "a refusal is a tool error, so a caller sees it failed: {result}"
    );
    let body = Wire::document(&result);
    assert_eq!(
        body,
        json!({
            "realm_id": kontor_mcp::fake::FIXTURE_REALM,
            "code": "revision_conflict",
            "rule": "the target aggregate moved since the caller read it",
            "current_revision": 12,
            "oldest_retained_cursor": Value::Null,
            "newest_cursor": Value::Null
        }),
        "the daemon's body is relayed byte for byte, including the revision the caller is owed"
    );
}

#[tokio::test]
async fn an_unsupported_runtime_capability_is_relayed_untouched() {
    // This refusal belongs to the runtime, through the daemon. A client that
    // translated it into something of its own would be claiming to know what a
    // runtime can do.
    let mut wire = Wire::open(CallerTier::Operator).await;
    wire.fake.push_refusal(
        422,
        "unsupported_capability",
        "this session's runtime never declared that operation",
    );

    let result = wire
        .call(
            "session_message",
            json!({ "agent_run_id": id(2), "body": "keep going" }),
        )
        .await;
    assert_eq!(result["isError"], json!(true));
    let body = Wire::document(&result);
    assert_eq!(body["code"], json!("unsupported_capability"));
    assert_eq!(
        body["rule"],
        json!("this session's runtime never declared that operation")
    );
}

#[tokio::test]
async fn an_authority_refusal_is_reported_without_the_daemon_being_asked() {
    let mut wire = Wire::open(CallerTier::Observer).await;
    let result = wire
        .call(
            "run_launch",
            json!({ "project_id": id(1), "agent_run_id": id(2), "expected_revision": 1 }),
        )
        .await;
    assert_eq!(result["isError"], json!(true));
    let body = Wire::document(&result);
    assert_eq!(
        body["code"],
        json!("forbidden"),
        "an authority refusal reports the code the contract already has for it"
    );
    assert_eq!(
        wire.fake.dispatched(),
        0,
        "and it happens before anything is dispatched"
    );
}

#[tokio::test]
async fn an_undeclared_argument_is_refused_and_never_forwarded() {
    let mut wire = Wire::open(CallerTier::Operator).await;
    let result = wire
        .call(
            "run_launch",
            json!({
                "project_id": id(1),
                "agent_run_id": id(2),
                "expected_revision": 1,
                "runtime_endpoint": "http://10.0.0.4:9000"
            }),
        )
        .await;
    assert_eq!(result["isError"], json!(true));
    let body = Wire::document(&result);
    assert_eq!(body["code"], json!("invalid_request"));
    assert!(
        body["rule"]
            .as_str()
            .is_some_and(|rule| rule.contains("runtime_endpoint")),
        "the refusal names the property, so a caller can remove it: {body}"
    );
    assert_eq!(
        wire.fake.dispatched(),
        0,
        "an argument the schema does not declare never reaches the daemon"
    );
}

#[tokio::test]
async fn an_unreachable_realm_is_reported_as_a_channel_fact() {
    let mut wire = Wire::open(CallerTier::Observer).await;
    wire.fake.go_unreachable();
    let result = wire.call("health_show", json!({})).await;
    assert_eq!(result["isError"], json!(true));
    let body = Wire::document(&result);
    assert_eq!(
        body["code"],
        json!("unavailable"),
        "no answer is a fact about the channel, and never a statement about the work"
    );
}

#[tokio::test]
async fn a_realm_that_answers_for_another_realm_is_refused_locally() {
    let mut wire = Wire::open(CallerTier::Observer).await;
    // The first answer establishes the expectation; the second claims to be a
    // different realm, which is exactly the case a cached identifier makes dangerous.
    wire.fake.push_ok(json!({ "value": { "live": true } }));
    wire.fake.push(
        200,
        json!({ "realm_id": id(0xee), "value": { "live": true } }),
    );

    let first = wire.call("health_show", json!({})).await;
    assert_ne!(
        first["isError"],
        json!(true),
        "the first answer establishes the realm"
    );

    let second = wire.call("health_show", json!({})).await;
    assert_eq!(second["isError"], json!(true));
    let body = Wire::document(&second);
    assert_eq!(
        body["code"],
        json!("realm_mismatch"),
        "a client that established one realm must not silently show another's rows"
    );
    assert_eq!(
        body["realm_id"],
        json!(kontor_mcp::fake::FIXTURE_REALM),
        "the refusal names the realm this client belongs to, not the one that answered"
    );
}

#[tokio::test]
async fn an_unknown_tool_is_refused_rather_than_approximated() {
    let mut wire = Wire::open(CallerTier::Admin).await;
    let response = wire
        .request(
            "tools/call",
            json!({ "name": "ticket_transition", "arguments": {} }),
        )
        .await;
    // rmcp validates the name against `get_tool` before dispatch, so this may come
    // back either as a protocol error or as the tool error `execute` produces.
    // Both are correct; silently doing something is not.
    let refused = response.get("error").is_some() || response["result"]["isError"] == json!(true);
    assert!(
        refused,
        "a staged surface must be refused however the protocol layer reports it: {response}"
    );
    assert_eq!(
        wire.fake.dispatched(),
        0,
        "and nothing about it reaches the daemon"
    );
}

// ---------------------------------------------------------------------------
// The local contract with the daemon's own files
// ---------------------------------------------------------------------------

#[test]
fn the_credential_file_matches_the_daemons() {
    // This crate cannot depend on `kontor-daemon` — that would pull in every runtime
    // adapter — so the file name and its three keys are repeated here. This is the
    // check that keeps the two spellings from drifting: the daemon writes
    // `credentials.json` with `schema_version`, `observer`, `operator` and `admin`.
    assert_eq!(kontor_mcp::client::CREDENTIAL_FILE, "credentials.json");
    assert_eq!(kontor_mcp::client::LOCAL_SCHEMA, 1);
    let tiers: Vec<&str> = CallerTier::ALL.iter().map(|tier| tier.as_str()).collect();
    assert_eq!(tiers, vec!["observer", "operator", "admin"]);
}

#[test]
fn the_default_port_matches_the_daemons() {
    // `kontor_daemon::DEFAULT_PORT`. Repeated for the same reason, and checked here
    // so a change on either side shows up as a failing test rather than as a CLI
    // that cannot find a running realm.
    assert_eq!(kontor_mcp::client::DEFAULT_PORT, 7717);
}
