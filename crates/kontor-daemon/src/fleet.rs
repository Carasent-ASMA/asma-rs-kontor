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

use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use kontor_core::id::{CanonicalDocument, ContentHash, Timestamp, format_utc_timestamp};
use kontor_core::spec::ModelRung;
use kontor_fleet::MAX_FILE_BYTES;
use kontor_fleet::rule::F04;
use kontor_fleet_activation::{
    ACTIVATION_V1, ACTIVATION_V2, BundleManifest, CORE_TEAM_HISTORY_DIR, Guard,
    ORCHESTRATION_HISTORY_DIR,
};
use serde::{Deserialize, Serialize};

pub use kontor_fleet::{FleetError, FleetSnapshot};
pub(crate) use kontor_fleet::{advisor_key, committee_key, independence_key, team_key};
// The record, its file name and the history directory are the shared reader's:
// one spelling for the daemon and the CLI's local resolution (ASMA-8280 B-1).
pub use kontor_fleet_activation::FleetActivation;
pub(crate) use kontor_fleet_activation::{FLEET_ACTIVATION_FILE, FLEET_HISTORY_DIR};

/// The fleet configuration file name inside a Realm state root.
pub(crate) const FLEET_FILE: &str = "fleet.yml";

/// One JSON-lines decision log per team run.
pub(crate) const FLEET_DECISIONS_DIR: &str = "fleet-decisions";

/// Whether the last edit was accepted, and why not when it was not.
pub(crate) const FLEET_STATUS_FILE: &str = "fleet-status.json";

/// One JSON-lines leadership decision log per SeatBinding, inside
/// [`FLEET_DECISIONS_DIR`].
pub(crate) const LEADERSHIP_DECISIONS_DIR: &str = "leadership";

/// One JSON-lines launch provenance log per launched subject, inside
/// [`FLEET_DECISIONS_DIR`]: what each launch requested and what the runtime
/// natively observed (ASMA-8280 G-3).
pub(crate) const LAUNCH_PROVENANCE_DIR: &str = "launches";

const F01: &str = "fleet.yml must be a regular file, not a symlink";
const F02: &str = "fleet.yml must not be writable by group or others";
const F03: &str = "fleet.yml must be owned by the state root's owner";
const F05: &str = "fleet.yml changed while it was being read";
const F06: &str = "fleet.yml is not UTF-8";

/// The refusal that means "no such published policy", which the read and
/// activate operations answer as not found rather than as invalid.
pub(crate) const UNPUBLISHED_POLICY: &str = kontor_fleet_activation::rule::A08;

/// The unmigrated `fleet.yml`'s own file rules, read by the shared guarded
/// reader so every state-root file is checked the same way.
const FLEET_GUARD: Guard = Guard {
    symlink: F01,
    writable: F02,
    owner: F03,
    oversized: F04,
    swapped: F05,
    utf8: F06,
    limit: MAX_FILE_BYTES,
};

/// The rungs one delivery seat may walk, and the fleet binding they came from.
///
/// `fleet` is `Some` exactly when the rungs came from the live fleet snapshot
/// rather than the frozen template chain: it carries that snapshot, the
/// binding key and its resolution, so the placement is chosen by the shared
/// resolver and recorded against the exact version that authorised it.
#[derive(Debug)]
pub(crate) struct DeclaredRungs {
    pub(crate) rungs: Vec<ModelRung>,
    pub(crate) fleet: Option<FleetBinding>,
}

