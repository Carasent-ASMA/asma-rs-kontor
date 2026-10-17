//! Typed client adapters for the five supported MCP clients.
//!
//! Each adapter patches exactly one `kontor` entry in one client-owned
//! configuration surface and reads the entry back through the same surface.
//! Ownership is provenance-based, never name-based: an executable called
//! `kontor-mcp` proves nothing. An existing entry may be replaced only when a
//! bootstrap-owned ledger records the exact full snapshot this adapter wrote,
//! or when a repair cites the exact full observed digest.
//!
//! Reference: the accepted KON-OP-08 table of authoritative user-scoped
//! client shapes.

pub mod claude;
pub mod file_adapter;
pub mod json_edit;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::artifact::install::InstallError;
use crate::artifact::manifest::sha256_hex;
use crate::confinement::{ConfinementError, Dir, MAX_DOCUMENT_BYTES, NodeKind};
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
/// and its arguments are not the receipt's business.
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

    /// Refuse a specification that cannot be safely written.
    ///
    /// # Errors
    /// [`AdapterError::InvalidSpec`] for a relative program, a wrong basename,
    /// an oversized argument vector, an oversized argument, or a NUL byte.
    pub fn validate(&self) -> Result<(), AdapterError> {
        let path = self.program.to_string_lossy();
        if !self.program.is_absolute()
            || !self.points_at_managed_executable()
            || path.contains('\0')
            || path.len() > 4096
            || self.args.len() > 32
            || self
                .args
                .iter()
                .any(|argument| argument.contains('\0') || argument.len() > 4096)
        {
            return Err(AdapterError::InvalidSpec);
        }
        Ok(())
    }
}

/// A digest of one complete observed snapshot (entry value or file region).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ObservedHash(String);

impl<'de> Deserialize<'de> for ObservedHash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        ObservedHash::parse(&text).ok_or_else(|| {
            serde::de::Error::custom("an observed hash must be 64 lowercase hex characters")
        })
    }
}

impl ObservedHash {
    /// The digest of one complete JSON value, canonically serialized.
    #[must_use]
    pub fn of_value(value: &Value) -> Self {
        Self(sha256_hex(
            serde_json::to_string(value).unwrap_or_default().as_bytes(),
        ))
    }

    /// The digest of one canonical `(command, args)` pair.
    #[must_use]
    pub fn of_entry(program: &str, args: &[String]) -> Self {
        Self::of_value(&serde_json::json!({"command": program, "args": args}))
    }

    /// The digest of arbitrary bytes, for a region that cannot be typed.
    #[must_use]
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self(sha256_hex(bytes))
    }

    /// The lowercase hex digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Test-only constructor that deliberately bypasses the checked
    /// deserializer, so nested receipt validation can be isolated from
    /// deserialization.
    #[cfg(test)]
    pub(crate) fn unchecked(text: &str) -> Self {
        Self(text.to_owned())
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
    /// The boundary cannot remove conditionally, so replacement is refused.
    CasUnavailable,
    /// The ownership ledger could not be read, so no write is safe.
    OwnershipUnreadable,
    /// A bounded document exceeded its size limit.
    TooLarge,
    /// A document nested deeper than the supported bound.
    TooDeep,
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
            ConflictReason::CasUnavailable => "cas_unavailable",
            ConflictReason::OwnershipUnreadable => "ownership_unreadable",
            ConflictReason::TooLarge => "too_large",
            ConflictReason::TooDeep => "too_deep",
        }
    }
}

/// What one adapter observed, without changing anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EntryState {
    /// The client itself is not present on this machine.
    ClientAbsent,
    /// The client is present but this build has no boundary to drive it.
    Unsupported,
    /// The client is present and has no `kontor` entry.
    EntryAbsent,
    /// The entry is bootstrap's own and already exact.
    Current {
        /// The full observed snapshot digest.
        observed: ObservedHash,
    },
    /// The entry is bootstrap's own but not the desired revision.
    ManagedStale {
        /// The full observed snapshot digest.
        observed: ObservedHash,
    },
    /// A same-name entry exists that bootstrap did not write.
    Unrelated {
        /// The full observed snapshot digest, which `repair` must cite.
        observed: ObservedHash,
    },
    /// The surface is present but cannot be interpreted without guessing.
    Refused {
        /// Why it was refused.
        reason: ConflictReason,
    },
}

