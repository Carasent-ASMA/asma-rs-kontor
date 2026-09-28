//! Live fleet model routing: read, validate, flatten and reload `fleet.yml`,
//! or serve the one activated fleet policy.
//!
//! The model route of a seat used to be frozen into a template version and then
//! copied into every team run that started from it. This module replaces that
//! for the seats a live fleet binds, with no rebuild, restart or republish.
//!
//! Parsing, validation, flattening, key construction and vendor lookup are
//! [`kontor_fleet`], the one implementation shared with direct-mode
//! orchestration. This module owns only what touches the Realm state root: the
//! file security checks, the last-valid state, the history copies, activation,
//! the status projection and the decision receipts.
//!
//! Two selections exist, and the activation record decides which one applies:
//!
//! - **No `fleet-activation.json`:** the unmigrated ASMA-8255 behaviour, byte
//!   for byte. An operator edits `fleet.yml`, the next placement reads it, an
//!   invalid edit keeps the last valid snapshot, and a missing file means
//!   legacy template routing.
//! - **A `fleet-activation.json`:** the generated record names one published,
//!   immutable `fleet-history/<policy-content-hash>.yml`, and placement reads
//!   exactly that. `fleet.yml`, any checkout and any unactivated publication
//!   have no effect. A record or artifact that cannot be verified refuses
//!   placement; it never falls back to `fleet.yml` or to template routing.

use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use kontor_core::id::{ContentHash, Timestamp, format_utc_timestamp};
use kontor_core::spec::ModelRung;
use kontor_fleet::MAX_FILE_BYTES;
use kontor_fleet::rule::F04;
use serde::{Deserialize, Serialize};

pub use kontor_fleet::{FleetError, FleetSnapshot};
pub(crate) use kontor_fleet::{FleetRoute, advisor_key, committee_key, independence_key, team_key};

/// The fleet configuration file name inside a Realm state root.
pub(crate) const FLEET_FILE: &str = "fleet.yml";

/// One immutable copy of every accepted fleet configuration, and every
/// published fleet policy, named by its content hash.
pub(crate) const FLEET_HISTORY_DIR: &str = "fleet-history";

/// One JSON-lines decision log per team run.
pub(crate) const FLEET_DECISIONS_DIR: &str = "fleet-decisions";

/// Whether the last edit was accepted, and why not when it was not.
pub(crate) const FLEET_STATUS_FILE: &str = "fleet-status.json";

/// The generated record naming the activated fleet policy.
pub(crate) const FLEET_ACTIVATION_FILE: &str = "fleet-activation.json";

/// One JSON-lines leadership decision log per SeatBinding, inside
/// [`FLEET_DECISIONS_DIR`].
pub(crate) const LEADERSHIP_DECISIONS_DIR: &str = "leadership";

/// Largest activation record this build reads, in bytes.
pub(crate) const MAX_ACTIVATION_BYTES: u64 = 4 * 1024;

/// The only activation record format this build reads.
const ACTIVATION_SCHEMA_VERSION: u32 = 1;

const F01: &str = "fleet.yml must be a regular file, not a symlink";
const F02: &str = "fleet.yml must not be writable by group or others";
const F03: &str = "fleet.yml must be owned by the state root's owner";
const F05: &str = "fleet.yml changed while it was being read";
const F06: &str = "fleet.yml is not UTF-8";

const P01: &str = "a published fleet policy must be a regular file, not a symlink";
const P02: &str = "a published fleet policy must not be writable by group or others";
const P03: &str = "a published fleet policy must be owned by the state root's owner";
const P04: &str = "a published fleet policy exceeds 256 KiB";
const P05: &str = "a published fleet policy changed while it was being read";
const P06: &str = "a published fleet policy is not UTF-8";

const A01: &str = "fleet-activation.json must be a regular file, not a symlink";
const A02: &str = "fleet-activation.json must not be writable by group or others";
const A03: &str = "fleet-activation.json must be owned by the state root's owner";
const A04: &str = "fleet-activation.json exceeds 4 KiB";
const A05: &str = "fleet-activation.json changed while it was being read";
const A06: &str = "fleet-activation.json is not UTF-8";
const A07: &str = "fleet-activation.json is not a schema_version 1 activation record";
const A08: &str = "the activated fleet policy is not published";

/// The refusal that means "no such published policy", which the read and
/// activate operations answer as not found rather than as invalid.
pub(crate) const UNPUBLISHED_POLICY: &str = A08;

/// The refusals one guarded read names, in the order the checks run.
struct Guard {
    symlink: &'static str,
    writable: &'static str,
    owner: &'static str,
    oversized: &'static str,
    swapped: &'static str,
    utf8: &'static str,
    limit: u64,
}

const FLEET_GUARD: Guard = Guard {
    symlink: F01,
    writable: F02,
    owner: F03,
    oversized: F04,
    swapped: F05,
    utf8: F06,
    limit: MAX_FILE_BYTES,
};

const POLICY_GUARD: Guard = Guard {
    symlink: P01,
    writable: P02,
    owner: P03,
    oversized: P04,
    swapped: P05,
    utf8: P06,
    limit: MAX_FILE_BYTES,
};

const ACTIVATION_GUARD: Guard = Guard {
    symlink: A01,
    writable: A02,
    owner: A03,
    oversized: A04,
    swapped: A05,
    utf8: A06,
    limit: MAX_ACTIVATION_BYTES,
};

/// The rungs one delivery seat may walk, and the fleet binding they came from.
///
/// `fleet` is `Some` exactly when the rungs came from the live fleet snapshot
/// rather than the frozen template chain: it carries that snapshot and the
/// binding key so an admitted placement can be recorded against the exact
/// version that authorised it.
#[derive(Debug)]
pub(crate) struct DeclaredRungs {
    pub(crate) rungs: Vec<ModelRung>,
    pub(crate) fleet: Option<(Arc<FleetSnapshot>, String)>,
}

/// The generated activation record: which published policy placement reads.
///
/// A runtime projection, never authoring input. It names the policy by content
/// hash and carries the schema needed to verify that artifact; the artifact
/// itself is the immutable `fleet-history/<policy_hash>.yml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FleetActivation {
    /// The format of this record.
    pub(crate) schema_version: u32,
    /// SHA-256 of exactly the activated policy bytes.
    pub(crate) policy_hash: ContentHash,
    /// The schema those bytes validate under.
    pub(crate) policy_schema_version: u32,
    /// When activation replaced the record.
    pub(crate) activated_at: String,
}

/// What one publication wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Published {
    /// SHA-256 of exactly the published bytes.
    pub(crate) hash: ContentHash,
    /// The schema those bytes validate under.
    pub(crate) schema_version: u32,
    /// Whether this call wrote the artifact rather than finding it.
    pub(crate) created: bool,
}

/// What one activation left selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Activated {
    /// The standing activation record.
    pub(crate) record: FleetActivation,
    /// Whether this call replaced the record rather than finding it.
    pub(crate) changed: bool,
}

/// Why an activation changed nothing.
#[derive(Debug)]
pub(crate) enum ActivationRefusal {
    /// The published artifact did not verify, or the record was not written.
    Policy(FleetError),
    /// The record selects another policy than the one the caller read.
    Moved,
}

/// The Realm's fleet selection as the read operation reports it.
#[derive(Debug)]
pub(crate) struct FleetPolicyStatus {
    /// Whether an activation record exists and so decides placement.
    pub(crate) activation: bool,
    /// The record as written, when it can be read.
    pub(crate) record: Option<FleetActivation>,
    /// The snapshot placement reads now: activated, legacy, or none.
    pub(crate) active: Option<Arc<FleetSnapshot>>,
    /// Why an existing activation cannot be served, when it cannot.
    pub(crate) refusal: Option<String>,
}

/// One leadership placement, as its decision log records it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LeadershipDecision {
    pub(crate) operation: String,
    pub(crate) project_id: String,
    pub(crate) epic_id: String,
    pub(crate) seat_binding_id: String,
    pub(crate) occupancy_generation: u64,
    pub(crate) core_team_revision_hash: String,
    pub(crate) core_team_version: u32,
    pub(crate) role_slot_id: String,
    pub(crate) binding_key: String,
    pub(crate) fleet_hash: String,
    pub(crate) policy_schema_version: u32,
    pub(crate) chain: String,
    pub(crate) step: u16,
    pub(crate) sub_step: u16,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) effort: Option<String>,
    pub(crate) vendor: String,
    pub(crate) decided_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    dev: u64,
    ino: u64,
}

impl Stamp {
    fn of(metadata: &std::fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        }
    }
}

/// What the last read of an activation record concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ActivationOutcome {
    Active {
        hash: ContentHash,
        activated_at: String,
    },
    Blocked(String),
}

#[derive(Debug, Default)]
struct Cache {
    stamp: Option<Stamp>,
    good: Option<Arc<FleetSnapshot>>,
    last_error: Option<String>,
    loaded_at: Option<String>,
    /// `Some` exactly while an activation record exists.
    activation: Option<ActivationOutcome>,
}

/// One placement decision, written to the decision log and the `info` log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FleetDecision {
    pub(crate) team_run_id: String,
    pub(crate) agent_run_id: String,
    pub(crate) binding_key: String,
    pub(crate) role_slot: String,
    pub(crate) fleet_hash: String,
    pub(crate) step: u16,
    pub(crate) sub_step: u16,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) effort: Option<String>,
    pub(crate) vendor: String,
    pub(crate) account_profile_id: Option<String>,
    pub(crate) decided_at: String,
}

