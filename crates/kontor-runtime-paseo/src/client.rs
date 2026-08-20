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
//! # The 0.3.1 socket
//!
//! [`PaseoLiveTransport`] speaks the live session protocol at
//! `ws://127.0.0.1:6767/ws`:
//!
//! 1. connect, then send exactly one `hello` carrying
//!    [`PASEO_WS_PROTOCOL_VERSION`] and [`PASEO_APP_VERSION`] as separate
//!    fields;
//! 2. wait for the daemon's pushed `status/server_info` and gate on it — no
//!    operational request is written before that push agrees with both pins;
//! 3. wrap every request as `{"type":"session","message":{…}}`, and accept an
//!    answer only when its `requestId` *and* its response type are the exact
//!    pair the request declared;
//! 4. route `agent_stream` frames by `payload.agentId` into a bounded per-agent
//!    queue, because they are never anybody's answer.
//!
//! One reader task owns the socket's read half and demultiplexes; writes are
//! serialized behind the connection lock. A reconnect throws away pending
//! correlation and buffered frames and re-gates from a fresh push, because a
//! request issued before a disconnect is not answered by a daemon that has
//! since restarted.
//!
//! # Secrets
//!
//! Paseo accepts a remote password only inside its `--host` URI, so the
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

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use kontor_core::DomainError;
use kontor_core::spec::{ModelRung, SeatAutonomy};
use kontor_runtime::adapter::{RuntimeError, RuntimeResult};
use secrecy::{ExposeSecret, SecretString};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::wire::{
    MAX_FRAME_BYTES, MAX_OUTPUT_BYTES, MAX_STREAM_QUEUE, PASEO_APP_VERSION,
    PASEO_CAP_SELECTIVE_AGENT_TIMELINE, PASEO_CLIENT_TYPE, PASEO_WS_PROTOCOL_VERSION,
    PaseoDirection, PaseoProjection, PaseoServerInfo, PaseoTimelineCursor,
};

/// The JSON flag every lifecycle command carries.
const JSON_FLAG: &str = "--json";

/// The environment variable Paseo reads the parent agent from.
pub const PARENT_AGENT_ENV: &str = "PASEO_AGENT_ID";

/// Paseo's provider-specific spelling of one [`SeatAutonomy`].
///
/// `paseo agent run --mode` is explicitly provider-specific. The same policy
/// therefore cannot be translated before the selected provider is known:
/// Codex, for example, accepts `auto-review` and `full-access`, while Claude
/// accepts `auto`, `bypassPermissions` and `plan`.
///
/// The mapping is exhaustive and fails closed when a provider cannot express
/// an advisory seat. Falling back to a generic spelling is not harmless: Paseo
/// 0.4.0 rejects `default` for Codex before creating the agent, which left every
/// replacement verifier permanently queued.
pub(crate) fn paseo_mode(
    provider: &str,
    autonomy: SeatAutonomy,
) -> RuntimeResult<Option<&'static str>> {
    match autonomy {
        SeatAutonomy::Supervised => permission_mode(provider),
        SeatAutonomy::Bounded => match provider {
            "claude" => Ok(Some("bypassPermissions")),
            "codex" => Ok(Some("full-access")),
            "copilot" => Ok(Some("allow-all")),
            "opencode" => Ok(Some("build")),
            "pi" => Ok(None),
            "omp" => Ok(Some("full")),
            other => Err(RuntimeError::PermissionModeUnsupported {
                provider: other.to_owned(),
            }),
        },
        SeatAutonomy::Advisory => match provider {
            "claude" | "opencode" => Ok(Some("plan")),
            "copilot" => Ok(Some(
                "https://agentclientprotocol.com/protocol/session-modes#plan",
            )),
            other => Err(RuntimeError::PermissionModeUnsupported {
                provider: other.to_owned(),
            }),
        },
    }
}

/// The explicit supervised mode Paseo exposes for each delivery provider.
///
/// Kontor pins this explicitly because an omitted mode delegates authority to
/// a mutable provider default (including Claude's `default` / Always Ask). The
/// [`paseo_mode`] selects this branch for supervised seats and the readback path
/// uses the same function, so launch and verification cannot disagree.
pub(crate) fn permission_mode(provider: &str) -> RuntimeResult<Option<&'static str>> {
    match provider {
        "claude" => Ok(Some("auto")),
        "codex" => Ok(Some("auto-review")),
        "copilot" => Ok(Some(
            "https://agentclientprotocol.com/protocol/session-modes#agent",
        )),
        "opencode" => Ok(Some("build")),
        "pi" => Ok(None),
        "omp" => Ok(Some("full")),
        _ => Err(RuntimeError::PermissionModeUnsupported {
            provider: provider.to_owned(),
        }),
    }
}

/// The provider-native non-mutating mode used by consultation seats.
pub(crate) fn consultation_permission_mode(provider: &str) -> RuntimeResult<Option<&'static str>> {
    match provider {
        "claude" | "cursor" => Ok(Some("plan")),
        "codex" => Ok(Some("auto-review")),
        // Providers without a proven read-only mode are not consultation-safe.
        other => Err(RuntimeError::PermissionModeUnsupported {
            provider: other.to_owned(),
        }),
    }
}

// ---------------------------------------------------------------------------
// CLI commands
// ---------------------------------------------------------------------------

/// One Paseo CLI invocation, as an argv array.
///
/// `argv` never contains `--host`: the live transport appends it. The
/// constructors below are the only way to build one, so no call site can invent
/// an unversioned subcommand or forget `--json`.
#[derive(Clone, PartialEq, Eq)]
pub struct PaseoCommand {
    argv: Vec<String>,
    /// The final positional argument, when the subcommand takes one.
    ///
    /// Held apart from `argv` because it has to be written **last**, after the
    /// transport has appended its own `--host`. A trailing positional placed
    /// while the command is being built ends up in front of that flag, and
    /// everything after the `--` terminator is a positional — so the host target
    /// would arrive as two more prompt words and the CLI would refuse the whole
    /// invocation. A live Grade-A launch caught exactly that.
    trailing: Option<String>,
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

impl std::fmt::Debug for PaseoCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PaseoCommand")
            .field("argv", &self.argv)
            .field("trailing", &self.trailing)
            .field("values", &self.values)
            .field("route", &self.route)
            .field(
                "env_names",
                &self.env.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            )
            .field("mutates", &self.mutates)
            .finish()
    }
}

impl PaseoCommand {
    /// `paseo --version --json`.
    ///
    /// 0.3.1 prints the bare version string for this one, not JSON, so its
    /// answer is read as text. See [`PaseoOutput::version`].
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
    /// There is no `--label`: Kontor therefore stores the native workspace id
    /// in its own durable binding rather than exposing machine identity here.
    #[must_use]
    pub fn workspace_create(canonical_cwd: &str, project_id: &str, title: &str) -> Self {
        Self::mutate(
            Argv::new(&["workspace", "create", "--isolation", "local"])
                .option("--path", canonical_cwd)
                .option("--project", project_id)
                .option("--title", title),
            "workspace create".to_owned(),
        )
    }

    /// `paseo workspace archive {id}`.
    #[must_use]
    pub fn workspace_archive(workspace_id: &str) -> Self {
        Self::mutate(
            Argv::new(&["workspace", "archive"]).value(workspace_id),
            format!("workspace archive {workspace_id}"),
        )
    }

