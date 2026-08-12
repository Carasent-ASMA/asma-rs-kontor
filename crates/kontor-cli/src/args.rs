//! The command line, and the one thing it turns into.
//!
//! # Every subcommand is an operation from the shared catalogue
//!
//! Parsing produces an [`Invocation`]: an operation name and an operand map. That
//! is the *same* pair the MCP server builds from a tool call, and it goes into the
//! same `kontor_mcp::execute`. So the CLI has no route table of its own, no second
//! idea of which authority an operation needs, and no way to drift from the tool
//! surface — the two are one surface with two front doors.
//!
//! # Open keys are strings, deliberately
//!
//! A work profile, a phase and a gate are named by *deployment* data. There is no
//! `ValueEnum` for them anywhere here, because an enum of seeded profile names
//! would refuse every profile a deployment added after this build shipped. The
//! validation that does happen is the domain's own open-key rule, applied in the
//! catalogue, and its refusal names the rule.
//!
//! A closed set is a different thing and is spelled as one: a permission decision
//! and a gate verdict come from closed domain enums, so clap offers them as choices.
//!
//! # Which credential each command uses
//!
//! The authority is the operation's own requirement unless `--authority` overrides
//! it. A read uses the observer secret, a write the operator secret, an account or
//! arming command the admin secret — so an ordinary command uses the least
//! credential that can do it rather than the strongest one available. Passing
//! `--authority observer` to a write is a way to prove a command is read-only: it
//! is refused before anything is dispatched.

use clap::{Args, Parser, Subcommand};
use kontor_mcp::client::CallerTier;
use serde_json::{Map, Value};

/// What one parsed command line asks for.
#[derive(Debug, Clone)]
pub struct Invocation {
    /// The catalogue operation to run.
    pub operation: &'static str,
    /// Its operands, in the shape the catalogue validates.
    pub operands: Map<String, Value>,
    /// The authority to act at, when the caller insisted on one.
    pub authority: Option<CallerTier>,
}

/// Build one operand map without repeating the `Value` plumbing.
#[derive(Debug, Default)]
struct Operands(Map<String, Value>);

impl Operands {
    /// A required value.
    fn text(mut self, name: &str, value: &str) -> Self {
        self.0
            .insert(name.to_owned(), Value::String(value.to_owned()));
        self
    }

    /// A value only when the caller gave one.
    ///
    /// Absent rather than null: the catalogue's schema is closed and a declared
    /// property carrying `null` is a different thing from a property nobody set.
    fn maybe(mut self, name: &str, value: Option<&String>) -> Self {
        if let Some(value) = value {
            self.0.insert(name.to_owned(), Value::String(value.clone()));
        }
        self
    }

    /// A whole number.
    fn number(mut self, name: &str, value: u64) -> Self {
        self.0.insert(name.to_owned(), Value::from(value));
        self
    }

    /// A whole number only when the caller gave one.
    fn maybe_number(mut self, name: &str, value: Option<u64>) -> Self {
        if let Some(value) = value {
            self.0.insert(name.to_owned(), Value::from(value));
        }
        self
    }

    /// A flag, only when it is set.
    fn flag(mut self, name: &str, value: bool) -> Self {
        if value {
            self.0.insert(name.to_owned(), Value::Bool(true));
        }
        self
    }

    /// A list, only when it is not empty.
    fn list(mut self, name: &str, values: &[String]) -> Self {
        if !values.is_empty() {
            self.0.insert(
                name.to_owned(),
                Value::Array(values.iter().cloned().map(Value::String).collect()),
            );
        }
        self
    }

    /// Finish one invocation.
    fn into_invocation(self, operation: &'static str, authority: Option<CallerTier>) -> Invocation {
        Invocation {
            operation,
            operands: self.0,
            authority,
        }
    }
}

/// The operands every mutation shares.
#[derive(Debug, Clone, Args)]
pub struct WriteArgs {
    /// The revision a read returned. The write is refused if the aggregate moved.
    #[arg(long)]
    pub expected_revision: u64,
    /// Why, recorded in the command's intent document.
    #[arg(long)]
    pub reason: Option<String>,
    /// The stable key this mutation commits under. Repeat it to replay.
    #[arg(long = "idempotency-key")]
    pub idempotency_key: Option<String>,
    /// Show the request that would be sent, and send nothing.
    #[arg(long)]
    pub dry_run: bool,
}

impl WriteArgs {
    /// Add the shared mutation operands.
    fn apply(&self, operands: Operands) -> Operands {
        operands
            .number("expected_revision", self.expected_revision)
            .maybe("reason", self.reason.as_ref())
            .maybe("idempotency_key", self.idempotency_key.as_ref())
            .flag("dry_run", self.dry_run)
    }
}