/// One delivery seat a fleet policy binds (ASMA-8280 G-4).
#[derive(Debug)]
pub(crate) struct FleetBinding {
    /// The snapshot the binding was resolved against.
    pub(crate) snapshot: Arc<FleetSnapshot>,
    /// The canonical binding key.
    pub(crate) binding_key: String,
    /// The bound chain, flattened, with its provenance.
    pub(crate) resolution: kontor_fleet::FleetResolution,
    /// The vendor the seat must avoid under `rules.independent_of`: part of
    /// the explicit eligibility the placement is chosen under.
    pub(crate) excluded_vendors: std::collections::BTreeSet<String>,
    /// The orchestration bundle an aligned activation names.
    pub(crate) source_bundle_hash: Option<ContentHash>,
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
pub struct Activated {
    /// The standing activation record.
    pub record: FleetActivation,
    /// Whether this call replaced the record rather than finding it.
    pub changed: bool,
}

/// Why an activation changed nothing.
#[derive(Debug)]
pub enum ActivationRefusal {
    /// The published artifact did not verify, or the record was not written.
    Policy(FleetError),
    /// The record selects another policy than the one the caller read.
    Moved,
}

/// The standing selection an activation must name to replace it: the policy
/// and, for an aligned activation, the bundle the caller read. `None` in both
/// is "no activation record".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivationFence {
    /// The policy the standing record names.
    pub policy_hash: Option<ContentHash>,
    /// The bundle the standing record names, for a schema_version 2 record.
    pub source_bundle_hash: Option<ContentHash>,
}

impl ActivationFence {
    fn of(record: Option<&FleetActivation>) -> Self {
        Self {
            policy_hash: record.map(|record| record.policy_hash.clone()),
            source_bundle_hash: record.and_then(|record| record.source_bundle_hash.clone()),
        }
    }
}

/// What one bundle publication left published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishedBundle {
    /// The canonical manifest's content hash: the bundle's identity.
    pub bundle_hash: ContentHash,
    /// The manifest as written.
    pub manifest: BundleManifest,
    /// Whether this call wrote any artifact rather than finding all of them.
    pub created: bool,
}

/// What placement reads: the policy, and for an aligned activation the exact
/// canonical Core Team revision it selects.
#[derive(Debug, Clone)]
pub(crate) struct Placement {
    /// The policy a seat binding resolves against.
    pub(crate) policy: Arc<FleetSnapshot>,
    /// `Some` exactly for a schema_version 2 activation: the roster whose hash
    /// every leadership binding names. A governed leadership launch confirms
    /// its pinned revision against these bytes before it resolves a route.
    pub(crate) roster: Option<Arc<CanonicalDocument>>,
    /// The bundle a schema_version 2 activation names, for the evidence.
    pub(crate) source_bundle_hash: Option<ContentHash>,
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
    /// The orchestration bundle an aligned activation names; absent under a
    /// schema_version 1 activation, so those rows keep their exact shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_bundle_hash: Option<String>,
    /// The eligibility the policy chose this route under, when the caller
    /// named no route; absent for an admitted caller route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) eligibility: Option<kontor_fleet::Eligibility>,
}

