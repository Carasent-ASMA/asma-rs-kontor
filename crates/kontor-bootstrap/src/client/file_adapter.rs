//! The four file-backed client adapters.
//!
//! Codex, OpenCode, VS Code/Copilot and Cursor each own one user-scoped
//! configuration file in a documented format. The adapter patches exactly the
//! `kontor` entry, leaves every other byte alone, refuses a same-name entry it
//! did not write, refuses a symlinked path, and compares the file immediately
//! before replacing it so a concurrent writer is never overwritten.
//!
//! The shapes are the KON-OP-08 table's: `[mcp_servers.kontor]` for Codex,
//! `mcp.servers.kontor` with a local command array for OpenCode,
//! `servers.kontor` stdio for Copilot, and `mcpServers.kontor` for Cursor.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use super::json_edit::{Dialect, JsonDocument};
use super::{
    AdapterError, AdapterResult, ClientAdapter, ClientId, ConflictReason, EntryState,
    MCP_EXECUTABLE, ObservedHash, ServerSpec,
};
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
    home: PathBuf,
}

impl FileClientAdapter {
    /// An adapter for one of the four file-backed clients.
    ///
    /// # Errors
    /// [`AdapterError::InvalidSpec`] for a client without a file surface.
    pub fn new(client: ClientId, home: &ClientHome) -> Result<Self, AdapterError> {
        match client {
            ClientId::Codex | ClientId::OpenCode | ClientId::Copilot | ClientId::Cursor => {
                Ok(Self {
                    client,
                    home: home.root().to_path_buf(),
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

    fn directory(&self) -> PathBuf {
        match self.client {
            ClientId::Codex => self.home.join(".codex"),
            ClientId::OpenCode => self.home.join(".config").join("opencode"),
            ClientId::Copilot => self.home.join(".copilot"),
            ClientId::Cursor => self.home.join(".cursor"),
            ClientId::ClaudeCode => unreachable!("no file surface for Claude Code"),
        }
    }

    /// The configuration file candidates in resolution order, each with its
    /// dialect.
    fn candidates(&self) -> Vec<(PathBuf, Dialect)> {
        let directory = self.directory();
        match self.client {
            ClientId::Codex => vec![(directory.join("config.toml"), Dialect::Json)],
            ClientId::OpenCode => vec![
                (directory.join("opencode.json"), Dialect::Json),
                (directory.join("opencode.jsonc"), Dialect::Jsonc),
            ],
            ClientId::Copilot => vec![(directory.join("mcp-config.json"), Dialect::Json)],
            ClientId::Cursor => vec![(directory.join("mcp.json"), Dialect::Json)],
            ClientId::ClaudeCode => Vec::new(),
        }
    }

    fn default_file(&self) -> PathBuf {
        match self.client {
            ClientId::OpenCode => self.directory().join("opencode.json"),
            _ => {
                self.candidates()
                    .into_iter()
                    .next()
                    .expect("a file surface")
                    .0
            }
        }
    }

    /// Refuse a symlink at the client directory or any component beneath the
    /// injected home, and report whether the directory exists.
    fn probe_directory(&self) -> Result<bool, ConflictReason> {
        let directory = self.directory();
        let relative = directory
            .strip_prefix(&self.home)
            .expect("adapter directory lives under the injected home");
        let mut prefix = self.home.clone();
        for component in relative.components() {
            prefix.push(component);
            match std::fs::symlink_metadata(&prefix) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(ConflictReason::Symlink);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(_) => return Ok(false),
            }
        }
        Ok(true)
    }

    /// Resolve exactly one existing configuration file, refusing ambiguity.
    fn resolve_existing(&self) -> Result<Option<(PathBuf, Dialect)>, ConflictReason> {
        if !self.probe_directory()? {
            return Ok(None);
        }
        let existing: Vec<(PathBuf, Dialect)> = self
            .candidates()
            .into_iter()
            .filter(|(path, _)| std::fs::symlink_metadata(path).is_ok())
            .collect();
        match existing.len() {
            0 => Ok(None),
            1 => {
                let (path, dialect) = existing.into_iter().next().expect("one");
                let metadata =
                    std::fs::symlink_metadata(&path).map_err(|_| ConflictReason::Unparsable)?;
                if metadata.file_type().is_symlink() {
                    return Err(ConflictReason::Symlink);
                }
                Ok(Some((path, dialect)))
            }
            _ => Err(ConflictReason::AmbiguousConfig),
        }
    }

    fn read_document(&self) -> Result<Option<ExistingDocument>, ConflictReason> {
        let Some((path, dialect)) = self.resolve_existing()? else {
            return Ok(None);
        };
        let bytes = std::fs::read(&path).map_err(|_| ConflictReason::Unparsable)?;
        let text = std::str::from_utf8(&bytes).map_err(|_| ConflictReason::Unparsable)?;
        let document = match self.client {
            ClientId::Codex => ConfigDocument::Toml(
                text.parse::<toml_edit::DocumentMut>()
                    .map_err(|_| ConflictReason::Unparsable)?,
            ),
            _ => ConfigDocument::Json(
                JsonDocument::parse(text, dialect).map_err(|_| ConflictReason::Unparsable)?,
            ),
        };
        Ok(Some(ExistingDocument {
            path,
            bytes,
            document,
        }))
    }

    fn observe(&self, document: &ConfigDocument, spec: &ServerSpec) -> EntryState {
        match self.parse_entry(document) {
            EntryObservation::Absent => EntryState::EntryAbsent,
            EntryObservation::Refused(reason) => EntryState::Refused { reason },
            EntryObservation::Entry { program, args, raw } => {
                let observed = match (&program, raw) {
                    (Some(program), _) => ObservedHash::of_entry(program, &args),
                    (None, Some(raw)) => ObservedHash::of_bytes(raw.as_bytes()),
                    (None, None) => ObservedHash::of_bytes(&[]),
                };
                match program {
                    Some(program) if basename(&program) == MCP_EXECUTABLE => {
                        if program.as_str() == spec.program.to_string_lossy().as_ref()
                            && args == spec.args
                        {
                            EntryState::Current { observed }
                        } else {
                            EntryState::ManagedStale { observed }
                        }
                    }
                    _ => EntryState::Unrelated { observed },
                }
            }
        }
    }

    fn parse_entry(&self, document: &ConfigDocument) -> EntryObservation {
        if self.client == ClientId::Codex {
            return parse_codex_toml(document);
        }
        let ConfigDocument::Json(document) = document else {
            return EntryObservation::Absent;
        };
        let (path, parents): (&[&str], &[&[&str]]) = match self.client {
            ClientId::OpenCode => (
                &["mcp", "servers", "kontor"],
                &[&["mcp"], &["mcp", "servers"], &["mcpServers"]],
            ),
            ClientId::Copilot => (&["servers", "kontor"], &[&["servers"], &["mcpServers"]]),
            ClientId::Cursor => (&["mcpServers", "kontor"], &[&["mcpServers"], &["servers"]]),
            ClientId::Codex | ClientId::ClaudeCode => return EntryObservation::Absent,
        };
        // A container the documented schema does not allow at all is refused
        // rather than interpreted as "no entry yet".
        for parent in parents {
            if let Some(value) = document.value(parent)
                && !value.is_object()
            {
                return EntryObservation::Refused(ConflictReason::LegacySchema);
            }
        }
        if self.client == ClientId::OpenCode && document.has_member(&["mcpServers"]) {
            return EntryObservation::Refused(ConflictReason::LegacySchema);
        }
        match document.value(path) {
            Some(value) => parse_json_entry(&value),
            None => EntryObservation::Absent,
        }
    }

    /// Whether a JSON parent container exists with the wrong shape.
    fn parent_shape_conflict(&self, document: &ConfigDocument) -> Option<ConflictReason> {
        let ConfigDocument::Json(document) = document else {
            return None;
        };
        match self.client {
            ClientId::Codex => None,
            ClientId::OpenCode => {
                if document.has_member(&["mcpServers"]) {
                    return Some(ConflictReason::LegacySchema);
                }
                document.value(&["mcp"]).and_then(|mcp| {
                    if mcp.is_object() {
                        document.value(&["mcp", "servers"]).and_then(|servers| {
                            (!servers.is_object()).then_some(ConflictReason::LegacySchema)
                        })
                    } else {
                        Some(ConflictReason::LegacySchema)
                    }
                })
            }
            ClientId::Copilot => document
                .value(&["servers"])
                .and_then(|servers| (!servers.is_object()).then_some(ConflictReason::LegacySchema)),
            ClientId::Cursor => document
                .value(&["mcpServers"])
                .and_then(|servers| (!servers.is_object()).then_some(ConflictReason::LegacySchema)),
            ClientId::ClaudeCode => None,
        }
    }

    fn empty_document(&self) -> Result<ConfigDocument, ConflictReason> {
        match self.client {
            ClientId::Codex => Ok(ConfigDocument::Toml(toml_edit::DocumentMut::new())),
            _ => Ok(ConfigDocument::Json(
                JsonDocument::parse("{}", Dialect::Json).map_err(|_| ConflictReason::Unparsable)?,
            )),
        }
    }

    /// Apply the desired entry, returning the full new file text.
    fn edit(&self, spec: &ServerSpec) -> Result<EditedFile, ConflictReason> {
        let (path, original, mut document) = match self.read_document()? {
            Some(existing) => (Some(existing.path), Some(existing.bytes), existing.document),
            None => (None, None, self.empty_document()?),
        };
        if let Some(reason) = self.parent_shape_conflict(&document) {
            return Err(reason);
        }
        match &mut document {
            ConfigDocument::Toml(document) => {
                apply_codex(document, spec)?;
            }
            ConfigDocument::Json(document) => match self.client {
                ClientId::OpenCode => {
                    apply_json(document, &["mcp", "servers", "kontor"], |document| {
                        ensure_object(document, &["mcp", "servers"])?;
                        Ok(json!({"type": "local", "command": command_array(spec)}))
                    })?;
                }
                ClientId::Copilot => {
                    apply_json(document, &["servers", "kontor"], |document| {
                        ensure_object(document, &["servers"])?;
                        Ok(json!({
                            "type": "stdio",
                            "command": spec.program.to_string_lossy(),
                            "args": spec.args,
                        }))
                    })?;
                }
                ClientId::Cursor => {
                    apply_json(document, &["mcpServers", "kontor"], |document| {
                        ensure_object(document, &["mcpServers"])?;
                        Ok(json!({
                            "command": spec.program.to_string_lossy(),
                            "args": spec.args,
                        }))
                    })?;
                }
                ClientId::Codex | ClientId::ClaudeCode => {
                    return Err(ConflictReason::Unparsable);
                }
            },
        }
        Ok(EditedFile {
            path: path.unwrap_or_else(|| self.default_file()),
            original,
            edited: document.to_text(),
        })
    }

    /// Replace the configuration file after proving it has not moved.
    fn commit(
        &self,
        edited: &EditedFile,
        faults: &mut dyn FaultInjector,
    ) -> Result<(), AdapterError> {
        faults.hit(FailPoint::BeforeClientWrite(self.client), &edited.path)?;
        if let Some(original) = &edited.original {
            let current = std::fs::read(&edited.path).map_err(|_| AdapterError::Io)?;
            if &current != original {
                return Err(AdapterError::ConcurrentChange);
            }
        }
        let parent = edited.path.parent().ok_or(AdapterError::Io)?;
        let file_name = edited
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(AdapterError::Io)?;
        let temporary = parent.join(format!(".{file_name}.kontor-bootstrap-{}.tmp", nonce()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| AdapterError::Io)?;
            std::io::Write::write_all(&mut file, edited.edited.as_bytes())
                .map_err(|_| AdapterError::Io)?;
            file.sync_all().map_err(|_| AdapterError::Io)?;
            if let Some(original) = &edited.original {
                let current = std::fs::read(&edited.path).map_err(|_| AdapterError::Io)?;
                if &current != original {
                    return Err(AdapterError::ConcurrentChange);
                }
                if let Ok(metadata) = std::fs::metadata(&edited.path) {
                    let _ = std::fs::set_permissions(&temporary, metadata.permissions());
                }
            }
            std::fs::rename(&temporary, &edited.path)
                .or_else(|_| {
                    std::fs::remove_file(&edited.path)?;
                    std::fs::rename(&temporary, &edited.path)
                })
                .map_err(|_| AdapterError::Io)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result?;
        faults.hit(FailPoint::AfterClientWrite(self.client), &edited.path)?;
        Ok(())
    }

    fn write(
        &mut self,
        spec: &ServerSpec,
        faults: &mut dyn FaultInjector,
    ) -> Result<Result<AdapterResult, ConflictReason>, AdapterError> {
        let edited = match self.edit(spec) {
            Ok(edited) => edited,
            Err(reason) => return Ok(Err(reason)),
        };
        match self.commit(&edited, faults) {
            Ok(()) => {}
            Err(AdapterError::ConcurrentChange) => {
                return Ok(Err(ConflictReason::ConcurrentChange));
            }
            Err(other) => return Err(other),
        }
        match self.inspect(spec)? {
            EntryState::Current { .. } => Ok(Ok(AdapterResult::Updated)),
            _ => Ok(Ok(AdapterResult::FailedReadback)),
        }
    }
}

impl ClientAdapter for FileClientAdapter {
    fn client(&self) -> ClientId {
        self.client
    }

    fn inspect(&mut self, spec: &ServerSpec) -> Result<EntryState, AdapterError> {
        if !spec.points_at_managed_executable() {
            return Err(AdapterError::InvalidSpec);
        }
        let document = match self.read_document() {
            Ok(Some(existing)) => existing.document,
            Ok(None) => {
                return match self.probe_directory() {
                    Ok(true) => Ok(EntryState::EntryAbsent),
                    Ok(false) => Ok(EntryState::ClientAbsent),
                    Err(reason) => Ok(EntryState::Refused { reason }),
                };
            }
            Err(reason) => return Ok(EntryState::Refused { reason }),
        };
        Ok(self.observe(&document, spec))
    }

    fn install(
        &mut self,
        spec: &ServerSpec,
        faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError> {
        if !spec.points_at_managed_executable() {
            return Ok(AdapterResult::Conflict {
                reason: ConflictReason::SameNameUnrelated,
                observed: None,
            });
        }
        match self.inspect(spec)? {
            EntryState::ClientAbsent => Ok(AdapterResult::Absent),
            EntryState::EntryAbsent => match self.write(spec, faults)? {
                Ok(AdapterResult::Updated) => Ok(AdapterResult::Installed),
                Ok(other) => Ok(other),
                Err(reason) => Ok(AdapterResult::Conflict {
                    reason,
                    observed: None,
                }),
            },
            EntryState::Current { .. } => Ok(AdapterResult::AlreadyCurrent),
            EntryState::ManagedStale { observed } => match self.write(spec, faults)? {
                Ok(result) => Ok(result),
                Err(reason) => Ok(AdapterResult::Conflict {
                    reason,
                    observed: Some(observed),
                }),
            },
            EntryState::Unrelated { observed } => Ok(AdapterResult::Conflict {
                reason: ConflictReason::SameNameUnrelated,
                observed: Some(observed),
            }),
            EntryState::Refused { reason } => Ok(AdapterResult::Conflict {
                reason,
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
        if !spec.points_at_managed_executable() {
            return Ok(AdapterResult::Conflict {
                reason: ConflictReason::SameNameUnrelated,
                observed: None,
            });
        }
        match self.inspect(spec)? {
            EntryState::ClientAbsent | EntryState::EntryAbsent => Ok(AdapterResult::Absent),
            EntryState::Current { .. } => Ok(AdapterResult::AlreadyCurrent),
            EntryState::ManagedStale { observed } | EntryState::Unrelated { observed } => {
                if observed != *expected {
                    return Ok(AdapterResult::Conflict {
                        reason: ConflictReason::ObservedHashMismatch,
                        observed: Some(observed),
                    });
                }
                match self.write(spec, faults)? {
                    Ok(result) => Ok(match result {
                        AdapterResult::Updated => AdapterResult::Repaired,
                        other => other,
                    }),
                    Err(reason) => Ok(AdapterResult::Conflict {
                        reason,
                        observed: Some(observed),
                    }),
                }
            }
            EntryState::Refused { reason } => Ok(AdapterResult::Conflict {
                reason,
                observed: None,
            }),
        }
    }
}

/// One existing configuration file with its parsed document.
struct ExistingDocument {
    path: PathBuf,
    bytes: Vec<u8>,
    document: ConfigDocument,
}

/// A parsed configuration document in its own format.
enum ConfigDocument {
    Toml(toml_edit::DocumentMut),
    Json(JsonDocument),
}

impl ConfigDocument {
    fn to_text(&self) -> String {
        match self {
            ConfigDocument::Toml(document) => document.to_string(),
            ConfigDocument::Json(document) => document.text().to_owned(),
        }
    }
}

/// One entry as observed, before classification against a desired spec.
enum EntryObservation {
    Absent,
    Refused(ConflictReason),
    Entry {
        program: Option<String>,
        args: Vec<String>,
        raw: Option<String>,
    },
}

struct EditedFile {
    path: PathBuf,
    original: Option<Vec<u8>>,
    edited: String,
}

fn basename(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or(program)
}

fn command_array(spec: &ServerSpec) -> Vec<String> {
    let mut command = vec![spec.program.to_string_lossy().into_owned()];
    command.extend(spec.args.iter().cloned());
    command
}

fn parse_codex_toml(document: &ConfigDocument) -> EntryObservation {
    let ConfigDocument::Toml(document) = document else {
        return EntryObservation::Absent;
    };
    let servers = match document.get("mcp_servers") {
        None => return EntryObservation::Absent,
        Some(item) => match item.as_table_like() {
            Some(_) => item,
            None => return EntryObservation::Refused(ConflictReason::LegacySchema),
        },
    };
    let entry = match servers.get("kontor") {
        None => return EntryObservation::Absent,
        Some(item) => item,
    };
    let Some(table) = entry.as_table_like() else {
        return EntryObservation::Entry {
            program: None,
            args: Vec::new(),
            raw: Some(entry.to_string()),
        };
    };
    let Some(command) = table.get("command").and_then(toml_edit::Item::as_str) else {
        return EntryObservation::Entry {
            program: None,
            args: Vec::new(),
            raw: Some(entry.to_string()),
        };
    };
    let args = table
        .get("args")
        .and_then(toml_edit::Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    EntryObservation::Entry {
        program: Some(command.to_owned()),
        args,
        raw: None,
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
    if servers.get("kontor").is_none() {
        servers.insert("kontor", toml_edit::Item::Table(toml_edit::Table::new()));
    }
    let entry = servers
        .get_mut("kontor")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or(ConflictReason::LegacySchema)?;
    entry.insert(
        "command",
        toml_edit::value(spec.program.to_string_lossy().into_owned()),
    );
    let mut args = toml_edit::Array::new();
    for argument in &spec.args {
        args.push(argument.as_str());
    }
    entry.insert("args", toml_edit::value(args));
    Ok(())
}

fn parse_json_entry(value: &Value) -> EntryObservation {
    let Some(object) = value.as_object() else {
        return EntryObservation::Entry {
            program: None,
            args: Vec::new(),
            raw: Some(value.to_string()),
        };
    };
    let raw = || Some(value.to_string());
    if let Some(command) = object.get("command").and_then(Value::as_array) {
        let mut strings = command.iter().map(Value::as_str);
        let program = strings.next().flatten().map(str::to_owned);
        let args: Option<Vec<String>> = strings.map(|value| value.map(str::to_owned)).collect();
        let (Some(program), Some(args)) = (program, args) else {
            return EntryObservation::Entry {
                program: None,
                args: Vec::new(),
                raw: raw(),
            };
        };
        return EntryObservation::Entry {
            program: Some(program),
            args,
            raw: None,
        };
    }
    if let Some(program) = object.get("command").and_then(Value::as_str) {
        let args = object
            .get("args")
            .and_then(Value::as_array)
            .map(|array| {
                array
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return EntryObservation::Entry {
            program: Some(program.to_owned()),
            args,
            raw: None,
        };
    }
    EntryObservation::Entry {
        program: None,
        args: Vec::new(),
        raw: raw(),
    }
}

fn ensure_object(document: &mut JsonDocument, path: &[&str]) -> Result<(), ConflictReason> {
    if document.has_member(path) {
        return Ok(());
    }
    for depth in 0..path.len() {
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

fn apply_json(
    document: &mut JsonDocument,
    path: &[&str],
    render: impl FnOnce(&mut JsonDocument) -> Result<Value, ConflictReason>,
) -> Result<(), ConflictReason> {
    let value = render(document)?;
    let serialized = serde_json::to_string(&value).map_err(|_| ConflictReason::Unparsable)?;
    if document.has_member(path) {
        document
            .replace_value(path, &serialized)
            .map_err(|_| ConflictReason::Unparsable)
    } else {
        let (parent, leaf) = path.split_at(path.len() - 1);
        document
            .insert_member(parent, leaf[0], &serialized)
            .map_err(|_| ConflictReason::Unparsable)
    }
}

fn nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{nanos}", std::process::id())
}
