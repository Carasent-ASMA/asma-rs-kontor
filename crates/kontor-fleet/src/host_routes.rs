//! Q-04: pure resolution of canonical fleet identity against frozen host inputs.
//! This module reads no files, holds no credentials, and grants no runtime effect.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::id::{ContentHash, Timestamp};
use kontor_core::launch_route::{ResolvedRoute, host_route_hash};
use kontor_core::spec::{EffortLevel, ModelRung};
use serde::Deserialize;

use crate::{FleetError, FleetResolution, FleetRoute, FleetSnapshot, invalid, normalize_provider};

/// Exact project pair allowlist, parsed only from a pinned orchestration artifact.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimePair {
    /// Optional presentation label; it confers no identity.
    #[serde(default)]
    pub name: Option<String>,
    /// Exact working harness.
    pub harness: String,
    /// Exact working provider family.
    pub provider: String,
}

/// The allowlist carried by one immutable orchestration source.
#[derive(Debug, Clone)]
pub struct RuntimeMatrix(Vec<RuntimePair>);

impl RuntimeMatrix {
    /// Parse a pinned source; an absent matrix authorizes no routes.
    ///
    /// # Errors
    /// Invalid/ambiguous pairs refuse without configured values in diagnostics.
    pub fn parse(source: &str) -> Result<Self, FleetError> {
        #[derive(Deserialize)]
        struct Source {
            project: Option<Project>,
        }
        #[derive(Deserialize)]
        struct Project {
            runtime: Option<Runtime>,
        }
        #[derive(Deserialize)]
        struct Runtime {
            matrix: Vec<RuntimePair>,
        }
        let source: Source = serde_yaml_ng::from_str(source)
            .map_err(|_| invalid("project_runtime_matrix_invalid"))?;
        let matrix = source
            .project
            .and_then(|project| project.runtime)
            .map_or_else(Vec::new, |runtime| runtime.matrix);
        let mut seen = BTreeSet::new();
        for pair in &matrix {
            if !token(&pair.harness)
                || normalize_provider(&pair.provider) != Some(pair.provider.as_str())
                || !seen.insert((&pair.harness, &pair.provider))
            {
                return Err(invalid("project_runtime_matrix_invalid"));
            }
        }
        Ok(Self(matrix))
    }

    /// Whether the exact working pair appears in this frozen source.
    #[must_use]
    pub fn allows(&self, harness: &str, provider: &str) -> bool {
        self.0
            .iter()
            .any(|pair| pair.harness == harness && pair.provider == provider)
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostAccount {
    provider: String,
    harness: String,
    native_alias: String,
    credential: String,
    serves: BTreeMap<String, String>,
    efforts: Option<Vec<EffortLevel>>,
    qualification: Option<String>,
}

/// Private bootstrap input. Debug deliberately excludes host selectors/references.
#[derive(Clone)]
pub struct HostRoutes {
    host_bytes: Vec<u8>,
    accounts: BTreeMap<String, HostAccount>,
    routes: BTreeMap<String, Vec<String>>,
}

impl std::fmt::Debug for HostRoutes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostRoutes").finish_non_exhaustive()
    }
}