/// One launch's fleet provenance: what Kontor requested and what the runtime
/// natively observed, kept apart (ASMA-8280 G-3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LaunchProvenanceRecord {
    /// `hosted_leadership`, `delivery` or `consultation`.
    pub(crate) launch: String,
    /// The launched subject: the SeatBinding, or the delivery agent run.
    pub(crate) subject: String,
    /// The native session the launch produced.
    pub(crate) native_id: String,
    /// What the launch requested, when the fleet policy chose its route.
    pub(crate) requested: Option<kontor_runtime::FleetLaunchProvenance>,
    /// What the runtime read back from its native surface, or why it could
    /// not: never a copy of `requested`.
    pub(crate) observed: kontor_runtime::FleetProvenanceObservation,
    /// Whether `observed` proves `requested`. An unsupported surface proves
    /// nothing and is recorded as such.
    pub(crate) proven: bool,
    pub(crate) recorded_at: String,
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
    /// The chain the binding resolved to (ASMA-8280 G-3). Absent only on
    /// rows written before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) chain: Option<String>,
    /// The orchestration bundle an aligned activation named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source_bundle_hash: Option<String>,
    pub(crate) step: u16,
    pub(crate) sub_step: u16,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) effort: Option<String>,
    pub(crate) vendor: String,
    pub(crate) account_profile_id: Option<String>,
    pub(crate) decided_at: String,
    /// The explicit eligibility the shared resolver chose this route under,
    /// translated from the exact quota observation (ASMA-8280 G-4). Absent
    /// only on rows written before it was recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) eligibility: Option<kontor_fleet::Eligibility>,
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
        Ok(self.placement()?.map(|placement| placement.policy))
    }

    /// The policy placement reads and, under an aligned activation, the Core
    /// Team revision it selects — both from one verified read, so a leadership
    /// launch never pairs a policy with a roster from another activation.
    ///
    /// # Errors
    /// As [`Self::policy`].
    /// Place one planning pair's two members (ASMA-8282) through the one
    /// verified reader and the shared allocator, on one activated snapshot.
    ///
    /// There is no `fleet.yml` and no last-valid fallback: a planning pair is
    /// placed only from an activation, exactly as the direct CLI places one.
    pub(crate) fn planning_pair_placement(
        &self,
        request: &kontor_fleet_activation::PlanningPairRequest,
    ) -> Result<kontor_fleet_activation::PlanningPairPlacement, FleetError> {
        kontor_fleet_activation::place_planning_pair(&self.state_root, request)
    }

    pub(crate) fn placement(&self) -> Result<Option<Placement>, FleetError> {
        let mut cache = self.lock();
        if let Err(error) = std::fs::symlink_metadata(self.state_root.join(FLEET_ACTIVATION_FILE))
            && error.kind() == std::io::ErrorKind::NotFound
        {
            if cache.activation.is_some() {
                // The unmigrated file is read afresh, not from a stamp taken
                // before the activation record appeared.
                *cache = Cache::default();
            }
            return Ok(self.legacy(&mut cache).map(|policy| Placement {
                policy,
                roster: None,
                source_bundle_hash: None,
            }));
        }
        // The one verified reader, shared with the CLI's local resolution:
        // the record, then the bundle manifest it names, then exactly the
        // policy and roster that manifest names. Nothing else is read.
        let verified = kontor_fleet_activation::load(&self.state_root);
        let outcome = match &verified {
            Ok(activated) => ActivationOutcome::Active {
                hash: activated.record.policy_hash.clone(),
                activated_at: activated.record.activated_at.clone(),
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
        verified.map(|activated| {
            Some(Placement {
                policy: Arc::new(activated.policy),
                roster: activated.bundle.map(|bundle| Arc::new(bundle.roster)),
                source_bundle_hash: activated.record.source_bundle_hash,
            })
        })
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
        let created = if std::fs::symlink_metadata(kontor_fleet_activation::policy_path(
            &self.state_root,
            snapshot.hash(),
        ))
        .is_ok()
        {
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
            schema_version: ACTIVATION_V1,
            source_bundle_hash: None,
            policy_hash: hash.clone(),
            policy_schema_version: snapshot.schema_version(),
            core_team_revision_hash: None,
            activated_at: format_utc_timestamp(Timestamp::now()),
        };
        self.replace_activation(&record)?;
        Ok(Activated {
            record,
            changed: true,
        })
    }

    /// Publish one resolved orchestration bundle: its policy, its canonical
    /// Core Team revision and its manifest, each immutable and addressed by
    /// its own content hash.
    ///
    /// Every artifact already present is re-verified rather than rewritten,
    /// and the bundle is then read back in full through the shared reader, so
    /// a publication that returns has left exactly what an activation will
    /// verify. Publication selects nothing; only [`Self::activate_bundle`]
    /// does.
    ///
    /// # Errors
    /// The first artifact that does not verify, or the write failure.
    pub fn publish_bundle(
        &self,
        bundle: &crate::orchestration::ResolvedBundle,
    ) -> Result<PublishedBundle, FleetError> {
        let policy = self.publish(bundle.policy.raw())?;
        let roster_path =
            kontor_fleet_activation::roster_path(&self.state_root, bundle.roster.hash());
        let roster_created = if std::fs::symlink_metadata(&roster_path).is_ok() {
            kontor_fleet_activation::verify_roster(&self.state_root, bundle.roster.hash())?;
            false
        } else {
            store_immutable(
                &self.state_root.join(CORE_TEAM_HISTORY_DIR),
                &roster_path,
                bundle.roster.json().as_bytes(),
            )
            .map_err(|source| FleetError::Write { source })?;
            true
        };
        let manifest = bundle.manifest.canonicalize()?;
        let manifest_path =
            kontor_fleet_activation::manifest_path(&self.state_root, manifest.hash());
        let manifest_created = if std::fs::symlink_metadata(&manifest_path).is_ok() {
            false
        } else {
            store_immutable(
                &self.state_root.join(ORCHESTRATION_HISTORY_DIR),
                &manifest_path,
                manifest.json().as_bytes(),
            )
            .map_err(|source| FleetError::Write { source })?;
            true
        };
        let verified = kontor_fleet_activation::verify_bundle(&self.state_root, manifest.hash())?;
        Ok(PublishedBundle {
            bundle_hash: manifest.hash().clone(),
            manifest: verified.manifest,
            created: policy.created || roster_created || manifest_created,
        })
    }

    /// Select one published bundle — its policy and its Core Team roster
    /// together — for every later placement, with a schema_version 2 record.
    ///
    /// `expected` is the standing selection the caller read; a record naming
    /// anything else refuses with [`ActivationRefusal::Moved`]. Selecting the
    /// bundle already active re-verifies it and writes nothing. Otherwise every
    /// artifact the bundle names is verified before the one pointer is
    /// replaced atomically; a refused activation changes nothing. Running and
    /// frozen epics keep their pins: this changes only what the next placement
    /// reads.
    ///
    /// # Errors
    /// As [`Self::activate`].
    pub fn activate_bundle(
        &self,
        bundle_hash: &ContentHash,
        expected: &ActivationFence,
    ) -> Result<Activated, ActivationRefusal> {
        let _cache = self.lock();
        let standing = self.recorded_activation();
        if let Some(record) = standing.as_ref()
            && record.source_bundle_hash.as_ref() == Some(bundle_hash)
        {
            kontor_fleet_activation::verify_bundle(&self.state_root, bundle_hash)
                .map_err(ActivationRefusal::Policy)?;
            return Ok(Activated {
                record: record.clone(),
                changed: false,
            });
        }
        if &ActivationFence::of(standing.as_ref()) != expected {
            return Err(ActivationRefusal::Moved);
        }
        let verified = kontor_fleet_activation::verify_bundle(&self.state_root, bundle_hash)
            .map_err(ActivationRefusal::Policy)?;
        let record = FleetActivation {
            schema_version: ACTIVATION_V2,
            source_bundle_hash: Some(bundle_hash.clone()),
            policy_hash: verified.manifest.policy_hash.clone(),
            policy_schema_version: verified.manifest.policy_schema_version,
            core_team_revision_hash: Some(verified.manifest.core_team_revision_hash.clone()),
            activated_at: format_utc_timestamp(Timestamp::now()),
        };
        self.replace_activation(&record)?;
        Ok(Activated {
            record,
            changed: true,
        })
    }

    /// One published bundle, verified in full through the shared reader:
    /// manifest, policy, roster and catalog pin.
    ///
    /// # Errors
    /// The first check that fails.
    pub(crate) fn verify_bundle(
        &self,
        bundle_hash: &ContentHash,
    ) -> Result<kontor_fleet_activation::VerifiedBundle, FleetError> {
        kontor_fleet_activation::verify_bundle(&self.state_root, bundle_hash)
    }

    /// One published bundle manifest, verified as a canonical manifest at its
    /// content address. The read operation reports it beside the record.
    ///
    /// # Errors
    /// The first check that fails.
    pub(crate) fn bundle_manifest(
        &self,
        bundle_hash: &ContentHash,
    ) -> Result<BundleManifest, FleetError> {
        kontor_fleet_activation::verify_manifest(&self.state_root, bundle_hash)
    }

    /// Replace the one activation pointer atomically, owner-only.
    fn replace_activation(&self, record: &FleetActivation) -> Result<(), ActivationRefusal> {
        let bytes = serde_json::to_vec_pretty(record).map_err(|error| {
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

    /// The decision recorded for one agent run of one team run, when one was:
    /// the fleet policy's authority a delivery launch names (ASMA-8280 G-3).
    pub(crate) fn decision_for(
        &self,
        team_run_id: &str,
        agent_run_id: &str,
    ) -> Option<FleetDecision> {
        let path = self
            .state_root
            .join(FLEET_DECISIONS_DIR)
            .join(format!("{team_run_id}.jsonl"));
        let text = std::fs::read_to_string(path).ok()?;
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str::<FleetDecision>(line).ok())
            .rfind(|decision| decision.agent_run_id == agent_run_id)
    }

    /// Append what one launch requested and what its runtime observed.
    ///
    /// Written after the native effect, which has happened either way; a
    /// failure to record is logged rather than undone, and leaves the launch
    /// without recorded proof rather than with invented proof.
    pub(crate) fn record_launch_provenance(&self, record: &LaunchProvenanceRecord) {
        let directory = self
            .state_root
            .join(FLEET_DECISIONS_DIR)
            .join(LAUNCH_PROVENANCE_DIR);
        let written = create_private_dir(&self.state_root.join(FLEET_DECISIONS_DIR))
            .and_then(|()| create_private_dir(&directory))
            .and_then(|()| {
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create(true).append(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(directory.join(format!("{}.jsonl", record.subject)))?;
                let line = serde_json::to_string(record).map_err(std::io::Error::other)?;
                file.write_all(line.as_bytes())?;
                file.write_all(b"\n")?;
                file.sync_data()
            });
        match written {
            Ok(()) => tracing::info!(
                launch = %record.launch,
                subject = %record.subject,
                native_id = %record.native_id,
                proven = record.proven,
                "fleet.launch_provenance_recorded"
            ),
            Err(error) => tracing::warn!(
                %error,
                subject = %record.subject,
                "fleet.launch_provenance_unrecorded"
            ),
        }
    }

    /// Every launch provenance record one subject holds, in order.
    #[cfg(test)]
    pub(crate) fn launch_provenance(&self, subject: &str) -> Vec<LaunchProvenanceRecord> {
        let path = self
            .state_root
            .join(FLEET_DECISIONS_DIR)
            .join(LAUNCH_PROVENANCE_DIR)
            .join(format!("{subject}.jsonl"));
        std::fs::read_to_string(path)
            .map(|text| {
                text.lines()
                    .filter_map(|line| serde_json::from_str(line).ok())
                    .collect()
            })
            .unwrap_or_default()
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
        FleetSnapshot::parse(&kontor_fleet_activation::read_guarded(
            &self.state_root,
            path,
            &FLEET_GUARD,
        )?)
    }

    /// The standing activation record as written, or `None` when there is no
    /// record or it cannot be read. Only the activation fence uses it: an
    /// unreadable record selects nothing a caller could have read.
    fn recorded_activation(&self) -> Option<FleetActivation> {
        kontor_fleet_activation::read_activation(&self.state_root).ok()
    }

    /// The published artifact for `hash`, re-read, re-hashed and re-validated.
    fn verify_published(&self, hash: &ContentHash) -> Result<FleetSnapshot, FleetError> {
        kontor_fleet_activation::verify_policy(&self.state_root, hash)
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

/// Write one immutable, content-addressed artifact the way [`FleetSource`]
/// writes a published policy: owner-only, through a temporary file and one
/// rename, so a reader sees no artifact or the whole one.
fn store_immutable(directory: &Path, target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    create_private_dir(directory)?;
    let temporary = target.with_extension("json.tmp");
    let result =
        write_owner_only(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, target));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
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
    use kontor_fleet_activation::MAX_ACTIVATION_BYTES;
    use kontor_fleet_activation::rule::*;
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
            eligibility: None,
            chain: None,
            source_bundle_hash: None,
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

    /// ASMA-8280 G-3: a delivery launch maps its provenance from the last
    /// decision recorded for exactly its agent run, and a row written before
    /// the chain was recorded still reads, with no chain.
    #[test]
    fn a_launch_reads_the_decision_recorded_for_its_own_agent_run() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        assert!(source.decision_for("run-1", "agent-1").is_none());
        source
            .record_decision(&decision("team/t/s", "anthropic"))
            .expect("record");
        let mut chained = decision("team/t/s", "openai");
        chained.chain = Some("lead".to_owned());
        source.record_decision(&chained).expect("record");
        let mut other = decision("team/t/s", "xai");
        other.agent_run_id = "agent-2".to_owned();
        source.record_decision(&other).expect("record");
        let read = source.decision_for("run-1", "agent-1").expect("a decision");
        assert_eq!(read.vendor, "openai");
        assert_eq!(read.chain.as_deref(), Some("lead"));
        assert_eq!(
            source
                .decision_for("run-1", "agent-2")
                .map(|read| read.vendor),
            Some("xai".to_owned())
        );
        let legacy: FleetDecision = serde_json::from_value(serde_json::json!({
            "team_run_id": "run-1", "agent_run_id": "agent-1", "binding_key": "team/t/s",
            "role_slot": "slot", "fleet_hash": "hash", "step": 1, "sub_step": 1,
            "provider": "provider", "model": "model", "vendor": "anthropic",
            "decided_at": "2026-09-24T00:00:00Z",
        }))
        .expect("a row written before G-3 still reads");
        assert!(legacy.chain.is_none() && legacy.source_bundle_hash.is_none());
        let written = serde_json::to_value(&legacy).expect("JSON");
        assert!(written.get("chain").is_none() && written.get("source_bundle_hash").is_none());
    }

    /// ASMA-8280 G-3: what a launch requested and what its runtime observed
    /// are recorded as two values, append-only and owner-only.
    #[test]
    fn a_launch_record_keeps_the_request_apart_from_the_observation() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        let requested = kontor_runtime::FleetLaunchProvenance {
            policy_hash: ContentHash::of(b"policy"),
            source_bundle_hash: None,
            binding_key: "team/t/s".to_owned(),
            chain: "lead".to_owned(),
            step: 1,
            sub_step: 1,
            vendor: "openai".to_owned(),
            eligibility: None,
        };
        let native_id = kontor_core::id::ExternalId::parse("native-1").expect("an id");
        let observed = kontor_runtime::FleetProvenanceObservation::without_surface(
            Some(&requested),
            "codex.exec",
            &native_id,
        );
        for _ in 0..2 {
            source.record_launch_provenance(&LaunchProvenanceRecord {
                launch: "delivery".to_owned(),
                subject: "agent-1".to_owned(),
                native_id: native_id.as_str().to_owned(),
                requested: Some(requested.clone()),
                observed: observed.clone(),
                proven: observed.proves(Some(&requested)),
                recorded_at: "2026-09-29T00:00:00Z".to_owned(),
            });
        }
        let records = source.launch_provenance("agent-1");
        assert_eq!(records.len(), 2, "append-only");
        assert_eq!(records[0].requested.as_ref(), Some(&requested));
        assert_eq!(records[0].observed, observed);
        assert!(!records[0].proven, "an unsupported surface proves nothing");
        let path = root
            .path()
            .join(FLEET_DECISIONS_DIR)
            .join(LAUNCH_PROVENANCE_DIR)
            .join("agent-1.jsonl");
        let mode = std::fs::metadata(path)
            .expect("the record")
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "owner-only");
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
            source_bundle_hash: None,
            eligibility: None,
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

    /// Publish one resolved authoring bundle and return what was published.
    fn publish_bundle(
        source: &FleetSource,
        authoring: &crate::orchestration::fixture::Authoring,
    ) -> (PublishedBundle, crate::orchestration::ResolvedBundle) {
        let resolved =
            crate::orchestration::resolve_bundle(&authoring.sources(), &authoring.catalog)
                .expect("the bundle resolves");
        (
            source.publish_bundle(&resolved).expect("published"),
            resolved,
        )
    }

    fn standing(root: &Path) -> ActivationFence {
        ActivationFence::of(kontor_fleet_activation::read_activation(root).ok().as_ref())
    }

    #[test]
    fn an_aligned_bundle_is_published_then_activated_as_one_selection() {
        use crate::orchestration::fixture::{ADVISOR, DELIVERY, REVIEWER, authoring};
        use kontor_fleet::Eligibility;

        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        let authoring = authoring();
        let (published, resolved) = publish_bundle(&source, &authoring);
        assert!(published.created);
        assert_eq!(published.manifest, resolved.manifest);
        for artifact in [
            kontor_fleet_activation::manifest_path(root.path(), &published.bundle_hash),
            kontor_fleet_activation::roster_path(root.path(), resolved.roster.hash()),
            kontor_fleet_activation::policy_path(root.path(), resolved.policy.hash()),
        ] {
            let metadata = std::fs::symlink_metadata(&artifact).expect("the artifact exists");
            assert!(metadata.file_type().is_file(), "{}", artifact.display());
            assert_eq!(metadata.mode() & 0o777, 0o600, "{}", artifact.display());
        }
        assert_eq!(
            std::fs::read_to_string(kontor_fleet_activation::roster_path(
                root.path(),
                resolved.roster.hash()
            ))
            .expect("the roster"),
            resolved.roster.json(),
            "the published roster is the exact canonical revision"
        );
        // Publication selects nothing, and republishing writes nothing.
        assert!(!activation_path(root.path()).exists());
        assert!(source.policy().expect("no activation").is_none());
        assert!(!publish_bundle(&source, &authoring).0.created);

        let activated = source
            .activate_bundle(&published.bundle_hash, &ActivationFence::default())
            .expect("activate");
        assert!(activated.changed);
        let record = &activated.record;
        assert_eq!(record.schema_version, ACTIVATION_V2);
        assert_eq!(
            record.source_bundle_hash.as_ref(),
            Some(&published.bundle_hash)
        );
        assert_eq!(&record.policy_hash, resolved.policy.hash());
        assert_eq!(record.policy_schema_version, 2);
        assert_eq!(
            record.core_team_revision_hash.as_ref(),
            Some(resolved.roster.hash())
        );
        let written: FleetActivation =
            serde_json::from_slice(&std::fs::read(activation_path(root.path())).expect("record"))
                .expect("record JSON");
        assert_eq!(&written, record);
        assert_eq!(
            std::fs::symlink_metadata(activation_path(root.path()))
                .expect("record")
                .mode()
                & 0o777,
            0o600
        );

        // The daemon's placement read and the direct reader are one read.
        let placement = source.placement().expect("verified").expect("activated");
        assert_eq!(placement.policy.hash(), resolved.policy.hash());
        assert_eq!(placement.roster.as_deref(), Some(&resolved.roster));
        assert_eq!(
            placement.source_bundle_hash.as_ref(),
            Some(&published.bundle_hash)
        );
        let eligibility = Eligibility::default();
        for seat in &resolved.revision.seats {
            let key =
                kontor_fleet::LeadershipKey::for_pinned_slot(&resolved.roster, &seat.role_slot_id)
                    .expect("the selected roster proves its slot");
            let governed = placement
                .policy
                .resolve_leadership(&key)
                .expect("bound")
                .select(&eligibility);
            let direct = kontor_fleet_activation::resolve(root.path(), key.as_str(), &eligibility)
                .expect("the direct reader resolves");
            assert_eq!(governed, direct, "{key}");
            assert!(direct.selected.is_some());
        }
        for key in [DELIVERY, REVIEWER, ADVISOR] {
            let governed = placement
                .policy
                .resolve(key)
                .expect("bound")
                .select(&eligibility);
            let direct = kontor_fleet_activation::resolve(root.path(), key, &eligibility)
                .expect("the direct reader resolves");
            assert_eq!(governed, direct, "{key}");
        }

        // Re-activating the standing bundle is the unchanged replay answer.
        let replay = source
            .activate_bundle(&published.bundle_hash, &ActivationFence::default())
            .expect("replay");
        assert!(!replay.changed);
        assert_eq!(&replay.record, record);

        // Another bundle needs the fence the caller read.
        let mut next = crate::orchestration::fixture::authoring();
        next.fleet = next.fleet.replace("lsa: codex-first", "lsa: claude-first");
        let (candidate, _) = publish_bundle(&source, &next);
        assert_ne!(candidate.bundle_hash, published.bundle_hash);
        assert!(matches!(
            source.activate_bundle(&candidate.bundle_hash, &ActivationFence::default()),
            Err(ActivationRefusal::Moved)
        ));
        assert_eq!(
            source
                .placement()
                .expect("verified")
                .expect("activated")
                .source_bundle_hash
                .as_ref(),
            Some(&published.bundle_hash),
            "a refused activation changes nothing"
        );
        let moved = source
            .activate_bundle(&candidate.bundle_hash, &standing(root.path()))
            .expect("activate the candidate");
        assert!(moved.changed);
        assert_eq!(
            moved.record.source_bundle_hash.as_ref(),
            Some(&candidate.bundle_hash)
        );
    }

    #[test]
    fn a_bundle_that_does_not_verify_is_never_activated_and_never_served() {
        use crate::orchestration::fixture::authoring;

        let root = tempfile::tempdir().expect("temporary state root");
        // An unmigrated fleet.yml is present throughout; it is never a fallback.
        write_fleet(root.path(), V1_CLAUDE_FIRST);
        let source = FleetSource::at(root.path());
        let authoring = authoring();
        let (published, resolved) = publish_bundle(&source, &authoring);
        let roster = kontor_fleet_activation::roster_path(root.path(), resolved.roster.hash());
        let good = std::fs::read(&roster).expect("roster");

        // A rewritten roster refuses activation and writes no record.
        write_private(&roster, b"{\"schema_version\":1,\"value\":\"lsa\"}");
        match source.activate_bundle(&published.bundle_hash, &ActivationFence::default()) {
            Err(ActivationRefusal::Policy(FleetError::Invalid { rule })) => assert_eq!(rule, C08),
            other => panic!("expected C-08, got {other:?}"),
        }
        assert!(!activation_path(root.path()).exists());
        write_private(&roster, &good);

        // An unpublished bundle refuses activation.
        let unpublished = ContentHash::of(b"no such bundle");
        match source.activate_bundle(&unpublished, &ActivationFence::default()) {
            Err(ActivationRefusal::Policy(FleetError::Invalid { rule })) => assert_eq!(rule, M07),
            other => panic!("expected M-07, got {other:?}"),
        }
        assert!(!activation_path(root.path()).exists());

        // Once active, a roster rewritten under it blocks placement; fleet.yml
        // and last-valid are not served instead.
        source
            .activate_bundle(&published.bundle_hash, &ActivationFence::default())
            .expect("activate");
        write_private(&roster, b"{}");
        assert_eq!(refusal(&source), C08);
        assert!(source.current().is_none());
        assert!(source.placement().is_err());
        std::fs::remove_file(&roster).expect("remove the roster");
        assert_eq!(refusal(&source), C07);
        write_private(&roster, &good);
        assert!(source.placement().expect("verified").is_some());
    }

    #[test]
    fn a_schema_version_1_record_keeps_its_exact_shape() {
        let root = tempfile::tempdir().expect("temporary state root");
        let source = FleetSource::at(root.path());
        activate(&source, V1_CLAUDE_FIRST);
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(activation_path(root.path())).expect("record"))
                .expect("record JSON");
        assert_eq!(
            written
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "activated_at",
                "policy_hash",
                "policy_schema_version",
                "schema_version"
            ]
        );
        assert_eq!(written["schema_version"], 1);
        let placement = source.placement().expect("verified").expect("activated");
        assert!(placement.roster.is_none(), "a v1 record selects no roster");
        assert!(placement.source_bundle_hash.is_none());
    }
}
