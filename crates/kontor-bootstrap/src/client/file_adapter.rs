//! The four file-backed client adapters.
//!
//! Codex, OpenCode, VS Code/Copilot and Cursor each own one user-scoped
//! configuration file in a documented format. The adapter patches exactly the
//! `kontor` entry, leaves every other byte alone, refuses a symlink at any
//! component, binds every write to the complete observed file and entry
//! snapshot, and requires bootstrap-owned provenance (or an explicit repair
//! digest) before replacing anything.
//!
//! The shapes follow the KON-OP-08 table as corrected: `[mcp_servers.kontor]`
//! for Codex, `mcpServers.kontor` stdio for Copilot, and `mcpServers.kontor`
//! for Cursor. OpenCode deliberately differs from that table's
//! `mcp.servers.kontor` row: OpenCode resolves the flat `mcp.kontor` member, so
//! that is the entry path (see the dated corrections beside those rows).

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::json_edit::{Dialect, EntryLookup, JsonDocument, JsonEditError};
use super::{
    AdapterError, AdapterResult, ClientAdapter, ClientId, ConflictReason, EntryState, ObservedHash,
    OwnershipStore, ServerSpec, desired_entry,
};
use crate::confinement::{ConfinementError, Dir, MAX_DOCUMENT_BYTES, NodeKind};
use crate::fault::{FailPoint, FaultInjector};

/// The injected client home: the synthetic stand-in for a user home.
#[derive(Debug, Clone)]
pub struct ClientHome {
    root: PathBuf,
}

impl ClientHome {
    /// A client home for one explicit root.
    ///
    /// # Errors
    /// [`AdapterError::RelativeRoot`] for a relative path.
    pub fn at(root: impl Into<PathBuf>) -> Result<Self, AdapterError> {
        let root = root.into();
        if !root.is_absolute() {
            return Err(AdapterError::RelativeRoot);
        }
        Ok(Self { root })
    }

    /// The root itself.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// One adapter over one client's configuration file.
#[derive(Debug, Clone)]
pub struct FileClientAdapter {
    client: ClientId,
    home_root: PathBuf,
    ownership: OwnershipStore,
}

/// The full observation one adapter made, holding open directory descriptors.
struct Observed {
    dir: Option<Dir>,
    file: Option<(&'static str, Dialect)>,
    original: Option<Vec<u8>>,
    state: EntryState,
}

enum ParsedEntry {
    Absent,
    Value(Value),
    Refused(ConflictReason),
}

/// Who owns the named client temporary at the moment an error is handled.
///
/// This is the explicit cleanup authorization for the whole write: a by-name
/// unlink on an error path is authorized only while the retained pre-exchange
/// ownership is still proven. After any name exchange, every mismatch,
/// observation error, `?` error or fault path preserves both names and never
/// reaches the generic unlink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TemporaryOwnership {
    /// No name exchange has occurred: the named temporary is still exactly
    /// the file this transaction created, so unlinking it through the
    /// retained identity is authorized.
    PreExchange,
    /// A name exchange occurred (or may have occurred). No error path may
    /// unlink either name; every post-exchange branch decides preservation
    /// explicitly.
    PostExchange,
}

impl FileClientAdapter {
    /// An adapter for one of the four file-backed clients.
    ///
    /// # Errors
    /// [`AdapterError::InvalidSpec`] for a client without a file surface.
    pub fn new(
        client: ClientId,
        home: &ClientHome,
        ownership: OwnershipStore,
    ) -> Result<Self, AdapterError> {
        match client {
            ClientId::Codex | ClientId::OpenCode | ClientId::Copilot | ClientId::Cursor => {
                Ok(Self {
                    client,
                    home_root: home.root().to_path_buf(),
                    ownership,
                })
            }
            ClientId::ClaudeCode => Err(AdapterError::InvalidSpec),
        }
    }

    /// Every client this constructor accepts.
    pub const FILE_CLIENTS: [ClientId; 4] = [
        ClientId::Codex,
        ClientId::OpenCode,
        ClientId::Copilot,
        ClientId::Cursor,
    ];

    fn components(&self) -> &'static [&'static str] {
        match self.client {
            ClientId::Codex => &[".codex"],
            ClientId::OpenCode => &[".config", "opencode"],
            ClientId::Copilot => &[".copilot"],
            ClientId::Cursor => &[".cursor"],
            ClientId::ClaudeCode => &[],
        }
    }

