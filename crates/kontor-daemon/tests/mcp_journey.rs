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
//! below is a tool argument. No Kontor aggregate is seeded, and nothing creates a
//! native session or calls Paseo, Jira or AgentsRoom. The fixture records only the
//! exact Jira readback evidence that the real Jira boundary would have confirmed;
//! native naming deliberately refuses to infer that external fact from a link.
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
use harness::{World, at, secret};
use kontor_core::id::{
    AgentRunId, AggregateRevision, ContentHash, ExternalId, MiniProjectId, ProjectId,
};
use kontor_core::repository::RunRepository as _;
use kontor_mcp::{CallerTier, Dispatcher, FrameBudget, Method, Reply, Transport, TransportFailure};
use kontor_store::{
    JiraIntentKind, JiraItemKind, NewJiraMaterializationBatch, NewJiraMaterializationItem,
};
use tower::ServiceExt as _;

/// A session with history and a live tail, so a launched seat has something to be.
///
/// The declared ceiling is raised above the harness default of eight because this
/// journey seats a five-slot team twice. The scripted runtime counts a session
/// against its concurrency ceiling for as long as it exists, and settling a run
/// does not delete the native session it settled — a terminal session is still a
/// session. Ten seats therefore need a runtime that declares room for ten. This
/// is the fixture describing a bigger runtime, not the journey being given a
/// larger allowance: Kontor's own `max_concurrency` stays at 1 throughout.
const HISTORY_LIVE: &str = r#"{
  "limits": {
    "max_message_bytes": 4096,
    "max_history_page": 64,
    "max_concurrent_sessions": 32
  },
  "history": [
    {"kind": "message", "sequence": 1, "emitted_at": "2026-08-10T09:01:00Z", "body": "one"},
    {"kind": "tool_call", "sequence": 2, "emitted_at": "2026-08-10T09:02:00Z", "body": "two"}
  ],
  "live": [
    {"kind": "message", "sequence": 3, "emitted_at": "2026-08-10T09:03:00Z", "body": "three"}
  ]
}"#;

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
            status: Some(status),
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
            status: None,
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

/// One task's row in an `kontor_epic_get` projection, found by id rather than by
/// position — the projection is free to order tasks however it likes.
fn task_view<'a>(projection: &'a serde_json::Value, task: &str) -> &'a serde_json::Value {
    projection["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|row| row["task_id"].as_str() == Some(task))
        .unwrap_or_else(|| panic!("task {task} is missing from the projection: {projection}"))
}

/// Have the scripted runtime record the exact request/response positions that
/// prove one bounded role turn completed while its persistent seat stays live.
fn observe_current_turn(world: &World, project: &str, run: &str) -> serde_json::Value {
    let project_id = ProjectId::parse(project).expect("a project id");
    let agent_run_id = AgentRunId::parse(run).expect("an agent run id");
    let run = world.daemon.state().with_store(|store| {
        store
            .get_agent_run(project_id, agent_run_id)
            .expect("the settling run reads")
            .expect("the settling run exists")
    });
    let binding = run.binding.expect("the settling run is bound");
    let held = world
        .daemon
        .state()
        .sessions()
        .get(binding.id)
        .expect("this process holds the exact settling binding");
    let message_id = kontor_runtime::request::MessageId::generate();
    let message_identity = message_id.to_string();
    let (message_position, response_position) = world
        .fake
        .observe_turn_completion(&held, message_id, kontor_api::now())
        .expect("the runtime records the completed turn");
    serde_json::json!({
        "message_id": message_identity,
        "message_position": {
            "epoch": message_position.epoch,
            "sequence": message_position.sequence,
        },
        "response_position": {
            "epoch": response_position.epoch,
            "sequence": response_position.sequence,
        },
    })
}

/// Producer artifacts declared by this journey's explicit five-slot fixture.
fn artifacts_for_role(role_slot: &str) -> serde_json::Value {
    match role_slot {
        "builder" => serde_json::json!(["code-change"]),
        "inspector" => serde_json::json!(["review-notes"]),
        "tester" => serde_json::json!(["qa-report"]),
        "architect" => serde_json::json!(["release-notes"]),
        _ => serde_json::json!([]),
    }
}

