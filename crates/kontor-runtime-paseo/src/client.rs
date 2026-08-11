//! The Paseo transport: one seam, two dispatch shapes, bounded and quiet.
//!
//! The adapter never runs a process and never frames a protocol message itself.
//! It builds a [`PaseoCommand`] or a [`PaseoRpc`], hands it to a
//! [`PaseoTransport`], and reads back JSON. That seam earns its keep three times
//! over, exactly as AO's does:
//!
//! * the contract suite can prove a refusal produced **zero** calls, which is a
//!   claim about the wire that no amount of return-value checking can make;
//! * "a lost acknowledgement must not cause a second `agent run`" becomes a
//!   count over a recorded ledger instead of an inference;
//! * a fault can be injected *after* the fixture-side effect committed, which is
//!   the one ordering that matters for confirmation-unknown.
//!
//! # Secrets
//!
//! Paseo 0.2.5 accepts a remote password only inside its `--host` URI, so the
//! complete host target is a credential. It lives in a [`SecretString`] owned by
//! the live transport and is appended immediately before dispatch. It is not a
//! field of [`PaseoCommand`], so it cannot reach a ledger, a checkpoint, an
//! error payload or a fixture — not because every call site remembers to redact
//! it, but because no call site is ever handed it.
//!
//! # No shell, ever
//!
//! Every command is an argv array. There is no string that a title, a path or a
//! prompt is interpolated into, so a hostile display name is an argument and
//! never a second command.

use std::collections::BTreeMap;
use std::fmt;

use async_trait::async_trait;
use kontor_core::DomainError;
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use secrecy::{ExposeSecret, SecretString};

use crate::wire::{MAX_FRAME_BYTES, MAX_OUTPUT_BYTES, PaseoProjection};

/// The JSON flag every lifecycle command carries.
const JSON_FLAG: &str = "--json";

/// The environment variable Paseo reads the parent agent from.
pub const PARENT_AGENT_ENV: &str = "PASEO_AGENT_ID";

// ---------------------------------------------------------------------------
// CLI commands
// ---------------------------------------------------------------------------

/// One Paseo CLI invocation, as an argv array.
///
/// `argv` never contains `--host`: the live transport appends it. The
/// constructors below are the only way to build one, so no call site can invent
/// an unversioned subcommand or forget `--json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoCommand {
    argv: Vec<String>,
    /// Every argv element that came from outside this adapter.
    ///
    /// Recorded separately because "is this argument flag-shaped?" is only a
    /// question about foreign values. Scanning the whole argv instead would
    /// refuse `--background --workspace`, where one flag legitimately follows
    /// another.
    values: Vec<String>,
    route: String,
    env: Vec<(String, String)>,
    mutates: bool,
}

impl PaseoCommand {
    /// `paseo --version --json`.
    #[must_use]
    pub fn version() -> Self {
        Self::read(Argv::new(&["--version"]), "version".to_owned())
    }

    /// `paseo workspace create --isolation local --path … --project … --title …`.
    ///
    /// `--isolation local` is what registers the *existing* task worktree rather
    /// than asking Paseo to provision one of its own, which would put the role
    /// in a tree Kontor never prepared.
    ///
    /// `labels` carries the Kontor team-run label, which is what makes the
    /// workspace's correlation evidence evidence. See
    /// [`crate::wire::PaseoWorkspace::labels`] for why that flag is the least
    /// certain part of this adapter's CLI surface.
    #[must_use]
    pub fn workspace_create(
        canonical_cwd: &str,
        project_id: &str,
        title: &str,
        labels: &BTreeMap<String, String>,
    ) -> Self {
        let mut argv = Argv::new(&["workspace", "create", "--isolation", "local"])
            .option("--path", canonical_cwd)
            .option("--project", project_id)
            .option("--title", title);
        for (key, value) in labels {
            argv = argv.option("--label", &format!("{key}={value}"));
        }
        Self::mutate(argv, "workspace create".to_owned())
    }