impl EntryState {
    /// The full observed digest, when the surface produced one.
    #[must_use]
    pub fn observed(&self) -> Option<&ObservedHash> {
        match self {
            EntryState::Current { observed }
            | EntryState::ManagedStale { observed }
            | EntryState::Unrelated { observed } => Some(observed),
            EntryState::ClientAbsent
            | EntryState::Unsupported
            | EntryState::EntryAbsent
            | EntryState::Refused { .. } => None,
        }
    }
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
    /// This build has no boundary that can drive the present client.
    Unsupported,
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
            AdapterResult::Unsupported => "unsupported",
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
    /// The injected client or state root is relative.
    #[error("the injected root must be an absolute path")]
    RelativeRoot,
    /// The server specification is not installable.
    #[error("the server specification is not installable")]
    InvalidSpec,
    /// A filesystem operation failed.
    #[error("the client configuration could not be written")]
    Io,
    /// Confined filesystem access refused an operation.
    #[error(transparent)]
    Confinement(#[from] ConfinementError),
    /// The client's command surface failed in a way bootstrap cannot classify.
    #[error("the client command surface failed")]
    CommandFailed,
    /// The target changed between reading and writing it.
    #[error("the client configuration changed while bootstrap was running")]
    ConcurrentChange,
    /// A fault injector interrupted the client write.
    #[error("the client write was interrupted")]
    Injected,
    /// The ownership ledger exists but cannot be read, so no write is safe.
    #[error("the ownership ledger could not be read; no write is safe")]
    OwnershipUnreadable,
    /// A replacement failed and the original entry could not be restored.
    #[error("the original entry could not be restored after a failed replacement")]
    RestoreFailed,
    /// Bootstrap-owned and foreign material could not be separated after the
    /// name exchange, so both names were preserved: neither the installed nor
    /// the displaced name was exchanged back or unlinked.
    #[error("both the installed and displaced material were preserved")]
    PreserveBoth,
}

impl From<InstallError> for AdapterError {
    fn from(error: InstallError) -> Self {
        match error {
            InstallError::Injected { .. } => AdapterError::Injected,
            InstallError::Confinement(error) => AdapterError::Confinement(error),
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

/// One provenance record: the exact full snapshot this adapter wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnershipRecord {
    entry_sha256: String,
    spec_sha256: String,
}

/// The bootstrap-owned provenance ledger, keyed by client.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnershipLedger {
    entries: BTreeMap<ClientId, OwnershipRecord>,
    pending: BTreeMap<ClientId, OwnershipRecord>,
}

impl OwnershipLedger {
    /// Whether this ledger proves bootstrap wrote exactly this snapshot.
    #[must_use]
    pub fn owns(&self, client: ClientId, observed: &ObservedHash) -> bool {
        self.entries
            .get(&client)
            .is_some_and(|record| record.entry_sha256 == observed.as_str())
    }