impl HostRoutes {
    /// Validate exact host bytes once; unrelated host bootstrap sections stay with their owners.
    ///
    /// # Errors
    /// Missing, malformed and duplicate host route declarations refuse by field.
    pub fn parse(text: &str) -> Result<Self, FleetError> {
        #[derive(Deserialize)]
        struct Host {
            schema_version: u32,
            provider_accounts: BTreeMap<String, HostAccount>,
            routes: BTreeMap<String, Vec<String>>,
        }
        let value: serde_yaml_ng::Value =
            serde_yaml_ng::from_str(text).map_err(|_| invalid("host_routes_invalid"))?;
        let host: Host =
            serde_yaml_ng::from_value(value).map_err(|_| invalid("host_routes_invalid"))?;
        if host.schema_version != 1 {
            return Err(invalid("host_routes_invalid"));
        }
        for (slot, account) in &host.provider_accounts {
            if !token(slot)
                || normalize_provider(&account.provider) != Some(account.provider.as_str())
                || !token(&account.harness)
                || !selector(&account.native_alias)
                || !account
                    .credential
                    .split_once(':')
                    .is_some_and(|(kind, reference)| {
                        matches!(kind, "keychain" | "env" | "file") && selector(reference)
                    })
                || account
                    .serves
                    .iter()
                    .any(|(model, id)| !token(model) || !selector(id))
                || account.efforts.as_ref().is_some_and(|efforts| {
                    efforts
                        .iter()
                        .map(|effort| effort.as_str())
                        .collect::<BTreeSet<_>>()
                        .len()
                        != efforts.len()
                })
            {
                return Err(invalid("host_provider_account_invalid"));
            }
            if let Some(reference) = &account.qualification {
                qualification_reference(reference)?;
            }
        }
        for (domain, slots) in &host.routes {
            if !token(domain)
                || slots.iter().collect::<BTreeSet<_>>().len() != slots.len()
                || slots
                    .iter()
                    .any(|slot| !host.provider_accounts.contains_key(slot))
            {
                return Err(invalid("host_routes_invalid"));
            }
        }
        Ok(Self {
            host_bytes: text.as_bytes().to_vec(),
            accounts: host.provider_accounts,
            routes: host.routes,
        })
    }

    /// Freeze one exact logical slot after canonical permission/eligibility admission.
    ///
    /// # Errors
    /// Never changes canonical model, effort or vendor to fit a host route.
    pub fn resolve(
        &self,
        fleet: &FleetSnapshot,
        rung: &ModelRung,
        matrix: &RuntimeMatrix,
        qualifications: Option<&QualificationSet>,
        now: Timestamp,
    ) -> Result<ResolvedRoute, FleetError> {
        if !fleet.lists(rung) {
            return Err(invalid("host_route_canonical_unproven"));
        }
        let model = fleet
            .model_of(rung)
            .ok_or_else(|| invalid("host_route_canonical_unproven"))?;
        let domain = &fleet.document.domains[&model.domain];
        let slots = self
            .routes
            .get(&model.domain)
            .ok_or_else(|| invalid("host_route_domain_unavailable"))?;
        if slots.iter().any(|slot| !domain.accounts.contains(slot)) {
            return Err(invalid("host_route_slot_domain_mismatch"));
        }
        if !slots.contains(&rung.provider.0) {
            return Err(invalid("host_route_slot_unavailable"));
        }
        let account = self
            .accounts
            .get(&rung.provider.0)
            .ok_or_else(|| invalid("host_route_slot_unavailable"))?;
        if !matrix.allows(&account.harness, &account.provider) {
            return Err(invalid("runtime_pair_not_allowed"));
        }
        let symbolic = fleet
            .document
            .models
            .iter()
            .find(|(_, candidate)| std::ptr::eq(*candidate, model))
            .map(|(name, _)| name)
            .ok_or_else(|| invalid("host_route_canonical_unproven"))?;
        let native_model = account
            .serves
            .get(symbolic)
            .ok_or_else(|| invalid("route_not_served"))?;
        if account.efforts.as_ref().is_some_and(|efforts| {
            rung.effort
                .map_or(!efforts.is_empty(), |effort| !efforts.contains(&effort))
        }) {
            return Err(invalid("route_effort_unsupported"));
        }
        match account.qualification.as_deref() {
            Some(reference) => {
                let qualification =
                    qualifications.ok_or_else(|| invalid("route_qualification_unproven"))?;
                if &qualification.policy_hash != fleet.hash() {
                    return Err(invalid("route_qualification_unproven"));
                }
                qualification.prove(reference, account, symbolic, model, rung.effort)?;
            }
            None if account.provider != domain.provider => {
                return Err(invalid("route_qualification_unproven"));
            }
            None if native_model != &model.id => return Err(invalid("route_model_id_unproven")),
            None => {}
        }
        Ok(ResolvedRoute {
            schema_version: 1,
            slot: rung.provider.0.clone(),
            route_provider: account.provider.clone(),
            route_harness: account.harness.clone(),
            native_alias: account.native_alias.clone(),
            qualification: account.qualification.clone(),
            route_model_id: native_model.clone(),
            canonical_model: model.id.clone(),
            canonical_effort: rung.effort,
            canonical_vendor: model.vendor.clone(),
            policy_hash: fleet.hash().clone(),
            host_route_hash: host_route_hash(&self.host_bytes, &model.domain, slots),
            resolved_at: now,
        })
    }

