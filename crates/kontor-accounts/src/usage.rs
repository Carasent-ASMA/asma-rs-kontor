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
//! # One reading type, one reader per vendor
//!
//! Two vendors describe the same fact incompatibly. Codex states a
//! `limit_reached` boolean and window spans in seconds; Claude states neither,
//! and a caller has to infer "blocked" from a utilisation percentage. So each
//! gets its own reader — [`read_chatgpt_usage`], [`read_claude_usage`] — and
//! both produce the same [`UsageReading`]. [`observe`] then judges that one
//! shape, so the rule about what a spent window *means* is written once and
//! cannot drift between providers.
//!
//! # What is deliberately not read out of the document
//!
//! Both responses name the account: an email address, a user id, a workspace id
//! and marketing copy written for a human. None of it reaches [`UsageReading`],
//! which holds two booleans and a set of windows. There is nowhere in this
//! module's output for an identifier to travel — the same construction
//! [`crate::CapacityReading`] uses, and the reason this crate can hash its own
//! evidence without hashing a fact about a person.
//!
//! # Why model-scoped windows are read and dropped
//!
//! Both vendors report per-model allowances beside the account-level ones —
//! Codex in `additional_rate_limits[]`, Claude as `seven_day_opus` and
//! `seven_day_omelette`. None of them reaches a stored window, because
//! `provider_quota_states` is keyed by *provider* and
//! [`kontor_core::quota::QuotaWindow`] has no field naming a scope. A spent
//! Opus week folded in as a plain weekly window would take every Sonnet seat on
//! the account out of service too, which is worse than not knowing.

use kontor_core::id::{ContentHash, Timestamp};
use kontor_core::quota::{QuotaWindow, QuotaWindowKind};
use kontor_core::spec::ProviderQuotaKind;
use serde::Deserialize;

use crate::quota::ObservedQuota;

/// The utilisation at which a window has nothing left.
const SPENT_PERCENT: u8 = 100;

/// Minutes in Claude's short rolling window.
const FIVE_HOUR_MINUTES: u32 = 300;

/// Minutes in a seven-day window.
const SEVEN_DAY_MINUTES: u32 = 10_080;

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

/// What one usage endpoint answered about one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageReading {
    /// The provider the reading is about, spelled as the catalog spells it.
    pub provider: String,
    /// Whether the provider is turning this account away right now.
    ///
    /// Stated outright by Codex. Derived for Claude, whose document carries no
    /// such flag — see [`read_claude_usage`].
    pub limit_reached: bool,
    /// Every *account-level* window the document named, in a stable order.
    ///
    /// A window only appears here if the vendor gave it a reset instant, since
    /// [`QuotaWindow`] requires one: an allowance that cannot say when it
    /// returns is a [`ProviderQuotaKind::Unknown`] state, not a window.
    pub windows: Vec<QuotaWindow>,
    /// Whether a prepaid balance, rather than a clock, is what is missing.
    pub credits_exhausted: bool,
}

impl UsageReading {
    /// The windows the provider reports as spent.
    fn spent(&self) -> impl Iterator<Item = &QuotaWindow> {
        self.windows
            .iter()
            .filter(|window| window.used_percent >= SPENT_PERCENT)
    }

