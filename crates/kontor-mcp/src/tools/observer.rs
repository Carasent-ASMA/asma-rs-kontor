//! Query-only operations. Every one of them is a `GET`, and none of them can be
//! built as anything else.
//!
//! An observer tool answers a question about what is recorded. It never records
//! anything, and — because [`Effect::Query`] operations are built as `GET`s and the
//! shape test in the parent module proves it — it cannot start recording something
//! by accident.
//!
//! # The two cursor spaces are spelled differently here too
//!
//! [`EVENTS`] resumes from a control-plane cursor: an integer this Realm's log
//! allocated. [`SESSION_TIMELINE`] and [`SESSION_STREAM`] resume from a runtime
//! content position: an opaque cursor or an `epoch:sequence` anchor the runtime
//! owns. The operands are named `after` in both cases because that is what the
//! routes call them, but they are never interchangeable and this surface never
//! converts one into the other — a hole in the first is a paging question and a
//! hole in the second is a refetch obligation.

use crate::client::{CallerTier, FrameBudget, Request};
use crate::tools::{Effect, Operands, Plan, Property, ToolSpec};

/// How many frames a bounded stream read takes by default.
const DEFAULT_FRAMES: usize = 100;

/// Liveness, Realm identity and whether scheduling is open.
const HEALTH: ToolSpec = ToolSpec {
    name: "health_show",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Report this realm's liveness, schema generation, configured runtime families, \
                  and whether startup reconciliation finished and scheduling is therefore open.",
    properties: &[],
    build: |_| Ok(Plan::of(Request::get("/v1/health"))),
};

/// The Realm's immutable identity.
const REALM: ToolSpec = ToolSpec {
    name: "realm_show",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Report this realm's immutable identity: its id, the envelope contract it was \
                  created under, and its non-secret label.",
    properties: &[],
    build: |_| Ok(Plan::of(Request::get("/v1/realm"))),
};

/// One project.
const PROJECT: ToolSpec = ToolSpec {
    name: "project_show",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read one project's name, root path and current revision.",
    properties: &[Property::required(
        "project_id",
        "The project, as a canonical identifier.",
    )],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}",
            operands.project_id()?
        ))))
    },
};

/// Every task in a project.
const TASKS: ToolSpec = ToolSpec {
    name: "task_list",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "List every task in one project, oldest first, with its state, contended module \
                  and current revision. This is how a caller finds the task it needs before \
                  naming it.",
    properties: &[Property::required(
        "project_id",
        "The project, as a canonical identifier.",
    )],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/tasks",
            operands.project_id()?
        ))))
    },
};

/// One task, its workflow phase, gates and pinned revisions.
const TASK: ToolSpec = ToolSpec {
    name: "task_show",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read one task's state, active workflow phase, gate states and the pinned \
                  specification revisions it is running under. The answer carries the revision a \
                  later write must present.",
    properties: &[
        Property::required(
            "project_id",
            "The project that owns the aggregate being read.",
        ),
        Property::required("task_id", "The task to read, as a canonical identifier."),
    ],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/tasks/{}",
            operands.project_id()?,
            operands.task_id()?
        ))))
    },
};

/// One task's gate states and the evidence behind them.
const TASK_GATES: ToolSpec = ToolSpec {
    name: "task_gates",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read one task's gates: the frozen work profile's phase and gate structure, the \
                  reduced state of each gate, and every recorded verdict with the role, account, \
                  principal and cited artifacts behind it.",
    properties: &[
        Property::required(
            "project_id",
            "The project that owns the aggregate being read.",
        ),
        Property::required("task_id", "The task to read, as a canonical identifier."),
    ],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/tasks/{}/gates",
            operands.project_id()?,
            operands.task_id()?
        ))))
    },
};