#[derive(Serialize)]
struct StatusFile<'a> {
    active_hash: Option<&'a str>,
    loaded_at: Option<&'a str>,
    last_error: Option<&'a str>,
    checked_at: String,
    /// Written only while an activation record selects the policy, so the
    /// unmigrated status file keeps its exact shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    selection: Option<&'static str>,
}

/// The live, stamp-tracked fleet configuration for one Realm.
///
/// One process holds one of these per Realm, on [`crate::applications::Services`].
/// It never watches a file; every placement asks it for the current snapshot.
#[derive(Debug)]
pub struct FleetSource {
    state_root: PathBuf,
    cache: Mutex<Cache>,
}

impl FleetSource {
    /// A source rooted at `state_root`; nothing is read until first use.
    pub fn at(state_root: &Path) -> Self {
        Self {
            state_root: state_root.to_path_buf(),
            cache: Mutex::new(Cache::default()),
        }
    }

    /// The current valid snapshot, or `None` when there is none to read.
    ///
    /// Without an activation record this is the unmigrated behaviour: a
    /// missing file clears the cache and means "legacy routing", a changed file
    /// is re-read, and an invalid edit keeps the last valid snapshot and is
    /// reported in `fleet-status.json`. With one, it is the activated policy,
    /// and `None` when that policy cannot be verified. `None` only narrows a
    /// route check to the compiled catalog: resolving a seat binding must use
    /// [`Self::policy`], which refuses instead of falling back.
    pub fn current(&self) -> Option<Arc<FleetSnapshot>> {
        self.policy().ok().flatten()
    }

    /// The policy a seat binding resolves against.
    ///
    /// `Ok(None)` is legacy routing with no fleet, which only the unmigrated
    /// selection can answer. While an activation record exists, the answer is
    /// the activated policy or a refusal: the record and its artifact are both
    /// verified on every call, and nothing else is read.
    ///
    /// # Errors
    /// Returns the first check the activation record or its published
    /// artifact fails.
    pub(crate) fn policy(&self) -> Result<Option<Arc<FleetSnapshot>>, FleetError> {
        let mut cache = self.lock();
        if let Err(error) = std::fs::symlink_metadata(self.state_root.join(FLEET_ACTIVATION_FILE))
            && error.kind() == std::io::ErrorKind::NotFound
        {
            if cache.activation.is_some() {
                // The unmigrated file is read afresh, not from a stamp taken
                // before the activation record appeared.
                *cache = Cache::default();
            }
            return Ok(self.legacy(&mut cache));
        }
        let verified = self.verify_activation();
        let outcome = match &verified {
            Ok((record, _)) => ActivationOutcome::Active {
                hash: record.policy_hash.clone(),
                activated_at: record.activated_at.clone(),
            },
            Err(error) => ActivationOutcome::Blocked(error.to_string()),
        };
        if cache.activation.as_ref() != Some(&outcome) {
            match &outcome {
                ActivationOutcome::Active { hash, .. } => {
                    tracing::info!(fleet_hash = %hash, "fleet.activated");
                }
                ActivationOutcome::Blocked(error) => {
                    tracing::warn!(error = %error, "fleet.activation_refused");
                }
            }
            self.write_activation_status(&outcome);
            cache.activation = Some(outcome);
        }
        verified.map(|(_, snapshot)| Some(snapshot))
    }

    /// Publish one validated policy as an immutable, content-addressed artifact.
    ///
    /// The bytes validated are the bytes written, under their own hash.
    /// Publishing an already published policy re-verifies the stored copy and
    /// writes nothing. Publication never changes what placement reads; only
    /// [`Self::activate`] does.
    ///
    /// # Errors
    /// Returns the policy's first refusal, a stored copy that no longer
    /// verifies, or the write failure.
    pub(crate) fn publish(&self, document: &str) -> Result<Published, FleetError> {
        let snapshot = FleetSnapshot::parse_policy(document)?;
        let created = if std::fs::symlink_metadata(self.published_path(snapshot.hash())).is_ok() {
            self.verify_published(snapshot.hash())?;
            false
        } else {
            self.store_history(&snapshot)
                .map_err(|source| FleetError::Write { source })?;
            true
        };
        Ok(Published {
            hash: snapshot.hash().clone(),
            schema_version: snapshot.schema_version(),
            created,
        })
    }

    /// Select one published policy for every later placement.
    ///
    /// `expected` is the policy the caller read as active, `None` for none; a
    /// record naming anything else refuses with [`ActivationRefusal::Moved`].
    /// Selecting the policy already active answers with the standing record
    /// and writes nothing, which is also how a replay converges. Otherwise the
    /// artifact is verified in full before the record changes, and the record
    /// is replaced atomically, so a reader sees the previous activation or this
    /// one and never a partial record. A refused activation changes nothing.
    ///
    /// # Errors
    /// Returns [`ActivationRefusal::Moved`] for a stale `expected`, and the
    /// published artifact's first failed check or the write failure otherwise.
    pub(crate) fn activate(
        &self,
        hash: &ContentHash,
        expected: Option<&ContentHash>,
    ) -> Result<Activated, ActivationRefusal> {
        // One writer at a time in this process, and no read in between.
        let _cache = self.lock();
        let standing = self.recorded_activation();
        if let Some(record) = standing.as_ref()
            && &record.policy_hash == hash
        {
            self.verify_published(hash)
                .map_err(ActivationRefusal::Policy)?;
            return Ok(Activated {
                record: record.clone(),
                changed: false,
            });
        }
        if standing.as_ref().map(|record| &record.policy_hash) != expected {
            return Err(ActivationRefusal::Moved);
        }
        let snapshot = self
            .verify_published(hash)
            .map_err(ActivationRefusal::Policy)?;
        let record = FleetActivation {
            schema_version: ACTIVATION_SCHEMA_VERSION,
            policy_hash: hash.clone(),
            policy_schema_version: snapshot.schema_version(),
            activated_at: format_utc_timestamp(Timestamp::now()),
        };
        let bytes = serde_json::to_vec_pretty(&record).map_err(|error| {
            ActivationRefusal::Policy(FleetError::Write {
                source: std::io::Error::other(error),
            })
        })?;
        let temporary = self.state_root.join(format!(
            "{FLEET_ACTIVATION_FILE}.{}.tmp",
            std::process::id()
        ));
        write_owner_only(&temporary, &bytes)
            .and_then(|()| std::fs::rename(&temporary, self.state_root.join(FLEET_ACTIVATION_FILE)))
            .map_err(|source| {
                let _ = std::fs::remove_file(&temporary);
                ActivationRefusal::Policy(FleetError::Write { source })
            })?;
        Ok(Activated {
            record,
            changed: true,
        })
    }

    /// What the Realm's fleet selection currently is, for the read operation.
    pub(crate) fn status(&self) -> FleetPolicyStatus {
        let selected = std::fs::symlink_metadata(self.state_root.join(FLEET_ACTIVATION_FILE))
            .map_or_else(
                |error| error.kind() != std::io::ErrorKind::NotFound,
                |_| true,
            );
        let (active, refusal) = match self.policy() {
            Ok(active) => (active, None),
            Err(error) => (None, Some(error.to_string())),
        };
        FleetPolicyStatus {
            activation: selected,
            record: self.recorded_activation(),
            active,
            refusal,
        }
    }

    /// Append one leadership placement the activated policy authorised.
    ///
    /// Written before the effect it authorises, per SeatBinding, so an auditor
    /// can reconstruct which policy bytes placed which occupancy of which slot
    /// of which pinned Core Team revision.
    ///
    /// # Errors
    /// Returns the I/O failure so the caller refuses the placement: an
    /// unrecorded leadership route could not be traced to its policy.
    pub(crate) fn record_leadership_decision(
        &self,
        decision: &LeadershipDecision,
    ) -> std::io::Result<()> {
        let directory = self
            .state_root
            .join(FLEET_DECISIONS_DIR)
            .join(LEADERSHIP_DECISIONS_DIR);
        create_private_dir(&self.state_root.join(FLEET_DECISIONS_DIR))?;
        create_private_dir(&directory)?;
        let path = directory.join(format!("{}.jsonl", decision.seat_binding_id));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        let line = serde_json::to_string(decision).map_err(std::io::Error::other)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        tracing::info!(
            operation = %decision.operation,
            seat_binding_id = %decision.seat_binding_id,
            occupancy_generation = decision.occupancy_generation,
            binding_key = %decision.binding_key,
            fleet_hash = %decision.fleet_hash,
            step = decision.step,
            sub_step = decision.sub_step,
            provider = %decision.provider,
            model = %decision.model,
            "fleet.leadership_route_decided"
        );
        Ok(())
    }

