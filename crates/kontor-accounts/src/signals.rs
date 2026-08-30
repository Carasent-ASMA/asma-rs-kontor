//! The deployment document that supplies vendor exhaustion wording.
//!
//! [`crate::classify`] applies [`QuotaSignal`] values; it never holds any. This
//! module is where a deployment states them, so tracking a vendor's copy change
//! is an edit to one YAML file rather than a rebuild.
//!
//! # Why an absent document is not an error
//!
//! Reactive classification is an *addition* to the 300-second usage poll, never
//! a replacement for it. A realm with no document therefore keeps exactly the
//! behaviour it had before this file existed: the poll stays the sole source of
//! truth and no refusal is ever read as a quota state. Failing a daemon to
//! start because an optional document is missing would trade a working realm
//! for a stricter one.
//!
//! A present but *invalid* document is a different fact and is refused loudly:
//! it states an intent that cannot be honoured, and silently ignoring it would
//! leave an operator believing classification is armed when it is not.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::quota::QuotaSignal;

/// The optional signals document inside a Realm state root.
pub const QUOTA_SIGNALS_FILE: &str = "quota-signals.yml";

/// Why the signals document could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum QuotaSignalsError {
    /// The file exists but could not be read.
    #[error("the quota signals document could not be read")]
    Read {
        /// The path that failed.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The document is not valid YAML for this schema.
    #[error("the quota signals document is not a valid schema-version-1 YAML document")]
    Document,
    /// The document is structurally valid but unusable or contradictory.
    #[error("the quota signals document is invalid: {rule}")]
    Invalid {
        /// The stable rule, never a configured value.
        rule: &'static str,
    },
}

/// A versioned quota-signals document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaSignalsDocument {
    /// Schema generation of this document.
    pub schema_version: u32,
    /// The wordings to apply, in the order they are tried.
    ///
    /// Order is meaningful: [`crate::classify`] returns the *first* signal whose
    /// markers all appear, so a more specific vendor must precede a more general
    /// one that its text would also satisfy.
    pub signals: Vec<QuotaSignal>,
}

impl QuotaSignalsDocument {
    /// Validate this document's usability and internal consistency.
    ///
    /// # Errors
    /// Returns a stable rule without echoing any configured vendor sentence.
    pub fn validate(&self) -> Result<(), QuotaSignalsError> {
        if self.schema_version != 1 {
            return invalid("schema_version must be 1");
        }
        if self.signals.is_empty() {
            return invalid("a present document must declare at least one signal");
        }
        for signal in &self.signals {
            if signal.provider.trim().is_empty() {
                return invalid("every signal must name a provider");
            }
            if signal.markers.is_empty() {
                return invalid("every signal must declare at least one marker");
            }
            if signal.markers.iter().any(|marker| marker.trim().is_empty()) {
                return invalid("a marker must not be blank");
            }
            // A blank prefix would capture from offset zero and read the whole
            // message as an instant, so it is a configuration error rather than
            // an absent prefix.
            if signal
                .reset_prefix
                .as_ref()
                .is_some_and(|prefix| prefix.trim().is_empty())
            {
                return invalid("a stated reset_prefix must not be blank");
            }
            if signal
                .reset_zone
                .as_ref()
                .is_some_and(|zone| zone.trim().is_empty())
            {
                return invalid("a stated reset_zone must not be blank");
            }
        }
        Ok(())
    }
}

/// Parse and validate one signals document.
///
/// # Errors
/// Returns [`QuotaSignalsError`] when the text is not a valid document.
pub fn parse(document: &str) -> Result<QuotaSignalsDocument, QuotaSignalsError> {
    let parsed: QuotaSignalsDocument =
        serde_yaml_ng::from_str(document).map_err(|_| QuotaSignalsError::Document)?;
    parsed.validate()?;
    Ok(parsed)
}

/// Read this Realm's optional quota-signals document.
///
/// `Ok(None)` means the realm configured none, which leaves reactive
/// classification inert and the usage poll authoritative.
///
/// # Errors
/// Returns [`QuotaSignalsError`] when a present file cannot be read or validated.
pub fn read(state_root: &Path) -> Result<Option<QuotaSignalsDocument>, QuotaSignalsError> {
    let path = state_root.join(QUOTA_SIGNALS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(document) => parse(&document).map(Some),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(QuotaSignalsError::Read { path, source }),
    }
}