    /// A digest over the numbers that were reported, and nothing that names the
    /// account.
    ///
    /// The obvious evidence digest is a hash of the response body, and it is
    /// the wrong one: the body carries an email address and a workspace id, and
    /// this crate does not put facts about a person into a durable, exportable
    /// column even in digested form. What it hashes instead is the reading —
    /// which is the evidence Kontor actually acted on, and which has the useful
    /// property that two identical readings digest identically, so an unchanged
    /// row is visibly unchanged and the poller can skip writing it.
    #[must_use]
    pub fn evidence(&self) -> ContentHash {
        let mut material = String::new();
        material.push_str("provider:");
        material.push_str(&self.provider);
        material.push_str("\nlimit_reached:");
        material.push_str(if self.limit_reached { "1" } else { "0" });
        material.push_str("\ncredits_exhausted:");
        material.push_str(if self.credits_exhausted { "1" } else { "0" });
        for window in &self.windows {
            material.push_str("\nwindow:");
            material.push_str(window.kind.as_str());
            material.push('=');
            material.push_str(&window.used_percent.to_string());
            material.push('@');
            material.push_str(&window.resets_at.to_string());
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
/// * turned away with a reset instant to point at —
///   [`ProviderQuotaKind::Exhausted`], which lifts itself at that instant;
/// * turned away with no instant at all, and a spent balance —
///   [`ProviderQuotaKind::Drained`], which only payment lifts;
/// * turned away with neither — [`ProviderQuotaKind::Unknown`], which blocks and
///   says so. Not `Drained`: asserting that money is the remedy when the
///   document did not say so sends an operator to a billing page for a limit
///   that would have cleared on its own.
///
/// **The instant is the latest among the *spent* windows.** An account holding a
/// five-hour and a seven-day allowance is usable again only when the last thing
/// standing in the way clears, so taking the earliest would send a launch back
/// into the limit it just hit. When the provider says it is turning the account
/// away but names no spent window, the latest instant it *did* name is used
/// instead of refusing to answer — parking on `Unknown` because a vendor
/// reported 99% and a refusal in the same breath discards a usable instant.
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
            // A structured report matched no fingerprint.
            signal: None,
            provider: reading.provider.clone(),
            kind: ProviderQuotaKind::Available,
            resets_at: None,
        };
    }
    let latest = reading
        .spent()
        .map(|window| window.resets_at)
        .max()
        .or_else(|| reading.windows.iter().map(|window| window.resets_at).max());
    match latest {
        Some(instant) => ObservedQuota {
            // A structured report matched no fingerprint.
            signal: None,
            provider: reading.provider.clone(),
            kind: ProviderQuotaKind::Exhausted,
            resets_at: Some(instant),
        },
        None if reading.credits_exhausted => ObservedQuota {
            // A structured report matched no fingerprint.
            signal: None,
            provider: reading.provider.clone(),
            kind: ProviderQuotaKind::Drained,
            resets_at: None,
        },
        None => ObservedQuota {
            // A structured report matched no fingerprint.
            signal: None,
            provider: reading.provider.clone(),
            kind: ProviderQuotaKind::Unknown,
            resets_at: None,
        },
    }
}

