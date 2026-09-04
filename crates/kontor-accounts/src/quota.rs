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
use kontor_core::id::{CanonicalDocument, ContentHash, SpecVersion, Timestamp};
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
    /// Stable logical identity of this signal, unique within one document.
    ///
    /// Identity, not routing. Two logins of one vendor carry the same wording
    /// under the same provider family and are still two different signals, so a
    /// provenance record naming only the alias could not say which fingerprint
    /// actually fired. Held as a `String` for serde and validated through
    /// [`kontor_core::id::ExternalName::parse`], so the core rules -- including
    /// sensitive-material rejection -- stay one source of truth rather than
    /// four re-implemented checks.
    pub id: String,
    /// Which revision of that logical signal this is.
    ///
    /// A wording or parser change increments it. `SpecVersion` deserializes
    /// from a bare integer and refuses `0` at the serde boundary, so a
    /// zero-versioned document never reaches validation.
    pub version: SpecVersion,
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

impl QuotaSignal {
    /// A canonical digest of this signal's complete definition.
    ///
    /// Covers identity *and* content: `id`, `version`, `provider`, `basis`, the
    /// markers in their configured order, the reset prefix and the zone. So a
    /// reordered marker, an altered prefix or a changed zone under an unchanged
    /// `id` and `version` produces a different hash -- which is exactly the
    /// case a provenance record has to be able to refuse, because immutable
    /// history expects the definition it was written against.
    ///
    /// The hash is taken over the canonical document encoding of the validated
    /// strings themselves, never a separately normalized spelling, so what is
    /// digested is what the deployment wrote.
    ///
    /// # Errors
    /// Propagates canonicalization failure, which a validated signal does not
    /// produce.
    pub fn definition_hash(&self) -> kontor_core::DomainResult<ContentHash> {
        let document = CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "id": self.id,
            "version": self.version.get(),
            "provider": self.provider,
            "basis": serde_json::to_value(self.basis).unwrap_or(serde_json::Value::Null),
            "markers": self.markers,
            "reset_prefix": self.reset_prefix,
            "reset_zone": self.reset_zone,
        }))?;
        Ok(document.hash().clone())
    }
}

/// The identity of the signal a refusal matched.
///
/// Identity, not routing: two logins of one vendor carry the same wording, so a
/// record naming only the alias could not say which fingerprint fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedSignal {
    /// The signal's stable logical id.
    pub id: String,
    /// Which revision of it matched.
    pub version: SpecVersion,
    /// Digest of its complete definition at the moment it matched.
    pub definition_hash: ContentHash,
}

/// What one refusal text was read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedQuota {
    /// The provider the signal named.
    pub provider: String,
    /// Which signal matched, when a signal did.
    ///
    /// `None` for a structured provider report: the poller reads numbers from
    /// an endpoint and no fingerprint was involved, so claiming one would be
    /// evidence nobody produced.
    pub signal: Option<MatchedSignal>,
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
pub fn classify(
    text: &str,
    signals: &[QuotaSignal],
    observed_at: Timestamp,
) -> Option<ObservedQuota> {
    let signal = signals.iter().find(|signal| {
        !signal.markers.is_empty()
            && signal
                .markers
                .iter()
                .all(|marker| find_ascii_ci(text, marker).is_some())
    })?;

    let matched = MatchedSignal {
        id: signal.id.clone(),
        version: signal.version,
        definition_hash: signal.definition_hash().ok()?,
    };

    if signal.basis == QuotaBasis::CreditBalance {
        return Some(ObservedQuota {
            provider: signal.provider.clone(),
            signal: Some(matched.clone()),
            kind: ProviderQuotaKind::Drained,
            resets_at: None,
        });
    }

    let resets_at = signal
        .reset_prefix
        .as_deref()
        .and_then(|prefix| after_prefix(text, prefix))
        .and_then(|tail| {
            parse_wall_clock(tail, signal.reset_zone.as_deref())
                // A vendor that states only a time of day -- "your session
                // limit will reset at 10:40pm" -- is describing the next such
                // instant *from when it said so*. The basis is therefore the
                // provider item's own timestamp, never the moment Kontor
                // happened to read it: a probe running hours later would
                // otherwise roll the reset forward a whole day.
                .or_else(|| {
                    parse_time_of_day(text, tail, signal.reset_zone.as_deref(), observed_at)
                })
        });

    Some(match resets_at {
        Some(instant) => ObservedQuota {
            provider: signal.provider.clone(),
            signal: Some(matched.clone()),
            kind: ProviderQuotaKind::Exhausted,
            resets_at: Some(instant),
        },
        None => ObservedQuota {
            provider: signal.provider.clone(),
            signal: Some(matched.clone()),
            kind: ProviderQuotaKind::Unknown,
            resets_at: None,
        },
    })
}

/// Byte offset of the first ASCII-case-insensitive occurrence of `needle`.
///
/// # Why not `to_lowercase().find(..)`
///
/// Because that is what this function used to do, and it was wrong. Unicode
/// case mapping is not length-preserving — `İ` lowercases to two chars, and
/// `ǅ` to one of different width — so a byte offset found in the *lowercased*
/// copy does not address the same position in the original. Applying it to the
/// original sliced at a shifted, possibly non-boundary index: a wrong reset
/// instant on the good day and a panic-free `None` on the bad one, from a
/// vendor message that merely contained a non-ASCII character anywhere before
/// the prefix.
///
/// Markers and prefixes are validated ASCII at configuration load, so an
/// ASCII-case-insensitive scan over the original bytes is exact *and* returns
/// an offset that is always a real char boundary: every matched byte equals an
/// ASCII byte, so none of them is a continuation byte.
fn find_ascii_ci(text: &str, needle: &str) -> Option<usize> {
    let (hay, nee) = (text.as_bytes(), needle.as_bytes());
    if nee.is_empty() || nee.len() > hay.len() {
        return None;
    }
    (0..=hay.len() - nee.len())
        .find(|start| hay[*start..*start + nee.len()].eq_ignore_ascii_case(nee))
}