fn invalid<T>(rule: &'static str) -> Result<T, QuotaSignalsError> {
    Err(QuotaSignalsError::Invalid { rule })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quota::{QuotaBasis, classify};
    use kontor_core::spec::ProviderQuotaKind;

    const EXAMPLE: &str = include_str!("../../../config/examples/quota-signals.yml");

    /// The text Codex actually produced on 2026-08-21, from the report Igor filed.
    const CODEX_LIMIT: &str = "[System Error] You've hit your usage limit. Visit \
         https://chatgpt.com/codex/settings/usage to purchase more credits or try \
         again at Aug 23rd, 2026 9:35 AM.";

    #[test]
    fn the_shipped_example_is_valid() {
        let document = parse(EXAMPLE).expect("the shipped example is valid");
        assert_eq!(document.schema_version, 1);
        assert!(
            document
                .signals
                .iter()
                .any(|signal| signal.provider == "claude")
        );
        assert!(
            document
                .signals
                .iter()
                .any(|signal| signal.provider == "codex")
        );
    }

    #[test]
    fn the_shipped_example_still_classifies_the_recorded_codex_refusal() {
        let document = parse(EXAMPLE).expect("valid document");
        let observed = classify(CODEX_LIMIT, &document.signals).expect("a quota refusal");
        assert_eq!(observed.provider, "codex");
        assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
    }

    /// The example lists `claude` before `codex` precisely so a Claude refusal
    /// carrying the words "usage limit" is not attributed to Codex, whose own
    /// marker those words alone satisfy.
    #[test]
    fn a_claude_refusal_is_not_attributed_to_codex() {
        let document = parse(EXAMPLE).expect("valid document");
        let observed = classify(
            "Claude usage limit reached. Your limit will reset at 9:35 AM.",
            &document.signals,
        )
        .expect("a quota refusal");
        assert_eq!(observed.provider, "claude");
    }

    #[test]
    fn an_ordinary_error_is_not_a_quota_refusal() {
        let document = parse(EXAMPLE).expect("valid document");
        assert!(classify("connection reset by peer", &document.signals).is_none());
    }

    #[test]
    fn an_absent_document_leaves_classification_inert() {
        let root = tempfile::tempdir().expect("temporary state root");
        assert_eq!(read(root.path()).expect("absence is valid"), None);
    }

    #[test]
    fn an_unparsable_document_is_refused_rather_than_ignored() {
        assert!(matches!(
            parse("schema_version: 1\nsignals: [{"),
            Err(QuotaSignalsError::Document)
        ));
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let document = "schema_version: 1\nsignals:\n  - provider: codex\n    basis: plan_allowance\n    markers: ['usage limit']\n    reset_after: nonsense\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Document)
        ));
    }

    #[test]
    fn a_future_schema_version_is_refused() {
        let document = "schema_version: 2\nsignals:\n  - provider: codex\n    basis: plan_allowance\n    markers: ['usage limit']\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "schema_version must be 1"
            })
        ));
    }

    #[test]
    fn a_signal_without_markers_is_refused() {
        let document =
            "schema_version: 1\nsignals:\n  - provider: codex\n    basis: plan_allowance\n    markers: []\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "every signal must declare at least one marker"
            })
        ));
    }

    #[test]
    fn a_blank_reset_prefix_is_refused_rather_than_capturing_from_zero() {
        let document = "schema_version: 1\nsignals:\n  - provider: codex\n    basis: plan_allowance\n    markers: ['usage limit']\n    reset_prefix: '   '\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "a stated reset_prefix must not be blank"
            })
        ));
    }

    #[test]
    fn an_empty_signal_list_is_refused() {
        assert!(matches!(
            parse("schema_version: 1\nsignals: []\n"),
            Err(QuotaSignalsError::Invalid {
                rule: "a present document must declare at least one signal"
            })
        ));
    }

    #[test]
    fn a_credit_basis_round_trips_through_the_document() {
        let document = "schema_version: 1\nsignals:\n  - provider: openrouter\n    basis: credit_balance\n    markers: ['insufficient', 'credits']\n";
        let parsed = parse(document).expect("valid document");
        assert_eq!(parsed.signals[0].basis, QuotaBasis::CreditBalance);
    }
}
