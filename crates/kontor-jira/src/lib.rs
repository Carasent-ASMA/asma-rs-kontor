//! Native Jira policy and transport for Kontor.
//!
//! Policy remains pure in [`jira`]. Live effects cross [`JiraConnector`], which
//! reads strict operator configuration and resolves its keychain secret for each
//! call. No Jira credential or arbitrary mutation document enters the public API.

#![allow(missing_docs)]

use std::fmt;

use kontor_core::DomainError;
use kontor_core::ticket::StatusConflictKind;

mod connector;
pub mod jira;

pub use connector::{
    JiraComment, JiraConfig, JiraConnector, JiraConnectors, JiraIssueKind, JiraIssuePlan,
    JiraIssueReadback, JiraProjectConfig, install_credentials,
};

/// Why Jira could not answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    Spawn,
    Transport,
    Timeout,
    OversizedOutput,
    ExitStatus,
    MalformedResponse,
    SchemaMismatch,
    Configuration,
    Credential,
}

impl UnavailableReason {
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
            Self::Configuration => "configuration",
            Self::Credential => "credential",
        }
    }
}

impl fmt::Display for UnavailableReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Why a pinned Jira specification could not be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionConflict {
    NoMatch,
    Ambiguous,
    ProfileRevisionMismatch,
}

impl SelectionConflict {
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

/// Every rejection the Jira policy or connector produces.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JiraError {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("cannot select the pinned {subject}: {conflict}")]
    Selection {
        subject: &'static str,
        conflict: SelectionConflict,
    },
    #[error("jira {operation} is unavailable ({reason}): {detail}")]
    Unavailable {
        operation: &'static str,
        reason: UnavailableReason,
        detail: String,
    },
    #[error("jira {operation} reconciliation conflict: {kind}")]
    Conflict {
        operation: &'static str,
        kind: StatusConflictKind,
    },
    #[error("jira {operation} refused: {rule}")]
    Refused {
        operation: &'static str,
        rule: &'static str,
    },
}

impl JiraError {
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

    #[must_use]
    pub const fn refused(operation: &'static str, rule: &'static str) -> Self {
        Self::Refused { operation, rule }
    }
}

const MAX_DETAIL_BYTES: usize = 512;

fn redact(detail: &str) -> String {
    let mut bounded: String = detail.chars().take(MAX_DETAIL_BYTES).collect();
    if bounded.len() < detail.len() {
        bounded.push('…');
    }
    let scrubbed = bounded.replace(['\r', '\n'], " ");
    if kontor_core::id::reject_sensitive_text("jira diagnostic", &scrubbed).is_err() {
        return "[redacted: the diagnostic carried credential material]".to_owned();
    }
    scrubbed
}

pub const WIRE_SCHEMA_VERSION: kontor_core::id::SchemaVersion = kontor_core::id::SCHEMA_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireTimestamp(kontor_core::id::Timestamp);

impl WireTimestamp {
    #[must_use]
    pub const fn new(timestamp: kontor_core::id::Timestamp) -> Self {
        Self(timestamp)
    }

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
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        kontor_core::id::parse_utc_timestamp(&text)
            .map(Self)
            .map_err(D::Error::custom)
    }
}

pub(crate) fn ensure_wire_schema(
    operation: &'static str,
    declared: kontor_core::id::SchemaVersion,
) -> Result<(), JiraError> {
    if declared == WIRE_SCHEMA_VERSION {
        return Ok(());
    }
    Err(JiraError::Unavailable {
        operation,
        reason: UnavailableReason::SchemaMismatch,
        detail: format!(
            "the response declares schema {} and this build speaks {}",
            declared.get(),
            WIRE_SCHEMA_VERSION.get()
        ),
    })
}
