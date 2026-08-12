//! The full tool-to-tier matrix, and the claim that a refusal happens before a
//! request exists (KON-MVP-16).
//!
//! Every assertion here runs against `FakeTransport`, which records what it was
//! asked. That is what makes the central claim checkable rather than merely stated:
//! a real server only ever sees the requests that *were* sent, so proving an
//! observer's mutation attempt was never dispatched needs a transport that can be
//! asked "what did you receive" and answer "nothing". Binding a socket to find out
//! would break TST-001, and nothing in this workspace does.
//!
//! The mutants this suite exists to kill:
//!
//! * gating a mutation on the daemon's `forbidden` instead of the local gate, so an
//!   observer-configured server sends the write and merely has it refused;
//! * checking authority *after* arguments are parsed, so a malformed argument masks
//!   an authority refusal — or worse, a well-formed one gets dispatched;
//! * letting an operator reach an admin tool because the comparison is `!=` rather
//!   than a rank;
//! * answering an unknown tool with a plausible default instead of failing closed;
//! * serving a staged surface whose owning seam has not merged;
//! * a dry run that dispatches the effect it claims to be describing;
//! * an authority mismatch between a tool's declared tier and the daemon's own
//!   `command_authority` split.

use kontor_mcp::capability::{Denied, Gate};
use kontor_mcp::client::{CallerTier, Method, RealmClient};
use kontor_mcp::fake::FakeTransport;
use kontor_mcp::tools::{Effect, ToolSpec};
use serde_json::{Map, Value};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A client over a recording fake at one authority, and the fake itself.
fn world(tier: CallerTier) -> (RealmClient, Arc<FakeTransport>) {
    let fake = Arc::new(FakeTransport::new(tier));
    let client = RealmClient::new(Box::new(Arc::clone(&fake)));
    (client, fake)
}

/// Plausible operands for one tool, so the matrix can call every tool.
fn operands(tool: &ToolSpec) -> Map<String, Value> {
    let identifier = uuid::Uuid::now_v7().as_hyphenated().to_string();
    let mut arguments = Map::new();
    for property in tool.properties {
        if !property.required {
            continue;
        }
        let value = match property.kind {
            kontor_mcp::tools::PropertyKind::Integer => Value::from(1),
            kontor_mcp::tools::PropertyKind::Boolean => Value::from(false),
            kontor_mcp::tools::PropertyKind::TextArray => Value::Array(Vec::new()),
            kontor_mcp::tools::PropertyKind::Choice(values) => Value::from(values[0]),
            kontor_mcp::tools::PropertyKind::Text => Value::from(match property.name {
                "profile_key" => "delivery".to_owned(),
                "gate" => "review".to_owned(),
                "body" => "do the work".to_owned(),
                "permission_request_id" => "perm-1".to_owned(),
                "after" => "1:1".to_owned(),
                _ => identifier.clone(),
            }),
        };
        arguments.insert(property.name.to_owned(), value);
    }
    arguments
}

// ---------------------------------------------------------------------------
// The matrix
// ---------------------------------------------------------------------------

#[tokio::test]
async fn every_tool_is_admitted_at_its_own_tier_and_above_and_refused_below() {
    for tool in kontor_mcp::tools::catalogue() {
        for configured in CallerTier::ALL.iter().copied() {
            let (client, fake) = world(configured);
            let outcome =
                kontor_mcp::execute(&client, Gate::new(configured), tool.name, &operands(&tool))
                    .await;

            if configured.at_least(tool.tier) {
                assert!(
                    outcome.is_ok(),
                    "{} requires {} and {configured} reaches it, so it must be admitted: {:?}",
                    tool.name,
                    tool.tier,
                    outcome.err().map(|error| error.to_string())
                );
                assert!(
                    fake.dispatched() > 0,
                    "{} was admitted at {configured} and must have reached the daemon",
                    tool.name
                );
            } else {
                let failure = outcome.expect_err("a tool above this authority is refused");
                assert_eq!(
                    failure.code(),
                    "forbidden",
                    "{} refused at {configured} must report forbidden",
                    tool.name
                );
                assert_eq!(
                    fake.dispatched(),
                    0,
                    "{} was refused at {configured} and must not have been dispatched at all — \
                     not even to be refused by the daemon",
                    tool.name
                );
            }
        }
    }
}

