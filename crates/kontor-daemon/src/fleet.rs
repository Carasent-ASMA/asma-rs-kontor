//! Live fleet model routing: read, validate, flatten and reload `fleet.yml`.
//!
//! The model route of a seat used to be frozen into a template version and then
//! copied into every team run that started from it. This module replaces that
//! for the seats a live `fleet.yml` binds: an operator edits one file in the
//! Realm state root and the next placement reads it, with no rebuild, restart or
//! republish.
//!
//! The module is pure where it can be. Parsing, validation, flattening and
//! vendor lookup take plain data and return plain data; only [`FleetSource`]
//! touches the filesystem, and it owns the security checks and the receipts.
//!
//! The binding contract is Appendix B of the live-fleet plan: every rule text
//! below is copied verbatim from that table, and rules are checked in table
//! order so the first refusal is deterministic.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use kontor_core::id::{ContentHash, Timestamp, format_utc_timestamp};
use kontor_core::spec::{ModelRef, ModelRung, ProviderRef};
use serde::{Deserialize, Serialize};

/// The fleet configuration file name inside a Realm state root.
pub(crate) const FLEET_FILE: &str = "fleet.yml";

/// One immutable copy of every accepted fleet configuration.
pub(crate) const FLEET_HISTORY_DIR: &str = "fleet-history";

/// One JSON-lines decision log per team run.
pub(crate) const FLEET_DECISIONS_DIR: &str = "fleet-decisions";

/// Whether the last edit was accepted, and why not when it was not.
pub(crate) const FLEET_STATUS_FILE: &str = "fleet-status.json";

/// Largest fleet configuration this build accepts, in bytes.
pub(crate) const MAX_FILE_BYTES: u64 = 256 * 1024;

/// Sanity ceiling on the number of steps in one chain.
pub(crate) const MAX_STEPS: usize = 16;

/// Sanity ceiling on the number of flattened routes in one chain.
pub(crate) const MAX_ROUTES: usize = 64;

const F01: &str = "fleet.yml must be a regular file, not a symlink";
const F02: &str = "fleet.yml must not be writable by group or others";
const F03: &str = "fleet.yml must be owned by the state root's owner";
const F04: &str = "fleet.yml exceeds 256 KiB";
const F05: &str = "fleet.yml changed while it was being read";
const F06: &str = "fleet.yml is not UTF-8";

const V01: &str = "schema_version must be 1";
const V02: &str = "a domain name must be a lowercase slug";
const V03: &str = "a domain provider must be claude, codex, cursor or opencode";
const V04: &str = "a domain must list at least one account";
const V05: &str =
    "an account alias must be its domain's provider or start with the provider and a hyphen";
const V06: &str = "a domain lists an account twice";
const V07: &str = "domains that share an account must each declare a different model prefix";
const V08: &str = "a model name must be a lowercase slug";
const V09: &str = "a model names an unknown domain";
const V10: &str = "a model id must be non-empty and start with its domain's model prefix";
const V11: &str = "a model vendor must be a lowercase slug";
const V12: &str = "a model effort is not in the runtime effort vocabulary";
const V13: &str = "a model lists an effort twice";
const V14: &str = "a model route failed route validation";
const V15: &str = "a chain name must be a lowercase slug";
const V16: &str = "a chain must have 1 to 16 non-empty steps";
const V17: &str = "a chain entry names an unknown model";
const V18: &str = "a chain entry effort does not match the model's efforts";
const V19: &str = "every entry in one step must use the same domain";
const V20: &str = "a chain uses the same domain in two steps";
const V21: &str = "a chain repeats a model";
const V22: &str = "a chain flattens to more than 64 routes";
const V23: &str = "a binding key must be team/<id>/<slot>, committee/<id>/<slot> or advisor/<id>";
const V24: &str = "a binding names an unknown chain";
const V25: &str =
    "a Committee, Advisor or independent seat chain may not use a model with vendor unknown";
const V26: &str = "an unavailable domain is not declared";
const V27: &str = "an unavailable account is not in any domain";
const V28: &str = "a rule names a seat key that has no binding";
const V29: &str = "a seat cannot be independent of itself";
const V30: &str = "independent_of pairs must be two seats of the same team template";

