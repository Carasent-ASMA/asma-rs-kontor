//! The adaptive admission transition.
//!
//! `kontor-scheduler` owns capacity *arithmetic* — how wide a window may be,
//! how it clamps, what one growth step adds. This module owns the *transition*:
//! which observation moves the window, and when. The split matters because the
//! arithmetic is a pure function of the configuration while the transition is a
//! fact about evidence this Realm has actually collected, and only one of those
//! belongs next to the accounts the evidence came from.
//!
//! # The rule, in one place
//!
//! ```text
//! same observation id       -> unchanged, including the streak
//! pressure                  -> width = floor, streak = 0
//! first distinct clean      -> width unchanged, streak = 1
//! second distinct clean     -> width = min(width + growth_step, ceiling), streak = 0
//! ```
//!
//! Two clean observations rather than one, deliberately. A single clean reading
//! is one sample of a provider that was quiet for a moment; growing on it makes
//! the window oscillate against exactly the throttling it is meant to back away
//! from. The streak is the memory that turns one sample into a trend, and it is
//! persisted rather than held in a process so a restart does not silently
//! restart the trend.
//!
//! Replay is unchanged rather than idempotent-by-luck. The last observation id
//! is compared before anything moves, so folding the same collector reading
//! twice — a retried refresh, a replayed key — cannot widen the window twice.

use kontor_core::id::ExternalId;
use kontor_scheduler::model::{AdaptiveWindow, AdaptiveWindowConfig, CapacityObservation};

/// One MiniProject's adaptive admission position.
///
/// The same three fields the store persists, without the identity and revision
/// the repository adds — so this module can be exercised without a database and
/// cannot accidentally decide who it is deciding about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdaptivePosition {
    /// Maximum new admissions in one scheduling pass.
    pub current_window: u32,
    /// Distinct clean observations accumulated at this width.
    pub clean_observation_streak: u32,
    /// The observation already folded in, if any.
    pub last_observation_id: Option<ExternalId>,
}

impl AdaptivePosition {
    /// The position a freshly pinned epic starts at.
    #[must_use]
    pub fn initial(config: AdaptiveWindowConfig) -> Self {
        Self {
            current_window: AdaptiveWindow::start(config).current(),
            clean_observation_streak: 0,
            last_observation_id: None,
        }
    }

    /// The window this position puts in force, clamped into the configured band.
    ///
    /// Clamping is [`AdaptiveWindow::restore`]'s, not a second copy: a
    /// configuration whose ceiling narrowed must take effect on the next pass
    /// rather than fail it.
    #[must_use]
    pub fn window(&self, config: AdaptiveWindowConfig) -> AdaptiveWindow {
        AdaptiveWindow::restore(config, self.current_window)
    }
}

/// Fold one collector observation into a persisted position.
///
/// Returns the position that should be persisted. It is equal to `current` when
/// the observation has already been applied, which is what a caller checks to
/// decide whether a write is owed at all.
#[must_use]
pub fn fold(
    config: AdaptiveWindowConfig,
    current: &AdaptivePosition,
    observation_id: &ExternalId,
    observation: CapacityObservation,
) -> AdaptivePosition {
    if current.last_observation_id.as_ref() == Some(observation_id) {
        return current.clone();
    }
    let restored = current.window(config);
    match observation {
        // Pressure narrows immediately and forgets the trend. `observe` already
        // knows that pressure means the floor; the streak reset is this
        // module's, because the streak is not something a window has.
        CapacityObservation::Pressure => AdaptivePosition {
            current_window: restored.observe(config, observation).current(),
            clean_observation_streak: 0,
            last_observation_id: Some(observation_id.clone()),
        },
        // The first clean reading is remembered, not acted on.
        CapacityObservation::Clean if current.clean_observation_streak == 0 => AdaptivePosition {
            current_window: restored.current(),
            clean_observation_streak: 1,
            last_observation_id: Some(observation_id.clone()),
        },
        // The second grows the window by exactly one step and starts again.
        CapacityObservation::Clean => AdaptivePosition {
            current_window: restored.observe(config, observation).current(),
            clean_observation_streak: 0,
            last_observation_id: Some(observation_id.clone()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: AdaptiveWindowConfig = AdaptiveWindowConfig {
        initial: 4,
        floor: 1,
        ceiling: 7,
        growth_step: 1,
    };

    fn id(text: &str) -> ExternalId {
        ExternalId::parse(text).expect("a valid external id")
    }

    #[test]
    fn a_fresh_position_starts_at_the_configured_width_with_no_trend() {
        let position = AdaptivePosition::initial(CONFIG);
        assert_eq!(position.current_window, 4);
        assert_eq!(position.clean_observation_streak, 0);
        assert_eq!(position.last_observation_id, None);
    }

    #[test]
    fn one_clean_observation_remembers_the_trend_and_does_not_grow() {
        let position = fold(
            CONFIG,
            &AdaptivePosition::initial(CONFIG),
            &id("obs-1"),
            CapacityObservation::Clean,
        );
        assert_eq!(position.current_window, 4, "one sample is not a trend");
        assert_eq!(position.clean_observation_streak, 1);
    }

    #[test]
    fn the_second_distinct_clean_observation_grows_by_exactly_one_step() {
        let first = fold(
            CONFIG,
            &AdaptivePosition::initial(CONFIG),
            &id("obs-1"),
            CapacityObservation::Clean,
        );
        let second = fold(CONFIG, &first, &id("obs-2"), CapacityObservation::Clean);
        assert_eq!(second.current_window, 5);
        assert_eq!(second.clean_observation_streak, 0, "the trend starts again");
    }

    #[test]
    fn replaying_one_observation_changes_nothing_at_all() {
        let first = fold(
            CONFIG,
            &AdaptivePosition::initial(CONFIG),
            &id("obs-1"),
            CapacityObservation::Clean,
        );
        let replayed = fold(CONFIG, &first, &id("obs-1"), CapacityObservation::Clean);
        assert_eq!(
            replayed, first,
            "a replayed reading must not advance the streak or the width"
        );

        // And specifically: a replay cannot stand in for the second distinct
        // observation that growth requires.
        assert_eq!(replayed.current_window, 4);
    }

    #[test]
    fn pressure_narrows_to_the_floor_and_forgets_the_trend() {
        let primed = fold(
            CONFIG,
            &AdaptivePosition::initial(CONFIG),
            &id("obs-1"),
            CapacityObservation::Clean,
        );
        let pressured = fold(CONFIG, &primed, &id("obs-2"), CapacityObservation::Pressure);
        assert_eq!(pressured.current_window, CONFIG.floor);
        assert_eq!(pressured.clean_observation_streak, 0);
    }

    #[test]
    fn growth_stops_at_the_ceiling_however_many_clean_pairs_arrive() {
        let mut position = AdaptivePosition::initial(CONFIG);
        for index in 0..40 {
            position = fold(
                CONFIG,
                &position,
                &id(&format!("obs-{index}")),
                CapacityObservation::Clean,
            );
        }
        assert_eq!(position.current_window, CONFIG.ceiling);
    }

    #[test]
    fn a_narrowed_ceiling_takes_effect_on_the_next_pass_rather_than_failing_it() {
        let wide = AdaptivePosition {
            current_window: 7,
            clean_observation_streak: 0,
            last_observation_id: None,
        };
        let narrowed = AdaptiveWindowConfig {
            ceiling: 3,
            ..CONFIG
        };
        assert_eq!(wide.window(narrowed).current(), 3);
    }
}