    /// `paseo agent run --background --workspace … --cwd … --title … --label … {prompt}`.
    ///
    /// Both `--workspace` and `--cwd` travel. Either alone is a hierarchy that
    /// can be right by accident: a workspace id with no directory would let
    /// Paseo pick one, and a directory with no workspace id would place the
    /// agent wherever that path currently resolves.
    ///
    /// The prompt is the trailing **positional** argument, which is 0.3.1's
    /// shape (`paseo agent run [options] <prompt>`); there is no `--prompt`.
    ///
    /// `--provider` is mandatory on this release.
    ///
    /// `--mode` carries the seat's declared [`SeatAutonomy`]. It is always sent,
    /// never omitted for the supervised case: omitting it would leave the
    /// authority a seat runs under to whatever the runtime happens to default to,
    /// which is how every seat came to ask about every tool call in the first
    /// place. Kontor declaring `default` and Paseo choosing `default` look the
    /// same on the wire and are not the same thing — only one of them is a
    /// decision.
    // The arity is the CLI's own: each parameter is one flag `paseo agent run`
    // takes, in the order it takes them. Bundling them into a struct would hide
    // exactly the mapping this function exists to make checkable.
    #[allow(clippy::too_many_arguments)]
    pub fn agent_run(
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        autonomy: SeatAutonomy,
        title: &str,
        labels: &BTreeMap<String, String>,
        parent_agent_id: &str,
        prompt: &str,
    ) -> RuntimeResult<Self> {
        let mode = paseo_mode(model_rung.provider.0.as_str(), autonomy)?;
        Self::agent_run_with_mode(
            workspace_id,
            canonical_cwd,
            model_rung,
            title,
            labels,
            parent_agent_id,
            prompt,
            mode,
        )
    }

    /// Start a consultation in the provider's non-mutating review/plan mode.
    #[allow(clippy::too_many_arguments)]
    pub fn consultation_run(
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        title: &str,
        labels: &BTreeMap<String, String>,
        parent_agent_id: &str,
        prompt: &str,
        credential: &str,
    ) -> RuntimeResult<Self> {
        let mode = consultation_permission_mode(model_rung.provider.0.as_str())?;
        let mut command = Self::agent_run_with_mode(
            workspace_id,
            canonical_cwd,
            model_rung,
            title,
            labels,
            parent_agent_id,
            prompt,
            mode,
        )?;
        command
            .env
            .push(("KONTOR_AUTH".to_owned(), credential.to_owned()));
        Ok(command)
    }

    #[allow(clippy::too_many_arguments)]
    fn agent_run_with_mode(
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        title: &str,
        labels: &BTreeMap<String, String>,
        parent_agent_id: &str,
        prompt: &str,
        permission_mode: Option<&str>,
    ) -> RuntimeResult<Self> {
        let mut argv = Argv::new(&["agent", "run", "--background"])
            .option("--workspace", workspace_id)
            .option("--cwd", canonical_cwd)
            .option("--provider", &model_rung.provider.0)
            .option("--model", &model_rung.model.0);
        if let Some(permission_mode) = permission_mode {
            argv = argv.option("--mode", permission_mode);
        }
        if let Some(effort) = model_rung.effort {
            argv = argv.option("--thinking", effort.as_str());
        }
        let mut argv = argv.option("--title", title);
        for (key, value) in labels {
            argv = argv.option("--label", &format!("{key}={value}"));
        }
        // Everything Paseo parses as a flag is already behind us, so the prompt
        // is positional and terminates the option list.
        argv = argv.trailing(prompt);
        let mut command = Self::mutate(argv, "agent run".to_owned());
        command
            .env
            .push((PARENT_AGENT_ENV.to_owned(), parent_agent_id.to_owned()));
        Ok(command)
    }

    /// `paseo agent update {id} --label …` — the adoption write, and nothing else.
    ///
    /// Note there is no `--mode` here: 0.3.1's `agent update` takes a name, a
    /// thinking option and labels, and nothing that changes authority. A seat's
    /// autonomy is therefore fixed at launch, which is the honest shape — it is
    /// part of what the launch was admitted as, not a dial to turn afterwards.
    #[must_use]
    pub fn agent_update_labels(agent_id: &str, labels: &BTreeMap<String, String>) -> Self {
        Self::agent_update(agent_id, None, labels)
    }

    /// `paseo agent update {id} --name {title} --label …` repairs display and
    /// correlation projection without changing provider, model or authority.
    #[must_use]
    pub fn agent_update(
        agent_id: &str,
        title: Option<&str>,
        labels: &BTreeMap<String, String>,
    ) -> Self {
        let mut argv = Argv::new(&["agent", "update"]).value(agent_id);
        if let Some(title) = title {
            argv = argv.option("--name", title);
        }
        for (key, value) in labels {
            argv = argv.option("--label", &format!("{key}={value}"));
        }
        Self::mutate(argv, format!("agent update {agent_id}"))
    }

    /// `paseo agent reload {id}` — a process restart for a closed agent, and
    /// never a way to simulate a new turn or a compaction.
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

    /// The option half of the argv, without `--host` and without the trailing
    /// positional.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// The final positional argument, when this command has one.
    ///
    /// A transport writes it after every flag it appends of its own, preceded by
    /// `--` so a value beginning with a dash is that value and not an option.
    #[must_use]
    pub fn trailing(&self) -> Option<&str> {
        self.trailing.as_deref()
    }

    /// The complete argv a dispatch produces, `--host` aside.
    ///
    /// Only for evidence and assertions; the live transport builds the real one
    /// itself because it alone holds the host target.
    #[must_use]
    pub fn dispatched_argv(&self) -> Vec<String> {
        let mut argv = self.argv.clone();
        if let Some(trailing) = &self.trailing {
            argv.push("--".to_owned());
            argv.push(trailing.clone());
        }
        argv
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

    /// The visible title this command sets, if it sets one.
    ///
    /// Narrow on purpose, and deliberately not part of [`PaseoCommand::route`].
    /// The route omits every foreign value so a ledger assertion can never
    /// quote an operator's prompt; a container's *title* is a different thing.
    /// It is the name humans read in the runtime, it is Kontor's to decide from
    /// its own scope, and a contract that could not assert it would let the one
    /// visible half of a placement drift unnoticed — which is exactly how a
    /// workspace ends up named after a node id.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.argv
            .iter()
            .position(|argument| argument == "--title" || argument == "--name")
            .and_then(|flag| self.argv.get(flag + 1))
            .map(String::as_str)
    }