#[tokio::test]
async fn an_observer_server_dispatches_no_write_of_any_kind() {
    // The single most important property of this crate, stated once over the whole
    // mutation surface rather than tool by tool.
    let (client, fake) = world(CallerTier::Observer);
    let mutations: Vec<ToolSpec> = kontor_mcp::tools::catalogue()
        .into_iter()
        .filter(|tool| tool.effect == Effect::Mutation)
        .collect();
    assert!(
        !mutations.is_empty(),
        "the catalogue must contain mutations, or this test proves nothing"
    );
    for tool in &mutations {
        let failure = kontor_mcp::execute(
            &client,
            Gate::new(CallerTier::Observer),
            tool.name,
            &operands(tool),
        )
        .await
        .expect_err("an observer may not mutate");
        assert_eq!(failure.code(), "forbidden", "{}", tool.name);
    }
    assert_eq!(
        fake.writes(),
        0,
        "an observer-configured server must never have produced a POST"
    );
    assert_eq!(
        fake.dispatched(),
        0,
        "and must not have produced any request at all"
    );
}

#[tokio::test]
async fn an_operator_cannot_reach_an_admin_tool() {
    let (client, fake) = world(CallerTier::Operator);
    for name in ["account_list", "account_show", "authorize_execution"] {
        let tool = kontor_mcp::tools::find(name).expect("an admin tool is served");
        assert_eq!(tool.tier, CallerTier::Admin, "{name} is an admin tool");
        let failure = kontor_mcp::execute(
            &client,
            Gate::new(CallerTier::Operator),
            name,
            &operands(&tool),
        )
        .await
        .expect_err("an operator may not reach an admin tool");
        assert_eq!(failure.code(), "forbidden", "{name}");
    }
    assert_eq!(
        fake.dispatched(),
        0,
        "an admin tool refused to an operator is refused before dispatch"
    );
}

#[tokio::test]
async fn authority_is_checked_before_arguments_are_read() {
    // The order matters: if arguments were validated first, this call would be
    // reported as a malformed argument and an operator reading the error would go
    // fix the argument rather than the authority.
    let (client, fake) = world(CallerTier::Observer);
    let mut nonsense = Map::new();
    nonsense.insert("project_id".to_owned(), Value::from("not-a-uuid"));
    nonsense.insert("an_undeclared_property".to_owned(), Value::from(1));

    let failure = kontor_mcp::execute(
        &client,
        Gate::new(CallerTier::Observer),
        "run_launch",
        &nonsense,
    )
    .await
    .expect_err("an observer may not launch");
    assert_eq!(
        failure.code(),
        "forbidden",
        "the authority refusal comes first, even when the arguments are also wrong"
    );
    assert_eq!(fake.dispatched(), 0);
}

#[tokio::test]
async fn an_unknown_or_staged_tool_fails_closed() {
    let (client, fake) = world(CallerTier::Admin);
    // Every one of these is either a surface `kontor_api::query::STAGED` names or a
    // capability nobody granted. All of them must be absent, at the highest
    // authority this build has.
    for name in [
        "ticket_comment_post",
        "ticket_comment_send",
        "intake_plan",
        "intake_approve",
        "intake_reject",
        "intake_replay",
        "calendar_assign",
        "calendar_show",
        "schedule_override_approve",
        "schedule_override_revoke",
        "holiday_list",
        "session_adopt",
        "workflow_spec_show",
        "runtime_credentials",
        "arm_everything",
        "",
        "RUN_LAUNCH",
    ] {
        let failure = kontor_mcp::execute(&client, Gate::new(CallerTier::Admin), name, &Map::new())
            .await
            .expect_err("an unserved tool is refused");
        assert_eq!(
            failure.code(),
            "not_found",
            "`{name}` must be absent rather than partially served"
        );
    }
    assert_eq!(
        fake.dispatched(),
        0,
        "an unknown tool never becomes a request"
    );
}

