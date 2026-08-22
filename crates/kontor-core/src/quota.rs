//! Provider quota headroom: concurrent windows, a depleting credit, and the
//! thresholds a deployment declares over them.
//!
//! # Why an account's usability is not one boolean
//!
//! A provider does not hold one allowance. On 2026-08-14 the Claude plan was
//! verified to expose a five-hour `session` window *and* a weekly one at the
//! same time, and a single `blocked_until` instant cannot describe two windows
//! that empty at different times. So an account carries a *set* of windows, and
//! the instant it is blocked until is the **latest** reset among the exhausted
//! ones — the earliest would unblock the account while a window it also needs is
//! still empty, which is how a scheduler walks straight back into the limit it
//! just recorded.
//!
//! # Why windows and credit never touch
//!
//! A *window* is subscription capacity that returns on a clock. *Credit* is
//! money that depletes and does not return until someone pays. They were
//! verified independent by sampling on 2026-08-14: the Claude org's
//! `used_credits` did not move at all while a session window climbed 11% → 28%.
//!
//! Two consequences are load-bearing here, and both are the opposite of what a
//! money-first design would do. Included windows are **free**, so spending one
//! to its limit is the goal rather than the risk. Credit is the guarded number,
//! so it has a reserve. Nothing in this module converts one dimension into the
//! other, and nothing compares two currencies: an `EUR` balance held against a
//! `USD` reserve is refused as unreadable, never rescaled.

use serde::{Deserialize, Serialize};

use crate::id::{Money, Timestamp};

// ---------------------------------------------------------------------------
// Window kinds
// ---------------------------------------------------------------------------

crate::closed_enum! {
    /// Which recurring allowance one window measures.
    ///
    /// Classified from the provider's own window *length* and never from the
    /// name of the field it arrived in. That rule is a correction, not a
    /// preference: the Codex payload carries its span as `window_minutes` beside
    /// keys named `primary` and `secondary`, and a reader that trusted those
    /// names recorded a weekly allowance as whatever `primary` happened to mean
    /// that quarter. The number is the fact; the key is the vendor's layout.
    QuotaWindowKind, "QuotaWindowKind" {
        /// Shorter than a day — the five-hour rolling window.
        Session => "session",
        /// About a day.
        Daily => "daily",
        /// About a week.
        Weekly => "weekly",
        /// A month or longer, including a billing cycle.
        Monthly => "monthly",
    }
}

impl QuotaWindowKind {
    /// Classify one window by the span the provider reported for it.
    ///
    /// The boundaries are inclusive-below, so the verified readings land where
    /// they should: 300 → [`Self::Session`], 1440 → [`Self::Daily`],
    /// 10080 → [`Self::Weekly`], 43200 → [`Self::Monthly`].
    #[must_use]
    pub const fn from_minutes(minutes: u32) -> Self {
        match minutes {
            0..1_440 => Self::Session,
            1_440..10_080 => Self::Daily,
            10_080..43_200 => Self::Weekly,
            _ => Self::Monthly,
        }
    }
}

// ---------------------------------------------------------------------------
// Thresholds
// ---------------------------------------------------------------------------

/// The consumption share, per window kind, at which an account stops accepting
/// **new** seats.
///
/// One number per kind rather than one number overall, because the kinds are not
/// interchangeable. A weekly window at 70% has days of runway and is worth
/// protecting; a five-hour window at 70% refills this afternoon and throttling
/// it wastes capacity the subscription already paid for.
///
/// Every field is a percentage of the window, and `100` means "spend it to the
/// limit" rather than "no limit" — there is no spelling here for unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadroomThresholds {
    /// Share of a session window that may be spent before admission stops.
    pub session_percent: u8,
    /// Share of a daily window.
    pub daily_percent: u8,
    /// Share of a weekly window.
    pub weekly_percent: u8,
    /// Share of a monthly window or billing cycle.
    pub monthly_percent: u8,
}

impl HeadroomThresholds {
    /// The threshold governing one window kind.
    #[must_use]
    pub const fn for_kind(&self, kind: QuotaWindowKind) -> u8 {
        match kind {
            QuotaWindowKind::Session => self.session_percent,
            QuotaWindowKind::Daily => self.daily_percent,
            QuotaWindowKind::Weekly => self.weekly_percent,
            QuotaWindowKind::Monthly => self.monthly_percent,
        }
    }