    /// `paseo workspace archive {id}`.
    #[must_use]
    pub fn workspace_archive(workspace_id: &str) -> Self {
        Self::mutate(
            Argv::new(&["workspace", "archive"]).value(workspace_id),
            format!("workspace archive {workspace_id}"),
        )
    }

    /// `paseo agent run --background --workspace … --cwd … --title … --label …`.
    ///
    /// Both `--workspace` and `--cwd` travel. Either alone is a hierarchy that
    /// can be right by accident: a workspace id with no directory would let
    /// Paseo pick one, and a directory with no workspace id would place the
    /// agent wherever that path currently resolves.
    #[must_use]
    pub fn agent_run(
        workspace_id: &str,
        canonical_cwd: &str,
        title: &str,
        labels: &BTreeMap<String, String>,
        parent_agent_id: &str,
        prompt: &str,
    ) -> Self {
        let mut argv = Argv::new(&["agent", "run", "--background"])
            .option("--workspace", workspace_id)
            .option("--cwd", canonical_cwd)
            .option("--title", title);
        for (key, value) in labels {
            argv = argv.option("--label", &format!("{key}={value}"));
        }
        argv = argv.option("--prompt", prompt);
        let mut command = Self::mutate(argv, "agent run".to_owned());
        command
            .env
            .push((PARENT_AGENT_ENV.to_owned(), parent_agent_id.to_owned()));
        command
    }

    /// `paseo agent inspect {id}`.
    #[must_use]
    pub fn agent_inspect(agent_id: &str) -> Self {
        Self::read(
            Argv::new(&["agent", "inspect"]).value(agent_id),
            format!("agent inspect {agent_id}"),
        )
    }

    /// `paseo agent update {id} --label …` — the adoption write, and nothing else.
    #[must_use]
    pub fn agent_update_labels(agent_id: &str, labels: &BTreeMap<String, String>) -> Self {
        let mut argv = Argv::new(&["agent", "update"]).value(agent_id);
        for (key, value) in labels {
            argv = argv.option("--label", &format!("{key}={value}"));
        }
        Self::mutate(argv, format!("agent update {agent_id}"))
    }

    /// `paseo agent reload {id}` — a process restart for an explicitly stopped
    /// agent, and never a way to simulate a new turn or a compaction.
    #[must_use]
    pub fn agent_reload(agent_id: &str) -> Self {
        Self::mutate(
            Argv::new(&["agent", "reload"]).value(agent_id),
            format!("agent reload {agent_id}"),
        )
    }

    /// `paseo agent stop {id}`.
    #[must_use]
    pub fn agent_stop(agent_id: &str) -> Self {
        Self::mutate(
            Argv::new(&["agent", "stop"]).value(agent_id),
            format!("agent stop {agent_id}"),
        )
    }

    /// `paseo agent archive {id}` — explicit retirement of a role session.
    #[must_use]
    pub fn agent_archive(agent_id: &str) -> Self {
        Self::mutate(
            Argv::new(&["agent", "archive"]).value(agent_id),
            format!("agent archive {agent_id}"),
        )
    }

    /// The argv the transport dispatches, without `--host`.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// The environment the child process is given.
    #[must_use]
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    /// The ledger key the contract suite counts calls by.
    ///
    /// Subcommand and addressed id only. A title, a path, a label value and a
    /// prompt are all absent by construction, so an assertion about the wire can
    /// never accidentally quote the operator's work — and neither can a log line
    /// that prints one of these.
    #[must_use]
    pub fn route(&self) -> &str {
        &self.route
    }

    /// Whether this command can change Paseo.
    #[must_use]
    pub const fn mutates(&self) -> bool {
        self.mutates
    }