    fn candidates(&self) -> &'static [(&'static str, Dialect)] {
        match self.client {
            ClientId::Codex => &[("config.toml", Dialect::Json)],
            ClientId::OpenCode => &[
                ("opencode.json", Dialect::Json),
                ("opencode.jsonc", Dialect::Jsonc),
            ],
            ClientId::Copilot => &[("mcp-config.json", Dialect::Json)],
            ClientId::Cursor => &[("mcp.json", Dialect::Json)],
            ClientId::ClaudeCode => &[],
        }
    }

    fn default_file(&self) -> (&'static str, Dialect) {
        match self.client {
            ClientId::OpenCode => ("opencode.json", Dialect::Json),
            _ => self.candidates()[0],
        }
    }

    fn entry_path(&self) -> &'static [&'static str] {
        match self.client {
            ClientId::Codex => &["mcp_servers", "kontor"],
            ClientId::OpenCode => &["mcp", "kontor"],
            ClientId::Copilot => &["mcpServers", "kontor"],
            ClientId::Cursor => &["mcpServers", "kontor"],
            ClientId::ClaudeCode => &[],
        }
    }

    fn logical_path(&self, file: &str) -> PathBuf {
        let mut path = self.home_root.clone();
        for component in self.components() {
            path.push(component);
        }
        path.push(file);
        path
    }

    /// Open the client directory, refusing every symlink component.
    fn client_directory(&self) -> Result<Option<Dir>, ConflictReason> {
        let mut current = match Dir::open_root(&self.home_root) {
            Ok(dir) => dir,
            Err(ConfinementError::Missing) => return Ok(None),
            Err(ConfinementError::Symlink) => return Err(ConflictReason::Symlink),
            Err(_) => return Err(ConflictReason::Unparsable),
        };
        for component in self.components() {
            match current.open_child(component) {
                Ok(dir) => current = dir,
                Err(ConfinementError::Missing) => return Ok(None),
                Err(ConfinementError::Symlink) => return Err(ConflictReason::Symlink),
                Err(_) => return Err(ConflictReason::Unparsable),
            }
        }
        Ok(Some(current))
    }

    /// Resolve exactly one configuration file, refusing ambiguity and symlinks.
    fn resolve_file(&self, dir: &Dir) -> Result<Option<(&'static str, Dialect)>, ConflictReason> {
        let mut found = Vec::new();
        for (name, dialect) in self.candidates() {
            match dir.kind_child(name) {
                Ok(Some(NodeKind::Symlink)) => return Err(ConflictReason::Symlink),
                Ok(Some(NodeKind::File)) => found.push((*name, *dialect)),
                Ok(Some(_)) => return Err(ConflictReason::Unparsable),
                Ok(None) => {}
                Err(_) => return Err(ConflictReason::Unparsable),
            }
        }
        match found.len() {
            0 => Ok(None),
            1 => Ok(Some(found[0])),
            _ => Err(ConflictReason::AmbiguousConfig),
        }
    }

    fn parse_document(
        &self,
        bytes: &[u8],
        dialect: Dialect,
    ) -> Result<ConfigDocument, ConflictReason> {
        match self.client {
            ClientId::Codex => {
                let text = std::str::from_utf8(bytes).map_err(|_| ConflictReason::Unparsable)?;
                Ok(ConfigDocument::Toml(
                    text.parse::<toml_edit::DocumentMut>()
                        .map_err(|_| ConflictReason::Unparsable)?,
                ))
            }
            _ => {
                let text = std::str::from_utf8(bytes).map_err(|_| ConflictReason::Unparsable)?;
                match JsonDocument::parse(text, dialect) {
                    Ok(document) => Ok(ConfigDocument::Json(document)),
                    Err(JsonEditError::TooLarge) => Err(ConflictReason::TooLarge),
                    Err(JsonEditError::TooDeep) => Err(ConflictReason::TooDeep),
                    Err(_) => Err(ConflictReason::Unparsable),
                }
            }
        }
    }

    fn parse_entry(&self, document: &ConfigDocument) -> ParsedEntry {
        match document {
            ConfigDocument::Toml(document) => match self.client {
                ClientId::Codex => parse_codex(document),
                _ => ParsedEntry::Refused(ConflictReason::Unparsable),
            },
            ConfigDocument::Json(document) => {
                if self.client == ClientId::OpenCode && document.has_member(&["mcpServers"]) {
                    return ParsedEntry::Refused(ConflictReason::LegacySchema);
                }
                let parents: &[&[&str]] = match self.client {
                    ClientId::OpenCode => &[&["mcp"]],
                    ClientId::Copilot => &[&["mcpServers"]],
                    ClientId::Cursor => &[&["mcpServers"]],
                    _ => &[],
                };
                for parent in parents {
                    if let Some(value) = document.value(parent)
                        && !value.is_object()
                    {
                        return ParsedEntry::Refused(ConflictReason::LegacySchema);
                    }
                }
                match document.lookup(self.entry_path()) {
                    EntryLookup::Absent => ParsedEntry::Absent,
                    EntryLookup::Value(value) => ParsedEntry::Value(value),
                    EntryLookup::Unrepresentable => {
                        ParsedEntry::Refused(ConflictReason::Unparsable)
                    }
                }
            }
        }
    }

    /// The full observation: open descriptors, raw bytes, full entry value and
    /// its classification. No effect of any kind.
    fn observe(&self, spec: &ServerSpec) -> Result<Observed, AdapterError> {
        if !crate::confinement::supported_target() {
            return Err(AdapterError::Confinement(
                ConfinementError::UnsupportedPlatform,
            ));
        }
        spec.validate()?;
        let directory = match self.client_directory() {
            Ok(Some(dir)) => dir,
            Ok(None) => {
                return Ok(Observed {
                    dir: None,
                    file: None,
                    original: None,
                    state: EntryState::ClientAbsent,
                });
            }
            Err(reason) => return Ok(refused(reason)),
        };
        let resolved = match self.resolve_file(&directory) {
            Ok(resolved) => resolved,
            Err(reason) => return Ok(refused(reason)),
        };
        let (file, dialect, original) = match resolved {
            Some((name, dialect)) => match directory.read_child(name, MAX_DOCUMENT_BYTES) {
                Ok(Some(bytes)) => (name, dialect, Some(bytes)),
                Ok(None) => (name, dialect, None),
                Err(ConfinementError::Symlink) => return Ok(refused(ConflictReason::Symlink)),
                Err(ConfinementError::TooLarge) => return Ok(refused(ConflictReason::TooLarge)),
                Err(_) => return Ok(refused(ConflictReason::Unparsable)),
            },
            None => {
                let (name, dialect) = self.default_file();
                (name, dialect, None)
            }
        };
        // A `PreserveBoth` leaves the displaced bytes in one bootstrap
        // temporary beside this file. Until the operator reconciles it, the
        // adapter keeps refusing instead of reporting the entry as current.
        if self.has_unreconciled_temporary(&directory, file)? {
            return Ok(refused(ConflictReason::UnreconciledTemporary));
        }

        let ledger = match self.ownership.load() {
            Ok(ledger) => ledger,
            Err(_) => {
                return Ok(Observed {
                    dir: Some(directory),
                    file: Some((file, dialect)),
                    original,
                    state: EntryState::Refused {
                        reason: ConflictReason::OwnershipUnreadable,
                    },
                });
            }
        };

        let Some(bytes) = &original else {
            return Ok(Observed {
                dir: Some(directory),
                file: Some((file, dialect)),
                original: None,
                state: EntryState::EntryAbsent,
            });
        };
        let document = match self.parse_document(bytes, dialect) {
            Ok(document) => document,
            Err(reason) => return Ok(refused(reason)),
        };
        let state = match self.parse_entry(&document) {
            ParsedEntry::Absent => EntryState::EntryAbsent,
            ParsedEntry::Refused(reason) => return Ok(refused(reason)),
            ParsedEntry::Value(value) => {
                let observed = ObservedHash::of_value(&value);
                if !schema_accepts(self.client, &value) {
                    EntryState::Unrelated { observed }
                } else {
                    let desired = desired_entry(self.client, spec);
                    super::classify(
                        &value,
                        &desired,
                        ledger.owns(self.client, &observed)
                            || ledger.pending_owns(self.client, &observed),
                    )
                }
            }
        };
        Ok(Observed {
            dir: Some(directory),
            file: Some((file, dialect)),
            original,
            state,
        })
    }

    /// Write the desired entry, bound to the exact expected observation.
    fn write(
        &self,
        spec: &ServerSpec,
        expected: &EntryState,
        faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError> {
        let observed = self.observe(spec)?;
        let Some(directory) = observed.dir.as_ref() else {
            return Ok(AdapterResult::Absent);
        };
        let Some((file, dialect)) = observed.file else {
            return Ok(AdapterResult::Absent);
        };
        if !same_snapshot(expected, &observed.state) {
            return Ok(AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                observed: observed.state.observed().cloned(),
            });
        }

        let edited = match self.render(&observed, spec, dialect) {
            Ok(edited) => edited,
            Err(reason) => {
                return Ok(AdapterResult::Conflict {
                    reason,
                    observed: observed.state.observed().cloned(),
                });
            }
        };

        let logical = self.logical_path(file);
        faults.hit(FailPoint::BeforeClientWrite(self.client), &logical)?;
        let temporary = format!(
            ".{file}.kontor-bootstrap-{}.tmp",
            crate::artifact::install::nonce()
        );
        let temporary_path = logical
            .parent()
            .map_or_else(|| logical.clone(), |parent| parent.join(&temporary));
        faults.hit(
            FailPoint::BeforeClientTempCreate(self.client),
            &temporary_path,
        )?;
        let final_mode = match observed.original {
            Some(_) => directory.child_mode(file)?.unwrap_or(0o600) & 0o777,
            None => 0o600,
        };
        // The file exists with its private mode before any content is
        // written. A name taken by foreign material is preserved: nothing of
        // ours exists yet, so there is nothing to clean up.
        let mut temp = match directory.create_child_empty(&temporary, 0o600) {
            Ok(temp) => temp,
            Err(ConfinementError::Exists) => {
                return Ok(AdapterResult::Conflict {
                    reason: ConflictReason::ConcurrentChange,
                    observed: observed.state.observed().cloned(),
                });
            }
            Err(error) => return Err(error.into()),
        };
        let temp_identity = Dir::file_identity(&temp)?;
        // The write is one transaction with an explicit cleanup disposition.
        // Until an exchange succeeds, the named temporary is our own
        // pre-exchange file and an error-path unlink by retained identity is
        // authorized. The moment an exchange succeeds the disposition turns
        // PostExchange and no error path may unlink either name.
        let mut ownership = TemporaryOwnership::PreExchange;
        let result = (|| -> Result<(), AdapterError> {
            faults.hit(FailPoint::AfterClientTempCreate(self.client), &logical)?;
            std::io::Write::write_all(&mut temp, edited.as_bytes())
                .map_err(|_| AdapterError::Io)?;
            temp.sync_all().map_err(|_| AdapterError::Io)?;
            Dir::set_file_mode(&temp, final_mode)?;

            faults.hit(FailPoint::AfterClientTempWrite(self.client), &logical)?;

            faults.hit(FailPoint::BeforeClientRename(self.client), &logical)?;
            // The final precondition is checked after the last injection point
            // and immediately before the effect.
            match &observed.original {
                None => {
                    if directory.kind_child(file)?.is_some() {
                        return Err(AdapterError::ConcurrentChange);
                    }
                }
                Some(original) => {
                    let current = match directory.read_child(file, MAX_DOCUMENT_BYTES)? {
                        Some(bytes) => bytes,
                        None => return Err(AdapterError::ConcurrentChange),
                    };
                    if &current != original {
                        return Err(AdapterError::ConcurrentChange);
                    }
                }
            }
            if directory.kind_child(file)? == Some(NodeKind::Symlink) {
                return Err(AdapterError::Confinement(ConfinementError::Symlink));
            }
            faults.hit(FailPoint::BeforeClientCommit(self.client), &logical)?;
            // Root clarification: the named temporary must still be the exact
            // regular file this transaction created, holding exactly the
            // rendered bytes, immediately before any promotion. An already
            // detected foreign temporary is never promoted.
            if directory.kind_child(&temporary)? != Some(NodeKind::File)
                || directory.child_identity(&temporary)? != Some(temp_identity)
            {
                return Err(AdapterError::ConcurrentChange);
            }
            match directory.read_child(&temporary, MAX_DOCUMENT_BYTES) {
                Ok(Some(bytes)) if bytes == edited.as_bytes() => {}
                _ => return Err(AdapterError::ConcurrentChange),
            }
            // The pending provenance is durable before the configuration
            // commits, so a replay can complete ownership if this process is
            // interrupted between the write and the ledger update.
            let desired = desired_entry(self.client, spec);
            let desired_hash = ObservedHash::of_value(&desired);
            self.ownership
                .begin(self.client, &desired_hash, &desired_hash)?;
            match &observed.original {
                None => {
                    // Creation is no-replace; the named temporary is
                    // pre-exchange and its cleanup uses the retained
                    // transaction-created identity.
                    directory.link_child(&temporary, directory, file)?;
                    let _ = directory.remove_child_if(&temporary, temp_identity);
                }
                Some(original) => {
                    // E: retain the target identity for displaced-side cleanup.
                    let original_identity = directory.child_identity(file)?;
                    faults.hit(FailPoint::BeforeClientExchange(self.client), &logical)?;
                    if let Err(error) = directory.exchange_children(&temporary, directory, file) {
                        // The exchange is atomic: on failure neither name
                        // moved and the temporary is still our pre-exchange
                        // file, so the pre-exchange disposition and its
                        // identity-bound cleanup stay authorized.
                        return Err(error.into());
                    }
                    ownership = TemporaryOwnership::PostExchange;
                    // Exchange-and-verify: after the swap, take the first
                    // identity+content snapshot of BOTH names.
                    let first_target = (
                        directory.child_identity(file),
                        directory.read_child(file, MAX_DOCUMENT_BYTES),
                    );
                    let first_displaced = (
                        directory.child_identity(&temporary),
                        directory.read_child(&temporary, MAX_DOCUMENT_BYTES),
                    );
                    faults.hit(FailPoint::AfterClientExchange(self.client), &logical)?;
                    let target_first_ok = matches!(
                        first_target,
                        (Ok(Some(identity)), Ok(Some(ref bytes)))
                            if identity == temp_identity && bytes == edited.as_bytes()
                    );
                    let displaced_first_original = matches!(
                        first_displaced,
                        (Ok(Some(identity)), Ok(Some(ref bytes)))
                            if Some(identity) == original_identity && bytes == original
                    );
                    // Fresh observation of both names, required before any
                    // commit or exchange-back.
                    let fresh_target = (
                        directory.child_identity(file),
                        directory.read_child(file, MAX_DOCUMENT_BYTES),
                    );
                    let fresh_displaced = (
                        directory.child_identity(&temporary),
                        directory.read_child(&temporary, MAX_DOCUMENT_BYTES),
                    );
                    let fresh_equal_first =
                        fresh_target == first_target && fresh_displaced == first_displaced;
                    if target_first_ok && displaced_first_original && fresh_equal_first {
                        // Commit: the displaced original is unlinked only
                        // through the retained pre-exchange identity.
                        if let Some(identity) = original_identity {
                            let _ = directory.remove_child_if(&temporary, identity);
                        }
                    } else {
                        // Explicit rollback eligibility requires fully
                        // observed, stable content on both sides: identity
                        // AND a successful content read equal to the first
                        // snapshots. Two equal errors are never proof of
                        // unchanged state, so unreadable content does not
                        // qualify. The target must be our inode with content
                        // that is not the rendered bytes (the rendered case
                        // is the commit branch).
                        let target_fully_observed = matches!(
                            first_target,
                            (Ok(Some(identity)), Ok(Some(_))) if identity == temp_identity
                        ) && matches!(
                            fresh_target,
                            (Ok(Some(identity)), Ok(Some(_))) if identity == temp_identity
                        );
                        let target_rendered = matches!(
                            first_target,
                            (Ok(Some(identity)), Ok(Some(ref bytes)))
                                if identity == temp_identity && bytes == edited.as_bytes()
                        );
                        let rollback_safe = target_fully_observed
                            && !target_rendered
                            && displaced_first_original
                            && fresh_equal_first;
                        if rollback_safe {
                            if directory
                                .exchange_children(&temporary, directory, file)
                                .is_err()
                            {
                                // A failed exchange-back leaves any state
                                // uncertain: preserve both names.
                                return Err(AdapterError::PreserveBoth);
                            }
                            let restored_target = (
                                directory.child_identity(file),
                                directory.read_child(file, MAX_DOCUMENT_BYTES),
                            );
                            let restored_temp = (
                                directory.child_identity(&temporary),
                                directory.read_child(&temporary, MAX_DOCUMENT_BYTES),
                            );
                            let target_restored = matches!(
                                restored_target,
                                (Ok(Some(identity)), Ok(Some(ref bytes)))
                                    if Some(identity) == original_identity && bytes == original
                            );
                            // The retained snapshot appropriate to the
                            // temporary after an exchange-back is its first
                            // post-exchange observation (identity and complete
                            // observed content), not the rendered document:
                            // rollback is eligible only when the target was
                            // not the rendered bytes, and the exchange
                            // restores exactly the pre-exchange states. A
                            // temporary that equals that retained snapshot is
                            // our own proved-owned file; anything else
                            // preserves both names.
                            let temp_restored = restored_temp == first_target;
                            faults.hit(FailPoint::AfterClientRollback(self.client), &logical)?;
                            if target_restored && temp_restored {
                                // Proved-owned restored-state cleanup: unlink
                                // the transaction-created temporary by its
                                // retained identity.
                                let _ = directory.remove_child_if(&temporary, temp_identity);
                                return Err(AdapterError::ConcurrentChange);
                            }
                            // A failed restored-state verification is
                            // uncertainty: both names are preserved.
                            return Err(AdapterError::PreserveBoth);
                        }
                        // Any other post-exchange mismatch, observation
                        // error or uncertainty preserves both names.
                        return Err(AdapterError::PreserveBoth);
                    }
                }
            }
            Ok(())
        })();
        if result.is_err() && ownership == TemporaryOwnership::PreExchange {
            // Pre-exchange cleanup is a stat-then-unlink observation bound to
            // the inode this transaction created; a foreign replacement of
            // the name has a different identity and is preserved. This branch
            // is never entered after a successful exchange. An active
            // uncooperative same-UID writer can still replace the name
            // between the observation and the unlink; that window is the
            // accepted residual, not conditional atomicity.
            let _ = directory.remove_child_if(&temporary, temp_identity);
        }
        match result {
            Ok(()) => {}
            Err(AdapterError::ConcurrentChange) => {
                return Ok(AdapterResult::Conflict {
                    reason: ConflictReason::ConcurrentChange,
                    observed: observed.state.observed().cloned(),
                });
            }
            Err(AdapterError::Confinement(ConfinementError::Symlink)) => {
                return Ok(AdapterResult::Conflict {
                    reason: ConflictReason::Symlink,
                    observed: None,
                });
            }
            Err(AdapterError::Confinement(ConfinementError::Exists)) => {
                return Ok(AdapterResult::Conflict {
                    reason: ConflictReason::ConcurrentChange,
                    observed: observed.state.observed().cloned(),
                });
            }
            Err(error) => return Err(error),
        }
        faults.hit(FailPoint::AfterClientWrite(self.client), &logical)?;

        let readback = self.observe(spec)?;
        match readback.state {
            EntryState::Current { observed: hash } => {
                let desired = desired_entry(self.client, spec);
                self.ownership
                    .record(self.client, &hash, &ObservedHash::of_value(&desired))?;
                Ok(AdapterResult::Updated)
            }
            _ => Ok(AdapterResult::FailedReadback),
        }
    }

    /// Whether an earlier `PreserveBoth` left its displaced bytes in one of
    /// this configuration file's bootstrap temporaries.
    fn has_unreconciled_temporary(
        &self,
        directory: &Dir,
        file: &str,
    ) -> Result<bool, AdapterError> {
        let prefix = format!(".{file}.kontor-bootstrap-");
        Ok(directory
            .entries()?
            .iter()
            .any(|name| name.starts_with(&prefix) && name.ends_with(".tmp")))
    }

    /// Complete the ownership record for a write a replay verified.
    ///
    /// A pending record is durable before the configuration commits. When the
    /// observed entry is exactly that pending snapshot, the write did land and
    /// only the post-write ledger update was lost, so complete it now.
    fn adopt_pending(
        &self,
        spec: &ServerSpec,
        observed: &ObservedHash,
    ) -> Result<(), AdapterError> {
        let ledger = self.ownership.load()?;
        if ledger.owns(self.client, observed) || !ledger.pending_owns(self.client, observed) {
            return Ok(());
        }
        let desired = desired_entry(self.client, spec);
        self.ownership
            .record(self.client, observed, &ObservedHash::of_value(&desired))
    }

    /// Render the complete new file text for one desired entry.
    fn render(
        &self,
        observed: &Observed,
        spec: &ServerSpec,
        dialect: Dialect,
    ) -> Result<String, ConflictReason> {
        let desired = desired_entry(self.client, spec);
        match self.client {
            ClientId::Codex => {
                let mut document = match &observed.original {
                    Some(bytes) => std::str::from_utf8(bytes)
                        .map_err(|_| ConflictReason::Unparsable)?
                        .parse::<toml_edit::DocumentMut>()
                        .map_err(|_| ConflictReason::Unparsable)?,
                    None => toml_edit::DocumentMut::new(),
                };
                apply_codex(&mut document, spec)?;
                Ok(document.to_string())
            }
            _ => {
                let text = match &observed.original {
                    Some(bytes) => std::str::from_utf8(bytes)
                        .map_err(|_| ConflictReason::Unparsable)?
                        .to_owned(),
                    None => "{}".to_owned(),
                };
                let mut document =
                    JsonDocument::parse(text, dialect).map_err(|error| match error {
                        JsonEditError::TooLarge => ConflictReason::TooLarge,
                        JsonEditError::TooDeep => ConflictReason::TooDeep,
                        _ => ConflictReason::Unparsable,
                    })?;
                let serialized =
                    serde_json::to_string(&desired).map_err(|_| ConflictReason::Unparsable)?;
                ensure_parents(&mut document, self.entry_path())?;
                if document.has_member(self.entry_path()) {
                    document
                        .replace_value(self.entry_path(), &serialized)
                        .map_err(|_| ConflictReason::Unparsable)?;
                } else {
                    let (parent, leaf) = self.entry_path().split_at(self.entry_path().len() - 1);
                    document
                        .insert_member(parent, leaf[0], &serialized)
                        .map_err(|_| ConflictReason::Unparsable)?;
                }
                Ok(document.text().to_owned())
            }
        }
    }
}

