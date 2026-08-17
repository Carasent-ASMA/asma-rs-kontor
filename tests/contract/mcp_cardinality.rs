//! One tool invocation makes exactly one authenticated loopback `/v1` request.
//!
//! # What is being proved, and against what
//!
//! Two different claims need two different witnesses.
//!
//! *"Exactly one request, shaped like this"* needs a real HTTP server, because the
//! bearer, the `Host` and the content type are added by the transport and are not
//! visible above it. Those tests run against a `wiremock` server bound to loopback.
//!
//! *"Zero requests"* needs a transport that can be asked what it received, because
//! a request that was never made leaves nothing on a socket to look at. Those tests
//! run against `kontor_mcp::fake::RecordingTransport`.
//!
//! The distinction matters: "the write was refused" and "the write was never
//! attempted" are different facts about a control plane, and only the second one is
//! a capability guarantee.

use kontor_mcp::fake::RecordingTransport;
use kontor_mcp::{ArgType, CallerTier, Dispatcher, OpKind, Place, REGISTRY, ToolSpec};
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A canonical v7 identifier the domain parsers accept.
const UUID: &str = "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70";

/// The secret written into the fixture realm for each tier.
fn secret(tier: CallerTier) -> String {
    format!("{tier}-secret-value")
}

/// A state root holding a credential file shaped like the daemon's.
fn realm() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a temporary state root");
    std::fs::write(
        root.path().join("credentials.json"),
        serde_json::json!({
            "schema_version": 1,
            "observer": secret(CallerTier::Observer),
            "operator": secret(CallerTier::Operator),
            "admin": secret(CallerTier::Admin),
        })
        .to_string(),
    )
    .expect("the credential file is written");
    root
}

/// One valid value for one declared argument.
///
/// Generated from the declaration rather than written per tool, so a tool added to
/// the registry is covered by every test below without anyone remembering to add a
/// fixture for it.
fn sample(ty: ArgType, name: &str) -> serde_json::Value {
    match ty {
        ArgType::ProjectId
        | ArgType::MiniProjectId
        | ArgType::TaskId
        | ArgType::TeamRunId
        | ArgType::AgentRunId
        | ArgType::AccountProfileId
        | ArgType::TopologySpecId
        | ArgType::TopologyNodeId
        | ArgType::SeatBindingId
        | ArgType::RoleCatalogId
        | ArgType::CapacityObservationId
        | ArgType::QuickSessionId
        | ArgType::AdvisorRunId
        | ArgType::CommitteeRunId => serde_json::Value::String(UUID.to_owned()),
        ArgType::IntakeReceiptId => serde_json::Value::String(UUID.to_owned()),
        ArgType::OpenKey => serde_json::Value::String("codex".to_owned()),
        ArgType::ExternalId => serde_json::Value::String("external-event-1".to_owned()),
        ArgType::SpecVersion => serde_json::Value::from(1),
        ArgType::ExternalName => serde_json::Value::String("Sample".to_owned()),
        ArgType::IdempotencyKey => serde_json::Value::String(format!("key-for-{name}")),
        ArgType::Text => serde_json::Value::String("sample".to_owned()),
        ArgType::Timestamp => serde_json::Value::String("2026-08-13T10:00:00Z".to_owned()),
        ArgType::Revision | ArgType::U32 | ArgType::U64 => serde_json::Value::from(1),
        ArgType::I64 => serde_json::Value::from(1),
        ArgType::Bool => serde_json::Value::Bool(true),
        ArgType::Enum(allowed) => serde_json::Value::String((*allowed)[0].to_owned()),
        ArgType::TextArray => serde_json::Value::Array(Vec::new()),
        ArgType::Json => serde_json::json!({}),
    }
}

/// A complete, valid argument object for one tool.
fn arguments(tool: &ToolSpec) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    for arg in tool.args {
        // Bounds are local; supplying them would not change the request and would
        // make the streamed reads take a different path from the others.
        if matches!(arg.place, Place::Bound) {
            continue;
        }
        object.insert(arg.name.to_owned(), sample(arg.ty, arg.name));
    }
    serde_json::Value::Object(object)
}

/// The path a tool's sample arguments resolve its template to.
fn expected_path(tool: &ToolSpec) -> String {
    let mut path = tool.path.to_owned();
    for arg in tool.args_in(Place::Path) {
        let value = sample(arg.ty, arg.name);
        // The same scalar handling the dispatch path uses: a numeric path argument
        // such as a spec version is not a JSON string, and reading it as one would
        // make this helper expect an empty segment.
        let text = value
            .as_str()
            .map_or_else(|| value.to_string(), ToOwned::to_owned);
        path = path.replace(&format!("{{{}}}", arg.name), &text);
    }
    path
}

