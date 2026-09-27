//! `kontor-fleet` — the one fleet policy parser, validator and route resolver.
//!
//! A fleet policy names model domains, models, ordered chains and the seat keys
//! bound to them. This crate reads that YAML, enforces every rule on it, and
//! turns a bound seat key into its ordered routes. Nothing here touches the
//! filesystem, a store or a runtime: the daemon owns reading, history and
//! decision evidence, so the same code can serve a reader that has no daemon.
//! There is no second interpreter.
//!
//! The binding contract is Appendix B of the live-fleet plan (ASMA-8255): every
//! rule text below is copied verbatim from that table, and rules are checked in
//! table order so the first refusal is deterministic.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::ContentHash;
use kontor_core::spec::{EffortLevel, ModelRef, ModelRung, ProviderRef};
use kontor_core::{DomainError, DomainResult};
use serde::Deserialize;

/// Largest fleet document this build accepts, in bytes.
pub const MAX_FILE_BYTES: u64 = 256 * 1024;

/// Sanity ceiling on the number of steps in one chain.
pub const MAX_STEPS: usize = 16;

/// Sanity ceiling on the number of flattened routes in one chain.
pub const MAX_ROUTES: usize = 64;

/// The stable rule texts a refusal names: Appendix B of the live-fleet plan,
/// verbatim.
#[allow(
    missing_docs,
    reason = "each Appendix B constant is documented by the verbatim text it holds"
)]
pub mod rule {
    /// The document is larger than [`crate::MAX_FILE_BYTES`].
    pub const F04: &str = "fleet.yml exceeds 256 KiB";

    pub const V01: &str = "schema_version must be 1";
    pub const V02: &str = "a domain name must be a lowercase slug";
    pub const V03: &str = "a domain provider must be claude, codex, cursor or opencode";
    pub const V04: &str = "a domain must list at least one account";
    pub const V05: &str =
        "an account alias must be its domain's provider or start with the provider and a hyphen";
    pub const V06: &str = "a domain lists an account twice";
    pub const V07: &str =
        "domains that share an account must each declare a different model prefix";
    pub const V08: &str = "a model name must be a lowercase slug";
    pub const V09: &str = "a model names an unknown domain";
    pub const V10: &str = "a model id must be non-empty and start with its domain's model prefix";
    pub const V11: &str = "a model vendor must be a lowercase slug";
    pub const V12: &str = "a model effort is not in the runtime effort vocabulary";
    pub const V13: &str = "a model lists an effort twice";
    pub const V14: &str = "a model route failed route validation";
    pub const V15: &str = "a chain name must be a lowercase slug";
    pub const V16: &str = "a chain must have 1 to 16 non-empty steps";
    pub const V17: &str = "a chain entry names an unknown model";
    pub const V18: &str = "a chain entry effort does not match the model's efforts";
    pub const V19: &str = "every entry in one step must use the same domain";
    pub const V20: &str = "a chain uses the same domain in two steps";
    pub const V21: &str = "a chain repeats a model";
    pub const V22: &str = "a chain flattens to more than 64 routes";
    pub const V23: &str =
        "a binding key must be team/<id>/<slot>, committee/<id>/<slot> or advisor/<id>";
    pub const V24: &str = "a binding names an unknown chain";
    pub const V25: &str =
        "a Committee, Advisor or independent seat chain may not use a model with vendor unknown";
    pub const V26: &str = "an unavailable domain is not declared";
    pub const V27: &str = "an unavailable account is not in any domain";
    pub const V28: &str = "a rule names a seat key that has no binding";
    pub const V29: &str = "a seat cannot be independent of itself";
    pub const V30: &str = "independent_of pairs must be two seats of the same team template";
}

use rule::{
    F04, V01, V02, V03, V04, V05, V06, V07, V08, V09, V10, V11, V12, V13, V14, V15, V16, V17, V18,
    V19, V20, V21, V22, V23, V24, V25, V26, V27, V28, V29, V30,
};

/// Why a fleet document could not be used as written.
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
pub struct FleetRoute {
    /// The exact account alias, model and effort.
    pub rung: ModelRung,
    /// The chain step, counted from one.
    pub step: u16,
    /// The route's position inside its step, counted from one.
    pub sub_step: u16,
    /// The model's maker.
    pub vendor: String,
}