fn refused(reason: ConflictReason) -> Observed {
    Observed {
        dir: None,
        file: None,
        original: None,
        state: EntryState::Refused { reason },
    }
}

fn same_snapshot(expected: &EntryState, fresh: &EntryState) -> bool {
    match (expected, fresh) {
        (EntryState::EntryAbsent, EntryState::EntryAbsent) => true,
        (EntryState::ClientAbsent, EntryState::ClientAbsent) => true,
        (EntryState::Current { observed: a }, EntryState::Current { observed: b })
        | (EntryState::ManagedStale { observed: a }, EntryState::ManagedStale { observed: b })
        | (EntryState::Unrelated { observed: a }, EntryState::Unrelated { observed: b })
        | (EntryState::Unrelated { observed: a }, EntryState::ManagedStale { observed: b })
        | (EntryState::ManagedStale { observed: a }, EntryState::Unrelated { observed: b })
        | (EntryState::Current { observed: a }, EntryState::ManagedStale { observed: b })
        | (EntryState::Current { observed: a }, EntryState::Unrelated { observed: b })
        | (EntryState::ManagedStale { observed: a }, EntryState::Current { observed: b })
        | (EntryState::Unrelated { observed: a }, EntryState::Current { observed: b }) => a == b,
        _ => false,
    }
}

