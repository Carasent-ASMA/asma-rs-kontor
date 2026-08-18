//! Native capacity collection.
//!
//! # What replaced what
//!
//! Kontor used to learn whether a coding account was usable by running
//! `asma fleet preflight` and reading the JSON back. That made a Realm's
//! admission decisions as available as another program was, put the cooldown
//! clock in a store Kontor did not own, and meant a machine without `asma`
//! could not schedule at all. The collector here reads the same fact from the
//! two things Kontor already holds: its own account configuration, and the
//! runtime family that account authenticates against.
//!
//! # A reading is evidence; availability is a conclusion
//!
//! [`CapacityReading`] is what a collector saw. [`derive`] is what the Realm
//! concluded from it. They are separate values because they are persisted
//! separately: a later reader must be able to see what the probe actually
//! reported even after an operator has overridden the conclusion, and an
//! override that could edit the reading would destroy the only record of the
//! disagreement.
//!
//! # Redacted by construction
//!
//! Every field of a reading is a closed token, a boolean or a runtime kind.
//! There is no free-text field a provider message, a path, an endpoint or a
//! credential could arrive in — not because a redactor strips them, but because
//! there is nowhere to put one. [`ProbeRefusal`] exists for exactly this reason:
//! a [`RuntimeError`]'s `Display` may name a workspace path, so the variant is
//! mapped to a token and the message is dropped at the boundary.

use kontor_core::id::{RuntimeKindKey, SchemaVersion, Timestamp};
use kontor_runtime::adapter::RuntimeError;
use kontor_runtime::capability::RuntimeCapabilities;
use serde::{Deserialize, Serialize};

use crate::launch::AccountAvailability;

/// How long an account cools after the runtime reported a limit.
///
/// This is the cooldown mechanic that used to live in `asma fleet`. A fixed
/// span rather than a provider-supplied reset instant: nothing in the runtime
/// contract carries one, and inventing a shorter cooldown from an optimistic
/// guess is how a Realm walks straight back into the limit it just hit. Five
/// minutes is the same order as the throttling windows the providers apply.
///
/// ponytail: one constant, not a per-provider table. A table needs a provider
/// taxonomy Kontor does not have; when one exists, this becomes a lookup in the
/// account's non-secret routing document.
pub const COOLDOWN_SECONDS: i64 = 300;

/// Why a runtime family could not answer a capacity probe.
///
/// A closed token per [`RuntimeError`] variant, carrying nothing from the error
/// itself. Kontor branches on none of them — they exist so an operator reading
/// a stored observation can tell "the adapter is not there" from "the adapter
/// refused me".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProbeRefusal {
    /// The family declared it cannot do what was asked.
    UnsupportedCapability,
    /// The family's trust grade is too low for Kontor's own authority.
    InsufficientTrust,
    /// The family cannot prove which account a run executes as.
    AccountEnvironmentUnavailable,
    /// A declared bound was exceeded.
    LimitExceeded,
    /// The family could not be reached or answered unusably.
    Unreachable,
}

impl ProbeRefusal {
    /// The token for one runtime refusal, dropping its message.
    #[must_use]
    pub const fn of(error: &RuntimeError) -> Self {
        match error {
            RuntimeError::UnsupportedCapability { .. } => Self::UnsupportedCapability,
            RuntimeError::InsufficientTrust { .. } => Self::InsufficientTrust,
            RuntimeError::AccountEnvironmentUnavailable => Self::AccountEnvironmentUnavailable,
            RuntimeError::LimitExceeded { .. } => Self::LimitExceeded,
            _ => Self::Unreachable,
        }
    }

    /// Whether this refusal is the provider pushing back rather than absent.
    ///
    /// Only a limit is pressure. An adapter that is missing, untrusted or
    /// account-blind is unusable, which is a different fact: narrowing the
    /// admission window because a runtime was never configured would punish
    /// every other account in the Realm for it.
    #[must_use]
    pub const fn is_pressure(self) -> bool {
        matches!(self, Self::LimitExceeded)
    }
}

/// What a runtime probe reported about one account's family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ProbeOutcome {
    /// The family answered and can prove which account a run executes as.
    AccountEnvironmentSupported,
    /// The family answered but cannot prove it.
    AccountEnvironmentUnsupported,
    /// The family refused.
    Refused {
        /// Why, as a closed token.
        refusal: ProbeRefusal,
    },
}

impl ProbeOutcome {
    /// The outcome of one live capability discovery.
    #[must_use]
    pub fn of(discovery: Result<&RuntimeCapabilities, &RuntimeError>) -> Self {
        match discovery {
            Ok(capabilities) if capabilities.account_env => Self::AccountEnvironmentSupported,
            Ok(_) => Self::AccountEnvironmentUnsupported,
            Err(error) => Self::Refused {
                refusal: ProbeRefusal::of(error),
            },
        }
    }
}

/// One collector reading — the raw evidence, exactly as observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapacityReading {
    /// Wire generation, so a stored reading stays parseable.
    pub schema_version: SchemaVersion,
    /// Whether the Realm's own configuration permits launches on this account.
    pub profile_enabled: bool,
    /// The runtime family the account authenticates against.
    pub runtime_kind: RuntimeKindKey,
    /// What probing that family reported.
    pub probe: ProbeOutcome,
}