    /// Whether a write of exactly this snapshot was in progress and is not yet
    /// completed in the committed entries.
    #[must_use]
    pub fn pending_owns(&self, client: ClientId, observed: &ObservedHash) -> bool {
        self.pending
            .get(&client)
            .is_some_and(|record| record.entry_sha256 == observed.as_str())
    }
}

/// The ledger file under an injected state root.
#[derive(Debug, Clone)]
pub struct OwnershipStore {
    state_root: PathBuf,
}

const LEDGER_FILE: &str = "bootstrap-ownership.json";
const LEDGER_LOCK_FILE: &str = ".bootstrap-ownership.lock";
const LEDGER_SCHEMA_VERSION: u32 = 1;

/// The exclusive holder of the ledger lock file.
struct LedgerLock {
    _file: std::fs::File,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerDocument {
    schema_version: u32,
    entries: BTreeMap<ClientId, OwnershipRecord>,
    #[serde(default)]
    pending: BTreeMap<ClientId, OwnershipRecord>,
}

impl OwnershipStore {
    /// A store under one explicit state root.
    ///
    /// # Errors
    /// [`AdapterError::RelativeRoot`] for a relative path.
    pub fn at(state_root: impl Into<PathBuf>) -> Result<Self, AdapterError> {
        let state_root = state_root.into();
        if !state_root.is_absolute() {
            return Err(AdapterError::RelativeRoot);
        }
        Ok(Self { state_root })
    }

    /// Load the ledger.
    ///
    /// A missing root or ledger is an empty ledger. A present but unreadable or
    /// malformed ledger is a typed error: treating it as empty could authorize
    /// an overwrite.
    ///
    /// # Errors
    /// [`AdapterError::OwnershipUnreadable`].
    pub fn load(&self) -> Result<OwnershipLedger, AdapterError> {
        let dir = match Dir::open_root(&self.state_root) {
            Ok(dir) => dir,
            Err(ConfinementError::Missing) => return Ok(OwnershipLedger::default()),
            Err(_) => return Err(AdapterError::OwnershipUnreadable),
        };
        let Some(bytes) = dir
            .read_child(LEDGER_FILE, MAX_DOCUMENT_BYTES)
            .map_err(|_| AdapterError::OwnershipUnreadable)?
        else {
            return Ok(OwnershipLedger::default());
        };
        let document: LedgerDocument =
            serde_json::from_slice(&bytes).map_err(|_| AdapterError::OwnershipUnreadable)?;
        if document.schema_version != LEDGER_SCHEMA_VERSION {
            return Err(AdapterError::OwnershipUnreadable);
        }
        for record in document.entries.values().chain(document.pending.values()) {
            if ObservedHash::parse(&record.entry_sha256).is_none()
                || ObservedHash::parse(&record.spec_sha256).is_none()
            {
                return Err(AdapterError::OwnershipUnreadable);
            }
        }
        Ok(OwnershipLedger {
            entries: document.entries,
            pending: document.pending,
        })
    }

    /// Persist that a write of exactly this snapshot is about to commit.
    ///
    /// The record is durable before the configuration changes, so a replay can
    /// complete ownership after an interruption between the write and the
    /// ledger update.
    fn begin(
        &self,
        client: ClientId,
        entry: &ObservedHash,
        spec: &ObservedHash,
    ) -> Result<(), AdapterError> {
        let _lock = self.lock()?;
        let mut ledger = self.load()?;
        ledger.pending.insert(
            client,
            OwnershipRecord {
                entry_sha256: entry.as_str().to_owned(),
                spec_sha256: spec.as_str().to_owned(),
            },
        );
        self.write(&ledger)
    }

    fn record(
        &self,
        client: ClientId,
        entry: &ObservedHash,
        spec: &ObservedHash,
    ) -> Result<(), AdapterError> {
        let _lock = self.lock()?;
        let mut ledger = self.load()?;
        ledger.entries.insert(
            client,
            OwnershipRecord {
                entry_sha256: entry.as_str().to_owned(),
                spec_sha256: spec.as_str().to_owned(),
            },
        );
        ledger.pending.remove(&client);
        self.write(&ledger)
    }

