//! The Claude Code adapter, through its native command boundary.
//!
//! Claude Code's user-scoped MCP store is not a documented file, so bootstrap
//! never edits one. It builds the documented command surface — `mcp add-json
//! <name> --scope user`, `mcp get <name>` and `mcp remove <name> --scope user`
//! — and delegates execution to an injected [`ClaudeBoundary`]. Only a fake
//! boundary ships here; nothing spawns the client in this unit.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{
    AdapterError, AdapterResult, ClientAdapter, ClientId, ConflictReason, EntryState,
    MCP_EXECUTABLE, ObservedHash, ServerSpec,
};
use crate::fault::FaultInjector;

/// The managed entry name.
pub const ENTRY_NAME: &str = "kontor";

/// The argv for `claude mcp add-json`.
#[must_use]
pub fn add_json_argv(name: &str, json: &str) -> Vec<String> {
    vec![
        "mcp".to_owned(),
        "add-json".to_owned(),
        name.to_owned(),
        "--scope".to_owned(),
        "user".to_owned(),
        json.to_owned(),
    ]
}

/// The argv for `claude mcp remove`.
#[must_use]
pub fn remove_argv(name: &str) -> Vec<String> {
    vec![
        "mcp".to_owned(),
        "remove".to_owned(),
        name.to_owned(),
        "--scope".to_owned(),
        "user".to_owned(),
    ]
}

/// The argv for `claude mcp get`.
#[must_use]
pub fn get_argv(name: &str) -> Vec<String> {
    vec!["mcp".to_owned(), "get".to_owned(), name.to_owned()]
}

/// Why the command boundary refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClaudeBoundaryError {
    /// Claude Code is not installed or not reachable.
    #[error("the Claude Code command surface is not available")]
    Unavailable,
    /// The boundary already holds an entry under that name.
    #[error("an entry with that name already exists")]
    AlreadyExists,
    /// The boundary holds no entry under that name.
    #[error("no entry with that name exists")]
    NotFound,
    /// The command itself failed.
    #[error("the client command failed")]
    Failed,
}

/// The injected command boundary: bootstrap calls this, never a process.
pub trait ClaudeBoundary {
    /// Read one user-scoped entry.
    ///
    /// # Errors
    /// [`ClaudeBoundaryError`].
    fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError>;

    /// Add one user-scoped entry from its JSON document.
    ///
    /// # Errors
    /// [`ClaudeBoundaryError`].
    fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError>;

    /// Remove one user-scoped entry.
    ///
    /// # Errors
    /// [`ClaudeBoundaryError`].
    fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError>;
}

/// The Claude Code adapter over one injected boundary.
pub struct ClaudeAdapter {
    boundary: Box<dyn ClaudeBoundary>,
}

impl ClaudeAdapter {
    /// An adapter over one boundary.
    #[must_use]
    pub fn new(boundary: Box<dyn ClaudeBoundary>) -> Self {
        Self { boundary }
    }

    fn desired_json(spec: &ServerSpec) -> String {
        serde_json::json!({
            "type": "stdio",
            "command": spec.program.to_string_lossy(),
            "args": spec.args,
        })
        .to_string()
    }

    fn observe(&mut self, spec: &ServerSpec) -> Result<EntryState, AdapterError> {
        let entry = match self.boundary.get(ENTRY_NAME) {
            Ok(entry) => entry,
            Err(ClaudeBoundaryError::Unavailable) => return Ok(EntryState::ClientAbsent),
            Err(_) => return Err(AdapterError::CommandFailed),
        };
        let Some(value) = entry else {
            return Ok(EntryState::EntryAbsent);
        };
        let Some(object) = value.as_object() else {
            return Ok(EntryState::Unrelated {
                observed: ObservedHash::of_bytes(value.to_string().as_bytes()),
            });
        };
        let Some(program) = object.get("command").and_then(Value::as_str) else {
            return Ok(EntryState::Unrelated {
                observed: ObservedHash::of_bytes(value.to_string().as_bytes()),
            });
        };
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
        let observed = ObservedHash::of_entry(program, &args);
        if basename(program) != MCP_EXECUTABLE {
            return Ok(EntryState::Unrelated { observed });
        }
        if program == spec.program.to_string_lossy().as_ref() && args == spec.args {
            Ok(EntryState::Current { observed })
        } else {
            Ok(EntryState::ManagedStale { observed })
        }
    }

    fn replace(&mut self, spec: &ServerSpec) -> Result<(), AdapterError> {
        let json = Self::desired_json(spec);
        match self.boundary.add_json(ENTRY_NAME, &json) {
            Ok(()) => Ok(()),
            Err(ClaudeBoundaryError::AlreadyExists) => {
                // The documented update path is an explicit remove followed by
                // the add; overwrite semantics are never assumed.
                self.boundary
                    .remove(ENTRY_NAME)
                    .map_err(|_| AdapterError::CommandFailed)?;
                self.boundary
                    .add_json(ENTRY_NAME, &json)
                    .map_err(|_| AdapterError::CommandFailed)
            }
            Err(_) => Err(AdapterError::CommandFailed),
        }
    }
}

impl ClientAdapter for ClaudeAdapter {
    fn client(&self) -> ClientId {
        ClientId::ClaudeCode
    }

    fn inspect(&mut self, spec: &ServerSpec) -> Result<EntryState, AdapterError> {
        if !spec.points_at_managed_executable() {
            return Err(AdapterError::InvalidSpec);
        }
        self.observe(spec)
    }

