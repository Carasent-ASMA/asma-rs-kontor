//! Reading one account's live quota from the provider's own usage endpoint.
//!
//! # Why this sits beside [`crate::classify`] instead of replacing it
//!
//! Classification reads a refusal. It therefore only ever runs *after* a launch
//! has already been turned away, it depends on wording the vendor can reword,
//! and it can never report that a window has reopened — nothing refuses when
//! quota is fine. A Realm built on refusals alone learns about every limit one
//! wasted launch late and then needs a human to clear the state afterwards.
//!
//! A usage reading is the same fact taken from the endpoint the vendor's own
//! client polls: structured, current, and answerable while everything is
//! working. Both are kept because they carry different authority and disagree
//! usefully — see [`kontor_core::spec::ProviderQuotaSource`].
//!
//! # What is deliberately not read out of the document
//!
//! The response names the account: an email address, a user id, a workspace id
//! and marketing copy written for a human. None of it reaches [`UsageReading`],
//! which holds two booleans and a window. There is nowhere in this module's
//! output for an identifier to travel — the same construction
//! [`crate::CapacityReading`] uses, and the reason this crate can hash its own
//! evidence without hashing a fact about a person.

use kontor_core::id::{ContentHash, Timestamp};
use kontor_core::spec::ProviderQuotaKind;
use serde::Deserialize;

use crate::quota::ObservedQuota;

/// Why a usage endpoint did not produce a reading.
///
/// A closed set of codes, for the same reason [`crate::ResolutionReason`] is
/// one: an HTTP error's `Display` can carry the URL it failed on, and that URL
/// is built from an account's own credential home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum UsageFailure {
    /// No credential could be found for the account.
    #[error("the account has no readable credential")]
    NoCredential,
    /// The provider rejected the credential.
    #[error("the provider rejected the account's credential")]
    Unauthorized,
    /// The endpoint could not be reached, or answered with a transport error.
    #[error("the provider's usage endpoint could not be reached")]
    Unreachable,
    /// The endpoint answered with something this build cannot read.
    #[error("the provider's usage endpoint answered unusably")]
    Unreadable,
}

/// One rate-limit window, reduced to the two facts an admission decision needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageWindow {
    /// How much of the window is spent, `0..=100`.
    pub used_percent: u8,
    /// When the window rolls over, when the provider states an instant.
    pub resets_at: Option<Timestamp>,
}

/// What one usage endpoint answered about one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageReading {
    /// The provider the reading is about, spelled as the catalog spells it.
    pub provider: String,
    /// Whether the provider is turning this account away right now.
    pub limit_reached: bool,
    /// The account's main allowance window, when the document carries one.
    ///
    /// Only the *primary* window. A document may also carry per-feature limits
    /// for individual models, and one of those being spent does not make the
    /// provider unusable — a row keyed by provider has nowhere to say "this
    /// model only", so folding them in would block a whole route because one
    /// niche model's window closed.
    ///
    /// ponytail: primary only. Per-feature windows need a grain
    /// `provider_quota_states` does not have; when it has one, they belong
    /// here as a second field rather than merged into this one.
    pub primary: Option<UsageWindow>,
    /// Whether a prepaid balance, rather than a clock, is what is missing.
    pub credits_exhausted: bool,
}

impl UsageReading {
    /// A digest over the numbers that were reported, and nothing that names the
    /// account.
    ///
    /// The obvious evidence digest is a hash of the response body, and it is
    /// the wrong one: the body carries an email address and a workspace id, and
    /// this crate does not put facts about a person into a durable, exportable
    /// column even in digested form. What it hashes instead is the reading —
    /// which is the evidence Kontor actually acted on, and which has the useful
    /// property that two identical readings digest identically, so an unchanged
    /// row is visibly unchanged.
    #[must_use]
    pub fn evidence(&self) -> ContentHash {
        let mut material = String::new();
        material.push_str("provider:");
        material.push_str(&self.provider);
        material.push_str("\nlimit_reached:");
        material.push_str(if self.limit_reached { "1" } else { "0" });
        material.push_str("\ncredits_exhausted:");
        material.push_str(if self.credits_exhausted { "1" } else { "0" });
        if let Some(window) = self.primary {
            material.push_str("\nused_percent:");
            material.push_str(&window.used_percent.to_string());
            if let Some(instant) = window.resets_at {
                material.push_str("\nresets_at:");
                material.push_str(&instant.to_string());
            }
        }
        material.push('\n');
        ContentHash::of(material.as_bytes())
    }
}

