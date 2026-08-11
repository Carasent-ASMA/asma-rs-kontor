//! The exact `codex exec --json` surface this adapter is pinned to, and nothing
//! else.
//!
//! Codex is not a session server. It is a one-shot process that prints JSON
//! Lines to stdout and exits, so the whole wire surface is: the frame envelope,
//! the way a frame is classified, the ways a process can end, and the non-secret
//! marker an operator plants in an approved `CODEX_HOME`.
//!
//! Two rules decide what is typed and what is not, and they pull in opposite
//! directions on purpose — the same split `kontor-runtime-ao` makes:
//!
//! 1. **A value Kontor would otherwise guess is closed.** How a process *ended*
//!    is [`CodexEnding`], and every variant of it means one thing: the process or
//!    the channel ended. There is deliberately no variant that means "finished",
//!    because Codex cannot tell a clean completion from a crash, and neither can
//!    an exit status.
//! 2. **A value Kontor only routes on is open text.** `msg.type` grows with every
//!    Codex release, so it is a `String`. An unrecognized type becomes a
//!    [`SessionEventKind::Log`] carrying its raw frame verbatim: delivered,
//!    sequenced and preserved, never silently dropped and never read as a state
//!    fact.
//!
//! Unknown *fields* inside a frame are accepted, because a Codex patch release
//! that adds one must not be a total adapter outage. The marker is the exception
//! and denies them: an operator-owned file that grew a `token` field is a file
//! this adapter refuses to hold in memory at all.

use kontor_core::id::{AccountProfileId, ExternalId};
use kontor_core::{DomainError, DomainResult};
use kontor_runtime::timeline::SessionEventKind;
use serde::{Deserialize, Serialize};

/// The Codex stdout protocol this adapter's types, fixtures and tests are
/// pinned to.
///
/// Named rather than versioned against a CLI release on purpose: `codex` has no
/// stable protocol version field to read back, so what is pinned is the *shape*
/// — `{"id": …, "msg": {"type": …}}` JSON Lines on stdout — and anything that is
/// not that shape fails as a typed domain error instead of being interpreted.
pub const CODEX_EXEC_SCHEMA: &str = "codex.exec.jsonl/v1";

/// The frame type that acknowledges a started Codex session.
pub const LAUNCH_ACK_TYPE: &str = "session_configured";

/// Largest single stdout line the adapter will accept, in bytes.
///
/// A Codex frame carries model output, so this is generous; what it refuses is
/// a runaway line that would otherwise be canonicalized and persisted whole.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;

/// Largest number of frames one drain will accept.
pub const MAX_FRAMES_PER_DRAIN: usize = 4096;

/// The non-secret identity marker an operator plants in each approved
/// `CODEX_HOME`.
pub const MARKER_FILE_NAME: &str = "kontor-profile.json";

/// The marker contract this adapter reads.
pub const MARKER_SCHEMA_VERSION: u32 = 1;

/// The one environment variable this adapter fills, and the only one it will
/// accept a resolved account environment for.
pub const CODEX_HOME: &str = "CODEX_HOME";

/// The variable the run's correlation label is planted in.
///
/// Codex ignores it. It exists so the environment block the child actually
/// receives can be checked against the label Kontor planted — see
/// [`crate::adapter::CodexAdapter`] for what that check does and does not prove.
pub const KONTOR_RUN_ENV: &str = "KONTOR_AGENT_RUN";

/// What every redacted rendering prints instead of a value.
///
/// One spelling, used by every hand-written `Debug` in this crate, because two
/// spellings is how a redaction gets written correctly in one place and forgotten
/// in the other. See [`crate::client::CodexCommand`] for the rule these
/// renderings follow and why none of them may be derived.
pub const REDACTED: &str = "<redacted>";

fn refuse(subject: &'static str, rule: &'static str) -> DomainError {
    DomainError::invalid(subject, rule)
}

// ---------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------

/// One `codex exec --json` stdout line.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CodexFrame {
    /// Codex's own submission id for the frame.
    pub id: String,
    /// The event body.
    pub msg: CodexMessage,
}

