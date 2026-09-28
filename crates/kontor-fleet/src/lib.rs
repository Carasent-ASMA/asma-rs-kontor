//! `kontor-fleet` — the one fleet policy parser, validator and route resolver.
//!
//! A fleet policy names model domains, models, ordered chains and the seat keys
//! bound to them. This crate reads that YAML, enforces every rule on it, and
//! turns a bound seat key into its ordered routes. Nothing here touches the
//! filesystem, a store or a runtime: the daemon owns reading, activation,
//! history and decision evidence, and direct-mode orchestration reads the same
//! activated bytes through this same code. There is no second interpreter.
//!
//! Two schema versions are read:
//!
//! - **schema_version 1** is the live-fleet document of ASMA-8255. The rules
//!   are Appendix B of the live-fleet plan, copied verbatim and checked in table
//!   order so the first refusal is deterministic. [`FleetSnapshot::parse`] reads
//!   only this version and is what the unmigrated state-root `fleet.yml` uses.
//! - **schema_version 2** is its successor for one activated policy shared by
//!   both orchestration modes (ASMA-8280). It keeps every v1 section, rule and
//!   binding family and adds exactly one key family,
//!   `leadership/<core-team-revision-hash>/<role-slot-id>`, which only
//!   [`LeadershipKey::for_pinned_seat`] can build. [`FleetSnapshot::parse_policy`]
//!   reads either version and is what activation uses.

mod leadership;

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::{ContentHash, RoleSlotId};
use kontor_core::spec::{EffortLevel, ModelRef, ModelRung, ProviderRef};
use kontor_core::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

pub use leadership::LeadershipKey;

/// Largest fleet document this build accepts, in bytes.
pub const MAX_FILE_BYTES: u64 = 256 * 1024;

/// Sanity ceiling on the number of steps in one chain.
pub const MAX_STEPS: usize = 16;

/// Sanity ceiling on the number of flattened routes in one chain.
pub const MAX_ROUTES: usize = 64;

/// The stable rule texts a refusal names.
///
/// `F04` and `V01`–`V30` are Appendix B of the live-fleet plan, verbatim. `V31`
/// and `V32` are the schema_version 2 successor's; `L01`–`L03` refuse a
/// leadership key whose pinned Core Team seat cannot be proved.
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

    /// A policy names a schema this build does not read.
    pub const V31: &str = "schema_version must be 1 or 2";
    /// A published policy names a value that could identify or authenticate
    /// an account, or locate a provider home.
    pub const V33: &str = "a fleet policy value must not carry credential material, a filesystem path or an email address";
    /// A schema_version 2 binding key is outside the five families.
    pub const V32: &str = "a binding key must be team/<id>/<slot>, committee/<id>/<slot>, advisor/<id> or leadership/<core-team-revision-hash>/<role-slot-id>";

    /// Policy bytes do not hash to the content address they are read under.
    pub const P07: &str = "a published fleet policy does not hash to its content address";
    /// Activated bytes validate under another schema than the record names.
    pub const A09: &str =
        "the activated fleet policy's schema_version differs from its activation record";

    /// The document handed in is not a canonical Core Team revision.
    pub const L01: &str =
        "a leadership key needs the canonical document of a complete Core Team revision";
    /// The slot is absent from, or repeated in, the pinned revision.
    pub const L02: &str =
        "a leadership seat must occur exactly once in its pinned Core Team revision";
    /// The seat's role is not the frozen role snapshot the revision pins.
    pub const L03: &str =
        "a leadership seat's role must match the frozen role snapshot of its slot";
}

use rule::{
    A09, F04, P07, V01, V02, V03, V04, V05, V06, V07, V08, V09, V10, V11, V12, V13, V14, V15, V16,
    V17, V18, V19, V20, V21, V22, V23, V24, V25, V26, V27, V28, V29, V30, V31, V32, V33,
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
    /// A validated policy or its activation record could not be written.
    #[error("the fleet policy could not be written")]
    Write {
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The document is not valid YAML for schema version 1.
    #[error("the fleet configuration is not a valid schema_version 1 document")]
    Document,
    /// The policy is not valid YAML for a schema version this build reads.
    #[error("the fleet policy is not a valid schema_version 1 or 2 document")]
    PolicyDocument,
    /// The document is structurally valid but unsafe or contradictory.
    #[error("the fleet configuration is invalid: {rule}")]
    Invalid {
        /// The stable rule, never a configured value.
        rule: &'static str,
    },
}

/// Which entry point is reading: the v1-only `fleet.yml` or a published policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reader {
    Fleet,
    Policy,
}

/// The schema a document was validated under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Schema {
    V1,
    V2,
}