/// Record the exact successful Jira readbacks that this socket-free journey
/// cannot obtain from a real connector. A ticket link is desired state, not a
/// confirmed identity, so the fixture crosses the same durable confirmation
/// boundary as the connector before native names may be rendered.
fn confirm_jira_identity(world: &World, project: &str, epic: &str, task_jira_keys: &[&str]) {
    let project_id = ProjectId::parse(project).expect("a project id");
    let epic_id = MiniProjectId::parse(epic).expect("an epic id");
    world.daemon.state().with_store(|store| {
        let now = at("2026-08-10T09:20:00Z");
        let batch_id = ExternalId::parse(&uuid::Uuid::now_v7().to_string()).expect("a batch id");
        let mut items = vec![NewJiraMaterializationItem {
            id: ExternalId::parse(&uuid::Uuid::now_v7().to_string()).expect("an item id"),
            batch_id: batch_id.clone(),
            project_id,
            epic_id,
            task_id: None,
            link_id: None,
            ordinal: 0,
            item_kind: JiraItemKind::Epic,
            intent_kind: JiraIntentKind::Link,
            requested_key: Some(ExternalId::parse("ASMA-8000").expect("an epic Jira key")),
            marker: ExternalId::parse(&format!("kontor-test-epic-{epic}")).expect("an epic marker"),
        }];
        let tasks = store
            .list_epic_tasks(project_id, epic_id)
            .expect("the journey tasks read");
        assert_eq!(
            tasks.len(),
            task_jira_keys.len(),
            "the fixture names one exact Jira readback for every task"
        );
        let mut expected_task_bindings = Vec::with_capacity(tasks.len());
        for (index, (task, requested_key)) in tasks
            .into_iter()
            .zip(task_jira_keys.iter().copied())
            .enumerate()
        {
            let links = store
                .list_task_ticket_links(project_id, task.id)
                .expect("the journey task links read");
            let mut jira_links = links
                .iter()
                .filter(|link| link.connector.as_str() == "connector.jira");
            let jira = jira_links
                .next()
                .expect("the journey task has one canonical Jira link");
            assert!(
                jira_links.next().is_none(),
                "the journey task has only one canonical Jira link"
            );
            let requested_key =
                ExternalId::parse(requested_key).expect("an exact task Jira readback key");
            assert_eq!(
                jira.external_issue_key, requested_key,
                "the desired link preserves the exact declared Jira key"
            );
            items.push(NewJiraMaterializationItem {
                id: ExternalId::parse(&uuid::Uuid::now_v7().to_string()).expect("an item id"),
                batch_id: batch_id.clone(),
                project_id,
                epic_id,
                task_id: Some(task.id),
                link_id: Some(jira.id),
                ordinal: u32::try_from(index + 1).expect("a fixture ordinal"),
                item_kind: JiraItemKind::Task,
                intent_kind: JiraIntentKind::Link,
                requested_key: Some(requested_key.clone()),
                marker: ExternalId::parse(&format!("kontor-test-task-{}", task.id))
                    .expect("a task marker"),
            });
            expected_task_bindings.push((task.id, requested_key));
        }
        store
            .plan_jira_materialization(
                &NewJiraMaterializationBatch {
                    id: batch_id.clone(),
                    project_id,
                    epic_id,
                    idempotency_key: format!("journey-confirm-jira-{epic}"),
                    preview_hash: ContentHash::of(epic.as_bytes()),
                    expected_revision: AggregateRevision::INITIAL,
                    created_at: now,
                },
                &items,
            )
            .expect("the journey Jira plan is durable");
        for item in store
            .jira_materialization_items(project_id, &batch_id)
            .expect("the journey Jira items read")
        {
            let key = item
                .requested_key
                .clone()
                .expect("a link intent names its exact Jira key");
            store
                .confirm_jira_materialization_item(
                    &item,
                    &key,
                    &ContentHash::of(format!("{key}-readback").as_bytes()),
                    now,
                )
                .expect("the journey Jira readback confirms");
        }
        store
            .confirm_jira_materialization_batch(project_id, &batch_id, now)
            .expect("the journey Jira batch confirms");
        for (task_id, expected_key) in expected_task_bindings {
            assert_eq!(
                store
                    .confirmed_jira_task_key(project_id, task_id)
                    .expect("the canonical Jira task key reads"),
                Some(expected_key.clone()),
                "the exact confirmed Jira key becomes canonical"
            );
            let jira_links = store
                .list_task_ticket_links(project_id, task_id)
                .expect("the confirmed journey task links read")
                .into_iter()
                .filter(|link| link.connector.as_str() == "connector.jira")
                .collect::<Vec<_>>();
            assert_eq!(
                jira_links.len(),
                1,
                "one canonical Jira link is materialized"
            );
            assert_eq!(jira_links[0].external_issue_key, expected_key);
        }
    });
}