/// How much of a stream one read takes.
#[derive(Debug, Clone, Args)]
pub struct StreamArgs {
    /// Stop after this many frames.
    #[arg(long)]
    pub max_frames: Option<u64>,
    /// Stop when no frame has arrived for this long.
    #[arg(long)]
    pub idle_ms: Option<u64>,
}

impl StreamArgs {
    /// Add the shared stream bounds.
    fn apply(&self, operands: Operands) -> Operands {
        operands
            .maybe_number("max_frames", self.max_frames)
            .maybe_number("idle_ms", self.idle_ms)
    }
}

/// The authority tiers a caller can insist on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Authority {
    /// Read-only.
    Observer,
    /// Control-plane writes and session writes.
    Operator,
    /// Account and policy authority.
    Admin,
}

impl From<Authority> for CallerTier {
    fn from(authority: Authority) -> Self {
        match authority {
            Authority::Observer => Self::Observer,
            Authority::Operator => Self::Operator,
            Authority::Admin => Self::Admin,
        }
    }
}

/// Kontor control plane command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "kontor",
    version,
    about = "Inspect and drive one Kontor realm over its loopback contract.",
    long_about = None
)]
pub struct Cli {
    /// The realm's state root: the directory holding its database, lock and
    /// `0600` credential file. Falls back to `KONTOR_STATE_ROOT`.
    #[arg(long, global = true)]
    pub state_root: Option<std::path::PathBuf>,
    /// The realm's loopback base URL. Falls back to `KONTOR_BASE_URL`, then to the
    /// state root's `endpoint.json`, then to the standard loopback port.
    #[arg(long, global = true)]
    pub base_url: Option<String>,
    /// Act at this authority instead of the least one the command requires.
    #[arg(long, global = true, value_enum)]
    pub authority: Option<Authority>,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The top-level nouns.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Report liveness, schema generation and whether scheduling is open.
    Health,
    /// Read a bounded prefix of the durable control-plane event feed.
    Events {
        /// Resume strictly after this control-plane cursor.
        #[arg(long)]
        after: Option<u64>,
        /// How much of the stream this read takes.
        #[command(flatten)]
        stream: StreamArgs,
    },
    /// This realm's immutable identity.
    #[command(subcommand)]
    Realm(RealmCommand),
    /// Projects.
    #[command(subcommand)]
    Project(ProjectCommand),
    /// Tasks.
    #[command(subcommand)]
    Task(TaskCommand),
    /// Gates on a task.
    #[command(subcommand)]
    Gate(GateCommand),
    /// Missions — team runs.
    #[command(subcommand)]
    Mission(MissionCommand),
    /// Agent runs.
    #[command(subcommand)]
    Run(RunCommand),
    /// Work profiles.
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Command receipts.
    #[command(subcommand)]
    Receipt(ReceiptCommand),
    /// Runtime families.
    #[command(subcommand)]
    Runtime(RuntimeCommand),
    /// Scheduling.
    #[command(subcommand)]
    Scheduler(SchedulerCommand),
    /// Live sessions.
    #[command(subcommand)]
    Session(SessionCommand),
    /// Coding-account profiles. Admin authority.
    #[command(subcommand)]
    Account(AccountCommand),
    /// Execution authorization. Admin authority.
    #[command(subcommand)]
    Authorize(AuthorizeCommand),
    /// External-ticket links and their evidence.
    #[command(subcommand)]
    Ticket(TicketCommand),
    /// Serve this realm's tool surface as an MCP server over stdio.
    Mcp {
        /// The one authority the server acts at. Every tool above it is refused
        /// and is not listed.
        #[arg(long, value_enum, default_value = "observer")]
        serve_as: Authority,
    },
}

/// Realm reads.
#[derive(Debug, Subcommand)]
pub enum RealmCommand {
    /// Show this realm's identity.
    Show,
}

/// Project reads.
#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// List every project in this realm.
    List,
    /// Show one project.
    Show {
        /// The project.
        project_id: String,
    },
}

/// Task reads and writes.
#[derive(Debug, Subcommand)]
pub enum TaskCommand {
    /// List every task in one project.
    List {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
    },
    /// Show one task, its phase, gates and pinned revisions.
    Show {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The task.
        task_id: String,
    },
    /// Show one task's gate states and the evidence behind them.
    Gates {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The task.
        task_id: String,
    },
    /// Return a blocked, parked or human-held task to ready.
    Resume {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The task.
        task_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: WriteArgs,
    },
}

