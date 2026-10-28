//! Private launch persistence. These values must never be projected onto API or native labels.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::id::{ContentHash, Timestamp};
use crate::spec::{EffortLevel, ModelRef, ModelRung, ProviderRef};
use crate::{DomainError, DomainResult};

/// The immutable host/policy join resolved before a native effect (PIN-001).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedRoute {
    /// Private carrier format; currently exactly one.
    pub schema_version: u32,
    /// Canonical logical account slot.
    pub slot: String,
    /// Working route provider, independent of canonical vendor.
    pub route_provider: String,
    /// Working route harness.
    pub route_harness: String,
    /// Host-only account selector; never publish or log it.
    pub native_alias: String,
    /// Exact activated route/revision reference, when required.
    pub qualification: Option<String>,
    /// Model id passed to the working route.
    pub route_model_id: String,
    /// Canonical fleet model id.
    pub canonical_model: String,
    /// Unchanged canonical effort.
    pub canonical_effort: Option<EffortLevel>,
    /// Canonical model vendor used for independence.
    pub canonical_vendor: String,
    /// Exact fleet content identity.
    pub policy_hash: ContentHash,
    /// Exact host bytes, domain and ordered slots identity.
    pub host_route_hash: ContentHash,
    /// Instant frozen before the effect.
    pub resolved_at: Timestamp,
}

impl fmt::Debug for ResolvedRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedRoute")
            .field("slot", &self.slot)
            .field("canonical_model", &self.canonical_model)
            .field("canonical_effort", &self.canonical_effort)
            .field("policy_hash", &self.policy_hash)
            .field("host_route_hash", &self.host_route_hash)
            .finish_non_exhaustive()
    }
}

impl ResolvedRoute {
    /// Validate persisted structure without consulting mutable host or policy inputs.
    ///
    /// # Errors
    /// Refuses unsupported or incomplete private snapshots without echoing their values.
    pub fn validate(&self) -> DomainResult<()> {
        if self.schema_version != 1
            || [
                &self.slot,
                &self.route_provider,
                &self.route_harness,
                &self.native_alias,
                &self.route_model_id,
                &self.canonical_model,
                &self.canonical_vendor,
            ]
            .iter()
            .any(|value| {
                value.is_empty()
                    || value.trim() != value.as_str()
                    || value.chars().any(char::is_control)
            })
            || self.qualification.as_ref().is_some_and(|value| {
                value.split_once('@').is_none_or(|(route, revision)| {
                    route.is_empty()
                        || revision.is_empty()
                        || revision.contains('@')
                        || value.chars().any(char::is_whitespace)
                })
            })
        {
            return Err(DomainError::invalid(
                "ResolvedRoute",
                "host_route_snapshot_invalid",
            ));
        }
        self.canonical_rung().validate()
    }

    /// Canonical comparison only; this is never the native launch selector.
    #[must_use]
    pub fn canonical_rung(&self) -> ModelRung {
        ModelRung {
            provider: ProviderRef(self.slot.clone()),
            model: ModelRef(self.canonical_model.clone()),
            effort: self.canonical_effort,
        }
    }
}

/// Versioned envelope in an existing private `model_rung` column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedRouteEnvelope {
    /// Envelope version, independently checked from the nested carrier version.
    pub schema_version: u32,
    /// Exact immutable private route.
    pub resolved_route: ResolvedRoute,
}

/// New launch snapshots use an envelope; historical bare rungs remain readable.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum StoredLaunchRoute {
    /// A route frozen under an explicit host/policy join.
    Resolved(Box<ResolvedRouteEnvelope>),
    /// Historical evidence; reading it does not qualify a new placement.
    Legacy(ModelRung),
}

impl fmt::Debug for StoredLaunchRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolved(envelope) => envelope.fmt(f),
            Self::Legacy(rung) => f
                .debug_struct("LegacyLaunchRoute")
                .field("canonical_model", &rung.model)
                .finish_non_exhaustive(),
        }
    }
}

impl<'de> Deserialize<'de> for StoredLaunchRoute {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let invalid = |_| serde::de::Error::custom("host_route_snapshot_invalid");
        if value.get("schema_version").is_some() || value.get("resolved_route").is_some() {
            let envelope = serde_json::from_value(value).map_err(invalid)?;
            let route = Self::Resolved(envelope);
            route
                .validate()
                .map_err(|_| serde::de::Error::custom("host_route_snapshot_invalid"))?;
            Ok(route)
        } else {
            // Historical evidence remains readable without applying new placement policy.
            serde_json::from_value(value)
                .map(Self::Legacy)
                .map_err(invalid)
        }
    }
}