/// One team run.
const MISSION: ToolSpec = ToolSpec {
    name: "mission_show",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read one team run — a mission — its lifecycle, the task it serves and the team \
                  template revision it froze.",
    properties: &[
        Property::required(
            "project_id",
            "The project that owns the aggregate being read.",
        ),
        Property::required(
            "team_run_id",
            "The team run to read, as a canonical identifier.",
        ),
    ],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/team-runs/{}",
            operands.project_id()?,
            operands.team_run_id()?
        ))))
    },
};

/// One agent run.
const RUN: ToolSpec = ToolSpec {
    name: "run_show",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read one agent run: its orthogonal lifecycle, desired and observed state, how \
                  fresh its newest confirmation is, its native binding, and every recorded \
                  discontinuity in its history.",
    properties: &[Property::required(
        "agent_run_id",
        "The agent run to read, as a canonical identifier.",
    )],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/runs/{}",
            operands.agent_run_id()?
        ))))
    },
};

/// One work-profile revision.
const PROFILE: ToolSpec = ToolSpec {
    name: "profile_show",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read one stored work-profile revision as its phase and gate structure. The \
                  profile key is deployment data: any open key a deployment defined is accepted, \
                  and no set of names is built in.",
    properties: &[
        Property::required(
            "project_id",
            "The project that owns the aggregate being read.",
        ),
        Property::required(
            "profile_key",
            "The work-profile key, as the deployment spells it.",
        ),
        Property::number(
            "version",
            "The pinned revision of that profile key. Revisions are immutable.",
        ),
    ],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/profiles/{}/{}",
            operands.project_id()?,
            operands.profile_key()?,
            operands.spec_version()?.get()
        ))))
    },
};

/// One command receipt and its history.
const RECEIPT: ToolSpec = ToolSpec {
    name: "receipt_show",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read one command receipt and every state it has been through. This is how a \
                  caller checks that a retried command replayed instead of recording a second \
                  one: the history does not grow.",
    properties: &[
        Property::required(
            "project_id",
            "The project that owns the aggregate being read.",
        ),
        Property::required(
            "receipt_id",
            "The command receipt to read, as a canonical identifier.",
        ),
    ],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/receipts/{}",
            operands.project_id()?,
            operands.receipt_id()?
        ))))
    },
};

/// Configured runtime families and what they declare right now.
const RUNTIMES: ToolSpec = ToolSpec {
    name: "runtime_list",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "List the runtime families this realm is configured with, whether each one \
                  answered, and the capabilities, trust grade and limits it declares right now. \
                  These are freshly discovered declarations and are never the frozen set a \
                  running session was bound at.",
    properties: &[],
    build: |_| Ok(Plan::of(Request::get("/v1/runtimes"))),
};

/// What scheduling would currently contend with.
const CONTENTION: ToolSpec = ToolSpec {
    name: "scheduler_contention",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read what is currently held and would therefore block work: live module claims, \
                  leased worktrees and tasks with open runs. This is contention evidence and not \
                  a scheduling decision — no plan is computed and none is implied.",
    properties: &[],
    build: |_| Ok(Plan::of(Request::get("/v1/scheduler/contention"))),
};

/// One page of a session's recorded content.
const SESSION_TIMELINE: ToolSpec = ToolSpec {
    name: "session_timeline",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read one page of a session's recorded content from its runtime. The answer \
                  carries an `anchor`, which is the position a live read must start strictly \
                  after. A `timeline_refetch_required` refusal means the content must be read \
                  again from the start; it never means the run ended.",
    properties: &[
        Property::required("agent_run_id", "The agent run whose session to read."),
        Property::optional(
            "after",
            "A runtime continuation cursor a previous page returned. Never a control-plane cursor.",
        ),
        Property::optional_number("limit", "How many items to return at most."),
    ],
    build: |operands| {
        Ok(Plan::of(
            Request::get(format!(
                "/v1/sessions/{}/timeline",
                operands.agent_run_id()?
            ))
            .with_optional_query("after", operands.optional_text("after"))
            .with_optional_query("limit", operands.optional_number("limit")),
        ))
    },
};

