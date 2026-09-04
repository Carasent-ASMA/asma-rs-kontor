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

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::quota::{QuotaBasis, QuotaSignal};

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
        // Identity first: a document that cannot say *which* signal fired
        // cannot support a provenance record that names one.
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for signal in &self.signals {
            // Delegated rather than re-implemented. `ExternalName::parse` also
            // applies the core sensitive-material rule, and a hand-rolled
            // non-empty/trimmed/bounded/control check would silently drop it.
            if kontor_core::id::ExternalName::parse(&signal.id).is_err() {
                return invalid("every signal id must be a valid external name");
            }
            // One active revision per logical signal. Two entries sharing an id
            // would make `(id, version)` ambiguous, and provenance keyed on it
            // could not say which definition authorized a retirement.
            if !seen.insert(signal.id.as_str()) {
                return invalid("signal ids must be unique within one document");
            }
            // A definition that cannot be digested cannot be cited later.
            if signal.definition_hash().is_err() {
                return invalid("every signal definition must be canonicalizable");
            }
            if signal.provider.trim().is_empty() {
                return invalid("every signal must name a provider");
            }
            if signal.markers.is_empty() {
                return invalid("every signal must declare at least one marker");
            }
            if signal.markers.iter().any(|marker| marker.trim().is_empty()) {
                return invalid("a marker must not be blank");
            }
            // The matcher scans ASCII-case-insensitively over the original
            // message so its byte offsets stay valid char boundaries. That is
            // exact only while the needles are ASCII, so the vendor grammar is
            // enforced here rather than assumed there.
            if signal.markers.iter().any(|marker| !marker.is_ascii()) {
                return invalid("a marker must be ASCII vendor wording");
            }
            if signal
                .reset_prefix
                .as_ref()
                .is_some_and(|prefix| !prefix.is_ascii())
            {
                return invalid("a reset_prefix must be ASCII vendor wording");
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
            // A zone that does not resolve is not a near-miss: every reset for
            // this vendor silently degrades to `Unknown`, which reads as "the
            // vendor stated no instant" when in fact the deployment named a
            // zone that does not exist. Refuse it while an operator is looking
            // at the file.
            if let Some(zone) = signal.reset_zone.as_deref()
                && jiff::tz::TimeZone::get(zone).is_err()
            {
                return invalid("a stated reset_zone must be a known IANA zone");
            }
            // Contradictory intent is refused rather than silently ignored. A
            // prepaid balance has no reset instant to parse, so reset fields on
            // one are configuration an operator believes is doing something.
            if signal.basis == QuotaBasis::CreditBalance
                && (signal.reset_prefix.is_some() || signal.reset_zone.is_some())
            {
                return invalid("a credit balance declares no reset_prefix or reset_zone");
            }
            // A zone qualifies a parsed instant, and nothing is parsed without
            // a prefix, so a zone alone is intent that never applies.
            if signal.reset_zone.is_some() && signal.reset_prefix.is_none() {
                return invalid("a reset_zone requires a reset_prefix");
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
    use crate::quota::classify;
    use kontor_core::id::parse_utc_timestamp;
    use kontor_core::spec::ProviderQuotaKind;

    const EXAMPLE: &str = include_str!("../../../config/examples/quota-signals.yml");

    /// The message Claude actually produced on 2026-08-30, verbatim.
    const CLAUDE_LIMIT: &str = "You've hit your individual spend limit · ask your admin to raise \
         it at claude.ai/settings/usage?from=cc_cli_limit_message · your session limit resets \
         10:40pm (Europe/Chisinau)";

    /// The text Codex actually produced on 2026-08-21, from the report Igor filed.
    const CODEX_LIMIT: &str = "[System Error] You've hit your usage limit. Visit \
         https://chatgpt.com/codex/settings/usage to purchase more credits or try \
         again at Aug 23rd, 2026 9:35 AM.";

    #[test]
    fn the_shipped_example_names_exact_catalog_aliases_not_vendor_families() {
        let document = parse(EXAMPLE).expect("the shipped example is valid");
        assert_eq!(document.schema_version, 1);
        let providers: Vec<&str> = document
            .signals
            .iter()
            .map(|signal| signal.provider.as_str())
            .collect();
        for alias in [
            "claude-work",
            "claude-personal",
            "codex-work",
            "codex-personal",
            "opencode",
        ] {
            assert!(providers.contains(&alias), "missing {alias}: {providers:?}");
        }
        assert!(
            !providers.contains(&"codex") && !providers.contains(&"claude"),
            "a bare vendor family can never be selected by an account: {providers:?}",
        );
        // Unverified vendor copy must not ship as active authority: a false
        // positive archives live work, a false negative costs nothing.
        // Captured 2026-08-30, so the Claude aliases are now active authority.
        assert_eq!(
            providers.iter().filter(|p| p.starts_with("claude")).count(),
            2,
            "one entry per Claude login: {providers:?}",
        );
    }

    #[test]
    fn every_active_alias_classifies_its_own_vendor_wording() {
        let document = parse(EXAMPLE).expect("valid document");
        for (alias, text) in [
            ("claude-work", CLAUDE_LIMIT),
            ("claude-personal", CLAUDE_LIMIT),
            ("codex-work", CODEX_LIMIT),
            ("codex-personal", CODEX_LIMIT),
            ("opencode", "insufficient credits remaining"),
        ] {
            let eligible: Vec<QuotaSignal> = document
                .signals
                .iter()
                .filter(|signal| signal.provider == alias)
                .cloned()
                .collect();
            assert!(!eligible.is_empty(), "{alias} is not declared");
            let observed = classify(
                text,
                &eligible,
                parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant"),
            )
            .unwrap_or_else(|| panic!("{alias} classifies nothing"));
            assert_eq!(observed.provider, alias);
        }
    }

    /// The false positive that motivated the full fingerprint: an assistant
    /// merely *discussing* usage limits is not a refusal, and misreading one
    /// archives a seat that was working.
    #[test]
    fn an_assistant_discussing_usage_limits_is_not_a_refusal() {
        let document = parse(EXAMPLE).expect("valid document");
        let codex: Vec<QuotaSignal> = document
            .signals
            .iter()
            .filter(|signal| signal.provider == "codex-work")
            .cloned()
            .collect();
        for innocuous in [
            "I'll add handling for the provider usage limit case and try again at the next step.",
            "The usage limit error should be retried; see the settings page for details.",
            "[System Error] the tool call failed; try again at your convenience.",
        ] {
            assert!(
                classify(
                    innocuous,
                    &codex,
                    parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant")
                )
                .is_none(),
                "ordinary prose must never retire a seat: {innocuous:?}",
            );
        }
    }

    #[test]
    fn the_shipped_example_still_classifies_the_recorded_codex_refusal() {
        let document = parse(EXAMPLE).expect("valid document");
        let eligible: Vec<QuotaSignal> = document
            .signals
            .iter()
            .filter(|signal| signal.provider == "codex-work")
            .cloned()
            .collect();
        let observed = classify(
            CODEX_LIMIT,
            &eligible,
            parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant"),
        )
        .expect("a quota refusal");
        assert_eq!(observed.provider, "codex-work");
        assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
    }

    /// The historical fingerprint keeps the zone of the message it was captured
    /// from. A host that later sits elsewhere must not move the instant.
    #[test]
    fn the_recorded_codex_reset_is_read_in_its_captured_zone() {
        let document = parse(EXAMPLE).expect("valid document");
        let eligible: Vec<QuotaSignal> = document
            .signals
            .iter()
            .filter(|signal| signal.provider == "codex-work")
            .cloned()
            .collect();
        assert_eq!(
            eligible[0].reset_zone.as_deref(),
            Some("Europe/Oslo"),
            "the captured 2026-08-21/23 message is Oslo-authored provenance",
        );
        let observed = classify(
            CODEX_LIMIT,
            &eligible,
            parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant"),
        )
        .expect("a quota refusal");
        // 09:35 in Oslo during August is CEST, two hours ahead of UTC.
        assert_eq!(
            observed.resets_at,
            Some(
                kontor_core::id::parse_utc_timestamp("2026-08-23T07:35:00Z")
                    .expect("a canonical instant")
            ),
        );
    }

    #[test]
    fn a_non_ascii_marker_is_refused_because_the_matcher_is_ascii_exact() {
        let document = "schema_version: 1\nsignals:\n  - id: t-codex-work\n    version: 1\n    provider: codex-work\n    basis: plan_allowance\n    markers: ['brukergrense']\n";
        assert!(parse(document).is_ok(), "plain ASCII is fine");
        let non_ascii = "schema_version: 1\nsignals:\n  - id: t-codex-work\n    version: 1\n    provider: codex-work\n    basis: plan_allowance\n    markers: ['kvote overskredet \u{e5}']\n";
        assert!(matches!(
            parse(non_ascii),
            Err(QuotaSignalsError::Invalid {
                rule: "a marker must be ASCII vendor wording"
            })
        ));
    }

    #[test]
    fn an_unknown_iana_zone_is_refused_rather_than_degrading_every_reset() {
        let document = "schema_version: 1\nsignals:\n  - id: t-codex-work\n    version: 1\n    provider: codex-work\n    basis: plan_allowance\n    markers: ['usage limit']\n    reset_prefix: 'try again at '\n    reset_zone: 'Europe/Nowhere'\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "a stated reset_zone must be a known IANA zone"
            })
        ));
    }

    #[test]
    fn a_credit_balance_declaring_reset_fields_is_refused_as_contradictory() {
        let document = "schema_version: 1\nsignals:\n  - id: t-opencode\n    version: 1\n    provider: opencode\n    basis: credit_balance\n    markers: ['insufficient']\n    reset_prefix: 'try again at '\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "a credit balance declares no reset_prefix or reset_zone"
            })
        ));
    }

    #[test]
    fn a_zone_without_a_prefix_is_refused_as_intent_that_never_applies() {
        let document = "schema_version: 1\nsignals:\n  - id: t-codex-work\n    version: 1\n    provider: codex-work\n    basis: plan_allowance\n    markers: ['usage limit']\n    reset_zone: Europe/Oslo\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "a reset_zone requires a reset_prefix"
            })
        ));
    }

    #[test]
    fn an_ordinary_error_is_not_a_quota_refusal() {
        let document = parse(EXAMPLE).expect("valid document");
        assert!(
            classify(
                "connection reset by peer",
                &document.signals,
                parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant")
            )
            .is_none()
        );
    }

    /// Absent, unreadable and invalid are **three** outcomes, not two. Only the
    /// first is inert; collapsing the others into it would leave an operator
    /// believing classification is armed when it is not.
    #[test]
    fn an_absent_document_leaves_classification_inert() {
        let root = tempfile::tempdir().expect("temporary state root");
        assert_eq!(read(root.path()).expect("absence is valid"), None);
    }

    #[test]
    fn a_present_but_unreadable_document_fails_loudly_rather_than_going_inert() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("temporary state root");
        let path = root.path().join(QUOTA_SIGNALS_FILE);
        std::fs::write(&path, EXAMPLE).expect("the document is written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
            .expect("permissions are set");
        let outcome = read(root.path());
        // Running as root defeats the permission bit; skip rather than assert a
        // falsehood about the environment.
        if let Ok(readable) = &outcome {
            assert!(readable.is_some(), "a readable document still parses");
            return;
        }
        assert!(
            matches!(outcome, Err(QuotaSignalsError::Read { .. })),
            "an unreadable document is a typed Read failure, never inert",
        );
    }

    #[test]
    fn a_present_but_invalid_document_fails_loudly_rather_than_going_inert() {
        let root = tempfile::tempdir().expect("temporary state root");
        std::fs::write(
            root.path().join(QUOTA_SIGNALS_FILE),
            "schema_version: 1\nsignals:\n  - id: t-codex-work\n    version: 1\n    provider: codex-work\n    basis: plan_allowance\n    markers: []\n",
        )
        .expect("the document is written");
        assert!(
            matches!(
                read(root.path()),
                Err(QuotaSignalsError::Invalid {
                    rule: "every signal must declare at least one marker"
                })
            ),
            "an invalid document is refused, never treated as absent",
        );
    }

    #[test]
    fn a_present_but_unparsable_document_fails_loudly_rather_than_going_inert() {
        let root = tempfile::tempdir().expect("temporary state root");
        std::fs::write(
            root.path().join(QUOTA_SIGNALS_FILE),
            "schema_version: 1\nsignals: [{",
        )
        .expect("the document is written");
        assert!(
            matches!(read(root.path()), Err(QuotaSignalsError::Document)),
            "an unparsable document is refused, never treated as absent",
        );
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
        let document = "schema_version: 1\nsignals:\n  - id: t-codex\n    version: 1\n    provider: codex\n    basis: plan_allowance\n    markers: ['usage limit']\n    reset_after: nonsense\n";
        assert!(matches!(parse(document), Err(QuotaSignalsError::Document)));
    }

    #[test]
    fn a_future_schema_version_is_refused() {
        let document = "schema_version: 2\nsignals:\n  - id: t-codex\n    version: 1\n    provider: codex\n    basis: plan_allowance\n    markers: ['usage limit']\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "schema_version must be 1"
            })
        ));
    }

    #[test]
    fn a_signal_without_markers_is_refused() {
        let document = "schema_version: 1\nsignals:\n  - id: t-codex\n    version: 1\n    provider: codex\n    basis: plan_allowance\n    markers: []\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "every signal must declare at least one marker"
            })
        ));
    }

    #[test]
    fn a_blank_reset_prefix_is_refused_rather_than_capturing_from_zero() {
        let document = "schema_version: 1\nsignals:\n  - id: t-codex\n    version: 1\n    provider: codex\n    basis: plan_allowance\n    markers: ['usage limit']\n    reset_prefix: '   '\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "a stated reset_prefix must not be blank"
            })
        ));
    }

    fn signal_yaml(id: &str, version: u32, marker: &str) -> String {
        format!(
            "schema_version: 1\nsignals:\n  - id: {id}\n    version: {version}\n    \
             provider: codex-work\n    basis: plan_allowance\n    markers: ['{marker}']\n"
        )
    }

    #[test]
    fn two_signals_may_not_share_one_id() {
        let document = "schema_version: 1\nsignals:\n  - id: same\n    version: 1\n    provider: codex-work\n    \
             basis: plan_allowance\n    markers: ['usage limit']\n  - id: same\n    version: 2\n    \
             provider: codex-personal\n    basis: plan_allowance\n    markers: ['usage limit']\n";
        assert!(matches!(
            parse(document),
            Err(QuotaSignalsError::Invalid {
                rule: "signal ids must be unique within one document"
            })
        ));
    }

    /// `SpecVersion` refuses zero at the serde boundary, so the document never
    /// reaches validation at all.
    #[test]
    fn a_zero_version_is_refused_while_parsing() {
        assert!(matches!(
            parse(&signal_yaml("codex-usage-limit", 0, "usage limit")),
            Err(QuotaSignalsError::Document)
        ));
        assert!(parse(&signal_yaml("codex-usage-limit", 1, "usage limit")).is_ok());
    }

    #[test]
    fn an_id_that_is_not_a_valid_external_name_is_refused() {
        for bad in ["", "  untrimmed", "has\u{7}control"] {
            let document = format!(
                "schema_version: 1\nsignals:\n  - id: \"{bad}\"\n    version: 1\n    \
                 provider: codex-work\n    basis: plan_allowance\n    markers: ['usage limit']\n"
            );
            assert!(
                matches!(
                    parse(&document),
                    Err(QuotaSignalsError::Invalid {
                        rule: "every signal id must be a valid external name"
                    }) | Err(QuotaSignalsError::Document)
                ),
                "{bad:?} must be refused",
            );
        }
    }

    /// Identity does not pin content. Changing the definition under an unchanged
    /// `id` and `version` must be *visible* as a different digest, so immutable
    /// history can refuse it rather than silently accept a substitution.
    #[test]
    fn the_same_id_and_version_with_changed_content_hashes_differently() {
        let first = parse(&signal_yaml("codex-usage-limit", 1, "usage limit"))
            .expect("valid")
            .signals[0]
            .definition_hash()
            .expect("a digest");
        for changed in [
            signal_yaml("codex-usage-limit", 1, "hit your usage limit"),
            "schema_version: 1\nsignals:\n  - id: codex-usage-limit\n    version: 1\n    \
             provider: codex-personal\n    basis: plan_allowance\n    markers: ['usage limit']\n"
                .to_owned(),
            "schema_version: 1\nsignals:\n  - id: codex-usage-limit\n    version: 1\n    \
             provider: codex-work\n    basis: plan_allowance\n    markers: ['usage limit']\n    \
             reset_prefix: 'try again at '\n"
                .to_owned(),
        ] {
            let other = parse(&changed).expect("valid").signals[0]
                .definition_hash()
                .expect("a digest");
            assert_ne!(
                first, other,
                "a changed definition must not reuse the digest of another",
            );
        }
        // And the identical definition is stable.
        let again = parse(&signal_yaml("codex-usage-limit", 1, "usage limit"))
            .expect("valid")
            .signals[0]
            .definition_hash()
            .expect("a digest");
        assert_eq!(first, again);
    }

    /// Marker order is part of the definition: classification returns the first
    /// match, so reordering can change behaviour and must change the digest.
    #[test]
    fn reordering_markers_changes_the_definition_hash() {
        let ordered = "schema_version: 1\nsignals:\n  - id: codex-usage-limit\n    version: 1\n    \
             provider: codex-work\n    basis: plan_allowance\n    markers: ['a', 'b']\n";
        let reversed = "schema_version: 1\nsignals:\n  - id: codex-usage-limit\n    version: 1\n    \
             provider: codex-work\n    basis: plan_allowance\n    markers: ['b', 'a']\n";
        assert_ne!(
            parse(ordered).expect("valid").signals[0]
                .definition_hash()
                .expect("a digest"),
            parse(reversed).expect("valid").signals[0]
                .definition_hash()
                .expect("a digest"),
        );
    }

    /// One provider may carry several distinct fingerprints -- a vendor that
    /// words a spend limit and a rate limit differently -- under distinct ids.
    #[test]
    fn one_provider_may_hold_several_fingerprints_under_distinct_ids() {
        let document = "schema_version: 1\nsignals:\n  - id: claude-spend-limit\n    version: 1\n    \
             provider: claude-work\n    basis: plan_allowance\n    markers: ['individual spend limit']\n  \
             - id: claude-rate-limit\n    version: 1\n    provider: claude-work\n    \
             basis: plan_allowance\n    markers: ['rate limit']\n";
        let parsed = parse(document).expect("distinct ids on one provider are valid");
        assert_eq!(parsed.signals.len(), 2);
        assert_ne!(
            parsed.signals[0].definition_hash().expect("a digest"),
            parsed.signals[1].definition_hash().expect("a digest"),
        );
    }

    /// The two shipped Claude entries share their wording and must still be
    /// distinguishable, because they authorize different accounts.
    #[test]
    fn the_shipped_claude_logins_are_distinct_signals() {
        let document = parse(EXAMPLE).expect("the shipped example is valid");
        let claude: Vec<&QuotaSignal> = document
            .signals
            .iter()
            .filter(|signal| signal.provider.starts_with("claude"))
            .collect();
        assert_eq!(claude.len(), 2);
        assert_ne!(
            claude[0].id, claude[1].id,
            "identical wording, distinct ids"
        );
        assert_ne!(
            claude[0].definition_hash().expect("a digest"),
            claude[1].definition_hash().expect("a digest"),
            "so provenance can say which login's fingerprint fired",
        );
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
        let document = "schema_version: 1\nsignals:\n  - id: t-openrouter\n    version: 1\n    provider: openrouter\n    basis: credit_balance\n    markers: ['insufficient', 'credits']\n";
        let parsed = parse(document).expect("valid document");
        assert_eq!(parsed.signals[0].basis, QuotaBasis::CreditBalance);
    }
}
