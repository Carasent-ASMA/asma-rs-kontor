//! Typed client adapters for the five supported MCP clients.
//!
//! Each adapter patches exactly one `kontor` entry in one client-owned
//! configuration surface and reads the entry back through the same surface.
//! The contracts are explicit per client rather than a plugin framework, and a
//! shape this repository does not document is refused with a typed conflict
//! instead of guessed at.
//!
//! Reference: the accepted KON-OP-08 table of authoritative user-scoped
//! client shapes.

pub mod claude;
pub mod file_adapter;
pub mod json_edit;

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::artifact::install::InstallError;
use crate::artifact::manifest::sha256_hex;
use crate::fault::FaultInjector;

/// The executable name every managed entry must point at.
pub const MCP_EXECUTABLE: &str = "kontor-mcp";

/// One supported MCP client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ClientId {
    /// The Codex CLI/IDE host configuration.
    #[serde(rename = "codex")]
    Codex,
    /// Claude Code, addressed only through its native command surface.
    #[serde(rename = "claude-code")]
    ClaudeCode,
    /// OpenCode's global configuration file.
    #[serde(rename = "opencode")]
    OpenCode,
    /// The VS Code/Copilot portable user configuration.
    #[serde(rename = "copilot")]
    Copilot,
    /// Cursor's user configuration.
    #[serde(rename = "cursor")]
    Cursor,
}

impl ClientId {
    /// Every supported client, in stable order.
    pub const ALL: [ClientId; 5] = [
        ClientId::Codex,
        ClientId::ClaudeCode,
        ClientId::OpenCode,
        ClientId::Copilot,
        ClientId::Cursor,
    ];

    /// The stable lowercase name used in receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ClientId::Codex => "codex",
            ClientId::ClaudeCode => "claude-code",
            ClientId::OpenCode => "opencode",
            ClientId::Copilot => "copilot",
            ClientId::Cursor => "cursor",
        }
    }

    /// Parse one stable name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|client| client.as_str() == text)
    }
}

impl fmt::Display for ClientId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The one server entry bootstrap manages, in typed form.
///
/// This value is never serialized into a receipt: its program is a host path
/// and its arguments carry no secret but are not the receipt's business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSpec {
    /// Absolute path of the installed `kontor-mcp` executable.
    pub program: PathBuf,
    /// Arguments: state root and credential tier, per KON-OP-08.
    pub args: Vec<String>,
}

impl ServerSpec {
    /// Whether the entry points at the managed executable name.
    #[must_use]
    pub fn points_at_managed_executable(&self) -> bool {
        self.program
            .file_name()
            .is_some_and(|name| name == MCP_EXECUTABLE)
    }
}

/// A digest an adapter observed for one existing entry.
///
/// Two hashes are comparable only when they address the same entry region, so
/// adapters expose the exact observed value and `repair` requires it verbatim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ObservedHash(String);

impl ObservedHash {
    /// The digest of one canonical `(command, args)` pair.
    #[must_use]
    pub fn of_entry(program: &str, args: &[String]) -> Self {
        let canonical = serde_json::json!({"command": program, "args": args});
        Self(sha256_hex(canonical.to_string().as_bytes()))
    }

    /// The digest of an entry that cannot be reduced to `(command, args)`.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(sha256_hex(bytes))
    }

    /// The lowercase hex digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parse one lowercase hex digest, as printed by [`ObservedHash::as_str`].
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let valid = text.len() == 64
            && text
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        valid.then(|| Self(text.to_owned()))
    }
}

impl fmt::Display for ObservedHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Why an entry was refused without being written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    /// A same-name entry exists that bootstrap did not write.
    SameNameUnrelated,
    /// The document uses a schema this build does not implement.
    LegacySchema,
    /// Two candidate configuration files exist and neither is authoritative.
    AmbiguousConfig,
    /// A path that would be written is a symlink.
    Symlink,
    /// The target changed between reading and writing it.
    ConcurrentChange,
    /// The existing document cannot be parsed.
    Unparsable,
    /// A repair was asked to replace a digest other than the one observed.
    ObservedHashMismatch,
}

impl ConflictReason {
    /// The stable lowercase name used in receipts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ConflictReason::SameNameUnrelated => "same_name_unrelated",
            ConflictReason::LegacySchema => "legacy_schema",
            ConflictReason::AmbiguousConfig => "ambiguous_config",
            ConflictReason::Symlink => "symlink",
            ConflictReason::ConcurrentChange => "concurrent_change",
            ConflictReason::Unparsable => "unparsable",
            ConflictReason::ObservedHashMismatch => "observed_hash_mismatch",
        }
    }
}