/// A bounded live read of one session's content.
const SESSION_STREAM: ToolSpec = ToolSpec {
    name: "session_stream",
    tier: CallerTier::Observer,
    effect: Effect::Stream,
    description: "Follow one session's content strictly after an anchor a timeline read returned, \
                  returning a bounded prefix of the frames. The anchor is required: without a \
                  position a previous read validated there is nothing for delivery to be strictly \
                  after.",
    properties: &[
        Property::required("agent_run_id", "The agent run whose session to follow."),
        Property::required("after", "The anchor a timeline read returned."),
        Property::optional_number("max_frames", "Stop after this many frames."),
        Property::optional_number("idle_ms", "Stop when no frame has arrived for this long."),
    ],
    build: |operands| {
        let request = Request::get(format!("/v1/sessions/{}/stream", operands.agent_run_id()?))
            .with_query("after", operands.opaque("after"));
        Ok(Plan::streaming(request, budget(operands)))
    },
};

/// A bounded read of the durable control-plane feed.
const EVENTS: ToolSpec = ToolSpec {
    name: "events_replay",
    tier: CallerTier::Observer,
    effect: Effect::Stream,
    description: "Read a bounded prefix of the durable control-plane event feed, resuming strictly \
                  after a control-plane cursor this realm allocated. Each frame's id is the next \
                  cursor to resume from. A `resnapshot_required` refusal means the position is \
                  outside the retained history.",
    properties: &[
        Property::optional_number(
            "after",
            "The control-plane cursor already seen. Delivery starts strictly after it.",
        ),
        Property::optional_number("max_frames", "Stop after this many frames."),
        Property::optional_number("idle_ms", "Stop when no frame has arrived for this long."),
    ],
    build: |operands| {
        let request = Request::get("/v1/events")
            .with_optional_query("after", operands.optional_number("after"));
        Ok(Plan::streaming(request, budget(operands)))
    },
};

/// Every project in this Realm.
const PROJECTS: ToolSpec = ToolSpec {
    name: "project_list",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "List every project in this realm with its name, root path and current revision. \
                  This is the entry point when no project is known yet.",
    properties: &[],
    build: |_| Ok(Plan::of(Request::get("/v1/projects"))),
};

/// Every mission in one project.
const MISSIONS: ToolSpec = ToolSpec {
    name: "mission_list",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "List every team run — mission — in one project, with the task it serves, the \
                  team template revision it froze and its lifecycle.",
    properties: &[Property::required(
        "project_id",
        "The project whose missions to list.",
    )],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/team-runs",
            operands.project_id()?
        ))))
    },
};

/// Every agent run in one project.
const RUNS: ToolSpec = ToolSpec {
    name: "run_list",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "List agent runs in one project, optionally only one mission's, with each run's \
                  lifecycle, desired and observed state and the revision a write must present.",
    properties: &[
        Property::required("project_id", "The project whose runs to list."),
        Property::optional("team_run", "Only the runs of this team run."),
    ],
    build: |operands| {
        Ok(Plan::of(
            Request::get(format!("/v1/projects/{}/runs", operands.project_id()?))
                .with_optional_query("team_run", operands.optional_text("team_run")),
        ))
    },
};

/// What a scheduling pass over one project would decide.
const PLAN: ToolSpec = ToolSpec {
    name: "scheduler_plan",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Explain what a scheduling pass over one project would decide, and why. Each \
                  refused task carries EVERY blocker that refuses it, in evaluation order, not \
                  only the first — so an operator is not sent round a loop fixing one at a time. \
                  Nothing is admitted, queued or launched: this is a read. The answer also names \
                  every snapshot field that had no stored source and the value it was assembled \
                  with, so a default is never mistaken for evidence.",
    properties: &[Property::required(
        "project_id",
        "The project to plan a pass over.",
    )],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/scheduler/plan",
            operands.project_id()?
        ))))
    },
};