/// The body of one frame.
///
/// Only the two fields the adapter acts on are declared. Everything else Codex
/// prints — model text, command output, token counts — stays in the raw line,
/// which is what gets canonicalized as evidence. A field that is never parsed
/// cannot be accidentally promoted into a lifecycle conclusion.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CodexMessage {
    /// The event type. Open text: routed on, never guessed into a run state.
    #[serde(rename = "type")]
    pub kind: String,
    /// Codex's own session identifier, present on the launch acknowledgement.
    #[serde(default)]
    pub session_id: Option<String>,
}

impl CodexFrame {
    /// Parse one bounded stdout line.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for a line over [`MAX_FRAME_BYTES`], for
    /// anything that is not JSON, and for JSON that is not this envelope. None of
    /// those is dropped: a malformed frame is a typed failure, because a hole a
    /// caller cannot see is worse than a refusal it can.
    pub fn parse(line: &str) -> DomainResult<Self> {
        if line.len() > MAX_FRAME_BYTES {
            return Err(refuse("CodexFrame", "exceeds the bounded frame size"));
        }
        serde_json::from_str(line)
            .map_err(|_| refuse("CodexFrame", "is not the codex exec JSON Lines envelope"))
    }

    /// The Codex session this frame acknowledges, when it is the launch
    /// acknowledgement.
    #[must_use]
    pub fn launch_ack_session(&self) -> Option<&str> {
        if self.msg.kind != LAUNCH_ACK_TYPE {
            return None;
        }
        self.msg.session_id.as_deref()
    }

    /// How this frame enters a session's content.
    ///
    /// Deliberately total. An unrecognized type is diagnostic output rather than
    /// an error *and* rather than a lifecycle fact: it is delivered with its raw
    /// frame intact so nothing is lost, and it can never move a run state,
    /// because only the types named here are read as state at all.
    #[must_use]
    pub fn event_kind(&self) -> SessionEventKind {
        match self.msg.kind.as_str() {
            "agent_message"
            | "agent_message_delta"
            | "agent_reasoning"
            | "agent_reasoning_delta"
            | "user_message" => SessionEventKind::Message,
            "exec_command_begin"
            | "exec_command_end"
            | "exec_command_output_delta"
            | "mcp_tool_call_begin"
            | "mcp_tool_call_end"
            | "patch_apply_begin"
            | "patch_apply_end"
            | "web_search_begin"
            | "web_search_end" => SessionEventKind::ToolCall,
            // The two lifecycle facts Codex states explicitly. They are session
            // *content*, not a control-plane observation, so neither can close a
            // run — see `CodexAdapter::inspect` for the only place a run state is
            // ever produced, and what it is allowed to say.
            LAUNCH_ACK_TYPE | "task_started" | "task_complete" | "turn_aborted" => {
                SessionEventKind::StateChange
            }
            _ => SessionEventKind::Log,
        }
    }
}

// ---------------------------------------------------------------------------
// How a process ended
// ---------------------------------------------------------------------------

/// The ways a Codex child stops being a channel Kontor can read.
///
/// Every variant means exactly one thing: **the process or its output channel
/// ended.** None of them means the work succeeded, failed or was cancelled, and
/// there is no variant that could — which is the point. A Codex `exec` that
/// crashed, one that was killed, one that hit its deadline and one that finished
/// its turn cleanly are indistinguishable from out here, and an exit status is
/// the most tempting way to pretend otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CodexEnding {
    /// Stdout closed.
    Eof,
    /// The process exited with a status. **Advisory:** a status is a fact about
    /// a process, never a verdict on the work.
    Exited {
        /// The reported status code.
        code: i32,
    },
    /// The process was terminated by a signal.
    Signalled {
        /// The reported signal number.
        signal: i32,
    },
    /// The adapter's deadline elapsed.
    TimedOut,
    /// The adapter killed the process on an explicit cancellation.
    Killed,
    /// The process is gone and no status could be read for it.
    Vanished,
}