    fn install(
        &mut self,
        spec: &ServerSpec,
        _faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError> {
        if !spec.points_at_managed_executable() {
            return Err(AdapterError::InvalidSpec);
        }
        match self.observe(spec)? {
            EntryState::ClientAbsent => Ok(AdapterResult::Absent),
            EntryState::EntryAbsent => {
                self.replace(spec)?;
                match self.observe(spec)? {
                    EntryState::Current { .. } => Ok(AdapterResult::Installed),
                    _ => Ok(AdapterResult::FailedReadback),
                }
            }
            EntryState::Current { .. } => Ok(AdapterResult::AlreadyCurrent),
            EntryState::ManagedStale { observed: _ } => {
                self.replace(spec)?;
                match self.observe(spec)? {
                    EntryState::Current { .. } => Ok(AdapterResult::Updated),
                    _ => Ok(AdapterResult::FailedReadback),
                }
            }
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
        _faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError> {
        if !spec.points_at_managed_executable() {
            return Err(AdapterError::InvalidSpec);
        }
        match self.observe(spec)? {
            EntryState::ClientAbsent | EntryState::EntryAbsent => Ok(AdapterResult::Absent),
            EntryState::Current { .. } => Ok(AdapterResult::AlreadyCurrent),
            EntryState::ManagedStale { observed } | EntryState::Unrelated { observed } => {
                if observed != *expected {
                    return Ok(AdapterResult::Conflict {
                        reason: ConflictReason::ObservedHashMismatch,
                        observed: Some(observed),
                    });
                }
                self.replace(spec)?;
                match self.observe(spec)? {
                    EntryState::Current { .. } => Ok(AdapterResult::Repaired),
                    _ => Ok(AdapterResult::FailedReadback),
                }
            }
            EntryState::Refused { reason } => Ok(AdapterResult::Conflict {
                reason,
                observed: None,
            }),
        }
    }
}

/// A boundary that is deliberately unwired.
///
/// This unit defines the command boundary but drives no client process; the
/// production boundary belongs to the later live-installation unit. Every call
/// therefore answers with a typed [`ClaudeBoundaryError::Unavailable`], which
/// the adapter reports as [`crate::client::EntryState::ClientAbsent`] rather
/// than as a success it did not achieve.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableClaudeBoundary;

impl ClaudeBoundary for UnavailableClaudeBoundary {
    fn get(&mut self, _name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
        Err(ClaudeBoundaryError::Unavailable)
    }

    fn add_json(&mut self, _name: &str, _json: &str) -> Result<(), ClaudeBoundaryError> {
        Err(ClaudeBoundaryError::Unavailable)
    }

    fn remove(&mut self, _name: &str) -> Result<(), ClaudeBoundaryError> {
        Err(ClaudeBoundaryError::Unavailable)
    }
}

/// A fake boundary that holds entries in memory and records every call.
#[derive(Debug, Default, Clone)]
pub struct FakeClaudeBoundary {
    entries: BTreeMap<String, Value>,
    calls: Vec<ClaudeCall>,
    available: bool,
}

/// One recorded boundary call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeCall {
    /// `mcp get <name>`.
    Get {
        /// The entry name.
        name: String,
    },
    /// `mcp add-json <name> --scope user <json>`.
    AddJson {
        /// The entry name.
        name: String,
        /// The JSON document passed.
        json: String,
    },
    /// `mcp remove <name> --scope user`.
    Remove {
        /// The entry name.
        name: String,
    },
}

impl FakeClaudeBoundary {
    /// An available fake with an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            available: true,
            ..Self::default()
        }
    }

    /// An unavailable fake, as on a machine without Claude Code.
    #[must_use]
    pub fn unavailable() -> Self {
        Self::default()
    }

    /// Every call this boundary received, in order.
    #[must_use]
    pub fn calls(&self) -> &[ClaudeCall] {
        &self.calls
    }

    /// Seed one existing entry.
    pub fn seed(&mut self, name: &str, value: Value) {
        self.entries.insert(name.to_owned(), value);
    }
}

impl ClaudeBoundary for FakeClaudeBoundary {
    fn get(&mut self, name: &str) -> Result<Option<Value>, ClaudeBoundaryError> {
        self.calls.push(ClaudeCall::Get {
            name: name.to_owned(),
        });
        if !self.available {
            return Err(ClaudeBoundaryError::Unavailable);
        }
        Ok(self.entries.get(name).cloned())
    }

    fn add_json(&mut self, name: &str, json: &str) -> Result<(), ClaudeBoundaryError> {
        self.calls.push(ClaudeCall::AddJson {
            name: name.to_owned(),
            json: json.to_owned(),
        });
        if !self.available {
            return Err(ClaudeBoundaryError::Unavailable);
        }
        let value: Value = serde_json::from_str(json).map_err(|_| ClaudeBoundaryError::Failed)?;
        if self.entries.contains_key(name) {
            return Err(ClaudeBoundaryError::AlreadyExists);
        }
        self.entries.insert(name.to_owned(), value);
        Ok(())
    }

    fn remove(&mut self, name: &str) -> Result<(), ClaudeBoundaryError> {
        self.calls.push(ClaudeCall::Remove {
            name: name.to_owned(),
        });
        if !self.available {
            return Err(ClaudeBoundaryError::Unavailable);
        }
        self.entries
            .remove(name)
            .map(|_| ())
            .ok_or(ClaudeBoundaryError::NotFound)
    }
}

fn basename(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or(program)
}