    /// Refuse a foreign value that would be read as a flag.
    ///
    /// Paseo ids, paths and titles are foreign strings. One beginning with `-`
    /// lands in argv as an option rather than as the value it was meant to be,
    /// which is the argv analogue of interpolating an id into a URL path: a
    /// workspace id of `--force` is not a workspace at all.
    ///
    /// ponytail: this also refuses a legitimate prompt that happens to begin
    /// with `-`. Paseo 0.2.5's recorded CLI shape is space-separated and this
    /// adapter will not invent a `--flag=value` or `--` contract it has not
    /// observed; refusing typed beats guessing at the parser. Relax it to
    /// `--prompt=<text>` once a live probe confirms that spelling.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] for an empty value and for one that
    /// starts with `-`.
    pub fn ensure_dispatchable(&self) -> RuntimeResult<()> {
        for value in &self.values {
            if value.is_empty() {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCommand",
                    "carries an empty argument",
                )));
            }
            if value.starts_with('-') {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    "PaseoCommand",
                    "carries a value that would be read as another option",
                )));
            }
        }
        Ok(())
    }

    fn read(argv: Argv, route: String) -> Self {
        Self::build(argv, route, false)
    }

    fn mutate(argv: Argv, route: String) -> Self {
        Self::build(argv, route, true)
    }

    fn build(argv: Argv, route: String, mutates: bool) -> Self {
        let Argv { mut argv, values } = argv;
        argv.push(JSON_FLAG.to_owned());
        Self {
            argv,
            values,
            route,
            env: Vec::new(),
            mutates,
        }
    }
}

/// An argv under construction, keeping trusted words and foreign values apart.
struct Argv {
    argv: Vec<String>,
    values: Vec<String>,
}

impl Argv {
    /// Start from literal subcommand words and flags this adapter wrote itself.
    fn new(parts: &[&str]) -> Self {
        Self {
            argv: parts.iter().map(|part| (*part).to_owned()).collect(),
            values: Vec::new(),
        }
    }

    /// Append one foreign value as a positional argument.
    fn value(mut self, value: &str) -> Self {
        self.argv.push(value.to_owned());
        self.values.push(value.to_owned());
        self
    }

    /// Append one flag and its foreign value.
    fn option(mut self, flag: &str, value: &str) -> Self {
        self.argv.push(flag.to_owned());
        self.value(value)
    }
}

/// One CLI answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoOutput {
    /// The process exit status.
    pub status: i32,
    /// Standard output, already bounded.
    pub stdout: String,
}

impl PaseoOutput {
    /// Build an answer.
    #[must_use]
    pub const fn new(status: i32, stdout: String) -> Self {
        Self { status, stdout }
    }

    /// The JSON of a successful invocation, deserialized.
    ///
    /// A non-zero exit is a [`RuntimeError::Transport`] naming only that fact.
    /// Paseo's stderr can quote a prompt, a path or the host URI it was given,
    /// so it is never read into a refusal.
    ///
    /// # Errors
    /// * [`RuntimeError::Transport`] — a non-zero exit.
    /// * [`RuntimeError::Domain`] — output that is not the pinned 0.2.5 shape,
    ///   which includes output that is not JSON at all.
    pub fn parse<T: serde::de::DeserializeOwned>(&self, subject: &'static str) -> RuntimeResult<T> {
        if self.status != 0 {
            return Err(RuntimeError::Transport {
                rule: "runtime refused the command",
            });
        }
        serde_json::from_str(&self.stdout).map_err(|_| {
            RuntimeError::Domain(DomainError::invalid(
                subject,
                "is not the Paseo 0.2.5 JSON this adapter is pinned to",
            ))
        })
    }
}

// ---------------------------------------------------------------------------
// Daemon protocol requests
// ---------------------------------------------------------------------------

/// One authenticated daemon protocol request.
///
/// `request_id` is the correlation key, and the transport must refuse an answer
/// that carries a different one. That is not defensive plumbing: the socket is
/// multiplexed, so an answer matched by arrival order rather than by id is an
/// answer about somebody else's agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoRpc {
    /// The protocol method.
    pub method: &'static str,
    /// The request correlation id.
    pub request_id: String,
    /// The request parameters.
    pub params: serde_json::Value,
    /// Whether this request can change Paseo.
    pub mutates: bool,
}

