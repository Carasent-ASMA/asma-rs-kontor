//! The Claude Code adapter, through its native command boundary.
//!
//! Claude Code's user-scoped MCP store is not a documented file, so bootstrap
//! never edits one. It builds the documented command surface — `mcp add-json
//! <name> --scope user`, `mcp get <name>` and `mcp remove <name> --scope user`
//! — and delegates execution to an injected [`ClaudeBoundary`]. Only a fake
//! boundary ships here; nothing spawns the client in this unit.
//!
//! Replacement is race-safe and failure-safe: the entry is re-read and its
//! full snapshot digest revalidated immediately before removal, the exact
//! original entry is captured, and a failed re-add attempts to restore it. An
//! entry created after observation is never deleted.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{
    AdapterError, AdapterResult, ClientAdapter, ClientId, ConflictReason, EntryState, ObservedHash,
    OwnershipStore, ServerSpec, desired_entry,
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
    ownership: OwnershipStore,
}

impl ClaudeAdapter {
    /// An adapter over one boundary and one ownership ledger.
    #[must_use]
    pub fn new(boundary: Box<dyn ClaudeBoundary>, ownership: OwnershipStore) -> Self {
        Self {
            boundary,
            ownership,
        }
    }

    fn observe(&mut self, spec: &ServerSpec) -> Result<EntryState, AdapterError> {
        spec.validate()?;
        let entry = match self.boundary.get(ENTRY_NAME) {
            Ok(entry) => entry,
            Err(ClaudeBoundaryError::Unavailable) => return Ok(EntryState::ClientAbsent),
            Err(_) => return Err(AdapterError::CommandFailed),
        };
        let Some(value) = entry else {
            return Ok(EntryState::EntryAbsent);
        };
        let observed = ObservedHash::of_value(&value);
        if !schema_accepts(&value) {
            return Ok(EntryState::Unrelated { observed });
        }
        let ledger = match self.ownership.load() {
            Ok(ledger) => ledger,
            Err(_) => {
                return Ok(EntryState::Refused {
                    reason: ConflictReason::OwnershipUnreadable,
                });
            }
        };
        let desired = desired_entry(ClientId::ClaudeCode, spec);
        Ok(super::classify(
            &value,
            &desired,
            ledger.owns(ClientId::ClaudeCode, &observed),
        ))
    }

    fn readback(&mut self, spec: &ServerSpec) -> Result<AdapterResult, AdapterError> {
        match self.observe(spec)? {
            EntryState::Current { observed } => {
                let desired = desired_entry(ClientId::ClaudeCode, spec);
                self.ownership.record(
                    ClientId::ClaudeCode,
                    &observed,
                    &ObservedHash::of_value(&desired),
                )?;
                Ok(AdapterResult::Updated)
            }
            _ => Ok(AdapterResult::FailedReadback),
        }
    }

    /// Add a desired entry into an observed-absent store.
    fn add_absent(
        &mut self,
        spec: &ServerSpec,
        faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError> {
        let json = serde_json::to_string(&desired_entry(ClientId::ClaudeCode, spec))
            .map_err(|_| AdapterError::CommandFailed)?;
        let _ = faults;
        match self.boundary.add_json(ENTRY_NAME, &json) {
            Ok(()) => {}
            Err(ClaudeBoundaryError::AlreadyExists) => {
                return Ok(AdapterResult::Conflict {
                    reason: ConflictReason::ConcurrentChange,
                    observed: None,
                });
            }
            Err(_) => return Err(AdapterError::CommandFailed),
        }
        self.readback(spec)
    }

    /// Replace an owned or explicitly authorized entry, race- and
    /// failure-safe.
    fn replace(
        &mut self,
        spec: &ServerSpec,
        expected: &ObservedHash,
    ) -> Result<AdapterResult, AdapterError> {
        let Some(current) = self
            .boundary
            .get(ENTRY_NAME)
            .map_err(|_| AdapterError::CommandFailed)?
        else {
            return Ok(AdapterResult::FailedReadback);
        };
        if &ObservedHash::of_value(&current) != expected {
            return Ok(AdapterResult::Conflict {
                reason: ConflictReason::ConcurrentChange,
                observed: Some(ObservedHash::of_value(&current)),
            });
        }
        let original = serde_json::to_string(&current).map_err(|_| AdapterError::CommandFailed)?;
        let desired = serde_json::to_string(&desired_entry(ClientId::ClaudeCode, spec))
            .map_err(|_| AdapterError::CommandFailed)?;

        if self.boundary.remove(ENTRY_NAME).is_err() {
            return Err(AdapterError::CommandFailed);
        }
        match self.boundary.add_json(ENTRY_NAME, &desired) {
            Ok(()) => {}
            Err(_) => {
                // The exact original entry is restored; a failed restore is a
                // typed error, never a silent success.
                self.boundary
                    .add_json(ENTRY_NAME, &original)
                    .map_err(|_| AdapterError::RestoreFailed)?;
                return Ok(AdapterResult::Conflict {
                    reason: ConflictReason::ReplacementFailed,
                    observed: Some(expected.clone()),
                });
            }
        }
        self.readback(spec)
    }
}

impl ClientAdapter for ClaudeAdapter {
    fn client(&self) -> ClientId {
        ClientId::ClaudeCode
    }

    fn inspect(&mut self, spec: &ServerSpec) -> Result<EntryState, AdapterError> {
        self.observe(spec)
    }

    fn install(
        &mut self,
        spec: &ServerSpec,
        faults: &mut dyn FaultInjector,
    ) -> Result<AdapterResult, AdapterError> {
        match self.observe(spec)? {
            EntryState::ClientAbsent => Ok(AdapterResult::Absent),
            EntryState::EntryAbsent => self.add_absent(spec, faults).map(|result| match result {
                AdapterResult::Updated => AdapterResult::Installed,
                other => other,
            }),
            EntryState::Current { .. } => Ok(AdapterResult::AlreadyCurrent),
            EntryState::ManagedStale { observed } => self.replace(spec, &observed),
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
        match self.observe(spec)? {
            EntryState::ClientAbsent | EntryState::EntryAbsent => Ok(AdapterResult::Absent),
            EntryState::Current { .. } => Ok(AdapterResult::AlreadyCurrent),
            EntryState::ManagedStale { observed } | EntryState::Unrelated { observed } => {
                if &observed != expected {
                    return Ok(AdapterResult::Conflict {
                        reason: ConflictReason::ObservedHashMismatch,
                        observed: Some(observed),
                    });
                }
                self.replace(spec, &observed).map(|result| match result {
                    AdapterResult::Updated => AdapterResult::Repaired,
                    other => other,
                })
            }
            EntryState::Refused { reason } => Ok(AdapterResult::Conflict {
                reason,
                observed: None,
            }),
        }
    }
}

fn schema_accepts(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.get("type").and_then(Value::as_str) == Some("stdio")
        && object.get("command").and_then(Value::as_str).is_some()
        && object
            .get("args")
            .and_then(Value::as_array)
            .is_some_and(|array| array.iter().all(Value::is_string))
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

    /// The entry this fake currently holds.
    #[must_use]
    pub fn entry(&self, name: &str) -> Option<&Value> {
        self.entries.get(name)
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
