//! Operations that change something. Every one of them requires operator
//! authority, carries an idempotency key and can be planned without being
//! performed.
//!
//! # Three properties every mutation here has
//!
//! * **A revision.** A write names the revision it was computed against, so a
//!   caller working from a stale read is told the current one and nothing is
//!   mutated. There is no "force" operand, and adding one would be adding a way to
//!   write over a state nobody looked at.
//! * **A key.** The `Idempotency-Key` is what makes a retry a replay: the same key
//!   with the same intent returns the receipt that was already durable. A caller
//!   that names none gets a fresh one, which is right for a first attempt and
//!   exactly wrong for a retry — so the operand exists.
//! * **A dry run.** `dry_run` returns the request that would be sent and sends
//!   nothing. It is not a server round trip: the check is the domain's own
//!   compatibility matrix, run on this machine, so a dry run of an illegal command
//!   fails without the daemon being asked.
//!
//! # What is deliberately not here
//!
//! Ticket synchronisation, assignment and transition; intake approval; calendar
//! assignment and schedule overrides. Every one of those is a command kind the
//! daemon already accepts, and not one of them is usable: nothing in this build can
//! *read* a ticket link, an intake proposal or a calendar assignment, so a caller
//! could not discover the identifier or the revision the command needs. Exposing a
//! write whose target cannot be found would be exposing a way to guess.
//!
//! Ticket transitions carry a second reason. Their operands would be an external
//! status, an assignee and a transition name — values this surface must never let a
//! caller choose freely, because a control plane that relays arbitrary external
//! workflow values is a control plane with no opinion about what it is doing.

use kontor_core::receipt::{AggregateRef, CommandKind};

use crate::client::{CallerTier, Request};
use crate::tools::{
    DRY_RUN, Effect, IDEMPOTENCY_KEY, Plan, Property, ToolSpec, command_request, intent,
};

/// The operands every run-lifecycle command takes.
const RUN_COMMAND: &[Property] = &[
    Property::required(
        "project_id",
        "The project that owns the aggregate being written to.",
    ),
    Property::required("agent_run_id", "The agent run to act on."),
    Property::number(
        "expected_revision",
        "The run's current revision, as a read returned it. The command is refused if the run has \
         moved since.",
    ),
    Property::optional("reason", "Why, recorded in the command's intent document."),
    IDEMPOTENCY_KEY,
    DRY_RUN,
];

/// Ask for a run to be launched.
const LAUNCH: ToolSpec = ToolSpec {
    name: "run_launch",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record the intent to launch one agent run. This records a desired state and a \
                  durable receipt; it is not a confirmation that the runtime started anything. \
                  Read the run afterwards to see what the runtime reported.",
    properties: RUN_COMMAND,
    build: |operands| {
        command_request(
            operands,
            CommandKind::LaunchRun,
            operands.project_id()?,
            AggregateRef::AgentRun {
                agent_run_id: operands.agent_run_id()?,
            },
            operands.expected_revision()?,
            intent(operands.optional_text("reason"), &[]),
        )
    },
};

/// Ask for a run to be cancelled.
const CANCEL: ToolSpec = ToolSpec {
    name: "run_cancel",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record the intent to cancel one agent run. An acknowledgement is not a \
                  completion: the run is closed only when the runtime confirms it.",
    properties: RUN_COMMAND,
    build: |operands| {
        command_request(
            operands,
            CommandKind::CancelRun,
            operands.project_id()?,
            AggregateRef::AgentRun {
                agent_run_id: operands.agent_run_id()?,
            },
            operands.expected_revision()?,
            intent(operands.optional_text("reason"), &[]),
        )
    },
};

/// Ask for a run to be parked.
const PARK: ToolSpec = ToolSpec {
    name: "run_park",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record the intent to park one agent run, holding it without a runtime verdict.",
    properties: RUN_COMMAND,
    build: |operands| {
        command_request(
            operands,
            CommandKind::ParkRun,
            operands.project_id()?,
            AggregateRef::AgentRun {
                agent_run_id: operands.agent_run_id()?,
            },
            operands.expected_revision()?,
            intent(operands.optional_text("reason"), &[]),
        )
    },
};

/// Ask for a run to be abandoned.
const ABANDON: ToolSpec = ToolSpec {
    name: "run_abandon",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record the intent to abandon one agent run without a runtime verdict. Use this \
                  when the runtime cannot be reached and the operator is deciding the outcome; the \
                  reason is recorded because nothing else will explain it later.",
    properties: RUN_COMMAND,
    build: |operands| {
        command_request(
            operands,
            CommandKind::AbandonRun,
            operands.project_id()?,
            AggregateRef::AgentRun {
                agent_run_id: operands.agent_run_id()?,
            },
            operands.expected_revision()?,
            intent(operands.optional_text("reason"), &[]),
        )
    },
};