/// Build one window, dropping it when the vendor gave no usable instant.
fn window(minutes: u32, used_percent: u8, resets_at: Option<Timestamp>) -> Option<QuotaWindow> {
    Some(QuotaWindow {
        kind: QuotaWindowKind::from_minutes(minutes),
        resets_at: resets_at?,
        used_percent: used_percent.min(SPENT_PERCENT),
    })
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
/// than the fields below, to describe one endpoint.
pub fn read_chatgpt_usage(provider: &str, body: &[u8]) -> Result<UsageReading, UsageFailure> {
    let document: ChatGptDocument =
        serde_json::from_slice(body).map_err(|_| UsageFailure::Unreadable)?;
    let rate_limit = document.rate_limit.unwrap_or_default();
    let windows = [rate_limit.primary_window, rate_limit.secondary_window]
        .into_iter()
        .flatten()
        .filter_map(|reported| {
            window(
                reported.window_minutes(),
                reported.percent(),
                reported.instant(),
            )
        })
        .collect();
    Ok(UsageReading {
        provider: provider.to_owned(),
        limit_reached: rate_limit.limit_reached,
        windows,
        credits_exhausted: document.credits.unwrap_or_default().is_exhausted(),
    })
}

/// Read a ChatGPT usage response for an admission preflight.
///
/// The background collector deliberately tolerates additive and partially
/// deployed vendor shapes. Admission cannot: an absent `rate_limit` or its
/// explicit `limit_reached` fact must not become a fresh `Available` proof.
/// A stated refusal with no usable reset is still a valid blocking `Unknown`
/// reading rather than a parser failure.
///
/// # Errors
/// [`UsageFailure::Unreadable`] when the document does not carry the minimum
/// catalogued account-level quota facts needed for admission.
pub fn read_chatgpt_usage_strict(
    provider: &str,
    body: &[u8],
) -> Result<UsageReading, UsageFailure> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| UsageFailure::Unreadable)?;
    let rate_limit = value
        .as_object()
        .and_then(|root| root.get("rate_limit"))
        .and_then(serde_json::Value::as_object)
        .ok_or(UsageFailure::Unreadable)?;
    let limit_reached = rate_limit
        .get("limit_reached")
        .and_then(serde_json::Value::as_bool)
        .ok_or(UsageFailure::Unreadable)?;
    let mut catalogued_fact = limit_reached;
    let mut resetless_spent_window = false;
    for key in ["primary_window", "secondary_window"] {
        let Some(window) = rate_limit.get(key) else {
            continue;
        };
        if window.is_null() {
            continue;
        }
        catalogued_fact = true;
        let window = window.as_object().ok_or(UsageFailure::Unreadable)?;
        let used = window
            .get("used_percent")
            .and_then(serde_json::Value::as_i64)
            .ok_or(UsageFailure::Unreadable)?;
        let reset_is_usable = window
            .get("reset_at")
            .and_then(serde_json::Value::as_i64)
            .is_some_and(|instant| instant > 0);
        if !reset_is_usable && used >= i64::from(SPENT_PERCENT) {
            resetless_spent_window = true;
        }
        if !reset_is_usable && !limit_reached && used < i64::from(SPENT_PERCENT) {
            return Err(UsageFailure::Unreadable);
        }
    }
    if let Some(credits) = value.get("credits") {
        catalogued_fact = true;
        let credits = credits.as_object().ok_or(UsageFailure::Unreadable)?;
        for field in ["has_credits", "unlimited", "overage_limit_reached"] {
            if !credits
                .get(field)
                .is_some_and(serde_json::Value::is_boolean)
            {
                return Err(UsageFailure::Unreadable);
            }
        }
    }
    if !catalogued_fact {
        return Err(UsageFailure::Unreadable);
    }
    let mut reading = read_chatgpt_usage(provider, body)?;
    if value.get("credits").is_none() {
        // The compatible reader's empty document default is conservative for
        // background collection. Admission may not turn an absent balance
        // section into evidence that prepaid credit is exhausted.
        reading.credits_exhausted = false;
    }
    if resetless_spent_window {
        // A contradictory or lagging top-level flag cannot erase the only
        // concrete account-level fact in the response: a fully consumed
        // allowance with no usable reset is blocking `Unknown` evidence.
        reading.limit_reached = true;
    }
    Ok(reading)
}