/// A validated, hashed view of one fleet document.
#[derive(Debug)]
pub struct FleetSnapshot {
    document: FleetDocument,
    raw: String,
    hash: ContentHash,
}

impl FleetSnapshot {
    /// Parse and fully validate a fleet document.
    ///
    /// # Errors
    /// Returns [`FleetError::Document`] for malformed YAML or unknown fields and
    /// [`FleetError::Invalid`] for the first violated rule of Appendix B.
    pub fn parse(document: &str) -> Result<Self, FleetError> {
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
    #[must_use]
    pub fn hash(&self) -> &ContentHash {
        &self.hash
    }

    /// The validated document, byte for byte.
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// The flattened routes bound to `key`, or `None` when nothing binds it.
    ///
    /// The result may be `Some(vec![])` when every route was filtered out; the
    /// caller turns that into the R-01 placement refusal.
    #[must_use]
    pub fn routes_for(&self, key: &str) -> Option<Vec<FleetRoute>> {
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
                    let effort = effort_text.and_then(|text| parse_effort(text).ok());
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
    #[must_use]
    pub fn lists(&self, rung: &ModelRung) -> bool {
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
    #[must_use]
    pub fn vendor_of(&self, rung: &ModelRung) -> Option<&str> {
        self.model_of(rung).map(|model| model.vendor.as_str())
    }

    /// The seat key that `key` must differ from, when the rules declare one.
    #[must_use]
    pub fn independent_of(&self, key: &str) -> Option<&str> {
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
                .any(|effort| parse_effort(effort).is_err())
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
#[must_use]
pub fn team_key(template_id: &str, slot: &str) -> String {
    format!("team/{template_id}/{slot}")
}

/// The seat key for one role slot of one Committee template.
#[must_use]
pub fn committee_key(template_id: &str, slot: &str) -> String {
    format!("committee/{template_id}/{slot}")
}

/// The seat key for one Advisor profile.
#[must_use]
pub fn advisor_key(profile_id: &str) -> String {
    format!("advisor/{profile_id}")
}

/// The independence key of a route, or `None` when it must not fill a reviewer.
///
/// With a fleet, the model's maker wins, so a Cursor route to Grok is `xai` and
/// no longer collides with an OpenCode route to GLM. Cursor Auto is `unknown`
/// and can never fill a reviewer slot. Without a fleet this is a pure renaming
/// of today's provider family, so legacy allocation is unchanged.
#[must_use]
pub fn independence_key(rung: &ModelRung, fleet: Option<&FleetSnapshot>) -> Option<String> {
    if let Some(vendor) = fleet.and_then(|fleet| fleet.vendor_of(rung)) {
        return if vendor == "unknown" {
            None
        } else {
            Some(vendor.to_owned())
        };
    }
    match provider_family(&rung.provider.0) {
        "claude" => Some("anthropic".to_owned()),
        "codex" => Some("openai".to_owned()),
        other => Some(other.to_owned()),
    }
}

/// The built-in family behind a selectable Paseo account alias.
#[must_use]
pub fn provider_family(provider: &str) -> &str {
    for family in ["claude", "codex", "copilot", "opencode", "pi", "omp"] {
        if provider == family
            || provider
                .strip_prefix(family)
                .is_some_and(|suffix| suffix.starts_with('-'))
        {
            return family;
        }
    }
    provider
}

/// Parse one effort in the runtime effort vocabulary.
///
/// # Errors
/// Rejects a spelling outside the vocabulary.
pub fn parse_effort(effort: &str) -> DomainResult<EffortLevel> {
    match effort {
        "off" => Ok(EffortLevel::Off),
        "low" => Ok(EffortLevel::Low),
        "medium" => Ok(EffortLevel::Medium),
        "high" => Ok(EffortLevel::High),
        "xhigh" => Ok(EffortLevel::Xhigh),
        "max" => Ok(EffortLevel::Max),
        "ultra" => Ok(EffortLevel::Ultra),
        "ultracode" => Ok(EffortLevel::Ultracode),
        _ => Err(DomainError::invalid(
            "ModelRung",
            "effort is not in the runtime effort vocabulary",
        )),
    }
}

fn invalid(rule: &'static str) -> FleetError {
    FleetError::Invalid { rule }
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