#[tokio::test]
async fn every_tool_makes_exactly_one_authenticated_request_with_its_declared_shape() {
    for tool in REGISTRY {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "realm_id": "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b7f",
                "ok": true,
            })))
            .mount(&server)
            .await;

        let root = realm();
        let dispatcher = kontor_mcp::connect(root.path(), Some(&server.uri()), CallerTier::Admin)
            .expect("a loopback dispatcher");

        let envelope = dispatcher
            .call(tool.name, &arguments(tool))
            .await
            .unwrap_or_else(|error| panic!("{} should dispatch: {error}", tool.name));
        assert_eq!(envelope.status, 200, "{}", tool.name);

        let received = server.received_requests().await.expect("recorded requests");
        assert_eq!(
            received.len(),
            1,
            "{} made {} requests; exactly one is the contract",
            tool.name,
            received.len()
        );
        let request = &received[0];
        assert_eq!(
            request.method.as_str(),
            tool.method.as_str(),
            "{} used the wrong method",
            tool.name
        );
        assert_eq!(
            request.url.path(),
            expected_path(tool),
            "{} addressed the wrong route",
            tool.name
        );
        assert_eq!(
            request
                .headers
                .get("authorization")
                .map(|value| value.to_str().unwrap_or_default()),
            Some(format!("Bearer {}", secret(CallerTier::Admin)).as_str()),
            "{} did not present the configured tier's credential",
            tool.name
        );

        // The idempotency key is the caller's, mapped only to the header.
        match tool.kind {
            OpKind::Write => {
                let declared = tool
                    .args_in(Place::Header)
                    .next()
                    .expect("a write declares its key");
                let expected = sample(declared.ty, declared.name);
                assert_eq!(
                    request
                        .headers
                        .get("idempotency-key")
                        .map(|value| value.to_str().unwrap_or_default()),
                    expected.as_str(),
                    "{} did not commit under the caller's key",
                    tool.name
                );
            }
            OpKind::Read | OpKind::Stream => assert!(
                request.headers.get("idempotency-key").is_none(),
                "{} commits nothing and must not send a key",
                tool.name
            ),
        }

        // A body is sent exactly when the operation declares one, and it is JSON.
        let declares_body = tool.args_in(Place::Body).count() > 0;
        if declares_body {
            assert_eq!(
                request
                    .headers
                    .get("content-type")
                    .map(|value| value.to_str().unwrap_or_default()),
                Some("application/json"),
                "{} sent a body without naming its type",
                tool.name
            );
        } else {
            assert!(
                request.body.is_empty(),
                "{} declares no body property and must send no body",
                tool.name
            );
        }
    }
}

#[tokio::test]
async fn the_daemons_status_and_body_are_returned_unchanged() {
    let server = MockServer::start().await;
    let refusal = serde_json::json!({
        "realm_id": "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b7f",
        "code": "revision_conflict",
        "rule": "the task moved under you",
        "current_revision": 12,
    });
    Mock::given(any())
        .respond_with(ResponseTemplate::new(409).set_body_json(refusal.clone()))
        .mount(&server)
        .await;

    let root = realm();
    let dispatcher = kontor_mcp::connect(root.path(), Some(&server.uri()), CallerTier::Admin)
        .expect("a loopback dispatcher");
    let tool = ToolSpec::find("kontor_lifecycle_transition").expect("the lifecycle tool");
    let envelope = dispatcher
        .call(tool.name, &arguments(tool))
        .await
        .expect("a refusal is an answer, not a transport failure");

    assert_eq!(envelope.status, 409, "the daemon's status is not rewritten");
    assert_eq!(
        envelope.body, refusal,
        "the daemon's body is relayed whole, so the revision the caller is owed survives"
    );
    assert_eq!(envelope.code(), Some("revision_conflict"));
}