/// The fields Kontor reads out of a ChatGPT usage response.
///
/// `deny_unknown_fields` is deliberately **absent**: the live document carries
/// referral programmes, upsell banners and promotional copy this build has no
/// interest in, and refusing the whole reading because one of them changed
/// would make the poller fail exactly when a vendor is changing something.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ChatGptDocument {
    rate_limit: Option<ChatGptRateLimit>,
    credits: Option<Credits>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ChatGptRateLimit {
    limit_reached: bool,
    primary_window: Option<ChatGptWindow>,
    secondary_window: Option<ChatGptWindow>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ChatGptWindow {
    used_percent: i32,
    /// The window's span. Absent on some shapes, which reads as a session.
    limit_window_seconds: Option<i64>,
    /// Seconds since the Unix epoch, unquoted.
    reset_at: Option<i64>,
}

impl ChatGptWindow {
    /// Clamped first, so the conversion cannot fail; a vendor reporting 143% or
    /// -1 is a reading, not a refusal.
    fn percent(&self) -> u8 {
        u8::try_from(self.used_percent.clamp(0, i32::from(SPENT_PERCENT))).unwrap_or(SPENT_PERCENT)
    }

    /// The span in minutes, defaulting to the shortest classification when the
    /// vendor omits it — a window of unknown length is not a weekly allowance.
    fn window_minutes(&self) -> u32 {
        self.limit_window_seconds
            .and_then(|seconds| u32::try_from(seconds / 60).ok())
            .unwrap_or(1)
    }

    /// An instant at or before the epoch is not a reset, it is a vendor sending
    /// a placeholder. A reset in the *past* is kept and harmless: a state whose
    /// instant has passed simply stops blocking.
    fn instant(&self) -> Option<Timestamp> {
        self.reset_at
            .filter(|seconds| *seconds > 0)
            .and_then(|seconds| Timestamp::from_second(seconds).ok())
    }
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

// ---------------------------------------------------------------------------
// The Claude usage document
// ---------------------------------------------------------------------------

/// Read a Claude OAuth usage response into a [`UsageReading`].
///
/// # Why `limit_reached` is derived here and stated there
///
/// The document carries **no** flag saying the account is being turned away.
/// It reports a utilisation percentage per window and nothing else, so "blocked"
/// is a conclusion this reader has to reach: an account is turned away when any
/// account-level window it named is at or over 100%. That is the one real
/// semantic difference between the two vendors, and putting it here rather than
/// in [`observe`] keeps the judgement of a *spent window* identical for both.
///
/// `seven_day_opus` and `seven_day_omelette` are parsed and dropped; see the
/// module note on why a model-scoped window must not become a plain one.
///
/// # Errors
/// Returns [`UsageFailure::Unreadable`] when the body is not JSON at all.
pub fn read_claude_usage(provider: &str, body: &[u8]) -> Result<UsageReading, UsageFailure> {
    let document: ClaudeDocument =
        serde_json::from_slice(body).map_err(|_| UsageFailure::Unreadable)?;
    let windows: Vec<QuotaWindow> = [
        (FIVE_HOUR_MINUTES, document.five_hour),
        (SEVEN_DAY_MINUTES, document.seven_day),
    ]
    .into_iter()
    .filter_map(|(minutes, reported)| {
        let reported = reported?;
        window(minutes, reported.percent(), reported.instant())
    })
    .collect();
    Ok(UsageReading {
        provider: provider.to_owned(),
        limit_reached: windows
            .iter()
            .any(|entry| entry.used_percent >= SPENT_PERCENT),
        windows,
        // Claude states no prepaid balance on this endpoint. `extra_usage`
        // reports only whether overflow billing is *enabled*, which is a plan
        // setting and not a balance, so nothing here may claim one is spent.
        credits_exhausted: false,
    })
}

/// Read a Claude usage response for an admission preflight.
///
/// At least one account-level window with a parseable utilisation is required.
/// A spent window whose reset is absent or unusable is retained as the blocking
/// fact `limit_reached = true` even though it cannot become a [`QuotaWindow`];
/// [`observe`] consequently returns `Unknown`, never `Available`.
///
/// # Errors
/// [`UsageFailure::Unreadable`] for an empty, model-only, malformed or
/// otherwise incomplete account-level document.
pub fn read_claude_usage_strict(provider: &str, body: &[u8]) -> Result<UsageReading, UsageFailure> {
    let document: ClaudeDocument =
        serde_json::from_slice(body).map_err(|_| UsageFailure::Unreadable)?;
    let reported = [
        (FIVE_HOUR_MINUTES, document.five_hour),
        (SEVEN_DAY_MINUTES, document.seven_day),
    ];
    if reported.iter().all(|(_, window)| window.is_none()) {
        return Err(UsageFailure::Unreadable);
    }
    let mut limit_reached = false;
    let mut windows = Vec::new();
    for (minutes, reported_window) in reported {
        let Some(reported_window) = reported_window else {
            continue;
        };
        let used = reported_window
            .strict_percent()
            .ok_or(UsageFailure::Unreadable)?;
        let instant = reported_window.instant();
        if used >= SPENT_PERCENT {
            limit_reached = true;
        } else if instant.is_none() {
            return Err(UsageFailure::Unreadable);
        }
        if let Some(window) = window(minutes, used, instant) {
            windows.push(window);
        }
    }
    Ok(UsageReading {
        provider: provider.to_owned(),
        limit_reached,
        windows,
        credits_exhausted: false,
    })
}

/// The fields Kontor reads out of a Claude usage response.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ClaudeDocument {
    five_hour: Option<ClaudeWindow>,
    seven_day: Option<ClaudeWindow>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ClaudeWindow {
    utilization: Option<LooseNumber>,
    /// An RFC 3339 instant, unlike Codex's epoch integer.
    resets_at: Option<String>,
}

impl ClaudeWindow {
    fn percent(&self) -> u8 {
        self.strict_percent().unwrap_or(0)
    }

    fn strict_percent(&self) -> Option<u8> {
        self.utilization.as_ref().and_then(LooseNumber::percent)
    }

    fn instant(&self) -> Option<Timestamp> {
        self.resets_at.as_deref()?.parse().ok()
    }
}

/// A number that may arrive quoted.
///
/// Defended against deliberately rather than optimistically: the vendor's own
/// client validates this field through a permissive number schema, which is
/// evidence that both forms occur in practice.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
enum LooseNumber {
    Number(f64),
    Text(String),
}

impl LooseNumber {
    fn percent(&self) -> Option<u8> {
        let value = match self {
            Self::Number(value) => *value,
            Self::Text(text) => text.trim().parse().ok()?,
        };
        if value.is_nan() {
            return None;
        }
        // Truncation is the safe direction for a *floor* comparison: 99.9% is
        // not spent, and rounding it to 100 would park a usable account.
        Some(value.clamp(0.0, f64::from(SPENT_PERCENT)) as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> Timestamp {
        Timestamp::from_second(seconds).expect("a representable instant")
    }

    fn weekly(used_percent: u8, seconds: i64) -> QuotaWindow {
        QuotaWindow {
            kind: QuotaWindowKind::Weekly,
            resets_at: at(seconds),
            used_percent,
        }
    }

    fn session(used_percent: u8, seconds: i64) -> QuotaWindow {
        QuotaWindow {
            kind: QuotaWindowKind::Session,
            resets_at: at(seconds),
            used_percent,
        }
    }

    fn reading(limit_reached: bool, windows: Vec<QuotaWindow>, credits: bool) -> UsageReading {
        UsageReading {
            provider: "codex".to_owned(),
            limit_reached,
            windows,
            credits_exhausted: credits,
        }
    }

    // -- what a reading means ------------------------------------------------

    #[test]
    fn an_account_that_is_not_being_turned_away_is_available() {
        let observed = observe(&reading(false, vec![weekly(12, 1_787_421_242)], false));
        assert_eq!(observed.kind, ProviderQuotaKind::Available);
        // Available carries no instant even though a window has one: the store
        // forbids a reset on anything but `exhausted`.
        assert_eq!(observed.resets_at, None);
    }

    #[test]
    fn the_instant_is_the_latest_among_the_spent_windows_not_the_earliest() {
        // A five-hour window clears long before the week does. Taking the
        // earliest would send the next launch straight back into the weekly
        // limit it just hit.
        let observed = observe(&reading(
            true,
            vec![session(100, 1_787_400_000), weekly(100, 1_787_470_519)],
            false,
        ));
        assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
        assert_eq!(observed.resets_at, Some(at(1_787_470_519)));
    }

    #[test]
    fn an_unspent_window_does_not_extend_the_wait() {
        // Only the session window is gone, so the account is usable again when
        // it clears — the weekly instant is irrelevant and must not be chosen.
        let observed = observe(&reading(
            true,
            vec![session(100, 1_787_400_000), weekly(40, 1_787_470_519)],
            false,
        ));
        assert_eq!(observed.resets_at, Some(at(1_787_400_000)));
    }

    #[test]
    fn a_refusal_naming_no_spent_window_still_uses_an_instant_it_did_name() {
        // Parking on `Unknown` because the vendor said 99% and "refused" in the
        // same breath throws away a usable instant.
        let observed = observe(&reading(true, vec![weekly(99, 1_787_470_519)], false));
        assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
        assert_eq!(observed.resets_at, Some(at(1_787_470_519)));
    }

    #[test]
    fn a_stated_reset_instant_outranks_a_spent_balance() {
        // The live shape of a workspace seat whose credits are gone and whose
        // weekly window still rolls over. `Drained` here would take the route
        // out of service until somebody paid for something about to fix itself.
        let observed = observe(&reading(true, vec![weekly(100, 1_787_421_242)], true));
        assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
        assert_eq!(observed.resets_at, Some(at(1_787_421_242)));
    }

    #[test]
    fn a_spent_balance_with_no_window_at_all_is_drained() {
        let observed = observe(&reading(true, Vec::new(), true));
        assert_eq!(observed.kind, ProviderQuotaKind::Drained);
        assert_eq!(observed.resets_at, None);
    }

    #[test]
    fn a_refusal_with_neither_a_window_nor_a_balance_is_unknown() {
        let observed = observe(&reading(true, Vec::new(), false));
        assert_eq!(observed.kind, ProviderQuotaKind::Unknown);
        assert_eq!(observed.resets_at, None);
    }

    #[test]
    fn two_identical_readings_digest_identically_and_a_changed_one_does_not() {
        let first = reading(true, vec![weekly(100, 1_787_421_242)], true);
        let same = reading(true, vec![weekly(100, 1_787_421_242)], true);
        let later = reading(true, vec![weekly(100, 1_787_470_519)], true);
        let fuller = reading(
            true,
            vec![weekly(100, 1_787_421_242), session(3, 1_787_400_000)],
            true,
        );
        assert_eq!(first.evidence(), same.evidence());
        assert_ne!(first.evidence(), later.evidence());
        // A window appearing must change the digest, or the poller would skip
        // the write that records it.
        assert_ne!(first.evidence(), fuller.evidence());
    }

    // -- the ChatGPT reader --------------------------------------------------

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
        assert_eq!(read.windows, vec![weekly(100, 1_787_421_242)]);
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Exhausted);
    }

    #[test]
    fn a_span_the_vendor_omits_is_classified_as_the_shortest_not_the_longest() {
        let body = br#"{"rate_limit": {"limit_reached": true,
            "primary_window": {"used_percent": 100, "reset_at": 1787421242}}}"#;
        let read = read_chatgpt_usage("codex", body).expect("the document reads");
        assert_eq!(
            read.windows.first().map(|window| window.kind),
            Some(QuotaWindowKind::Session),
            "an unknown span must not be promoted to a weekly allowance"
        );
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
            "rate_limit": {"limit_reached": false,
                           "primary_window": {"used_percent": 3, "limit_window_seconds": 604800,
                                              "reset_at": 1787421242},
                           "brand_new_field": {"nested": true}},
            "something_added_last_tuesday": [1, 2, 3]
        }"#;
        let read = read_chatgpt_usage("codex", body).expect("the document reads");
        assert!(!read.limit_reached);
        assert_eq!(read.windows, vec![weekly(3, 1_787_421_242)]);
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Available);
    }

    #[test]
    fn a_body_that_is_not_json_is_refused_without_quoting_it() {
        let failure = read_chatgpt_usage("codex", b"<html>login</html>").expect_err("refused");
        assert_eq!(failure, UsageFailure::Unreadable);
        assert!(!failure.to_string().contains("html"));
        let claude = read_claude_usage("claude", b"<html>login</html>").expect_err("refused");
        assert_eq!(claude, UsageFailure::Unreadable);
    }

    #[test]
    fn a_placeholder_reset_instant_yields_no_window_at_all() {
        let body = br#"{"rate_limit": {"limit_reached": true,
                        "primary_window": {"used_percent": 100, "reset_at": 0}},
                        "credits": {"has_credits": true}}"#;
        let read = read_chatgpt_usage("codex", body).expect("the document reads");
        assert!(read.windows.is_empty());
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Unknown);
    }

    #[test]
    fn strict_chatgpt_refuses_empty_or_incomplete_documents_but_keeps_a_resetless_refusal_blocking()
    {
        assert_eq!(
            read_chatgpt_usage_strict("codex", b"{}").expect_err("empty is not evidence"),
            UsageFailure::Unreadable
        );
        assert_eq!(
            read_chatgpt_usage_strict("codex", br#"{"rate_limit": {}}"#)
                .expect_err("an unstated disposition is not evidence"),
            UsageFailure::Unreadable
        );
        assert_eq!(
            read_chatgpt_usage_strict("codex", br#"{"rate_limit": {"limit_reached": false}}"#,)
                .expect_err("an available flag without an allowance is incomplete"),
            UsageFailure::Unreadable
        );
        let blocked = read_chatgpt_usage_strict(
            "codex",
            br#"{"rate_limit": {"limit_reached": false,
                    "primary_window": {"used_percent": 100, "reset_at": 0}}}"#,
        )
        .expect("a resetless spent window remains usable blocking evidence");
        assert!(blocked.limit_reached);
        assert!(blocked.windows.is_empty());
        assert_eq!(observe(&blocked).kind, ProviderQuotaKind::Unknown);
    }

    #[test]
    fn strict_chatgpt_accepts_a_complete_available_control() {
        let reading = read_chatgpt_usage_strict(
            "codex",
            br#"{"rate_limit": {"limit_reached": false,
                    "primary_window": {"used_percent": 9,
                                       "limit_window_seconds": 18000,
                                       "reset_at": 1787421242}}}"#,
        )
        .expect("the catalogued account-level shape reads");
        assert_eq!(observe(&reading).kind, ProviderQuotaKind::Available);
        assert_eq!(reading.windows, vec![session(9, 1_787_421_242)]);
    }

    // -- the Claude reader ---------------------------------------------------

    #[test]
    fn claude_being_turned_away_is_derived_because_the_document_never_says_so() {
        // The whole semantic difference between the two vendors: there is no
        // `limit_reached` here, so a spent window is the only evidence.
        let body = br#"{
            "five_hour": {"utilization": 12, "resets_at": "2026-08-22T18:00:00Z"},
            "seven_day": {"utilization": 100, "resets_at": "2026-08-25T09:00:00Z"},
            "extra_usage": {"is_enabled": false}
        }"#;
        let read = read_claude_usage("claude", body).expect("the document reads");
        assert!(read.limit_reached, "a window at 100% is being turned away");
        let observed = observe(&read);
        assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
        assert_eq!(
            observed.resets_at,
            Some("2026-08-25T09:00:00Z".parse().expect("an instant"))
        );
    }

    #[test]
    fn claude_below_the_line_is_available_with_both_windows_recorded() {
        let body = br#"{
            "five_hour": {"utilization": 40, "resets_at": "2026-08-22T18:00:00Z"},
            "seven_day": {"utilization": 61, "resets_at": "2026-08-25T09:00:00Z"}
        }"#;
        let read = read_claude_usage("claude", body).expect("the document reads");
        assert!(!read.limit_reached);
        assert_eq!(
            read.windows
                .iter()
                .map(|window| (window.kind, window.used_percent))
                .collect::<Vec<_>>(),
            vec![
                (QuotaWindowKind::Session, 40),
                (QuotaWindowKind::Weekly, 61)
            ]
        );
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Available);
    }

    #[test]
    fn a_spent_model_scoped_week_does_not_take_the_whole_provider_out() {
        // `seven_day_opus` at 100% must not become a plain weekly window, or
        // every Sonnet seat on this account stops with it.
        let body = br#"{
            "five_hour": {"utilization": 5, "resets_at": "2026-08-22T18:00:00Z"},
            "seven_day": {"utilization": 20, "resets_at": "2026-08-25T09:00:00Z"},
            "seven_day_opus": {"utilization": 100, "resets_at": "2026-08-25T09:00:00Z"},
            "seven_day_omelette": {"utilization": 100, "resets_at": "2026-08-25T09:00:00Z"},
            "limits": [{"kind": "weekly_scoped", "utilization": 100}]
        }"#;
        let read = read_claude_usage("claude", body).expect("the document reads");
        assert!(!read.limit_reached);
        assert_eq!(read.windows.len(), 2, "only the account-level windows");
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Available);
    }

    #[test]
    fn claude_never_claims_a_spent_balance_because_the_endpoint_reports_none() {
        // `extra_usage.is_enabled` is a plan setting, not a balance. Reading it
        // as one would produce a `Drained` row that no clock ever lifts.
        let body = br#"{"seven_day": {"utilization": 100, "resets_at": "2026-08-25T09:00:00Z"},
                        "extra_usage": {"is_enabled": true}}"#;
        let read = read_claude_usage("claude", body).expect("the document reads");
        assert!(!read.credits_exhausted);
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Exhausted);
    }

    #[test]
    fn a_quoted_utilization_reads_and_a_fractional_one_floors() {
        let body = br#"{"five_hour": {"utilization": "99.9", "resets_at": "2026-08-22T18:00:00Z"},
                        "seven_day": {"utilization": 100.0, "resets_at": "2026-08-25T09:00:00Z"}}"#;
        let read = read_claude_usage("claude", body).expect("the document reads");
        assert_eq!(read.windows[0].used_percent, 99, "99.9% is not spent");
        assert_eq!(read.windows[1].used_percent, 100);
        assert!(read.limit_reached);
    }

    #[test]
    fn a_claude_window_with_no_reset_instant_is_dropped() {
        // `QuotaWindow` requires an instant; an allowance that cannot say when
        // it returns is an `Unknown` state rather than a window.
        let body = br#"{"seven_day": {"utilization": 100, "resets_at": null}}"#;
        let read = read_claude_usage("claude", body).expect("the document reads");
        assert!(read.windows.is_empty());
        assert!(!read.limit_reached, "no window means nothing to be spent");
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Available);
    }

    #[test]
    fn an_empty_claude_document_reads_as_available_rather_than_failing() {
        let read = read_claude_usage("claude", b"{}").expect("the document reads");
        assert!(read.windows.is_empty());
        assert_eq!(observe(&read).kind, ProviderQuotaKind::Available);
    }

    #[test]
    fn strict_claude_refuses_empty_model_only_and_incomplete_documents() {
        for body in [
            br#"{}"#.as_slice(),
            br#"{"seven_day_opus": {"utilization": 10,
                    "resets_at": "2026-08-25T09:00:00Z"}}"#,
            br#"{"five_hour": {"resets_at": "2026-08-22T18:00:00Z"}}"#,
        ] {
            assert_eq!(
                read_claude_usage_strict("claude", body)
                    .expect_err("incomplete account evidence is refused"),
                UsageFailure::Unreadable
            );
        }
    }

    #[test]
    fn strict_claude_keeps_a_spent_resetless_window_blocking() {
        let reading = read_claude_usage_strict(
            "claude",
            br#"{"seven_day": {"utilization": 100, "resets_at": null}}"#,
        )
        .expect("the spent fact is useful even without a reset");
        assert!(reading.limit_reached);
        assert!(reading.windows.is_empty());
        assert_eq!(observe(&reading).kind, ProviderQuotaKind::Unknown);
    }

    #[test]
    fn strict_claude_accepts_a_complete_available_control() {
        let reading = read_claude_usage_strict(
            "claude",
            br#"{"five_hour": {"utilization": 40,
                                 "resets_at": "2026-08-22T18:00:00Z"},
                    "seven_day": {"utilization": 61,
                                  "resets_at": "2026-08-25T09:00:00Z"}}"#,
        )
        .expect("the catalogued account-level shape reads");
        assert_eq!(observe(&reading).kind, ProviderQuotaKind::Available);
        assert_eq!(reading.windows.len(), 2);
    }
}