impl Schema {
    fn accepts_key(self, key: &str) -> bool {
        match self {
            Self::V1 => is_binding_key(key),
            Self::V2 => is_binding_key(key) || is_leadership_key(key),
        }
    }

    fn key_rule(self) -> &'static str {
        match self {
            Self::V1 => V23,
            Self::V2 => V32,
        }
    }
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

/// Which policy bytes and which binding produced a resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FleetProvenance {
    /// SHA-256 of exactly the policy bytes that were validated.
    pub policy_hash: ContentHash,
    /// The schema those bytes were validated under.
    pub schema_version: u32,
    /// The seat key that was looked up.
    pub binding_key: String,
    /// The chain that key is bound to.
    pub chain: String,
}

/// The ordered routes one bound seat key may walk, and where they came from.
///
/// `routes` may be empty when the unavailable, calibration and vision rules
/// removed every route; the caller turns that into its placement refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FleetResolution {
    /// The exact policy and binding that selected these routes.
    pub provenance: FleetProvenance,
    /// Model-major, then account, in declared order.
    pub routes: Vec<FleetRoute>,
    /// What the policy itself removed from the chain, and why, in chain order.
    pub excluded: Vec<PolicyExclusion>,
}

/// Why a chain entry offers no route, or why a route was passed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// The policy lists the model's domain under `unavailable.domains`.
    UnavailableDomain,
    /// The policy lists the account under `unavailable.accounts`.
    UnavailableAccount,
    /// The seat is calibration-scoped and the model is not calibrated.
    NotCalibrated,
    /// The seat is vision-scoped and the model has no vision.
    NoVision,
    /// The caller observed the account unavailable for this selection.
    AccountUnavailableNow,
    /// The seat must avoid the model's vendor for this selection.
    VendorExcluded,
}

/// One chain entry, or one of its accounts, that the policy removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PolicyExclusion {
    /// The chain step, counted from one.
    pub step: u16,
    /// The provider-native model id.
    pub model: String,
    /// The account alias, when one account alone was removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Why it offers no route.
    pub reason: ExclusionReason,
}

/// The runtime facts one selection is made under. None of this is policy:
/// it is what the caller observes now, stated explicitly so the choice can be
/// reproduced from the receipt.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Eligibility {
    /// Account aliases that cannot take a seat right now — exhausted, signed
    /// out or blocked by an operator.
    pub unavailable_accounts: BTreeSet<String>,
    /// Vendors the seat must avoid, such as the vendor of the seat it must be
    /// independent of.
    pub excluded_vendors: BTreeSet<String>,
}

/// One route a selection considered, and why it was passed over if it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConsideredRoute {
    /// The route, in chain order.
    pub route: FleetRoute,
    /// `None` for the route selected or any eligible route after it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluded: Option<ExclusionReason>,
}

/// The policy's choice for one seat under stated eligibility: what a launch
/// receipt records.
///
/// `selected` is the first route in chain order that the eligibility admits;
/// `None` is the defined block result, and `considered` then says why every
/// route was passed over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FleetSelection {
    /// The exact policy and binding the choice came from.
    pub provenance: FleetProvenance,
    /// The eligibility the choice was made under.
    pub eligibility: Eligibility,
    /// The chosen route, or `None` when nothing is eligible.
    pub selected: Option<FleetRoute>,
    /// Every resolved route, in chain order, with its verdict.
    pub considered: Vec<ConsideredRoute>,
    /// What the policy removed before any selection.
    pub excluded_by_policy: Vec<PolicyExclusion>,
}

impl FleetResolution {
    /// Choose the first route the stated eligibility admits.
    ///
    /// Deterministic: equal policy bytes, key and eligibility give an equal
    /// selection in every mode, because the order is the chain's and nothing
    /// else is read.
    #[must_use]
    pub fn select(&self, eligibility: &Eligibility) -> FleetSelection {
        let mut selected = None;
        let considered = self
            .routes
            .iter()
            .map(|route| {
                let excluded = if eligibility
                    .unavailable_accounts
                    .contains(&route.rung.provider.0)
                {
                    Some(ExclusionReason::AccountUnavailableNow)
                } else if eligibility.excluded_vendors.contains(&route.vendor) {
                    Some(ExclusionReason::VendorExcluded)
                } else {
                    None
                };
                if excluded.is_none() && selected.is_none() {
                    selected = Some(route.clone());
                }
                ConsideredRoute {
                    route: route.clone(),
                    excluded,
                }
            })
            .collect();
        FleetSelection {
            provenance: self.provenance.clone(),
            eligibility: eligibility.clone(),
            selected,
            considered,
            excluded_by_policy: self.excluded.clone(),
        }
    }