/// The verdicts a gate can be given. A closed domain set, so it is a choice.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Verdict {
    /// The evaluator started work on the gate.
    Started,
    /// The evaluator passed the gate.
    Passed,
    /// The evaluator rejected the gate.
    Rejected,
    /// A waiver authority waived the gate.
    Waived,
    /// The gate was parked without a verdict.
    Parked,
}

impl Verdict {
    /// The wire spelling.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Passed => "passed",
            Self::Rejected => "rejected",
            Self::Waived => "waived",
            Self::Parked => "parked",
        }
    }
}

/// Gate writes.
#[derive(Debug, Subcommand)]
pub enum GateCommand {
    /// Record a verdict on one of a task's gates.
    Verdict {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The task.
        #[arg(long = "task")]
        task_id: String,
        /// The gate key, as the deployment's work profile spells it.
        #[arg(long)]
        gate: String,
        /// The verdict to record.
        #[arg(long, value_enum)]
        verdict: Verdict,
        /// An artifact key cited as evidence. Repeatable.
        #[arg(long = "evidence")]
        evidence: Vec<String>,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: WriteArgs,
    },
}

/// Mission reads.
#[derive(Debug, Subcommand)]
pub enum MissionCommand {
    /// List every team run in one project.
    List {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
    },
    /// Show one team run.
    Show {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The team run.
        team_run_id: String,
    },
}

/// Run reads and lifecycle writes.
#[derive(Debug, Subcommand)]
pub enum RunCommand {
    /// List agent runs in one project, optionally one mission's.
    List {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// Only the runs of this team run.
        #[arg(long = "team-run")]
        team_run: Option<String>,
    },
    /// Show one agent run.
    Show {
        /// The agent run.
        agent_run_id: String,
    },
    /// Record the intent to launch one agent run.
    Launch {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The agent run.
        agent_run_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Record the intent to cancel one agent run.
    Cancel {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The agent run.
        agent_run_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Record the intent to park one agent run.
    Park {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The agent run.
        agent_run_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: WriteArgs,
    },
    /// Record the intent to abandon one agent run without a runtime verdict.
    Abandon {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The agent run.
        agent_run_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: WriteArgs,
    },
}

/// Work-profile reads.
#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// Show one stored work-profile revision as its phase and gate structure.
    Show {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The work-profile key, as the deployment spells it.
        profile_key: String,
        /// The pinned revision.
        version: u64,
    },
}

/// Receipt reads.
#[derive(Debug, Subcommand)]
pub enum ReceiptCommand {
    /// Show one command receipt and its transition history.
    Show {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The receipt.
        receipt_id: String,
    },
}

/// Runtime reads.
#[derive(Debug, Subcommand)]
pub enum RuntimeCommand {
    /// List configured runtime families and what each declares right now.
    List,
}

/// Scheduling reads.
#[derive(Debug, Subcommand)]
pub enum SchedulerCommand {
    /// Show what is currently held and would therefore block work.
    Contention,
    /// Explain what a scheduling pass over one project would decide, with every
    /// blocker per task rather than only the first. Reads only; admits nothing.
    Plan {
        /// The project to plan a pass over.
        #[arg(long = "project")]
        project_id: String,
    },
}

/// The ways a permission request can be answered. A closed domain set.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Decision {
    /// The action may proceed.
    Allow,
    /// The action is refused.
    Deny,
}

impl Decision {
    /// The wire spelling.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

/// Session reads and writes.
#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Ask one runtime which native sessions it owns, and which this realm knows.
    Discover {
        /// The runtime family to ask.
        runtime_kind: String,
    },
    /// Read one page of a session's recorded content.
    Timeline {
        /// The agent run.
        agent_run_id: String,
        /// A runtime continuation cursor a previous page returned.
        #[arg(long)]
        after: Option<String>,
        /// How many items to return at most.
        #[arg(long)]
        limit: Option<u64>,
    },
    /// Follow a session's content after a validated anchor, bounded.
    Stream {
        /// The agent run.
        agent_run_id: String,
        /// The anchor a timeline read returned.
        #[arg(long)]
        after: String,
        /// How much of the stream this read takes.
        #[command(flatten)]
        stream: StreamArgs,
    },
    /// Deliver one follow-up message into a running session.
    Message {
        /// The agent run.
        agent_run_id: String,
        /// The message to deliver.
        #[arg(long)]
        body: String,
        /// The stable client message id. A canonical UUID v7.
        #[arg(long = "idempotency-key")]
        idempotency_key: Option<String>,
        /// Show the request that would be sent, and send nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Answer one permission request a session raised.
    Permission {
        /// The agent run.
        agent_run_id: String,
        /// The runtime's own id for the request being answered.
        permission_request_id: String,
        /// The answer to apply.
        #[arg(long, value_enum)]
        decision: Decision,
        /// The stable client response id. A canonical UUID v7.
        #[arg(long = "idempotency-key")]
        idempotency_key: Option<String>,
        /// Show the request that would be sent, and send nothing.
        #[arg(long)]
        dry_run: bool,
    },
}

/// Account reads.
#[derive(Debug, Subcommand)]
pub enum AccountCommand {
    /// List the coding-account profiles in one project.
    List {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
    },
    /// Show one coding-account profile.
    Show {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The account profile.
        account_profile_id: String,
    },
}

/// The aggregates an execution authorization can be scoped to.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ScopeKind {
    /// The whole project.
    Project,
    /// One goal.
    MiniProject,
    /// One task.
    Task,
}