impl CodexEnding {
    /// Every ending, for a suite that must state its rule over all of them.
    pub const ALL: &'static [Self] = &[
        Self::Eof,
        Self::Exited { code: 0 },
        Self::Exited { code: 1 },
        Self::Signalled { signal: 9 },
        Self::TimedOut,
        Self::Killed,
        Self::Vanished,
    ];

    /// The stable spelling recorded in canonical evidence.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eof => "eof",
            Self::Exited { .. } => "exited",
            Self::Signalled { .. } => "signalled",
            Self::TimedOut => "timed_out",
            Self::Killed => "killed",
            Self::Vanished => "vanished",
        }
    }

    /// The status or signal this ending reported, when it reported one.
    ///
    /// Recorded as evidence and read by nothing that decides a run state.
    #[must_use]
    pub const fn reported_code(self) -> Option<i32> {
        match self {
            Self::Exited { code } => Some(code),
            Self::Signalled { signal } => Some(signal),
            Self::Eof | Self::TimedOut | Self::Killed | Self::Vanished => None,
        }
    }
}

// ---------------------------------------------------------------------------
// The approved-home marker
// ---------------------------------------------------------------------------

/// The operator-owned, non-secret identity marker inside one approved
/// `CODEX_HOME`.
///
/// It exists so a resolved home can be checked against the account profile the
/// run is pinned to *before* a Codex process is started under it. Everything it
/// carries is a name: a schema version, the profile id, and the non-secret
/// provider identity the deployment already records on the profile.
///
/// `deny_unknown_fields` is the security property, not tidiness. Kontor never
/// opens `auth.json`, a token file, a cookie jar or a keychain entry, and a
/// marker that grew a `token` field would be exactly that in disguise — so a
/// marker carrying anything beyond these three fields fails to parse and is
/// never held in this process at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexHomeMarker {
    /// The marker contract this file was written under.
    pub schema_version: u32,
    /// The account profile this home authenticates as.
    pub account_profile_id: AccountProfileId,
    /// The non-secret provider identity the profile is expected to carry.
    pub provider_identity: ExternalId,
}