/// Return a held task to `ready`.
const RESUME: ToolSpec = ToolSpec {
    name: "task_resume",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record the intent to return one blocked, parked or human-held task to ready, so \
                  scheduling may consider it again.",
    properties: &[
        Property::required(
            "project_id",
            "The project that owns the aggregate being written to.",
        ),
        Property::required(
            "task_id",
            "The task to return to ready, as a canonical identifier.",
        ),
        Property::number(
            "expected_revision",
            "The task's current revision, as a read returned it.",
        ),
        Property::optional("reason", "Why, recorded in the command's intent document."),
        IDEMPOTENCY_KEY,
        DRY_RUN,
    ],
    build: |operands| {
        command_request(
            operands,
            CommandKind::ResumeTask,
            operands.project_id()?,
            AggregateRef::Task {
                task_id: operands.task_id()?,
            },
            operands.expected_revision()?,
            intent(operands.optional_text("reason"), &[]),
        )
    },
};

/// The verdicts a gate can be given.
///
/// The set mirrors `kontor_core::state::GateVerdict`, which is a closed domain
/// enum: an unknown spelling is refused by the daemon anyway, and enumerating them
/// here means a caller is told the options instead of discovering them by being
/// refused. `the_gate_verdicts_are_the_domains_own` below holds the two lists to
/// each other, so a new domain verdict cannot go unoffered.
const GATE_VERDICTS: &[&str] = &["started", "passed", "rejected", "waived", "parked"];

/// Record a gate verdict.
const GATE_VERDICT: ToolSpec = ToolSpec {
    name: "gate_verdict",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record a verdict on one of a task's gates, citing the artifacts that are its \
                  evidence. The gate key is deployment data from the task's frozen work profile: \
                  read the task's gates first to see which keys exist and what state each is in.",
    properties: &[
        Property::required(
            "project_id",
            "The project that owns the aggregate being written to.",
        ),
        Property::required(
            "task_id",
            "The task whose gate is being judged, as a canonical identifier.",
        ),
        Property::number(
            "expected_revision",
            "The task's current revision, as a read returned it.",
        ),
        Property::required("gate", "The gate key, as the work profile spells it."),
        Property::choice(
            "verdict",
            GATE_VERDICTS,
            "The verdict to record against the gate.",
        ),
        Property::optional_list(
            "evidence",
            "The artifact keys cited as evidence for this verdict.",
        ),
        Property::optional("reason", "Why, recorded in the command's intent document."),
        IDEMPOTENCY_KEY,
        DRY_RUN,
    ],
    build: |operands| {
        let gate = operands.gate_key()?;
        let evidence: Vec<serde_json::Value> = operands
            .list("evidence")
            .into_iter()
            .map(serde_json::Value::String)
            .collect();
        command_request(
            operands,
            CommandKind::RecordGateVerdict,
            operands.project_id()?,
            AggregateRef::Task {
                task_id: operands.task_id()?,
            },
            operands.expected_revision()?,
            intent(
                operands.optional_text("reason"),
                &[
                    ("gate", serde_json::Value::String(gate.as_str().to_owned())),
                    (
                        "verdict",
                        serde_json::Value::String(operands.opaque("verdict").to_owned()),
                    ),
                    ("evidence", serde_json::Value::Array(evidence)),
                ],
            ),
        )
    },
};

/// Deliver a message into a live session.
const MESSAGE: ToolSpec = ToolSpec {
    name: "session_message",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Deliver one follow-up message into a running session. The idempotency key is \
                  also the stable client message id the runtime keys the effect on, so repeating \
                  it replays the original acknowledgement instead of sending a second message. A \
                  generated key is a canonical UUID v7, which is what that route requires.",
    properties: &[
        Property::required("agent_run_id", "The agent run whose session to write to."),
        Property::required("body", "The text to deliver into the running session."),
        IDEMPOTENCY_KEY,
        DRY_RUN,
    ],
    build: |operands| {
        let request = Request::post(
            format!("/v1/sessions/{}/messages", operands.agent_run_id()?),
            serde_json::json!({ "body": operands.opaque("body") }),
        )
        .with_key(operands.session_key()?);
        Ok(Plan::of(request).dry(operands.dry_run()))
    },
};