/// What one usage reading means for admission.
///
/// The four outcomes, and the one rule that is not obvious:
///
/// * not turned away yet — [`ProviderQuotaKind::Available`]. This is the state
///   no other source can produce, because nothing refuses when quota is fine;
/// * turned away with a stated reset instant — [`ProviderQuotaKind::Exhausted`],
///   which lifts itself at that instant;
/// * turned away with no instant, and a spent balance —
///   [`ProviderQuotaKind::Drained`], which only payment lifts;
/// * turned away with neither — [`ProviderQuotaKind::Unknown`], which blocks and
///   says so. Not `Drained`: asserting that money is the remedy when the
///   document did not say so sends an operator to a billing page for a limit
///   that would have cleared on its own.
///
/// **A stated reset instant wins over a spent balance.** A plan can report both
/// at once — a workspace seat whose credits are gone still has a weekly window
/// that rolls over — and when it does, the instant is the operationally true
/// fact: the account *will* work again then, with nobody paying anything.
/// Recording `Drained` there would take a route out of service indefinitely and
/// point the operator at the wrong remedy. It is also what the store's own
/// invariant expects, since a `drained` row is forbidden from carrying a reset.
#[must_use]
pub fn observe(reading: &UsageReading) -> ObservedQuota {
    if !reading.limit_reached {
        return ObservedQuota {
            provider: reading.provider.clone(),
            kind: ProviderQuotaKind::Available,
            resets_at: None,
        };
    }
    match reading.primary.and_then(|window| window.resets_at) {
        Some(instant) => ObservedQuota {
            provider: reading.provider.clone(),
            kind: ProviderQuotaKind::Exhausted,
            resets_at: Some(instant),
        },
        None if reading.credits_exhausted => ObservedQuota {
            provider: reading.provider.clone(),
            kind: ProviderQuotaKind::Drained,
            resets_at: None,
        },
        None => ObservedQuota {
            provider: reading.provider.clone(),
            kind: ProviderQuotaKind::Unknown,
            resets_at: None,
        },
    }
}

// ---------------------------------------------------------------------------
// The ChatGPT usage document
// ---------------------------------------------------------------------------

/// Read a ChatGPT usage response into a [`UsageReading`].
///
/// # Errors
/// Returns [`UsageFailure::Unreadable`] when the body is not JSON at all. A
/// body that *is* JSON but has lost or gained fields still reads: every field
/// below is optional and defaulted, deliberately, because the alternative is a
/// Realm that stops observing its own quota the week a vendor adds a key.
///
/// # Why this shape lives in Rust when [`crate::QuotaSignal`] is data
///
/// A refusal signal is a handful of substrings, so it can be configuration and
/// a vendor rewording costs nothing. A response schema is a tree, and making it
/// data means shipping a path language and a validator for it — much more code
/// than the six fields below, to describe one endpoint. When a second vendor
/// needs one, it gets its own reader beside this one.
pub fn read_chatgpt_usage(provider: &str, body: &[u8]) -> Result<UsageReading, UsageFailure> {
    let document: UsageDocument =
        serde_json::from_slice(body).map_err(|_| UsageFailure::Unreadable)?;
    let rate_limit = document.rate_limit.unwrap_or_default();
    Ok(UsageReading {
        provider: provider.to_owned(),
        limit_reached: rate_limit.limit_reached,
        primary: rate_limit.primary_window.map(|window| UsageWindow {
            // Clamped first, so the conversion cannot fail; a vendor
            // reporting 143% or -1 is a reading, not a refusal.
            used_percent: u8::try_from(window.used_percent.clamp(0, 100)).unwrap_or(100),
            // An instant before the epoch is not a reset, it is a vendor
            // sending a placeholder; a reset in the past is harmless, since a
            // state whose instant has passed simply stops blocking.
            resets_at: window
                .reset_at
                .filter(|seconds| *seconds > 0)
                .and_then(|seconds| Timestamp::from_second(seconds).ok()),
        }),
        credits_exhausted: document.credits.unwrap_or_default().is_exhausted(),
    })
}

