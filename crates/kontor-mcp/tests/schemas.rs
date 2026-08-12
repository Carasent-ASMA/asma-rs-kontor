//! What may and may not appear in a tool schema (KON-MVP-16).
//!
//! Every tool's JSON Schema is *derived* from its property declaration, so scanning
//! the schemas is scanning the whole accepted surface — there is no second place an
//! operand could be smuggled in. That is what makes a disclosure test meaningful
//! here rather than merely suggestive.
//!
//! The scans are stated as vocabularies rather than as exact property names, because
//! the mistake being prevented is not a specific field. It is somebody adding a
//! plausible-looking operand — `endpoint`, `api_key`, `assignee`, `auto_arm` — that
//! turns a narrow control-plane tool into a way to drive a foreign system or to hold
//! a standing permission.
//!
//! The mutants this suite exists to kill:
//!
//! * a runtime endpoint, `CODEX_HOME` or host target becoming an operand, so a tool
//!   could point a session at an arbitrary address;
//! * a credential, token or environment *value* becoming an operand;
//! * an outbound ticket comment, or a free-choice external status, transition or
//!   assignee, so the control plane starts relaying arbitrary foreign workflow;
//! * an unbounded or self-renewing arming control;
//! * `additionalProperties` drifting to `true`, which would reopen all of the above
//!   at once;
//! * a description that leaks a value instead of describing a parameter.

use kontor_mcp::tools::{Effect, PropertyKind};
use serde_json::Value;

/// Words that name *where* a runtime is, or *how* to authenticate to one.
///
/// None of these belongs on this surface at all: the daemon owns every runtime
/// endpoint and every credential, and a tool that could name one would be a way
/// around it.
const RUNTIME_AND_SECRET: &[&str] = &[
    "endpoint",
    "base_url",
    "host",
    "port",
    "url",
    "uri",
    "address",
    "socket",
    "token",
    "secret",
    "password",
    "api_key",
    "apikey",
    "bearer",
    "authorization",
    "credential",
    "cookie",
    "codex_home",
    "config_home",
    "env_value",
    "environment_value",
    "executable",
    "command_line",
    "argv",
    "state_root",
    "database",
    "sqlite",
];

/// Words that would let a caller drive a foreign ticketing system directly.
///
/// A control plane that relays an arbitrary external status, transition or assignee
/// has no opinion about what it is doing — and an outbound comment is a message
/// this surface must never be able to post on somebody's behalf.
const FOREIGN_WORKFLOW: &[&str] = &[
    "comment",
    "comment_body",
    "assignee",
    "assign_to",
    "transition",
    "transition_id",
    "external_status",
    "status_name",
    "jira",
    "issue_key",
    "webhook",
];

/// Words that would turn a bounded grant into a standing one.
const UNBOUNDED_ARMING: &[&str] = &[
    "auto_arm",
    "autoarm",
    "always_arm",
    "arm_all",
    "renew",
    "recurring",
    "forever",
    "unbounded",
    "indefinite",
    "all_projects",
    "every_task",
    "disable_guardrails",
    "force",
    "skip_gate",
    "bypass",
];

/// Every property name in every served schema, as `(tool, property)`.
fn every_property() -> Vec<(&'static str, String)> {
    let mut found = Vec::new();
    for tool in kontor_mcp::tools::catalogue() {
        let schema = tool.input_schema();
        let properties = schema["properties"]
            .as_object()
            .expect("every schema declares a properties object")
            .clone();
        for name in properties.keys() {
            found.push((tool.name, name.clone()));
        }
    }
    found
}

/// Whether `name` contains any word in `vocabulary`, comparing loosely.
///
/// Loosely on purpose: `apiKey`, `api-key` and `api_key` are the same mistake, and a
/// scan that only matched one spelling would be a scan somebody routes around
/// without meaning to.
fn mentions<'a>(name: &str, vocabulary: &[&'a str]) -> Option<&'a str> {
    let normalized: String = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    vocabulary.iter().copied().find(|word| {
        let target: String = word
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .collect();
        normalized.contains(&target)
    })
}

#[test]
fn no_schema_accepts_a_runtime_endpoint_or_a_credential() {
    for (tool, property) in every_property() {
        assert!(
            mentions(&property, RUNTIME_AND_SECRET).is_none(),
            "tool `{tool}` accepts `{property}`, which names a runtime location or a credential. \
             Those live in the daemon; this surface must not be able to choose one."
        );
    }
}