/// The two ways a permission request can be answered.
///
/// Mirrors `kontor_runtime::request::PermissionDecision`, which this crate does not
/// depend on: `kontor-runtime` is the runtime port, and a CLI that linked it would be
/// one careless line from instantiating an adapter. The two spellings are therefore
/// kept by hand — the set has been `allow`/`deny` since the port was written, and a
/// third would be a `kontor-runtime` change that this list would have to follow.
const DECISIONS: &[&str] = &["allow", "deny"];

/// Answer a permission request raised inside a session.
const PERMISSION: ToolSpec = ToolSpec {
    name: "session_permission",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Answer one permission request a running session raised. The request id is the \
                  runtime's own, read from the session's content. An identical retry is answered \
                  with the original acknowledgement; a contradictory one is a conflict.",
    properties: &[
        Property::required("agent_run_id", "The agent run whose session raised it."),
        Property::required(
            "permission_request_id",
            "The runtime's own id for the request being answered.",
        ),
        Property::choice("decision", DECISIONS, "The answer to apply."),
        IDEMPOTENCY_KEY,
        DRY_RUN,
    ],
    build: |operands| {
        let request = Request::post(
            format!(
                "/v1/sessions/{}/permissions/{}",
                operands.agent_run_id()?,
                operands.external_id("permission_request_id")?
            ),
            serde_json::json!({ "decision": operands.opaque("decision") }),
        )
        .with_key(operands.session_key()?);
        Ok(Plan::of(request).dry(operands.dry_run()))
    },
};

/// The operands every ticket convergence command takes.
///
/// Note what is **not** here: no external status, no transition id, no assignee, no
/// comment. "Converge" means make the external ticket match what this realm already
/// decided, so there is nothing for a caller to choose — the target is computed from
/// the stored projection and the pinned external-workflow specification, by the
/// daemon. A caller that could name a status would be driving Jira through Kontor
/// rather than projecting Kontor into Jira, and those are different products.
const TICKET_COMMAND: &[Property] = &[
    Property::required("project_id", "The project that owns the ticket link."),
    Property::required("link_id", "The ticket link to converge."),
    Property::number(
        "expected_revision",
        "The ticket link's current revision, as `ticket_show` or `ticket_list` returned it.",
    ),
    Property::optional("reason", "Why, recorded in the command's intent document."),
    IDEMPOTENCY_KEY,
    DRY_RUN,
];

/// Write this Realm's projection to the external ticket.
const TICKET_SYNC: ToolSpec = ToolSpec {
    name: "ticket_sync",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record the intent to write this realm's computed projection to one external \
                  ticket. The fields written are the projection `ticket_show` reports; this \
                  command carries no field values of its own. The answer is a durable receipt, \
                  not a confirmation that the external system changed.",
    properties: TICKET_COMMAND,
    build: |operands| {
        command_request(
            operands,
            CommandKind::SyncTicket,
            operands.project_id()?,
            AggregateRef::TicketLink {
                link_id: operands.ticket_link_id()?,
            },
            operands.expected_revision()?,
            intent(operands.optional_text("reason"), &[]),
        )
    },
};

/// Converge the external ticket's assignee.
const TICKET_ASSIGN: ToolSpec = ToolSpec {
    name: "ticket_assign",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record the intent to converge one external ticket's assignee to the principal \
                  this realm's projection names. There is deliberately no operand for who that \
                  is: the assignee follows from the projection, and a command that could name an \
                  arbitrary account would be reassigning somebody else's ticket.",
    properties: TICKET_COMMAND,
    build: |operands| {
        command_request(
            operands,
            CommandKind::AssignTicket,
            operands.project_id()?,
            AggregateRef::TicketLink {
                link_id: operands.ticket_link_id()?,
            },
            operands.expected_revision()?,
            intent(operands.optional_text("reason"), &[]),
        )
    },
};

/// Converge the external ticket's status.
const TICKET_TRANSITION: ToolSpec = ToolSpec {
    name: "ticket_transition",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record the intent to converge one external ticket's status to the milestone \
                  this realm's workflow has reached. There is deliberately no operand for a \
                  target status or a transition name: which transition applies is read from the \
                  pinned external-workflow specification, and a caller that could choose one \
                  would be moving a ticket to a state the internal work is not in.",
    properties: TICKET_COMMAND,
    build: |operands| {
        command_request(
            operands,
            CommandKind::TransitionTicket,
            operands.project_id()?,
            AggregateRef::TicketLink {
                link_id: operands.ticket_link_id()?,
            },
            operands.expected_revision()?,
            intent(operands.optional_text("reason"), &[]),
        )
    },
};