/// The fields Kontor reads out of a ChatGPT usage response.
///
/// `deny_unknown_fields` is deliberately **absent**: the live document carries
/// referral programmes, upsell banners and promotional copy this build has no
/// interest in, and refusing the whole reading because one of them changed
/// would make the poller fail exactly when a vendor is changing something.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct UsageDocument {
    rate_limit: Option<RateLimit>,
    credits: Option<Credits>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RateLimit {
    limit_reached: bool,
    primary_window: Option<Window>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Window {
    used_percent: i32,
    /// Seconds since the Unix epoch, unquoted.
    reset_at: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Credits {
    has_credits: bool,
    unlimited: bool,
    overage_limit_reached: bool,
}

impl Credits {
    /// Whether a balance, rather than a clock, is what is missing.
    ///
    /// An unlimited account never qualifies however the other two flags read: a
    /// plan with no balance to spend cannot have spent it.
    const fn is_exhausted(&self) -> bool {
        !self.unlimited && (!self.has_credits || self.overage_limit_reached)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(limit_reached: bool, resets_at: Option<i64>, credits_exhausted: bool) -> UsageReading {
        UsageReading {
            provider: "codex".to_owned(),
            limit_reached,
            primary: Some(UsageWindow {
                used_percent: if limit_reached { 100 } else { 12 },
                resets_at: resets_at.map(|seconds| {
                    Timestamp::from_second(seconds).expect("a representable instant")
                }),
            }),
            credits_exhausted,
        }
    }

    #[test]
    fn an_account_that_is_not_being_turned_away_is_available() {
        let observed = observe(&reading(false, Some(1_787_421_242), false));
        assert_eq!(observed.kind, ProviderQuotaKind::Available);
        // Available carries no instant even though the window has one: the
        // store forbids a reset on anything but `exhausted`.
        assert_eq!(observed.resets_at, None);
    }

    #[test]
    fn a_stated_reset_instant_outranks_a_spent_balance() {
        // The live shape of a workspace seat whose credits are gone and whose
        // weekly window still rolls over. `Drained` here would take the route
        // out of service until somebody paid for something that was about to
        // fix itself.
        let observed = observe(&reading(true, Some(1_787_421_242), true));
        assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
        assert_eq!(
            observed.resets_at,
            Some(Timestamp::from_second(1_787_421_242).expect("a representable instant"))
        );
    }

    #[test]
    fn a_spent_balance_with_no_instant_is_drained() {
        let observed = observe(&reading(true, None, true));
        assert_eq!(observed.kind, ProviderQuotaKind::Drained);
        assert_eq!(observed.resets_at, None);
    }

    #[test]
    fn a_refusal_with_neither_an_instant_nor_a_balance_is_unknown() {
        let observed = observe(&reading(true, None, false));
        assert_eq!(observed.kind, ProviderQuotaKind::Unknown);
        assert_eq!(observed.resets_at, None);
    }

    #[test]
    fn the_live_work_account_document_reads_as_exhausted() {
        // Trimmed from a real 2026-08-21 response for the team-plan account:
        // credits depleted *and* a weekly window that returns.
        let body = br#"{
            "user_id": "user-redacted",
            "email": "someone@example.com",
            "plan_type": "team",
            "rate_limit": {
                "allowed": false,
                "limit_reached": true,
                "primary_window": {
                    "used_percent": 100,
                    "limit_window_seconds": 604800,
                    "reset_after_seconds": 74033,
                    "reset_at": 1787421242
                },
                "secondary_window": null
            },
            "additional_rate_limits": null,
            "credits": {"has_credits": false, "unlimited": false, "overage_limit_reached": false},
            "rate_limit_reached_type": {"type": "workspace_member_credits_depleted"},
            "rate_limit_upsell": {"title": "Get 250 credits"}
        }"#;
        let read = read_chatgpt_usage("codex", body).expect("the document reads");
        assert!(read.limit_reached);
        assert!(read.credits_exhausted);
        assert_eq!(
            read.primary.and_then(|window| window.resets_at),
            Some(Timestamp::from_second(1_787_421_242).expect("a representable instant"))
        );
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Exhausted);
    }

    #[test]
    fn an_unlimited_account_is_never_read_as_having_spent_a_balance() {
        let body = br#"{
            "rate_limit": {"limit_reached": true, "primary_window": null},
            "credits": {"has_credits": false, "unlimited": true, "overage_limit_reached": false}
        }"#;
        let read = read_chatgpt_usage("codex", body).expect("the document reads");
        assert!(!read.credits_exhausted);
        // With no balance to blame and no instant to wait for, the honest
        // answer is that nobody knows.
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Unknown);
    }

    #[test]
    fn an_unfamiliar_document_still_reads_rather_than_refusing() {
        // A vendor adding a key, renaming a sibling and dropping `credits`
        // must not stop a Realm observing its own quota.
        let body = br#"{
            "rate_limit": {"limit_reached": false, "primary_window": {"used_percent": 3},
                           "brand_new_field": {"nested": true}},
            "something_added_last_tuesday": [1, 2, 3]
        }"#;
        let read = read_chatgpt_usage("codex", body).expect("the document reads");
        assert!(!read.limit_reached);
        assert_eq!(read.primary.map(|window| window.used_percent), Some(3));
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Available);
    }

    #[test]
    fn a_body_that_is_not_json_is_refused_without_quoting_it() {
        let failure = read_chatgpt_usage("codex", b"<html>login</html>").expect_err("refused");
        assert_eq!(failure, UsageFailure::Unreadable);
        assert!(!failure.to_string().contains("html"));
    }

    #[test]
    fn a_placeholder_reset_instant_is_not_treated_as_one() {
        let body = br#"{"rate_limit": {"limit_reached": true,
                        "primary_window": {"used_percent": 100, "reset_at": 0}},
                        "credits": {"has_credits": true}}"#;
        let read = read_chatgpt_usage("codex", body).expect("the document reads");
        assert_eq!(read.primary.and_then(|window| window.resets_at), None);
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Unknown);
    }

    #[test]
    fn two_identical_readings_digest_identically_and_a_changed_one_does_not() {
        let first = reading(true, Some(1_787_421_242), true);
        let same = reading(true, Some(1_787_421_242), true);
        let later = reading(true, Some(1_787_470_519), true);
        assert_eq!(first.evidence(), same.evidence());
        assert_ne!(first.evidence(), later.evidence());
    }
}