/// What one adapter observed, without changing anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EntryState {
    /// The client itself is not present on this machine.
    ClientAbsent,
    /// The client is present and has no `kontor` entry.
    EntryAbsent,
    /// The entry is bootstrap's own and already exact.
    Current {
        /// The observed digest.
        observed: ObservedHash,
    },
    /// The entry is bootstrap's own but not the desired revision.
    ManagedStale {
        /// The observed digest.
        observed: ObservedHash,
    },
    /// A same-name entry exists that bootstrap did not write.
    Unrelated {
        /// The observed digest, which `repair` must cite.
        observed: ObservedHash,
    },
    /// The surface is present but cannot be interpreted without guessing.
    Refused {
        /// Why it was refused.
        reason: ConflictReason,
    },
}

/// What one install or repair operation did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum AdapterResult {
    /// The entry was absent and has been written.
    Installed,
    /// An entry bootstrap owns was replaced by the desired revision.
    Updated,
    /// The entry already matched the desired revision.
    AlreadyCurrent,
    /// An explicitly authorized repair replaced a refused entry.
    Repaired,
    /// The client or entry is absent; nothing was written.
    Absent,
    /// The entry was refused and nothing was written.
    Conflict {
        /// Why.
        reason: ConflictReason,
        /// The observed digest when the surface could produce one.
        observed: Option<ObservedHash>,
    },
    /// The write landed but the entry did not read back as desired.
    FailedReadback,
}

impl AdapterResult {
    /// The stable lowercase name used in receipts.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            AdapterResult::Installed => "installed",
            AdapterResult::Updated => "updated",
            AdapterResult::AlreadyCurrent => "already_current",
            AdapterResult::Repaired => "repaired",
            AdapterResult::Absent => "absent",
            AdapterResult::Conflict { .. } => "conflict",
            AdapterResult::FailedReadback => "failed_readback",
        }
    }
}

/// Why an adapter could not complete.
///
/// Variants are path-free so an error can reach a redacted receipt or a stderr
/// line without leaking a host location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AdapterError {
    /// The injected client root is relative, which bootstrap refuses.
    #[error("the client root must be an absolute path")]
    RelativeRoot,
    /// The server specification cannot be written.
    #[error("the server specification is not installable")]
    InvalidSpec,
    /// A filesystem operation failed.
    #[error("the client configuration could not be written")]
    Io,
    /// The client's command surface failed in a way bootstrap cannot classify.
    #[error("the client command surface failed")]
    CommandFailed,
    /// The target changed between reading and writing it.
    #[error("the client configuration changed while bootstrap was running")]
    ConcurrentChange,
    /// A fault injector interrupted the client write.
    #[error("the client write was interrupted")]
    Injected,
}

impl From<InstallError> for AdapterError {
    fn from(error: InstallError) -> Self {
        match error {
            InstallError::Injected { .. } => AdapterError::Injected,
            _ => AdapterError::CommandFailed,
        }
    }
}

/// A redacted per-client receipt record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientReceipt {
    /// Which client.
    pub client: ClientId,
    /// The typed outcome.
    pub result: AdapterResult,
}

/// One client adapter.
pub trait ClientAdapter {
    /// Which client this adapter drives.
    fn client(&self) -> ClientId;

    /// Observe the surface without changing it.
    ///
    /// # Errors
    /// A filesystem failure, an installable-spec violation, or an
    /// unclassifiable command-boundary failure.
    fn inspect(&mut self, spec: &ServerSpec) -> Result<EntryState, AdapterError>;

    /// Install or update the `kontor` entry.
    ///
    /// # Errors
    /// A filesystem or command-boundary failure that is not itself a typed
    /// conflict.
    fn install(
        &mut self,
        spec: &ServerSpec,
        faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError>;

    /// Replace a refused entry only when its observed digest is the expected
    /// one, proving the operator saw exactly what is being replaced.
    ///
    /// # Errors
    /// A filesystem or command-boundary failure that is not itself a typed
    /// conflict.
    fn repair(
        &mut self,
        spec: &ServerSpec,
        expected: &ObservedHash,
        faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError>;
}