impl ScopeKind {
    /// The wire spelling.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::MiniProject => "mini_project",
            Self::Task => "task",
        }
    }
}

/// Arming writes.
#[derive(Debug, Subcommand)]
pub enum AuthorizeCommand {
    /// Grant a bounded execution authorization over one work scope.
    Execution {
        /// The project the grant is recorded in.
        #[arg(long = "project")]
        project_id: String,
        /// Which kind of aggregate the scope is.
        #[arg(long, value_enum)]
        target_kind: ScopeKind,
        /// The scope's own identifier.
        #[arg(long)]
        target_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: WriteArgs,
    },
}

/// The operands every ticket convergence command shares.
#[derive(Debug, Clone, Args)]
pub struct TicketWriteArgs {
    /// The project that owns the ticket link.
    #[arg(long = "project")]
    pub project_id: String,
    /// The ticket link revision a read returned.
    #[arg(long)]
    pub expected_revision: u64,
    /// Why, recorded in the command's intent document.
    #[arg(long)]
    pub reason: Option<String>,
    /// The stable key this mutation commits under. Repeat it to replay.
    #[arg(long = "idempotency-key")]
    pub idempotency_key: Option<String>,
    /// Show the request that would be sent, and send nothing.
    #[arg(long)]
    pub dry_run: bool,
}

impl TicketWriteArgs {
    /// Add the shared ticket-command operands.
    fn apply(&self, operands: Operands, link_id: &str) -> Operands {
        operands
            .text("project_id", &self.project_id)
            .text("link_id", link_id)
            .number("expected_revision", self.expected_revision)
            .maybe("reason", self.reason.as_ref())
            .maybe("idempotency_key", self.idempotency_key.as_ref())
            .flag("dry_run", self.dry_run)
    }
}

/// External-ticket reads and convergence commands.
///
/// None of the write subcommands takes an external status, transition or assignee.
/// Converging means making the external ticket match what this realm already
/// decided, so there is nothing for a caller to choose.
#[derive(Debug, Subcommand)]
pub enum TicketCommand {
    /// List the external-ticket links in one project.
    List {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
    },
    /// Show one ticket's projection, newest observation and conflicts.
    Show {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The ticket link.
        link_id: String,
    },
    /// Show the comments this realm mirrored inbound from one ticket.
    Comments {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The ticket link.
        link_id: String,
        /// How many comments at most.
        #[arg(long)]
        limit: Option<u64>,
    },
    /// Show every convergence attempt made against one ticket.
    Transitions {
        /// The project.
        #[arg(long = "project")]
        project_id: String,
        /// The ticket link.
        link_id: String,
        /// How many attempts at most.
        #[arg(long)]
        limit: Option<u64>,
    },
    /// Record the intent to write this realm's projection to the ticket.
    Sync {
        /// The ticket link.
        link_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: TicketWriteArgs,
    },
    /// Record the intent to converge the ticket's assignee.
    Assign {
        /// The ticket link.
        link_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: TicketWriteArgs,
    },
    /// Record the intent to converge the ticket's status.
    Transition {
        /// The ticket link.
        link_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: TicketWriteArgs,
    },
    /// Record the intent to resolve one detected conflict.
    ResolveConflict {
        /// The ticket link.
        link_id: String,
        /// The conflict, as `ticket show` reported it.
        #[arg(long = "conflict")]
        conflict_id: String,
        /// The revision, reason, key and dry-run operands every write shares.
        #[command(flatten)]
        write: TicketWriteArgs,
    },
}

