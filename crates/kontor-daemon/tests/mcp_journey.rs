//! A realm bootstrapped through MCP tools alone, against a real daemon.
//!
//! # What this proves that the contract crate cannot
//!
//! The contract crate proves the wrapper's *shape* against a recording server: one
//! request per invocation, the declared method and path, the caller's key on the
//! header, the daemon's body relayed whole. It cannot prove that the shape is one
//! `kontord` accepts, because nothing there is a `kontord`.
//!
//! This drives the real router — the real ingress check, the real bearer
//! comparison, the real `caller.require`, the real application services and the
//! real store — using nothing but the admin Lead seat's own tools. Every argument
//! below is a tool argument. Nothing seeds SQLite, creates a native session, or
//! calls Paseo, Jira or AgentsRoom.
//!
//! # Why it is not a socket
//!
//! TST-001: no test in this crate binds a socket, spawns a child process or runs
//! the daemon binary. [`RouterTransport`] therefore implements the transport seam
//! `kontor-mcp` is written against by driving the same `axum::Router` the binary
//! serves, through `tower::ServiceExt::oneshot`. Everything above the seam — the
//! registry, the authority gate, the schema validation, the one-request rule — is
//! the production code path, unchanged.
//!
//! The full live proof on real harnesses belongs to KON-MVP-18.

// The harness is one module shared by two test binaries, and this one drives the
// router through the tool surface rather than through `Call`. The helpers it does
// not reach are live code in `loopback_api`, so the unused warning here is an
// artefact of per-binary compilation rather than anything to delete.
#[allow(dead_code)]
mod harness;

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::Request;
use harness::{World, secret};
use kontor_mcp::{CallerTier, Dispatcher, FrameBudget, Method, Reply, Transport, TransportFailure};
use tower::ServiceExt as _;

/// The transport seam, bound to a real router instead of a real socket.
struct RouterTransport {
    router: Router,
    tier: CallerTier,
    secret: String,
    /// Every request that reached the router, so the one-call rule is checked here
    /// too rather than only against the recording server.
    seen: Mutex<Vec<(String, String)>>,
    count: AtomicUsize,
}

impl std::fmt::Debug for RouterTransport {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RouterTransport")
            .field("tier", &self.tier)
            .finish_non_exhaustive()
    }
}

