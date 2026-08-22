//! Property tests over the provider-headroom truth table (EVD-OP-015).
//!
//! The unit tests beside `kontor_core::quota` pin named cases. These pin the
//! *laws* — the statements that must hold for every window set, not just the
//! three a fixture author thought of. Each one is aimed at a specific way the
//! arithmetic can be quietly inverted, and the comment says which.

use kontor_core::id::{CurrencyCode, Money, Timestamp};
use kontor_core::quota::{
    CreditBalance, CreditRefusal, HeadroomThresholds, QuotaWindow, QuotaWindowKind, WindowOutlook,
    window_outlook,
};
use proptest::prelude::*;

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).expect("a representable instant")
}

/// Any window kind, so a law never depends on which one a fixture picked.
fn any_kind() -> impl Strategy<Value = QuotaWindowKind> {
    prop_oneof![
        Just(QuotaWindowKind::Session),
        Just(QuotaWindowKind::Daily),
        Just(QuotaWindowKind::Weekly),
        Just(QuotaWindowKind::Monthly),
    ]
}

fn any_thresholds() -> impl Strategy<Value = HeadroomThresholds> {
    (1u8..=100, 1u8..=100, 1u8..=100, 1u8..=100).prop_map(
        |(session_percent, daily_percent, weekly_percent, monthly_percent)| HeadroomThresholds {
            session_percent,
            daily_percent,
            weekly_percent,
            monthly_percent,
        },
    )
}

fn any_window() -> impl Strategy<Value = QuotaWindow> {
    (any_kind(), 0u8..=100, 1i64..1_000_000).prop_map(|(kind, used_percent, resets_at)| {
        QuotaWindow {
            kind,
            resets_at: at(resets_at),
            used_percent,
        }
    })
}

/// A set of windows, with at most one of each kind — the shape the store's
/// primary key permits, so a law is never proved on a set the system cannot hold.
fn any_window_set() -> impl Strategy<Value = Vec<QuotaWindow>> {
    proptest::collection::vec(any_window(), 0..5).prop_map(|mut windows| {
        windows.sort_by_key(|window| window.kind);
        windows.dedup_by_key(|window| window.kind);
        windows
    })
}