    /// The resolved route that is exactly `rung`, or `None` when the bound
    /// chain does not admit it.
    ///
    /// This is the one answer to "may a caller name this route for this seat":
    /// the account alias, model and effort must all match a route the chain
    /// still offers after the unavailable, calibration and vision rules.
    #[must_use]
    pub fn route_for(&self, rung: &ModelRung) -> Option<&FleetRoute> {
        self.routes.iter().find(|route| route.rung == *rung)
    }
}

/// A validated, hashed view of one fleet document.
#[derive(Debug)]
pub struct FleetSnapshot {
    document: FleetDocument,
    raw: String,
    hash: ContentHash,
}

impl FleetSnapshot {
    /// Parse and fully validate a schema_version 1 document.
    ///
    /// This is the unmigrated `fleet.yml` reader, and it reads no successor:
    /// any other `schema_version` is refused with V-01.
    ///
    /// # Errors
    /// Returns [`FleetError::Document`] for malformed YAML or unknown fields and
    /// [`FleetError::Invalid`] for the first violated rule of Appendix B.
    pub fn parse(document: &str) -> Result<Self, FleetError> {
        Self::read(document, Reader::Fleet)
    }

    /// Parse and fully validate a published policy of schema_version 1 or 2.
    ///
    /// A version 1 document is held to exactly the checks [`Self::parse`]
    /// applies. A version 2 document is held to the same checks and may also
    /// bind `leadership/<core-team-revision-hash>/<role-slot-id>` keys.
    ///
    /// # Errors
    /// Returns [`FleetError::PolicyDocument`] for malformed YAML or unknown
    /// fields, V-31 for any other version, and [`FleetError::Invalid`] for the
    /// first violated rule.
    pub fn parse_policy(document: &str) -> Result<Self, FleetError> {
        Self::read(document, Reader::Policy)
    }

    /// Re-admit policy bytes read back under the content address they were
    /// published at.
    ///
    /// # Errors
    /// P-07 when the bytes are not that address, then as [`Self::parse_policy`].
    pub fn published(hash: &ContentHash, document: &str) -> Result<Self, FleetError> {
        if ContentHash::of(document.as_bytes()) != *hash {
            return Err(invalid(P07));
        }
        Self::parse_policy(document)
    }

    /// Admit the policy an activation record selects: exactly the bytes at its
    /// content address, validating under the schema it names.
    ///
    /// This is the one activation decision every reader shares — the daemon
    /// and any direct-mode reader alike. Bytes that are not the activated
    /// ones, however valid, are refused, so an edited authoring file, an
    /// unactivated publication or a rewritten artifact never becomes the
    /// policy a placement reads.
    ///
    /// # Errors
    /// As [`Self::published`], and A-09 when the schema differs from the record.
    pub fn activated(
        hash: &ContentHash,
        schema_version: u32,
        document: &str,
    ) -> Result<Self, FleetError> {
        let snapshot = Self::published(hash, document)?;
        if snapshot.schema_version() != schema_version {
            return Err(invalid(A09));
        }
        Ok(snapshot)
    }