/// Resolve one detected status conflict.
const TICKET_RESOLVE: ToolSpec = ToolSpec {
    name: "ticket_resolve_conflict",
    tier: CallerTier::Operator,
    effect: Effect::Mutation,
    description: "Record the intent to resolve one conflict `ticket_show` reported. The conflict \
                  is named by this realm's own identifier, so resolving one is an operator \
                  decision about a specific detected disagreement rather than a way to clear \
                  every conflict at once.",
    properties: &[
        Property::required("project_id", "The project that owns the ticket link."),
        Property::required("link_id", "The ticket link the conflict was detected on."),
        Property::required(
            "conflict_id",
            "The conflict to resolve, as `ticket_show` reported it.",
        ),
        Property::number(
            "expected_revision",
            "The ticket link's current revision, as a read returned it.",
        ),
        Property::optional("reason", "Why, recorded in the command's intent document."),
        IDEMPOTENCY_KEY,
        DRY_RUN,
    ],
    build: |operands| {
        let conflict = operands.conflict_id()?;
        command_request(
            operands,
            CommandKind::ResolveStatusConflict,
            operands.project_id()?,
            AggregateRef::TicketLink {
                link_id: operands.ticket_link_id()?,
            },
            operands.expected_revision()?,
            intent(
                operands.optional_text("reason"),
                &[("conflict", serde_json::Value::String(conflict.to_string()))],
            ),
        )
    },
};