    /// Refuse a foreign value that would be read as a flag.
    ///
    /// Paseo ids, paths and titles are foreign strings. One beginning with `-`
    /// lands in argv as an option rather than as the value it was meant to be,
    /// which is the argv analogue of interpolating an id into a URL path: a
    /// workspace id of `--force` is not a workspace at all.
    ///
    /// The trailing prompt is exempt, because it is positional and this adapter
    /// puts `--` in front of it.
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
        let Argv {
            mut argv,
            values,
            trailing,
        } = argv;
        argv.push(JSON_FLAG.to_owned());
        Self {
            argv,
            values,
            trailing,
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
    trailing: Option<String>,
}

impl Argv {
    /// Start from literal subcommand words and flags this adapter wrote itself.
    fn new(parts: &[&str]) -> Self {
        Self {
            argv: parts.iter().map(|part| (*part).to_owned()).collect(),
            values: Vec::new(),
            trailing: None,
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

    /// The final positional argument, placed after `--`.
    fn trailing(mut self, value: &str) -> Self {
        self.trailing = Some(value.to_owned());
        self
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
    /// * [`RuntimeError::Domain`] — output that is not the pinned 0.3.1 shape,
    ///   which includes output that is not JSON at all.
    pub fn parse<T: serde::de::DeserializeOwned>(&self, subject: &'static str) -> RuntimeResult<T> {
        self.succeeded()?;
        serde_json::from_str(self.json_body()).map_err(|_| {
            RuntimeError::Domain(DomainError::invalid(
                subject,
                "is not the Paseo 0.3.1 JSON this adapter is pinned to",
            ))
        })
    }

    /// The JSON document inside `--json` output.
    ///
    /// 0.3.1 writes operator chatter to **stdout** ahead of the payload — a
    /// launch prints `Using workspace wks_…` before its object — so the stream
    /// is "a notice, then JSON" rather than JSON. Parsing from the first opening
    /// brace is the smallest thing that reads the document Paseo meant to send
    /// without inventing a tolerance for anything else: a body that is not JSON
    /// from there on still fails, and the notice never reaches a DTO.
    fn json_body(&self) -> &str {
        let start = self.stdout.find(['{', '[']).unwrap_or(self.stdout.len());
        self.stdout.get(start..).unwrap_or_default()
    }

    /// The bare version string `paseo --version --json` prints.
    ///
    /// Text rather than JSON, because that is what 0.3.1 actually writes: the
    /// root `--version` flag short-circuits the formatter.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] for a non-zero exit.
    pub fn version(&self) -> RuntimeResult<String> {
        self.succeeded()?;
        Ok(self.stdout.trim().to_owned())
    }

    fn succeeded(&self) -> RuntimeResult<()> {
        if self.status == 0 {
            return Ok(());
        }
        Err(RuntimeError::Transport {
            rule: "runtime refused the command",
        })
    }
}

// ---------------------------------------------------------------------------
// Session protocol requests
// ---------------------------------------------------------------------------

/// One session-protocol request, with the exact response type that answers it.
///
/// `request_id` is the correlation key and `response_type` is the other half of
/// it. Neither alone is enough on a multiplexed socket: an answer matched by
/// arrival order is an answer about somebody else's agent, and an answer matched
/// by id alone accepts an `rpc_error` — or any other frame the daemon chose to
/// stamp with that id — as the readback a placement rule is about to be decided
/// from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoRpc {
    /// The session message, ready to be wrapped in the `session` envelope.
    pub message: serde_json::Value,
    /// The inbound message type, for the ledger.
    pub request_type: &'static str,
    /// The exact outbound message type that answers it.
    pub response_type: &'static str,
    /// The request correlation id.
    pub request_id: String,
    /// Whether this request can change Paseo.
    pub mutates: bool,
}

impl PaseoRpc {
    /// `daemon.get_status.request` — the correlated version readback.
    #[must_use]
    pub fn daemon_status(request_id: String) -> Self {
        Self::read(
            "daemon.get_status.request",
            "daemon.get_status.response",
            request_id,
            serde_json::json!({}),
        )
    }

    /// `project.list.request`.
    #[must_use]
    pub fn project_list(request_id: String) -> Self {
        Self::read(
            "project.list.request",
            "project.list.response",
            request_id,
            serde_json::json!({}),
        )
    }

    /// `project.add.request`, keyed by the durable command id.
    ///
    ///
    /// The request id *is* the command id, so a redelivery of the same intent
    /// carries the same correlation and cannot be mistaken for a second one.
    #[must_use]
    pub fn project_add(request_id: String, cwd: &str) -> Self {
        Self::mutate(
            "project.add.request",
            "project.add.response",
            request_id,
            // `cwd`, and only `cwd`. The 0.2.5 spelling was `path` with a
            // `name`, and 0.3.1 accepts neither: a live probe against the
            // qualified daemon answered `path` with "Unknown request, try
            // upgrading the daemon", because the inbound schema is
            // `{type, cwd, requestId}` and a message that misses it never
            // reaches a handler. There is no name field at all — the daemon
            // derives a project's display name from the directory, which is why
            // `prepare_project` reports drift instead of setting one.
            serde_json::json!({ "cwd": cwd }),
        )
    }

    /// `project.rename.request`, available only when the exact daemon
    /// connection satisfies [`PaseoServerInfo::supports_project_rename`].
    #[must_use]
    pub fn project_rename(request_id: String, project_id: &str, custom_name: &str) -> Self {
        Self::mutate(
            "project.rename.request",
            "project.rename.response",
            request_id,
            serde_json::json!({
                "projectId": project_id,
                "customName": custom_name,
            }),
        )
    }

    /// `fetch_workspaces_request`, narrowed to one project and one bounded page.
    ///
    /// 0.3.1 has no fetch-one-workspace request; the authoritative readback of a
    /// single workspace is this list plus an exact-id select, which is why the
    /// filter and the page bound are not optional here.
    #[must_use]
    pub fn workspace_list(
        request_id: String,
        project_id: &str,
        limit: u32,
        cursor: Option<&str>,
    ) -> Self {
        let mut page = serde_json::json!({ "limit": limit });
        if let Some(cursor) = cursor {
            page["cursor"] = serde_json::json!(cursor);
        }
        Self::read(
            "fetch_workspaces_request",
            "fetch_workspaces_response",
            request_id,
            serde_json::json!({
                "filter": { "projectId": project_id },
                "page": page,
            }),
        )
    }

    /// `fetch_agents_request`, narrowed by exact labels and one bounded page.
    #[must_use]
    pub fn agent_list(
        request_id: String,
        labels: &BTreeMap<String, String>,
        include_archived: bool,
        limit: u32,
        cursor: Option<&str>,
    ) -> Self {
        let mut page = serde_json::json!({ "limit": limit });
        if let Some(cursor) = cursor {
            page["cursor"] = serde_json::json!(cursor);
        }
        let mut filter = serde_json::json!({ "includeArchived": include_archived });
        if !labels.is_empty() {
            filter["labels"] = serde_json::json!(labels);
        }
        Self::read(
            "fetch_agents_request",
            "fetch_agents_response",
            request_id,
            serde_json::json!({ "filter": filter, "page": page }),
        )
    }

    /// `fetch_agent_request` — the authoritative readback by exact id.
    #[must_use]
    pub fn agent_fetch(request_id: String, agent_id: &str) -> Self {
        Self::read(
            "fetch_agent_request",
            "fetch_agent_response",
            request_id,
            serde_json::json!({ "agentId": agent_id }),
        )
    }

    /// `fetch_agent_timeline_request` under one projection and direction.
    ///
    /// The projection is a parameter rather than a constant so the recorded
    /// suite can prove what `projected` costs; every production call site passes
    /// [`PaseoProjection::Canonical`].
    #[must_use]
    pub fn timeline_fetch(
        request_id: String,
        agent_id: &str,
        projection: PaseoProjection,
        direction: PaseoDirection,
        cursor: Option<&PaseoTimelineCursor>,
        limit: u32,
    ) -> Self {
        let mut message = serde_json::json!({
            "agentId": agent_id,
            "projection": projection.as_str(),
            "direction": direction.as_str(),
            "limit": limit,
        });
        if let Some(cursor) = cursor {
            message["cursor"] = serde_json::json!({
                "epoch": cursor.epoch,
                "seq": cursor.seq,
            });
        }
        Self::read(
            "fetch_agent_timeline_request",
            "fetch_agent_timeline_response",
            request_id,
            message,
        )
    }

    /// `agent.timeline.set_subscription.request` — narrow the live stream.
    ///
    /// The whole subscribed set travels on every call, because that is what the
    /// request means: it *replaces* this connection's set rather than adding to
    /// it.
    #[must_use]
    pub fn timeline_subscribe(request_id: String, agent_ids: &[String]) -> Self {
        Self::read(
            "agent.timeline.set_subscription.request",
            "agent.timeline.set_subscription.response",
            request_id,
            serde_json::json!({ "agentIds": agent_ids }),
        )
    }

    /// `send_agent_message_request` with the caller's own message id.
    #[must_use]
    pub fn send_message(request_id: String, agent_id: &str, message_id: &str, body: &str) -> Self {
        Self::mutate(
            "send_agent_message_request",
            "send_agent_message_response",
            request_id,
            serde_json::json!({
                "agentId": agent_id,
                "text": body,
                "messageId": message_id,
            }),
        )
    }

    /// `agent_permission_response`, bound to the exact pending request.
    ///
    /// The correlation id *is* the permission request id, because that is the
    /// only id both halves of this exchange carry:
    /// `agent_permission_resolved` reports `payload.requestId`, and it is the
    /// permission's.
    #[must_use]
    pub fn permission_response(agent_id: &str, permission_id: &str, allow: bool) -> Self {
        let response = if allow {
            serde_json::json!({ "behavior": "allow" })
        } else {
            serde_json::json!({ "behavior": "deny" })
        };
        Self::mutate(
            "agent_permission_response",
            "agent_permission_resolved",
            permission_id.to_owned(),
            serde_json::json!({ "agentId": agent_id, "response": response }),
        )
    }

    /// The ledger key: the request type only, never the parameters.
    #[must_use]
    pub fn route(&self) -> String {
        format!("rpc {}", self.request_type)
    }

    /// The complete outbound frame, envelope and all.
    #[must_use]
    pub fn envelope(&self) -> serde_json::Value {
        serde_json::json!({ "type": "session", "message": self.message })
    }

    fn read(
        request_type: &'static str,
        response_type: &'static str,
        request_id: String,
        params: serde_json::Value,
    ) -> Self {
        Self::build(request_type, response_type, request_id, params, false)
    }

    fn mutate(
        request_type: &'static str,
        response_type: &'static str,
        request_id: String,
        params: serde_json::Value,
    ) -> Self {
        Self::build(request_type, response_type, request_id, params, true)
    }

    fn build(
        request_type: &'static str,
        response_type: &'static str,
        request_id: String,
        mut params: serde_json::Value,
        mutates: bool,
    ) -> Self {
        // Type and correlation id are written last and by this constructor
        // only, so no call site can build a message whose declared type and
        // expected answer disagree.
        params["type"] = serde_json::json!(request_type);
        params["requestId"] = serde_json::json!(request_id);
        Self {
            message: params,
            request_type,
            response_type,
            request_id,
            mutates,
        }
    }
}

/// One daemon answer, still correlated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaseoFrame {
    /// The outbound message type this frame arrived as.
    pub response_type: String,
    /// The correlation id its payload carried.
    pub request_id: String,
    /// The payload, when the daemon answered.
    pub payload: Option<serde_json::Value>,
    /// The daemon's own error code, when it refused.
    pub error_code: Option<String>,
}

impl PaseoFrame {
    /// A successful answer of `response_type`.
    #[must_use]
    pub fn ok(
        response_type: impl Into<String>,
        request_id: String,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            response_type: response_type.into(),
            request_id,
            payload: Some(payload),
            error_code: None,
        }
    }