/// The original-case remainder of `text` after the first ASCII-case-insensitive
/// hit of `prefix`.
fn after_prefix<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let at = find_ascii_ci(text, prefix)?;
    // `at + prefix.len()` lands immediately after bytes that are all ASCII, so
    // it is a char boundary by construction.
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

/// Which half of a 12-hour clock a vendor printed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Meridiem {
    Am,
    Pm,
}

/// Parse a bare time of day into the next such instant at or after `basis`.
///
/// Accepts `10:40pm`, `10:40 PM` and 24-hour `22:40`. The date comes from
/// `basis` read in `zone`, and when the stated time has already passed on that
/// day the **next occurrence** is taken — a limit cannot reset in the past.
///
/// A spring-forward gap or a fall-back overlap is resolved by jiff's compatible
/// rule, as the dated parser does: a reset an hour out beats no reset at all,
/// which would record `Unknown` and block until a human intervened.
fn parse_time_of_day(
    message: &str,
    tail: &str,
    zone_name: Option<&str>,
    basis: Timestamp,
) -> Option<Timestamp> {
    let zone = match zone_name {
        Some(name) => TimeZone::get(name).ok()?,
        None => TimeZone::UTC,
    };
    // `10:40pm` arrives as one token, `10:40 PM` as two, and `22:40` with no
    // meridiem at all.
    //
    // A vendor may also restate its zone in trailing parentheses --
    // `10:40pm (Europe/Chisinau)`. That annotation is never part of the clock,
    // but it is not ignored either: silently dropping it would let a message
    // that says `(Europe/Oslo)` be converted as Chisinau and land an hour
    // wrong, with nothing to show for it.
    //
    // So when the annotation names a zone the tzdb knows, it must **agree**
    // with the declared one; a disagreement yields no instant at all, which
    // records a blocking `Unknown` and is the visible prompt to fix the
    // signal. An annotation that is not an IANA name -- an abbreviation like
    // `(EEST)` -- cannot be compared and is left alone rather than guessed at.
    // **Every** parenthesized token, not the first. Taking only the first let an
    // unrecognised annotation mask a later conflicting one -- `10:40pm (EEST)
    // (Europe/Oslo)` short-circuited on `(EEST)`, never compared `(Europe/Oslo)`,
    // and converted the clock as Chisinau anyway. This signal can retire a live
    // seat, so a disagreement anywhere in the *message* is a disagreement --
    // the whole message, not just the tail after the prefix, because a vendor
    // may state its zone before the reset phrase as easily as after it.
    for word in message.split_whitespace() {
        let Some(inner) = word.strip_prefix('(') else {
            continue;
        };
        let stated = inner.trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/');
        if TimeZone::get(stated).is_ok()
            && !zone_name.is_some_and(|declared| declared.eq_ignore_ascii_case(stated))
        {
            return None;
        }
    }
    let mut words = tail
        .split_whitespace()
        .filter(|word| !word.starts_with('('));
    // Trailing punctuation first: the vendor writes "…reset at 10:40pm." and a
    // meridiem suffix check against `10:40PM.` silently fails, reading the
    // clock as 10:40 and then rolling it to the next morning.
    let token = words
        .next()?
        .trim_end_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_ascii_uppercase();
    let (clock, mut meridiem) = if let Some(stripped) = token.strip_suffix("AM") {
        (stripped, Some(Meridiem::Am))
    } else if let Some(stripped) = token.strip_suffix("PM") {
        (stripped, Some(Meridiem::Pm))
    } else {
        (token.as_str(), None)
    };
    if meridiem.is_none() {
        meridiem = match words.next().map(str::to_ascii_uppercase) {
            Some(word) if word.starts_with("AM") => Some(Meridiem::Am),
            Some(word) if word.starts_with("PM") => Some(Meridiem::Pm),
            _ => None,
        };
    }
    let clock = clock.trim_end_matches(|c: char| !c.is_ascii_digit());
    let (hour_text, minute_text) = clock.split_once(':')?;
    let mut hour: i8 = hour_text.trim().parse().ok()?;
    let minute: i8 = minute_text.trim().parse().ok()?;
    match meridiem {
        // A 12-hour clock only when the vendor printed a meridiem; 24-hour text
        // simply has none and must not be shifted.
        Some(Meridiem::Pm) if hour < 12 => hour += 12,
        Some(Meridiem::Am) if hour == 12 => hour = 0,
        _ => {}
    }
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) {
        return None;
    }
    let local = basis.to_zoned(zone.clone()).date();
    for day in 0..=1 {
        let date = if day == 0 {
            local
        } else {
            local.tomorrow().ok()?
        };
        let candidate = zone
            .to_ambiguous_zoned(date.at(hour, minute, 0, 0))
            .compatible()
            .ok()?
            .timestamp();
        if candidate > basis {
            return Some(candidate);
        }
    }
    None
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