    /// Take the exclusive cross-process lock that serializes one complete
    /// load-modify-write of the ledger.
    fn lock(&self) -> Result<LedgerLock, AdapterError> {
        let dir = self.open_or_create_root()?;
        let file = match dir.kind_child(LEDGER_LOCK_FILE) {
            Ok(Some(NodeKind::File)) => dir
                .open_child_file_rw(LEDGER_LOCK_FILE)
                .map_err(|_| AdapterError::OwnershipUnreadable)?,
            Ok(Some(_)) => return Err(AdapterError::OwnershipUnreadable),
            Ok(None) => match dir.create_child_file(LEDGER_LOCK_FILE, &[], 0o600) {
                Ok(file) => file,
                Err(ConfinementError::Exists) => dir
                    .open_child_file_rw(LEDGER_LOCK_FILE)
                    .map_err(|_| AdapterError::OwnershipUnreadable)?,
                Err(_) => return Err(AdapterError::OwnershipUnreadable),
            },
            Err(_) => return Err(AdapterError::OwnershipUnreadable),
        };
        FileExt::lock(&file).map_err(|_| AdapterError::Io)?;
        Ok(LedgerLock { _file: file })
    }

    fn write(&self, ledger: &OwnershipLedger) -> Result<(), AdapterError> {
        let document = LedgerDocument {
            schema_version: LEDGER_SCHEMA_VERSION,
            entries: ledger.entries.clone(),
            pending: ledger.pending.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&document).map_err(|_| AdapterError::Io)?;
        let dir = self.open_or_create_root()?;
        let temporary = format!(
            ".{LEDGER_FILE}.tmp-{}-{}",
            std::process::id(),
            crate::artifact::install::nonce_suffix()
        );
        let file = dir.create_child_file(&temporary, &bytes, 0o600)?;
        drop(file);
        dir.rename_child(&temporary, &dir, LEDGER_FILE)?;
        Ok(())
    }

    fn open_or_create_root(&self) -> Result<Dir, AdapterError> {
        match Dir::open_root(&self.state_root) {
            Ok(dir) => Ok(dir),
            Err(ConfinementError::Missing) => {
                std::fs::create_dir_all(&self.state_root).map_err(|_| AdapterError::Io)?;
                Dir::open_root(&self.state_root).map_err(AdapterError::from)
            }
            Err(error) => Err(error.into()),
        }
    }
}

/// One client adapter.
pub trait ClientAdapter {
    /// Which client this adapter drives.
    fn client(&self) -> ClientId;

    /// Observe the surface without changing it.
    ///
    /// # Errors
    /// A filesystem failure, an installable-spec violation, an unreadable
    /// ownership ledger, or an unclassifiable command-boundary failure.
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

