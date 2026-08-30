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

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use kontor_core::DomainError;
use kontor_core::id::ExternalId;
use kontor_core::spec::{ModelRung, SeatAutonomy};
use kontor_runtime::adapter::{ConsultationRouteProvenance, RuntimeError, RuntimeResult};
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

/// The environment variable Paseo reads the caller agent from.
///
/// Kontor launches top-level into an explicitly attested workspace and never
/// sets this variable. It remains here only for the optional command-level
/// contract used by callers that deliberately request native parentage.
const PARENT_AGENT_ENV: &str = "PASEO_AGENT_ID";

/// The largest one seat environment value may be.
///
/// The rendered configuration is a few kilobytes; this is headroom, not a
/// target. An unbounded value would reach the process table and the daemon's
/// argv, where nothing else in this module puts unbounded input.
const MAX_SEAT_ENVIRONMENT_VALUE: usize = 64 * 1024;

/// Refuse anything that is not exactly the closed seat-environment set.
///
/// The values are Kontor's own, so this is defence in depth rather than input
/// validation — which is the point: it is what keeps the set closed when a later
/// caller has a reason to pass one more thing.
///
/// The credential check is **marker-based and narrow by construction**. It
/// catches the shapes a secret is usually written in; it is not a proof that a
/// value carries none, and is not relied on as one. The real guarantee is that
/// only [`crate::posture::seat_environment`] builds these values.
fn validate_seat_environment(entries: &[(&'static str, String)]) -> RuntimeResult<()> {
    fn looks_like_a_credential(value: &str) -> bool {
        const MARKERS: &[&str] = &[
            "-----begin",
            "api_key",
            "apikey",
            "authorization:",
            "bearer ",
            "password",
            "secret",
        ];
        let lowered = value.to_ascii_lowercase();
        MARKERS.iter().any(|marker| lowered.contains(marker))
    }

    if entries.len() != crate::posture::SEAT_ENVIRONMENT_KEYS.len() {
        return Err(RuntimeError::LaunchNotAdmitted {
            rule: "a seat environment carries the whole closed set or the launch is refused",
        });
    }
    let mut seen = BTreeSet::new();
    for (key, value) in entries {
        if !crate::posture::SEAT_ENVIRONMENT_KEYS.contains(key) {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "a seat environment may only carry Kontor's own closed variable set",
            });
        }
        if !seen.insert(*key) {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "a seat environment names each variable exactly once",
            });
        }
        if key.contains('\0') || value.contains('\0') {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "a seat environment value contains no NUL",
            });
        }
        if value.is_empty() || value.len() > MAX_SEAT_ENVIRONMENT_VALUE {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "a seat environment value is present and bounded",
            });
        }
        if looks_like_a_credential(value) {
            return Err(RuntimeError::LaunchNotAdmitted {
                rule: "a seat environment carries configuration, never a credential",
            });
        }
    }
    // Length and membership together are set equality: six unique keys, each
    // drawn from the closed set. An omission is as refused as an addition —
    // dropping `OPENCODE_DISABLE_PROJECT_CONFIG` alone would silently re-admit
    // every project layer this posture exists to exclude.
    Ok(())
}

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
        SeatAutonomy::Bounded => match built_in_provider(provider) {
            "claude" => Ok(Some("bypassPermissions")),
            "codex" => Ok(Some("full-access")),
            "copilot" => Ok(Some("allow-all")),
            "cursor" => Ok(Some("agent")),
            "opencode" => Ok(Some("build")),
            "pi" => Ok(None),
            "omp" => Ok(Some("full")),
            other => Err(RuntimeError::PermissionModeUnsupported {
                provider: other.to_owned(),
            }),
        },
        SeatAutonomy::Advisory => match built_in_provider(provider) {
            // Cursor is deliberately absent: `consultation_permission_mode`
            // refuses it on measured behaviour — its ACP runtime permits shell
            // writes in `plan` and shell *and* file writes in `ask`. A mode
            // label is not a permission boundary, and delivery must not claim
            // the containment consultation already refuses to claim.
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
    match built_in_provider(provider) {
        "claude" => Ok(Some("auto")),
        "codex" => Ok(Some("auto-review")),
        "copilot" => Ok(Some(
            "https://agentclientprotocol.com/protocol/session-modes#agent",
        )),
        // Cursor has an `ask` mode and it is *not* an asking posture: the same
        // measured finding that keeps cursor out of consultation records file
        // and shell writes proceeding under it. Refused until Paseo exposes an
        // attested permission boundary for cursor, rather than mapped to a
        // label that would report a guarantee nothing enforces.
        "opencode" => Ok(Some("build")),
        "pi" => Ok(None),
        "omp" => Ok(Some("full")),
        _ => Err(RuntimeError::PermissionModeUnsupported {
            provider: provider.to_owned(),
        }),
    }
}

/// The provider-native contained mode used by ordinary consultation seats.
///
/// Cursor is deliberately absent. Mode names and provider metadata are not
/// evidence of containment: Cursor describes `plan` as read-only and `ask` as
/// lacking edits or command execution, but its ACP runtime permits shell writes
/// in both modes (and file writes in `ask`). Kontor cannot sandbox an
/// already-launched Paseo agent, so Cursor remains refused until Paseo exposes
/// and attests an enforced non-mutating execution boundary.
pub(crate) fn consultation_permission_mode(provider: &str) -> RuntimeResult<Option<&'static str>> {
    match built_in_provider(provider) {
        "claude" => Ok(Some("plan")),
        "codex" => Ok(Some("auto-review")),
        // Providers without a proven contained mode remain fail-closed here.
        other => Err(RuntimeError::PermissionModeUnsupported {
            provider: other.to_owned(),
        }),
    }
}

/// Resolve a consultation route under its immutable policy provenance.
///
/// OpenCode is not added to the ordinary provider table above. The sole
/// exception is the exact ASMA-8001 progression fallback accepted by an
/// operator in an Admin-authorized initial recovery profile. Its `plan` mode is
/// behavioral guidance, not OS-level containment; the qualified canary proved
/// shell writes remain possible. Every other OpenCode provider alias, model,
/// effort, template route and future recovery source remains refused.
pub(crate) fn consultation_route_permission_mode(
    rung: &ModelRung,
    provenance: &ConsultationRouteProvenance,
) -> RuntimeResult<Option<&'static str>> {
    if rung.provider.0 == "opencode" {
        let exact_model = rung.model.0 == "deepseek/deepseek-v4-flash";
        let exact_effort = rung.effort.is_some_and(|effort| effort.as_str() == "max");
        if exact_model && exact_effort && provenance.is_operator_accepted_initial_recovery_profile()
        {
            return Ok(Some("plan"));
        }
        return Err(RuntimeError::PermissionModeUnsupported {
            provider: rung.provider.0.clone(),
        });
    }
    consultation_permission_mode(&rung.provider.0)
}