    /// Host order within each unchanged canonical chain step; incompatible routes descend.
    ///
    /// # Errors
    /// No compatible route or no independent vendor is a named refusal.
    pub fn candidates(
        &self,
        fleet: &FleetSnapshot,
        resolution: &FleetResolution,
        matrix: &RuntimeMatrix,
        qualifications: Option<&QualificationSet>,
        excluded_vendors: &BTreeSet<String>,
        now: Timestamp,
    ) -> Result<Vec<(FleetRoute, ResolvedRoute)>, FleetError> {
        if fleet
            .resolve_bound(&resolution.provenance.binding_key)
            .as_ref()
            != Some(resolution)
        {
            return Err(invalid("host_route_policy_mismatch"));
        }
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        let mut reason = "host_route_unavailable";
        for route in &resolution.routes {
            let identity = (
                route.step,
                route.rung.model.0.clone(),
                route.rung.effort.map(|effort| effort.as_str()),
            );
            if !seen.insert(identity) {
                continue;
            }
            if excluded_vendors.contains(&route.vendor) {
                reason = "vendor_independence_unpreserved";
                continue;
            }
            let model = fleet
                .model_of(&route.rung)
                .ok_or_else(|| invalid("host_route_canonical_unproven"))?;
            let Some(slots) = self.routes.get(&model.domain) else {
                continue;
            };
            for slot in slots {
                let Some(candidate) = resolution.routes.iter().find(|candidate| {
                    candidate.step == route.step
                        && candidate.rung.model == route.rung.model
                        && candidate.rung.effort == route.rung.effort
                        && candidate.rung.provider.0 == *slot
                }) else {
                    continue;
                };
                match self.resolve(fleet, &candidate.rung, matrix, qualifications, now) {
                    Ok(resolved) => result.push((candidate.clone(), resolved)),
                    Err(FleetError::Invalid { rule }) => reason = rule,
                    Err(error) => return Err(error),
                }
            }
        }
        if result.is_empty() {
            return Err(invalid(reason));
        }
        Ok(result)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QualificationRecord {
    route: String,
    harness: String,
    model: String,
    canonical_model_id: String,
    canonical_vendor: String,
    route_model_id: String,
    efforts: Vec<EffortLevel>,
    revision: String,
    record_hash: ContentHash,
}

impl QualificationRecord {
    fn hash(&self) -> ContentHash {
        ContentHash::of(format!("route={}\nharness={}\nmodel={}\ncanonical_model_id={}\ncanonical_vendor={}\nroute_model_id={}\nefforts={}\nrevision={}",
            self.route, self.harness, self.model, self.canonical_model_id, self.canonical_vendor,
            self.route_model_id, self.efforts.iter().map(|effort| effort.as_str()).collect::<Vec<_>>().join(","), self.revision).as_bytes())
    }
}

/// Validated exact qualification bytes; activation is supplied only by the pinned reader.
#[derive(Debug)]
pub struct QualificationSet {
    records: Vec<QualificationRecord>,
    activated_at: Option<Timestamp>,
    policy_hash: ContentHash,
}

impl QualificationSet {
    /// Validate every record and cross-reference against the same exact fleet policy.
    /// This validates candidates; it does not activate them.
    ///
    /// # Errors
    /// Malformed, duplicate, incorrectly hashed or contradictory records refuse.
    pub fn parse(text: &str, fleet: &FleetSnapshot) -> Result<Self, FleetError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Source {
            schema_version: u32,
            records: Vec<QualificationRecord>,
        }
        let source: Source =
            serde_yaml_ng::from_str(text).map_err(|_| invalid("route_qualification_malformed"))?;
        if source.schema_version != 1 {
            return Err(invalid("route_qualification_malformed"));
        }
        let mut seen = BTreeSet::new();
        for record in &source.records {
            if !seen.insert((&record.route, &record.revision))
                || normalize_provider(&record.route) != Some(record.route.as_str())
                || !token(&record.harness)
                || !token(&record.revision)
                || !selector(&record.route_model_id)
                || record
                    .efforts
                    .iter()
                    .map(|effort| effort.as_str())
                    .collect::<BTreeSet<_>>()
                    .len()
                    != record.efforts.len()
            {
                return Err(invalid("route_qualification_malformed"));
            }
            if record.hash() != record.record_hash {
                return Err(invalid("route_qualification_hash_mismatch"));
            }
            let model = fleet
                .document
                .models
                .get(&record.model)
                .ok_or_else(|| invalid("route_qualification_unproven"))?;
            if model.id != record.canonical_model_id
                || model.vendor != record.canonical_vendor
                || record.efforts.iter().any(|effort| {
                    !model
                        .efforts
                        .iter()
                        .any(|allowed| allowed == effort.as_str())
                })
                || (record.efforts.is_empty() != model.efforts.is_empty())
            {
                return Err(invalid("route_qualification_unproven"));
            }
        }
        Ok(Self {
            records: source.records,
            activated_at: None,
            policy_hash: fleet.hash().clone(),
        })
    }

    /// Admit exact retained bytes under the already-linked FleetActivation record.
    ///
    /// # Errors
    /// Source hash disagreement refuses; candidate validation alone grants nothing.
    pub fn activated(
        text: &str,
        source_hash: &ContentHash,
        fleet: &FleetSnapshot,
        activated_at: Timestamp,
    ) -> Result<Self, FleetError> {
        if &ContentHash::of(text.as_bytes()) != source_hash {
            return Err(invalid("route_qualification_hash_mismatch"));
        }
        let mut records = Self::parse(text, fleet)?;
        records.activated_at = Some(activated_at);
        Ok(records)
    }

    fn prove(
        &self,
        reference: &str,
        account: &HostAccount,
        symbolic: &str,
        model: &crate::ModelSpec,
        effort: Option<EffortLevel>,
    ) -> Result<(), FleetError> {
        if self.activated_at.is_none() {
            return Err(invalid("route_qualification_unproven"));
        }
        let (route, revision) = qualification_reference(reference)?;
        let record = self
            .records
            .iter()
            .find(|record| record.route == route && record.revision == revision)
            .ok_or_else(|| invalid("route_qualification_unproven"))?;
        if route != account.provider
            || record.harness != account.harness
            || record.model != symbolic
            || record.canonical_model_id != model.id
            || record.canonical_vendor != model.vendor
            || account.serves.get(symbolic) != Some(&record.route_model_id)
            || effort.map_or(!record.efforts.is_empty(), |effort| {
                !record.efforts.contains(&effort)
            })
        {
            return Err(invalid("route_qualification_unproven"));
        }
        Ok(())
    }
}

fn qualification_reference(reference: &str) -> Result<(&str, &str), FleetError> {
    reference
        .split_once('@')
        .filter(|(route, revision)| token(route) && token(revision) && !revision.contains('@'))
        .ok_or_else(|| invalid("route_qualification_malformed"))
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

fn selector(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontor_core::spec::{ModelRef, ProviderRef};
    const FLEET: &str = include_str!("../../../config/examples/orchestration/fleet.yml");
    const HOST: &str = include_str!("../../../config/examples/host.example.yml");
    const ALTERNATE: &str = include_str!("../../../tests/fixtures/config/host-alternate.yml");
    const CROSS: &str = include_str!("../../../tests/fixtures/config/host-cross-provider.yml");
    const PROJECT: &str = include_str!("../../../config/examples/orchestration/orchestration.yml");
    const QUALIFICATIONS: &str =
        include_str!("../../../config/examples/orchestration/route-qualifications.yml");

    fn rung(slot: &str, model: &str, effort: EffortLevel) -> ModelRung {
        ModelRung {
            provider: ProviderRef(slot.into()),
            model: ModelRef(model.into()),
            effort: Some(effort),
        }
    }

    fn rule<T: std::fmt::Debug>(result: Result<T, FleetError>) -> &'static str {
        match result.unwrap_err() {
            FleetError::Invalid { rule } => rule,
            error => panic!("{error}"),
        }
    }

    fn activated(fleet: &FleetSnapshot) -> QualificationSet {
        QualificationSet::activated(
            QUALIFICATIONS,
            &ContentHash::of(QUALIFICATIONS.as_bytes()),
            fleet,
            Timestamp::now(),
        )
        .unwrap()
    }

    #[test]
    fn host_aliases_and_order_change_only_the_private_snapshot() {
        let fleet = FleetSnapshot::parse_policy(FLEET).unwrap();
        let matrix = RuntimeMatrix::parse(PROJECT).unwrap();
        let canonical = rung("claude-primary", "claude-opus-example", EffortLevel::Xhigh);
        let now = Timestamp::now();
        let first = HostRoutes::parse(HOST)
            .unwrap()
            .resolve(&fleet, &canonical, &matrix, None, now)
            .unwrap();
        let other = HostRoutes::parse(ALTERNATE)
            .unwrap()
            .resolve(&fleet, &canonical, &matrix, None, now)
            .unwrap();
        assert_eq!(first.canonical_rung(), other.canonical_rung());
        assert_eq!(first.canonical_vendor, other.canonical_vendor);
        assert_ne!(first.native_alias, other.native_alias);
        assert_ne!(first.host_route_hash, other.host_route_hash);
        assert!(!format!("{first:?}").contains(&first.native_alias));
        let resolution = fleet
            .resolve("team/01936f5a-0000-7000-8000-000000000001/implement")
            .unwrap();
        let candidates = HostRoutes::parse(ALTERNATE)
            .unwrap()
            .candidates(&fleet, &resolution, &matrix, None, &BTreeSet::new(), now)
            .unwrap();
        assert_eq!(candidates[0].1.slot, "claude-secondary");
        assert_eq!(candidates[0].1.canonical_model, first.canonical_model);
        assert_eq!(first.native_alias, "laptop-claude-1");
    }

    #[test]
    fn qualified_cross_provider_preserves_canonical_vendor_model_and_effort() {
        let fleet = FleetSnapshot::parse_policy(FLEET).unwrap();
        let matrix = RuntimeMatrix::parse(PROJECT).unwrap();
        let canonical = rung("opencode", "example/model", EffortLevel::Max);
        let host = HostRoutes::parse(CROSS).unwrap();
        let now = Timestamp::now();
        assert_eq!(
            rule(host.resolve(&fleet, &canonical, &matrix, None, now)),
            "route_qualification_unproven"
        );
        let candidate = QualificationSet::parse(QUALIFICATIONS, &fleet).unwrap();
        assert_eq!(
            rule(host.resolve(&fleet, &canonical, &matrix, Some(&candidate), now)),
            "route_qualification_unproven"
        );
        let qualified = activated(&fleet);
        let resolved = host
            .resolve(&fleet, &canonical, &matrix, Some(&qualified), now)
            .unwrap();
        assert_eq!(resolved.route_provider, "glm-acp-agent");
        assert_eq!(resolved.route_harness, "acp-agent");
        assert_eq!(resolved.route_model_id, "glm-5.3");
        assert_eq!(resolved.canonical_rung(), canonical);
        assert_eq!(resolved.canonical_vendor, "example");
        let resolution = fleet
            .resolve("team/01936f5a-0000-7000-8000-000000000001/open")
            .unwrap();
        assert_eq!(
            rule(host.candidates(
                &fleet,
                &resolution,
                &matrix,
                Some(&qualified),
                &BTreeSet::from(["example".into()]),
                now
            )),
            "vendor_independence_unpreserved"
        );
    }

    #[test]
    fn qualification_hash_revision_identity_and_matrix_refuse_closed() {
        let fleet = FleetSnapshot::parse_policy(FLEET).unwrap();
        let matrix = RuntimeMatrix::parse(PROJECT).unwrap();
        let canonical = rung("opencode", "example/model", EffortLevel::Max);
        let qualified = activated(&fleet);
        let now = Timestamp::now();
        let edited =
            FleetSnapshot::parse_policy(&format!("{FLEET}\n# another exact policy identity\n"))
                .unwrap();
        assert_eq!(
            rule(HostRoutes::parse(CROSS).unwrap().resolve(
                &edited,
                &canonical,
                &matrix,
                Some(&qualified),
                now
            )),
            "route_qualification_unproven"
        );
        let changed = CROSS.replace("glm-acp-agent@1", "glm-acp-agent@2");
        assert_eq!(
            rule(HostRoutes::parse(&changed).unwrap().resolve(
                &fleet,
                &canonical,
                &matrix,
                Some(&qualified),
                now
            )),
            "route_qualification_unproven"
        );
        assert_eq!(
            rule(QualificationSet::parse(
                &QUALIFICATIONS.replace("glm-5.3", "other-id"),
                &fleet
            )),
            "route_qualification_hash_mismatch"
        );
        assert_eq!(
            rule(QualificationSet::activated(
                QUALIFICATIONS,
                &ContentHash::of(b"other"),
                &fleet,
                now
            )),
            "route_qualification_hash_mismatch"
        );
        let empty = RuntimeMatrix::parse("project: {}\n").unwrap();
        assert_eq!(
            rule(HostRoutes::parse(CROSS).unwrap().resolve(
                &fleet,
                &canonical,
                &empty,
                Some(&qualified),
                now
            )),
            "runtime_pair_not_allowed"
        );
        assert_eq!(
            rule(HostRoutes::parse(
                &CROSS.replace("glm-acp-agent@1", "glm-acp-agent@1@2")
            )),
            "route_qualification_malformed"
        );
        assert_eq!(
            rule(HostRoutes::parse(&CROSS.replace(
                "provider: glm-acp-agent",
                "provider: unknown-provider"
            ))),
            "host_provider_account_invalid"
        );
    }

    #[test]
    fn duplicate_slots_domains_and_serves_mappings_are_ambiguous_sources() {
        let duplicate_slot = HOST.replace("  codex-primary:\n", "  codex-primary:\n    provider: codex\n    harness: paseo\n    native_alias: synthetic-first-account\n    credential: keychain:synthetic/reference\n    serves: {sol-example: gpt-example}\n  codex-primary:\n");
        let duplicate_domain = HOST.replace(
            "  codex: [codex-primary]",
            "  codex: []\n  codex: [codex-primary]",
        );
        let duplicate_model = HOST.replace(
            "sol-example: gpt-example",
            "sol-example: unproven-model\n      sol-example: gpt-example",
        );
        for text in [duplicate_slot, duplicate_domain, duplicate_model] {
            assert_eq!(rule(HostRoutes::parse(&text)), "host_routes_invalid");
        }
    }

    #[test]
    fn unserved_or_unsupported_routes_descend_without_changing_the_canonical_rung() {
        let fleet = FleetSnapshot::parse_policy(FLEET).unwrap();
        let matrix = RuntimeMatrix::parse(PROJECT).unwrap();
        let host = HostRoutes::parse(CROSS).unwrap();
        let now = Timestamp::now();
        let low = rung("opencode", "example/model", EffortLevel::Low);
        assert_eq!(
            rule(host.resolve(&fleet, &low, &matrix, Some(&activated(&fleet)), now)),
            "route_effort_unsupported"
        );
        let resolution = fleet
            .resolve("team/01936f5a-0000-7000-8000-000000000001/fallback")
            .unwrap();
        let result = host
            .candidates(
                &fleet,
                &resolution,
                &matrix,
                Some(&activated(&fleet)),
                &BTreeSet::new(),
                now,
            )
            .unwrap();
        assert_eq!(result[0].1.canonical_model, "gpt-example");
        assert_eq!(result[0].1.canonical_effort, Some(EffortLevel::Xhigh));
        let changed = HOST.replace("sol-example: gpt-example", "sol-example: unproven-id");
        let canonical = rung("codex-primary", "gpt-example", EffortLevel::Xhigh);
        assert_eq!(
            rule(
                HostRoutes::parse(&changed)
                    .unwrap()
                    .resolve(&fleet, &canonical, &matrix, None, now)
            ),
            "route_model_id_unproven"
        );
        let missing = HOST.replace("codex: [codex-primary]", "codex: []");
        assert_eq!(
            rule(
                HostRoutes::parse(&missing)
                    .unwrap()
                    .resolve(&fleet, &canonical, &matrix, None, now)
            ),
            "host_route_slot_unavailable"
        );
        let unserved = HOST.replace("sol-example: gpt-example", "other-model: gpt-example");
        assert_eq!(
            rule(
                HostRoutes::parse(&unserved)
                    .unwrap()
                    .resolve(&fleet, &canonical, &matrix, None, now)
            ),
            "route_not_served"
        );
    }
}