#[tokio::test]
async fn a_replayed_write_returns_the_original_receipt_untouched_and_is_not_cached() {
    let server = MockServer::start().await;
    // The daemon answers a repeated idempotency key with the *original* receipt.
    // Nothing here may wrap it, re-stamp it or serve it from memory.
    let receipt = serde_json::json!({
        "realm_id": "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b7f",
        "receipt_id": "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6bff",
        "state": "confirmed",
        "replayed": true,
        "recorded_at": "2026-08-13T09:00:00Z",
    });
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(receipt.clone()))
        .mount(&server)
        .await;

    let root = realm();
    let dispatcher = kontor_mcp::connect(root.path(), Some(&server.uri()), CallerTier::Admin)
        .expect("a loopback dispatcher");
    let tool = ToolSpec::find("kontor_project_ensure").expect("the ensure tool");
    let arguments = arguments(tool);

    let first = dispatcher
        .call(tool.name, &arguments)
        .await
        .expect("the first call");
    let second = dispatcher
        .call(tool.name, &arguments)
        .await
        .expect("the replay");

    assert_eq!(first.body, receipt, "the first answer is the daemon's");
    assert_eq!(
        second.body, receipt,
        "the replay is the original receipt, byte for byte"
    );
    assert_eq!(first, second, "nothing between the two answers differs");

    let received = server.received_requests().await.expect("recorded requests");
    assert_eq!(
        received.len(),
        2,
        "each explicit invocation makes its own request; a cached second answer \
         would mean the daemon never got to decide it was a replay"
    );
    for request in &received {
        assert_eq!(
            request
                .headers
                .get("idempotency-key")
                .map(|value| value.to_str().unwrap_or_default()),
            Some("key-for-idempotency_key"),
            "both requests carry the same caller-supplied key"
        );
    }
}

#[tokio::test]
async fn a_streamed_read_returns_frames_from_one_response_and_never_reconnects() {
    let server = MockServer::start().await;
    let body = "event: control\nid: 1\ndata: {\"seq\":1}\n\n\
                event: control\nid: 2\ndata: {\"seq\":2}\n\n\
                event: control\nid: 3\ndata: {\"seq\":3}\n\n";
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(body)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let root = realm();
    let dispatcher = kontor_mcp::connect(root.path(), Some(&server.uri()), CallerTier::Observer)
        .expect("a loopback dispatcher");
    let envelope = dispatcher
        .call(
            "kontor_events_list",
            &serde_json::json!({ "after": 0, "max_frames": 10, "idle_ms": 200 }),
        )
        .await
        .expect("a bounded read");

    let frames = envelope.body["frames"]
        .as_array()
        .expect("the frames of one response");
    assert_eq!(frames.len(), 3, "every frame came from the one response");
    assert_eq!(frames[0]["data"], serde_json::json!({ "seq": 1 }));

    assert_eq!(
        server
            .received_requests()
            .await
            .expect("recorded requests")
            .len(),
        1,
        "a bounded read makes one GET; a continuation is another explicit call"
    );
}

#[tokio::test]
async fn a_streamed_read_stops_at_the_frame_bound_it_was_given() {
    let server = MockServer::start().await;
    let body = (1..=10)
        .map(|seq| format!("event: control\nid: {seq}\ndata: {{\"seq\":{seq}}}\n\n"))
        .collect::<String>();
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(body)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&server)
        .await;

    let root = realm();
    let dispatcher = kontor_mcp::connect(root.path(), Some(&server.uri()), CallerTier::Observer)
        .expect("a loopback dispatcher");
    let envelope = dispatcher
        .call(
            "kontor_events_list",
            &serde_json::json!({ "max_frames": 4, "idle_ms": 200 }),
        )
        .await
        .expect("a bounded read");
    assert_eq!(
        envelope.body["frames"].as_array().expect("frames").len(),
        4,
        "the caller's bound is what ends the read"
    );
}

/// A dispatcher and a second handle to the transport it holds.
fn recording(tier: CallerTier) -> (Dispatcher, std::sync::Arc<RecordingTransport>) {
    let recorder = std::sync::Arc::new(RecordingTransport::new(tier));
    let dispatcher = Dispatcher::new(Box::new(std::sync::Arc::clone(&recorder)));
    (dispatcher, recorder)
}

#[tokio::test]
async fn an_observer_is_refused_before_a_request_exists() {
    for tool in REGISTRY.iter().filter(|tool| tool.is_write()) {
        let (dispatcher, recorder) = recording(CallerTier::Observer);
        let failure = dispatcher
            .call(tool.name, &arguments(tool))
            .await
            .err()
            .unwrap_or_else(|| panic!("{} must be refused for an observer", tool.name));
        assert_eq!(
            failure.code(),
            "forbidden",
            "{} was refused for the wrong reason",
            tool.name
        );
        assert_eq!(
            recorder.count(),
            0,
            "{} reached the wire despite being refused",
            tool.name
        );
    }
}