    /// A refusal.
    #[must_use]
    pub fn failed(request_id: String, error_code: String) -> Self {
        Self {
            response_type: "rpc_error".to_owned(),
            request_id,
            payload: None,
            error_code: Some(error_code),
        }
    }

    /// Resolve this frame against the request it must answer.
    ///
    /// Both halves are checked. On one multiplexed socket an answer taken by
    /// arrival order is an answer about whatever the daemon happened to finish
    /// first, which for a workspace readback is another project's workspace —
    /// accepted, bound, and then edited in. And an answer taken by id alone
    /// accepts an `rpc_error` frame, or a `fetch_agent_response` where a
    /// `fetch_workspaces_response` was asked for, as though the daemon had
    /// answered the question.
    ///
    /// # Errors
    /// * [`RuntimeError::Transport`] — the frame answers another request, is
    ///   another kind of answer, or the daemon refused.
    /// * [`RuntimeError::Domain`] — the payload is not the pinned 0.3.1 shape.
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
        if self.response_type != request.response_type {
            return Err(RuntimeError::Transport {
                rule: "answer was not the response type this request declared",
            });
        }
        let Some(payload) = &self.payload else {
            // The daemon's own message can quote a path or a prompt, so only the
            // fact of refusal survives into the error.
            return Err(RuntimeError::Transport {
                rule: "runtime refused the request",
            });
        };
        ensure_frame_bounded(payload)?;
        serde_json::from_value(payload.clone()).map_err(|_| {
            RuntimeError::Domain(DomainError::invalid(
                subject,
                "is not the Paseo 0.3.1 frame this adapter is pinned to",
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
    /// The identity the daemon pushed when this connection was established.
    ///
    /// A push rather than a request, so the adapter asks the transport for the
    /// copy it gated on instead of inventing a `server_info` request 0.3.1 does
    /// not have.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when no gated connection could be
    /// established.
    async fn server_identity(&self) -> RuntimeResult<PaseoServerInfo>;

    /// Run one CLI command.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Transport`] when the channel failed. That is a
    /// fact about the channel and never about the work: an implementation must
    /// not turn a timeout into an empty success.
    async fn run(&self, command: &PaseoCommand) -> RuntimeResult<PaseoOutput>;

    /// Make one session-protocol request.
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

// ---------------------------------------------------------------------------
// The live socket
// ---------------------------------------------------------------------------

/// Everything one live connection owns, shared with its reader task.
#[derive(Debug, Default)]
struct Multiplex {
    /// Answers still owed, by correlation id.
    pending: std::sync::Mutex<HashMap<String, oneshot::Sender<PaseoFrame>>>,
    /// Unsolicited frames, by agent.
    streams: std::sync::Mutex<BTreeMap<String, VecDeque<serde_json::Value>>>,
}

impl Multiplex {
    /// Route one decoded outbound frame, or drop it.
    ///
    /// Three outcomes and no fourth: it answers a pending request, it is an
    /// `agent_stream` for some agent, or it is neither and nothing here is
    /// interested. A frame that is "close enough" to an answer is dropped, not
    /// delivered — the wrong-type check lives in [`PaseoFrame::resolve`] and it
    /// can only work if this side never guesses.
    fn route(&self, message: &serde_json::Value) {
        let Some(response_type) = message.get("type").and_then(serde_json::Value::as_str) else {
            return;
        };
        let payload = message.get("payload");
        if response_type == "agent_stream" {
            let agent_id = payload
                .and_then(|payload| payload.get("agentId"))
                .and_then(serde_json::Value::as_str);
            if let Some(agent_id) = agent_id {
                let mut streams = self.streams.lock().expect("the transport lock is intact");
                let queue = streams.entry(agent_id.to_owned()).or_default();
                if queue.len() >= MAX_STREAM_QUEUE {
                    queue.pop_front();
                }
                queue.push_back(message.clone());
            }
            return;
        }
        let Some(request_id) = payload
            .and_then(|payload| payload.get("requestId"))
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        let waiting = self
            .pending
            .lock()
            .expect("the transport lock is intact")
            .remove(request_id);
        if let Some(waiting) = waiting {
            let frame = if response_type == "rpc_error" {
                PaseoFrame::failed(request_id.to_owned(), "rpc_error".to_owned())
            } else {
                PaseoFrame::ok(
                    response_type,
                    request_id.to_owned(),
                    payload.cloned().unwrap_or(serde_json::Value::Null),
                )
            };
            // A receiver that has already given up is not an error here: the
            // request timed out, and its slot is gone.
            let _ = waiting.send(frame);
        }
    }
}

type LiveSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// One gated connection: the write half, the shared multiplex, and the identity
/// the daemon pushed.
struct LiveConnection {
    writer: futures::stream::SplitSink<LiveSocket, Message>,
    multiplex: Arc<Multiplex>,
    identity: PaseoServerInfo,
    reader: tokio::task::JoinHandle<()>,
}

impl fmt::Debug for LiveConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveConnection")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl Drop for LiveConnection {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

/// The live transport: a real Paseo executable, and the real 0.3.1 session
/// socket.
///
/// # What runs
///
/// [`PaseoTransport::run`] dispatches the CLI with an argv array, one deadline,
/// one output bound, and `--host` appended from a [`SecretString`] this type
/// owns. No shell is involved at any point.
///
/// [`PaseoTransport::request`] and [`PaseoTransport::drain_stream`] speak the
/// WebSocket session protocol described in the module docs. The connection is
/// established lazily and gated on the pushed `status/server_info`: a daemon
/// that never pushes one, or pushes one off the pins, is refused before any
/// operational frame is written.
#[derive(Debug)]
pub struct PaseoLiveTransport {
    executable: String,
    host: SecretString,
    endpoint: String,
    client_id: String,
    timeout_seconds: u64,
    connection: Mutex<Option<LiveConnection>>,
}

/// The default loopback endpoint Kontor 1.0 qualifies.
pub const PASEO_DEFAULT_ENDPOINT: &str = "ws://127.0.0.1:6767/ws";

impl PaseoLiveTransport {
    /// Build a transport that dispatches `executable` against `host_target` and
    /// speaks the session protocol at `endpoint`.
    ///
    /// `host_target` is the complete `--host` argument, password and all. It is
    /// taken as a [`SecretString`] so the caller cannot have been holding it in
    /// an ordinary `String` that a `Debug` derive would print.
    ///
    /// `client_id` must be stable for this Kontor plane: the daemon keys a
    /// resumable session on it, so a fresh id per connection leaks one session
    /// per reconnect.
    ///
    /// # Errors
    /// Returns [`RuntimeError::Domain`] for an empty executable, host target,
    /// endpoint or client id.
    pub fn new(
        executable: &str,
        host_target: SecretString,
        endpoint: &str,
        client_id: &str,
        timeout_seconds: u64,
    ) -> RuntimeResult<Self> {
        for (subject, value) in [
            ("PaseoExecutable", executable),
            ("PaseoEndpoint", endpoint),
            ("PaseoClientId", client_id),
        ] {
            if value.is_empty() {
                return Err(RuntimeError::Domain(DomainError::invalid(
                    subject,
                    "must not be empty",
                )));
            }
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
            endpoint: endpoint.to_owned(),
            client_id: client_id.to_owned(),
            timeout_seconds,
            connection: Mutex::new(None),
        })
    }

    /// The hello this transport opens every connection with.
    ///
    /// Protocol and app version are separate fields, because they are separate
    /// pins: the daemon closes the socket on a protocol number it does not
    /// implement, and the app version is what this adapter's own gate reads.
    #[must_use]
    pub fn hello(client_id: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "hello",
            "clientId": client_id,
            "clientType": PASEO_CLIENT_TYPE,
            "protocolVersion": PASEO_WS_PROTOCOL_VERSION,
            "appVersion": PASEO_APP_VERSION,
            "capabilities": { PASEO_CAP_SELECTIVE_AGENT_TIMELINE: true },
        })
    }

    /// The pushed server identity, connecting first if necessary.
    async fn gated(&self) -> RuntimeResult<PaseoServerInfo> {
        let mut held = self.connection.lock().await;
        if held.is_none() {
            *held = Some(self.connect().await?);
        }
        let connection = held.as_ref().expect("a connection was just established");
        if connection.reader.is_finished() {
            // The reader ended, so the socket is gone and every pending
            // correlation with it. Reconnecting here rather than writing into a
            // dead socket is what keeps a stale answer from a previous daemon
            // out of a fresh request.
            *held = Some(self.connect().await?);
        }
        Ok(held
            .as_ref()
            .expect("a connection is held")
            .identity
            .clone())
    }

    /// Establish one gated connection: connect, hello, wait for the push.
    async fn connect(&self) -> RuntimeResult<LiveConnection> {
        let deadline = Duration::from_secs(self.timeout_seconds);
        let (socket, _) = tokio::time::timeout(
            deadline,
            tokio_tungstenite::connect_async(self.endpoint.as_str()),
        )
        .await
        .map_err(|_| RuntimeError::Transport {
            rule: "runtime did not accept a connection within the deadline",
        })?
        .map_err(|_| RuntimeError::Transport {
            rule: "the daemon protocol socket could not be opened",
        })?;

        let (mut writer, mut readable) = socket.split();
        writer
            .send(Message::Text(
                Self::hello(&self.client_id).to_string().into(),
            ))
            .await
            .map_err(|_| RuntimeError::Transport {
                rule: "channel failed before the runtime answered",
            })?;

        // Read until the daemon volunteers its identity. Nothing else may be
        // written before this arrives, so the loop is here rather than in the
        // reader task: an operational frame written against an ungated
        // connection is exactly the ordering the gate exists to prevent.
        let identity = tokio::time::timeout(deadline, Self::await_identity(&mut readable))
            .await
            .map_err(|_| RuntimeError::Transport {
                rule: "runtime did not announce itself within the deadline",
            })??;

        let multiplex = Arc::new(Multiplex::default());
        let routed = Arc::clone(&multiplex);
        let reader = tokio::spawn(async move {
            while let Some(Ok(message)) = readable.next().await {
                if let Some(decoded) = decode_session_frame(&message) {
                    routed.route(&decoded);
                }
            }
        });
        Ok(LiveConnection {
            writer,
            multiplex,
            identity,
            reader,
        })
    }

    /// Read frames until the pushed `status/server_info` arrives.
    async fn await_identity(
        readable: &mut futures::stream::SplitStream<LiveSocket>,
    ) -> RuntimeResult<PaseoServerInfo> {
        while let Some(message) = readable.next().await {
            let message = message.map_err(|_| RuntimeError::Transport {
                rule: "channel failed before the runtime announced itself",
            })?;
            let Some(decoded) = decode_session_frame(&message) else {
                continue;
            };
            if decoded.get("type").and_then(serde_json::Value::as_str) != Some("status") {
                continue;
            }
            let Some(payload) = decoded.get("payload") else {
                continue;
            };
            if payload.get("status").and_then(serde_json::Value::as_str) != Some("server_info") {
                continue;
            }
            return serde_json::from_value(payload.clone()).map_err(|_| {
                RuntimeError::Domain(DomainError::invalid(
                    "PaseoServerInfo",
                    "is not the Paseo 0.3.1 frame this adapter is pinned to",
                ))
            });
        }
        // A daemon that closed the socket instead of announcing itself is the
        // shape a protocol-version rejection takes: 0.3.1 closes with
        // `Incompatible protocol version` and says nothing else.
        Err(RuntimeError::Transport {
            rule: "runtime closed the connection without announcing itself",
        })
    }
}

/// Decode one WebSocket message into the session message it carries.
///
/// Binary frames, oversized frames, malformed JSON and unknown outer envelopes
/// all decode to `None` — they are not answers and they are not content, so the
/// only safe thing to do with them is nothing.
fn decode_session_frame(message: &Message) -> Option<serde_json::Value> {
    let Message::Text(text) = message else {
        return None;
    };
    if text.len() > MAX_FRAME_BYTES {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    match parsed.get("type").and_then(serde_json::Value::as_str)? {
        "session" => parsed.get("message").cloned(),
        // `pong` is the only other outer envelope 0.3.1 sends, and nothing here
        // asks for one.
        _ => None,
    }
}

#[async_trait]
impl PaseoTransport for PaseoLiveTransport {
    async fn server_identity(&self) -> RuntimeResult<PaseoServerInfo> {
        self.gated().await
    }

    async fn run(&self, command: &PaseoCommand) -> RuntimeResult<PaseoOutput> {
        command.ensure_dispatchable()?;
        let mut process = tokio::process::Command::new(&self.executable);
        process.args(command.argv());
        // Resolved here and nowhere else, immediately before dispatch.
        process.arg("--host").arg(self.host.expose_secret());
        // …and only then the trailing positional, behind `--`. Everything after
        // that terminator is a positional, so any flag written after it — this
        // `--host` included — would arrive as prompt text.
        if let Some(trailing) = command.trailing() {
            process.arg("--").arg(trailing);
        }
        for (name, value) in command.env() {
            process.env(name, value);
        }
        process.stdin(std::process::Stdio::null());
        let output =
            tokio::time::timeout(Duration::from_secs(self.timeout_seconds), process.output())
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

    async fn request(&self, request: &PaseoRpc) -> RuntimeResult<PaseoFrame> {
        // The gate first, always. `gated` establishes the connection if there
        // is none and re-establishes it if the reader died, so no frame is ever
        // written before a pushed identity has been read on *this* socket.
        self.gated().await?;
        let (answered, waiting) = oneshot::channel();
        let deadline = Duration::from_secs(self.timeout_seconds);
        {
            let mut held = self.connection.lock().await;
            let connection = held.as_mut().ok_or(RuntimeError::Transport {
                rule: "the daemon protocol socket is not connected",
            })?;
            connection
                .multiplex
                .pending
                .lock()
                .expect("the transport lock is intact")
                .insert(request.request_id.clone(), answered);
            let sent = connection
                .writer
                .send(Message::Text(request.envelope().to_string().into()))
                .await;
            if sent.is_err() {
                connection
                    .multiplex
                    .pending
                    .lock()
                    .expect("the transport lock is intact")
                    .remove(&request.request_id);
                return Err(RuntimeError::Transport {
                    rule: "channel failed before the runtime answered",
                });
            }
        }
        match tokio::time::timeout(deadline, waiting).await {
            Ok(Ok(frame)) => Ok(frame),
            // The sender was dropped, which means the reader task ended: the
            // socket died with this request in flight.
            Ok(Err(_)) => Err(RuntimeError::Transport {
                rule: "channel failed before the runtime answered",
            }),
            Err(_) => {
                let held = self.connection.lock().await;
                if let Some(connection) = held.as_ref() {
                    connection
                        .multiplex
                        .pending
                        .lock()
                        .expect("the transport lock is intact")
                        .remove(&request.request_id);
                }
                Err(RuntimeError::Transport {
                    rule: "runtime did not answer within the request deadline",
                })
            }
        }
    }

    async fn drain_stream(&self, agent_id: &str) -> RuntimeResult<Vec<serde_json::Value>> {
        self.gated().await?;
        let held = self.connection.lock().await;
        let connection = held.as_ref().ok_or(RuntimeError::Transport {
            rule: "the daemon protocol socket is not connected",
        })?;
        let mut streams = connection
            .multiplex
            .streams
            .lock()
            .expect("the transport lock is intact");
        Ok(streams
            .get_mut(agent_id)
            .map(|frames| frames.drain(..).collect())
            .unwrap_or_default())
    }
}

/// Refuse a raw daemon frame larger than the accepted bound.
///
/// Enforced at every point a frame is accepted — [`PaseoFrame::resolve`] for the
/// request/response half and the subscription drain for the pushed half — and
/// *before* the frame is deserialized into anything, because a bound checked
/// after parsing has already paid the cost it exists to refuse. The live reader
/// enforces the same bound on the raw text, where the bytes actually arrive.
///
/// # Errors
/// Returns [`RuntimeError::Transport`] for a frame over [`MAX_FRAME_BYTES`].
pub fn ensure_frame_bounded(raw: &serde_json::Value) -> RuntimeResult<()> {
    // ponytail: re-serializing to measure is one pass over a frame that is
    // about to be parsed anyway; the streaming byte count lives in
    // `decode_session_frame`, which is where the bytes actually arrive.
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
    use kontor_core::spec::{EffortLevel, ModelRef, ProviderRef};

    fn labels() -> BTreeMap<String, String> {
        [("kontor.role".to_owned(), "implement".to_owned())]
            .into_iter()
            .collect()
    }

    fn route(provider: &str, model: &str, effort: Option<EffortLevel>) -> ModelRung {
        ModelRung {
            provider: ProviderRef(provider.to_owned()),
            model: ModelRef(model.to_owned()),
            effort,
        }
    }

    #[test]
    fn every_lifecycle_command_is_json_and_carries_no_host() {
        let commands = [
            PaseoCommand::version(),
            PaseoCommand::workspace_create("/w/task-1", "prj_1", "TSW · ASMA-7755 · KON-11"),
            PaseoCommand::workspace_archive("wks_1"),
            PaseoCommand::agent_run(
                "wks_1",
                "/w/task-1",
                &route("codex", "gpt-5.6-sol", Some(EffortLevel::Xhigh)),
                SeatAutonomy::Supervised,
                "KON-MVP-11 Implement",
                &labels(),
                "agt_orchestrator",
                "do the work",
            )
            .expect("Codex has a pinned permission mode"),
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
    fn a_prompt_is_the_trailing_positional_argument() {
        // 0.3.1 spells it `paseo agent run [options] <prompt>`. Sending
        // `--prompt` would be an unknown flag, and a prompt that starts with a
        // dash is why `--` precedes it.
        let command = PaseoCommand::agent_run(
            "wks_1",
            "/w/task-1",
            &route("codex", "gpt-5.6-sol", None),
            SeatAutonomy::Supervised,
            "t",
            &labels(),
            "agt_p",
            "--not-a-flag",
        )
        .expect("Codex has a pinned permission mode");
        // The option half never carries it, because the transport writes its
        // own `--host` after these and everything past `--` is a positional.
        assert_eq!(command.trailing(), Some("--not-a-flag"));
        assert!(!command.argv().iter().any(|arg| arg == "--not-a-flag"));
        assert!(!command.argv().iter().any(|arg| arg == "--"));
        let dispatched = command.dispatched_argv();
        assert_eq!(dispatched[dispatched.len() - 2], "--");
        assert_eq!(dispatched[dispatched.len() - 1], "--not-a-flag");
        assert!(!dispatched.iter().any(|arg| arg == "--prompt"));
        command
            .ensure_dispatchable()
            .expect("a positional prompt after `--` is not another option");
    }

    #[test]
    fn an_agent_launch_carries_the_selected_route() {
        let command = PaseoCommand::agent_run(
            "wks_1",
            "/w/task-1",
            &route("claude", "claude-opus-5", Some(EffortLevel::Xhigh)),
            SeatAutonomy::Supervised,
            "t",
            &labels(),
            "agt_p",
            "p",
        )
        .expect("Claude has a pinned permission mode");
        let argv = command.argv();
        assert!(argv.windows(2).any(|pair| pair == ["--provider", "claude"]));
        assert!(
            argv.windows(2)
                .any(|pair| pair == ["--model", "claude-opus-5"])
        );
        assert!(argv.windows(2).any(|pair| pair == ["--thinking", "xhigh"]));
        assert!(argv.windows(2).any(|pair| pair == ["--mode", "auto"]));
    }

    /// Paseo 0.4.0 rejects the provider-neutral `default` spelling for Codex.
    /// A standard delivery seat must keep the explicit auto-review behavior the
    /// provider default gave it before autonomy was frozen into launch requests.
    #[test]
    fn a_codex_delivery_launch_uses_a_supported_provider_mode() {
        let command = PaseoCommand::agent_run(
            "wks_1",
            "/w/task-1",
            &route("codex", "gpt-5.6-sol", Some(EffortLevel::Xhigh)),
            SeatAutonomy::Supervised,
            "Verifier",
            &labels(),
            "agt_p",
            "verify",
        )
        .expect("Codex supervised delivery has a pinned mode");

        assert!(
            command
                .argv()
                .windows(2)
                .any(|pair| pair == ["--mode", "auto-review"])
        );
        assert!(!command.argv().iter().any(|argument| argument == "default"));
    }

    #[test]
    fn every_supported_provider_gets_an_explicit_unattended_mode() {
        let expected = [
            ("claude", Some("auto")),
            ("codex", Some("auto-review")),
            (
                "copilot",
                Some("https://agentclientprotocol.com/protocol/session-modes#agent"),
            ),
            ("opencode", Some("build")),
            ("pi", None),
            ("omp", Some("full")),
        ];
        for (provider, mode) in expected {
            assert_eq!(
                permission_mode(provider).expect("a supported provider"),
                mode
            );
        }
        assert!(matches!(
            permission_mode("new-provider"),
            Err(RuntimeError::PermissionModeUnsupported { .. })
        ));
    }

    #[test]
    fn consultation_routes_are_read_only_and_the_scoped_secret_is_not_debugged() {
        for (provider, model, expected_mode) in [
            ("claude", "claude-opus-5", "plan"),
            ("cursor", "composer-2", "plan"),
            ("codex", "gpt-5.6-sol", "auto-review"),
        ] {
            let command = PaseoCommand::consultation_run(
                "wks_1",
                "/w/epic",
                &route(provider, model, None),
                "Reviewer",
                &labels(),
                "agt_orchestrator",
                "read only",
                "seat-secret-value",
            )
            .expect("a consultation-safe provider");
            assert!(
                command
                    .argv()
                    .windows(2)
                    .any(|pair| pair == ["--mode", expected_mode])
            );
            assert!(
                command
                    .env()
                    .contains(&("KONTOR_AUTH".to_owned(), "seat-secret-value".to_owned()))
            );
            assert!(!format!("{command:?}").contains("seat-secret-value"));
        }
        assert!(matches!(
            consultation_permission_mode("opencode"),
            Err(RuntimeError::PermissionModeUnsupported { .. })
        ));
    }

    /// Every launch states the authority it runs under, and the three intents
    /// reach Paseo as three different modes.
    ///
    /// This is the whole of the "hundreds of permission prompts" fix. Before it,
    /// `agent run` carried no `--mode` at all, so every seat inherited whatever
    /// the provider defaulted to and asked the operator about every guarded tool
    /// call — a question Kontor had already answered by arming the work. The
    /// mutant this kills is the tempting one: omitting the flag for `supervised`
    /// because "that is the default anyway". It is only the default until it
    /// isn't, and a seat's authority may not depend on a provider's release
    /// notes.
    #[test]
    fn every_launch_declares_the_authority_it_runs_under() {
        let mode_of = |autonomy| {
            let command = PaseoCommand::agent_run(
                "wks_1",
                "/w/task-1",
                &route("claude", "claude-opus-5", None),
                autonomy,
                "t",
                &labels(),
                "agt_p",
                "p",
            )
            .expect("the autonomy maps to a Paseo mode");
            let argv = command.argv().to_vec();
            let index = argv
                .iter()
                .position(|arg| arg == "--mode")
                .unwrap_or_else(|| panic!("{autonomy} declared no mode: {argv:?}"));
            argv[index + 1].clone()
        };

        assert_eq!(mode_of(SeatAutonomy::Supervised), "auto");
        assert_eq!(mode_of(SeatAutonomy::Bounded), "bypassPermissions");
        assert_eq!(mode_of(SeatAutonomy::Advisory), "plan");

        // Three intents, three spellings: a mapping that collapsed two of them
        // would silently give one seat another's authority.
        let modes: std::collections::BTreeSet<String> = [
            SeatAutonomy::Supervised,
            SeatAutonomy::Bounded,
            SeatAutonomy::Advisory,
        ]
        .into_iter()
        .map(mode_of)
        .collect();
        assert_eq!(modes.len(), 3, "two autonomy levels share one Paseo mode");
    }

    #[test]
    fn the_ledger_route_never_quotes_the_operators_work() {
        let command = PaseoCommand::agent_run(
            "wks_1",
            "/private/worktrees/secret-project",
            &route("codex", "gpt-5.6-sol", None),
            SeatAutonomy::Supervised,
            "KON-MVP-11 Implement",
            &labels(),
            "agt_orchestrator",
            "the actual prompt",
        )
        .expect("Codex has a pinned permission mode");
        assert_eq!(command.route(), "agent run");
        assert!(!command.route().contains("the actual prompt"));
        assert!(!command.route().contains("secret-project"));
        // …while the dispatched argv, which only the transport sees, still
        // carries them.
        assert!(
            command
                .dispatched_argv()
                .iter()
                .any(|arg| arg == "the actual prompt")
        );
    }

    #[test]
    fn a_parent_agent_travels_in_the_environment_rather_than_a_flag() {
        let command = PaseoCommand::agent_run(
            "wks_1",
            "/w/task-1",
            &route("codex", "gpt-5.6-sol", None),
            SeatAutonomy::Supervised,
            "t",
            &labels(),
            "agt_orchestrator",
            "p",
        )
        .expect("Codex has a pinned permission mode");
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
            PaseoCommand::agent_archive("--force")
                .ensure_dispatchable()
                .is_err()
        );
        assert!(
            PaseoCommand::workspace_create("--isolation", "prj_1", "t")
                .ensure_dispatchable()
                .is_err()
        );
        // …and a command whose own flags sit next to each other is fine, which
        // a whole-argv scan would have refused.
        PaseoCommand::agent_run(
            "wks_1",
            "/w/task-1",
            &route("codex", "gpt-5.6-sol", None),
            SeatAutonomy::Supervised,
            "t",
            &labels(),
            "agt_p",
            "p",
        )
        .expect("Codex has a pinned permission mode")
        .ensure_dispatchable()
        .expect("`--background --workspace wks_1` is an ordinary command");
        PaseoCommand::agent_stop("agt_1")
            .ensure_dispatchable()
            .expect("an ordinary id dispatches");
    }

    #[test]
    fn only_writes_are_counted_as_mutations() {
        assert!(!PaseoCommand::version().mutates());
        assert!(
            PaseoCommand::agent_run(
                "w",
                "/w/t",
                &route("codex", "gpt-5.6-sol", None),
                SeatAutonomy::Supervised,
                "t",
                &labels(),
                "agt_p",
                "p",
            )
            .expect("Codex has a pinned permission mode")
            .mutates()
        );
        assert!(PaseoCommand::workspace_create("/w/t", "p", "t").mutates());
        assert!(PaseoCommand::agent_update_labels("agt_1", &labels()).mutates());
        assert!(!PaseoRpc::project_list("req-1".to_owned()).mutates);
        assert!(!PaseoRpc::daemon_status("req-1".to_owned()).mutates);
        assert!(PaseoRpc::project_add("req-1".to_owned(), "/w").mutates);
        assert!(PaseoRpc::send_message("req-1".to_owned(), "agt_1", "msg", "body").mutates);
    }

    #[test]
    fn every_request_carries_its_type_and_correlation_id_in_the_session_envelope() {
        let request = PaseoRpc::agent_fetch("req-1".to_owned(), "agt_1");
        let envelope = request.envelope();
        assert_eq!(envelope["type"], "session");
        assert_eq!(envelope["message"]["type"], "fetch_agent_request");
        assert_eq!(envelope["message"]["requestId"], "req-1");
        assert_eq!(envelope["message"]["agentId"], "agt_1");
        assert_eq!(request.response_type, "fetch_agent_response");
        assert_eq!(request.route(), "rpc fetch_agent_request");
    }

    #[test]
    fn the_hello_pins_protocol_and_app_version_as_separate_fields() {
        let hello = PaseoLiveTransport::hello("kontor-plane-1");
        assert_eq!(hello["type"], "hello");
        assert_eq!(hello["clientId"], "kontor-plane-1");
        assert_eq!(hello["clientType"], "cli");
        assert_eq!(hello["protocolVersion"], 1);
        assert_eq!(hello["appVersion"], PASEO_APP_VERSION);
        // The daemon's capability table spells this one snake_case; the
        // camelCase spelling is silently ignored, which is worse than an error.
        assert_eq!(hello["capabilities"]["selective_agent_timeline"], true);
    }

    #[test]
    fn an_answer_for_another_request_or_of_another_type_is_refused_rather_than_read() {
        let request = PaseoRpc::agent_fetch("req-1".to_owned(), "agt_1");
        let mine = PaseoFrame::ok(
            "fetch_agent_response",
            "req-1".to_owned(),
            serde_json::json!({ "agent": null }),
        );
        mine.resolve::<serde_json::Value>(&request, "PaseoAgentAnswer")
            .expect("my own answer resolves");

        let theirs = PaseoFrame::ok(
            "fetch_agent_response",
            "req-2".to_owned(),
            serde_json::json!({ "agent": null }),
        );
        assert_eq!(
            theirs
                .resolve::<serde_json::Value>(&request, "PaseoAgentAnswer")
                .expect_err("another request's answer is not mine"),
            RuntimeError::Transport {
                rule: "answer carried another request's correlation id"
            }
        );

        // Same id, wrong kind of answer. This is the one an id-only check lets
        // through, and it is the one that decides a placement rule from a
        // frame about something else.
        let wrong_kind = PaseoFrame::ok(
            "fetch_workspaces_response",
            "req-1".to_owned(),
            serde_json::json!({ "entries": [] }),
        );
        assert_eq!(
            wrong_kind
                .resolve::<serde_json::Value>(&request, "PaseoAgentAnswer")
                .expect_err("a workspace page does not answer an agent fetch"),
            RuntimeError::Transport {
                rule: "answer was not the response type this request declared"
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
            .resolve::<serde_json::Value>(&request, "PaseoAgentAnswer")
            .expect_err("a refusal is not an answer");
        assert!(!format!("{error:?}").contains("secret-project"));
    }

    #[test]
    fn a_non_zero_exit_is_a_channel_fact_and_reads_no_output() {
        let refused = PaseoOutput::new(1, "{\"agentId\":\"agt_1\"}".to_owned());
        assert_eq!(
            refused
                .parse::<serde_json::Value>("PaseoCliAgentStarted")
                .expect_err("a non-zero exit is not an answer"),
            RuntimeError::Transport {
                rule: "runtime refused the command"
            }
        );
        assert!(refused.version().is_err());
        assert_eq!(
            PaseoOutput::new(0, "0.3.1\n".to_owned())
                .version()
                .expect("a bare version string"),
            "0.3.1"
        );
    }

    #[test]
    fn a_live_transport_needs_an_executable_a_host_an_endpoint_and_a_client_id() {
        let host = || SecretString::from("127.0.0.1:6767".to_owned());
        assert!(
            PaseoLiveTransport::new("paseo", host(), PASEO_DEFAULT_ENDPOINT, "kon-1", 30).is_ok()
        );
        assert!(PaseoLiveTransport::new("", host(), PASEO_DEFAULT_ENDPOINT, "kon-1", 30).is_err());
        assert!(PaseoLiveTransport::new("paseo", host(), "", "kon-1", 30).is_err());
        assert!(PaseoLiveTransport::new("paseo", host(), PASEO_DEFAULT_ENDPOINT, "", 30).is_err());
        assert!(
            PaseoLiveTransport::new(
                "paseo",
                SecretString::from(String::new()),
                PASEO_DEFAULT_ENDPOINT,
                "kon-1",
                30
            )
            .is_err()
        );
    }

    #[test]
    fn a_live_transport_never_prints_its_host_target() {
        let transport = PaseoLiveTransport::new(
            "paseo",
            SecretString::from("tcp://u:p@host?password=secret".to_owned()),
            PASEO_DEFAULT_ENDPOINT,
            "kon-1",
            30,
        )
        .expect("a valid transport");
        let printed = format!("{transport:?}");
        assert!(!printed.contains("password=secret"), "got {printed}");
        assert!(!printed.contains("u:p@host"), "got {printed}");
    }

    #[test]
    fn the_reader_routes_answers_by_id_and_streams_by_agent() {
        let multiplex = Multiplex::default();
        let (sender, mut receiver) = oneshot::channel();
        multiplex
            .pending
            .lock()
            .expect("lock")
            .insert("req-1".to_owned(), sender);

        // An unsolicited frame that arrives *before* the answer must not be
        // handed to the request that is waiting.
        multiplex.route(&serde_json::json!({
            "type": "agent_stream",
            "payload": { "agentId": "agt_1", "event": { "type": "timeline" }, "seq": 2 },
        }));
        assert!(
            receiver.try_recv().is_err(),
            "a pushed frame is not an answer"
        );

        multiplex.route(&serde_json::json!({
            "type": "fetch_agent_response",
            "payload": { "requestId": "req-1", "agent": null },
        }));
        let frame = receiver.try_recv().expect("the answer arrives");
        assert_eq!(frame.response_type, "fetch_agent_response");
        assert_eq!(frame.request_id, "req-1");

        // …and agent B's frame never lands in agent A's queue.
        multiplex.route(&serde_json::json!({
            "type": "agent_stream",
            "payload": { "agentId": "agt_2", "event": { "type": "timeline" }, "seq": 9 },
        }));
        let streams = multiplex.streams.lock().expect("lock");
        assert_eq!(streams.get("agt_1").map(VecDeque::len), Some(1));
        assert_eq!(streams.get("agt_2").map(VecDeque::len), Some(1));
    }

    #[test]
    fn the_stream_queue_is_bounded() {
        let multiplex = Multiplex::default();
        for seq in 0..(MAX_STREAM_QUEUE + 10) {
            multiplex.route(&serde_json::json!({
                "type": "agent_stream",
                "payload": { "agentId": "agt_1", "event": { "type": "timeline" }, "seq": seq },
            }));
        }
        let streams = multiplex.streams.lock().expect("lock");
        assert_eq!(streams["agt_1"].len(), MAX_STREAM_QUEUE);
    }

    #[test]
    fn an_rpc_error_is_a_refusal_rather_than_an_answer() {
        let multiplex = Multiplex::default();
        let (sender, mut receiver) = oneshot::channel();
        multiplex
            .pending
            .lock()
            .expect("lock")
            .insert("req-1".to_owned(), sender);
        multiplex.route(&serde_json::json!({
            "type": "rpc_error",
            "payload": { "requestId": "req-1", "error": "/Users/someone/secret" },
        }));
        let frame = receiver.try_recv().expect("a refusal is delivered");
        assert!(frame.payload.is_none());
        assert!(!format!("{frame:?}").contains("secret"));
    }

    #[test]
    fn a_binary_or_malformed_or_oversized_frame_decodes_to_nothing() {
        assert!(decode_session_frame(&Message::Binary(vec![1, 2, 3].into())).is_none());
        assert!(decode_session_frame(&Message::Text("not json".into())).is_none());
        assert!(
            decode_session_frame(&Message::Text(
                serde_json::json!({ "type": "pong" }).to_string().into()
            ))
            .is_none()
        );
        let oversized = format!(
            "{{\"type\":\"session\",\"message\":{{\"pad\":\"{}\"}}}}",
            "x".repeat(MAX_FRAME_BYTES + 1)
        );
        assert!(decode_session_frame(&Message::Text(oversized.into())).is_none());
        assert!(
            decode_session_frame(&Message::Text(
                serde_json::json!({ "type": "session", "message": { "type": "status" } })
                    .to_string()
                    .into()
            ))
            .is_some()
        );
    }
}

#[cfg(test)]
mod stdout_tests {
    use super::*;

    #[test]
    fn a_leading_operator_notice_does_not_stop_the_json_being_read() {
        // 0.3.1 prints `Using workspace wks_…` on stdout before a launch's JSON,
        // so a whole-stream parse fails against the very build this adapter is
        // pinned to.
        let noisy = PaseoOutput::new(
            0,
            "Using workspace wks_1\n{\"agentId\":\"agt_1\",\"status\":\"created\"}\n".to_owned(),
        );
        let started: crate::wire::PaseoCliAgentStarted = noisy
            .parse("PaseoCliAgentStarted")
            .expect("the document is read");
        assert_eq!(started.agent_id, "agt_1");

        // …and a body that is not JSON from the first brace on is still a
        // refusal, so the tolerance is for the notice and nothing else.
        let broken = PaseoOutput::new(0, "Using workspace wks_1\n{not json".to_owned());
        assert!(
            broken
                .parse::<crate::wire::PaseoCliAgentStarted>("x")
                .is_err()
        );
        let empty = PaseoOutput::new(0, "Using workspace wks_1\n".to_owned());
        assert!(
            empty
                .parse::<crate::wire::PaseoCliAgentStarted>("x")
                .is_err()
        );
    }
}