#[test]
fn no_schema_accepts_an_outbound_comment_or_a_foreign_workflow_value() {
    for (tool, property) in every_property() {
        assert!(
            mentions(&property, FOREIGN_WORKFLOW).is_none(),
            "tool `{tool}` accepts `{property}`, which would let a caller drive an external \
             ticketing system directly. Ticket convergence is staged (KON-MVP-14/21) and is not \
             reached from here."
        );
    }
}

#[test]
fn no_schema_accepts_an_unbounded_or_self_renewing_control() {
    for (tool, property) in every_property() {
        assert!(
            mentions(&property, UNBOUNDED_ARMING).is_none(),
            "tool `{tool}` accepts `{property}`, which would make a bounded decision a standing \
             one, or would let a caller step around a guardrail."
        );
    }
}

#[test]
fn every_schema_is_closed() {
    // One `additionalProperties: true` would reopen every vocabulary above at once,
    // because an undeclared property would be accepted and forwarded.
    for tool in kontor_mcp::tools::catalogue() {
        let schema = tool.input_schema();
        assert_eq!(
            schema["additionalProperties"],
            Value::Bool(false),
            "{} must not accept an undeclared property",
            tool.name
        );
        assert_eq!(
            schema["type"],
            Value::from("object"),
            "{} must take an arguments object",
            tool.name
        );
    }
}

#[test]
fn every_property_is_described_and_no_description_carries_a_value() {
    for tool in kontor_mcp::tools::catalogue() {
        let schema = tool.input_schema();
        let properties = schema["properties"]
            .as_object()
            .expect("a properties object")
            .clone();
        for (name, fragment) in &properties {
            let description = fragment["description"]
                .as_str()
                .unwrap_or_else(|| panic!("{}.{name} must be described", tool.name));
            assert!(
                description.len() > 10,
                "{}.{name} needs a description a caller can act on, not `{description}`",
                tool.name
            );
            // A description reaches a language model. One that contained a token,
            // an endpoint or a path would be teaching a caller a value rather than
            // a parameter.
            for forbidden in [
                "http://",
                "https://",
                "Bearer ",
                "sk-",
                "/Users/",
                "127.0.0.1",
            ] {
                assert!(
                    !description.contains(forbidden),
                    "{}.{name} describes itself with `{forbidden}`, which is a value and not a \
                     parameter",
                    tool.name
                );
            }
        }
    }
}

#[test]
fn a_closed_domain_set_is_offered_as_a_choice_and_an_open_key_is_not() {
    // The distinction is load-bearing. A gate verdict comes from a closed domain
    // enum, so enumerating it helps a caller. A gate *key* is deployment data, and
    // enumerating it would refuse every gate a deployment added after this build.
    let verdict = kontor_mcp::tools::find("gate_verdict").expect("the gate_verdict tool");
    let by_name = |wanted: &str| {
        verdict
            .properties
            .iter()
            .find(|property| property.name == wanted)
            .unwrap_or_else(|| panic!("gate_verdict declares {wanted}"))
    };
    assert!(
        matches!(by_name("verdict").kind, PropertyKind::Choice(_)),
        "a gate verdict is a closed domain set and is offered as one"
    );
    assert_eq!(
        by_name("gate").kind,
        PropertyKind::Text,
        "a gate key is an open deployment key and must not be enumerated"
    );

    let profile = kontor_mcp::tools::find("profile_show").expect("the profile_show tool");
    let key = profile
        .properties
        .iter()
        .find(|property| property.name == "profile_key")
        .expect("profile_show declares profile_key");
    assert_eq!(
        key.kind,
        PropertyKind::Text,
        "a work-profile key is deployment data; no seeded pack is built into this surface"
    );
}

#[test]
fn a_control_plane_cursor_is_an_integer_and_a_content_anchor_is_text() {
    // The two cursor spaces are spelled differently in the schemas as well as on
    // the routes, so a caller cannot pass one where the other belongs and a model
    // reading the schema is told which is which.
    let events = kontor_mcp::tools::find("events_replay").expect("the events_replay tool");
    let after = events
        .properties
        .iter()
        .find(|property| property.name == "after")
        .expect("events_replay resumes from a cursor");
    assert_eq!(
        after.kind,
        PropertyKind::Integer,
        "a control-plane cursor is an integer this realm allocated"
    );

    let timeline = kontor_mcp::tools::find("session_timeline").expect("the session_timeline tool");
    let after = timeline
        .properties
        .iter()
        .find(|property| property.name == "after")
        .expect("session_timeline resumes from a cursor");
    assert_eq!(
        after.kind,
        PropertyKind::Text,
        "a content position is the runtime's own opaque cursor"
    );
}