/// Why a fleet configuration could not be used as written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FleetError {
    /// The file exists but could not be read.
    #[error("the fleet configuration could not be read")]
    Read {
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The document is not valid YAML for schema version 1.
    #[error("the fleet configuration is not a valid schema_version 1 document")]
    Document,
    /// The document is structurally valid but unsafe or contradictory.
    #[error("the fleet configuration is invalid: {rule}")]
    Invalid {
        /// The stable rule, never a configured value.
        rule: &'static str,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FleetDocument {
    schema_version: u32,
    domains: BTreeMap<String, DomainSpec>,
    #[serde(default)]
    unavailable: UnavailableSpec,
    models: BTreeMap<String, ModelSpec>,
    /// chain name -> ordered steps -> entries written `model` or `model@effort`
    chains: BTreeMap<String, Vec<Vec<String>>>,
    bindings: BTreeMap<String, String>,
    #[serde(default)]
    rules: RulesSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DomainSpec {
    provider: String,
    accounts: Vec<String>,
    #[serde(default)]
    model_prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelSpec {
    domain: String,
    id: String,
    vendor: String,
    #[serde(default)]
    efforts: Vec<String>,
    #[serde(default)]
    vision: bool,
    #[serde(default)]
    calibrated: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct UnavailableSpec {
    #[serde(default)]
    domains: Vec<String>,
    #[serde(default)]
    accounts: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RulesSpec {
    #[serde(default)]
    calibration_required: Vec<String>,
    #[serde(default)]
    vision_required: Vec<String>,
    #[serde(default)]
    independent_of: BTreeMap<String, String>,
}

/// One admissible flattened route of a fleet chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FleetRoute {
    pub(crate) rung: ModelRung,
    pub(crate) step: u16,
    pub(crate) sub_step: u16,
    pub(crate) vendor: String,
}

/// The rungs one delivery seat may walk, and the fleet binding they came from.
///
/// `fleet` is `Some` exactly when the rungs came from the live `fleet.yml`
/// snapshot rather than the frozen template chain: it carries that snapshot and
/// the binding key so an admitted placement can be recorded against the exact
/// version that authorised it.
#[derive(Debug)]
pub(crate) struct DeclaredRungs {
    pub(crate) rungs: Vec<ModelRung>,
    pub(crate) fleet: Option<(Arc<FleetSnapshot>, String)>,
}

/// A validated, hashed view of one `fleet.yml` document.
#[derive(Debug)]
pub struct FleetSnapshot {
    document: FleetDocument,
    raw: String,
    hash: ContentHash,
}

impl FleetSnapshot {
    /// Parse and fully validate a `fleet.yml` document.
    ///
    /// # Errors
    /// Returns [`FleetError::Document`] for malformed YAML or unknown fields and
    /// [`FleetError::Invalid`] for the first violated rule of Appendix B.
    pub(crate) fn parse(document: &str) -> Result<Self, FleetError> {
        if document.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid(F04));
        }
        let parsed: FleetDocument =
            serde_yaml_ng::from_str(document).map_err(|_| FleetError::Document)?;
        parsed.validate()?;
        Ok(Self {
            document: parsed,
            raw: document.to_owned(),
            hash: ContentHash::of(document.as_bytes()),
        })
    }

    /// The SHA-256 of exactly the bytes that were read.
    pub(crate) fn hash(&self) -> &ContentHash {
        &self.hash
    }

    /// The validated document, for the history copy and `lists`.
    #[cfg(test)]
    fn raw(&self) -> &str {
        &self.raw
    }

    /// The flattened routes bound to `key`, or `None` when nothing binds it.
    ///
    /// The result may be `Some(vec![])` when every route was filtered out; the
    /// caller turns that into the R-01 placement refusal.
    pub(crate) fn routes_for(&self, key: &str) -> Option<Vec<FleetRoute>> {
        let chain = self.document.chains.get(self.document.bindings.get(key)?)?;
        let calibration_required = self
            .document
            .rules
            .calibration_required
            .iter()
            .any(|listed| listed == key);
        let vision_required = self
            .document
            .rules
            .vision_required
            .iter()
            .any(|listed| listed == key);
        let mut routes = Vec::new();
        for (index, step) in chain.iter().enumerate() {
            let step_number = u16::try_from(index + 1).unwrap_or(u16::MAX);
            let mut sub_step = 0u16;
            for entry in step {
                let (name, effort_text) = entry_parts(entry);
                let Some(model) = self.document.models.get(name) else {
                    continue;
                };
                if self
                    .document
                    .unavailable
                    .domains
                    .iter()
                    .any(|domain| domain == &model.domain)
                {
                    continue;
                }
                if calibration_required && !model.calibrated {
                    continue;
                }
                if vision_required && !model.vision {
                    continue;
                }
                let Some(domain) = self.document.domains.get(&model.domain) else {
                    continue;
                };
                for account in &domain.accounts {
                    if self
                        .document
                        .unavailable
                        .accounts
                        .iter()
                        .any(|listed| listed == account)
                    {
                        continue;
                    }
                    sub_step = sub_step.saturating_add(1);
                    let effort =
                        effort_text.and_then(|text| crate::applications::parse_effort(text).ok());
                    routes.push(FleetRoute {
                        rung: ModelRung {
                            provider: ProviderRef(account.clone()),
                            model: ModelRef(model.id.clone()),
                            effort,
                        },
                        step: step_number,
                        sub_step,
                        vendor: model.vendor.clone(),
                    });
                }
            }
        }
        Some(routes)
    }

    /// Whether the snapshot lists this route: matching account, model and effort.
    pub(crate) fn lists(&self, rung: &ModelRung) -> bool {
        let Some(model) = self.model_of(rung) else {
            return false;
        };
        if model.efforts.is_empty() {
            return rung.effort.is_none();
        }
        rung.effort
            .is_some_and(|effort| model.efforts.iter().any(|listed| listed == effort.as_str()))
    }

    /// The model's maker for a route the snapshot lists, if any.
    pub(crate) fn vendor_of(&self, rung: &ModelRung) -> Option<&str> {
        self.model_of(rung).map(|model| model.vendor.as_str())
    }

    /// The seat key that `key` must differ from, when the rules declare one.
    pub(crate) fn independent_of(&self, key: &str) -> Option<&str> {
        self.document
            .rules
            .independent_of
            .get(key)
            .map(String::as_str)
    }

    fn model_of(&self, rung: &ModelRung) -> Option<&ModelSpec> {
        self.document.models.values().find(|model| {
            model.id == rung.model.0
                && self
                    .document
                    .domains
                    .get(&model.domain)
                    .is_some_and(|domain| {
                        domain
                            .accounts
                            .iter()
                            .any(|account| account == &rung.provider.0)
                    })
        })
    }
}

impl FleetDocument {
    fn validate(&self) -> Result<(), FleetError> {
        if self.schema_version != 1 {
            return Err(invalid(V01));
        }
        self.validate_domains()?;
        self.validate_models()?;
        self.validate_chains()?;
        self.validate_bindings()?;
        self.validate_unavailable()?;
        self.validate_rules()
    }

    fn validate_domains(&self) -> Result<(), FleetError> {
        for name in self.domains.keys() {
            if !is_generic_slug(name) {
                return Err(invalid(V02));
            }
        }
        for domain in self.domains.values() {
            if !matches!(
                domain.provider.as_str(),
                "claude" | "codex" | "cursor" | "opencode"
            ) {
                return Err(invalid(V03));
            }
        }
        for domain in self.domains.values() {
            if domain.accounts.is_empty() {
                return Err(invalid(V04));
            }
        }
        for domain in self.domains.values() {
            for account in &domain.accounts {
                let named = account == &domain.provider
                    || account
                        .strip_prefix(domain.provider.as_str())
                        .is_some_and(|suffix| suffix.starts_with('-'));
                if !named {
                    return Err(invalid(V05));
                }
            }
        }
        for domain in self.domains.values() {
            let unique: BTreeSet<&str> = domain.accounts.iter().map(String::as_str).collect();
            if unique.len() != domain.accounts.len() {
                return Err(invalid(V06));
            }
        }
        let names: Vec<&String> = self.domains.keys().collect();
        for (index, first) in names.iter().enumerate() {
            for second in &names[index + 1..] {
                let one = &self.domains[*first];
                let two = &self.domains[*second];
                let shared = one
                    .accounts
                    .iter()
                    .any(|account| two.accounts.contains(account));
                if shared {
                    let distinct = one.model_prefix.is_some()
                        && two.model_prefix.is_some()
                        && one.model_prefix != two.model_prefix;
                    if !distinct {
                        return Err(invalid(V07));
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_models(&self) -> Result<(), FleetError> {
        for name in self.models.keys() {
            if !is_model_slug(name) {
                return Err(invalid(V08));
            }
        }
        for model in self.models.values() {
            if !self.domains.contains_key(&model.domain) {
                return Err(invalid(V09));
            }
        }
        for model in self.models.values() {
            let Some(domain) = self.domains.get(&model.domain) else {
                return Err(invalid(V09));
            };
            let prefixed = domain
                .model_prefix
                .as_ref()
                .is_some_and(|prefix| !model.id.starts_with(prefix));
            if model.id.is_empty() || prefixed {
                return Err(invalid(V10));
            }
        }
        for model in self.models.values() {
            if !is_generic_slug(&model.vendor) {
                return Err(invalid(V11));
            }
        }
        for model in self.models.values() {
            if model
                .efforts
                .iter()
                .any(|effort| crate::applications::parse_effort(effort).is_err())
            {
                return Err(invalid(V12));
            }
        }
        for model in self.models.values() {
            let unique: BTreeSet<&str> = model.efforts.iter().map(String::as_str).collect();
            if unique.len() != model.efforts.len() {
                return Err(invalid(V13));
            }
        }
        for model in self.models.values() {
            let Some(domain) = self.domains.get(&model.domain) else {
                return Err(invalid(V09));
            };
            for account in &domain.accounts {
                let rung = ModelRung {
                    provider: ProviderRef(account.clone()),
                    model: ModelRef(model.id.clone()),
                    effort: None,
                };
                if rung.validate().is_err() {
                    return Err(invalid(V14));
                }
            }
        }
        Ok(())
    }

    fn validate_chains(&self) -> Result<(), FleetError> {
        for name in self.chains.keys() {
            if !is_generic_slug(name) {
                return Err(invalid(V15));
            }
        }
        for steps in self.chains.values() {
            if steps.is_empty() || steps.len() > MAX_STEPS || steps.iter().any(Vec::is_empty) {
                return Err(invalid(V16));
            }
        }
        for steps in self.chains.values() {
            for entry in steps.iter().flatten() {
                if !self.models.contains_key(entry_parts(entry).0) {
                    return Err(invalid(V17));
                }
            }
        }
        for steps in self.chains.values() {
            for entry in steps.iter().flatten() {
                let (name, effort) = entry_parts(entry);
                let Some(model) = self.models.get(name) else {
                    return Err(invalid(V17));
                };
                if !effort_matches(effort, &model.efforts) {
                    return Err(invalid(V18));
                }
            }
        }
        for steps in self.chains.values() {
            for step in steps {
                let mut domain: Option<&str> = None;
                for entry in step {
                    let Some(model) = self.models.get(entry_parts(entry).0) else {
                        return Err(invalid(V17));
                    };
                    match domain {
                        None => domain = Some(model.domain.as_str()),
                        Some(current) if current == model.domain.as_str() => {}
                        Some(_) => return Err(invalid(V19)),
                    }
                }
            }
        }
        for steps in self.chains.values() {
            let mut used: BTreeSet<&str> = BTreeSet::new();
            for step in steps {
                let Some(domain) = step
                    .first()
                    .and_then(|entry| self.models.get(entry_parts(entry).0))
                    .map(|model| model.domain.as_str())
                else {
                    return Err(invalid(V17));
                };
                if !used.insert(domain) {
                    return Err(invalid(V20));
                }
            }
        }
        for steps in self.chains.values() {
            let mut seen: BTreeSet<(&str, Option<&str>)> = BTreeSet::new();
            for entry in steps.iter().flatten() {
                if !seen.insert(entry_parts(entry)) {
                    return Err(invalid(V21));
                }
            }
        }
        for steps in self.chains.values() {
            let mut total: usize = 0;
            for entry in steps.iter().flatten() {
                let Some(model) = self.models.get(entry_parts(entry).0) else {
                    return Err(invalid(V17));
                };
                total = total.saturating_add(
                    self.domains
                        .get(&model.domain)
                        .map_or(0, |domain| domain.accounts.len()),
                );
            }
            if total > MAX_ROUTES {
                return Err(invalid(V22));
            }
        }
        Ok(())
    }

    fn validate_bindings(&self) -> Result<(), FleetError> {
        for key in self.bindings.keys() {
            if !is_binding_key(key) {
                return Err(invalid(V23));
            }
        }
        for chain in self.bindings.values() {
            if !self.chains.contains_key(chain) {
                return Err(invalid(V24));
            }
        }
        let mut independent: BTreeSet<&str> = BTreeSet::new();
        for (key, value) in &self.rules.independent_of {
            independent.insert(key.as_str());
            independent.insert(value.as_str());
        }
        for key in self.bindings.keys() {
            let special = key.starts_with("committee/")
                || key.starts_with("advisor/")
                || independent.contains(key.as_str());
            if !special {
                continue;
            }
            let Some(chain) = self.bindings.get(key) else {
                continue;
            };
            let Some(steps) = self.chains.get(chain) else {
                continue;
            };
            if self.chain_has_unknown_vendor(steps) {
                return Err(invalid(V25));
            }
        }
        Ok(())
    }

    fn validate_unavailable(&self) -> Result<(), FleetError> {
        for domain in &self.unavailable.domains {
            if !self.domains.contains_key(domain) {
                return Err(invalid(V26));
            }
        }
        let known: BTreeSet<&str> = self
            .domains
            .values()
            .flat_map(|domain| domain.accounts.iter().map(String::as_str))
            .collect();
        for account in &self.unavailable.accounts {
            if !known.contains(account.as_str()) {
                return Err(invalid(V27));
            }
        }
        Ok(())
    }

    fn validate_rules(&self) -> Result<(), FleetError> {
        for key in self
            .rules
            .calibration_required
            .iter()
            .chain(self.rules.vision_required.iter())
        {
            if !self.bindings.contains_key(key) {
                return Err(invalid(V28));
            }
        }
        for (key, value) in &self.rules.independent_of {
            if !self.bindings.contains_key(key) || !self.bindings.contains_key(value) {
                return Err(invalid(V28));
            }
        }
        for (key, value) in &self.rules.independent_of {
            if key == value {
                return Err(invalid(V29));
            }
        }
        for (key, value) in &self.rules.independent_of {
            if !same_team_template(key, value) {
                return Err(invalid(V30));
            }
        }
        Ok(())
    }

    fn chain_has_unknown_vendor(&self, steps: &[Vec<String>]) -> bool {
        steps.iter().flatten().any(|entry| {
            self.models
                .get(entry_parts(entry).0)
                .is_some_and(|model| model.vendor == "unknown")
        })
    }
}

/// The seat key for one role slot of one team template.
pub(crate) fn team_key(template_id: &str, slot: &str) -> String {
    format!("team/{template_id}/{slot}")
}

/// The seat key for one role slot of one Committee template.
pub(crate) fn committee_key(template_id: &str, slot: &str) -> String {
    format!("committee/{template_id}/{slot}")
}

/// The seat key for one Advisor profile.
pub(crate) fn advisor_key(profile_id: &str) -> String {
    format!("advisor/{profile_id}")
}

/// The independence key of a route, or `None` when it must not fill a reviewer.
///
/// With a fleet, the model's maker wins, so a Cursor route to Grok is `xai` and
/// no longer collides with an OpenCode route to GLM. Cursor Auto is `unknown`
/// and can never fill a reviewer slot. Without a fleet this is a pure renaming
/// of today's provider family, so legacy allocation is unchanged.
pub(crate) fn independence_key(rung: &ModelRung, fleet: Option<&FleetSnapshot>) -> Option<String> {
    if let Some(vendor) = fleet.and_then(|fleet| fleet.vendor_of(rung)) {
        return if vendor == "unknown" {
            None
        } else {
            Some(vendor.to_owned())
        };
    }
    match crate::applications::provider_family(&rung.provider.0) {
        "claude" => Some("anthropic".to_owned()),
        "codex" => Some("openai".to_owned()),
        other => Some(other.to_owned()),
    }
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

#[derive(Debug, Default)]
struct Cache {
    stamp: Option<Stamp>,
    good: Option<Arc<FleetSnapshot>>,
    last_error: Option<String>,
    loaded_at: Option<String>,
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
}

/// The live, stamp-tracked fleet configuration for one Realm.
///
/// One process holds one of these per Realm, on [`crate::applications::Services`].
/// It never watches the file; every placement asks it for the current snapshot,
/// and the file is re-parsed only when its stamp changed.
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

    /// The current valid snapshot, or `None` when no valid file is present.
    ///
    /// A missing file clears the cache and means "legacy routing". A changed
    /// file is re-read; an invalid edit keeps the last valid snapshot and is
    /// reported in `fleet-status.json`.
    pub fn current(&self) -> Option<Arc<FleetSnapshot>> {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
                    self.write_status_file(&cache);
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
        self.write_status_file(&cache);
        cache.good.clone()
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

    fn load(&self, path: &Path) -> Result<FleetSnapshot, FleetError> {
        let metadata =
            std::fs::symlink_metadata(path).map_err(|source| FleetError::Read { source })?;
        if !metadata.file_type().is_file() {
            return Err(invalid(F01));
        }
        if metadata.mode() & 0o022 != 0 {
            return Err(invalid(F02));
        }
        let root = std::fs::symlink_metadata(&self.state_root)
            .map_err(|source| FleetError::Read { source })?;
        if metadata.uid() != root.uid() {
            return Err(invalid(F03));
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Err(invalid(F04));
        }
        let file = std::fs::File::open(path).map_err(|source| FleetError::Read { source })?;
        let opened = file
            .metadata()
            .map_err(|source| FleetError::Read { source })?;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() {
            return Err(invalid(F05));
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| FleetError::Read { source })?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid(F04));
        }
        let document = std::str::from_utf8(&bytes).map_err(|_| invalid(F06))?;
        FleetSnapshot::parse(document)
    }

    fn write_history(&self, snapshot: &FleetSnapshot) {
        let directory = self.state_root.join(FLEET_HISTORY_DIR);
        if let Err(error) = create_private_dir(&directory) {
            tracing::warn!(error = %error, "fleet.history_write_failed");
            return;
        }
        let target = directory.join(format!("{}.yml", snapshot.hash()));
        if target.exists() {
            return;
        }
        let temporary = directory.join(format!("{}.yml.tmp", snapshot.hash()));
        let result = write_owner_only(&temporary, snapshot.raw.as_bytes())
            .and_then(|()| std::fs::rename(&temporary, &target));
        if let Err(error) = result {
            tracing::warn!(error = %error, "fleet.history_write_failed");
            let _ = std::fs::remove_file(&temporary);
        }
    }

    fn write_status_file(&self, cache: &Cache) {
        let status = StatusFile {
            active_hash: cache.good.as_ref().map(|good| good.hash().as_str()),
            loaded_at: cache.loaded_at.as_deref(),
            last_error: cache.last_error.as_deref(),
            checked_at: format_utc_timestamp(Timestamp::now()),
        };
        if let Err(error) = self.write_status(&status) {
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

fn entry_parts(entry: &str) -> (&str, Option<&str>) {
    entry
        .split_once('@')
        .map_or((entry, None), |(model, effort)| (model, Some(effort)))
}

fn effort_matches(effort: Option<&str>, model_efforts: &[String]) -> bool {
    match (effort, model_efforts.is_empty()) {
        (None, true) => true,
        (Some(entry), false) => model_efforts.iter().any(|listed| listed == entry),
        _ => false,
    }
}

fn is_generic_slug(name: &str) -> bool {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    name.len() <= 32
        && characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn is_model_slug(name: &str) -> bool {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }
    name.len() <= 48
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '.'
                || character == '-'
        })
}

fn is_binding_key(key: &str) -> bool {
    let mut parts = key.split('/');
    let kind = parts.next();
    let first = parts.next();
    let second = parts.next();
    if parts.next().is_some() {
        return false;
    }
    match kind {
        Some("advisor") => second.is_none() && first.is_some_and(is_key_segment),
        Some("team" | "committee") => {
            first.is_some_and(is_key_segment) && second.is_some_and(is_key_segment)
        }
        _ => false,
    }
}

fn is_key_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
}

fn team_template(key: &str) -> Option<&str> {
    let mut parts = key.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("team"), Some(id), Some(_), None) => Some(id),
        _ => None,
    }
}

fn same_team_template(first: &str, second: &str) -> bool {
    matches!(
        (team_template(first), team_template(second)),
        (Some(one), Some(two)) if one == two
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