impl RouterTransport {
    fn new(world: &World, tier: CallerTier) -> Self {
        Self {
            router: world.router.clone(),
            tier,
            secret: secret(world, tier.as_str()),
            seen: Mutex::new(Vec::new()),
            count: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    fn routes(&self) -> Vec<(String, String)> {
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[async_trait::async_trait]
impl Transport for RouterTransport {
    fn tier(&self) -> CallerTier {
        self.tier
    }

    fn base_url(&self) -> String {
        "http://127.0.0.1:7717".to_owned()
    }

    async fn call(&self, request: &kontor_mcp::Request) -> Result<Reply, TransportFailure> {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((request.method.as_str().to_owned(), request.path.clone()));

        let mut uri = request.path.clone();
        if !request.query.is_empty() {
            let query: Vec<String> = request
                .query
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            uri = format!("{uri}?{}", query.join("&"));
        }
        let mut builder = Request::builder()
            .method(match request.method {
                Method::Get => axum::http::Method::GET,
                Method::Post => axum::http::Method::POST,
            })
            .uri(uri)
            // The same three headers the real transport sets, so the daemon's
            // ingress and authentication run exactly as they would on the wire.
            .header("host", "127.0.0.1:7717")
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", self.secret));
        if let Some(key) = &request.idempotency_key {
            builder = builder.header("idempotency-key", key);
        }
        let body = request.body.as_ref().map_or_else(Body::empty, |document| {
            Body::from(serde_json::to_vec(document).expect("a serializable body"))
        });
        let response = self
            .router
            .clone()
            .oneshot(builder.body(body).expect("a well-formed request"))
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("the whole body is readable");
        let body = serde_json::from_slice(&bytes).map_err(|_| TransportFailure::Protocol {
            path: request.path.clone(),
            detail: "the body was not JSON",
        })?;
        Ok(Reply { status, body })
    }

    async fn frames(
        &self,
        request: &kontor_mcp::Request,
        _budget: FrameBudget,
    ) -> Result<Reply, TransportFailure> {
        // A server-sent stream does not end, and `oneshot` reads a whole body.
        // Bounded streamed reads are proved against the recording HTTP server in
        // the contract crate, where the body is finite; refusing here is honest
        // rather than hanging or pretending.
        Err(TransportFailure::Protocol {
            path: request.path.clone(),
            detail: "streamed reads are proved against the http transport",
        })
    }
}

/// The admin Lead seat: one dispatcher, one credential, the whole vocabulary.
fn lead_seat(world: &World) -> (Dispatcher, std::sync::Arc<RouterTransport>) {
    let transport = std::sync::Arc::new(RouterTransport::new(world, CallerTier::Admin));
    (
        Dispatcher::new(Box::new(std::sync::Arc::clone(&transport))),
        transport,
    )
}

/// Call one tool and require the daemon to have accepted it.
async fn ok(
    dispatcher: &Dispatcher,
    tool: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let envelope = dispatcher
        .call(tool, &arguments)
        .await
        .unwrap_or_else(|error| panic!("{tool} should reach the daemon: {error}"));
    assert!(
        envelope.is_success(),
        "{tool} was refused: {} {}",
        envelope.status,
        envelope.body
    );
    envelope.body
}

#[tokio::test]
async fn an_empty_realm_is_bootstrapped_through_mcp_tools_alone() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;
    let (lead, transport) = lead_seat(&world);

    // 1. Identity, then the catalogs a graph is chosen from. Nothing is seeded:
    //    every value below comes out of a tool answer.
    let realm = ok(&lead, "kontor_realm_get", serde_json::json!({})).await;
    assert!(realm["realm_id"].is_string(), "the realm names itself");

    let profiles = ok(&lead, "kontor_work_profiles_list", serde_json::json!({})).await;
    let category = profiles.as_array().expect("a catalog")[0]["category"]
        .as_str()
        .expect("a category")
        .to_owned();
    ok(&lead, "kontor_team_templates_list", serde_json::json!({})).await;
    let capabilities = ok(
        &lead,
        "kontor_runtime_capabilities_list",
        serde_json::json!({}),
    )
    .await;
    assert!(
        capabilities.as_array().is_some_and(|list| !list.is_empty()),
        "the configured fleet reports what it can prove"
    );

    // 2. The project, created through the tool and replayed through the same key.
    let created = ok(
        &lead,
        "kontor_project_ensure",
        serde_json::json!({
            "idempotency_key": "journey-project-1",
            "name": "Kontor",
            "root_path": "/tmp/kontor-mcp-journey",
        }),
    )
    .await;
    assert_eq!(created["applied"], "created");
    let project = created["project_id"]
        .as_str()
        .expect("a project id")
        .to_owned();
    let revision = created["revision"].as_u64().expect("a revision");

    let replayed = ok(
        &lead,
        "kontor_project_ensure",
        serde_json::json!({
            "idempotency_key": "journey-project-1",
            "name": "Kontor",
            "root_path": "/tmp/kontor-mcp-journey",
        }),
    )
    .await;
    // The daemon's own ensure semantics, relayed rather than interpreted: the same
    // root is the same project, and it says so by reporting `unchanged` instead of
    // creating a second one. The tool neither hides that marker nor invents one.
    assert_eq!(replayed["applied"], "unchanged");
    assert_eq!(replayed["project_id"], created["project_id"]);
    assert_eq!(replayed["revision"], created["revision"]);
    assert_eq!(replayed["created_at"], created["created_at"]);

    // 3. An account profile a run could be pinned to.
    let account = ok(
        &lead,
        "kontor_account_profile_ensure",
        serde_json::json!({
            "project_id": project,
            "idempotency_key": "journey-account-1",
            "label": "Primary",
            "harness": "fake.runtime",
            "credential_alias": "journey-alias",
            "enabled": true,
        }),
    )
    .await;
    assert!(account["account_profile_id"].is_string());

    // 4. The whole graph, applied atomically, with its dependency edge and its
    //    ticket link resolved inside `kontord`.
    let applied = ok(
        &lead,
        "kontor_epic_apply",
        serde_json::json!({
            "project_id": project,
            "idempotency_key": "journey-epic-1",
            "expected_revision": revision,
            "name": "Bootstrap epic",
            "work_profile_category": category,
            "runtime_family": "fake.runtime",
            "tasks": [
                {"title": "Design the thing", "ticket_links": [
                    {"connector": "jira", "external_issue_key": "ASMA-1"}
                ]},
                {"title": "Build the thing", "depends_on": ["Design the thing"]}
            ],
        }),
    )
    .await;
    let epic = applied["epic_id"].as_str().expect("an epic id").to_owned();
    assert_eq!(
        applied["tasks"].as_array().expect("tasks").len(),
        2,
        "one tool call applied the whole graph"
    );

    // 5. The projection reads the graph back, including the workflow revision a
    //    gate recording has to present.
    let projection = ok(
        &lead,
        "kontor_epic_get",
        serde_json::json!({ "project_id": project, "epic_id": epic }),
    )
    .await;
    assert_eq!(projection["tasks"].as_array().expect("tasks").len(), 2);
    assert!(
        projection["tasks"][0]["workflow_revision"].is_u64(),
        "the projection reports what a verdict must cite: {projection}"
    );
    let epic_revision = projection["revision"]
        .as_u64()
        .or_else(|| applied["revision"].as_u64())
        .unwrap_or(1);

    // 6. Arming, then the plan it makes possible. A plan commits nothing, which is
    //    why it takes no key.
    let armed = lead
        .call(
            "kontor_execution_arm",
            &serde_json::json!({
                "project_id": project,
                "epic_id": epic,
                "idempotency_key": "journey-arm-1",
                "expected_revision": epic_revision,
                "allowed_start": "2026-08-13T00:00:00Z",
                "allowed_end": "2026-08-14T00:00:00Z",
                "max_concurrency": 1,
                "budget": {
                    "max_tokens": 100_000,
                    "max_commands": 100,
                    "max_duration_seconds": 3_600,
                    "max_cost_minor_units": 1_000,
                    "cost_currency": "NOK",
                },
                "granted_by": account["account_profile_id"],
                "reason": "the journey is authorized",
            }),
        )
        .await
        .expect("arming reaches the daemon");
    assert!(
        armed.is_success() || armed.status == 409,
        "arming either grants or refuses on the domain's own terms: {} {}",
        armed.status,
        armed.body
    );

    let planned = lead
        .call(
            "kontor_scheduler_plan",
            &serde_json::json!({ "project_id": project, "epic_id": epic }),
        )
        .await
        .expect("planning reaches the daemon");
    assert!(
        planned.is_success(),
        "a plan is a read the operator tier may always take: {} {}",
        planned.status,
        planned.body
    );

    // Every step above was one tool call and one HTTP operation. Nothing composed
    // two requests, and nothing retried.
    let routes = transport.routes();
    assert_eq!(
        transport.calls(),
        routes.len(),
        "the counter and the record agree"
    );
    assert_eq!(
        transport.calls(),
        11,
        "eleven tool invocations made eleven requests: {routes:#?}"
    );
    assert!(
        routes.iter().all(|(_, path)| path.starts_with("/v1/")),
        "every request addressed the versioned contract: {routes:#?}"
    );
}

#[tokio::test]
async fn an_observer_seat_reads_the_realm_and_cannot_change_it() {
    let world = World::open_empty().await;
    world.daemon.reconcile().await;

    let transport = std::sync::Arc::new(RouterTransport::new(&world, CallerTier::Observer));
    let observer = Dispatcher::new(Box::new(std::sync::Arc::clone(&transport)));

    // The reviewer seat's whole job, and it works.
    let realm = observer
        .call("kontor_realm_get", &serde_json::json!({}))
        .await
        .expect("an observer reads the realm");
    assert!(realm.is_success(), "{} {}", realm.status, realm.body);
    let reads = transport.calls();
    assert_eq!(reads, 1);

    // And every write is refused here, before the daemon is asked.
    for (tool, arguments) in [
        (
            "kontor_project_ensure",
            serde_json::json!({
                "idempotency_key": "nope",
                "name": "Kontor",
                "root_path": "/tmp/nope",
            }),
        ),
        (
            "kontor_scheduler_start",
            serde_json::json!({
                "project_id": "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b70",
                "epic_id": "01936b3e-7c2a-7bd0-9f4a-2c8e1d5a6b71",
                "idempotency_key": "nope",
                "plan_hash": "abc",
            }),
        ),
    ] {
        let failure = observer
            .call(tool, &arguments)
            .await
            .err()
            .unwrap_or_else(|| panic!("{tool} must be refused for an observer"));
        assert_eq!(failure.code(), "forbidden");
    }
    assert_eq!(
        transport.calls(),
        reads,
        "a refused write must not reach the daemon at all"
    );
}