    /// Append one admitted placement to the run's decision log and the `info` log.
    ///
    /// # Errors
    /// Returns the I/O failure so the caller can fail the placement: an
    /// unrecorded route would silently break `independent_of`.
    pub(crate) fn record_decision(&self, decision: &FleetDecision) -> std::io::Result<()> {
        let directory = self.state_root.join(FLEET_DECISIONS_DIR);
        create_private_dir(&directory)?;
        let path = directory.join(format!("{}.jsonl", decision.team_run_id));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        let line = serde_json::to_string(decision).map_err(std::io::Error::other)?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_data()?;
        tracing::info!(
            team_run_id = %decision.team_run_id,
            agent_run_id = %decision.agent_run_id,
            binding_key = %decision.binding_key,
            role_slot = %decision.role_slot,
            fleet_hash = %decision.fleet_hash,
            step = decision.step,
            sub_step = decision.sub_step,
            provider = %decision.provider,
            model = %decision.model,
            effort = decision.effort.as_deref().unwrap_or(""),
            vendor = %decision.vendor,
            account = decision.account_profile_id.as_deref().unwrap_or(""),
            "fleet.route_decided"
        );
        Ok(())
    }

    /// The vendor of the last recorded route for `binding_key` in this run.
    pub(crate) fn last_vendor(&self, team_run_id: &str, binding_key: &str) -> Option<String> {
        let path = self
            .state_root
            .join(FLEET_DECISIONS_DIR)
            .join(format!("{team_run_id}.jsonl"));
        let text = std::fs::read_to_string(path).ok()?;
        let mut vendor = None;
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<FleetDecision>(line) {
                Ok(decision) => {
                    if decision.binding_key == binding_key {
                        vendor = Some(decision.vendor);
                    }
                }
                Err(_) => tracing::warn!(team_run_id, "fleet.decision_line_unreadable"),
            }
        }
        vendor
    }

    fn lock(&self) -> MutexGuard<'_, Cache> {
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The unmigrated selection: stamp-tracked `fleet.yml` with last-valid.
    fn legacy(&self, cache: &mut Cache) -> Option<Arc<FleetSnapshot>> {
        let path = self.state_root.join(FLEET_FILE);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let was_tracking =
                    cache.stamp.is_some() || cache.good.is_some() || cache.last_error.is_some();
                cache.stamp = None;
                cache.good = None;
                cache.last_error = None;
                cache.loaded_at = None;
                if was_tracking {
                    self.write_status_file(cache);
                }
                return None;
            }
            Err(error) => {
                cache.last_error = Some(format!(
                    "the fleet configuration could not be read: {error}"
                ));
                return cache.good.clone();
            }
        };
        let stamp = Stamp::of(&metadata);
        if cache.stamp == Some(stamp) {
            return cache.good.clone();
        }
        cache.stamp = Some(stamp);
        match self.load(&path) {
            Ok(snapshot) => {
                let snapshot = Arc::new(snapshot);
                let changed = cache
                    .good
                    .as_ref()
                    .is_none_or(|good| good.hash() != snapshot.hash());
                if changed {
                    self.write_history(&snapshot);
                    tracing::info!(fleet_hash = %snapshot.hash(), "fleet.loaded");
                    cache.good = Some(snapshot);
                    cache.loaded_at = Some(format_utc_timestamp(Timestamp::now()));
                }
                cache.last_error = None;
            }
            Err(error) => {
                tracing::warn!(error = %error, "fleet.rejected");
                cache.last_error = Some(error.to_string());
            }
        }
        self.write_status_file(cache);
        cache.good.clone()
    }

    fn load(&self, path: &Path) -> Result<FleetSnapshot, FleetError> {
        FleetSnapshot::parse(&self.read_guarded(path, &FLEET_GUARD)?)
    }

    /// The standing activation record as written, or `None` when there is no
    /// record or it cannot be read. Only the activation fence uses it: an
    /// unreadable record selects nothing a caller could have read.
    fn recorded_activation(&self) -> Option<FleetActivation> {
        let text = self
            .read_guarded(
                &self.state_root.join(FLEET_ACTIVATION_FILE),
                &ACTIVATION_GUARD,
            )
            .ok()?;
        serde_json::from_str(&text).ok()
    }

    /// The activation record and the exact policy it names, both verified.
    fn verify_activation(&self) -> Result<(FleetActivation, Arc<FleetSnapshot>), FleetError> {
        let text = self.read_guarded(
            &self.state_root.join(FLEET_ACTIVATION_FILE),
            &ACTIVATION_GUARD,
        )?;
        let record: FleetActivation = serde_json::from_str(&text).map_err(|_| invalid(A07))?;
        if record.schema_version != ACTIVATION_SCHEMA_VERSION {
            return Err(invalid(A07));
        }
        // The one activation decision, shared with every direct-mode reader:
        // only the bytes at the record's address, under the record's schema.
        let snapshot = FleetSnapshot::activated(
            &record.policy_hash,
            record.policy_schema_version,
            &self.read_published(&record.policy_hash)?,
        )?;
        Ok((record, Arc::new(snapshot)))
    }

    /// The published artifact for `hash`, re-read, re-hashed and re-validated.
    fn verify_published(&self, hash: &ContentHash) -> Result<FleetSnapshot, FleetError> {
        FleetSnapshot::published(hash, &self.read_published(hash)?)
    }

    /// The bytes published under `hash`, under the artifact's own file rules.
    fn read_published(&self, hash: &ContentHash) -> Result<String, FleetError> {
        match self.read_guarded(&self.published_path(hash), &POLICY_GUARD) {
            Err(FleetError::Read { source }) if source.kind() == std::io::ErrorKind::NotFound => {
                Err(invalid(A08))
            }
            other => other,
        }
    }

    fn published_path(&self, hash: &ContentHash) -> PathBuf {
        self.state_root
            .join(FLEET_HISTORY_DIR)
            .join(format!("{hash}.yml"))
    }

    /// Read one state-root file under its own refusal texts.
    fn read_guarded(&self, path: &Path, guard: &Guard) -> Result<String, FleetError> {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|source| FleetError::Read { source })?;
        if !metadata.file_type().is_file() {
            return Err(invalid(guard.symlink));
        }
        if metadata.mode() & 0o022 != 0 {
            return Err(invalid(guard.writable));
        }
        let root = std::fs::symlink_metadata(&self.state_root)
            .map_err(|source| FleetError::Read { source })?;
        if metadata.uid() != root.uid() {
            return Err(invalid(guard.owner));
        }
        if metadata.len() > guard.limit {
            return Err(invalid(guard.oversized));
        }
        let file = std::fs::File::open(path).map_err(|source| FleetError::Read { source })?;
        let opened = file
            .metadata()
            .map_err(|source| FleetError::Read { source })?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(invalid(guard.swapped));
        }
        let mut bytes = Vec::new();
        file.take(guard.limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| FleetError::Read { source })?;
        if bytes.len() as u64 > guard.limit {
            return Err(invalid(guard.oversized));
        }
        String::from_utf8(bytes).map_err(|_| invalid(guard.utf8))
    }

    fn write_history(&self, snapshot: &FleetSnapshot) {
        if let Err(error) = self.store_history(snapshot) {
            tracing::warn!(error = %error, "fleet.history_write_failed");
        }
    }

    fn store_history(&self, snapshot: &FleetSnapshot) -> std::io::Result<()> {
        let directory = self.state_root.join(FLEET_HISTORY_DIR);
        create_private_dir(&directory)?;
        let target = directory.join(format!("{}.yml", snapshot.hash()));
        if target.exists() {
            return Ok(());
        }
        let temporary = directory.join(format!("{}.yml.tmp", snapshot.hash()));
        let result = write_owner_only(&temporary, snapshot.raw().as_bytes())
            .and_then(|()| std::fs::rename(&temporary, &target));
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    fn write_status_file(&self, cache: &Cache) {
        self.write_status_logged(&StatusFile {
            active_hash: cache.good.as_ref().map(|good| good.hash().as_str()),
            loaded_at: cache.loaded_at.as_deref(),
            last_error: cache.last_error.as_deref(),
            checked_at: format_utc_timestamp(Timestamp::now()),
            selection: None,
        });
    }

    fn write_activation_status(&self, outcome: &ActivationOutcome) {
        let (active_hash, loaded_at, last_error) = match outcome {
            ActivationOutcome::Active { hash, activated_at } => {
                (Some(hash.as_str()), Some(activated_at.as_str()), None)
            }
            ActivationOutcome::Blocked(error) => (None, None, Some(error.as_str())),
        };
        self.write_status_logged(&StatusFile {
            active_hash,
            loaded_at,
            last_error,
            checked_at: format_utc_timestamp(Timestamp::now()),
            selection: Some("activation"),
        });
    }

    fn write_status_logged(&self, status: &StatusFile<'_>) {
        if let Err(error) = self.write_status(status) {
            tracing::warn!(error = %error, "fleet.status_write_failed");
        }
    }

    fn write_status(&self, status: &StatusFile<'_>) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(status).map_err(std::io::Error::other)?;
        let temporary =
            self.state_root
                .join(format!("{}.{}.tmp", FLEET_STATUS_FILE, std::process::id()));
        write_owner_only(&temporary, &bytes)?;
        std::fs::rename(&temporary, self.state_root.join(FLEET_STATUS_FILE))
    }
}

fn invalid(rule: &'static str) -> FleetError {
    FleetError::Invalid { rule }
}

fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn write_owner_only(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_core::spec::{ModelRef, ProviderRef};
    use kontor_fleet::rule::*;
    use std::os::unix::fs::PermissionsExt;

    const EXAMPLE: &str = include_str!("../../../config/examples/fleet.yml");

    fn write_fleet(root: &Path, contents: &str) -> PathBuf {
        let path = root.join(FLEET_FILE);
        std::fs::write(&path, contents).expect("write fleet.yml");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only mode");
        path
    }

    fn read_status(root: &Path) -> serde_json::Value {
        let bytes = std::fs::read(root.join(FLEET_STATUS_FILE)).expect("status file");
        serde_json::from_slice(&bytes).expect("status is JSON")
    }

    fn decision(binding_key: &str, vendor: &str) -> FleetDecision {
        FleetDecision {
            team_run_id: "run-1".to_owned(),
            agent_run_id: "agent-1".to_owned(),
            binding_key: binding_key.to_owned(),
            role_slot: "slot".to_owned(),
            fleet_hash: "hash".to_owned(),
            step: 1,
            sub_step: 1,
            provider: "provider".to_owned(),
            model: "model".to_owned(),
            effort: None,
            vendor: vendor.to_owned(),
            account_profile_id: None,
            decided_at: "2026-09-24T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn the_shipped_example_parses() {
        let snapshot = FleetSnapshot::parse(EXAMPLE).expect("the shipped example is valid");
        assert_eq!(snapshot.raw(), EXAMPLE);
        assert!(!snapshot.hash().as_str().is_empty());
    }

    #[test]
    fn each_rule_in_appendix_b_is_enforced() {
        let accounts: Vec<String> = std::iter::once("cursor".to_owned())
            .chain((1..=64).map(|index| format!("cursor-a{index}")))
            .collect();
        let cases: Vec<(&str, String)> = vec![
            (V01, EXAMPLE.replacen("schema_version: 1", "schema_version: 2", 1)),
            (
                V02,
                EXAMPLE.replacen("  claude:     { provider:", "  Claude:     { provider:", 1),
            ),
            (
                V03,
                EXAMPLE.replacen("provider: claude,   accounts", "provider: bogus,    accounts", 1),
            ),
            (
                V04,
                EXAMPLE.replacen(
                    "accounts: [claude-personal, claude-work]",
                    "accounts: []",
                    1,
                ),
            ),
            (
                V05,
                EXAMPLE.replacen(
                    "accounts: [claude-personal, claude-work]",
                    "accounts: [personal]",
                    1,
                ),
            ),
            (
                V06,
                EXAMPLE.replacen(
                    "accounts: [claude-personal, claude-work]",
                    "accounts: [claude-personal, claude-personal]",
                    1,
                ),
            ),
            (
                V07,
                EXAMPLE.replacen(", model_prefix: \"deepseek/\"", "", 1),
            ),
            (
                V08,
                EXAMPLE.replacen("  opus-5.5:       { domain:", "  Opus-5.5:       { domain:", 1),
            ),
            (
                V09,
                EXAMPLE.replacen("opus-5.5:       { domain: claude,", "opus-5.5:       { domain: nope,", 1),
            ),
            (
                V10,
                EXAMPLE.replacen(
                    "id: openrouter/z-ai/glm-5.3-flash,",
                    "id: z-ai/glm-5.3-flash,",
                    1,
                ),
            ),
            (V11, EXAMPLE.replacen("vendor: deepseek,", "vendor: DeepSeek,", 1)),
            (V12, EXAMPLE.replacen("max, ultra]", "max, bogus]", 1)),
            (
                V13,
                EXAMPLE.replacen("efforts: [low, high, max]", "efforts: [low, high, high]", 1),
            ),
            (
                V14,
                EXAMPLE.replacen(
                    "id: deepseek/deepseek-flash,",
                    "id: deepseek/deepseek-v4-pro,",
                    1,
                ),
            ),
            (V15, EXAMPLE.replacen("  review-a:", "  Review-a:", 1)),
            (
                V16,
                EXAMPLE.replacen("    - [sol@xhigh]", "    - []", 1),
            ),
            (V17, EXAMPLE.replace("- [sol@xhigh]", "- [soll@xhigh]")),
            (V18, EXAMPLE.replace("grok-4.7@xhigh", "grok-4.7@ultra")),
            (
                V19,
                EXAMPLE.replacen(
                    "    - [sol@xhigh]",
                    "    - [sol@xhigh, grok-4.7@xhigh]",
                    1,
                ),
            ),
            (
                V20,
                EXAMPLE.replacen(
                    "    - [grok-4.7@xhigh, grok-4.6@high]",
                    "    - [sol@xhigh]",
                    1,
                ),
            ),
            (
                V21,
                EXAMPLE.replacen(
                    "    - [grok-4.7@xhigh, grok-4.6@high]",
                    "    - [grok-4.7@xhigh, grok-4.7@xhigh]",
                    1,
                ),
            ),
            (
                V22,
                EXAMPLE.replace(
                    "accounts: [cursor]",
                    &format!("accounts: [{}]", accounts.join(", ")),
                ),
            ),
            (
                V23,
                EXAMPLE.replacen(
                    "  team/01936f5a-0000-7000-8000-000000000101/scope:",
                    "  core/01936f5a-0000-7000-8000-000000000101/scope:",
                    1,
                ),
            ),
            (
                V24,
                EXAMPLE.replacen(
                    "  team/01936f5a-0000-7000-8000-000000000101/scope: codex-first",
                    "  team/01936f5a-0000-7000-8000-000000000101/scope: nope",
                    1,
                ),
            ),
            (
                V25,
                EXAMPLE.replacen(
                    "    - [grok-4.7@xhigh, grok-4.6@high]",
                    "    - [grok-4.7@xhigh, grok-4.6@high, cursor-auto]",
                    1,
                ),
            ),
            (
                V25,
                EXAMPLE.replace(
                    "    - [composer-2.5, grok-4.5@high]",
                    "    - [composer-2.5, grok-4.5@high, cursor-auto]",
                ),
            ),
            (
                V26,
                EXAMPLE.replacen(
                    "domains: [openrouter, deepseek]",
                    "domains: [openrouter, deepseek, nope]",
                    1,
                ),
            ),
            (
                V27,
                EXAMPLE.replacen(
                    "accounts: [claude-work]",
                    "accounts: [claude-work, nope]",
                    1,
                ),
            ),
            (
                V28,
                EXAMPLE.replacen(
                    "  vision_required:",
                    "    - team/01936f5a-0000-7000-8000-000000000101/nope\n  vision_required:",
                    1,
                ),
            ),
            (
                V29,
                EXAMPLE.replacen(
                    "team/01936f5a-0000-7000-8000-000000000101/verify: team/01936f5a-0000-7000-8000-000000000101/implement",
                    "team/01936f5a-0000-7000-8000-000000000101/verify: team/01936f5a-0000-7000-8000-000000000101/verify",
                    1,
                ),
            ),
            (
                V30,
                EXAMPLE.replacen(
                    "team/01936f5a-0000-7000-8000-000000000101/verify: team/01936f5a-0000-7000-8000-000000000101/implement",
                    "team/01936f5a-0000-7000-8000-000000000101/verify: team/01936f5a-0000-7000-8000-000000000102/implement",
                    1,
                ),
            ),
        ];
        for (expected, yaml) in cases {
            match FleetSnapshot::parse(&yaml) {
                Err(FleetError::Invalid { rule }) => {
                    assert_eq!(rule, expected, "the first refusal must be the edited rule");
                }
                other => panic!("expected {expected}, got {other:?}"),
            }
        }
    }

    #[test]
    fn malformed_yaml_and_unknown_fields_are_document_errors() {
        assert!(matches!(
            FleetSnapshot::parse("schema_version: [1"),
            Err(FleetError::Document)
        ));
        let unknown = EXAMPLE.replace("schema_version: 1", "schema_version: 1\nunknown_section: 1");
        assert!(matches!(
            FleetSnapshot::parse(&unknown),
            Err(FleetError::Document)
        ));
    }

    #[test]
    fn flattening_is_model_major_then_account() {
        let yaml = "\
schema_version: 1
domains:
  claude: { provider: claude, accounts: [claude-personal, claude-work] }
  codex: { provider: codex, accounts: [codex-work] }
models:
  opus-5: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
  opus-4.8: { domain: claude, id: claude-opus-4-8, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
  sol: { domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }
chains:
  c:
    - [opus-5@xhigh, opus-4.8@xhigh]
    - [sol@xhigh]
bindings:
  team/t/s: c
";
        let snapshot = FleetSnapshot::parse(yaml).expect("valid document");
        let routes = snapshot.routes_for("team/t/s").expect("bound");
        let seen: Vec<(&str, &str, u16, u16)> = routes
            .iter()
            .map(|route| {
                (
                    route.rung.provider.0.as_str(),
                    route.rung.model.0.as_str(),
                    route.step,
                    route.sub_step,
                )
            })
            .collect();
        assert_eq!(
            seen,
            vec![
                ("claude-personal", "claude-opus-5", 1, 1),
                ("claude-work", "claude-opus-5", 1, 2),
                ("claude-personal", "claude-opus-4-8", 1, 3),
                ("claude-work", "claude-opus-4-8", 1, 4),
                ("codex-work", "gpt-5.6-sol", 2, 1),
            ]
        );
    }

    #[test]
    fn an_unavailable_domain_or_account_is_skipped() {
        let yaml = "\
schema_version: 1
domains:
  claude: { provider: claude, accounts: [claude-personal, claude-work] }
  codex: { provider: codex, accounts: [codex-work] }
unavailable:
  domains: [codex]
  accounts: [claude-work]
models:
  opus-5: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
  sol: { domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }
chains:
  c:
    - [sol@xhigh]
    - [opus-5@xhigh]
bindings:
  team/t/s: c
";
        let snapshot = FleetSnapshot::parse(yaml).expect("valid document");
        let routes = snapshot.routes_for("team/t/s").expect("bound");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].rung.provider.0, "claude-personal");
        assert_eq!(routes[0].step, 2);
        assert_eq!(routes[0].sub_step, 1);
    }

    #[test]
    fn calibration_and_vision_rules_drop_models() {
        let yaml = "\
schema_version: 1
domains:
  cursor: { provider: cursor, accounts: [cursor] }
models:
  grok-4.7: { domain: cursor, id: grok-4.7, vendor: xai, efforts: [xhigh], vision: true, calibrated: false }
  grok-4.6: { domain: cursor, id: grok-4.6, vendor: xai, efforts: [xhigh], vision: true, calibrated: true }
  composer: { domain: cursor, id: composer-2.5, vendor: cursor, vision: false, calibrated: false }
chains:
  c:
    - [grok-4.7@xhigh, grok-4.6@xhigh, composer]
bindings:
  team/t/s: c
  team/t/unlisted: c
rules:
  calibration_required: [team/t/s]
  vision_required: [team/t/s]
";
        let snapshot = FleetSnapshot::parse(yaml).expect("valid document");
        let routes = snapshot.routes_for("team/t/s").expect("bound");
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].rung.model.0, "grok-4.6");
        // Both rules are scoped to the keys they list. A seat outside them keeps
        // the uncalibrated and non-vision models, which is what tells a scoped
        // rule from one applied to every key.
        let unlisted = snapshot.routes_for("team/t/unlisted").expect("bound");
        assert_eq!(
            unlisted
                .iter()
                .map(|route| route.rung.model.0.as_str())
                .collect::<Vec<_>>(),
            ["grok-4.7", "grok-4.6", "composer-2.5"]
        );
    }

    #[test]
    fn an_unbound_key_returns_none() {
        let snapshot = FleetSnapshot::parse(EXAMPLE).expect("valid document");
        assert!(snapshot.routes_for("team/does-not-exist/slot").is_none());
    }

    #[test]
    fn an_unknown_vendor_has_no_independence_key() {
        let snapshot = FleetSnapshot::parse(EXAMPLE).expect("valid document");
        let rung = ModelRung {
            provider: ProviderRef("cursor".to_owned()),
            model: ModelRef("auto-smart".to_owned()),
            effort: None,
        };
        assert_eq!(snapshot.vendor_of(&rung), Some("unknown"));
        assert_eq!(independence_key(&rung, Some(&snapshot)), None);
    }

    #[test]
    fn a_listed_route_must_match_the_models_efforts() {
        use kontor_core::spec::EffortLevel;
        let snapshot = FleetSnapshot::parse(EXAMPLE).expect("valid document");
        let route = |model: &str, effort: Option<EffortLevel>| ModelRung {
            provider: ProviderRef("cursor".to_owned()),
            model: ModelRef(model.to_owned()),
            effort,
        };
        // `composer-2.5` declares no efforts, so only an effort-less route is it.
        assert!(snapshot.lists(&route("composer-2.5", None)));
        assert!(!snapshot.lists(&route("composer-2.5", Some(EffortLevel::High))));
        // `grok-4.7` declares efforts, so a route must name one of them.
        assert!(snapshot.lists(&route("grok-4.7", Some(EffortLevel::Xhigh))));
        assert!(!snapshot.lists(&route("grok-4.7", Some(EffortLevel::Max))));
        assert!(!snapshot.lists(&route("grok-4.7", None)));
    }

    #[test]
    fn without_a_fleet_claude_and_codex_map_to_their_vendor() {
        let claude = ModelRung {
            provider: ProviderRef("claude-work".to_owned()),
            model: ModelRef("claude-opus-5".to_owned()),
            effort: None,
        };
        let codex = ModelRung {
            provider: ProviderRef("codex-work".to_owned()),
            model: ModelRef("gpt-5.6-sol".to_owned()),
            effort: None,
        };
        assert_eq!(
            independence_key(&claude, None),
            Some("anthropic".to_owned())
        );
        assert_eq!(independence_key(&codex, None), Some("openai".to_owned()));
    }

    #[test]
    fn a_missing_file_means_no_fleet() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        assert!(source.current().is_none());
    }

    #[test]
    fn an_edit_is_picked_up_without_restart() {
        let root = tempfile::tempdir().expect("temporary state root");
        let first_yaml = "\
schema_version: 1
domains:
  codex: { provider: codex, accounts: [codex-work] }
  claude: { provider: claude, accounts: [claude-personal] }
models:
  sol: { domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }
  opus: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
chains:
  c:
    - [sol@xhigh]
    - [opus@xhigh]
bindings:
  team/t/s: c
";
        let second_yaml = "\
schema_version: 1
domains:
  codex: { provider: codex, accounts: [codex-work] }
  claude: { provider: claude, accounts: [claude-personal] }
models:
  sol: { domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }
  opus: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
chains:
  c:
    - [opus@xhigh]
    - [sol@xhigh]
bindings:
  team/t/s: c
";
        write_fleet(root.path(), first_yaml);
        let source = FleetSource::at(root.path());
        let first = source.current().expect("loaded");
        write_fleet(root.path(), second_yaml);
        let second = source.current().expect("reloaded");
        assert_ne!(first.hash(), second.hash());
        assert_eq!(
            second.routes_for("team/t/s").expect("bound")[0]
                .rung
                .provider
                .0,
            "claude-personal"
        );
    }

    #[test]
    fn an_invalid_edit_keeps_the_last_valid_snapshot() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), EXAMPLE);
        let source = FleetSource::at(root.path());
        let good = source.current().expect("loaded");
        write_fleet(root.path(), "schema_version: [");
        let after = source.current().expect("keeps the last valid snapshot");
        assert_eq!(after.hash(), good.hash());
    }

    #[test]
    fn a_group_writable_file_is_refused() {
        let root = tempfile::tempdir().expect("temporary state root");
        let path = root.path().join(FLEET_FILE);
        std::fs::write(&path, EXAMPLE).expect("write fleet.yml");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).expect("mode");
        let source = FleetSource::at(root.path());
        match source.load(&path) {
            Err(FleetError::Invalid { rule }) => assert_eq!(rule, F02),
            other => panic!("expected a group-writable refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_symlink_is_refused() {
        let root = tempfile::tempdir().expect("temporary state root");
        let real = root.path().join("real.yml");
        std::fs::write(&real, EXAMPLE).expect("write real.yml");
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).expect("mode");
        let link = root.path().join(FLEET_FILE);
        std::os::unix::fs::symlink(&real, &link).expect("symlink");
        let source = FleetSource::at(root.path());
        match source.load(&link) {
            Err(FleetError::Invalid { rule }) => assert_eq!(rule, F01),
            other => panic!("expected a symlink refusal, got {other:?}"),
        }
    }

    #[test]
    fn an_oversized_file_is_refused() {
        let root = tempfile::tempdir().expect("temporary state root");
        let path = write_fleet(root.path(), &"x".repeat(MAX_FILE_BYTES as usize + 1));
        let source = FleetSource::at(root.path());
        match source.load(&path) {
            Err(FleetError::Invalid { rule }) => assert_eq!(rule, F04),
            other => panic!("expected an oversized refusal, got {other:?}"),
        }
    }

    #[test]
    fn a_non_utf8_file_is_refused() {
        let root = tempfile::tempdir().expect("temporary state root");
        let path = root.path().join(FLEET_FILE);
        std::fs::write(&path, [0xff, 0xfe, 0xfd]).expect("write invalid UTF-8");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("mode");
        let source = FleetSource::at(root.path());
        match source.load(&path) {
            Err(FleetError::Invalid { rule }) => assert_eq!(rule, F06),
            other => panic!("expected a UTF-8 refusal, got {other:?}"),
        }
    }

    #[test]
    fn each_accepted_hash_is_written_to_history_once() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), EXAMPLE);
        let source = FleetSource::at(root.path());
        let first = source.current().expect("loaded");
        let history = root.path().join(FLEET_HISTORY_DIR);
        assert!(history.join(format!("{}.yml", first.hash())).is_file());
        source.current();
        assert_eq!(
            std::fs::read_dir(&history)
                .expect("history directory")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "yml"))
                .count(),
            1
        );
    }

    #[test]
    fn the_status_file_reports_the_last_error() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(
            root.path(),
            &EXAMPLE.replacen("schema_version: 1", "schema_version: 10", 1),
        );
        let source = FleetSource::at(root.path());
        assert!(source.current().is_none());
        let status = read_status(root.path());
        assert!(status["active_hash"].is_null());
        assert!(
            status["last_error"]
                .as_str()
                .is_some_and(|error| error.contains("schema_version must be 1"))
        );

        write_fleet(root.path(), EXAMPLE);
        let snapshot = source.current().expect("valid edit is accepted");
        let status = read_status(root.path());
        assert_eq!(
            status["active_hash"].as_str(),
            Some(snapshot.hash().as_str())
        );
        assert!(status["last_error"].is_null());
    }

    #[test]
    fn a_recorded_decision_is_the_last_vendor() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        source
            .record_decision(&decision("team/t/s", "anthropic"))
            .expect("record");
        assert_eq!(
            source.last_vendor("run-1", "team/t/s"),
            Some("anthropic".to_owned())
        );
        source
            .record_decision(&decision("team/t/s", "openai"))
            .expect("record");
        assert_eq!(
            source.last_vendor("run-1", "team/t/s"),
            Some("openai".to_owned())
        );
        assert_eq!(source.last_vendor("run-1", "team/other/s"), None);
    }

    #[test]
    fn every_rule_string_is_verbatim_from_appendix_b() {
        assert_eq!(F01, "fleet.yml must be a regular file, not a symlink");
        assert_eq!(F02, "fleet.yml must not be writable by group or others");
        assert_eq!(F03, "fleet.yml must be owned by the state root's owner");
        assert_eq!(F04, "fleet.yml exceeds 256 KiB");
        assert_eq!(F05, "fleet.yml changed while it was being read");
        assert_eq!(F06, "fleet.yml is not UTF-8");
        assert_eq!(V01, "schema_version must be 1");
        assert_eq!(V02, "a domain name must be a lowercase slug");
        assert_eq!(
            V03,
            "a domain provider must be claude, codex, cursor or opencode"
        );
        assert_eq!(V04, "a domain must list at least one account");
        assert_eq!(
            V05,
            "an account alias must be its domain's provider or start with the provider and a hyphen"
        );
        assert_eq!(V06, "a domain lists an account twice");
        assert_eq!(
            V07,
            "domains that share an account must each declare a different model prefix"
        );
        assert_eq!(V08, "a model name must be a lowercase slug");
        assert_eq!(V09, "a model names an unknown domain");
        assert_eq!(
            V10,
            "a model id must be non-empty and start with its domain's model prefix"
        );
        assert_eq!(V11, "a model vendor must be a lowercase slug");
        assert_eq!(
            V12,
            "a model effort is not in the runtime effort vocabulary"
        );
        assert_eq!(V13, "a model lists an effort twice");
        assert_eq!(V14, "a model route failed route validation");
        assert_eq!(V15, "a chain name must be a lowercase slug");
        assert_eq!(V16, "a chain must have 1 to 16 non-empty steps");
        assert_eq!(V17, "a chain entry names an unknown model");
        assert_eq!(
            V18,
            "a chain entry effort does not match the model's efforts"
        );
        assert_eq!(V19, "every entry in one step must use the same domain");
        assert_eq!(V20, "a chain uses the same domain in two steps");
        assert_eq!(V21, "a chain repeats a model");
        assert_eq!(V22, "a chain flattens to more than 64 routes");
        assert_eq!(
            V23,
            "a binding key must be team/<id>/<slot>, committee/<id>/<slot> or advisor/<id>"
        );
        assert_eq!(V24, "a binding names an unknown chain");
        assert_eq!(
            V25,
            "a Committee, Advisor or independent seat chain may not use a model with vendor unknown"
        );
        assert_eq!(V26, "an unavailable domain is not declared");
        assert_eq!(V27, "an unavailable account is not in any domain");
        assert_eq!(V28, "a rule names a seat key that has no binding");
        assert_eq!(V29, "a seat cannot be independent of itself");
        assert_eq!(
            V30,
            "independent_of pairs must be two seats of the same team template"
        );
    }

    // ---------------------------------------------------------------------
    // ASMA-8280: one activated policy, and nothing else, reaches placement.
    // ---------------------------------------------------------------------

    const V1_CLAUDE_FIRST: &str = "\
schema_version: 1
domains:
  codex: { provider: codex, accounts: [codex-work] }
  claude: { provider: claude, accounts: [claude-personal] }
