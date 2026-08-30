//! Turning a provider's own refusal text into a typed quota state.
//!
//! # Why this is data and not code
//!
//! Every vendor words exhaustion differently, and each will reword it. Codex
//! says "You've hit your usage limit ... try again at Aug 23rd, 2026 9:35 AM";
//! a credit vendor answers 402 with "insufficient balance" and no instant at
//! all. Encoding those sentences as Rust constants means a rebuild to track a
//! copy change, so the sentences live in [`QuotaSignal`] values a deployment
//! supplies and this module only *applies* them.
//!
//! # Why there is no regular-expression engine here
//!
//! The workspace pins every dependency exactly and justifies each one; adding a
//! regex crate to classify a handful of fixed sentences is not a trade this
//! module needs to make. Marker substrings plus one delimited capture cover
//! every message shape observed so far, and the matcher stays auditable by
//! reading it.

use jiff::civil;
use jiff::tz::TimeZone;
use kontor_core::id::Timestamp;
use kontor_core::spec::ProviderQuotaKind;
use serde::{Deserialize, Serialize};

/// How a provider charges, which decides what an unresolved refusal means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuotaBasis {
    /// A plan allowance that returns at an instant.
    PlanAllowance,
    /// A prepaid balance that returns only when someone pays.
    CreditBalance,
}

/// One vendor's exhaustion wording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaSignal {
    /// The provider this wording belongs to, spelled as the catalog spells it.
    pub provider: String,
    /// How the provider charges.
    pub basis: QuotaBasis,
    /// Every marker must appear, case-insensitively, before the text is read as
    /// a quota refusal. Several short markers are deliberately safer than one
    /// long sentence: a vendor reflows wording far more often than it drops a
    /// distinctive phrase.
    pub markers: Vec<String>,
    /// The text immediately preceding a stated reset instant, when the vendor
    /// states one. Matched case-insensitively.
    #[serde(default)]
    pub reset_prefix: Option<String>,
    /// The IANA zone a bare wall-clock reset is stated in. `None` reads it as
    /// UTC.
    ///
    /// A vendor that prints local time without naming a zone cannot be read
    /// correctly without this, and guessing wrong shifts the reset by hours.
    /// Being *early* is the safe direction: the state stops blocking, a launch
    /// is placed on a provider that is still out, and the runtime's own
    /// provider-outage refusal sends it back — where the next observation
    /// records the instant again. Being late merely wastes the tail of an
    /// outage.
    #[serde(default)]
    pub reset_zone: Option<String>,
}

/// What one refusal text was read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedQuota {
    /// The provider the signal named.
    pub provider: String,
    /// The state to record.
    pub kind: ProviderQuotaKind,
    /// The instant an allowance returns, when one was stated and parsed.
    pub resets_at: Option<Timestamp>,
}

/// Read one runtime text against every configured signal.
///
/// Returns the first signal whose markers all appear. `None` means the text is
/// not a quota refusal at all — the overwhelmingly common case, since this is
/// asked about every error a seat reports.
///
/// The three outcomes, and why the third exists:
///
/// * a credit vendor yields [`ProviderQuotaKind::Drained`], which no clock lifts;
/// * a plan allowance with a parsable instant yields
///   [`ProviderQuotaKind::Exhausted`], which lifts itself at that instant;
/// * a plan allowance whose instant is absent or unparsable yields
///   [`ProviderQuotaKind::Unknown`], **not** `Drained`. Both block, but only
///   `Unknown` says "a provider refused and this row cannot say when it
///   returns", which is a visible prompt to fix the signal. Recording a plan
///   allowance as a drained balance would assert that money is the remedy.
#[must_use]
pub fn classify(text: &str, signals: &[QuotaSignal]) -> Option<ObservedQuota> {
    let haystack = text.to_lowercase();
    let signal = signals.iter().find(|signal| {
        !signal.markers.is_empty()
            && signal
                .markers
                .iter()
                .all(|marker| haystack.contains(&marker.to_lowercase()))
    })?;

    if signal.basis == QuotaBasis::CreditBalance {
        return Some(ObservedQuota {
            provider: signal.provider.clone(),
            kind: ProviderQuotaKind::Drained,
            resets_at: None,
        });
    }

    let resets_at = signal
        .reset_prefix
        .as_deref()
        .and_then(|prefix| after_prefix(text, &haystack, prefix))
        .and_then(|tail| parse_wall_clock(tail, signal.reset_zone.as_deref()));

    Some(match resets_at {
        Some(instant) => ObservedQuota {
            provider: signal.provider.clone(),
            kind: ProviderQuotaKind::Exhausted,
            resets_at: Some(instant),
        },
        None => ObservedQuota {
            provider: signal.provider.clone(),
            kind: ProviderQuotaKind::Unknown,
            resets_at: None,
        },
    })
}

/// The original-case remainder of `text` after the first case-insensitive hit of
/// `prefix`.
fn after_prefix<'a>(text: &'a str, haystack: &str, prefix: &str) -> Option<&'a str> {
    let at = haystack.find(&prefix.to_lowercase())?;
    text.get(at + prefix.len()..)
}

/// Parse a stated wall-clock instant such as `Aug 23rd, 2026 9:35 AM`.
///
/// Hand-rolled rather than handed to a format string because the shapes that
/// actually appear carry an ordinal suffix (`23rd`) and a 12-hour clock, and a
/// format string that covers those is less readable than the scan below — and
/// silently wrong when the vendor moves a comma.
fn parse_wall_clock(text: &str, zone: Option<&str>) -> Option<Timestamp> {
    let mut fields = text.split_whitespace();
    let month = month_number(fields.next()?)?;
    let day: i8 = strip_ordinal(fields.next()?.trim_end_matches(','))
        .parse()
        .ok()?;
    let year: i16 = fields.next()?.trim_end_matches(',').parse().ok()?;
    let (hour_text, minute_text) = fields.next()?.split_once(':')?;
    let mut hour: i8 = hour_text.parse().ok()?;
    let minute: i8 = minute_text.parse().ok()?;
    // A 12-hour clock only when the vendor prints a meridiem; 24-hour text
    // simply has no fourth field and must not be shifted.
    match fields.next().map(str::to_ascii_uppercase).as_deref() {
        Some("PM") if hour < 12 => hour += 12,
        Some("AM") if hour == 12 => hour = 0,
        _ => {}
    }
    let civil = civil::date(year, month, day).at(hour, minute, 0, 0);
    let zone = match zone {
        Some(name) => TimeZone::get(name).ok()?,
        None => TimeZone::UTC,
    };
    // An instant that does not exist locally (a spring-forward gap) or exists
    // twice (a fall-back overlap) is resolved by jiff's compatible rule rather
    // than refused: a reset an hour out is a far better answer than no reset at
    // all, which would record `Unknown` and block until a human intervened.
    zone.to_ambiguous_zoned(civil)
        .compatible()
        .ok()
        .map(|zoned| zoned.timestamp())
}

/// `23rd` -> `23`.
fn strip_ordinal(text: &str) -> &str {
    for suffix in ["st", "nd", "rd", "th"] {
        if let Some(stripped) = text.strip_suffix(suffix) {
            return stripped;
        }
    }
    text
}

/// A three-letter English month abbreviation, case-insensitively.
fn month_number(text: &str) -> Option<i8> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lowered = text.to_lowercase();
    let head = lowered.get(..3)?;
    MONTHS
        .iter()
        .position(|month| *month == head)
        .and_then(|index| i8::try_from(index + 1).ok())
}