impl ClientAdapter for FileClientAdapter {
    fn client(&self) -> ClientId {
        self.client
    }

    fn inspect(&mut self, spec: &ServerSpec) -> Result<EntryState, AdapterError> {
        Ok(self.observe(spec)?.state)
    }

    fn install(
        &mut self,
        spec: &ServerSpec,
        faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError> {
        let state = self.observe(spec)?.state;
        match &state {
            EntryState::ClientAbsent => Ok(AdapterResult::Absent),
            EntryState::Unsupported => Ok(AdapterResult::Unsupported),
            EntryState::EntryAbsent => {
                self.write(spec, &state, faults).map(|result| match result {
                    AdapterResult::Updated => AdapterResult::Installed,
                    other => other,
                })
            }
            EntryState::Current { observed } => {
                self.adopt_pending(spec, observed)?;
                Ok(AdapterResult::AlreadyCurrent)
            }
            EntryState::ManagedStale { observed: _ } => self.write(spec, &state, faults),
            EntryState::Unrelated { observed } => Ok(AdapterResult::Conflict {
                reason: ConflictReason::SameNameUnrelated,
                observed: Some(observed.clone()),
            }),
            EntryState::Refused { reason } => Ok(AdapterResult::Conflict {
                reason: *reason,
                observed: None,
            }),
        }
    }

    fn repair(
        &mut self,
        spec: &ServerSpec,
        expected: &ObservedHash,
        faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError> {
        let state = self.observe(spec)?.state;
        match &state {
            EntryState::ClientAbsent | EntryState::EntryAbsent => Ok(AdapterResult::Absent),
            EntryState::Unsupported => Ok(AdapterResult::Unsupported),
            EntryState::Current { observed } => {
                self.adopt_pending(spec, observed)?;
                Ok(AdapterResult::AlreadyCurrent)
            }
            EntryState::ManagedStale { observed } | EntryState::Unrelated { observed } => {
                if observed != expected {
                    return Ok(AdapterResult::Conflict {
                        reason: ConflictReason::ObservedHashMismatch,
                        observed: Some(observed.clone()),
                    });
                }
                self.write(spec, &state, faults).map(|result| match result {
                    AdapterResult::Updated => AdapterResult::Repaired,
                    other => other,
                })
            }
            EntryState::Refused { reason } => Ok(AdapterResult::Conflict {
                reason: *reason,
                observed: None,
            }),
        }
    }
}

/// A parsed configuration document in its own format.
enum ConfigDocument {
    Toml(toml_edit::DocumentMut),
    Json(JsonDocument),
}

fn parse_codex(document: &toml_edit::DocumentMut) -> ParsedEntry {
    let servers = match document.get("mcp_servers") {
        None => return ParsedEntry::Absent,
        Some(item) => match item.as_table_like() {
            Some(_) => item,
            None => return ParsedEntry::Refused(ConflictReason::LegacySchema),
        },
    };
    let entry = match servers.get("kontor") {
        None => return ParsedEntry::Absent,
        Some(item) => item,
    };
    match toml_item_to_value(entry) {
        Some(value) => ParsedEntry::Value(value),
        None => ParsedEntry::Refused(ConflictReason::Unparsable),
    }
}

fn toml_item_to_value(item: &toml_edit::Item) -> Option<Value> {
    match item {
        toml_edit::Item::Value(value) => toml_value_to_value(value),
        toml_edit::Item::Table(table) => toml_table_to_value(table),
        toml_edit::Item::ArrayOfTables(array) => {
            let mut values = Vec::new();
            for table in array.iter() {
                values.push(toml_table_to_value(table)?);
            }
            Some(Value::Array(values))
        }
        toml_edit::Item::None => None,
    }
}

fn toml_table_to_value(table: &dyn toml_edit::TableLike) -> Option<Value> {
    let mut object = serde_json::Map::new();
    for (key, item) in table.iter() {
        object.insert(key.to_owned(), toml_item_to_value(item)?);
    }
    Some(Value::Object(object))
}

fn toml_value_to_value(value: &toml_edit::Value) -> Option<Value> {
    match value {
        toml_edit::Value::String(text) => Some(Value::String(text.value().clone())),
        toml_edit::Value::Integer(number) => Some(Value::Number((*number.value()).into())),
        toml_edit::Value::Float(number) => {
            serde_json::Number::from_f64(*number.value()).map(Value::Number)
        }
        toml_edit::Value::Boolean(boolean) => Some(Value::Bool(*boolean.value())),
        toml_edit::Value::Datetime(datetime) => Some(Value::String(datetime.to_string())),
        toml_edit::Value::Array(array) => {
            let mut values = Vec::new();
            for element in array.iter() {
                values.push(toml_value_to_value(element)?);
            }
            Some(Value::Array(values))
        }
        toml_edit::Value::InlineTable(table) => {
            let mut object = serde_json::Map::new();
            for (key, element) in table.iter() {
                object.insert(key.to_owned(), toml_value_to_value(element)?);
            }
            Some(Value::Object(object))
        }
    }
}

/// Strict per-client schema validation. A value that fails here is never
/// current and never authorizes a write from the entry itself.
fn schema_accepts(client: ClientId, value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    let string_field = |key: &str| object.get(key).and_then(Value::as_str);
    let string_array = |key: &str| -> Option<Vec<&str>> {
        let array = object.get(key)?.as_array()?;
        array.iter().map(Value::as_str).collect()
    };
    match client {
        ClientId::Codex | ClientId::Copilot => {
            string_field("command").is_some() && string_array("args").is_some()
        }
        ClientId::OpenCode => {
            string_field("type") == Some("local")
                && string_array("command").is_some_and(|command| !command.is_empty())
        }
        ClientId::Cursor => {
            object
                .get("type")
                .is_none_or(|kind| kind.as_str() == Some("stdio"))
                && string_field("command").is_some()
                && object.get("args").is_none_or(|args| {
                    args.as_array()
                        .is_some_and(|a| a.iter().all(Value::is_string))
                })
        }
        ClientId::ClaudeCode => {
            string_field("type") == Some("stdio")
                && string_field("command").is_some()
                && string_array("args").is_some()
        }
    }
}

fn apply_codex(
    document: &mut toml_edit::DocumentMut,
    spec: &ServerSpec,
) -> Result<(), ConflictReason> {
    if document.get("mcp_servers").is_none() {
        document["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let servers = document
        .get_mut("mcp_servers")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or(ConflictReason::LegacySchema)?;
    let mut entry = toml_edit::Table::new();
    entry.insert(
        "command",
        toml_edit::value(spec.program.to_string_lossy().into_owned()),
    );
    let mut args = toml_edit::Array::new();
    for argument in &spec.args {
        args.push(argument.as_str());
    }
    entry.insert("args", toml_edit::value(args));
    servers.insert("kontor", toml_edit::Item::Table(entry));
    Ok(())
}

fn ensure_parents(document: &mut JsonDocument, path: &[&str]) -> Result<(), ConflictReason> {
    for depth in 0..path.len().saturating_sub(1) {
        let target = &path[..=depth];
        if document.has_member(target) {
            continue;
        }
        document
            .insert_member(&path[..depth], path[depth], "{}")
            .map_err(|_| ConflictReason::Unparsable)?;
    }
    Ok(())
}