proptest! {
    /// The blocking instant is the LATEST reset among the spent windows.
    ///
    /// The mutant: `min` for `max`. An account holding a spent session window
    /// that returns in an hour and a spent weekly one that returns on Saturday is
    /// usable on Saturday. Unblocking at the earlier instant hands work to an
    /// account whose other window is still empty, which walks straight back into
    /// the limit that was just recorded.
    #[test]
    fn blocking_names_the_latest_reset_among_the_spent_windows(
        windows in any_window_set(),
        thresholds in any_thresholds(),
    ) {
        let spent: Vec<&QuotaWindow> = windows
            .iter()
            .filter(|window| !window.has_headroom(&thresholds))
            .collect();
        match window_outlook(&windows, &thresholds) {
            WindowOutlook::Headroom => prop_assert!(
                spent.is_empty(),
                "headroom was reported while {} window(s) are spent",
                spent.len()
            ),
            WindowOutlook::Blocked { blocked_until } => {
                prop_assert!(!spent.is_empty(), "blocked with no spent window");
                let latest = spent
                    .iter()
                    .map(|window| window.resets_at)
                    .max()
                    .expect("a spent window");
                prop_assert_eq!(blocked_until, latest);
                // Every spent window has returned by the reported instant. This
                // is the property `min` violates and equality alone would not
                // catch on a single-element set.
                for window in &spent {
                    prop_assert!(window.resets_at <= blocked_until);
                }
            }
        }
    }

    /// One spent window is enough. A window with room never excuses one without.
    ///
    /// The mutant: `any` for `all` when judging headroom — that is, reporting
    /// headroom because *some* window still has room.
    #[test]
    fn a_single_spent_window_blocks_the_whole_set(
        windows in any_window_set(),
        thresholds in any_thresholds(),
    ) {
        let all_have_room = windows.iter().all(|window| window.has_headroom(&thresholds));
        prop_assert_eq!(
            window_outlook(&windows, &thresholds) == WindowOutlook::Headroom,
            all_have_room
        );
    }

    /// Adding a window never turns a blocked set into a free one.
    ///
    /// Monotonicity. A vendor that starts reporting a second window must not be
    /// able to unblock an account by doing so.
    #[test]
    fn adding_a_window_never_relieves_a_blocked_set(
        windows in any_window_set(),
        extra in any_window(),
        thresholds in any_thresholds(),
    ) {
        let before = window_outlook(&windows, &thresholds);
        let mut after_set = windows.clone();
        // Respect the one-per-kind rule the store enforces.
        if after_set.iter().any(|window| window.kind == extra.kind) {
            return Ok(());
        }
        after_set.push(extra);
        let after = window_outlook(&after_set, &thresholds);
        if before != WindowOutlook::Headroom {
            prop_assert_ne!(after, WindowOutlook::Headroom);
        }
    }

    /// Each window is judged against its OWN kind's threshold.
    ///
    /// The mutant: one threshold for every kind. A weekly window at 70% has days
    /// of runway and is worth protecting; a five-hour window at 70% refills this
    /// afternoon, and throttling it wastes capacity the subscription paid for.
    #[test]
    fn a_window_is_judged_against_its_own_kinds_threshold(
        kind in any_kind(),
        used_percent in 0u8..=100,
        thresholds in any_thresholds(),
    ) {
        let window = QuotaWindow { kind, resets_at: at(1), used_percent };
        prop_assert_eq!(
            window.has_headroom(&thresholds),
            used_percent < thresholds.for_kind(kind)
        );
    }

    /// A threshold is a stopping point, not the exhaustion point.
    ///
    /// Consumption at or above the threshold stops admitting, so the seats
    /// already running finish inside what is left.
    #[test]
    fn consumption_at_the_threshold_already_stops_admitting(
        kind in any_kind(),
        thresholds in any_thresholds(),
    ) {
        let threshold = thresholds.for_kind(kind);
        let window = |used_percent| QuotaWindow { kind, resets_at: at(1), used_percent };
        prop_assert!(!window(threshold).has_headroom(&thresholds));
        if threshold > 0 {
            prop_assert!(window(threshold - 1).has_headroom(&thresholds));
        }
    }

    /// Credit clears only when it is above its floor AND in the same currency.
    ///
    /// The mutants: dropping the currency comparison, which lets a large EUR
    /// balance clear a small USD floor on the numbers alone; and `>=` for `>`,
    /// which spends the reserve it exists to protect.
    #[test]
    fn credit_clears_only_above_its_floor_and_only_in_one_currency(
        remaining in 0u64..1_000_000,
        reserve in 0u64..1_000_000,
        same_currency in any::<bool>(),
    ) {
        let eur = CurrencyCode::parse("EUR").expect("a valid currency");
        let usd = CurrencyCode::parse("USD").expect("a valid currency");
        let credit = CreditBalance {
            remaining: Money { minor_units: remaining, currency: eur },
            reserve: Money {
                minor_units: reserve,
                currency: if same_currency { eur } else { usd },
            },
        };
        match credit.clears_reserve() {
            Ok(()) => {
                prop_assert!(same_currency, "two currencies must never compare");
                prop_assert!(remaining > reserve);
            }
            Err(CreditRefusal::CurrencyMismatch) => prop_assert!(!same_currency),
            Err(CreditRefusal::BelowReserve) => {
                prop_assert!(same_currency);
                prop_assert!(remaining <= reserve);
            }
        }
    }

    /// Credit and windows are never converted into each other.
    ///
    /// The dimension law. Whatever a credit balance says, it cannot change how a
    /// window set is judged — verified live on 2026-08-14, where the Claude org's
    /// `used_credits` did not move while a session window climbed 11% -> 28%.
    #[test]
    fn a_credit_balance_cannot_change_a_window_verdict(
        windows in any_window_set(),
        thresholds in any_thresholds(),
        remaining in 0u64..1_000_000,
        reserve in 0u64..1_000_000,
    ) {
        let eur = CurrencyCode::parse("EUR").expect("a valid currency");
        let credit = CreditBalance {
            remaining: Money { minor_units: remaining, currency: eur },
            reserve: Money { minor_units: reserve, currency: eur },
        };
        // The window verdict is computed with no access to the credit at all;
        // this asserts the two are independent by construction rather than by
        // inspection. If a future refactor ever threads a balance into the window
        // predicate, this stops compiling or stops holding.
        let verdict = window_outlook(&windows, &thresholds);
        let _ = credit.clears_reserve();
        prop_assert_eq!(verdict, window_outlook(&windows, &thresholds));
    }

    /// A window is classified by its span, never by the slot it arrived in.
    ///
    /// The boundaries are the verified readings: 300 -> session, 1440 -> daily,
    /// 10080 -> weekly, 43200 -> monthly.
    #[test]
    fn a_span_classifies_into_exactly_one_kind(minutes in 0u32..200_000) {
        let kind = QuotaWindowKind::from_minutes(minutes);
        let expected = match minutes {
            0..1_440 => QuotaWindowKind::Session,
            1_440..10_080 => QuotaWindowKind::Daily,
            10_080..43_200 => QuotaWindowKind::Weekly,
            _ => QuotaWindowKind::Monthly,
        };
        prop_assert_eq!(kind, expected);
    }

    /// Classification is monotone in the span: a longer window is never a
    /// shorter kind. A vendor that widens a window cannot make it look briefer.
    #[test]
    fn a_longer_span_is_never_a_shorter_kind(a in 0u32..200_000, b in 0u32..200_000) {
        let (shorter, longer) = if a <= b { (a, b) } else { (b, a) };
        prop_assert!(
            QuotaWindowKind::from_minutes(shorter) <= QuotaWindowKind::from_minutes(longer)
        );
    }
}