impl PaseoRpc {
    /// `server_info` — identity, version and advertised features.
    #[must_use]
    pub fn server_info(request_id: String) -> Self {
        Self::read("server_info", request_id, serde_json::json!({}))
    }

    /// `project.list.request`.
    #[must_use]
    pub fn project_list(request_id: String) -> Self {
        Self::read("project.list.request", request_id, serde_json::json!({}))
    }

    /// `project.add.request`, keyed by the durable command id.
    ///
    /// The request id *is* the command id, so a redelivery of the same intent
    /// carries the same correlation and cannot be mistaken for a second one.
    #[must_use]
    pub fn project_add(request_id: String, path: &str, name: &str) -> Self {
        Self::mutate(
            "project.add.request",
            request_id,
            serde_json::json!({ "path": path, "name": name }),
        )
    }

    /// `workspace.list.request`, narrowed to one project.
    #[must_use]
    pub fn workspace_list(request_id: String, project_id: &str) -> Self {
        Self::read(
            "workspace.list.request",
            request_id,
            serde_json::json!({ "projectId": project_id }),
        )
    }

    /// `workspace.fetch.request` — the authoritative readback by exact id.
    #[must_use]
    pub fn workspace_fetch(request_id: String, workspace_id: &str) -> Self {
        Self::read(
            "workspace.fetch.request",
            request_id,
            serde_json::json!({ "workspaceId": workspace_id }),
        )
    }

    /// `agent.list.request` — the census discovery and recovery run over.
    #[must_use]
    pub fn agent_list(request_id: String, project_id: &str) -> Self {
        Self::read(
            "agent.list.request",
            request_id,
            serde_json::json!({ "projectId": project_id }),
        )
    }

    /// `agent.fetch.request` — the authoritative readback by exact id.
    #[must_use]
    pub fn agent_fetch(request_id: String, agent_id: &str) -> Self {
        Self::read(
            "agent.fetch.request",
            request_id,
            serde_json::json!({ "agentId": agent_id }),
        )
    }

    /// `fetch_agent_timeline_request` under one projection.
    ///
    /// The projection is a parameter rather than a constant so the recorded
    /// suite can prove what `projected` costs; every production call site passes
    /// [`PaseoProjection::Canonical`].
    #[must_use]
    pub fn timeline_fetch(
        request_id: String,
        agent_id: &str,
        projection: PaseoProjection,
        after: Option<u64>,
        limit: u32,
    ) -> Self {
        Self::read(
            "fetch_agent_timeline_request",
            request_id,
            serde_json::json!({
                "agentId": agent_id,
                "projection": projection.as_str(),
                "after": after,
                "limit": limit,
            }),
        )
    }

    /// `setAgentTimelineSubscription` — narrow the live stream to one agent.
    #[must_use]
    pub fn timeline_subscribe(request_id: String, agent_id: &str, after: u64) -> Self {
        Self::read(
            "setAgentTimelineSubscription",
            request_id,
            serde_json::json!({ "agentId": agent_id, "after": after }),
        )
    }

    /// `send_agent_message_request` with the caller's own message id.
    #[must_use]
    pub fn send_message(request_id: String, agent_id: &str, message_id: &str, body: &str) -> Self {
        Self::mutate(
            "send_agent_message_request",
            request_id,
            serde_json::json!({
                "agentId": agent_id,
                "messageId": message_id,
                "body": body,
            }),
        )
    }

    /// `agent_permission_response`, bound to the exact pending request.
    #[must_use]
    pub fn permission_response(
        request_id: String,
        agent_id: &str,
        permission_id: &str,
        decision: &str,
    ) -> Self {
        Self::mutate(
            "agent_permission_response",
            request_id,
            serde_json::json!({
                "agentId": agent_id,
                "permissionId": permission_id,
                "decision": decision,
            }),
        )
    }