/// Read-only interpretation of historical evidence, never a qualified host route.
#[derive(Clone, PartialEq, Eq)]
pub struct LegacyRouteView {
    /// Historical logical selector, preserved verbatim.
    pub slot: String,
    /// Historical native selector; private and never remapped.
    pub native_alias: String,
    /// Proven shared provider family.
    pub route_provider: String,
    /// Harness proven from the seat's recorded runtime kind.
    pub route_harness: String,
    /// Historical route launch id.
    pub route_model_id: String,
    /// Historical canonical model id.
    pub canonical_model: String,
    /// Historical canonical effort.
    pub canonical_effort: Option<EffortLevel>,
    /// Vendor proven by the same frozen historical evidence.
    pub canonical_vendor: String,
    /// Legacy evidence has no qualification grant.
    pub qualification: Option<String>,
    /// No invented policy pin.
    pub policy_hash: Option<ContentHash>,
    /// No invented host pin.
    pub host_route_hash: Option<ContentHash>,
}

impl fmt::Debug for LegacyRouteView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LegacyRouteView")
            .field("canonical_model", &self.canonical_model)
            .finish_non_exhaustive()
    }
}

impl LegacyRouteView {
    /// Interpret a historical rung using independently proven frozen harness/vendor facts.
    /// Neither current host configuration nor a current catalog is evidence here.
    ///
    /// # Errors
    /// Missing family, harness or vendor proof refuses `host_route_legacy_unproven`.
    pub fn from_frozen_evidence(
        rung: &ModelRung,
        harness: Option<&str>,
        vendor: Option<&str>,
    ) -> DomainResult<Self> {
        let unproven = || DomainError::invalid("LegacyRouteView", "host_route_legacy_unproven");
        let family = crate::provider::normalize_provider(&rung.provider.0).ok_or_else(unproven)?;
        let harness = harness
            .filter(|value| {
                !value.is_empty() && value.trim() == *value && !value.chars().any(char::is_control)
            })
            .ok_or_else(unproven)?;
        let vendor = vendor
            .filter(|value| {
                !value.is_empty()
                    && *value != "unknown"
                    && value.trim() == *value
                    && !value.chars().any(char::is_control)
            })
            .ok_or_else(unproven)?;
        if rung.model.0.trim().is_empty() {
            return Err(unproven());
        }
        Ok(Self {
            slot: rung.provider.0.clone(),
            native_alias: rung.provider.0.clone(),
            route_provider: family.into(),
            route_harness: harness.into(),
            route_model_id: rung.model.0.clone(),
            canonical_model: rung.model.0.clone(),
            canonical_effort: rung.effort,
            canonical_vendor: vendor.into(),
            qualification: None,
            policy_hash: None,
            host_route_hash: None,
        })
    }
}

impl From<ModelRung> for StoredLaunchRoute {
    fn from(value: ModelRung) -> Self {
        Self::Legacy(value)
    }
}

impl StoredLaunchRoute {
    /// Construct a validated envelope for a new launch.
    ///
    /// # Errors
    /// Refuses invalid snapshots.
    pub fn resolved(route: ResolvedRoute) -> DomainResult<Self> {
        route.validate()?;
        Ok(Self::Resolved(Box::new(ResolvedRouteEnvelope {
            schema_version: 1,
            resolved_route: route,
        })))
    }

    /// Check private persistence structure, without repairing historical evidence.
    ///
    /// # Errors
    /// Refuses unsupported envelope versions or invalid routes.
    pub fn validate(&self) -> DomainResult<()> {
        match self {
            Self::Resolved(envelope) if envelope.schema_version == 1 => {
                envelope.resolved_route.validate()
            }
            Self::Resolved(_) => Err(DomainError::invalid(
                "StoredLaunchRoute",
                "host_route_snapshot_invalid",
            )),
            Self::Legacy(rung) => rung.validate(),
        }
    }

    /// Canonical projection for existing seat bindings and public comparisons.
    #[must_use]
    pub fn canonical_rung(&self) -> ModelRung {
        match self {
            Self::Resolved(envelope) => envelope.resolved_route.canonical_rung(),
            Self::Legacy(rung) => rung.clone(),
        }
    }

    /// Private frozen host route, when one was actually recorded.
    #[must_use]
    pub fn frozen(&self) -> Option<&ResolvedRoute> {
        match self {
            Self::Resolved(envelope) => Some(&envelope.resolved_route),
            Self::Legacy(_) => None,
        }
    }
}