/// Every external-ticket link in one project.
const TICKETS: ToolSpec = ToolSpec {
    name: "ticket_list",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "List the external-ticket links in one project: which task each links, which \
                  connector, which external issue key, and the revision a convergence command \
                  must present.",
    properties: &[Property::required(
        "project_id",
        "The project whose ticket links to list.",
    )],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/tickets",
            operands.project_id()?
        ))))
    },
};

/// One ticket's stored evidence.
const TICKET: ToolSpec = ToolSpec {
    name: "ticket_show",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read the evidence this realm holds about one external ticket: the projection it \
                  computed, the newest observation of the ticket's own status and assignee, and \
                  every conflict it detected. The observation is what was last seen and is never \
                  a claim about now. Nothing here contacts the external system.",
    properties: &[
        Property::required("project_id", "The project that owns the ticket link."),
        Property::required("link_id", "The ticket link to read."),
    ],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/projects/{}/tickets/{}",
            operands.project_id()?,
            operands.ticket_link_id()?
        ))))
    },
};

/// One ticket's inbound comments.
const TICKET_COMMENTS: ToolSpec = ToolSpec {
    name: "ticket_comments",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read the comments this realm mirrored inbound from one external ticket, newest \
                  first. Inbound only: this control plane has no outbound comment path at all, \
                  so there is no tool that posts one.",
    properties: &[
        Property::required("project_id", "The project that owns the ticket link."),
        Property::required("link_id", "The ticket link to read."),
        Property::optional_number("limit", "How many comments at most."),
    ],
    build: |operands| {
        Ok(Plan::of(
            Request::get(format!(
                "/v1/projects/{}/tickets/{}/comments",
                operands.project_id()?,
                operands.ticket_link_id()?
            ))
            .with_optional_query("limit", operands.optional_number("limit")),
        ))
    },
};

/// One ticket's convergence attempts.
const TICKET_TRANSITIONS: ToolSpec = ToolSpec {
    name: "ticket_transitions",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Read every convergence attempt this realm made against one external ticket: \
                  which internal milestone it was projecting, which external status it aimed at, \
                  which transition it used, and whether a REFETCHED observation confirmed it. An \
                  unconfirmed attempt is not a failed one.",
    properties: &[
        Property::required("project_id", "The project that owns the ticket link."),
        Property::required("link_id", "The ticket link to read."),
        Property::optional_number("limit", "How many attempts at most."),
    ],
    build: |operands| {
        Ok(Plan::of(
            Request::get(format!(
                "/v1/projects/{}/tickets/{}/transitions",
                operands.project_id()?,
                operands.ticket_link_id()?
            ))
            .with_optional_query("limit", operands.optional_number("limit")),
        ))
    },
};

/// The native sessions one runtime currently owns.
const SESSION_DISCOVER: ToolSpec = ToolSpec {
    name: "session_discover",
    tier: CallerTier::Observer,
    effect: Effect::Query,
    description: "Ask one runtime family which native sessions it currently owns, and whether \
                  this realm already holds a binding for each. A session with no binding exists \
                  natively and is unknown to this realm. Adopting one is not served: no command \
                  kind records that intent, so this reports and claims nothing.",
    properties: &[Property::required(
        "runtime_kind",
        "The runtime family to ask, as `runtime_list` spells it.",
    )],
    build: |operands| {
        Ok(Plan::of(Request::get(format!(
            "/v1/runtimes/{}/sessions",
            operands.runtime_kind()?
        ))))
    },
};

/// Read the stream bounds a caller asked for, or the defaults.
fn budget(operands: &Operands<'_>) -> FrameBudget {
    let default = FrameBudget::default();
    FrameBudget {
        max_frames: operands
            .optional_number("max_frames")
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_FRAMES),
        idle: operands
            .optional_number("idle_ms")
            .filter(|value| *value > 0)
            .map_or(default.idle, std::time::Duration::from_millis),
    }
}