    /// The ledger key: the method only, never the parameters.
    #[must_use]
    pub fn route(&self) -> String {
        format!("rpc {}", self.method)
    }

    const fn read(method: &'static str, request_id: String, params: serde_json::Value) -> Self {
        Self {
            method,
            request_id,
            params,
            mutates: false,
        }
    }

    const fn mutate(method: &'static str, request_id: String, params: serde_json::Value) -> Self {
        Self {
            method,
            request_id,
            params,
            mutates: true,
        }
    }
}

/// One daemon answer, still correlated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoFrame {
    /// The request this frame answers.
    pub request_id: String,
    /// The payload, when the daemon answered successfully.
    pub result: Option<serde_json::Value>,
    /// The daemon's own error code, when it refused.
    pub error_code: Option<String>,
}

impl PaseoFrame {
    /// A successful answer.
    #[must_use]
    pub const fn ok(request_id: String, result: serde_json::Value) -> Self {
        Self {
            request_id,
            result: Some(result),
            error_code: None,
        }
    }

    /// A refusal.
    #[must_use]
    pub const fn failed(request_id: String, error_code: String) -> Self {
        Self {
            request_id,
            result: None,
            error_code: Some(error_code),
        }
    }

    /// Resolve this frame against the request it must answer.
    ///
    /// The correlation check is the whole point of the type. On one multiplexed
    /// socket an answer taken by arrival order is an answer about whatever the
    /// daemon happened to finish first, which for a `workspace.fetch` is another
    /// project's workspace — accepted, bound, and then edited in.
    ///
    /// # Errors
    /// * [`RuntimeError::Transport`] — the frame answers another request, or the
    ///   daemon refused.
    /// * [`RuntimeError::Domain`] — the payload is not the pinned 0.2.5 shape.
    pub fn resolve<T: serde::de::DeserializeOwned>(
        &self,
        request: &PaseoRpc,
        subject: &'static str,
    ) -> RuntimeResult<T> {
        if self.request_id != request.request_id {
            return Err(RuntimeError::Transport {
                rule: "answer carried another request's correlation id",
            });
        }
        let Some(result) = &self.result else {
            // The daemon's own message can quote a path or a prompt, so only the
            // fact of refusal survives into the error.
            return Err(RuntimeError::Transport {
                rule: "runtime refused the request",
            });
        };
        ensure_frame_bounded(result)?;
        serde_json::from_value(result.clone()).map_err(|_| {
            RuntimeError::Domain(DomainError::invalid(
                subject,
                "is not the Paseo 0.2.5 frame this adapter is pinned to",
            ))
        })
    }
}

// ---------------------------------------------------------------------------
// The seam
// ---------------------------------------------------------------------------

/// The seam between the adapter's policy and Paseo's two surfaces.
#[async_trait]
pub trait PaseoTransport: Send + Sync + fmt::Debug {
    /// Run one CLI command.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the channel failed. That is a
    /// fact about the channel and never about the work: an implementation must
    /// not turn a timeout into an empty success.
    async fn run(&self, command: &PaseoCommand) -> RuntimeResult<PaseoOutput>;

    /// Make one daemon protocol request.
    ///
    /// # Errors
    /// As [`PaseoTransport::run`].
    async fn request(&self, request: &PaseoRpc) -> RuntimeResult<PaseoFrame>;

    /// Drain the frames the daemon pushed for `agent_id` since the subscription
    /// was activated.
    ///
    /// Separated from [`PaseoTransport::request`] because a subscription is not
    /// a request/response pair: frames arrive unsolicited, and the adapter has
    /// to be able to buffer them *while* it runs the canonical catch-up fetch
    /// that closes the history/live race.
    ///
    /// # Errors
    /// As [`PaseoTransport::run`].
    async fn drain_stream(&self, agent_id: &str) -> RuntimeResult<Vec<serde_json::Value>>;
}