impl CodexHomeMarker {
    /// Read one marker file's text.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for a marker that is not this shape, that
    /// carries any field beyond the three above, or that was written under
    /// another schema version.
    pub fn parse(raw: &str) -> DomainResult<Self> {
        if raw.len() > MAX_FRAME_BYTES {
            return Err(refuse(
                "CodexHomeMarker",
                "is implausibly large for a marker",
            ));
        }
        let marker: Self = serde_json::from_str(raw).map_err(|_| {
            refuse(
                "CodexHomeMarker",
                "is not the non-secret Kontor profile marker this adapter reads",
            )
        })?;
        if marker.schema_version != MARKER_SCHEMA_VERSION {
            return Err(refuse(
                "CodexHomeMarker",
                "was written under a marker schema version this binary does not read",
            ));
        }
        Ok(marker)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_malformed_or_oversized_frame_fails_typed_rather_than_being_dropped() {
        assert!(CodexFrame::parse("not json").is_err());
        assert!(CodexFrame::parse("[]").is_err());
        // The envelope is required whole: a body without `type` is not a frame
        // whose kind this adapter may guess at.
        assert!(CodexFrame::parse("{\"id\":\"0\",\"msg\":{}}").is_err());
        assert!(CodexFrame::parse("{\"msg\":{\"type\":\"task_started\"}}").is_err());
        let oversized = format!(
            "{{\"id\":\"0\",\"msg\":{{\"type\":\"agent_message\",\"m\":\"{}\"}}}}",
            "x".repeat(MAX_FRAME_BYTES)
        );
        assert!(CodexFrame::parse(&oversized).is_err());
    }

    #[test]
    fn an_unknown_frame_type_is_delivered_as_a_log_rather_than_refused_or_guessed() {
        // The mutant this kills is either direction: refusing the whole stream
        // because Codex added an event type, or rounding that event into a
        // lifecycle fact.
        let frame = CodexFrame::parse(
            "{\"id\":\"7\",\"msg\":{\"type\":\"some_future_event\",\"detail\":{\"a\":1}}}",
        )
        .expect("an unknown type still parses as a frame");
        assert_eq!(frame.event_kind(), SessionEventKind::Log);
        assert_eq!(frame.launch_ack_session(), None);
    }

    #[test]
    fn only_the_launch_acknowledgement_carries_a_session() {
        let ack = CodexFrame::parse(
            "{\"id\":\"0\",\"msg\":{\"type\":\"session_configured\",\"session_id\":\"cdx-1\"}}",
        )
        .expect("a launch acknowledgement");
        assert_eq!(ack.launch_ack_session(), Some("cdx-1"));
        // A session id on any other frame is not an acknowledgement, so it can
        // never become the native identity a binding is built on.
        let other = CodexFrame::parse(
            "{\"id\":\"1\",\"msg\":{\"type\":\"task_started\",\"session_id\":\"cdx-9\"}}",
        )
        .expect("a task frame");
        assert_eq!(other.launch_ack_session(), None);
        // And an acknowledgement with no session id is not one either.
        let bare = CodexFrame::parse("{\"id\":\"0\",\"msg\":{\"type\":\"session_configured\"}}")
            .expect("a bare acknowledgement");
        assert_eq!(bare.launch_ack_session(), None);
    }

    #[test]
    fn frames_are_classified_into_content_kinds_and_never_into_permissions() {
        for (kind, expected) in [
            ("agent_message", SessionEventKind::Message),
            ("agent_reasoning", SessionEventKind::Message),
            ("exec_command_begin", SessionEventKind::ToolCall),
            ("mcp_tool_call_end", SessionEventKind::ToolCall),
            ("task_complete", SessionEventKind::StateChange),
            ("error", SessionEventKind::Log),
            ("token_count", SessionEventKind::Log),
        ] {
            let frame =
                CodexFrame::parse(&format!("{{\"id\":\"1\",\"msg\":{{\"type\":\"{kind}\"}}}}"))
                    .expect("a frame");
            assert_eq!(frame.event_kind(), expected, "{kind}");
            // Codex `exec` has no structured permission surface, so no frame may
            // ever be read as one: a synthesized permission request would invite
            // an answer nothing can deliver.
            assert!(!matches!(
                frame.event_kind(),
                SessionEventKind::PermissionRequest | SessionEventKind::PermissionResolved
            ));
        }
    }

    #[test]
    fn no_ending_can_spell_a_verdict() {
        // The whole point of the type: there is no `Succeeded`, no `Failed` and
        // no `Cancelled` variant to reach for, and a zero exit is spelled the
        // same way as a non-zero one.
        for ending in CodexEnding::ALL {
            assert!(matches!(
                ending.as_str(),
                "eof" | "exited" | "signalled" | "timed_out" | "killed" | "vanished"
            ));
        }
        assert_eq!(CodexEnding::Exited { code: 0 }.as_str(), "exited");
        assert_eq!(CodexEnding::Exited { code: 1 }.as_str(), "exited");
        assert_eq!(CodexEnding::Exited { code: 0 }.reported_code(), Some(0));
        assert_eq!(CodexEnding::Eof.reported_code(), None);
    }

    #[test]
    fn a_marker_that_carries_anything_else_is_refused_rather_than_read() {
        let good = CodexHomeMarker::parse(
            "{\"schema_version\":1,\
             \"account_profile_id\":\"01936f4a-0000-7000-8000-00000000000a\",\
             \"provider_identity\":\"codex-account-a@example.test\"}",
        )
        .expect("the non-secret marker");
        assert_eq!(good.schema_version, MARKER_SCHEMA_VERSION);

        // A marker that grew a credential field is a credential file, and this
        // adapter does not read credential files.
        assert!(
            CodexHomeMarker::parse(
                "{\"schema_version\":1,\
                 \"account_profile_id\":\"01936f4a-0000-7000-8000-00000000000a\",\
                 \"provider_identity\":\"codex-account-a@example.test\",\
                 \"refresh\":\"kontor-canary-marker-extra\"}",
            )
            .is_err()
        );
        // Another schema version is refused rather than read optimistically.
        assert!(
            CodexHomeMarker::parse(
                "{\"schema_version\":2,\
                 \"account_profile_id\":\"01936f4a-0000-7000-8000-00000000000a\",\
                 \"provider_identity\":\"codex-account-a@example.test\"}",
            )
            .is_err()
        );
        assert!(CodexHomeMarker::parse("{}").is_err());
    }
}