/// Exact host-route hash. Struct field order is part of the PIN-001 contract.
#[must_use]
pub fn host_route_hash(host_bytes: &[u8], domain: &str, slots: &[String]) -> ContentHash {
    #[derive(Serialize)]
    struct Identity<'a> {
        host_sha256: ContentHash,
        domain: &'a str,
        slots: &'a [String],
    }
    let bytes = serde_json::to_vec(&Identity {
        host_sha256: ContentHash::of(host_bytes),
        domain,
        slots,
    })
    .expect("host route identity contains only serializable strings");
    ContentHash::of(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> ResolvedRoute {
        ResolvedRoute {
            schema_version: 1,
            slot: "codex-primary".into(),
            route_provider: "codex".into(),
            route_harness: "paseo".into(),
            native_alias: "host-private-account".into(),
            qualification: None,
            route_model_id: "gpt-example".into(),
            canonical_model: "gpt-example".into(),
            canonical_effort: Some(EffortLevel::High),
            canonical_vendor: "example-vendor".into(),
            policy_hash: ContentHash::of(b"policy"),
            host_route_hash: ContentHash::of(b"host"),
            resolved_at: Timestamp::now(),
        }
    }

    #[test]
    fn envelope_and_historical_bare_rung_round_trip_without_public_alias() {
        let stored = StoredLaunchRoute::resolved(route()).unwrap();
        let text = serde_json::to_string(&stored).unwrap();
        assert!(text.starts_with("{\"schema_version\":1,\"resolved_route\":{"));
        assert_eq!(
            serde_json::from_str::<StoredLaunchRoute>(&text).unwrap(),
            stored
        );
        assert!(!format!("{stored:?}").contains("host-private-account"));
        assert!(
            !serde_json::to_string(&stored.canonical_rung())
                .unwrap()
                .contains("host-private-account")
        );
        let bare = serde_json::to_string(&stored.canonical_rung()).unwrap();
        let legacy: StoredLaunchRoute = serde_json::from_str(&bare).unwrap();
        assert!(matches!(legacy, StoredLaunchRoute::Legacy(_)));
        assert_eq!(serde_json::to_string(&legacy).unwrap(), bare);
        let changed = text.replacen("\"schema_version\":1", "\"schema_version\":2", 1);
        assert!(serde_json::from_str::<StoredLaunchRoute>(&changed).is_err());
        let mut mixed: serde_json::Value = serde_json::from_str(&text).unwrap();
        mixed["schema_version"] = 2.into();
        mixed["provider"] = "codex".into();
        mixed["model"] = "gpt-example".into();
        assert!(
            serde_json::from_value::<StoredLaunchRoute>(mixed).is_err(),
            "an invalid envelope cannot fall back to legacy decoding"
        );
        let mut malformed: serde_json::Value = serde_json::from_str(&text).unwrap();
        malformed["resolved_route"]["canonical_effort"] = "host-private-account".into();
        let error = serde_json::from_value::<StoredLaunchRoute>(malformed).unwrap_err();
        assert!(!error.to_string().contains("host-private-account"));
    }

    #[test]
    fn hash_binds_exact_bytes_domain_and_slot_order() {
        let slots = vec!["codex-primary".into(), "codex-secondary".into()];
        let host = b"schema_version: 1\n";
        let expected = format!(
            "{{\"host_sha256\":\"{}\",\"domain\":\"codex\",\"slots\":[\"codex-primary\",\"codex-secondary\"]}}",
            ContentHash::of(host)
        );
        assert_eq!(
            host_route_hash(host, "codex", &slots),
            ContentHash::of(expected.as_bytes())
        );
        assert_ne!(
            host_route_hash(host, "codex", &slots),
            host_route_hash(b"schema_version: 1", "codex", &slots)
        );
        let reversed = slots.iter().cloned().rev().collect::<Vec<_>>();
        assert_ne!(
            host_route_hash(host, "codex", &slots),
            host_route_hash(host, "codex", &reversed)
        );
        assert_ne!(
            host_route_hash(host, "codex", &slots),
            host_route_hash(host, "claude", &slots)
        );
    }

    #[test]
    fn legacy_readback_never_invents_policy_host_or_qualification_proof() {
        let rung = route().canonical_rung();
        for (harness, vendor) in [
            (None, Some("example")),
            (Some("paseo"), None),
            (Some("paseo"), Some("unknown")),
        ] {
            assert!(
                LegacyRouteView::from_frozen_evidence(&rung, harness, vendor)
                    .unwrap_err()
                    .to_string()
                    .contains("host_route_legacy_unproven")
            );
        }
        let legacy =
            LegacyRouteView::from_frozen_evidence(&rung, Some("paseo"), Some("example")).unwrap();
        assert_eq!(legacy.slot, rung.provider.0);
        assert_eq!(legacy.native_alias, rung.provider.0);
        assert_eq!(legacy.route_provider, "codex");
        assert_eq!(legacy.route_model_id, rung.model.0);
        assert!(legacy.policy_hash.is_none());
        assert!(legacy.host_route_hash.is_none());
        assert!(legacy.qualification.is_none());
        let unknown = ModelRung {
            provider: ProviderRef("unknown-host".into()),
            ..rung
        };
        assert!(
            LegacyRouteView::from_frozen_evidence(&unknown, Some("paseo"), Some("example"))
                .is_err()
        );
    }
}
