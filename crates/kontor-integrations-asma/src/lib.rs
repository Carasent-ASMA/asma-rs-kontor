//! `kontor-integrations-asma` — asma fleet and asma jira sync subprocess boundaries
//!
//! Kontor compiles the *policy* for an external ticket and delegates every
//! *effect* to the supported `asma` executable. Three properties are structural
//! here rather than conventional:
//!
//! 1. **One writer.** This crate has no HTTP client, no Jira URL, no filesystem
//!    edge and no knowledge of `~/.asma/fleet`. Its only way to reach the world
//!    is [`AsmaExecutable`], which runs an argv array — never a shell — and
//!    exchanges one schema-versioned JSON document over stdin/stdout.
//! 2. **Policy is data, decisions are core.** Status ids, status names, field
//!    ids and option ids live in the versioned specifications under `fixtures/`
//!    and in receipts. The decision itself is
//!    [`kontor_core::ticket::reconcile`]; this crate adds no evaluator branch,
//!    so a second project with an entirely different status vocabulary takes the
//!    identical code path.
//! 3. **Nothing is believed without a refetch.** An apply is only `applied`
//!    once a fresh observation confirms it, and an ambiguous result is
//!    reconciled — never replayed — before any retry.
//!
//! ## What crosses the boundary
//!
//! Rust resolves *identity and semantics*: which field id, which option id,
//! which destination status selector, which ownership action. The CLI owns
//! *wire encoding*: turning bounded text into the connector's document format
//! with its one already-verified converter. Reimplementing that encoder here
//! would create a second producer of the same document and therefore a second
//! canonical form for identical content, which is exactly the drift the
//! encoding comparison on the CLI side exists to detect.
//!
//! A transition id is never configured and never remembered: it is read from the
//! live observation that immediately precedes the apply, matched on its
//! destination status id, and recorded as evidence.

use std::fmt;

use kontor_core::DomainError;
use kontor_core::ticket::StatusConflictKind;

pub mod fleet;
pub mod jira;
mod process;

pub use process::{AsmaExecutable, DEFAULT_MAX_STDOUT_BYTES, DEFAULT_TIMEOUT};

/// Why the `asma` boundary could not answer.
///
/// Every variant is a *typed unavailable result*, not a panic and not a silent
/// empty answer: a caller must be able to tell "the world says no" from "we
/// could not ask".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    /// The executable could not be started.
    Spawn,
    /// Reading or writing the child's pipes failed.
    Transport,
    /// The invocation exceeded its wall-clock budget.
    Timeout,
    /// The child wrote more than the configured output bound.
    OversizedOutput,
    /// The child exited non-zero.
    ExitStatus,
    /// The child's answer was not the expected JSON document.
    MalformedResponse,
    /// The answer declared a schema this build does not speak.
    SchemaMismatch,
}

impl UnavailableReason {
    /// The stable spelling, for receipts and diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spawn => "spawn",
            Self::Transport => "transport",
            Self::Timeout => "timeout",
            Self::OversizedOutput => "oversized_output",
            Self::ExitStatus => "exit_status",
            Self::MalformedResponse => "malformed_response",
            Self::SchemaMismatch => "schema_mismatch",
        }
    }
}

impl fmt::Display for UnavailableReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Why a specification could not be selected.
///
/// "Choose the newest one" is deliberately absent: a work item that pinned a
/// revision which no longer exists must fail loudly rather than silently follow
/// the specification as it changes underneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionConflict {
    /// No specification matches the requested keys and revision.
    NoMatch,
    /// More than one specification matches; the catalogue is ambiguous.
    Ambiguous,
    /// Specifications exist for these keys, but none at the pinned work-profile
    /// revision.
    ProfileRevisionMismatch,
}

impl SelectionConflict {
    /// The stable spelling, for receipts and diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoMatch => "no_match",
            Self::Ambiguous => "ambiguous",
            Self::ProfileRevisionMismatch => "profile_revision_mismatch",
        }
    }
}