    fn read(document: &str, reader: Reader) -> Result<Self, FleetError> {
        if document.len() as u64 > MAX_FILE_BYTES {
            return Err(invalid(F04));
        }
        let parsed: FleetDocument =
            serde_yaml_ng::from_str(document).map_err(|_| match reader {
                Reader::Fleet => FleetError::Document,
                Reader::Policy => FleetError::PolicyDocument,
            })?;
        let schema = match (parsed.schema_version, reader) {
            (1, _) => Schema::V1,
            (2, Reader::Policy) => Schema::V2,
            (_, Reader::Policy) => return Err(invalid(V31)),
            (_, Reader::Fleet) => return Err(invalid(V01)),
        };
        parsed.validate(schema)?;
        if reader == Reader::Policy {
            parsed.validate_publishable()?;
        }
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

    /// The schema version the document was validated under.
    #[must_use]
    pub fn schema_version(&self) -> u32 {
        self.document.schema_version
    }

    /// The flattened routes bound to a team, committee or advisor key, or
    /// `None` when nothing binds it.
    ///
    /// The result may be `Some(vec![])` when every route was filtered out; the
    /// caller turns that into the R-01 placement refusal.
    #[must_use]
    pub fn routes_for(&self, key: &str) -> Option<Vec<FleetRoute>> {
        self.resolve(key).map(|resolution| resolution.routes)
    }

    /// Resolve a team, committee or advisor key, with its provenance.
    ///
    /// A leadership key is never resolved from text: the string form of one is
    /// `None` here even when the policy binds it. Use
    /// [`Self::resolve_leadership`] with a key built from the pinned seat.
    #[must_use]
    pub fn resolve(&self, key: &str) -> Option<FleetResolution> {
        if !is_binding_key(key) {
            return None;
        }
        self.resolve_bound(key)
    }

    /// Resolve one proved leadership seat, with its provenance.
    #[must_use]
    pub fn resolve_leadership(&self, key: &LeadershipKey) -> Option<FleetResolution> {
        self.resolve_bound(key.as_str())
    }

    fn resolve_bound(&self, key: &str) -> Option<FleetResolution> {
        let chain_name = self.document.bindings.get(key)?;
        let chain = self.document.chains.get(chain_name)?;
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
        let mut excluded = Vec::new();
        for (index, step) in chain.iter().enumerate() {
            let step_number = u16::try_from(index + 1).unwrap_or(u16::MAX);
            let mut sub_step = 0u16;
            for entry in step {
                let (name, effort_text) = entry_parts(entry);
                let Some(model) = self.document.models.get(name) else {
                    continue;
                };
                let whole_model = if self
                    .document
                    .unavailable
                    .domains
                    .iter()
                    .any(|domain| domain == &model.domain)
                {
                    Some(ExclusionReason::UnavailableDomain)
                } else if calibration_required && !model.calibrated {
                    Some(ExclusionReason::NotCalibrated)
                } else if vision_required && !model.vision {
                    Some(ExclusionReason::NoVision)
                } else {
                    None
                };
                if let Some(reason) = whole_model {
                    excluded.push(PolicyExclusion {
                        step: step_number,
                        model: model.id.clone(),
                        account: None,
                        reason,
                    });
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
                        excluded.push(PolicyExclusion {
                            step: step_number,
                            model: model.id.clone(),
                            account: Some(account.clone()),
                            reason: ExclusionReason::UnavailableAccount,
                        });
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
        Some(FleetResolution {
            provenance: FleetProvenance {
                policy_hash: self.hash.clone(),
                schema_version: self.document.schema_version,
                binding_key: key.to_owned(),
                chain: chain_name.clone(),
            },
            routes,
            excluded,
        })
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
    fn validate(&self, schema: Schema) -> Result<(), FleetError> {
        self.validate_domains()?;
        self.validate_models()?;
        self.validate_chains()?;
        self.validate_bindings(schema)?;
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

    fn validate_bindings(&self, schema: Schema) -> Result<(), FleetError> {
        for key in self.bindings.keys() {
            if !schema.accepts_key(key) {
                return Err(invalid(schema.key_rule()));
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

    /// The publication rule: no value a published policy carries may be a
    /// credential, a filesystem path (a provider home) or an email address (a
    /// provider-native account identity). Account aliases, model ids and keys
    /// are names; anything that authenticates or locates an account stays in
    /// the Realm's credential homes, never in shared policy.
    fn validate_publishable(&self) -> Result<(), FleetError> {
        let mut values: Vec<&str> = Vec::new();
        for (name, domain) in &self.domains {
            values.extend([name.as_str(), domain.provider.as_str()]);
            values.extend(domain.accounts.iter().map(String::as_str));
            values.extend(domain.model_prefix.as_deref());
        }
        values.extend(self.unavailable.domains.iter().map(String::as_str));
        values.extend(self.unavailable.accounts.iter().map(String::as_str));
        for (name, model) in &self.models {
            values.extend([
                name.as_str(),
                model.domain.as_str(),
                model.id.as_str(),
                model.vendor.as_str(),
            ]);
            values.extend(model.efforts.iter().map(String::as_str));
        }
        for (name, steps) in &self.chains {
            values.push(name.as_str());
            for entry in steps.iter().flatten() {
                let (model, effort) = entry_parts(entry);
                values.push(model);
                values.extend(effort);
            }
        }
        for (key, chain) in &self.bindings {
            values.extend([key.as_str(), chain.as_str()]);
        }
        values.extend(self.rules.calibration_required.iter().map(String::as_str));
        values.extend(self.rules.vision_required.iter().map(String::as_str));
        for (key, other) in &self.rules.independent_of {
            values.extend([key.as_str(), other.as_str()]);
        }
        for value in values {
            let located = value.starts_with('/') || value.starts_with('~') || value.contains('\\');
            if located
                || value.contains('@')
                || kontor_core::id::reject_sensitive_text("fleet", value).is_err()
            {
                return Err(invalid(V33));
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

/// `leadership/<core-team-revision-hash>/<role-slot-id>`, and nothing shorter.
///
/// The hash must be a full canonical content hash, so a role code, a slot id or
/// a display label in that position is refused rather than read as a roster.
fn is_leadership_key(key: &str) -> bool {
    let mut parts = key.split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("leadership"), Some(hash), Some(slot), None) => {
            ContentHash::parse(hash).is_ok() && RoleSlotId::parse(slot).is_ok()
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
mod tests;