/// The live transport: a real Paseo executable, and the daemon socket that is
/// not built into this adapter.
///
/// # What runs
///
/// [`PaseoTransport::run`] dispatches the real CLI with an argv array, one
/// deadline, one output bound, and `--host` appended from a [`SecretString`]
/// this type owns. No shell is involved at any point.
///
/// # What does not
///
/// [`PaseoTransport::request`] and [`PaseoTransport::drain_stream`] refuse. The
/// daemon protocol's semantics are implemented — see [`PaseoRpc`],
/// [`PaseoFrame`] and [`crate::wire`] — and the whole adapter is proved against
/// them through [`crate::fixture::RecordedPaseo`]; what is absent is the
/// WebSocket that carries the frames, which needs an exact workspace-pinned
/// dependency the root manifest does not have. `kontor-runtime-ao` left its
/// `/mux` client out for the same reason and in the same shape. Hand-rolling
///framing to dodge that gate is rejected (ALT-006), and answering a protocol
/// request with a plausible empty success would be worse than refusing: every
/// readback in this adapter exists precisely because the CLI's answer is not
/// enough.
#[derive(Debug)]
pub struct PaseoLiveTransport {
    executable: String,
    host: SecretString,
    timeout_seconds: u64,
}

impl PaseoLiveTransport {
    /// Build a transport that dispatches `executable` against `host_target`.
    ///
    /// `host_target` is the complete `--host` argument, password and all. It is
    /// taken as a [`SecretString`] so the caller cannot have been holding it in
    /// an ordinary `String` that a `Debug` derive would print.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] for an empty executable or an empty host
    /// target.
    pub fn new(
        executable: &str,
        host_target: SecretString,
        timeout_seconds: u64,
    ) -> RuntimeResult<Self> {
        if executable.is_empty() {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "PaseoExecutable",
                "must not be empty",
            )));
        }
        if host_target.expose_secret().is_empty() {
            return Err(RuntimeError::Domain(DomainError::invalid(
                "PaseoHostTarget",
                "must not be empty",
            )));
        }
        Ok(Self {
            executable: executable.to_owned(),
            host: host_target,
            timeout_seconds,
        })
    }
}

#[async_trait]
impl PaseoTransport for PaseoLiveTransport {
    async fn run(&self, command: &PaseoCommand) -> RuntimeResult<PaseoOutput> {
        command.ensure_dispatchable()?;
        let mut process = tokio::process::Command::new(&self.executable);
        process.args(command.argv());
        // Resolved here and nowhere else, immediately before dispatch.
        process.arg("--host").arg(self.host.expose_secret());
        for (name, value) in command.env() {
            process.env(name, value);
        }
        process.stdin(std::process::Stdio::null());
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_seconds),
            process.output(),
        )
        .await
        .map_err(|_| RuntimeError::Transport {
            rule: "runtime did not answer within the command deadline",
        })?
        .map_err(|_| RuntimeError::Transport {
            rule: "channel failed before the runtime answered",
        })?;
        if output.stdout.len() > MAX_OUTPUT_BYTES {
            return Err(RuntimeError::Transport {
                rule: "answer exceeded the bounded output size",
            });
        }
        // Stderr is deliberately dropped unread. Paseo writes the host URI it
        // was given into some diagnostics, and that URI is the credential.
        let stdout = String::from_utf8(output.stdout).map_err(|_| RuntimeError::Transport {
            rule: "answer was not valid UTF-8",
        })?;
        Ok(PaseoOutput::new(output.status.code().unwrap_or(-1), stdout))
    }

    async fn request(&self, _request: &PaseoRpc) -> RuntimeResult<PaseoFrame> {
        Err(RuntimeError::Transport {
            rule: "daemon protocol socket is not compiled into this adapter build",
        })
    }

    async fn drain_stream(&self, _agent_id: &str) -> RuntimeResult<Vec<serde_json::Value>> {
        Err(RuntimeError::Transport {
            rule: "daemon protocol socket is not compiled into this adapter build",
        })
    }
}