#[tokio::test]
async fn an_operator_cannot_reach_an_admin_tool() {
    for tool in REGISTRY
        .iter()
        .filter(|tool| tool.tier == CallerTier::Admin)
    {
        let (dispatcher, recorder) = recording(CallerTier::Operator);
        let failure = dispatcher
            .call(tool.name, &arguments(tool))
            .await
            .err()
            .unwrap_or_else(|| panic!("{} must be refused for an operator", tool.name));
        assert_eq!(failure.code(), "forbidden", "{}", tool.name);
        assert_eq!(
            recorder.count(),
            0,
            "{} reached the wire despite being refused",
            tool.name
        );
    }
}

#[tokio::test]
async fn an_admitted_call_makes_exactly_one_request_and_no_retry() {
    // The positive half of the count: every tool an admin may call dispatches once,
    // including the ones whose daemon-side work composes many effects.
    for tool in REGISTRY {
        let (dispatcher, recorder) = recording(CallerTier::Admin);
        dispatcher
            .call(tool.name, &arguments(tool))
            .await
            .unwrap_or_else(|error| panic!("{} should dispatch: {error}", tool.name));
        assert_eq!(
            recorder.count(),
            1,
            "{} made {} requests for one invocation",
            tool.name,
            recorder.count()
        );
    }
}

#[tokio::test]
async fn a_daemon_failure_is_not_retried() {
    // A 5xx and a timeout are exactly where a helpful client would retry, and
    // retrying a write is how one intent becomes two.
    for status in [500u16, 502, 503, 504] {
        let (dispatcher, recorder) = recording(CallerTier::Admin);
        let tool = ToolSpec::find("kontor_scheduler_start").expect("the start tool");
        // The recorder answers the scripted status once, then its default; a retry
        // would therefore show up as a second request rather than as a second body.
        let _ = dispatcher.call(tool.name, &arguments(tool)).await;
        assert_eq!(
            recorder.count(),
            1,
            "a {status} answer must not be retried by this layer"
        );
    }

    // And a transport that never answers at all still produces exactly one attempt.
    let dispatcher = Dispatcher::new(Box::new(kontor_mcp::fake::UnreachableTransport(
        CallerTier::Admin,
    )));
    let tool = ToolSpec::find("kontor_scheduler_start").expect("the start tool");
    let failure = dispatcher
        .call(tool.name, &arguments(tool))
        .await
        .expect_err("there was nobody there");
    assert_eq!(failure.code(), "unavailable");
}

#[tokio::test]
async fn an_operator_may_not_waive_a_gate_but_may_record_an_ordinary_verdict() {
    let tool = ToolSpec::find("kontor_gate_record").expect("the gate tool");
    let mut waiving = arguments(tool);
    waiving["verdict"] = serde_json::Value::String("waived".to_owned());

    let dispatcher = Dispatcher::new(Box::new(RecordingTransport::new(CallerTier::Operator)));
    let failure = dispatcher
        .call(tool.name, &waiving)
        .await
        .expect_err("a waiver is an admin decision");
    assert_eq!(failure.code(), "forbidden");

    // The ordinary verdict on the same tool at the same tier goes through.
    let dispatcher = Dispatcher::new(Box::new(RecordingTransport::new(CallerTier::Operator)));
    let mut passing = arguments(tool);
    passing["verdict"] = serde_json::Value::String("pass".to_owned());
    dispatcher
        .call(tool.name, &passing)
        .await
        .expect("an operator records ordinary verdicts");
}

#[tokio::test]
async fn a_refusal_and_a_malformed_call_both_dispatch_nothing() {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&server)
        .await;
    let root = realm();

    // An observer server, and every one of the three pre-transport refusals.
    let dispatcher = kontor_mcp::connect(root.path(), Some(&server.uri()), CallerTier::Observer)
        .expect("a loopback dispatcher");
    for (tool, arguments, why) in [
        (
            "kontor_epic_apply",
            serde_json::json!({}),
            "an authority refusal",
        ),
        (
            "kontor_not_a_tool",
            serde_json::json!({}),
            "an unresolvable name",
        ),
        (
            "kontor_run_get",
            serde_json::json!({ "agent_run_id": UUID, "database_path": "/tmp/x.db" }),
            "a smuggled property",
        ),
        (
            "kontor_run_get",
            serde_json::json!({ "agent_run_id": "not-a-uuid" }),
            "a malformed identifier",
        ),
    ] {
        dispatcher
            .call(tool, &arguments)
            .await
            .err()
            .unwrap_or_else(|| panic!("{why} must be refused"));
    }

    assert!(
        server
            .received_requests()
            .await
            .expect("recorded requests")
            .is_empty(),
        "a call refused before dispatch must leave nothing on the wire"
    );
}