impl Cli {
    /// Turn one parsed command line into the operation it names.
    ///
    /// Returns `None` for `kontor mcp`, which is not a catalogue operation: it
    /// serves them rather than performing one.
    #[must_use]
    pub fn invocation(&self) -> Option<Invocation> {
        let insisted = self.authority.map(CallerTier::from);
        let operands = Operands::default();
        Some(match &self.command {
            Command::Mcp { .. } => return None,
            Command::Health => operands.into_invocation("health_show", insisted),
            Command::Realm(RealmCommand::Show) => operands.into_invocation("realm_show", insisted),
            Command::Events { after, stream } => stream
                .apply(operands.maybe_number("after", *after))
                .into_invocation("events_replay", insisted),
            Command::Project(ProjectCommand::List) => {
                operands.into_invocation("project_list", insisted)
            }
            Command::Mission(MissionCommand::List { project_id }) => operands
                .text("project_id", project_id)
                .into_invocation("mission_list", insisted),
            Command::Run(RunCommand::List {
                project_id,
                team_run,
            }) => operands
                .text("project_id", project_id)
                .maybe("team_run", team_run.as_ref())
                .into_invocation("run_list", insisted),
            Command::Scheduler(SchedulerCommand::Plan { project_id }) => operands
                .text("project_id", project_id)
                .into_invocation("scheduler_plan", insisted),
            Command::Session(SessionCommand::Discover { runtime_kind }) => operands
                .text("runtime_kind", runtime_kind)
                .into_invocation("session_discover", insisted),
            Command::Ticket(TicketCommand::List { project_id }) => operands
                .text("project_id", project_id)
                .into_invocation("ticket_list", insisted),
            Command::Ticket(TicketCommand::Show {
                project_id,
                link_id,
            }) => operands
                .text("project_id", project_id)
                .text("link_id", link_id)
                .into_invocation("ticket_show", insisted),
            Command::Ticket(TicketCommand::Comments {
                project_id,
                link_id,
                limit,
            }) => operands
                .text("project_id", project_id)
                .text("link_id", link_id)
                .maybe_number("limit", *limit)
                .into_invocation("ticket_comments", insisted),
            Command::Ticket(TicketCommand::Transitions {
                project_id,
                link_id,
                limit,
            }) => operands
                .text("project_id", project_id)
                .text("link_id", link_id)
                .maybe_number("limit", *limit)
                .into_invocation("ticket_transitions", insisted),
            Command::Ticket(
                TicketCommand::Sync { link_id, write }
                | TicketCommand::Assign { link_id, write }
                | TicketCommand::Transition { link_id, write },
            ) => write
                .apply(operands, link_id)
                .into_invocation(ticket_operation(&self.command), insisted),
            Command::Ticket(TicketCommand::ResolveConflict {
                link_id,
                conflict_id,
                write,
            }) => write
                .apply(operands, link_id)
                .text("conflict_id", conflict_id)
                .into_invocation("ticket_resolve_conflict", insisted),
            Command::Project(ProjectCommand::Show { project_id }) => operands
                .text("project_id", project_id)
                .into_invocation("project_show", insisted),
            Command::Task(TaskCommand::List { project_id }) => operands
                .text("project_id", project_id)
                .into_invocation("task_list", insisted),
            Command::Task(TaskCommand::Show {
                project_id,
                task_id,
            }) => operands
                .text("project_id", project_id)
                .text("task_id", task_id)
                .into_invocation("task_show", insisted),
            Command::Task(TaskCommand::Gates {
                project_id,
                task_id,
            }) => operands
                .text("project_id", project_id)
                .text("task_id", task_id)
                .into_invocation("task_gates", insisted),
            Command::Task(TaskCommand::Resume {
                project_id,
                task_id,
                write,
            }) => write
                .apply(
                    operands
                        .text("project_id", project_id)
                        .text("task_id", task_id),
                )
                .into_invocation("task_resume", insisted),
            Command::Gate(GateCommand::Verdict {
                project_id,
                task_id,
                gate,
                verdict,
                evidence,
                write,
            }) => write
                .apply(
                    operands
                        .text("project_id", project_id)
                        .text("task_id", task_id)
                        .text("gate", gate)
                        .text("verdict", verdict.as_str())
                        .list("evidence", evidence),
                )
                .into_invocation("gate_verdict", insisted),
            Command::Mission(MissionCommand::Show {
                project_id,
                team_run_id,
            }) => operands
                .text("project_id", project_id)
                .text("team_run_id", team_run_id)
                .into_invocation("mission_show", insisted),
            Command::Run(RunCommand::Show { agent_run_id }) => operands
                .text("agent_run_id", agent_run_id)
                .into_invocation("run_show", insisted),
            Command::Run(
                RunCommand::Launch {
                    project_id,
                    agent_run_id,
                    write,
                }
                | RunCommand::Cancel {
                    project_id,
                    agent_run_id,
                    write,
                }
                | RunCommand::Park {
                    project_id,
                    agent_run_id,
                    write,
                }
                | RunCommand::Abandon {
                    project_id,
                    agent_run_id,
                    write,
                },
            ) => write
                .apply(
                    operands
                        .text("project_id", project_id)
                        .text("agent_run_id", agent_run_id),
                )
                .into_invocation(run_operation(&self.command), insisted),
            Command::Profile(ProfileCommand::Show {
                project_id,
                profile_key,
                version,
            }) => operands
                .text("project_id", project_id)
                .text("profile_key", profile_key)
                .number("version", *version)
                .into_invocation("profile_show", insisted),
            Command::Receipt(ReceiptCommand::Show {
                project_id,
                receipt_id,
            }) => operands
                .text("project_id", project_id)
                .text("receipt_id", receipt_id)
                .into_invocation("receipt_show", insisted),
            Command::Runtime(RuntimeCommand::List) => {
                operands.into_invocation("runtime_list", insisted)
            }
            Command::Scheduler(SchedulerCommand::Contention) => {
                operands.into_invocation("scheduler_contention", insisted)
            }
            Command::Session(SessionCommand::Timeline {
                agent_run_id,
                after,
                limit,
            }) => operands
                .text("agent_run_id", agent_run_id)
                .maybe("after", after.as_ref())
                .maybe_number("limit", *limit)
                .into_invocation("session_timeline", insisted),
            Command::Session(SessionCommand::Stream {
                agent_run_id,
                after,
                stream,
            }) => stream
                .apply(
                    operands
                        .text("agent_run_id", agent_run_id)
                        .text("after", after),
                )
                .into_invocation("session_stream", insisted),
            Command::Session(SessionCommand::Message {
                agent_run_id,
                body,
                idempotency_key,
                dry_run,
            }) => operands
                .text("agent_run_id", agent_run_id)
                .text("body", body)
                .maybe("idempotency_key", idempotency_key.as_ref())
                .flag("dry_run", *dry_run)
                .into_invocation("session_message", insisted),
            Command::Session(SessionCommand::Permission {
                agent_run_id,
                permission_request_id,
                decision,
                idempotency_key,
                dry_run,
            }) => operands
                .text("agent_run_id", agent_run_id)
                .text("permission_request_id", permission_request_id)
                .text("decision", decision.as_str())
                .maybe("idempotency_key", idempotency_key.as_ref())
                .flag("dry_run", *dry_run)
                .into_invocation("session_permission", insisted),
            Command::Account(AccountCommand::List { project_id }) => operands
                .text("project_id", project_id)
                .into_invocation("account_list", insisted),
            Command::Account(AccountCommand::Show {
                project_id,
                account_profile_id,
            }) => operands
                .text("project_id", project_id)
                .text("account_profile_id", account_profile_id)
                .into_invocation("account_show", insisted),
            Command::Authorize(AuthorizeCommand::Execution {
                project_id,
                target_kind,
                target_id,
                write,
            }) => write
                .apply(
                    operands
                        .text("project_id", project_id)
                        .text("target_kind", target_kind.as_str())
                        .text("target_id", target_id),
                )
                .into_invocation("authorize_execution", insisted),
        })
    }
}