#[tokio::test]
async fn a_dry_run_describes_the_write_and_performs_none() {
    let (client, fake) = world(CallerTier::Operator);
    let tool = kontor_mcp::tools::find("run_launch").expect("the run_launch tool");
    let mut arguments = operands(&tool);
    arguments.insert("dry_run".to_owned(), Value::Bool(true));

    let envelope = kontor_mcp::execute(
        &client,
        Gate::new(CallerTier::Operator),
        "run_launch",
        &arguments,
    )
    .await
    .expect("a dry run succeeds");

    assert_eq!(
        envelope.data["dry_run"],
        Value::Bool(true),
        "a dry run says so in its answer"
    );
    assert_eq!(
        envelope.data["request"]["path"],
        Value::from("/v1/commands/launch_run"),
        "and describes the request it would have made"
    );
    assert_eq!(
        envelope.receipt,
        Value::Null,
        "a dry run produced no receipt, because it recorded nothing"
    );
    assert!(
        !envelope.realm_id.is_empty(),
        "a dry run is still realm-qualified"
    );
    assert_eq!(
        fake.writes(),
        0,
        "a dry run must not have dispatched the write it described"
    );
    // The one request it may make is the realm identity read that qualifies the
    // answer. Anything else means the dry run did more than describe.
    let dispatched = fake.recorded();
    assert!(
        dispatched
            .iter()
            .all(|request| request.method == Method::Get && request.path == "/v1/realm"),
        "a dry run reads the realm identity and nothing else, but dispatched {:?}",
        dispatched
            .iter()
            .map(|request| request.path.clone())
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_mutation_that_is_not_a_dry_run_does_dispatch_its_write() {
    // The complement of the test above: if `dry_run` were ignored in the other
    // direction, every write would silently become a description.
    let (client, fake) = world(CallerTier::Operator);
    let tool = kontor_mcp::tools::find("run_launch").expect("the run_launch tool");
    kontor_mcp::execute(
        &client,
        Gate::new(CallerTier::Operator),
        "run_launch",
        &operands(&tool),
    )
    .await
    .expect("a launch is recorded");
    assert_eq!(
        fake.writes(),
        1,
        "a real mutation reaches the daemon exactly once"
    );
    let recorded = fake.recorded();
    let write = recorded
        .iter()
        .find(|request| request.method == Method::Post)
        .expect("the write was recorded");
    assert!(
        write.idempotency_key.is_some(),
        "every mutation carries an idempotency key"
    );
}

#[tokio::test]
async fn a_read_is_a_get_and_carries_no_idempotency_key() {
    // A read that carried a key would look like a mutation to anything counting
    // them, and a read committed under a key is a contradiction.
    let (client, fake) = world(CallerTier::Admin);
    for tool in kontor_mcp::tools::catalogue()
        .into_iter()
        .filter(|tool| tool.effect != Effect::Mutation)
    {
        kontor_mcp::execute(
            &client,
            Gate::new(CallerTier::Admin),
            tool.name,
            &operands(&tool),
        )
        .await
        .unwrap_or_else(|error| panic!("{} is a read and should succeed: {error}", tool.name));
    }
    for request in fake.recorded() {
        assert_eq!(
            request.method,
            Method::Get,
            "{} is a read and must be a GET",
            request.path
        );
        assert!(
            request.idempotency_key.is_none(),
            "{} is a read and must carry no idempotency key",
            request.path
        );
    }
}

#[tokio::test]
async fn a_gate_refusal_names_both_authorities() {
    let refusal = Gate::new(CallerTier::Observer)
        .admit("account_list", CallerTier::Admin)
        .expect_err("an observer may not reach an admin tool");
    assert_eq!(
        refusal,
        Denied::Authority {
            tool: "account_list".to_owned(),
            required: CallerTier::Admin,
            configured: CallerTier::Observer,
        },
        "an operator must be able to see what to reconfigure without reading the source"
    );
}

#[tokio::test]
async fn the_served_list_matches_what_the_gate_would_admit() {
    // Otherwise a server could list a tool it refuses, or refuse one it lists.
    for configured in CallerTier::ALL.iter().copied() {
        let (client, _fake) = world(configured);
        let server = kontor_mcp::server::KontorMcp::new(client);
        let served: Vec<&str> = server.served().iter().map(|tool| tool.name).collect();
        for tool in kontor_mcp::tools::catalogue() {
            let admitted = Gate::new(configured).admit(tool.name, tool.tier).is_ok();
            assert_eq!(
                served.contains(&tool.name),
                admitted,
                "{} is {} at {configured} authority but the other way round in the served list",
                tool.name,
                if admitted { "admitted" } else { "refused" }
            );
        }
    }
}

#[tokio::test]
async fn no_observer_tool_can_be_built_as_a_write() {
    // Stated over dispatch rather than over the declaration, so a tool that declared
    // itself a query and built a POST would be caught.
    let (client, fake) = world(CallerTier::Observer);
    for tool in kontor_mcp::tools::catalogue()
        .into_iter()
        .filter(|tool| tool.tier == CallerTier::Observer)
    {
        kontor_mcp::execute(
            &client,
            Gate::new(CallerTier::Observer),
            tool.name,
            &operands(&tool),
        )
        .await
        .unwrap_or_else(|error| panic!("{} is an observer tool: {error}", tool.name));
    }
    assert_eq!(
        fake.writes(),
        0,
        "an observer tool that produced a POST would be a mutation wearing a read's tier"
    );
}