impl fmt::Display for SelectionConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Every rejection this crate produces.
///
/// Diagnostics carry a static operation name plus, for an unavailable result, a
/// bounded and credential-scrubbed tail of what the child said. A candidate
/// value is never echoed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AsmaError {
    /// A domain type refused a value.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// A pinned specification could not be selected.
    #[error("cannot select the pinned {subject}: {conflict}")]
    Selection {
        /// Which specification kind was being selected.
        subject: &'static str,
        /// Why selection failed.
        conflict: SelectionConflict,
    },
    /// The `asma` boundary could not answer.
    #[error("asma {operation} is unavailable ({reason}): {detail}")]
    Unavailable {
        /// The operation that was attempted.
        operation: &'static str,
        /// Why it could not answer.
        reason: UnavailableReason,
        /// Bounded, credential-scrubbed evidence.
        detail: String,
    },
    /// Reconciliation produced a conflict for a human.
    #[error("asma {operation} reconciliation conflict: {kind}")]
    Conflict {
        /// The operation that was attempted.
        operation: &'static str,
        /// The conflict the pure evaluator returned.
        kind: StatusConflictKind,
    },
    /// This crate refused to build the request at all.
    #[error("asma {operation} refused: {rule}")]
    Refused {
        /// The operation that was attempted.
        operation: &'static str,
        /// The invariant that was violated.
        rule: &'static str,
    },
}

impl AsmaError {
    /// Build an [`AsmaError::Unavailable`], bounding and scrubbing `detail`.
    #[must_use]
    pub fn unavailable(
        operation: &'static str,
        reason: UnavailableReason,
        detail: impl AsRef<str>,
    ) -> Self {
        Self::Unavailable {
            operation,
            reason,
            detail: redact(detail.as_ref()),
        }
    }

    /// Build an [`AsmaError::Refused`].
    #[must_use]
    pub const fn refused(operation: &'static str, rule: &'static str) -> Self {
        Self::Refused { operation, rule }
    }
}

/// Maximum length of any diagnostic tail this crate is willing to carry.
const MAX_DETAIL_BYTES: usize = 512;

/// Bound a diagnostic and drop it entirely if it carries credential material.
///
/// The scrubber is [`kontor_core::id::reject_sensitive_text`], which is the same
/// primitive every persisted string in the domain goes through — so a token that
/// would be refused by the store is also refused by a log line here. It is
/// all-or-nothing on purpose: partially masking a secret still tells a reader
/// how long it was and where it started.
fn redact(detail: &str) -> String {
    let mut bounded: String = detail.chars().take(MAX_DETAIL_BYTES).collect();
    if bounded.len() < detail.len() {
        bounded.push('…');
    }
    let scrubbed = bounded.replace(['\r', '\n'], " ");
    if kontor_core::id::reject_sensitive_text("asma diagnostic", &scrubbed).is_err() {
        return "[redacted: the child's diagnostic carried credential material]".to_owned();
    }
    scrubbed
}

/// The schema generation of every request and response this build speaks.
///
/// It is [`kontor_core::id::SCHEMA_VERSION`] rather than a private constant so
/// the wire and the persisted domain cannot drift apart by one being bumped
/// alone.
pub const WIRE_SCHEMA_VERSION: kontor_core::id::SchemaVersion = kontor_core::id::SCHEMA_VERSION;

/// A timestamp on the wire, admitted only in the form the domain persists.
///
/// The wire is held to the same rule as SQL: canonical UTC RFC 3339, so a value
/// that parses but renders differently — an offset, a lowercase `z`, a redundant
/// fraction — is refused instead of silently normalized. Without this, two
/// receipts recording the same instant could disagree byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireTimestamp(kontor_core::id::Timestamp);

impl WireTimestamp {
    /// Carry an already-validated instant onto the wire.
    #[must_use]
    pub const fn new(timestamp: kontor_core::id::Timestamp) -> Self {
        Self(timestamp)
    }

    /// The validated instant.
    #[must_use]
    pub const fn get(self) -> kontor_core::id::Timestamp {
        self.0
    }
}

impl fmt::Display for WireTimestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&kontor_core::id::format_utc_timestamp(self.0))
    }
}

impl serde::Serialize for WireTimestamp {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&kontor_core::id::format_utc_timestamp(self.0))
    }
}

impl<'de> serde::Deserialize<'de> for WireTimestamp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let text = String::deserialize(deserializer)?;
        kontor_core::id::parse_utc_timestamp(&text)
            .map(Self)
            .map_err(D::Error::custom)
    }
}

/// Refuse a response that declares a schema this build does not speak.
///
/// # Errors
/// Returns [`AsmaError::Unavailable`] with
/// [`UnavailableReason::SchemaMismatch`].
pub(crate) fn ensure_wire_schema(
    operation: &'static str,
    declared: kontor_core::id::SchemaVersion,
) -> Result<(), AsmaError> {
    if declared == WIRE_SCHEMA_VERSION {
        return Ok(());
    }
    Err(AsmaError::Unavailable {
        operation,
        reason: UnavailableReason::SchemaMismatch,
        detail: format!(
            "the response declares schema {} and this build speaks {}",
            declared.get(),
            WIRE_SCHEMA_VERSION.get()
        ),
    })
}
