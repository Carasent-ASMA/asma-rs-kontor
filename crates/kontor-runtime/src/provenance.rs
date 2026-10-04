//! The fleet policy's authority for one launch, at the runtime boundary
//! (ASMA-8280 G-3).
//!
//! One optional value travels on every hosted-leadership, delivery and
//! consultation launch: which activated policy (and, for an aligned
//! activation, which orchestration bundle) chose the route, under which
//! binding and chain, at which position, for which vendor, and under which
//! stated eligibility. The daemon maps it from the shared resolver's selection
//! and the decision it recorded; a runtime never invents one.
//!
//! What a launch *requested* and what the runtime *observed* are two values.
//! A runtime writes the requested provenance to a native surface where one can
//! carry it, reads it back from that surface, and reports what it read. Where
//! no native surface can carry it, the runtime says so with the native id and
//! the surface it would have used. It never answers a request with the request.

use std::collections::BTreeSet;

use kontor_core::id::{ContentHash, ExternalId};
use serde::{Deserialize, Serialize};

/// The eligibility a fleet choice was made under, as a launch carries it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchEligibility {
    /// Account aliases that could not take the seat when it was chosen.
    pub unavailable_accounts: BTreeSet<String>,
    /// Vendors the seat had to avoid.
    pub excluded_vendors: BTreeSet<String>,
}

/// Which fleet policy authorised one launch, as Kontor requests it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetLaunchProvenance {
    /// SHA-256 of exactly the activated policy bytes.
    pub policy_hash: ContentHash,
    /// The orchestration bundle an aligned (schema_version 2) activation names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_bundle_hash: Option<ContentHash>,
    /// The canonical binding key the seat resolved.
    pub binding_key: String,
    /// The chain that key is bound to.
    pub chain: String,
    /// The chain step, counted from one.
    pub step: u16,
    /// The route's position inside its step, counted from one.
    pub sub_step: u16,
    /// The model's maker, as the policy names it.
    pub vendor: String,
    /// The eligibility the policy chose the route under. `None` when the
    /// route was the caller's own, admitted by the policy rather than chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eligibility: Option<LaunchEligibility>,
}

/// What a runtime natively observed of one launch's fleet provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FleetProvenanceObservation {
    /// The launch requested no fleet provenance, so there was nothing to
    /// write or read back.
    NotRequested,
    /// Read back from the native surface after the launch.
    Observed {
        /// Where it was read, such as `paseo.agent.labels`.
        surface: String,
        /// Exactly what the native surface holds.
        provenance: FleetLaunchProvenance,
    },
    /// The native surface this launch used cannot carry the provenance, so
    /// nothing was written and nothing can be read back.
    Unsupported {
        /// The surface that cannot carry it.
        surface: String,
        /// The native session the launch produced.
        native_id: ExternalId,
    },
}

impl FleetProvenanceObservation {
    /// The answer of a runtime whose native surface carries no provenance:
    /// unsupported when some was requested, and not requested otherwise.
    #[must_use]
    pub fn without_surface(
        requested: Option<&FleetLaunchProvenance>,
        surface: &str,
        native_id: &ExternalId,
    ) -> Self {
        if requested.is_some() {
            Self::Unsupported {
                surface: surface.to_owned(),
                native_id: native_id.clone(),
            }
        } else {
            Self::NotRequested
        }
    }

    /// Whether this observation proves `requested`: it was read back from a
    /// native surface and is exactly what was requested. An unsupported
    /// surface proves nothing, and nothing requested needs no proof.
    #[must_use]
    pub fn proves(&self, requested: Option<&FleetLaunchProvenance>) -> bool {
        match (self, requested) {
            (Self::NotRequested, None) => true,
            (Self::Observed { provenance, .. }, Some(requested)) => provenance == requested,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provenance() -> FleetLaunchProvenance {
        FleetLaunchProvenance {
            policy_hash: ContentHash::of(b"policy"),
            source_bundle_hash: Some(ContentHash::of(b"bundle")),
            binding_key: "team/t/s".to_owned(),
            chain: "lead".to_owned(),
            step: 1,
            sub_step: 2,
            vendor: "openai".to_owned(),
            eligibility: Some(LaunchEligibility {
                unavailable_accounts: BTreeSet::from(["codex-work".to_owned()]),
                excluded_vendors: BTreeSet::new(),
            }),
        }
    }

    #[test]
    fn only_a_read_back_equal_to_the_request_proves_it() {
        let requested = provenance();
        let native = ExternalId::parse("agent-1").expect("an id");
        let observed = FleetProvenanceObservation::Observed {
            surface: "paseo.agent.labels".to_owned(),
            provenance: requested.clone(),
        };
        assert!(observed.proves(Some(&requested)));
        let mut drifted = requested.clone();
        drifted.sub_step = 1;
        assert!(!observed.proves(Some(&drifted)));
        assert!(!observed.proves(None));

        let unsupported = FleetProvenanceObservation::without_surface(
            Some(&requested),
            "codex.app_server",
            &native,
        );
        assert_eq!(
            unsupported,
            FleetProvenanceObservation::Unsupported {
                surface: "codex.app_server".to_owned(),
                native_id: native.clone(),
            }
        );
        assert!(!unsupported.proves(Some(&requested)));
        let quiet = FleetProvenanceObservation::without_surface(None, "codex.app_server", &native);
        assert_eq!(quiet, FleetProvenanceObservation::NotRequested);
        assert!(quiet.proves(None));
    }

    #[test]
    fn the_record_shape_is_stable() {
        let json = serde_json::to_value(FleetProvenanceObservation::Unsupported {
            surface: "fake.runtime".to_owned(),
            native_id: ExternalId::parse("native-1").expect("an id"),
        })
        .expect("JSON");
        assert_eq!(
            json,
            serde_json::json!({"status": "unsupported", "surface": "fake.runtime", "native_id": "native-1"})
        );
        let mut caller = provenance();
        caller.eligibility = None;
        caller.source_bundle_hash = None;
        let json = serde_json::to_value(&caller).expect("JSON");
        assert!(json.get("eligibility").is_none() && json.get("source_bundle_hash").is_none());
        let back: FleetLaunchProvenance = serde_json::from_value(json).expect("round trip");
        assert_eq!(back, caller);
    }
}