    /// Validate every threshold.
    ///
    /// # Errors
    /// Rejects a zero threshold, which would stop admission on an untouched
    /// window, and anything above 100, which is not a share of anything.
    pub fn validate(&self) -> crate::DomainResult<()> {
        for percent in [
            self.session_percent,
            self.daily_percent,
            self.weekly_percent,
            self.monthly_percent,
        ] {
            if percent == 0 || percent > 100 {
                return Err(crate::DomainError::invalid(
                    "HeadroomThresholds",
                    "every threshold must be a share between 1 and 100",
                ));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// One concurrent quota window on one `(account, provider)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaWindow {
    /// Which allowance this is.
    pub kind: QuotaWindowKind,
    /// When it refills. Always present: a window is a span with an end, and one
    /// that could not say when it returns is an [`crate::spec::ProviderQuotaKind::Unknown`]
    /// state rather than a window.
    pub resets_at: Timestamp,
    /// How much of it the provider reports consumed, as a percentage.
    pub used_percent: u8,
}

impl QuotaWindow {
    /// Whether this window still has room for a **new** seat at `thresholds`.
    ///
    /// Compared against the threshold and not against 100: the point of a
    /// threshold is to stop admitting before the window is gone, so the seats
    /// already running can finish inside what is left.
    #[must_use]
    pub const fn has_headroom(&self, thresholds: &HeadroomThresholds) -> bool {
        self.used_percent < thresholds.for_kind(self.kind)
    }
}

/// Where a set of windows stands against one deployment's thresholds.
///
/// A conclusion rather than a filter, because the blocking case has to carry the
/// instant it clears — and deriving that instant is the one arithmetic in this
/// module a mutant can quietly invert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowOutlook {
    /// Every window has room.
    Headroom,
    /// At least one window is spent, and this is when the **last** of the spent
    /// ones returns.
    Blocked {
        /// The latest reset among the exhausted windows.
        blocked_until: Timestamp,
    },
}

/// Judge a set of concurrent windows against one deployment's thresholds.
///
/// An empty set is [`WindowOutlook::Headroom`]: no window is not the same fact
/// as an empty one, and refusing on absence would stop every launch in a realm
/// whose collector has never run. Absence of *observation* is failed closed one
/// level up, where the observation state lives — not here, where absence of a
/// window genuinely means the provider has no such allowance.
#[must_use]
pub fn window_outlook(windows: &[QuotaWindow], thresholds: &HeadroomThresholds) -> WindowOutlook {
    // `max`, not `min`. An account holding a spent session window that returns
    // in an hour and a spent weekly one that returns on Saturday is usable on
    // Saturday, not in an hour: unblocking at the earlier instant hands work to
    // an account whose other window is still empty.
    windows
        .iter()
        .filter(|window| !window.has_headroom(thresholds))
        .map(|window| window.resets_at)
        .max()
        .map_or(WindowOutlook::Headroom, |blocked_until| {
            WindowOutlook::Blocked { blocked_until }
        })
}

// ---------------------------------------------------------------------------
// Credit
// ---------------------------------------------------------------------------

/// A depleting prepaid balance and the floor a deployment keeps under it.
///
/// This is the one place money is a control in Kontor. It is a property of the
/// account rather than of a task: capping what one task may spend cannot prevent
/// the exhaustion that actually halts work, while it can refuse work that would
/// have cost nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreditBalance {
    /// What is left.
    pub remaining: Money,
    /// The floor new work may not eat into.
    pub reserve: Money,
}

/// Why a credit balance does not clear its reserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditRefusal {
    /// The balance is at or under the floor.
    BelowReserve,
    /// The balance and the reserve are in different currencies.
    ///
    /// Refused rather than converted. A rate is a fact about a market at an
    /// instant, this module holds no rate and should hold none, and a scheduling
    /// decision taken through one would change with the market rather than with
    /// the account.
    CurrencyMismatch,
}