/// What the Realm concluded from one reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivedAvailability {
    /// The typed availability the launch path judges a pin against.
    pub availability: AccountAvailability,
    /// Whether the account may be used now.
    pub available: bool,
    /// Whether the reading indicated the provider pushing back.
    pub pressure: bool,
}

/// Derive availability and pressure from one reading.
///
/// Disabled is deliberately not pressure. An operator turning a profile off is
/// the Realm's own decision; treating it as the provider throttling would
/// narrow the admission window for work that has nothing to do with it.
#[must_use]
pub fn derive(reading: &CapacityReading, observed_at: Timestamp) -> DerivedAvailability {
    if !reading.profile_enabled {
        return DerivedAvailability {
            availability: AccountAvailability::Unknown,
            available: false,
            pressure: false,
        };
    }
    match reading.probe {
        ProbeOutcome::AccountEnvironmentSupported => DerivedAvailability {
            availability: AccountAvailability::Available,
            available: true,
            pressure: false,
        },
        ProbeOutcome::AccountEnvironmentUnsupported => DerivedAvailability {
            availability: AccountAvailability::Unknown,
            available: false,
            pressure: false,
        },
        ProbeOutcome::Refused { refusal } if refusal.is_pressure() => DerivedAvailability {
            availability: AccountAvailability::Cooling {
                blocked_until: cools_until(observed_at),
            },
            available: false,
            pressure: true,
        },
        ProbeOutcome::Refused { .. } => DerivedAvailability {
            availability: AccountAvailability::Unknown,
            available: false,
            pressure: false,
        },
    }
}

/// When an account observed under a limit at `observed_at` becomes usable.
#[must_use]
pub fn cools_until(observed_at: Timestamp) -> Timestamp {
    // A cooldown that could not be represented is one the clock has run past
    // the end of; the account stays exactly as blocked as it already was.
    Timestamp::from_second(observed_at.as_second().saturating_add(COOLDOWN_SECONDS))
        .unwrap_or(observed_at)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use kontor_runtime::capability::{RuntimeLimits, TrustGrade};

    use super::*;

    fn kind() -> RuntimeKindKey {
        RuntimeKindKey::parse("paseo").expect("a valid runtime kind")
    }

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("a representable instant")
    }

    fn capabilities(account_env: bool) -> RuntimeCapabilities {
        RuntimeCapabilities {
            trust_grade: TrustGrade::A,
            supported: BTreeSet::new(),
            account_env,
            limits: RuntimeLimits {
                max_message_bytes: 1,
                max_history_page: 1,
                max_concurrent_sessions: 1,
                context_window: kontor_core::spec::ContextWindowBounds::default(),
            },
        }
    }

    fn reading(profile_enabled: bool, probe: ProbeOutcome) -> CapacityReading {
        CapacityReading {
            schema_version: kontor_core::id::SCHEMA_VERSION,
            profile_enabled,
            runtime_kind: kind(),
            probe,
        }
    }

    #[test]
    fn a_family_that_can_prove_the_account_is_clean_and_available() {
        let derived = derive(
            &reading(true, ProbeOutcome::of(Ok(&capabilities(true)))),
            at(1_000),
        );
        assert_eq!(derived.availability, AccountAvailability::Available);
        assert!(derived.available);
        assert!(!derived.pressure);
    }

    #[test]
    fn a_declared_limit_cools_the_account_and_is_the_only_pressure() {
        let error = RuntimeError::LimitExceeded {
            subject: "prompt bytes",
            limit: 1,
        };
        let derived = derive(&reading(true, ProbeOutcome::of(Err(&error))), at(1_000));
        assert!(derived.pressure);
        assert!(!derived.available);
        assert_eq!(
            derived.availability,
            AccountAvailability::Cooling {
                blocked_until: at(1_000 + COOLDOWN_SECONDS)
            }
        );
    }

    #[test]
    fn an_absent_or_account_blind_runtime_is_unusable_but_not_pressure() {
        for probe in [
            ProbeOutcome::of(Ok(&capabilities(false))),
            ProbeOutcome::of(Err(&RuntimeError::AccountEnvironmentUnavailable)),
            ProbeOutcome::of(Err(&RuntimeError::CorrelationFailed)),
        ] {
            let derived = derive(&reading(true, probe), at(1_000));
            assert!(!derived.available, "{probe:?} must not read as available");
            assert!(
                !derived.pressure,
                "{probe:?} is a missing runtime, not a provider pushing back"
            );
            assert_eq!(derived.availability, AccountAvailability::Unknown);
        }
    }

    #[test]
    fn a_disabled_profile_is_unavailable_without_narrowing_anything() {
        let derived = derive(
            &reading(false, ProbeOutcome::of(Ok(&capabilities(true)))),
            at(1_000),
        );
        assert!(!derived.available);
        assert!(
            !derived.pressure,
            "the Realm's own configuration is not the provider throttling"
        );
    }

    #[test]
    fn a_reading_round_trips_and_carries_no_free_text() {
        let error = RuntimeError::LimitExceeded {
            subject: "prompt bytes",
            limit: 1,
        };
        let stored = reading(true, ProbeOutcome::of(Err(&error)));
        let json = serde_json::to_value(&stored).expect("a serializable reading");
        assert_eq!(
            serde_json::from_value::<CapacityReading>(json.clone()).expect("a parseable reading"),
            stored
        );
        // The limit's own `subject` must not have travelled with the token.
        assert!(
            !json.to_string().contains("prompt bytes"),
            "a reading carries tokens, never the runtime's message: {json}"
        );
    }
}