models:
  sol: { domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }
  opus: { domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }
chains:
  c:
    - [opus@xhigh]
    - [sol@xhigh]
bindings:
  team/t/s: c
";

    fn codex_first() -> String {
        V1_CLAUDE_FIRST.replacen(
            "    - [opus@xhigh]\n    - [sol@xhigh]",
            "    - [sol@xhigh]\n    - [opus@xhigh]",
            1,
        )
    }

    fn first_provider(snapshot: &FleetSnapshot) -> String {
        snapshot.routes_for("team/t/s").expect("bound")[0]
            .rung
            .provider
            .0
            .clone()
    }

    fn activation_path(root: &Path) -> PathBuf {
        root.join(FLEET_ACTIVATION_FILE)
    }

    fn published(root: &Path, hash: &ContentHash) -> PathBuf {
        root.join(FLEET_HISTORY_DIR).join(format!("{hash}.yml"))
    }

    fn refusal(source: &FleetSource) -> &'static str {
        match source.policy() {
            Err(FleetError::Invalid { rule }) => rule,
            other => panic!("expected a fail-closed refusal, got {other:?}"),
        }
    }

    fn write_private(path: &Path, contents: &[u8]) {
        std::fs::write(path, contents).expect("write");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .expect("owner-only mode");
    }

    /// Publish and activate `yaml`, as the registered operation will.
    fn activate(source: &FleetSource, yaml: &str) -> ContentHash {
        let hash = source.publish(yaml).expect("publish").hash;
        let standing = source
            .recorded_activation()
            .map(|record| record.policy_hash);
        source.activate(&hash, standing.as_ref()).expect("activate");
        hash
    }

    #[test]
    fn an_activated_policy_replaces_fleet_yml_for_placement() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), V1_CLAUDE_FIRST);
        let source = FleetSource::at(root.path());
        assert_eq!(
            first_provider(&source.current().expect("legacy")),
            "claude-personal"
        );

        let hash = activate(&source, &codex_first());
        let active = source.policy().expect("verified").expect("activated");
        assert_eq!(active.hash(), &hash);
        assert_eq!(first_provider(&active), "codex-work");
        let status = read_status(root.path());
        assert_eq!(status["active_hash"].as_str(), Some(hash.as_str()));
        assert_eq!(status["selection"].as_str(), Some("activation"));
        assert!(status["last_error"].is_null());
    }

    #[test]
    fn an_unactivated_edit_or_publication_has_no_live_effect() {
        let root = tempfile::tempdir().expect("temporary state root");
        let checkout = tempfile::tempdir().expect("temporary authoring checkout");
        let authoring = checkout.path().join("fleet.yml");
        std::fs::write(&authoring, codex_first()).expect("author");
        let source = FleetSource::at(root.path());
        let activated = activate(
            &source,
            &std::fs::read_to_string(&authoring).expect("read authoring"),
        );

        // A dirty checkout, a branch switch, a hand edit of the state-root file
        // and a published-but-unactivated candidate: none of them is read.
        std::fs::write(&authoring, V1_CLAUDE_FIRST).expect("edit authoring");
        write_fleet(root.path(), V1_CLAUDE_FIRST);
        let candidate = source.publish(V1_CLAUDE_FIRST).expect("publish only").hash;
        assert_ne!(candidate, activated);

        for _ in 0..2 {
            let active = source.policy().expect("verified").expect("activated");
            assert_eq!(active.hash(), &activated);
            assert_eq!(first_provider(&active), "codex-work");
            assert_eq!(source.current().expect("activated").hash(), &activated);
        }

        // Only activation moves the next placement.
        source
            .activate(&candidate, Some(&activated))
            .expect("activate the candidate");
        assert_eq!(
            source
                .policy()
                .expect("verified")
                .expect("activated")
                .hash(),
            &candidate
        );
    }

    #[test]
    fn a_tampered_published_policy_fails_closed_without_falling_back() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), V1_CLAUDE_FIRST);
        let source = FleetSource::at(root.path());
        let hash = activate(&source, &codex_first());
        assert!(source.policy().is_ok());

        // Valid YAML, wrong bytes for its address.
        write_private(&published(root.path(), &hash), V1_CLAUDE_FIRST.as_bytes());
        assert_eq!(refusal(&source), P07);
        assert!(
            source.current().is_none(),
            "neither the activated snapshot nor fleet.yml is served"
        );
        let status = read_status(root.path());
        assert!(status["active_hash"].is_null());
        assert_eq!(status["selection"].as_str(), Some("activation"));
        assert!(
            status["last_error"]
                .as_str()
                .is_some_and(|error| error.contains(P07))
        );
    }

    #[test]
    fn a_missing_published_policy_fails_closed() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), V1_CLAUDE_FIRST);
        let source = FleetSource::at(root.path());
        let hash = activate(&source, &codex_first());
        std::fs::remove_file(published(root.path(), &hash)).expect("remove artifact");
        assert_eq!(refusal(&source), A08);
        assert!(source.current().is_none());
    }

    #[test]
    fn an_unsupported_activation_record_fails_closed() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), V1_CLAUDE_FIRST);
        let source = FleetSource::at(root.path());
        let hash = activate(&source, &codex_first());
        let record = |schema: u32, policy_schema: u32, extra: &str| {
            format!(
                "{{\"schema_version\": {schema}, \"policy_hash\": \"{hash}\", \
                 \"policy_schema_version\": {policy_schema}, \
                 \"activated_at\": \"2026-09-27T00:00:00Z\"{extra}}}"
            )
        };
        for (contents, expected) in [
            (record(2, 1, ""), A07),
            (record(1, 1, ", \"source\": \"checkout\""), A07),
            ("{\"schema_version\": 1}".to_owned(), A07),
            (record(1, 1, "").replace(hash.as_str(), "../fleet"), A07),
            (record(1, 2, ""), A09),
        ] {
            write_private(&activation_path(root.path()), contents.as_bytes());
            assert_eq!(refusal(&source), expected, "{contents}");
            assert!(source.current().is_none(), "{contents}");
        }
        write_private(&activation_path(root.path()), record(1, 1, "").as_bytes());
        assert_eq!(
            source
                .policy()
                .expect("verified")
                .expect("activated")
                .hash(),
            &hash,
            "a corrected record is read again at once"
        );
    }

    #[test]
    fn an_unsafe_activation_record_or_artifact_fails_closed() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        let hash = activate(&source, &codex_first());
        let record = activation_path(root.path());
        let good = std::fs::read(&record).expect("record");

        std::fs::set_permissions(&record, std::fs::Permissions::from_mode(0o664)).expect("mode");
        assert_eq!(refusal(&source), A02);
        write_private(&record, &[0xff, 0xfe]);
        assert_eq!(refusal(&source), A06);
        write_private(
            &record,
            " ".repeat(MAX_ACTIVATION_BYTES as usize + 1).as_bytes(),
        );
        assert_eq!(refusal(&source), A04);
        let real = root.path().join("record.json");
        write_private(&real, &good);
        std::fs::remove_file(&record).expect("remove record");
        std::os::unix::fs::symlink(&real, &record).expect("symlink");
        assert_eq!(refusal(&source), A01);

        std::fs::remove_file(&record).expect("remove symlink");
        write_private(&record, &good);
        assert!(source.policy().is_ok());
        let artifact = published(root.path(), &hash);
        std::fs::set_permissions(&artifact, std::fs::Permissions::from_mode(0o666)).expect("mode");
        assert_eq!(refusal(&source), P02);
    }

    #[test]
    fn a_refused_publication_or_activation_changes_nothing() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        let hash = activate(&source, &codex_first());
        let record = std::fs::read(activation_path(root.path())).expect("record");

        for invalid_policy in [
            codex_first().replacen("schema_version: 1", "schema_version: 3", 1),
            codex_first().replacen("team/t/s", "core/lsa", 1),
            "schema_version: [".to_owned(),
        ] {
            assert!(source.publish(&invalid_policy).is_err(), "{invalid_policy}");
        }
        match source.activate(&ContentHash::of(b"never published"), Some(&hash)) {
            Err(ActivationRefusal::Policy(FleetError::Invalid { rule })) => assert_eq!(rule, A08),
            other => panic!("expected an unpublished refusal, got {other:?}"),
        }

        assert_eq!(
            std::fs::read(activation_path(root.path())).expect("record"),
            record,
            "the previous activation stands"
        );
        let entries: Vec<String> = std::fs::read_dir(root.path().join(FLEET_HISTORY_DIR))
            .expect("history")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            entries,
            [format!("{hash}.yml")],
            "nothing else was published"
        );
    }

    #[test]
    fn the_activation_record_is_an_owner_only_atomic_projection() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        let hash = source.publish(&codex_first()).expect("publish").hash;
        let record = source.activate(&hash, None).expect("activate").record;
        assert_eq!(record.schema_version, 1);
        assert_eq!(record.policy_hash, hash);
        assert_eq!(record.policy_schema_version, 1);

        let path = activation_path(root.path());
        let metadata = std::fs::symlink_metadata(&path).expect("record");
        assert!(metadata.file_type().is_file());
        assert_eq!(metadata.mode() & 0o777, 0o600);
        let written: FleetActivation =
            serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("record JSON");
        assert_eq!(written, record);
        let artifact = std::fs::symlink_metadata(published(root.path(), &hash)).expect("artifact");
        assert_eq!(artifact.mode() & 0o777, 0o600);
        assert!(
            std::fs::read_dir(root.path())
                .expect("state root")
                .filter_map(Result::ok)
                .all(|entry| !entry.file_name().to_string_lossy().ends_with(".tmp")),
            "no temporary file is left behind"
        );
    }

    #[test]
    fn removing_the_activation_record_restores_the_unmigrated_behaviour() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), V1_CLAUDE_FIRST);
        let source = FleetSource::at(root.path());
        let legacy = source.current().expect("legacy").hash().clone();
        let activated = activate(&source, &codex_first());
        assert_eq!(source.current().expect("activated").hash(), &activated);

        std::fs::remove_file(activation_path(root.path())).expect("remove record");
        assert_eq!(source.current().expect("legacy again").hash(), &legacy);
        assert!(read_status(root.path()).get("selection").is_none());
    }

    #[test]
    fn one_activated_policy_resolves_leadership_delivery_and_consultation() {
        use kontor_core::id::SpecVersion;
        use kontor_fleet::LeadershipKey;
        use kontor_teams::CoreTeamRevision;

        let catalog = kontor_profiles::seeds::bundled_operational_domain()
            .expect("the bundled domain loads")
            .role_catalogs
            .remove(0);
        let roster =
            CoreTeamRevision::resolve(SpecVersion::FIRST, &catalog, &[]).expect("LSA and TPM");
        let pinned = roster.canonicalize().expect("canonical roster");
        let keys: Vec<LeadershipKey> = roster
            .seats
            .iter()
            .map(|seat| {
                LeadershipKey::for_pinned_seat(&pinned, &seat.role_slot_id, &seat.role)
                    .expect("the pinned seat proves its key")
            })
            .collect();
        assert_eq!(
            keys.iter()
                .map(|key| key.role_slot_id().as_str())
                .collect::<Vec<_>>(),
            ["lsa", "tpm"]
        );
        let delivery = team_key("01936f5a-0000-7000-8000-000000000102", "implement");
        let reviewer = committee_key("01991c00-0000-7000-8000-000000000001", "reviewer-a");
        let advisor = advisor_key("01a02d00-0000-7000-8000-00000000ad01");
        let yaml = format!(
            "\
schema_version: 2
domains:
  codex: {{ provider: codex, accounts: [codex-work] }}
  claude: {{ provider: claude, accounts: [claude-personal] }}
models:
  sol: {{ domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }}
  opus: {{ domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }}
chains:
  codex-first:
    - [sol@xhigh]
    - [opus@xhigh]
  claude-first:
    - [opus@xhigh]
    - [sol@xhigh]
bindings:
  {lsa}: codex-first
  {tpm}: claude-first
  {delivery}: claude-first
  {reviewer}: codex-first
  {advisor}: claude-first
",
            lsa = keys[0],
            tpm = keys[1],
        );
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        let hash = activate(&source, &yaml);
        let record: FleetActivation =
            serde_json::from_slice(&std::fs::read(activation_path(root.path())).expect("record"))
                .expect("record JSON");
        let policy = source.policy().expect("verified").expect("activated");

        let resolutions = [
            policy.resolve_leadership(&keys[0]).expect("LSA"),
            policy.resolve_leadership(&keys[1]).expect("TPM"),
            policy.resolve(&delivery).expect("delivery"),
            policy.resolve(&reviewer).expect("committee"),
            policy.resolve(&advisor).expect("advisor"),
        ];
        let expected = [
            (keys[0].as_str(), "codex-first", "codex-work"),
            (keys[1].as_str(), "claude-first", "claude-personal"),
            (delivery.as_str(), "claude-first", "claude-personal"),
            (reviewer.as_str(), "codex-first", "codex-work"),
            (advisor.as_str(), "claude-first", "claude-personal"),
        ];
        for (resolution, (key, chain, first)) in resolutions.iter().zip(expected) {
            // Every slot class names the same activated bytes, its own key and
            // its own chain; none is inferred from anything but the policy.
            assert_eq!(resolution.provenance.policy_hash, hash, "{key}");
            assert_eq!(
                resolution.provenance.policy_hash, record.policy_hash,
                "{key}"
            );
            assert_eq!(resolution.provenance.schema_version, 2, "{key}");
            assert_eq!(resolution.provenance.binding_key, key);
            assert_eq!(resolution.provenance.chain, chain, "{key}");
            assert_eq!(resolution.routes[0].rung.provider.0, first, "{key}");
        }
        assert!(
            policy.routes_for(keys[0].as_str()).is_none(),
            "the leadership key is not resolvable from its text"
        );
    }

    #[test]
    fn activation_is_fenced_on_the_policy_the_caller_read() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        let first = source.publish(&codex_first()).expect("publish");
        assert!(first.created);
        assert!(
            !source.publish(&codex_first()).expect("republish").created,
            "publishing the same bytes again writes nothing"
        );
        let second = source.publish(V1_CLAUDE_FIRST).expect("publish").hash;

        assert!(matches!(
            source.activate(&first.hash, Some(&second)),
            Err(ActivationRefusal::Moved)
        ));
        let activated = source.activate(&first.hash, None).expect("activate");
        assert!(activated.changed);
        assert!(matches!(
            source.activate(&second, None),
            Err(ActivationRefusal::Moved)
        ));
        let record = std::fs::read(activation_path(root.path())).expect("record");

        // Selecting what is already selected is the replay answer, whatever the
        // caller last read, and it rewrites nothing.
        let again = source
            .activate(&first.hash, Some(&second))
            .expect("already active");
        assert!(!again.changed);
        assert_eq!(again.record, activated.record);
        assert_eq!(
            std::fs::read(activation_path(root.path())).expect("record"),
            record
        );

        let moved = source
            .activate(&second, Some(&first.hash))
            .expect("activate the second");
        assert!(moved.changed);
        assert_eq!(moved.record.policy_hash, second);
    }

    #[test]
    fn the_status_names_the_selection_and_why_it_cannot_be_served() {
        let root = tempfile::tempdir().expect("temporary state root");
        write_fleet(root.path(), V1_CLAUDE_FIRST);
        let source = FleetSource::at(root.path());
        let legacy = source.status();
        assert!(!legacy.activation);
        assert!(legacy.record.is_none());
        assert!(legacy.active.is_some());
        assert!(legacy.refusal.is_none());

        let hash = activate(&source, &codex_first());
        let active = source.status();
        assert!(active.activation);
        assert_eq!(active.record.expect("record").policy_hash, hash);
        assert_eq!(active.active.expect("activated").hash(), &hash);

        std::fs::remove_file(published(root.path(), &hash)).expect("remove artifact");
        let blocked = source.status();
        assert!(blocked.activation);
        assert!(blocked.active.is_none(), "fleet.yml is not served instead");
        assert!(blocked.refusal.is_some_and(|refusal| refusal.contains(A08)));
    }

    #[test]
    fn a_leadership_decision_is_appended_per_seat_binding() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        let decision = |generation: u64| LeadershipDecision {
            operation: "materialize_core_team".to_owned(),
            project_id: "project".to_owned(),
            epic_id: "epic".to_owned(),
            seat_binding_id: "binding-1".to_owned(),
            occupancy_generation: generation,
            core_team_revision_hash: "roster".to_owned(),
            core_team_version: 1,
            role_slot_id: "lsa".to_owned(),
            binding_key: "leadership/roster/lsa".to_owned(),
            fleet_hash: "policy".to_owned(),
            policy_schema_version: 2,
            chain: "codex-first".to_owned(),
            step: 1,
            sub_step: 1,
            provider: "codex-work".to_owned(),
            model: "gpt-5.6-sol".to_owned(),
            effort: Some("xhigh".to_owned()),
            vendor: "openai".to_owned(),
            decided_at: "2026-09-27T00:00:00Z".to_owned(),
        };
        source
            .record_leadership_decision(&decision(1))
            .expect("record");
        source
            .record_leadership_decision(&decision(2))
            .expect("record");
        let path = root
            .path()
            .join(FLEET_DECISIONS_DIR)
            .join(LEADERSHIP_DECISIONS_DIR)
            .join("binding-1.jsonl");
        let lines: Vec<LeadershipDecision> = std::fs::read_to_string(&path)
            .expect("the log")
            .lines()
            .map(|line| serde_json::from_str(line).expect("a decision line"))
            .collect();
        assert_eq!(lines, [decision(1), decision(2)]);
        assert_eq!(
            std::fs::symlink_metadata(&path).expect("log").mode() & 0o777,
            0o600
        );
        assert!(
            source
                .last_vendor("binding-1", "leadership/roster/lsa")
                .is_none(),
            "leadership decisions are not a TeamRun's delivery log"
        );
    }

    /// ASMA-8280 direct mode: a reader that follows only the activation
    /// record's two files through `kontor-fleet` — what a Paseo-direct consumer
    /// does without the daemon — chooses exactly what governed placement
    /// chooses, for leadership, delivery and consultation keys alike, and its
    /// choice is a receipt that names the activated bytes and carries no
    /// secret material.
    #[test]
    fn a_direct_reader_of_the_activation_chooses_what_placement_chooses() {
        use kontor_core::id::{CanonicalDocument, SpecVersion};
        use kontor_fleet::{Eligibility, LeadershipKey};
        use kontor_teams::CoreTeamRevision;
        use std::collections::BTreeSet;

        let catalog = kontor_profiles::seeds::bundled_operational_domain()
            .expect("the bundled domain loads")
            .role_catalogs
            .remove(0);
        let roster =
            CoreTeamRevision::resolve(SpecVersion::FIRST, &catalog, &[]).expect("LSA and TPM");
        let pinned = roster.canonicalize().expect("canonical roster");
        let leadership: Vec<LeadershipKey> = roster
            .seats
            .iter()
            .map(|seat| {
                LeadershipKey::for_pinned_seat(&pinned, &seat.role_slot_id, &seat.role)
                    .expect("the pinned seat proves its key")
            })
            .collect();
        let delivery = team_key("01936f5a-0000-7000-8000-000000000102", "implement");
        let reviewer = committee_key("01991c00-0000-7000-8000-000000000001", "reviewer-a");
        let advisor = advisor_key("01a02d00-0000-7000-8000-00000000ad01");
        let policy = |first: &str, second: &str| {
            format!(
                "schema_version: 2\n\
                 domains:\n  codex: {{ provider: codex, accounts: [codex-work, codex-personal] }}\n  claude: {{ provider: claude, accounts: [claude-personal] }}\n\
                 models:\n  sol: {{ domain: codex, id: gpt-5.6-sol, vendor: openai, efforts: [xhigh], vision: true, calibrated: true }}\n  opus: {{ domain: claude, id: claude-opus-5, vendor: anthropic, efforts: [xhigh], vision: true, calibrated: true }}\n\
                 chains:\n  lead:\n    - [{first}@xhigh]\n    - [{second}@xhigh]\n\
                 bindings:\n  {lsa}: lead\n  {tpm}: lead\n  {delivery}: lead\n  {reviewer}: lead\n  {advisor}: lead\n",
                lsa = leadership[0],
                tpm = leadership[1],
            )
        };
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        let hash = activate(&source, &policy("sol", "opus"));

        // What a direct reader holds: the two files, and nothing of the daemon.
        let direct = || {
            let record: serde_json::Value = serde_json::from_slice(
                &std::fs::read(activation_path(root.path())).expect("the activation record"),
            )
            .expect("the record is JSON");
            let named = ContentHash::parse(record["policy_hash"].as_str().expect("a hash"))
                .expect("a content hash");
            let schema = u32::try_from(record["policy_schema_version"].as_u64().expect("a schema"))
                .expect("a schema version");
            let bytes = std::fs::read_to_string(published(root.path(), &named))
                .expect("the activated artifact");
            FleetSnapshot::activated(&named, schema, &bytes)
        };
        let eligibilities = [
            Eligibility::default(),
            Eligibility {
                unavailable_accounts: BTreeSet::from(["codex-work".to_owned()]),
                excluded_vendors: BTreeSet::new(),
            },
            Eligibility {
                unavailable_accounts: BTreeSet::new(),
                excluded_vendors: BTreeSet::from(["openai".to_owned()]),
            },
        ];
        let compare = |expected_hash: &ContentHash| {
            let governed = source.policy().expect("verified").expect("activated");
            let direct = direct().expect("the direct reader admits the activation");
            assert_eq!(direct.hash(), expected_hash);
            assert_eq!(governed.hash(), direct.hash(), "one activated hash");
            for eligibility in &eligibilities {
                let pairs = leadership
                    .iter()
                    .map(|key| {
                        (
                            governed.resolve_leadership(key).expect("leadership"),
                            direct.resolve_leadership(key).expect("leadership"),
                        )
                    })
                    .chain([&delivery, &reviewer, &advisor].into_iter().map(|key| {
                        (
                            governed.resolve(key).expect("bound"),
                            direct.resolve(key).expect("bound"),
                        )
                    }));
                for (governed, direct) in pairs {
                    let (governed, direct) =
                        (governed.select(eligibility), direct.select(eligibility));
                    assert_eq!(governed, direct, "{eligibility:?}");
                    assert_eq!(&direct.provenance.policy_hash, expected_hash);
                    // The receipt is canonical evidence: no credential, token or
                    // provider home survives into it.
                    CanonicalDocument::from_serializable(&serde_json::json!({
                        "schema_version": 1,
                        "selection": direct,
                    }))
                    .expect("the selection is admissible evidence");
                }
            }
        };
        compare(&hash);
        let chosen = direct()
            .expect("admitted")
            .resolve_leadership(&leadership[0])
            .expect("LSA")
            .select(&Eligibility::default())
            .selected
            .expect("a route");
        assert_eq!(
            (
                chosen.rung.provider.0.as_str(),
                chosen.step,
                chosen.sub_step
            ),
            ("codex-work", 1, 1),
            "the policy, not the caller, chooses the route"
        );

        // Unactivated edits: fleet.yml, a published candidate, a checkout copy.
        write_fleet(root.path(), V1_CLAUDE_FIRST);
        let candidate = source
            .publish(&policy("opus", "sol"))
            .expect("publish only")
            .hash;
        assert_ne!(candidate, hash);
        compare(&hash);

        // A rewritten artifact fails closed for both readers alike.
        write_private(
            &published(root.path(), &hash),
            policy("opus", "sol").as_bytes(),
        );
        assert!(matches!(
            direct(),
            Err(FleetError::Invalid { rule }) if rule == P07
        ));
        assert_eq!(refusal(&source), P07);
        write_private(
            &published(root.path(), &hash),
            policy("sol", "opus").as_bytes(),
        );

        // Only activation moves both readers, and both move together.
        source
            .activate(&candidate, Some(&hash))
            .expect("activate the candidate");
        compare(&candidate);
    }
}