#[test]
fn every_mutation_declares_both_of_the_operands_that_make_it_safe() {
    for tool in kontor_mcp::tools::catalogue()
        .into_iter()
        .filter(|tool| tool.effect == Effect::Mutation)
    {
        let names: Vec<&str> = tool.properties.iter().map(|p| p.name).collect();
        assert!(
            names.contains(&"idempotency_key"),
            "{} must be replayable, or a retry becomes a second effect",
            tool.name
        );
        assert!(
            names.contains(&"dry_run"),
            "{} must be inspectable without being performed",
            tool.name
        );
    }
}

#[test]
fn every_control_plane_write_demands_the_revision_it_was_computed_against() {
    // A write with no revision is a write over a state nobody read. The session
    // writes are the deliberate exception: they are keyed on the runtime's own
    // message ledger rather than on a control-plane aggregate revision.
    for tool in kontor_mcp::tools::catalogue()
        .into_iter()
        .filter(|tool| tool.effect == Effect::Mutation)
        .filter(|tool| !tool.name.starts_with("session_"))
    {
        let revision = tool
            .properties
            .iter()
            .find(|property| property.name == "expected_revision")
            .unwrap_or_else(|| {
                panic!(
                    "{} writes to a control-plane aggregate and must name its revision",
                    tool.name
                )
            });
        assert!(
            revision.required,
            "{}'s revision must be required, not optional",
            tool.name
        );
        assert_eq!(revision.kind, PropertyKind::Integer);
    }
}

#[test]
fn no_staged_surface_is_served() {
    // `kontor_api::query::STAGED` is the list and this crate cannot depend on it, so
    // what is checked here is the property that matters: none of those surfaces is
    // reachable. `tests/capability_matrix.rs` checks the same thing by calling them.
    //
    // The vocabulary is deliberately about the *staged* seams only. Ticket evidence,
    // the scheduling plan and session discovery are wired now, so their names are
    // expected to appear — what must not appear is an intake proposal, a calendar
    // window, an outbound comment or an adoption.
    let served: Vec<&str> = kontor_mcp::tools::catalogue()
        .iter()
        .map(|tool| tool.name)
        .collect();
    for staged in [
        "intake",
        "calendar",
        "override",
        "holiday",
        "adopt",
        "comment_post",
        "comment_send",
        "workflow_spec",
        "field_spec",
    ] {
        assert!(
            !served.iter().any(|name| name.contains(staged)),
            "`{staged}` names a staged seam and must not appear in a served tool name"
        );
    }
}

#[test]
fn the_newly_wired_surfaces_are_served() {
    // The complement, and the reason this file is not just a deny-list: a surface
    // whose seam is merged must actually be reachable, or staging it was the bug.
    for wired in [
        "project_list",
        "mission_list",
        "run_list",
        "scheduler_plan",
        "ticket_list",
        "ticket_show",
        "ticket_comments",
        "ticket_transitions",
        "ticket_sync",
        "ticket_assign",
        "ticket_transition",
        "ticket_resolve_conflict",
        "session_discover",
    ] {
        assert!(
            kontor_mcp::tools::find(wired).is_some(),
            "`{wired}` has a merged seam behind it and must be served"
        );
    }
}

#[test]
fn no_ticket_command_can_name_an_external_status_assignee_or_transition() {
    // The single most important disclosure rule on the ticket surface: converging
    // means making the external ticket match what this realm decided, so a caller
    // has nothing to choose. An operand here would turn a projection into a remote
    // control.
    for tool in kontor_mcp::tools::catalogue()
        .into_iter()
        .filter(|tool| tool.name.starts_with("ticket_"))
    {
        for property in tool.properties {
            for forbidden in [
                "status",
                "transition_id",
                "assignee",
                "account_id",
                "comment",
                "body",
                "fields",
                "milestone",
            ] {
                assert_ne!(
                    property.name, forbidden,
                    "{} must not accept `{forbidden}`: the target is computed from the stored \
                     projection and the pinned specification, never chosen by a caller",
                    tool.name
                );
            }
        }
    }
}