/// The built-in Paseo provider an id resolves to.
///
/// A second account for the same provider is an ordinary `agents.providers`
/// entry that `extends` a built-in one, so `codex-work` and `codex-personal` are
/// two provider ids over one harness. The mode tables above are keyed by the
/// built-in because the mode vocabulary belongs to the harness, not to the
/// account; `--provider` still carries the full id, which is what selects the
/// account's own credential home.
///
/// The derivation is a prefix match against the built-in set, not a split on
/// the last `-`: `deepseek-harness` is an `acp` provider whose id contains a
/// hyphen and whose harness is not `deepseek`. No built-in is a prefix of
/// another, so the match is unique, and an id that resolves to no built-in is
/// returned unchanged so the callers above still fail closed on it.
///
/// A Paseo provider id matches `^[a-z][a-z0-9-]*$`, so a colon cannot appear in
/// one: the `codex:team` spelling the fleet policy uses for an account is a
/// label, never a provider id.
pub(crate) fn built_in_provider(provider: &str) -> &str {
    const BUILT_INS: [&str; 7] = [
        "claude", "codex", "copilot", "cursor", "opencode", "pi", "omp",
    ];
    for built_in in BUILT_INS {
        if provider == built_in {
            return built_in;
        }
        if let Some(account) = provider.strip_prefix(built_in)
            && account.starts_with('-')
        {
            return built_in;
        }
    }
    provider
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
    /// `--env` values are redacted, and that is the whole reason this is written
    /// out rather than derived: those arguments carry a seat's entire rendered
    /// configuration, and a derived `Debug` would put it in the first `{:?}`
    /// anybody reaches for. The flag itself stays visible — the shape of a
    /// command is useful in a diagnostic; its payload is not.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut argv = Vec::with_capacity(self.argv.len());
        let mut redact_next = false;
        for part in &self.argv {
            if std::mem::take(&mut redact_next) {
                argv.push("<redacted>".to_owned());
                continue;
            }
            if part == "--env" {
                redact_next = true;
            }
            argv.push(part.clone());
        }
        // `values` keeps foreign values apart from trusted words, and a seat's
        // environment lands there as well — so the same redaction has to cover
        // it. Missing this is how the first attempt still printed the whole
        // rendered configuration while the argv looked clean.
        let values: Vec<String> = self
            .values
            .iter()
            .map(|value| {
                if crate::posture::SEAT_ENVIRONMENT_KEYS
                    .iter()
                    .any(|key| value.starts_with(&format!("{key}=")))
                {
                    "<redacted>".to_owned()
                } else {
                    value.clone()
                }
            })
            .collect();
        formatter
            .debug_struct("PaseoCommand")
            .field("argv", &argv)
            .field("trailing", &self.trailing)
            .field("values", &values)
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
        caller_agent_id: Option<&str>,
        prompt: &str,
    ) -> RuntimeResult<Self> {
        let mode = paseo_mode(model_rung.provider.0.as_str(), autonomy)?;
        Self::agent_run_with_mode(
            workspace_id,
            canonical_cwd,
            model_rung,
            title,
            labels,
            caller_agent_id,
            prompt,
            mode,
        )
    }

    /// `agent run` with a per-agent environment.
    ///
    /// The environment of the **agent process**, emitted as repeated
    /// `--env key=value`. Deliberately not [`PaseoCommand::env`], which is the
    /// environment of the CLI invocation itself and already carries
    /// `KONTOR_CALLER_AGENT_ID`: setting a seat's posture there would configure
    /// the wrong process entirely.
    ///
    /// # Errors
    /// [`RuntimeError::LaunchNotAdmitted`] when the set is anything other than
    /// *exactly* [`SEAT_ENVIRONMENT_KEYS`](crate::posture::SEAT_ENVIRONMENT_KEYS)
    /// with one value each: an operator-supplied key, a duplicate, an embedded
    /// NUL, an oversized value, or content shaped like a credential. This is a
    /// closed internal set and the refusal is what keeps it closed.
    #[allow(clippy::too_many_arguments)]
    pub fn agent_run_with_environment(
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        autonomy: SeatAutonomy,
        title: &str,
        labels: &BTreeMap<String, String>,
        caller_agent_id: Option<&str>,
        prompt: &str,
        seat_environment: &[(&'static str, String)],
    ) -> RuntimeResult<Self> {
        validate_seat_environment(seat_environment)?;
        let mode = paseo_mode(model_rung.provider.0.as_str(), autonomy)?;
        Self::agent_run_with_mode_and_environment(
            workspace_id,
            canonical_cwd,
            model_rung,
            title,
            labels,
            caller_agent_id,
            prompt,
            mode,
            seat_environment,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn agent_run_with_mode(
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        title: &str,
        labels: &BTreeMap<String, String>,
        caller_agent_id: Option<&str>,
        prompt: &str,
        permission_mode: Option<&str>,
    ) -> RuntimeResult<Self> {
        Self::agent_run_with_mode_and_environment(
            workspace_id,
            canonical_cwd,
            model_rung,
            title,
            labels,
            caller_agent_id,
            prompt,
            permission_mode,
            &[],
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn agent_run_with_mode_and_environment(
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        title: &str,
        labels: &BTreeMap<String, String>,
        caller_agent_id: Option<&str>,
        prompt: &str,
        permission_mode: Option<&str>,
        seat_environment: &[(&'static str, String)],
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
        // Before the trailing prompt, which terminates the option list.
        for (key, value) in seat_environment {
            argv = argv.option("--env", &format!("{key}={value}"));
        }
        // Everything Paseo parses as a flag is already behind us, so the prompt
        // is positional and terminates the option list.
        argv = argv.trailing(prompt);
        let mut command = Self::mutate(argv, "agent run".to_owned());
        if let Some(caller_agent_id) = caller_agent_id {
            command
                .env
                .push((PARENT_AGENT_ENV.to_owned(), caller_agent_id.to_owned()));
        }
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

    /// The daemon's own diagnostics for one provider: which binary it resolves,
    /// at which version, on which `PATH`.
    ///
    /// Read-only, and the *authoritative* answer to "which OpenCode will this
    /// daemon launch". Resolving `opencode` on the daemon's behalf would answer a
    /// different question — the Paseo daemon runs from an application bundle with
    /// its own `PATH`, so a binary Kontor finds is not necessarily the one Paseo
    /// spawns.
    #[must_use]
    pub fn provider_diagnostic(provider: &str) -> Self {
        Self::read(
            // `--json` is appended by `build`, which is why it is not written
            // here: `paseo provider diagnostic <provider> --json`.
            Argv::new(&["provider", "diagnostic"]).value(provider),
            "provider diagnostic".to_owned(),
        )
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
#[derive(Clone, PartialEq, Eq)]
pub struct PaseoOutput {
    /// The process exit status.
    pub status: i32,
    /// Standard output, already bounded.
    pub stdout: String,
    /// Standard error, already bounded. It is never returned verbatim; only
    /// closed, validated refusals may be derived from it.
    stderr: String,
}

impl fmt::Debug for PaseoOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PaseoOutput")
            .field("status", &self.status)
            .field("stdout", &"<redacted>")
            .field("stderr", &"<redacted>")
            .finish()
    }
}

impl PaseoOutput {
    /// Build an answer.
    #[must_use]
    pub const fn new(status: i32, stdout: String) -> Self {
        Self {
            status,
            stdout,
            stderr: String::new(),
        }
    }

    /// Build an answer with a bounded stderr stream.
    #[must_use]
    pub const fn with_stderr(status: i32, stdout: String, stderr: String) -> Self {
        Self {
            status,
            stdout,
            stderr,
        }
    }

    /// The JSON of a successful invocation, deserialized.
    ///
    /// A non-zero exit is either a closed, validated native refusal or a
    /// [`RuntimeError::Transport`] naming only that fact. Paseo's stderr can
    /// quote a prompt, a path or the host URI it was given, so arbitrary text
    /// is never read into a refusal.
    ///
    /// # Errors
    /// * [`RuntimeError::CallerAgentNotFound`] — Paseo's validated missing-
    ///   caller refusal.
    /// * [`RuntimeError::Transport`] — every other non-zero exit.
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
        if let Some(caller_agent_id) =
            caller_agent_not_found(&self.stdout).or_else(|| caller_agent_not_found(&self.stderr))
        {
            return Err(RuntimeError::CallerAgentNotFound { caller_agent_id });
        }
        Err(RuntimeError::Transport {
            rule: "runtime refused the command",
        })
    }
}

/// Extract only Paseo's closed missing-caller refusal from an untrusted stream.
///
/// No other text crosses the boundary. In particular the surrounding line is
/// discarded because Paseo diagnostics can also contain its credential-bearing
/// host URI, the task path or prompt text.
fn caller_agent_not_found(stream: &str) -> Option<ExternalId> {
    const PREFIX: &str = "Caller agent ";
    const SUFFIX: &str = " not found";
    let after = stream.split_once(PREFIX)?.1;
    let candidate = after.split_once(SUFFIX)?.0.trim();
    if candidate.is_empty() || candidate.len() > 128 {
        return None;
    }
    ExternalId::parse(candidate).ok()
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
#[derive(Clone)]
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
    /// Agent-process environment held outside the ordinary message so Debug,
    /// fixtures, ledgers and checkpoints can never observe its values.
    secret_env: BTreeMap<String, SecretString>,
}

impl fmt::Debug for PaseoRpc {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PaseoRpc")
            .field("request_type", &self.request_type)
            .field("response_type", &self.response_type)
            .field("request_id", &self.request_id)
            .field("mutates", &self.mutates)
            .field(
                "secret_env_names",
                &self.secret_env.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl PartialEq for PaseoRpc {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message
            && self.request_type == other.request_type
            && self.response_type == other.response_type
            && self.request_id == other.request_id
            && self.mutates == other.mutates
            && self.secret_env.len() == other.secret_env.len()
            && self.secret_env.iter().all(|(name, value)| {
                other
                    .secret_env
                    .get(name)
                    .is_some_and(|other| value.expose_secret() == other.expose_secret())
            })
    }
}

impl Eq for PaseoRpc {}

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

    /// `create_agent_request` for one provenance-validated consultation seat.
    ///
    /// The scoped credential travels in the daemon session frame's `env`
    /// object. It is deliberately absent from both the CLI process environment
    /// and argv: the short-lived CLI does not own the agent process, while argv
    /// would expose the value to process inspection.
    #[allow(clippy::too_many_arguments)]
    pub fn consultation_agent_create(
        request_id: String,
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        route_provenance: &ConsultationRouteProvenance,
        title: &str,
        labels: &BTreeMap<String, String>,
        prompt: &str,
        credential: &str,
    ) -> RuntimeResult<Self> {
        let mode = consultation_route_permission_mode(model_rung, route_provenance)?;
        Self::scoped_seat_agent_create(
            request_id,
            workspace_id,
            canonical_cwd,
            model_rung,
            title,
            labels,
            prompt,
            credential,
            mode,
        )
    }

    /// `create_agent_request` for one delivery seat, carrying its posture.
    ///
    /// # Why the posture rides in `providerOptions` and not in a file
    ///
    /// The installed Paseo 0.6.1 accepts a typed, zod-validated per-agent
    /// `providerOptions.permission` — OpenCode's own `Config.permission` shape —
    /// persists it on the agent record, and on every turn rebuilds it into
    /// `{permission, pattern, action}` rules that it passes to
    /// `session.promptAsync`. OpenCode installs those on the session before it
    /// evaluates any tool call. The policy is therefore delivered
    /// **provider-natively into the running session**, not merged out of files
    /// or environment variables that ambient configuration can outrank.
    ///
    /// A provider whose definition declares no `validateOptions` is *refused* by
    /// the daemon when options are sent, so this only ever sends them for a
    /// posture that renders a block — which today is OpenCode alone.
    ///
    /// # Why there is no `initialPrompt`
    ///
    /// The acceptance of the first real turn is what proves the policy was
    /// applied. A prompt carried on the create would start that turn before
    /// Kontor has an agent id to compensate against, so the create is made
    /// bare, the id is read from `agent_created`, and the prompt is a second,
    /// separately correlated call.
    ///
    /// # Errors
    /// As [`paseo_mode`]: a provider that cannot express the declared posture is
    /// refused rather than launched under a different one.
    #[allow(clippy::too_many_arguments)]
    pub fn delivery_agent_create(
        request_id: String,
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        autonomy: SeatAutonomy,
        title: &str,
        labels: &BTreeMap<String, String>,
        allowances: &[crate::posture::PermissionAllowance],
        seat_mcp: Option<&crate::seat_mcp::SeatMcp>,
        serve_profile: &str,
    ) -> RuntimeResult<Self> {
        // Rendered here, from the declaration, rather than accepted alongside
        // it. Taking a `SeatPosture` *and* an autonomy was two independent
        // statements of one thing: a miswired caller could send `modeId: build`
        // with a plan seat's deny block and every payload assertion would still
        // have passed. The mode below comes from the same render, so the two
        // halves of a posture cannot disagree by construction.
        let posture =
            crate::posture::render_posture(model_rung.provider.0.as_str(), autonomy, allowances)?;
        let mode = posture.mode;
        let mut config = serde_json::json!({
            "provider": model_rung.provider.0,
            "cwd": canonical_cwd,
            "model": model_rung.model.0,
            "title": title,
        });
        if let Some(mode) = mode {
            config["modeId"] = serde_json::json!(mode);
        }
        if let Some(effort) = model_rung.effort {
            config["thinkingOptionId"] = serde_json::json!(effort.as_str());
        }
        if let Some(permission) = posture.permission.as_ref() {
            config["providerOptions"] = serde_json::json!({ "permission": permission });
        }
        if let Some(seat_mcp) = seat_mcp {
            config["mcpServers"] = seat_mcp.server_config(serve_profile);
        }
        Ok(Self::mutate(
            "create_agent_request",
            "status",
            request_id,
            serde_json::json!({
                "config": config,
                "workspaceId": workspace_id,
                "labels": labels,
            }),
        ))
    }

    /// `create_agent_request` for one persistent hosted leadership seat. The
    /// credential uses the same secret-only frame channel as consultation
    /// credentials, while the seat retains its supervised provider mode.
    #[allow(clippy::too_many_arguments)]
    pub fn hosted_seat_agent_create(
        request_id: String,
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        title: &str,
        labels: &BTreeMap<String, String>,
        prompt: &str,
        credential: &str,
    ) -> RuntimeResult<Self> {
        let mode = paseo_mode(model_rung.provider.0.as_str(), SeatAutonomy::Supervised)?;
        Self::scoped_seat_agent_create(
            request_id,
            workspace_id,
            canonical_cwd,
            model_rung,
            title,
            labels,
            prompt,
            credential,
            mode,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn scoped_seat_agent_create(
        request_id: String,
        workspace_id: &str,
        canonical_cwd: &str,
        model_rung: &ModelRung,
        title: &str,
        labels: &BTreeMap<String, String>,
        prompt: &str,
        credential: &str,
        mode: Option<&str>,
    ) -> RuntimeResult<Self> {
        let mut config = serde_json::json!({
            "provider": model_rung.provider.0,
            "cwd": canonical_cwd,
            "model": model_rung.model.0,
            "title": title,
        });
        if let Some(mode) = mode {
            config["modeId"] = serde_json::json!(mode);
        }
        if let Some(effort) = model_rung.effort {
            config["thinkingOptionId"] = serde_json::json!(effort.as_str());
        }
        let mut request = Self::mutate(
            "create_agent_request",
            "status",
            request_id,
            serde_json::json!({
                "config": config,
                "workspaceId": workspace_id,
                "initialPrompt": prompt,
                "labels": labels,
            }),
        );
        request.secret_env.insert(
            "KONTOR_AUTH".to_owned(),
            SecretString::from(credential.to_owned()),
        );
        Ok(request)
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

    /// `archive_agent_request` for one exact agent.
    ///
    /// Compensation for a seat that was created and could not be proved. Sent
    /// over the same socket as the create so the archive is correlated the same
    /// way, rather than reaching for a second surface mid-failure.
    #[must_use]
    pub fn agent_archive(request_id: String, agent_id: &str) -> Self {
        Self::mutate(
            "archive_agent_request",
            "status",
            request_id,
            serde_json::json!({ "agentId": agent_id }),
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
        let mut message = self.message.clone();
        if !self.secret_env.is_empty() {
            message["env"] = serde_json::Value::Object(
                self.secret_env
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            serde_json::Value::String(value.expose_secret().to_owned()),
                        )
                    })
                    .collect(),
            );
        }
        serde_json::json!({ "type": "session", "message": message })
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
            secret_env: BTreeMap::new(),
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
        if output.stdout.len() > MAX_OUTPUT_BYTES || output.stderr.len() > MAX_OUTPUT_BYTES {
            return Err(RuntimeError::Transport {
                rule: "answer exceeded the bounded output size",
            });
        }
        // Stderr is retained only inside `PaseoOutput`, whose refusal parser
        // extracts one closed native-id shape and discards every other byte.
        // It is never logged or serialized: Paseo may write its credential-
        // bearing host URI, a path or prompt text there.
        let stdout = String::from_utf8(output.stdout).map_err(|_| RuntimeError::Transport {
            rule: "answer was not valid UTF-8",
        })?;
        let stderr = String::from_utf8(output.stderr).unwrap_or_default();
        Ok(PaseoOutput::with_stderr(
            output.status.code().unwrap_or(-1),
            stdout,
            stderr,
        ))
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
    use crate::wire::PaseoServerInfo;
    use kontor_core::id::ContentHash;
    use kontor_core::spec::{EffortLevel, ModelRef, ProviderRef};
    use kontor_runtime::adapter::{ConsultationFallbackDisposition, ConsultationRouteSource};

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

    fn template_provenance() -> ConsultationRouteProvenance {
        ConsultationRouteProvenance::template(ContentHash::of(b"template"))
    }

    fn accepted_initial_recovery_provenance() -> ConsultationRouteProvenance {
        ConsultationRouteProvenance::operator_accepted_initial_recovery_profile(ContentHash::of(
            b"initial recovery profile",
        ))
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
                Some("agt_orchestrator"),
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
            Some("agt_p"),
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
            Some("agt_p"),
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
            Some("agt_p"),
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

    /// A second account for one provider is a second provider id, so the mode
    /// has to come from the harness while `--provider` keeps the account.
    ///
    /// Getting this backwards is not a cosmetic error in either direction:
    /// resolving the mode from the full id refuses the launch outright, and
    /// sending the built-in on `--provider` would silently run the seat on
    /// whichever account the ambient credential home happens to hold.
    #[test]
    fn an_account_qualified_provider_resolves_its_harness_mode_and_keeps_its_own_id() {
        for (provider, built_in) in [
            ("codex", "codex"),
            ("codex-work", "codex"),
            ("codex-personal", "codex"),
            ("claude", "claude"),
            ("claude-work", "claude"),
            ("opencode-personal", "opencode"),
            ("omp-second", "omp"),
            ("pi-second", "pi"),
            // An `acp` provider whose id merely contains a hyphen, and an id
            // that only shares a prefix: neither names an account.
            ("deepseek-harness", "deepseek-harness"),
            ("codexfoo", "codexfoo"),
        ] {
            assert_eq!(built_in_provider(provider), built_in, "{provider}");
        }

        assert_eq!(
            permission_mode("codex-work").expect("an account of a supported provider"),
            Some("auto-review")
        );
        assert_eq!(
            paseo_mode("claude-work", SeatAutonomy::Bounded).expect("an account of Claude"),
            Some("bypassPermissions")
        );
        assert_eq!(
            consultation_permission_mode("codex-personal").expect("an account of Codex"),
            Some("auto-review")
        );

        // An id that resolves to no built-in still fails closed.
        assert!(matches!(
            permission_mode("deepseek-harness"),
            Err(RuntimeError::PermissionModeUnsupported { .. })
        ));

        let command = PaseoCommand::agent_run(
            "wks_1",
            "/w/task-1",
            &route("codex-work", "gpt-5.6-sol", Some(EffortLevel::Xhigh)),
            SeatAutonomy::Supervised,
            "KON-OP-13 Implement",
            &labels(),
            Some("agt_orchestrator"),
            "do the work",
        )
        .expect("an account of Codex has a pinned permission mode");
        let argv = command.argv();
        assert!(
            argv.windows(2)
                .any(|pair| pair == ["--provider", "codex-work"]),
            "the account id selects the credential home and must reach Paseo verbatim"
        );
        assert!(
            argv.windows(2)
                .any(|pair| pair == ["--mode", "auto-review"]),
            "the mode comes from the harness, not from the account id"
        );
    }

    #[test]
    fn consultation_routes_are_read_only_and_the_scoped_secret_crosses_only_the_session_frame() {
        for (provider, model, expected_mode) in [
            ("claude", "claude-opus-5", "plan"),
            ("codex", "gpt-5.6-sol", "auto-review"),
        ] {
            let request = PaseoRpc::consultation_agent_create(
                "request-1".to_owned(),
                "wks_1",
                "/w/epic",
                &route(provider, model, None),
                &template_provenance(),
                "Reviewer",
                &labels(),
                "read only",
                "seat-secret-value",
            )
            .expect("a consultation-safe provider");
            assert_eq!(request.message["config"]["modeId"], expected_mode);
            assert!(request.message.get("env").is_none());
            assert!(!format!("{request:?}").contains("seat-secret-value"));
            assert_eq!(
                request.envelope()["message"]["env"]["KONTOR_AUTH"],
                "seat-secret-value"
            );
        }
        assert!(matches!(
            consultation_permission_mode("cursor"),
            Err(RuntimeError::PermissionModeUnsupported { .. })
        ));
    }

    /// The ASMA-8001 fallback is exact and intentionally risk-accepted.
    ///
    /// OpenCode `plan` describes the seat's expected behavior but does not
    /// claim OS-level containment. The qualified canary demonstrated that a
    /// shell write remains possible. This contract proves only that the frozen
    /// DeepSeek Flash/max route and its scoped seat credential are transported
    /// without silently changing provider, model, effort or permission mode.
    #[test]
    fn opencode_deepseek_flash_max_consultation_fallback_is_constructible() {
        let request = PaseoRpc::consultation_agent_create(
            "request-deepseek-fallback".to_owned(),
            "wks_1",
            "/w/epic",
            &route(
                "opencode",
                "deepseek/deepseek-v4-flash",
                Some(EffortLevel::Max),
            ),
            &accepted_initial_recovery_provenance(),
            "Reviewer",
            &labels(),
            "audit without mutation",
            "seat-secret-value",
        )
        .expect("the operator-authorized OpenCode fallback is constructible");

        assert_eq!(request.message["config"]["provider"], "opencode");
        assert_eq!(
            request.message["config"]["model"],
            "deepseek/deepseek-v4-flash"
        );
        assert_eq!(request.message["config"]["thinkingOptionId"], "max");
        assert_eq!(request.message["config"]["modeId"], "plan");
        assert!(request.message.get("env").is_none());
        assert!(!format!("{request:?}").contains("seat-secret-value"));
        assert_eq!(
            request.envelope()["message"]["env"]["KONTOR_AUTH"],
            "seat-secret-value"
        );
    }

    #[test]
    fn opencode_consultation_fallback_rejects_every_non_exact_route_or_provenance() {
        let accepted = accepted_initial_recovery_provenance();
        let profile_hash = accepted.evidence_hash.clone();
        let cases = [
            (
                route("opencode", "deepseek/deepseek-v4-flash", None),
                accepted.clone(),
                "missing effort",
            ),
            (
                route(
                    "opencode",
                    "deepseek/deepseek-v4-flash",
                    Some(EffortLevel::High),
                ),
                accepted.clone(),
                "other effort",
            ),
            (
                route("opencode", "deepseek/deepseek-v3", Some(EffortLevel::Max)),
                accepted.clone(),
                "other model",
            ),
            (
                route(
                    "opencode-personal",
                    "deepseek/deepseek-v4-flash",
                    Some(EffortLevel::Max),
                ),
                accepted.clone(),
                "provider alias",
            ),
            (
                route(
                    "opencode",
                    "deepseek/deepseek-v4-flash",
                    Some(EffortLevel::Max),
                ),
                template_provenance(),
                "template or Advisor path",
            ),
            (
                route(
                    "opencode",
                    "deepseek/deepseek-v4-flash",
                    Some(EffortLevel::Max),
                ),
                ConsultationRouteProvenance {
                    source: ConsultationRouteSource::InitialRecoveryProfile,
                    evidence_hash: profile_hash.clone(),
                    fallback_disposition: None,
                },
                "missing operator disposition",
            ),
            (
                route(
                    "opencode",
                    "deepseek/deepseek-v4-flash",
                    Some(EffortLevel::Max),
                ),
                ConsultationRouteProvenance {
                    source: ConsultationRouteSource::InitialRecoveryProfile,
                    evidence_hash: profile_hash.clone(),
                    fallback_disposition: Some(ConsultationFallbackDisposition::Rejected),
                },
                "unaccepted operator disposition",
            ),
            (
                route(
                    "opencode",
                    "deepseek/deepseek-v4-flash",
                    Some(EffortLevel::Max),
                ),
                ConsultationRouteProvenance {
                    source: ConsultationRouteSource::MaterializationRecoveryProfile,
                    evidence_hash: profile_hash,
                    fallback_disposition: Some(ConsultationFallbackDisposition::OperatorAccepted),
                },
                "future recovery source",
            ),
        ];

        for (rung, provenance, case) in cases {
            let error = PaseoRpc::consultation_agent_create(
                format!("request-{case}"),
                "wks_1",
                "/w/epic",
                &rung,
                &provenance,
                "Reviewer",
                &labels(),
                "audit without mutation",
                "seat-secret-value",
            )
            .expect_err(case);
            assert!(
                matches!(error, RuntimeError::PermissionModeUnsupported { .. }),
                "{case}: {error:?}"
            );
        }
    }

    /// Regression contract for the Cursor/Grok 4.6 containment canary.
    ///
    /// On the qualified host, Cursor `plan` denied direct file tools but let a
    /// shell delete and create files; `ask` let file creation, replacement and
    /// shell creation execute, prompting only for deletion. Neither mode is an
    /// enforceable read-only boundary. Refusal must therefore happen while the
    /// request is being built, before a transport can dispatch a create-agent
    /// frame or an operator can be asked to decide a mutation.
    #[test]
    fn cursor_consultation_is_refused_before_an_rpc_can_be_constructed() {
        let error = PaseoRpc::consultation_agent_create(
            "request-cursor-canary".to_owned(),
            "wks_1",
            "/w/epic",
            &route("cursor", "grok-4.6", None),
            &template_provenance(),
            "Reviewer",
            &labels(),
            "read only",
            "seat-secret-value",
        )
        .expect_err("Cursor has no enforced non-mutating consultation mode");

        assert!(matches!(
            error,
            RuntimeError::PermissionModeUnsupported { provider } if provider == "cursor"
        ));
    }

    #[test]
    fn hosted_leadership_uses_supervised_mode_and_the_same_secret_only_frame() {
        let request = PaseoRpc::hosted_seat_agent_create(
            "request-1".to_owned(),
            "wks_1",
            "/w/epic",
            &route("claude-personal", "claude-opus-5", None),
            "LSA",
            &labels(),
            "continue governed leadership",
            "leadership-seat-secret",
        )
        .expect("a hosted Claude account route");
        assert_eq!(request.message["config"]["modeId"], "auto");
        assert!(request.message.get("env").is_none());
        assert!(!format!("{request:?}").contains("leadership-seat-secret"));
        assert_eq!(
            request.envelope()["message"]["env"]["KONTOR_AUTH"],
            "leadership-seat-secret"
        );
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
                Some("agt_p"),
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
            Some("agt_orchestrator"),
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
            Some("agt_orchestrator"),
            "p",
        )
        .expect("Codex has a pinned permission mode");
        assert_eq!(
            command.env(),
            [(PARENT_AGENT_ENV.to_owned(), "agt_orchestrator".to_owned())]
        );
    }

    /// Kontor already selected and attested the exact workspace. Its runtime
    /// configuration may still contain a caller left by another epic, but a
    /// top-level launch must not inherit it or create cross-epic parentage.
    #[test]
    fn a_top_level_launch_carries_no_ambient_caller() {
        let command = PaseoCommand::agent_run(
            "wks_1",
            "/w/task-1",
            &route("claude-work", "claude-opus-5", Some(EffortLevel::Xhigh)),
            SeatAutonomy::Supervised,
            "Architect · ASMA-7681",
            &labels(),
            None,
            "architect the task",
        )
        .expect("Claude Work has a pinned permission mode");

        assert!(command.env().is_empty());
        assert!(
            !command
                .argv()
                .iter()
                .any(|argument| { argument == "619d6f8a-0bbc-4b8d-a3ad-8e38a0cd8234" })
        );
    }

    #[test]
    fn a_missing_caller_refusal_preserves_only_the_exact_actionable_identity() {
        let caller = "619d6f8a-0bbc-4b8d-a3ad-8e38a0cd8234";
        let output = PaseoOutput::with_stderr(
            1,
            "stdout may also quote a credential-bearing host or prompt".to_owned(),
            format!(
                "credential-bearing host and prompt must disappear\nFailed to create agent: Caller agent {caller} not found"
            ),
        );
        let debug = format!("{output:?}");
        assert!(!debug.contains("credential-bearing"));
        assert!(!debug.contains("stdout may"));
        assert!(!debug.contains(caller));
        let refusal = output
            .parse::<serde_json::Value>("PaseoCliAgentStarted")
            .expect_err("Paseo refused the caller before creating an agent");

        assert_eq!(
            refusal,
            RuntimeError::CallerAgentNotFound {
                caller_agent_id: ExternalId::parse(caller).expect("a native caller id"),
            }
        );
        let rendered = refusal.to_string();
        assert!(rendered.contains(caller));
        assert!(!rendered.contains("credential-bearing"));
        assert!(!rendered.contains("prompt"));
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
            Some("agt_p"),
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
                Some("agt_p"),
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

    // ---- the delivery create contract, pinned to installed Paseo 0.6.1 -------

    fn delivery_create(
        autonomy: SeatAutonomy,
        provider: &str,
        mcp: Option<&crate::seat_mcp::SeatMcp>,
    ) -> PaseoRpc {
        delivery_create_with(autonomy, provider, mcp, &[])
    }

    fn delivery_create_with(
        autonomy: SeatAutonomy,
        provider: &str,
        mcp: Option<&crate::seat_mcp::SeatMcp>,
        allowances: &[crate::posture::PermissionAllowance],
    ) -> PaseoRpc {
        PaseoRpc::delivery_agent_create(
            "req-1".to_owned(),
            "wks_1",
            "/w/task-1",
            &route(provider, "deepseek/deepseek-v4-flash", None),
            autonomy,
            "Implement",
            &labels(),
            allowances,
            mcp,
            "worker",
        )
        .expect("the provider expresses this posture")
    }

    /// The exact shape `CreateAgentRequestMessageSchema` accepts in 0.6.1, and
    /// the posture in the field the daemon actually persists and replays.
    #[test]
    fn a_delivery_create_carries_the_posture_in_provider_options() {
        let request = delivery_create(SeatAutonomy::Bounded, "opencode", None);
        assert_eq!(request.request_type, "create_agent_request");
        assert!(request.mutates);

        let config = &request.message["config"];
        assert_eq!(config["provider"], "opencode");
        assert_eq!(config["modeId"], "build");
        let permission = &config["providerOptions"]["permission"];
        assert_eq!(permission["bash"]["*"], "allow");
        assert_eq!(permission["edit"], "allow");
        for pattern in crate::posture::DESTRUCTIVE_BASH_DENIES {
            assert_eq!(
                permission["bash"][*pattern], "deny",
                "the floor travels in the payload: `{pattern}`"
            );
        }
    }

    /// No prompt on the create: the id has to exist, and be compensable, before
    /// any turn starts.
    #[test]
    fn a_delivery_create_carries_no_initial_prompt() {
        let request = delivery_create(SeatAutonomy::Bounded, "opencode", None);
        assert!(
            request.message.get("initialPrompt").is_none(),
            "the first turn is a separate, separately correlated call"
        );
        assert!(request.message.get("clientMessageId").is_none());
    }

    /// A provider whose definition declares no `validateOptions` is refused by
    /// the daemon when options are sent, so they are only ever sent for a
    /// posture that renders a block.
    #[test]
    fn only_a_posture_with_a_block_sends_provider_options() {
        for provider in ["claude", "codex"] {
            let request = delivery_create(SeatAutonomy::Bounded, provider, None);
            assert!(
                request.message["config"].get("providerOptions").is_none(),
                "{provider} must not be sent providerOptions"
            );
        }
        assert!(
            delivery_create(SeatAutonomy::Bounded, "opencode", None).message["config"]
                .get("providerOptions")
                .is_some()
        );
    }

    /// Each posture maps to its own payload, exactly.
    #[test]
    fn each_posture_renders_its_own_provider_options_payload() {
        let bounded = delivery_create(SeatAutonomy::Bounded, "opencode", None);
        let ask = delivery_create(SeatAutonomy::Supervised, "opencode", None);
        let plan = delivery_create(SeatAutonomy::Advisory, "opencode", None);

        let permission =
            |r: &PaseoRpc| r.message["config"]["providerOptions"]["permission"].clone();
        assert_eq!(permission(&bounded)["bash"]["*"], "allow");
        assert_eq!(permission(&ask)["bash"]["*"], "ask");
        assert_eq!(permission(&plan)["bash"]["*"], "deny");
        assert_eq!(permission(&plan)["*"], "deny");
        assert_eq!(bounded.message["config"]["modeId"], "build");
        assert_eq!(plan.message["config"]["modeId"], "plan");

        assert_ne!(permission(&bounded), permission(&ask));
        assert_ne!(permission(&ask), permission(&plan));
    }

    /// The MCP surface belongs in the create config, never in a worktree file —
    /// and is built from the typed seat value rather than whatever JSON a caller
    /// assembled.
    #[test]
    fn the_seat_mcp_surface_travels_in_the_create_config() {
        let seat = crate::seat_mcp::SeatMcp {
            command: "kontor-mcp".to_owned(),
            state_root: std::path::PathBuf::from("/realm/state"),
        };
        let request = delivery_create(SeatAutonomy::Bounded, "opencode", Some(&seat));
        let entry = &request.message["config"]["mcpServers"]["kontor"];
        assert_eq!(entry["type"], "local");
        assert_eq!(
            entry["command"],
            serde_json::json!([
                "kontor-mcp",
                "--state-root",
                "/realm/state",
                "--credential-tier",
                "operator",
                "--serve-profile",
                "worker"
            ]),
            "one server, at operator tier, under the profile the caller named"
        );
    }

    /// The exact-floor allowance invariant survives into the payload: an
    /// exception flips one existing key and adds none.
    #[test]
    fn an_allowance_flips_one_floor_key_in_the_payload() {
        let allowance =
            crate::posture::PermissionAllowance::parse("*git rm --cached*").expect("floor member");
        let request = delivery_create_with(
            SeatAutonomy::Bounded,
            "opencode",
            None,
            std::slice::from_ref(&allowance),
        );

        let bash = &request.message["config"]["providerOptions"]["permission"]["bash"];
        assert_eq!(bash["*git rm --cached*"], "allow");
        assert_eq!(bash["*rm -rf *"], "deny");
        let plain = delivery_create(SeatAutonomy::Bounded, "opencode", None);
        let keys = |v: &serde_json::Value| {
            v.as_object()
                .expect("map")
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(
            keys(bash),
            keys(&plain.message["config"]["providerOptions"]["permission"]["bash"]),
            "an exception changes a value, never the key set"
        );
    }

    /// Mode and permission always come from **one** render.
    ///
    /// The constructor takes no posture, so the two halves cannot be supplied
    /// separately. This pins that they agree, and that the exact miswiring the
    /// old signature allowed — a `build` seat carrying a plan seat's deny block,
    /// or a `plan` seat carrying an allow-all one — cannot appear in any payload
    /// it produces.
    #[test]
    fn the_mode_and_the_permission_come_from_one_render() {
        for autonomy in [
            SeatAutonomy::Bounded,
            SeatAutonomy::Supervised,
            SeatAutonomy::Advisory,
        ] {
            let rendered =
                crate::posture::render_posture("opencode", autonomy, &[]).expect("a posture");
            let request = delivery_create(autonomy, "opencode", None);
            let config = &request.message["config"];
            assert_eq!(
                config["modeId"],
                serde_json::json!(rendered.mode.expect("opencode names a mode")),
                "{autonomy:?}: the mode is the rendered one"
            );
            assert_eq!(
                config["providerOptions"]["permission"],
                rendered.permission.expect("opencode renders a block"),
                "{autonomy:?}: the permission is the rendered one"
            );

            let bash = &config["providerOptions"]["permission"]["bash"]["*"];
            assert!(
                !(config["modeId"] == "build" && bash == "deny"),
                "{autonomy:?}: `build` must never carry a deny-all block"
            );
            assert!(
                !(config["modeId"] == "plan" && bash == "allow"),
                "{autonomy:?}: `plan` must never carry an allow-all block"
            );
        }
    }

    /// The payload carries the renderer's block verbatim.
    ///
    /// This is what makes hostile ambient configuration irrelevant: the rules
    /// are built in memory from [`crate::posture::render_posture`] and sent to
    /// the daemon, which persists them on the agent record and replays them into
    /// `session.promptAsync` every turn. Nothing on this path reads a file or an
    /// environment variable, so there is no layer for a global, project, managed
    /// or active-org configuration to win from — the payload is the policy.
    #[test]
    fn the_payload_carries_the_renderers_block_verbatim() {
        for autonomy in [
            SeatAutonomy::Bounded,
            SeatAutonomy::Supervised,
            SeatAutonomy::Advisory,
        ] {
            let rendered = crate::posture::render_posture("opencode", autonomy, &[])
                .expect("a posture")
                .permission
                .expect("opencode renders a block");
            let request = delivery_create(autonomy, "opencode", None);
            assert_eq!(
                request.message["config"]["providerOptions"]["permission"], rendered,
                "{autonomy:?}: the payload is the renderer's output and nothing else's"
            );
        }
    }

    // ---- seat environment, provider diagnostic, daemon contract -------------

    fn seat_env() -> Vec<(&'static str, String)> {
        crate::posture::seat_environment(
            &crate::posture::SeatConfigRoot::new("/realm/state/seats/agent-1"),
            &crate::posture::owned_config(&serde_json::json!({"bash": {"*": "deny"}}), None),
        )
    }

    /// The exact argv the installed CLI accepts. `--format json` is not a flag
    /// `provider diagnostic` has; `--json` is.
    #[test]
    fn the_provider_diagnostic_argv_is_the_one_the_cli_accepts() {
        let command = PaseoCommand::provider_diagnostic("opencode");
        assert_eq!(
            command.argv(),
            ["provider", "diagnostic", "opencode", "--json"],
            "pinned against `paseo provider diagnostic --help` on 0.6.1"
        );
        assert_eq!(command.route(), "provider diagnostic");
        assert!(!command.mutates(), "a diagnostic reads and changes nothing");
    }

    /// All six variables reach the agent, exactly once each, before the prompt.
    #[test]
    fn a_seat_environment_reaches_the_agent_as_repeated_env_flags() {
        let environment = seat_env();
        let command = PaseoCommand::agent_run_with_environment(
            "wks_1",
            "/w/task-1",
            &route("opencode", "deepseek/deepseek-v4-flash", None),
            SeatAutonomy::Bounded,
            "Implement",
            &labels(),
            None,
            "bootstrap",
            &environment,
        )
        .expect("the whole closed set is admitted");

        let argv = command.argv();
        let emitted: Vec<&String> = argv
            .iter()
            .enumerate()
            .filter(|(index, _)| *index > 0 && argv[index - 1] == "--env")
            .map(|(_, value)| value)
            .collect();
        assert_eq!(
            emitted.len(),
            crate::posture::SEAT_ENVIRONMENT_KEYS.len(),
            "one --env per declared variable, no more"
        );
        for key in crate::posture::SEAT_ENVIRONMENT_KEYS {
            assert_eq!(
                emitted
                    .iter()
                    .filter(|value| value.starts_with(&format!("{key}=")))
                    .count(),
                1,
                "`{key}` is carried exactly once"
            );
        }
        // The prompt is the trailing positional and never an argv word, so no
        // `--env` can be parsed as part of it.
        assert!(
            !argv.iter().any(|part| part == "bootstrap"),
            "the prompt stays out of the option list"
        );
    }

    /// The closed set is *whole*: an omission is refused like an addition,
    /// because dropping one variable silently re-admits what it excluded.
    #[test]
    fn a_seat_environment_that_is_not_the_whole_closed_set_is_refused() {
        let whole = seat_env();
        let attempt = |environment: &[(&'static str, String)]| {
            PaseoCommand::agent_run_with_environment(
                "wks_1",
                "/w/task-1",
                &route("opencode", "deepseek/deepseek-v4-flash", None),
                SeatAutonomy::Bounded,
                "Implement",
                &labels(),
                None,
                "bootstrap",
                environment,
            )
        };

        assert!(attempt(&[]).is_err(), "an empty set is not the closed set");

        for dropped in 0..whole.len() {
            let mut short = whole.clone();
            let (name, _) = short.remove(dropped);
            assert!(
                attempt(&short).is_err(),
                "dropping `{name}` must refuse the launch"
            );
        }

        let mut duplicated = whole.clone();
        duplicated[1] = duplicated[0].clone();
        assert!(attempt(&duplicated).is_err(), "a duplicate key is refused");

        let mut foreign = whole.clone();
        foreign[0] = ("PATH", "/tmp/evil".to_owned());
        assert!(attempt(&foreign).is_err(), "a foreign key is refused");

        let mut credential = whole.clone();
        credential[1].1 = r#"{"token":"bearer hunter2","password":"x"}"#.to_owned();
        assert!(
            attempt(&credential).is_err(),
            "configuration only, never a credential"
        );

        let mut oversized = whole.clone();
        oversized[1].1 = "x".repeat(MAX_SEAT_ENVIRONMENT_VALUE + 1);
        assert!(attempt(&oversized).is_err(), "values are bounded");
    }

    /// A seat's environment never reaches a log line.
    #[test]
    fn a_debug_rendering_redacts_every_env_value() {
        let command = PaseoCommand::agent_run_with_environment(
            "wks_1",
            "/w/task-1",
            &route("opencode", "deepseek/deepseek-v4-flash", None),
            SeatAutonomy::Bounded,
            "Implement",
            &labels(),
            None,
            "bootstrap",
            &seat_env(),
        )
        .expect("admitted");

        let rendered = format!("{command:?}");
        assert!(rendered.contains("--env"), "the shape stays visible");
        assert!(
            !rendered.contains("OPENCODE_PERMISSION="),
            "no value is rendered: {rendered}"
        );
        assert!(
            !rendered.contains("/realm/state/seats/agent-1"),
            "not even a path: {rendered}"
        );
        assert_eq!(
            rendered.matches("<redacted>").count(),
            crate::posture::SEAT_ENVIRONMENT_KEYS.len() * 2,
            "redacted in the argv and in the separate values list"
        );
        assert!(
            !rendered.contains("opencode.ai/config.json"),
            "the carried configuration is not rendered either: {rendered}"
        );
    }

    /// The CLI accepting `--env` is not proof that the daemon applies it, so the
    /// contract is read from the daemon's own reported version and fails closed
    /// on anything it cannot read.
    #[test]
    fn per_agent_environment_is_gated_on_the_daemons_reported_version() {
        let at = |version: Option<&str>| PaseoServerInfo {
            server_id: "srv".to_owned(),
            version: version.map(str::to_owned),
            hostname: None,
            features: BTreeMap::new(),
        };
        assert!(
            !at(None).supports_seat_environment(),
            "a daemon that reports no version has not agreed to anything"
        );
        assert!(!at(Some("")).supports_seat_environment());
        assert!(!at(Some("not-a-version")).supports_seat_environment());
        assert!(!at(Some("0.4.0")).supports_seat_environment());
        assert!(!at(Some("0.6.0")).supports_seat_environment());
        assert!(
            !at(Some("0.6.1-beta.1")).supports_seat_environment(),
            "a pre-release sorts below the release it is named for"
        );
        assert!(at(Some("0.6.1")).supports_seat_environment());
        assert!(at(Some("0.7.0")).supports_seat_environment());
        assert!(
            at(Some("0.10.0")).supports_seat_environment(),
            "compared numerically, not as text"
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