/// Which ticket convergence operation a `ticket` subcommand names.
///
/// The three share their operands, so they share one match arm above; this is the
/// one place they differ.
const fn ticket_operation(command: &Command) -> &'static str {
    match command {
        Command::Ticket(TicketCommand::Sync { .. }) => "ticket_sync",
        Command::Ticket(TicketCommand::Assign { .. }) => "ticket_assign",
        Command::Ticket(TicketCommand::Transition { .. }) => "ticket_transition",
        // Unreachable: the caller matched a ticket write arm to get here. The read
        // is the fallback, because naming a convergence would be the dangerous one.
        _ => "ticket_list",
    }
}

/// Which run-lifecycle operation a `run` subcommand names.
///
/// The four share their operands, so they share one match arm above; this is the
/// one place they differ.
const fn run_operation(command: &Command) -> &'static str {
    match command {
        Command::Run(RunCommand::Launch { .. }) => "run_launch",
        Command::Run(RunCommand::Cancel { .. }) => "run_cancel",
        Command::Run(RunCommand::Park { .. }) => "run_park",
        Command::Run(RunCommand::Abandon { .. }) => "run_abandon",
        // Unreachable: the caller matched a run lifecycle arm to get here. Naming
        // the launch would be the dangerous default, so the read is the fallback.
        _ => "run_show",
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(arguments: &[&str]) -> Cli {
        Cli::try_parse_from(arguments).expect("the command line parses")
    }

    #[test]
    fn the_command_line_is_internally_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_subcommand_names_an_operation_the_catalogue_serves() {
        // The whole reason the CLI has no route table: if a subcommand named an
        // operation that does not exist, it would fail at runtime for every caller
        // rather than here for one test.
        for arguments in [
            vec!["kontor", "health"],
            vec!["kontor", "realm", "show"],
            vec!["kontor", "events"],
            vec!["kontor", "project", "show", "p"],
            vec!["kontor", "task", "list", "--project", "p"],
            vec!["kontor", "task", "show", "--project", "p", "t"],
            vec!["kontor", "task", "gates", "--project", "p", "t"],
            vec![
                "kontor",
                "task",
                "resume",
                "--project",
                "p",
                "t",
                "--expected-revision",
                "1",
            ],
            vec![
                "kontor",
                "gate",
                "verdict",
                "--project",
                "p",
                "--task",
                "t",
                "--gate",
                "g",
                "--verdict",
                "passed",
                "--expected-revision",
                "1",
            ],
            vec!["kontor", "mission", "show", "--project", "p", "m"],
            vec!["kontor", "run", "show", "r"],
            vec![
                "kontor",
                "run",
                "launch",
                "--project",
                "p",
                "r",
                "--expected-revision",
                "1",
            ],
            vec![
                "kontor",
                "run",
                "cancel",
                "--project",
                "p",
                "r",
                "--expected-revision",
                "1",
            ],
            vec![
                "kontor",
                "run",
                "park",
                "--project",
                "p",
                "r",
                "--expected-revision",
                "1",
            ],
            vec![
                "kontor",
                "run",
                "abandon",
                "--project",
                "p",
                "r",
                "--expected-revision",
                "1",
            ],
            vec!["kontor", "profile", "show", "--project", "p", "k", "1"],
            vec!["kontor", "receipt", "show", "--project", "p", "c"],
            vec!["kontor", "runtime", "list"],
            vec!["kontor", "scheduler", "contention"],
            vec!["kontor", "session", "timeline", "r"],
            vec!["kontor", "session", "stream", "r", "--after", "1:1"],
            vec!["kontor", "session", "message", "r", "--body", "go"],
            vec![
                "kontor",
                "session",
                "permission",
                "r",
                "q",
                "--decision",
                "allow",
            ],
            vec!["kontor", "account", "list", "--project", "p"],
            vec!["kontor", "account", "show", "--project", "p", "a"],
            vec![
                "kontor",
                "authorize",
                "execution",
                "--project",
                "p",
                "--target-kind",
                "task",
                "--target-id",
                "t",
                "--expected-revision",
                "1",
            ],
            // The surfaces wired in the KON-MVP-16 second amendment.
            vec!["kontor", "project", "list"],
            vec!["kontor", "mission", "list", "--project", "p"],
            vec!["kontor", "run", "list", "--project", "p"],
            vec!["kontor", "run", "list", "--project", "p", "--team-run", "m"],
            vec!["kontor", "scheduler", "plan", "--project", "p"],
            vec!["kontor", "session", "discover", "fake.runtime"],
            vec!["kontor", "ticket", "list", "--project", "p"],
            vec!["kontor", "ticket", "show", "--project", "p", "l"],
            vec!["kontor", "ticket", "comments", "--project", "p", "l"],
            vec!["kontor", "ticket", "transitions", "--project", "p", "l"],
            vec![
                "kontor",
                "ticket",
                "sync",
                "l",
                "--project",
                "p",
                "--expected-revision",
                "1",
            ],
            vec![
                "kontor",
                "ticket",
                "assign",
                "l",
                "--project",
                "p",
                "--expected-revision",
                "1",
            ],
            vec![
                "kontor",
                "ticket",
                "transition",
                "l",
                "--project",
                "p",
                "--expected-revision",
                "1",
            ],
            vec![
                "kontor",
                "ticket",
                "resolve-conflict",
                "l",
                "--conflict",
                "c",
                "--project",
                "p",
                "--expected-revision",
                "1",
            ],
        ] {
            let invocation = parse(&arguments)
                .invocation()
                .unwrap_or_else(|| panic!("{arguments:?} names an operation"));
            let served = kontor_mcp::tools::find(invocation.operation).unwrap_or_else(|| {
                panic!(
                    "{arguments:?} names `{}`, which the catalogue does not serve",
                    invocation.operation
                )
            });
            served
                .validate(&invocation.operands)
                .unwrap_or_else(|error| {
                    panic!(
                        "{arguments:?} builds operands `{}` refuses: {error}",
                        invocation.operation
                    )
                });
        }
    }

    #[test]
    fn the_four_run_lifecycle_commands_are_four_different_operations() {
        // They share one match arm because they share their operands, which is
        // exactly the shape a copy-paste bug hides in.
        let names: Vec<&str> = ["launch", "cancel", "park", "abandon"]
            .into_iter()
            .map(|verb| {
                parse(&[
                    "kontor",
                    "run",
                    verb,
                    "--project",
                    "p",
                    "r",
                    "--expected-revision",
                    "1",
                ])
                .invocation()
                .expect("a run command names an operation")
                .operation
            })
            .collect();
        assert_eq!(
            names,
            vec!["run_launch", "run_cancel", "run_park", "run_abandon"]
        );
    }

    #[test]
    fn the_three_ticket_convergence_commands_are_three_different_operations() {
        // Same shape as the run-lifecycle arm, and the same copy-paste risk.
        let names: Vec<&str> = ["sync", "assign", "transition"]
            .into_iter()
            .map(|verb| {
                parse(&[
                    "kontor",
                    "ticket",
                    verb,
                    "l",
                    "--project",
                    "p",
                    "--expected-revision",
                    "1",
                ])
                .invocation()
                .expect("a ticket command names an operation")
                .operation
            })
            .collect();
        assert_eq!(
            names,
            vec!["ticket_sync", "ticket_assign", "ticket_transition"]
        );
    }

    #[test]
    fn no_ticket_command_line_accepts_an_external_status_or_assignee() {
        // The command line is the other place a caller could be handed a way to
        // drive a foreign workflow, so the flag must not exist at all.
        for flag in [
            "--status",
            "--transition-id",
            "--assignee",
            "--comment",
            "--milestone",
        ] {
            assert!(
                Cli::try_parse_from([
                    "kontor",
                    "ticket",
                    "transition",
                    "l",
                    "--project",
                    "p",
                    "--expected-revision",
                    "1",
                    flag,
                    "anything",
                ])
                .is_err(),
                "`{flag}` must not be a ticket command operand"
            );
        }
    }

    #[test]
    fn mcp_is_not_a_catalogue_operation() {
        assert!(
            parse(&["kontor", "mcp"]).invocation().is_none(),
            "`kontor mcp` serves the operations rather than performing one"
        );
    }

    #[test]
    fn an_open_key_is_a_string_and_no_profile_name_is_built_in() {
        // A deployment names its own profiles and gates. If these were `ValueEnum`s
        // this test could not be written, which is the point of writing it.
        for key in ["delivery", "qa.sign-off", "team-b_flow", "0-bootstrap"] {
            let invocation = parse(&["kontor", "profile", "show", "--project", "p", key, "2"])
                .invocation()
                .expect("a profile read names an operation");
            assert_eq!(
                invocation.operands["profile_key"],
                Value::String(key.to_owned())
            );
        }
    }

    #[test]
    fn an_unset_optional_operand_is_absent_rather_than_null() {
        // The catalogue's schema is closed and typed: a declared property carrying
        // `null` would be refused as the wrong type, so "not given" has to mean
        // "not present".
        let invocation = parse(&["kontor", "session", "timeline", "r"])
            .invocation()
            .expect("a timeline read names an operation");
        assert!(!invocation.operands.contains_key("after"));
        assert!(!invocation.operands.contains_key("limit"));
        assert!(
            !invocation.operands.contains_key("dry_run"),
            "an unset flag is absent too"
        );
    }

    #[test]
    fn an_insisted_authority_is_carried_and_absence_means_the_operations_own() {
        let insisted = parse(&["kontor", "--authority", "observer", "run", "show", "r"])
            .invocation()
            .expect("a run read names an operation");
        assert_eq!(insisted.authority, Some(CallerTier::Observer));

        let default = parse(&["kontor", "run", "show", "r"])
            .invocation()
            .expect("a run read names an operation");
        assert_eq!(
            default.authority, None,
            "absence lets the command use the least credential that can do it"
        );
    }

    #[test]
    fn repeated_evidence_becomes_a_list_and_no_evidence_becomes_nothing() {
        let with = parse(&[
            "kontor",
            "gate",
            "verdict",
            "--project",
            "p",
            "--task",
            "t",
            "--gate",
            "g",
            "--verdict",
            "rejected",
            "--evidence",
            "diff",
            "--evidence",
            "tests",
            "--expected-revision",
            "3",
        ])
        .invocation()
        .expect("a verdict names an operation");
        assert_eq!(
            with.operands["evidence"],
            serde_json::json!(["diff", "tests"])
        );

        let without = parse(&[
            "kontor",
            "gate",
            "verdict",
            "--project",
            "p",
            "--task",
            "t",
            "--gate",
            "g",
            "--verdict",
            "passed",
            "--expected-revision",
            "3",
        ])
        .invocation()
        .expect("a verdict names an operation");
        assert!(!without.operands.contains_key("evidence"));
    }
}