/// Every operation that changes something at operator authority.
#[must_use]
pub const fn tools() -> &'static [ToolSpec] {
    &[
        ABANDON,
        CANCEL,
        GATE_VERDICT,
        LAUNCH,
        MESSAGE,
        PARK,
        PERMISSION,
        RESUME,
        TICKET_ASSIGN,
        TICKET_RESOLVE,
        TICKET_SYNC,
        TICKET_TRANSITION,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::find;

    fn arguments(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().expect("an arguments object").clone()
    }

    #[test]
    fn the_gate_verdicts_are_the_domains_own() {
        // The offered set and the domain's closed enum must be the same set. If they
        // drift, a caller is either offered a verdict the daemon refuses or denied
        // one it would accept — and both are found here rather than in production.
        let domain: Vec<&str> = kontor_core::state::GateVerdict::ALL
            .iter()
            .map(|verdict| verdict.as_str())
            .collect();
        assert_eq!(
            GATE_VERDICTS.to_vec(),
            domain,
            "the verdicts this tool offers are exactly kontor_core::state::GateVerdict"
        );
    }

    #[test]
    fn the_permission_decisions_are_the_two_the_runtime_port_has() {
        // Mirrored by hand, so it is asserted by hand. The runtime port is not a
        // dependency of this crate on purpose; see `DECISIONS`.
        assert_eq!(DECISIONS, &["allow", "deny"]);
    }

    #[test]
    fn every_operator_tool_is_a_mutation_at_operator_authority() {
        for tool in tools() {
            assert_eq!(tool.effect, Effect::Mutation, "{} must write", tool.name);
            assert_eq!(
                tool.tier,
                CallerTier::Operator,
                "{} is in the operator module and must require operator authority",
                tool.name
            );
        }
    }

    #[test]
    fn a_run_command_names_its_own_kind_and_never_another() {
        for (name, path) in [
            ("run_launch", "/v1/commands/launch_run"),
            ("run_cancel", "/v1/commands/cancel_run"),
            ("run_park", "/v1/commands/park_run"),
            ("run_abandon", "/v1/commands/abandon_run"),
        ] {
            let plan = find(name)
                .expect("the tool is served")
                .plan(&arguments(serde_json::json!({
                    "project_id": "0192f0c0-0000-7000-8000-000000000001",
                    "agent_run_id": "0192f0c0-0000-7000-8000-000000000002",
                    "expected_revision": 4
                })))
                .expect("the command plans");
            assert_eq!(plan.request.path, path);
        }
    }

    #[test]
    fn a_stale_revision_is_carried_and_a_zero_one_is_refused() {
        let launch = find("run_launch").expect("the run_launch tool");
        let plan = launch
            .plan(&arguments(serde_json::json!({
                "project_id": "0192f0c0-0000-7000-8000-000000000001",
                "agent_run_id": "0192f0c0-0000-7000-8000-000000000002",
                "expected_revision": 9
            })))
            .expect("the command plans");
        assert_eq!(
            plan.request.body.as_ref().expect("a body")["expected_revision"],
            serde_json::json!(9),
            "the caller's revision reaches the daemon unchanged, because it is what the \
             compare-and-swap compares"
        );

        // Zero is what an uninitialized field looks like, and a revision counts
        // from one. Accepting it would send a write computed against nothing.
        assert!(
            launch
                .plan(&arguments(serde_json::json!({
                    "project_id": "0192f0c0-0000-7000-8000-000000000001",
                    "agent_run_id": "0192f0c0-0000-7000-8000-000000000002",
                    "expected_revision": 0
                })))
                .is_err(),
            "a zero revision is refused before dispatch"
        );
    }

    #[test]
    fn a_gate_verdict_carries_its_gate_verdict_and_evidence_in_the_intent() {
        let plan = find("gate_verdict")
            .expect("the gate_verdict tool")
            .plan(&arguments(serde_json::json!({
                "project_id": "0192f0c0-0000-7000-8000-000000000001",
                "task_id": "0192f0c0-0000-7000-8000-000000000002",
                "expected_revision": 2,
                "gate": "code-review",
                "verdict": "rejected",
                "evidence": ["diff", "test-report"],
                "reason": "the migration is missing"
            })))
            .expect("the verdict plans");
        let body = plan.request.body.as_ref().expect("a body");
        assert_eq!(body["intent"]["gate"], serde_json::json!("code-review"));
        assert_eq!(body["intent"]["verdict"], serde_json::json!("rejected"));
        assert_eq!(
            body["intent"]["evidence"],
            serde_json::json!(["diff", "test-report"])
        );
        assert_eq!(
            body["intent"]["reason"],
            serde_json::json!("the migration is missing")
        );
        assert_eq!(
            body["intent"]["schema_version"],
            serde_json::json!(1),
            "an intent is a canonical document and must declare its generation"
        );
    }

    #[test]
    fn an_arbitrary_gate_key_is_accepted_and_no_profile_is_built_in() {
        // The whole point of an open key: a deployment names its own gates, and a
        // control plane that enumerated them would refuse every gate a deployment
        // added after this build shipped.
        for gate in [
            "review",
            "qa.sign-off",
            "security_check",
            "gate-7",
            "0-init",
        ] {
            let plan = find("gate_verdict")
                .expect("the gate_verdict tool")
                .plan(&arguments(serde_json::json!({
                    "project_id": "0192f0c0-0000-7000-8000-000000000001",
                    "task_id": "0192f0c0-0000-7000-8000-000000000002",
                    "expected_revision": 1,
                    "gate": gate,
                    "verdict": "passed"
                })))
                .unwrap_or_else(|error| panic!("{gate} is a legal open key: {error}"));
            assert_eq!(
                plan.request.body.as_ref().expect("a body")["intent"]["gate"],
                serde_json::json!(gate)
            );
        }
    }

    #[test]
    fn a_session_write_commits_under_a_uuid_the_runtime_will_accept() {
        let plan = find("session_message")
            .expect("the session_message tool")
            .plan(&arguments(serde_json::json!({
                "agent_run_id": "0192f0c0-0000-7000-8000-000000000001",
                "body": "keep going"
            })))
            .expect("the message plans");
        let key = plan
            .request
            .idempotency_key
            .as_deref()
            .expect("a session write carries a key");
        let parsed = uuid::Uuid::parse_str(key).expect("the key is a uuid");
        assert_eq!(
            parsed.get_version_num(),
            7,
            "the session routes read the key as a client message id, which must be a uuid v7"
        );

        // A caller's own key must still satisfy that rule, so a non-uuid is refused
        // here rather than by the daemon after the request was built.
        assert!(
            find("session_message")
                .expect("the session_message tool")
                .plan(&arguments(serde_json::json!({
                    "agent_run_id": "0192f0c0-0000-7000-8000-000000000001",
                    "body": "keep going",
                    "idempotency_key": "not-a-uuid"
                })))
                .is_err(),
            "a session key that the runtime's ledger cannot key on is refused before dispatch"
        );
    }

    #[test]
    fn a_permission_answer_addresses_the_runtimes_own_request_id() {
        let plan = find("session_permission")
            .expect("the session_permission tool")
            .plan(&arguments(serde_json::json!({
                "agent_run_id": "0192f0c0-0000-7000-8000-000000000001",
                "permission_request_id": "req-42",
                "decision": "deny"
            })))
            .expect("the answer plans");
        assert_eq!(
            plan.request.path,
            "/v1/sessions/0192f0c0-0000-7000-8000-000000000001/permissions/req-42"
        );
        assert_eq!(
            plan.request.body.as_ref().expect("a body")["decision"],
            serde_json::json!("deny")
        );
    }
}