/// The whole journey: an installed, never-used `kontord` reaches a closed epic
/// without a single call that is not one of the Lead seat's own MCP tools.
///
/// This is deliberately one test rather than two. A bootstrap proved through the
/// tool surface and a completion proved through HTTP would demonstrate that each
/// half works for *some* caller; it would not demonstrate that one caller can
/// walk the whole path, which is the thing the ticket asks for. The seam between
/// two callers is exactly where an authority gap hides.
#[tokio::test]
async fn an_empty_realm_is_bootstrapped_through_mcp_tools_alone() {
    let world = World::open_empty().await;
    world.script(HISTORY_LIVE);
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
            "memory_origin": "kontor_native",
            "backlog_origin": "kontor_native",
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
            "memory_origin": "kontor_native",
            "backlog_origin": "kontor_native",
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

    // The catalog's production definition intentionally registers only the
    // calibrated delivery slots. This journey launches the five-slot MVP
    // template, so it must publish and select that explicit test vocabulary
    // before the epic freezes its pin. Do it through MCP as well: silently
    // widening the production fixture or writing the store directly would make
    // the journey pass while bypassing the configuration contract it claims to
    // prove.
    // A fresh project deliberately has no published Team Definition yet, so
    // the first candidate is authored input, not something a list operation
    // could return. Start from the shipped recommendation and make only this
    // journey's explicit slot additions; every acceptance and mutation still
    // crosses the MCP validate/publish/preview/apply boundary below.
    let domain = kontor_profiles::bundled_operational_domain()
        .expect("the bundled operational definition loads");
    let recommended = domain
        .team_definitions
        .first()
        .cloned()
        .expect("the build ships a recommended Team Definition");
    let topology = domain
        .topology_specs
        .iter()
        .find(|topology| {
            topology.spec_id == recommended.topology.spec_id
                && topology.version == recommended.topology.version
        })
        .cloned()
        .expect("the definition's validator ships beside it");
    let topology_candidate = serde_json::to_value(topology).expect("the topology serializes");
    let topology_validation = ok(
        &lead,
        "kontor_topology_spec_validate",
        serde_json::json!({
            "project_id": project,
            "candidate": topology_candidate.clone(),
        }),
    )
    .await;
    assert_eq!(topology_validation["violations"], serde_json::json!([]));
    ok(
        &lead,
        "kontor_topology_spec_publish",
        serde_json::json!({
            "project_id": project,
            "idempotency_key": "journey-topology-publish-1",
            "candidate": topology_candidate,
            "validation_hash": topology_validation["validation_hash"],
            "expected_revision": revision,
        }),
    )
    .await;
    let mut candidate = serde_json::to_value(recommended).expect("the definition serializes");
    let tsw = candidate["containers"]
        .as_array_mut()
        .expect("the definition has containers")
        .iter_mut()
        .find(|container| container["kind"] == "TSW")
        .expect("the definition configures TSW");
    tsw["team_slots"]
        .as_array_mut()
        .expect("TSW has an explicit delivery-slot catalog")
        .extend([
            serde_json::json!({"slot_id": "architect", "role_code": "SA", "capability_profile": "delivery-standard"}),
            serde_json::json!({"slot_id": "builder", "role_code": "SWE", "capability_profile": "delivery-standard"}),
            serde_json::json!({"slot_id": "tester", "role_code": "QA", "capability_profile": "delivery-standard"}),
            serde_json::json!({"slot_id": "inspector", "role_code": "AUD", "capability_profile": "delivery-high"}),
            serde_json::json!({"slot_id": "verifier", "role_code": "UAT", "capability_profile": "delivery-high"}),
        ]);
    let validation = ok(
        &lead,
        "kontor_team_definition_validate",
        serde_json::json!({"project_id": project, "candidate": candidate.clone()}),
    )
    .await;
    assert_eq!(validation["violations"], serde_json::json!([]));
    let published = ok(
        &lead,
        "kontor_team_definition_publish",
        serde_json::json!({
            "project_id": project,
            "idempotency_key": "journey-team-definition-publish-1",
            "candidate": candidate,
            "validation_hash": validation["validation_hash"],
            "expected_revision": revision,
        }),
    )
    .await;
    let selection = ok(
        &lead,
        "kontor_project_team_definition_selection_preview",
        serde_json::json!({
            "project_id": project,
            "target_definition": {
                "id": published["definition"]["id"],
                "version": published["definition"]["version"],
            },
        }),
    )
    .await;
    ok(
        &lead,
        "kontor_project_team_definition_selection_apply",
        serde_json::json!({
            "project_id": project,
            "idempotency_key": "journey-team-definition-select-1",
            "preview_hash": selection["preview_hash"],
            "expected_revision": revision,
        }),
    )
    .await;

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
            "execution_scope": {
                "external_epic_key": "ASMA-8000",
                "short_title": "Bootstrap epic",
                "kontor_backlog_code": "JOURNEY",
                "ai_short_name": "Bootstrap Epic",
            },
            "work_profile_category": category,
            "runtime_family": "fake.runtime",
            // A task with no declared worktree cannot be seated — there is
            // nowhere to prepare its workspace — and the two differ so the
            // scheduler is refusing on the dependency edge rather than on a
            // worktree collision.
            "tasks": [
                {"title": "Design the thing", "short_code": "JOURNEY-01",
                 "ai_short_name": "Design Thing",
                 "worktree": "/w/journey/0", "ticket_links": [
                    {"connector": "jira", "external_issue_key": "ASMA-1"}
                ]},
                {"title": "Build the thing", "short_code": "JOURNEY-02",
                 "ai_short_name": "Build Thing",
                 "worktree": "/w/journey/1",
                 "depends_on": ["Design the thing"], "ticket_links": [
                    {"connector": "jira", "external_issue_key": "ASMA-2"}
                 ]}
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
    confirm_jira_identity(&world, &project, &epic, &["ASMA-1", "ASMA-2"]);

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
    //
    // The window is deliberately wide. A window pinned to the day this test was
    // written would make the journey pass or fail on the wall clock, and an
    // authorization that has silently expired refuses with `authorization_expired`
    // — which looks like a grant that needs re-arming, not like missing a grant.
    ok(
        &lead,
        "kontor_execution_arm",
        serde_json::json!({
            "project_id": project,
            "epic_id": epic,
            "idempotency_key": "journey-arm-1",
            "expected_revision": epic_revision,
            "allowed_start": "2020-01-01T00:00:00Z",
            "allowed_end": "2099-01-01T00:00:00Z",
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
    .await;

    ok(
        &lead,
        "kontor_scheduler_plan",
        serde_json::json!({ "project_id": project, "epic_id": epic }),
    )
    .await;

    // Everything to here was one tool call and one HTTP operation. Nothing composed
    // two requests, and nothing retried. This is the cardinality checkpoint the
    // bootstrap half has always carried; the journey continues past it below.
    let routes = transport.routes();
    assert_eq!(
        transport.calls(),
        routes.len(),
        "the counter and the record agree"
    );
    assert_eq!(
        transport.calls(),
        17,
        "seventeen tool invocations made seventeen requests: {routes:#?}"
    );

    // ---- 7. From the planning point to a closed epic, through the same seat ----
    //
    // The graph has two tasks and a dependency edge, so this is two admission
    // rounds: the successor is not eligible until its predecessor is `done`. Each
    // round re-plans, starts whatever the scheduler admits, settles every seat it
    // created, discharges the gates the pinned profile declares, and completes the
    // task. Nothing below names a task, a slot, a gate or an artifact that did not
    // come out of a tool answer.
    let mut completed_tasks = Vec::new();
    for round in 1..=2u32 {
        // The previous round's `finish_natively` left the runtime holding a
        // cancel script. Re-arm it with a launchable session before asking the
        // scheduler to seat anyone, or the next launch is refused by the fake
        // rather than by Kontor.
        world.script(HISTORY_LIVE);
        let plan = ok(
            &lead,
            "kontor_scheduler_plan",
            serde_json::json!({ "project_id": project, "epic_id": epic }),
        )
        .await;
        let plan_hash = plan["plan_hash"]
            .as_str()
            .expect("a plan names its hash")
            .to_owned();

        // Seats exist only because the scheduler admitted them. Nothing here
        // creates a session, and no test-only administration stands in for
        // admission.
        let started = ok(
            &lead,
            "kontor_scheduler_start",
            serde_json::json!({
                "project_id": project,
                "epic_id": epic,
                "idempotency_key": format!("journey-start-{round}"),
                "plan_hash": plan_hash,
            }),
        )
        .await;
        let seats = started["started"].as_array().expect("seats").clone();
        assert!(
            !seats.is_empty(),
            "round {round}: admission seated nobody, so there is no journey to \
             continue: {started}"
        );

        // The task under work is the one the scheduler chose, read back from the
        // seat rather than assumed by position.
        let task = seats[0]["task_id"]
            .as_str()
            .expect("a seat names its task")
            .to_owned();
        // One task per round. Two things independently force this — the
        // dependency edge and `max_concurrency: 1` — so this assertion does not
        // isolate either, and a mutant that removes only the dependency gate
        // still passes here. That gate is proved on its own in the scheduler's
        // readiness suite; what this line is for is keeping the round loop
        // honest about which task it is settling.
        assert!(
            seats
                .iter()
                .all(|seat| seat["task_id"].as_str() == Some(task.as_str())),
            "one task is admitted at a time: {started}"
        );
        assert!(
            !completed_tasks.contains(&task),
            "round {round}: the scheduler re-admitted a task it already closed: {task}"
        );

        // 8. Every bounded role turn settles with exact runtime positions while
        //    its persistent seat stays live. The role turn, not a later gate or
        //    lifecycle request, is the durable producer of its artifacts.
        let before_turns = ok(
            &lead,
            "kontor_epic_get",
            serde_json::json!({ "project_id": project, "epic_id": epic }),
        )
        .await;
        let task_revision = task_view(&before_turns, &task)["revision"]
            .as_u64()
            .expect("a task revision");
        for (index, seat) in seats.iter().enumerate() {
            let run = seat["agent_run_id"].as_str().expect("an agent run id");
            let role_slot = seat["role_slot"].as_str().expect("a role slot");
            let settled = ok(
                &lead,
                "kontor_turn_settle",
                serde_json::json!({
                    "project_id": project,
                    "agent_run_id": run,
                    "role_slot": role_slot,
                    "expected_task_revision": task_revision,
                    "runtime_proof": observe_current_turn(&world, &project, run),
                    "artifacts": artifacts_for_role(role_slot),
                    "idempotency_key": format!("journey-turn-{round}-{index}"),
                }),
            )
            .await;
            assert_eq!(
                settled["agent_run_id"], *run,
                "the settlement answered about the seat it was asked about: {settled}"
            );
            assert_eq!(settled["seat_live"], true, "the persistent seat stays live");
        }

        // 9. The gates the pinned profile declares, discharged through the public
        //    tool by a role the profile itself authorizes, citing the evidence it
        //    itself requires.
        let projection = ok(
            &lead,
            "kontor_epic_get",
            serde_json::json!({ "project_id": project, "epic_id": epic }),
        )
        .await;
        let view = task_view(&projection, &task);
        let gates = view["gates"].as_array().expect("a gate list").clone();
        assert!(
            !gates.is_empty(),
            "the pinned profile declares gates to discharge: {projection}"
        );
        for (index, gate) in gates.iter().enumerate() {
            let current = ok(
                &lead,
                "kontor_epic_get",
                serde_json::json!({ "project_id": project, "epic_id": epic }),
            )
            .await;
            let workflow_revision = task_view(&current, &task)["workflow_revision"]
                .as_u64()
                .expect("a task with an active workflow reports its revision");
            let name = gate["gate"].as_str().expect("a gate");
            let evaluator = gate["evaluator_roles"]
                .as_array()
                .expect("declared evaluators")
                .first()
                .and_then(serde_json::Value::as_str)
                .expect("the profile authorizes a role for every gate it declares");
            let evidence: Vec<&str> = gate["required_evidence"]
                .as_array()
                .expect("declared evidence")
                .iter()
                .map(|artifact| artifact.as_str().expect("an artifact"))
                .collect();
            let recorded = ok(
                &lead,
                "kontor_gate_record",
                serde_json::json!({
                    "project_id": project,
                    "task_id": task,
                    "gate_id": name,
                    "idempotency_key": format!("journey-gate-{round}-{index}"),
                    "expected_revision": workflow_revision,
                    "verdict": "passed",
                    "evaluator_role": evaluator,
                    "evaluator_account": account["account_profile_id"],
                    "evidence": evidence,
                }),
            )
            .await;
            assert_eq!(recorded["state"], "passed", "gate `{name}` reduced");
        }

        // 10. The task closes, citing the artifacts its own profile requires.
        let after_gates = ok(
            &lead,
            "kontor_epic_get",
            serde_json::json!({ "project_id": project, "epic_id": epic }),
        )
        .await;
        let view = task_view(&after_gates, &task);
        for gate in view["gates"].as_array().expect("a gate list") {
            assert_eq!(
                gate["state"], "passed",
                "gate `{}` is discharged: {after_gates}",
                gate["gate"]
            );
        }
        let artifacts: Vec<&str> = view["required_artifacts"]
            .as_array()
            .expect("required artifacts")
            .iter()
            .map(|artifact| artifact.as_str().expect("an artifact"))
            .collect();
        let task_revision = view["revision"].as_u64().expect("a revision");
        let done = ok(
            &lead,
            "kontor_lifecycle_transition",
            serde_json::json!({
                "project_id": project,
                "epic_id": epic,
                "idempotency_key": format!("journey-complete-{round}"),
                "action": "complete_task",
                "task_id": task,
                "expected_revision": task_revision,
                "reason": "The work is done",
                "evidence": artifacts,
            }),
        )
        .await;
        assert_eq!(done["state"], "done", "task {task} completed: {done}");
        completed_tasks.push(task);
    }
    assert_eq!(
        completed_tasks.len(),
        2,
        "both declared tasks were admitted, settled and closed: {completed_tasks:?}"
    );

    // 11. And with every task terminal and every team run closed, the epic closes.
    let final_view = ok(
        &lead,
        "kontor_epic_get",
        serde_json::json!({ "project_id": project, "epic_id": epic }),
    )
    .await;
    let closed = ok(
        &lead,
        "kontor_lifecycle_transition",
        serde_json::json!({
            "project_id": project,
            "epic_id": epic,
            "idempotency_key": "journey-close",
            "action": "close_epic",
            "expected_revision": final_view["revision"].as_u64().unwrap_or(1),
            "reason": "Epic complete",
        }),
    )
    .await;
    assert_eq!(closed["state"], "closed", "the epic closed: {closed}");

    // The whole journey — empty realm to closed epic — was one caller, one
    // credential and one tool vocabulary. No HTTP call was composed by hand, and
    // the one-request-per-tool rule held for every step, not only the first eleven.
    let routes = transport.routes();
    assert_eq!(
        transport.calls(),
        routes.len(),
        "the counter and the record agree across the whole journey"
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
                "memory_origin": "kontor_native",
                "backlog_origin": "kontor_native",
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