/// Every query-only operation.
#[must_use]
pub const fn tools() -> &'static [ToolSpec] {
    &[
        CONTENTION,
        EVENTS,
        HEALTH,
        MISSION,
        MISSIONS,
        PLAN,
        PROFILE,
        PROJECT,
        PROJECTS,
        REALM,
        RECEIPT,
        RUN,
        RUNS,
        RUNTIMES,
        SESSION_DISCOVER,
        SESSION_STREAM,
        SESSION_TIMELINE,
        TASK,
        TASKS,
        TASK_GATES,
        TICKET,
        TICKETS,
        TICKET_COMMENTS,
        TICKET_TRANSITIONS,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_observer_tool_is_a_mutation() {
        for tool in tools() {
            assert_ne!(
                tool.effect,
                Effect::Mutation,
                "{} is served as an observer tool and must not write",
                tool.name
            );
            assert_eq!(
                tool.tier,
                CallerTier::Observer,
                "{} is in the observer module and must require observer authority",
                tool.name
            );
        }
    }

    #[test]
    fn a_stream_read_is_bounded_by_default_and_by_request() {
        let stream = SESSION_STREAM;
        let arguments = serde_json::json!({
            "agent_run_id": "0192f0c0-0000-7000-8000-000000000001",
            "after": "1:12"
        });
        let plan = stream
            .plan(arguments.as_object().expect("an arguments object"))
            .expect("a stream read plans");
        let budget = plan.budget.expect("a stream read carries a budget");
        assert_eq!(
            budget.max_frames, DEFAULT_FRAMES,
            "an unbounded follow would never return, so there is always a bound"
        );

        let narrowed = serde_json::json!({
            "agent_run_id": "0192f0c0-0000-7000-8000-000000000001",
            "after": "1:12",
            "max_frames": 3,
            "idle_ms": 50
        });
        let plan = stream
            .plan(narrowed.as_object().expect("an arguments object"))
            .expect("a stream read plans");
        let budget = plan.budget.expect("a stream read carries a budget");
        assert_eq!(budget.max_frames, 3);
        assert_eq!(budget.idle, std::time::Duration::from_millis(50));
    }

    #[test]
    fn a_zero_bound_falls_back_rather_than_returning_nothing() {
        // A caller that asks for zero frames has made a mistake, and honouring it
        // literally would answer "the realm is quiet" for a realm that is not.
        let arguments = serde_json::json!({ "max_frames": 0, "idle_ms": 0 });
        let plan = EVENTS
            .plan(arguments.as_object().expect("an arguments object"))
            .expect("an events read plans");
        let budget = plan.budget.expect("a stream read carries a budget");
        assert_eq!(budget.max_frames, DEFAULT_FRAMES);
        assert_eq!(budget.idle, FrameBudget::default().idle);
    }

    #[test]
    fn a_control_plane_cursor_and_a_content_anchor_are_carried_differently() {
        let events = EVENTS
            .plan(
                serde_json::json!({ "after": 42 })
                    .as_object()
                    .expect("an arguments object"),
            )
            .expect("an events read plans");
        assert_eq!(
            events.request.query,
            vec![("after".to_owned(), "42".to_owned())],
            "a control-plane position is an integer this realm allocated"
        );

        let session = SESSION_TIMELINE
            .plan(
                serde_json::json!({
                    "agent_run_id": "0192f0c0-0000-7000-8000-000000000001",
                    "after": "3:118"
                })
                .as_object()
                .expect("an arguments object"),
            )
            .expect("a timeline read plans");
        assert_eq!(
            session.request.query,
            vec![("after".to_owned(), "3:118".to_owned())],
            "a content position is the runtime's own opaque cursor, relayed as text"
        );
        assert!(
            EVENTS
                .properties
                .iter()
                .any(|property| property.name == "after"
                    && property.kind == crate::tools::PropertyKind::Integer),
            "the control-plane cursor is declared as an integer, so an anchor cannot be sent as one"
        );
    }
}