    /// Replace a refused entry only when its full observed digest is the
    /// expected one, proving the operator saw exactly what is being replaced.
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

/// Whether a program string names the managed executable.
///
/// This is *not* an ownership test: it only says the entry points at a path
/// whose final component is `kontor-mcp`. Ownership comes from the ledger.
#[must_use]
pub fn basename(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or(program)
}

/// Whether a path names the managed executable.
#[must_use]
pub fn is_managed_executable(program: &str) -> bool {
    basename(program) == MCP_EXECUTABLE
}

/// The canonical JSON of one desired entry, per client.
#[must_use]
pub fn desired_entry(client: ClientId, spec: &ServerSpec) -> Value {
    match client {
        ClientId::Codex => serde_json::json!({
            "command": spec.program.to_string_lossy(),
            "args": spec.args,
        }),
        ClientId::ClaudeCode => serde_json::json!({
            "type": "stdio",
            "command": spec.program.to_string_lossy(),
            "args": spec.args,
        }),
        ClientId::OpenCode => {
            let mut command = vec![spec.program.to_string_lossy().into_owned()];
            command.extend(spec.args.iter().cloned());
            serde_json::json!({"type": "local", "command": command})
        }
        ClientId::Copilot => serde_json::json!({
            "type": "stdio",
            "command": spec.program.to_string_lossy(),
            "args": spec.args,
        }),
        ClientId::Cursor => serde_json::json!({
            "command": spec.program.to_string_lossy(),
            "args": spec.args,
        }),
    }
}

/// Classify one observed full entry value against the ledger and the spec.
#[must_use]
pub fn classify(observed: &Value, desired: &Value, owned: bool) -> EntryState {
    let hash = ObservedHash::of_value(observed);
    if observed == desired {
        return EntryState::Current { observed: hash };
    }
    if owned {
        return EntryState::ManagedStale { observed: hash };
    }
    EntryState::Unrelated { observed: hash }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> ServerSpec {
        ServerSpec {
            program: PathBuf::from("/opt/kontor tools/current/kontor-mcp"),
            args: vec![
                "--state-root".to_owned(),
                "/synthetic/realm".to_owned(),
                "--credential-tier".to_owned(),
                "admin".to_owned(),
            ],
        }
    }

    #[test]
    fn a_spec_is_validated_before_any_effect() {
        assert!(spec().validate().is_ok());
        let mut relative = spec();
        relative.program = PathBuf::from("relative/kontor-mcp");
        assert_eq!(relative.validate(), Err(AdapterError::InvalidSpec));
        let mut other = spec();
        other.program = PathBuf::from("/opt/other-mcp");
        assert_eq!(other.validate(), Err(AdapterError::InvalidSpec));
        let mut huge = spec();
        huge.args = vec!["x".repeat(5000)];
        assert_eq!(huge.validate(), Err(AdapterError::InvalidSpec));
    }

    #[test]
    fn ownership_is_by_exact_snapshot_not_by_name() {
        let desired = desired_entry(ClientId::Codex, &spec());
        let unowned = serde_json::json!({"command": "/x/kontor-mcp", "args": []});
        assert!(matches!(
            classify(&unowned, &desired, false),
            EntryState::Unrelated { .. }
        ));
        // Even the exact bytes we would write are not "owned" on their own.
        assert!(matches!(
            classify(&desired, &desired, false),
            EntryState::Current { .. }
        ));
        let changed = serde_json::json!({
            "command": desired["command"],
            "args": desired["args"],
            "env": {"KONTOR_X": "1"}
        });
        assert!(matches!(
            classify(&changed, &desired, true),
            EntryState::ManagedStale { .. }
        ));
        assert!(matches!(
            classify(&changed, &desired, false),
            EntryState::Unrelated { .. }
        ));
    }

    #[test]
    fn a_full_snapshot_hash_changes_with_env_and_extra_fields() {
        let first = serde_json::json!({"command": "/x/kontor-mcp", "args": [], "env": {"A": "1"}});
        let second = serde_json::json!({"command": "/x/kontor-mcp", "args": [], "env": {"A": "2"}});
        let third = serde_json::json!({"command": "/x/kontor-mcp", "args": [], "extra": true});
        assert_ne!(
            ObservedHash::of_value(&first),
            ObservedHash::of_value(&second)
        );
        assert_ne!(
            ObservedHash::of_value(&first),
            ObservedHash::of_value(&third)
        );
    }

    #[test]
    fn observed_hashes_are_checked_on_deserialization() {
        let valid = ObservedHash::of_bytes(b"x");
        let text = serde_json::to_string(&valid).expect("serialize");
        assert_eq!(
            serde_json::from_str::<ObservedHash>(&text).expect("valid"),
            valid
        );
        assert!(serde_json::from_str::<ObservedHash>("\"not-a-hash\"").is_err());
        assert!(serde_json::from_str::<ObservedHash>("\"ABCDEF\"").is_err());
    }

    #[test]
    fn observed_hash_guards_are_length_and_charset_specific() {
        // Three distinct invalid shapes plus the legitimate control, so a
        // length-only or charset-only regression cannot pass unnoticed.
        let short_lowercase_hex = "abcdef".to_owned();
        let short_by_one = "a".repeat(63);
        let uppercase = "A".repeat(64);
        let non_hex = format!("{}g", "a".repeat(63));
        for forged in [&short_lowercase_hex, &short_by_one, &uppercase, &non_hex] {
            assert!(
                ObservedHash::parse(forged).is_none(),
                "not a canonical digest: {forged:?}"
            );
            assert!(
                serde_json::from_str::<ObservedHash>(&format!("\"{forged}\"")).is_err(),
                "the checked deserializer refuses {forged:?}"
            );
        }
        let legitimate = "0123456789abcdef".repeat(4);
        let parsed = ObservedHash::parse(&legitimate).expect("the lowercase-64 control parses");
        assert_eq!(parsed.as_str(), legitimate);
        let round_trip: ObservedHash = serde_json::from_str(&format!("\"{legitimate}\""))
            .expect("the control survives the checked deserializer");
        assert_eq!(round_trip, parsed);
    }

    #[test]
    fn the_ledger_is_bound_to_an_absolute_state_root() {
        assert_eq!(
            OwnershipStore::at("relative").unwrap_err(),
            AdapterError::RelativeRoot
        );
        let holder = tempfile::tempdir().expect("tempdir");
        let store = OwnershipStore::at(holder.path().join("state")).expect("store");
        assert_eq!(store.load().expect("load"), OwnershipLedger::default());
        let record = ObservedHash::of_bytes(b"entry");
        let spec_hash = ObservedHash::of_bytes(b"spec");
        store
            .record(ClientId::Codex, &record, &spec_hash)
            .expect("record");
        let loaded = store.load().expect("load");
        assert!(loaded.owns(ClientId::Codex, &record));
        assert!(!loaded.owns(ClientId::Cursor, &record));
        assert!(!loaded.owns(ClientId::Codex, &ObservedHash::of_bytes(b"other")));
    }

    #[test]
    fn a_ledger_write_holds_an_exclusive_cross_process_lock() {
        let holder = tempfile::tempdir().expect("tempdir");
        let state = holder.path().join("state");
        let store = OwnershipStore::at(&state).expect("store");
        let guard = store.lock().expect("lock");
        let other = std::fs::File::open(state.join(LEDGER_LOCK_FILE)).expect("open lock file");
        assert!(matches!(
            FileExt::try_lock(&other),
            Err(fs4::TryLockError::WouldBlock)
        ));
        drop(guard);
        assert!(FileExt::try_lock(&other).is_ok());
    }

    #[test]
    fn a_ledger_update_waits_for_the_held_exclusive_lock() {
        use std::sync::{Arc, Barrier};
        use std::time::Duration;

        let holder = tempfile::tempdir().expect("tempdir");
        let state = holder.path().join("state");
        let store = OwnershipStore::at(&state).expect("store");
        let guard = store.lock().expect("lock");
        let record = ObservedHash::of_bytes(b"entry");
        let spec_hash = ObservedHash::of_bytes(b"spec");
        let worker = store.clone();
        let worker_record = record.clone();
        let ready = Arc::new(Barrier::new(2));
        let worker_ready = Arc::clone(&ready);
        let handle = std::thread::spawn(move || {
            worker_ready.wait();
            worker.record(ClientId::Codex, &worker_record, &spec_hash)
        });
        ready.wait();
        // The worker has reached `record` and is blocked on the held lock. If
        // `record` stops taking the lock, the entry lands inside this window.
        std::thread::sleep(Duration::from_millis(250));
        assert!(
            !store.load().expect("load").owns(ClientId::Codex, &record),
            "record did not wait for the held ledger lock"
        );
        drop(guard);
        handle
            .join()
            .expect("the record thread")
            .expect("record after the lock is released");
        assert!(store.load().expect("load").owns(ClientId::Codex, &record));
    }

    #[test]
    fn a_malformed_ledger_refuses_instead_of_reading_as_empty() {
        let holder = tempfile::tempdir().expect("tempdir");
        let state = holder.path().join("state");
        std::fs::create_dir_all(&state).expect("state");
        std::fs::write(state.join(LEDGER_FILE), "not json").expect("write");
        let store = OwnershipStore::at(&state).expect("store");
        assert_eq!(store.load().unwrap_err(), AdapterError::OwnershipUnreadable);
    }
}