/// Refuse a raw daemon frame larger than the accepted bound.
///
/// Enforced at every point a frame is accepted — [`PaseoFrame::resolve`] for the
/// request/response half and the subscription drain for the pushed half — and
/// *before* the frame is deserialized into anything, because a bound checked
/// after parsing has already paid the cost it exists to refuse.
///
/// # Errors
/// Returns [`RuntimeError::Transport`] for a frame over [`MAX_FRAME_BYTES`].
pub fn ensure_frame_bounded(raw: &serde_json::Value) -> RuntimeResult<()> {
    // ponytail: re-serializing to measure is one pass over a frame that is
    // about to be parsed anyway; a streaming byte count belongs with the
    // WebSocket reader, which is where the bytes actually arrive.
    if serde_json::to_string(raw).is_ok_and(|text| text.len() <= MAX_FRAME_BYTES) {
        return Ok(());
    }
    Err(RuntimeError::Transport {
        rule: "frame exceeded the bounded frame size",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> BTreeMap<String, String> {
        [("kontor.role".to_owned(), "implement".to_owned())]
            .into_iter()
            .collect()
    }

    #[test]
    fn every_lifecycle_command_is_json_and_carries_no_host() {
        let commands = [
            PaseoCommand::version(),
            PaseoCommand::workspace_create(
                "/w/task-1",
                "prj_1",
                "KON-MVP-11 Paseo adapter",
                &labels(),
            ),
            PaseoCommand::workspace_archive("wks_1"),
            PaseoCommand::agent_run(
                "wks_1",
                "/w/task-1",
                "KON-MVP-11 Implement",
                &labels(),
                "agt_orchestrator",
                "do the work",
            ),
            PaseoCommand::agent_inspect("agt_1"),
            PaseoCommand::agent_update_labels("agt_1", &labels()),
            PaseoCommand::agent_reload("agt_1"),
            PaseoCommand::agent_stop("agt_1"),
            PaseoCommand::agent_archive("agt_1"),
        ];
        for command in &commands {
            assert!(
                command.argv().contains(&JSON_FLAG.to_owned()),
                "{} does not ask for JSON",
                command.route()
            );
            assert!(
                !command.argv().iter().any(|arg| arg == "--host"),
                "{} carries a host target the transport must own",
                command.route()
            );
        }
    }

    #[test]
    fn the_ledger_route_never_quotes_the_operators_work() {
        let command = PaseoCommand::agent_run(
            "wks_1",
            "/private/worktrees/secret-project",
            "KON-MVP-11 Implement",
            &labels(),
            "agt_orchestrator",
            "the actual prompt",
        );
        assert_eq!(command.route(), "agent run");
        assert!(!command.route().contains("the actual prompt"));
        assert!(!command.route().contains("secret-project"));
        // …while the argv, which only the transport sees, still carries them.
        assert!(command.argv().iter().any(|arg| arg == "the actual prompt"));
    }

    #[test]
    fn a_parent_agent_travels_in_the_environment_rather_than_a_flag() {
        let command = PaseoCommand::agent_run(
            "wks_1",
            "/w/task-1",
            "t",
            &labels(),
            "agt_orchestrator",
            "p",
        );
        assert_eq!(
            command.env(),
            [(PARENT_AGENT_ENV.to_owned(), "agt_orchestrator".to_owned())]
        );
    }

    #[test]
    fn a_flag_shaped_identifier_cannot_become_another_option() {
        // Paseo ids are opaque foreign strings. `--force` interpolated where a
        // workspace id belongs is an option, not a value.
        assert!(
            PaseoCommand::agent_inspect("--force")
                .ensure_dispatchable()
                .is_err()
        );
        assert!(
            PaseoCommand::workspace_create("--isolation", "prj_1", "t", &labels())
                .ensure_dispatchable()
                .is_err()
        );
        // …and a command whose own flags sit next to each other is fine, which
        // a whole-argv scan would have refused.
        PaseoCommand::agent_run("wks_1", "/w/task-1", "t", &labels(), "agt_p", "p")
            .ensure_dispatchable()
            .expect("`--background --workspace wks_1` is an ordinary command");
        PaseoCommand::agent_inspect("agt_1")
            .ensure_dispatchable()
            .expect("an ordinary id dispatches");
    }

    #[test]
    fn only_writes_are_counted_as_mutations() {
        assert!(!PaseoCommand::version().mutates());
        assert!(!PaseoCommand::agent_inspect("agt_1").mutates());
        assert!(PaseoCommand::agent_run("w", "/w/t", "t", &labels(), "agt_p", "p").mutates());
        assert!(PaseoCommand::workspace_create("/w/t", "p", "t", &labels()).mutates());
        assert!(PaseoCommand::agent_update_labels("agt_1", &labels()).mutates());
        assert!(!PaseoRpc::project_list("req-1".to_owned()).mutates);
        assert!(PaseoRpc::project_add("req-1".to_owned(), "/w", "n").mutates);
    }

    #[test]
    fn an_answer_for_another_request_is_refused_rather_than_read() {
        let request = PaseoRpc::agent_fetch("req-1".to_owned(), "agt_1");
        let mine = PaseoFrame::ok("req-1".to_owned(), serde_json::json!({ "id": "agt_1" }));
        let theirs = PaseoFrame::ok("req-2".to_owned(), serde_json::json!({ "id": "agt_9" }));
        mine.resolve::<serde_json::Value>(&request, "PaseoAgent")
            .expect("my own answer resolves");
        assert_eq!(
            theirs
                .resolve::<serde_json::Value>(&request, "PaseoAgent")
                .expect_err("another request's answer is not mine"),
            RuntimeError::Transport {
                rule: "answer carried another request's correlation id"
            }
        );
    }

    #[test]
    fn a_daemon_refusal_carries_no_payload() {
        let request = PaseoRpc::agent_fetch("req-1".to_owned(), "agt_1");
        let refused = PaseoFrame::failed(
            "req-1".to_owned(),
            "/Users/someone/secret-project".to_owned(),
        );
        let error = refused
            .resolve::<serde_json::Value>(&request, "PaseoAgent")
            .expect_err("a refusal is not an answer");
        assert!(!format!("{error:?}").contains("secret-project"));
    }

    #[test]
    fn a_non_zero_exit_is_a_channel_fact_and_reads_no_output() {
        let refused = PaseoOutput::new(1, "{\"id\":\"agt_1\"}".to_owned());
        assert_eq!(
            refused
                .parse::<serde_json::Value>("PaseoCliAgent")
                .expect_err("a non-zero exit is not an answer"),
            RuntimeError::Transport {
                rule: "runtime refused the command"
            }
        );
    }

    #[test]
    fn a_live_transport_needs_an_executable_and_a_host() {
        assert!(
            PaseoLiveTransport::new("paseo", SecretString::from("https://host".to_owned()), 30)
                .is_ok()
        );
        assert!(
            PaseoLiveTransport::new("", SecretString::from("https://host".to_owned()), 30).is_err()
        );
        assert!(PaseoLiveTransport::new("paseo", SecretString::from(String::new()), 30).is_err());
    }

    #[test]
    fn a_live_transport_never_prints_its_host_target() {
        let transport = PaseoLiveTransport::new(
            "paseo",
            SecretString::from("https://u:p@host".to_owned()),
            30,
        )
        .expect("a valid transport");
        let printed = format!("{transport:?}");
        assert!(!printed.contains("u:p@host"), "got {printed}");
    }
}