impl CreditBalance {
    /// Whether new work may draw on this balance.
    ///
    /// # Errors
    /// Returns [`CreditRefusal`] naming which of the two refusals applies, so a
    /// mismatch is visible as a configuration fault rather than reported as an
    /// empty wallet.
    pub fn clears_reserve(&self) -> Result<(), CreditRefusal> {
        // Compared as codes, never rescaled. Two currencies are not two numbers.
        if self.remaining.currency != self.reserve.currency {
            return Err(CreditRefusal::CurrencyMismatch);
        }
        if self.remaining.minor_units <= self.reserve.minor_units {
            return Err(CreditRefusal::BelowReserve);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::CurrencyCode;

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("a representable instant")
    }

    fn thresholds() -> HeadroomThresholds {
        HeadroomThresholds {
            session_percent: 90,
            daily_percent: 80,
            weekly_percent: 70,
            monthly_percent: 80,
        }
    }

    fn window(kind: QuotaWindowKind, used_percent: u8, resets_at: i64) -> QuotaWindow {
        QuotaWindow {
            kind,
            resets_at: at(resets_at),
            used_percent,
        }
    }

    fn money(minor_units: u64, currency: &str) -> Money {
        Money {
            minor_units,
            currency: CurrencyCode::parse(currency).expect("a valid currency"),
        }
    }

    #[test]
    fn a_window_is_classified_by_its_span_not_by_its_field_name() {
        assert_eq!(QuotaWindowKind::from_minutes(300), QuotaWindowKind::Session);
        assert_eq!(QuotaWindowKind::from_minutes(1_440), QuotaWindowKind::Daily);
        assert_eq!(
            QuotaWindowKind::from_minutes(10_080),
            QuotaWindowKind::Weekly
        );
        assert_eq!(
            QuotaWindowKind::from_minutes(43_200),
            QuotaWindowKind::Monthly
        );
    }

    #[test]
    fn two_exhausted_windows_block_until_the_later_reset() {
        let outlook = window_outlook(
            &[
                window(QuotaWindowKind::Session, 95, 1_000),
                window(QuotaWindowKind::Weekly, 99, 500_000),
            ],
            &thresholds(),
        );
        assert_eq!(
            outlook,
            WindowOutlook::Blocked {
                blocked_until: at(500_000)
            },
            "the earlier reset would unblock an account whose weekly window is still empty"
        );
    }

    #[test]
    fn one_exhausted_window_blocks_even_beside_a_window_with_room() {
        let outlook = window_outlook(
            &[
                window(QuotaWindowKind::Session, 1, 1_000),
                window(QuotaWindowKind::Weekly, 70, 500_000),
            ],
            &thresholds(),
        );
        assert_eq!(
            outlook,
            WindowOutlook::Blocked {
                blocked_until: at(500_000)
            }
        );
    }

    #[test]
    fn every_window_below_its_own_threshold_is_headroom() {
        let outlook = window_outlook(
            &[
                window(QuotaWindowKind::Session, 89, 1_000),
                window(QuotaWindowKind::Weekly, 69, 500_000),
            ],
            &thresholds(),
        );
        assert_eq!(outlook, WindowOutlook::Headroom);
    }

    #[test]
    fn thresholds_are_per_kind_and_not_one_number() {
        let same_share = thresholds();
        // 75% spends past the weekly threshold but not the session one.
        assert!(window(QuotaWindowKind::Session, 75, 1).has_headroom(&same_share));
        assert!(!window(QuotaWindowKind::Weekly, 75, 1).has_headroom(&same_share));
    }

    #[test]
    fn no_window_is_not_the_same_fact_as_an_empty_one() {
        assert_eq!(window_outlook(&[], &thresholds()), WindowOutlook::Headroom);
    }

    #[test]
    fn credit_above_its_reserve_clears_and_credit_at_the_floor_does_not() {
        assert_eq!(
            CreditBalance {
                remaining: money(20_000, "EUR"),
                reserve: money(10_000, "EUR"),
            }
            .clears_reserve(),
            Ok(())
        );
        assert_eq!(
            CreditBalance {
                remaining: money(10_000, "EUR"),
                reserve: money(10_000, "EUR"),
            }
            .clears_reserve(),
            Err(CreditRefusal::BelowReserve),
            "the floor is a floor, not a target to land on"
        );
    }

    #[test]
    fn two_currencies_are_refused_rather_than_converted() {
        // A large EUR balance against a small USD reserve would clear on the
        // numbers alone. Comparing them at all is the defect.
        assert_eq!(
            CreditBalance {
                remaining: money(50_000, "EUR"),
                reserve: money(100, "USD"),
            }
            .clears_reserve(),
            Err(CreditRefusal::CurrencyMismatch)
        );
    }

    #[test]
    fn a_threshold_of_zero_or_over_a_hundred_is_refused() {
        for bad in [0, 101] {
            let mut thresholds = thresholds();
            thresholds.weekly_percent = bad;
            assert!(thresholds.validate().is_err(), "{bad} is not a share");
        }
        assert!(thresholds().validate().is_ok());
    }
}
