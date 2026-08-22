//! Which account and which rung a launch takes, decided from provider headroom.
//!
//! # Account before rung
//!
//! Model chains are ordered by scarcity, not by price, so the two ways out of a
//! blocked route do not cost the same. Moving to a second account on the *same*
//! rung costs nothing — it is the same model, the same effort, the same quality
//! of output. Dropping a rung costs quality on every turn that follows. So the
//! walk exhausts the accounts eligible for the current rung before it takes the
//! next rung, and never the other way round.
//!
//! The consequence is that a chain declaring four rungs can actually reach the
//! fourth. Before this, selection took the first rung whose provider was clear
//! and fell back to the frozen primary, which meant rungs three and four were
//! decoration.
//!
//! # Waiting beats shipping worse work
//!
//! A rung whose accounts are all blocked is not automatically a reason to
//! descend. If the blocking window returns inside the declared short horizon,
//! the launch waits for it: minutes of delay on the pinned model is a better
//! trade than a whole task's output from a worse one. Only a reset beyond that
//! horizon justifies paying the quality cost.
//!
//! # This module admits; it never pre-empts
//!
//! Every threshold here gates the admission of a **new** seat. Nothing in
//! [`Placement`] can name a running seat, cancel one, or move one, because
//! moving a live seat discards the context it has built and restarts work that
//! was progressing. An account over its threshold simply stops accepting new
//! work while the seats already on it run to completion.
//!
//! # Total exhaustion is a wait, not an escalation
//!
//! When no eligible account on any rung has headroom, the work parks until the
//! earliest reset. That condition is predictable, self-clearing, and carries a
//! known instant — none of which is true of the situations a human is for.
//! A human is reached only when the earliest instant is beyond the declared
//! escalation horizon, or when nothing has an instant at all because every
//! refusal is one no clock lifts.

use std::collections::BTreeSet;

use kontor_core::id::{AccountProfileId, ExternalName, Timestamp};
use kontor_core::quota::HeadroomThresholds;
use kontor_core::repository::{ProviderHeadroom, ProviderQuotaState};
use kontor_core::spec::ModelRung;
use kontor_core::{DomainError, DomainResult};
use kontor_policy::{DeliberationStep, NeedsHumanPayload};
use serde::{Deserialize, Serialize};

/// Which pool a seat draws from, and therefore which thresholds bind it.
///
/// The vocabulary of topology node kinds — `ECP`, `TSW`, and the rest — belongs
/// to the pinned specification, so this crate deliberately does not hold a copy
/// of it. The caller reads the kind and states the consequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeatClass {
    /// An epic's own control seats — the ones hosted by its `ECP`.
    ///
    /// They are admitted against the full threshold, which is what the reserve
    /// below buys them. An epic whose architect and tracker cannot run is an
    /// epic that cannot reroute anything, so the seats that do the rerouting
    /// must not be the first ones starved by the seats they route.
    ControlPlane,
    /// A seat doing the work. Admitted against the threshold less the reserve.
    Delivery,
}

/// The headroom policy a deployment declares.
///
/// Every number is configured. There is no compiled default: a deployment that
/// wants the fleet policy's shape — 70% of a Codex weekly window, a five-hour
/// window spent to the limit — writes those numbers here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadroomConfig {
    /// Where admission stops, per window kind.
    pub thresholds: HeadroomThresholds,
    /// Percentage points held back from delivery seats and left for the epic's
    /// own control seats.
    pub control_plane_reserve_percent: u8,
    /// A blocking window returning within this span is waited for rather than
    /// descended around.
    pub short_horizon_seconds: i64,
    /// Beyond this span, total exhaustion stops being a wait and becomes a
    /// question for a human.
    pub escalation_horizon_seconds: i64,
}

impl HeadroomConfig {
    /// Validate the policy.
    ///
    /// # Errors
    /// Rejects thresholds outside 1..=100, a reserve that would leave a delivery
    /// seat no room at all, and a short horizon that is not inside the
    /// escalation horizon — a wait that outlasts the point where a human would
    /// have been asked is not a wait, it is a silent stall.
    pub fn validate(&self) -> DomainResult<()> {
        self.thresholds.validate()?;
        if self.short_horizon_seconds <= 0 || self.escalation_horizon_seconds <= 0 {
            return Err(DomainError::invalid(
                "HeadroomConfig",
                "both horizons must be positive spans",
            ));
        }
        if self.short_horizon_seconds > self.escalation_horizon_seconds {
            return Err(DomainError::invalid(
                "HeadroomConfig",
                "the short horizon must fall inside the escalation horizon",
            ));
        }
        let smallest = [
            self.thresholds.session_percent,
            self.thresholds.daily_percent,
            self.thresholds.weekly_percent,
            self.thresholds.monthly_percent,
        ]
        .into_iter()
        .min()
        .unwrap_or_default();
        if self.control_plane_reserve_percent >= smallest {
            return Err(DomainError::invalid(
                "HeadroomConfig",
                "the control-plane reserve must leave a delivery seat some headroom",
            ));
        }
        Ok(())
    }

    /// The thresholds one seat class is admitted against.
    ///
    /// The reserve is subtracted from the delivery class rather than added to
    /// the control class, so a deployment's declared threshold stays the real
    /// ceiling on the provider and the reserve only ever moves who reaches it
    /// first.
    #[must_use]
    pub fn thresholds_for(&self, seat: SeatClass) -> HeadroomThresholds {
        match seat {
            SeatClass::ControlPlane => self.thresholds,
            SeatClass::Delivery => HeadroomThresholds {
                session_percent: self.reserved(self.thresholds.session_percent),
                daily_percent: self.reserved(self.thresholds.daily_percent),
                weekly_percent: self.reserved(self.thresholds.weekly_percent),
                monthly_percent: self.reserved(self.thresholds.monthly_percent),
            },
        }
    }

    /// One threshold less the reserve, never below 1.
    ///
    /// Saturating at 1 rather than 0: a zero threshold would refuse a launch on
    /// an untouched window, which is a misconfiguration presenting itself as an
    /// outage. `validate` refuses the configuration that would reach here.
    fn reserved(&self, percent: u8) -> u8 {
        percent
            .saturating_sub(self.control_plane_reserve_percent)
            .max(1)
    }
    /// The policy a realm that has declared none is resolved under.
    ///
    /// Every threshold is 100 — "spend the window to its limit" — because that
    /// is what *no declared threshold* actually means, not a number this crate
    /// chose on a deployment's behalf. The reserve is zero for the same reason.
    /// Both horizons are one second, so a blocked rung descends immediately,
    /// which is the behaviour that shipped before this policy existed.
    ///
    /// The recorded provider state still blocks: absence of a *threshold* is not
    /// absence of the observation that a provider refused.
    #[must_use]
    pub const fn state_only() -> Self {
        Self {
            thresholds: HeadroomThresholds {
                session_percent: 100,
                daily_percent: 100,
                weekly_percent: 100,
                monthly_percent: 100,
            },
            control_plane_reserve_percent: 0,
            short_horizon_seconds: 1,
            escalation_horizon_seconds: 1,
        }
    }
}

/// Where a launch was placed, or why it was not.
///
/// Note what is absent: there is no variant naming an already-running seat.
/// Admission is the only thing decided here, so "pre-empt the seat that is
/// using the quota" is not a decision this type can express.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Launch on this pair.
    Admit {
        /// The rung to freeze.
        rung: ModelRung,
        /// The account to pin.
        account: AccountProfileId,
    },
    /// Nothing is admissible yet, but something will be at this instant.
    Wait {
        /// When the work may be reconsidered.
        until: Timestamp,
        /// Why it is waiting rather than running or escalating.
        reason: WaitReason,
    },
    /// No clock will resolve this inside the declared horizon.
    NeedsHuman {
        /// The earliest reset anything holds, when anything holds one at all.
        /// `None` means every refusal is one only money or an operator lifts.
        earliest_reset: Option<Timestamp>,
        /// What the operator is being asked to confirm, and what was already
        /// tried before they were asked.
        ///
        /// Carried in the variant rather than assembled by whoever handles it,
        /// so OP-REQ-036 is satisfied by the type: there is no way to express
        /// "escalate this without a recommendation" and then be refused for it.
        escalation: NeedsHumanPayload,
    },
}

/// The escalation one exhausted resolution hands a human.
///
/// The deliberation path is the walk itself. That is not a formality: the
/// operator's first question is "did it try the other account?", and the answer
/// is exactly the set of rungs and accounts this resolution consulted before it
/// ran out.
fn quota_escalation(
    rungs: usize,
    accounts: usize,
    earliest_reset: Option<Timestamp>,
) -> DomainResult<NeedsHumanPayload> {
    let recommendation = match earliest_reset {
        Some(instant) => format!(
            "Every eligible account is out of quota on all {rungs} declared rungs, and the \
             earliest reset is {instant}, which is beyond the declared escalation horizon. \
             Either widen the horizon and let the work park, register another account for one of \
             these rungs, or accept the delay deliberately."
        ),
        None => format!(
            "Every eligible account is refused on all {rungs} declared rungs by a state no clock \
             lifts — a drained balance, or a reserve that cannot be compared with its balance. \
             Top up the balance, correct the reserve's currency, or register another account. \
             Waiting will not clear this."
        ),
    };
    NeedsHumanPayload::new(
        ExternalName::parse(&recommendation)?,
        vec![DeliberationStep {
            role: ExternalName::parse("scheduler")?,
            consultation: ExternalName::parse("provider-headroom-resolution")?,
            // One resolution pass is one round. A second round is a second call,
            // after something about the accounts or the clock has changed.
            round: 1,
            outcome: ExternalName::parse(&format!(
                "walked {rungs} rungs across {accounts} eligible accounts; none admissible"
            ))?,
        }],
    )
}

/// Why a placement is waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitReason {
    /// A better rung's account returns inside the short horizon. Descending
    /// would buy minutes at the price of every turn that follows.
    NearReset,
    /// No eligible account on any rung has headroom.
    Exhausted,
}

/// One account a launch may be pinned to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EligibleAccount {
    /// The profile.
    pub account_profile_id: AccountProfileId,
    /// The providers this account can actually be *selected* for.
    ///
    /// This is the governed pin, not a wish: a runtime that cannot prove which
    /// account a run executed as makes the pin unverifiable, and an account
    /// listing no provider here is one this deployment cannot address
    /// per-provider at all. Leaving it out of the walk is the same refusal
    /// `account_env: false` makes at dispatch, taken early enough that nothing
    /// is queued which dispatch would have to throw away.
    pub selectable_providers: BTreeSet<String>,
}

/// Resolve one launch: which account, on which rung, or what to do instead.
///
/// The walk is deterministic. Accounts are taken in ascending profile-id order
/// within each rung, so the same inputs always place the same launch and a
/// replay of a plan selects what the plan selected.
///
/// `provider_enabled` is the deployment's own standing exclusion — an operator
/// who turned a provider off in settings, which no stored observation overrides.
///
/// # Errors
/// Returns [`DomainError`] for an empty chain, which a validated
/// [`kontor_core::spec::ModelChainPolicy`] cannot be.
pub fn resolve<F>(
    rungs: &[ModelRung],
    accounts: &[EligibleAccount],
    states: &[ProviderQuotaState],
    config: &HeadroomConfig,
    seat: SeatClass,
    now: Timestamp,
    provider_enabled: F,
) -> DomainResult<Placement>
where
    F: Fn(&str) -> bool,
{
    if rungs.is_empty() {
        return Err(DomainError::invalid(
            "ModelChainPolicy",
            "a model chain must declare at least one rung",
        ));
    }
    let thresholds = config.thresholds_for(seat);
    let mut ordered: Vec<&EligibleAccount> = accounts.iter().collect();
    ordered.sort_by_key(|account| account.account_profile_id);

    // Every reset seen anywhere in the walk, so total exhaustion can name the
    // earliest one without a second pass.
    let mut all_resets: Vec<Timestamp> = Vec::new();

    for rung in rungs {
        let provider = rung.provider.0.as_str();
        if !provider_enabled(provider) {
            continue;
        }
        // The resets blocking *this* rung specifically. Kept separate from
        // `all_resets`: the short-horizon wait is a statement about the rung the
        // launch would rather have, and a near reset three rungs down is no
        // reason to refuse a rung that is free right now.
        let mut rung_resets: Vec<Timestamp> = Vec::new();
        let mut had_candidate = false;

        for account in &ordered {
            if !account.selectable_providers.contains(provider) {
                continue;
            }
            had_candidate = true;
            match headroom_of(
                states,
                account.account_profile_id,
                provider,
                &thresholds,
                now,
            ) {
                // Account before rung: the first account with room takes the
                // launch, and no lower rung is consulted at all.
                ProviderHeadroom::Admissible => {
                    return Ok(Placement::Admit {
                        rung: rung.clone(),
                        account: account.account_profile_id,
                    });
                }
                ProviderHeadroom::Blocked { blocked_until } => {
                    rung_resets.push(blocked_until);
                    all_resets.push(blocked_until);
                }
                // No instant to record: a drained balance, or a currency pair
                // nobody can compare, is not something a timer resolves.
                ProviderHeadroom::Unavailable => {}
            }
        }

        // Wait for a near reset on a rung we would rather have than descend past
        // it. `min` here and `max` inside a single account's windows are
        // different questions: the account is usable once its *last* spent
        // window returns, and the rung is usable once its *first* account does.
        if had_candidate
            && let Some(soonest) = rung_resets.iter().copied().min()
            && within(now, soonest, config.short_horizon_seconds)
        {
            return Ok(Placement::Wait {
                until: soonest,
                reason: WaitReason::NearReset,
            });
        }
    }

    // Every rung walked and nothing admitted.
    match all_resets.iter().copied().min() {
        Some(earliest) if within(now, earliest, config.escalation_horizon_seconds) => {
            Ok(Placement::Wait {
                until: earliest,
                reason: WaitReason::Exhausted,
            })
        }
        earliest_reset => Ok(Placement::NeedsHuman {
            earliest_reset,
            escalation: quota_escalation(rungs.len(), ordered.len(), earliest_reset)?,
        }),
    }
}

/// One `(account, provider)` pair's standing, with absence permitting.
///
/// No row is not the same fact as
/// [`kontor_core::spec::ProviderQuotaKind::Unknown`], which is an explicit
/// refusal. Blocking on absence would stop every launch in a realm whose
/// collector has never run, which is every realm before its first observation.
fn headroom_of(
    states: &[ProviderQuotaState],
    account: AccountProfileId,
    provider: &str,
    thresholds: &HeadroomThresholds,
    now: Timestamp,
) -> ProviderHeadroom {
    states
        .iter()
        .find(|state| state.account_profile_id == account && state.provider == provider)
        .map_or(ProviderHeadroom::Admissible, |state| {
            state.headroom(thresholds, now)
        })
}

/// Whether `instant` falls within `span` seconds after `now`.
///
/// An instant already past is within every span: a reset the clock has run past
/// is not a wait at all, and reporting it as one would park work that could run.
fn within(now: Timestamp, instant: Timestamp, span_seconds: i64) -> bool {
    instant.as_second().saturating_sub(now.as_second()) <= span_seconds
}

#[cfg(test)]
mod tests {
    use kontor_core::id::{ContentHash, CurrencyCode, Money, ProjectId};
    use kontor_core::quota::{CreditBalance, QuotaWindow, QuotaWindowKind};
    use kontor_core::spec::{ModelRef, ProviderQuotaKind, ProviderQuotaSource, ProviderRef};

    use super::*;

    const NOW: i64 = 1_000_000;

    fn at(second: i64) -> Timestamp {
        Timestamp::from_second(second).expect("a representable instant")
    }

    fn now() -> Timestamp {
        at(NOW)
    }

    fn config() -> HeadroomConfig {
        HeadroomConfig {
            thresholds: kontor_core::quota::HeadroomThresholds {
                session_percent: 90,
                daily_percent: 85,
                weekly_percent: 70,
                monthly_percent: 80,
            },
            control_plane_reserve_percent: 10,
            short_horizon_seconds: 900,
            escalation_horizon_seconds: 7_200,
        }
    }

    /// Four rungs on three providers, so account-before-rung and fourth-rung
    /// reachability are both observable on one chain.
    fn chain() -> Vec<ModelRung> {
        ["codex", "claude", "openrouter", "deepseek"]
            .into_iter()
            .map(|provider| ModelRung {
                provider: ProviderRef(provider.to_owned()),
                model: ModelRef(format!("{provider}-model")),
                effort: None,
            })
            .collect()
    }

    fn account(byte: u8, providers: &[&str]) -> EligibleAccount {
        EligibleAccount {
            account_profile_id: profile(byte),
            selectable_providers: providers.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    /// Deterministic, ordered profile ids: `profile(1) < profile(2)`.
    fn profile(byte: u8) -> AccountProfileId {
        let text = format!("01890000-0000-7000-8000-0000000000{byte:02x}");
        AccountProfileId::parse(&text).expect("a valid profile id")
    }

    struct StateBuilder {
        state: ProviderQuotaState,
    }

    fn state(account: AccountProfileId, provider: &str, kind: ProviderQuotaKind) -> StateBuilder {
        StateBuilder {
            state: ProviderQuotaState {
                project_id: ProjectId::parse("01890000-0000-7000-8000-00000000f001")
                    .expect("a valid project id"),
                account_profile_id: account,
                provider: provider.to_owned(),
                state: kind,
                resets_at: None,
                windows: Vec::new(),
                credit: None,
                evidence_hash: ContentHash::of(b"evidence"),
                source: ProviderQuotaSource::RuntimeObservation,
                observed_at: now(),
                revision: kontor_core::id::AggregateRevision::INITIAL,
                updated_at: now(),
            },
        }
    }

    impl StateBuilder {
        fn resets_at(mut self, second: i64) -> Self {
            self.state.resets_at = Some(at(second));
            self
        }

        fn window(mut self, kind: QuotaWindowKind, used_percent: u8, resets_at: i64) -> Self {
            self.state.windows.push(QuotaWindow {
                kind,
                resets_at: at(resets_at),
                used_percent,
            });
            self
        }

        fn credit(mut self, remaining: u64, reserve: u64, currency: &str) -> Self {
            let code = CurrencyCode::parse(currency).expect("a valid currency");
            self.state.credit = Some(CreditBalance {
                remaining: Money {
                    minor_units: remaining,
                    currency: code,
                },
                reserve: Money {
                    minor_units: reserve,
                    currency: code,
                },
            });
            self
        }

        fn mixed_currency_credit(mut self, remaining: u64, reserve: u64) -> Self {
            self.state.credit = Some(CreditBalance {
                remaining: Money {
                    minor_units: remaining,
                    currency: CurrencyCode::parse("EUR").expect("a valid currency"),
                },
                reserve: Money {
                    minor_units: reserve,
                    currency: CurrencyCode::parse("USD").expect("a valid currency"),
                },
            });
            self
        }

        fn build(self) -> ProviderQuotaState {
            self.state
        }
    }

    /// Assert an escalation, and assert it is a *complete* one.
    ///
    /// Every `NeedsHuman` check goes through here, so no fixture can assert the
    /// instant while quietly accepting an escalation with nothing for an
    /// operator to act on.
    fn assert_needs_human(placement: &Placement, expected_reset: Option<Timestamp>) {
        let Placement::NeedsHuman {
            earliest_reset,
            escalation,
        } = placement
        else {
            panic!("expected an escalation, got {placement:?}");
        };
        assert_eq!(*earliest_reset, expected_reset);
        assert!(
            !escalation.recommended_resolution().as_str().is_empty(),
            "OP-REQ-036: an operator is owed the recommended resolution"
        );
        assert!(
            !escalation.tried_deliberation_path().is_empty(),
            "OP-REQ-036: an operator is owed what was already tried"
        );
    }

    fn place(
        accounts: &[EligibleAccount],
        states: &[ProviderQuotaState],
        seat: SeatClass,
    ) -> Placement {
        resolve(&chain(), accounts, states, &config(), seat, now(), |_| true)
            .expect("a non-empty chain")
    }

    // -----------------------------------------------------------------------
    // Account before rung
    // -----------------------------------------------------------------------

    #[test]
    fn a_second_account_on_the_same_rung_is_taken_before_the_next_rung() {
        // Account 1 is out of Codex; account 2 is not. Descending to Claude here
        // would pay a quality cost to avoid a move that costs nothing.
        let accounts = [account(1, &["codex", "claude"]), account(2, &["codex"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Exhausted)
            .resets_at(NOW + 500_000)
            .build()];
        assert_eq!(
            place(&accounts, &states, SeatClass::Delivery),
            Placement::Admit {
                rung: chain()[0].clone(),
                account: profile(2),
            },
            "the walk must exhaust a rung's accounts before descending"
        );
    }

    #[test]
    fn the_rung_descends_only_once_no_eligible_account_remains() {
        let accounts = [account(1, &["codex", "claude"]), account(2, &["codex"])];
        let states = [
            state(profile(1), "codex", ProviderQuotaKind::Exhausted)
                .resets_at(NOW + 500_000)
                .build(),
            state(profile(2), "codex", ProviderQuotaKind::Exhausted)
                .resets_at(NOW + 500_000)
                .build(),
        ];
        assert_eq!(
            place(&accounts, &states, SeatClass::Delivery),
            Placement::Admit {
                rung: chain()[1].clone(),
                account: profile(1),
            }
        );
    }

    #[test]
    fn a_chain_declaring_four_rungs_can_reach_the_fourth() {
        let accounts = [account(1, &["codex", "claude", "openrouter", "deepseek"])];
        let states = ["codex", "claude", "openrouter"]
            .map(|provider| {
                state(profile(1), provider, ProviderQuotaKind::Exhausted)
                    .resets_at(NOW + 500_000)
                    .build()
            })
            .to_vec();
        assert_eq!(
            place(&accounts, &states, SeatClass::Delivery),
            Placement::Admit {
                rung: chain()[3].clone(),
                account: profile(1),
            },
            "rungs three and four must not be decoration"
        );
    }

    #[test]
    fn selection_is_deterministic_regardless_of_the_order_accounts_arrive_in() {
        let states: [ProviderQuotaState; 0] = [];
        let ascending = [account(1, &["codex"]), account(2, &["codex"])];
        let descending = [account(2, &["codex"]), account(1, &["codex"])];
        assert_eq!(
            place(&ascending, &states, SeatClass::Delivery),
            place(&descending, &states, SeatClass::Delivery),
            "a replay of a plan must select what the plan selected"
        );
    }

    #[test]
    fn an_account_that_cannot_be_selected_for_a_provider_is_not_walked_for_it() {
        // The governed pin is what makes an account addressable per provider. An
        // account listing no provider is refused here rather than queued for a
        // dispatch that would refuse it.
        let accounts = [account(1, &[])];
        let states: [ProviderQuotaState; 0] = [];
        assert_needs_human(&place(&accounts, &states, SeatClass::Delivery), None);
    }

    // -----------------------------------------------------------------------
    // Windows
    // -----------------------------------------------------------------------

    #[test]
    fn an_account_holding_two_spent_windows_blocks_until_the_later_reset() {
        let accounts = [account(1, &["codex"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Available)
            .window(QuotaWindowKind::Session, 95, NOW + 60)
            .window(QuotaWindowKind::Weekly, 99, NOW + 500_000)
            .build()];
        // Only the weekly reset is beyond the escalation horizon, so the earlier
        // one would have produced a wait instead.
        assert_needs_human(
            &place(&accounts, &states, SeatClass::Delivery),
            Some(at(NOW + 500_000)),
        );
    }

    #[test]
    fn a_window_with_room_does_not_excuse_one_without() {
        let accounts = [account(1, &["codex"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Available)
            .window(QuotaWindowKind::Session, 1, NOW + 60)
            .window(QuotaWindowKind::Weekly, 99, NOW + 500_000)
            .build()];
        assert!(matches!(
            place(&accounts, &states, SeatClass::Delivery),
            Placement::NeedsHuman { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // Credit versus windows
    // -----------------------------------------------------------------------

    #[test]
    fn exhausted_credit_refuses_even_while_every_window_has_room() {
        let accounts = [account(1, &["codex"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Available)
            .window(QuotaWindowKind::Weekly, 1, NOW + 500_000)
            .credit(100, 100, "EUR")
            .build()];
        assert_needs_human(&place(&accounts, &states, SeatClass::Delivery), None);
    }

    #[test]
    fn a_spent_window_refuses_even_while_credit_is_far_above_its_reserve() {
        let accounts = [account(1, &["codex"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Available)
            .window(QuotaWindowKind::Weekly, 99, NOW + 500_000)
            .credit(9_999_999, 100, "EUR")
            .build()];
        assert_needs_human(
            &place(&accounts, &states, SeatClass::Delivery),
            Some(at(NOW + 500_000)),
        );
    }

    #[test]
    fn a_balance_and_a_reserve_in_two_currencies_are_refused_not_converted() {
        let accounts = [account(1, &["codex"])];
        // 50000 minor units against a 100 minor-unit floor clears on the numbers
        // alone. Comparing them at all is the defect.
        let states = [state(profile(1), "codex", ProviderQuotaKind::Available)
            .mixed_currency_credit(50_000, 100)
            .build()];
        assert_needs_human(&place(&accounts, &states, SeatClass::Delivery), None);
    }

    // -----------------------------------------------------------------------
    // The three observation states
    // -----------------------------------------------------------------------

    #[test]
    fn a_provider_that_cannot_report_headroom_is_used_rather_than_retired() {
        let accounts = [account(1, &["codex"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::CannotReport).build()];
        assert_eq!(
            place(&accounts, &states, SeatClass::Delivery),
            Placement::Admit {
                rung: chain()[0].clone(),
                account: profile(1),
            },
            "failing closed on a number the provider was never going to give retires it permanently"
        );
    }

    #[test]
    fn an_unreadable_refusal_still_fails_closed() {
        let accounts = [account(1, &["codex"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Unknown).build()];
        assert_needs_human(&place(&accounts, &states, SeatClass::Delivery), None);
    }

    #[test]
    fn no_recorded_state_at_all_permits_the_launch() {
        let accounts = [account(1, &["codex"])];
        let states: [ProviderQuotaState; 0] = [];
        assert!(matches!(
            place(&accounts, &states, SeatClass::Delivery),
            Placement::Admit { .. }
        ));
    }

    // -----------------------------------------------------------------------
    // Waiting versus descending
    // -----------------------------------------------------------------------

    #[test]
    fn a_window_returning_inside_the_short_horizon_is_waited_for_not_descended_around() {
        let accounts = [account(1, &["codex", "claude"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Exhausted)
            .resets_at(NOW + 300)
            .build()];
        assert_eq!(
            place(&accounts, &states, SeatClass::Delivery),
            Placement::Wait {
                until: at(NOW + 300),
                reason: WaitReason::NearReset,
            },
            "five minutes of delay beats a whole task on a worse model"
        );
    }

    #[test]
    fn a_reset_beyond_the_short_horizon_descends_instead_of_waiting() {
        let accounts = [account(1, &["codex", "claude"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Exhausted)
            .resets_at(NOW + 5_000)
            .build()];
        assert_eq!(
            place(&accounts, &states, SeatClass::Delivery),
            Placement::Admit {
                rung: chain()[1].clone(),
                account: profile(1),
            }
        );
    }

    // -----------------------------------------------------------------------
    // Park versus escalate
    // -----------------------------------------------------------------------

    #[test]
    fn total_exhaustion_parks_until_the_earliest_reset() {
        let accounts = [account(1, &["codex", "claude", "openrouter", "deepseek"])];
        let states = [
            ("codex", NOW + 3_000),
            ("claude", NOW + 2_000),
            ("openrouter", NOW + 4_000),
            ("deepseek", NOW + 5_000),
        ]
        .map(|(provider, resets)| {
            state(profile(1), provider, ProviderQuotaKind::Exhausted)
                .resets_at(resets)
                .build()
        })
        .to_vec();
        assert_eq!(
            place(&accounts, &states, SeatClass::Delivery),
            Placement::Wait {
                until: at(NOW + 2_000),
                reason: WaitReason::Exhausted,
            },
            "a predictable, self-clearing condition with a known instant is not a question for a human"
        );
    }

    #[test]
    fn a_reset_beyond_the_escalation_horizon_reaches_for_a_human_with_the_instant() {
        let accounts = [account(1, &["codex", "claude", "openrouter", "deepseek"])];
        let states = ["codex", "claude", "openrouter", "deepseek"]
            .map(|provider| {
                state(profile(1), provider, ProviderQuotaKind::Exhausted)
                    .resets_at(NOW + 200_000)
                    .build()
            })
            .to_vec();
        assert_needs_human(
            &place(&accounts, &states, SeatClass::Delivery),
            Some(at(NOW + 200_000)),
        );
    }

    #[test]
    fn a_refusal_no_clock_lifts_reaches_for_a_human_with_no_instant() {
        let accounts = [account(1, &["codex"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Drained).build()];
        assert_needs_human(&place(&accounts, &states, SeatClass::Delivery), None);
    }

    // -----------------------------------------------------------------------
    // The control-plane reserve
    // -----------------------------------------------------------------------

    #[test]
    fn the_reserve_keeps_a_control_seat_admissible_where_a_delivery_seat_is_not() {
        let accounts = [account(1, &["codex"])];
        // 65% is under the 70% weekly threshold but over the 60% a delivery seat
        // is held to once the reserve is taken out.
        let states = [state(profile(1), "codex", ProviderQuotaKind::Available)
            .window(QuotaWindowKind::Weekly, 65, NOW + 500_000)
            .build()];
        assert_eq!(
            place(&accounts, &states, SeatClass::ControlPlane),
            Placement::Admit {
                rung: chain()[0].clone(),
                account: profile(1),
            },
            "an epic whose control seats cannot run cannot reroute anything"
        );
        assert!(
            matches!(
                place(&accounts, &states, SeatClass::Delivery),
                Placement::NeedsHuman { .. }
            ),
            "a delivery seat must be the one that stops first"
        );
    }

    #[test]
    fn the_reserve_does_not_raise_the_ceiling_a_control_seat_is_held_to() {
        let accounts = [account(1, &["codex"])];
        let states = [state(profile(1), "codex", ProviderQuotaKind::Available)
            .window(QuotaWindowKind::Weekly, 70, NOW + 500_000)
            .build()];
        assert!(
            matches!(
                place(&accounts, &states, SeatClass::ControlPlane),
                Placement::NeedsHuman { .. }
            ),
            "the declared threshold stays the real ceiling; the reserve only moves who reaches it first"
        );
    }

    // -----------------------------------------------------------------------
    // Standing operator exclusion, and configuration
    // -----------------------------------------------------------------------

    #[test]
    fn a_provider_the_deployment_disabled_is_skipped_whatever_the_rows_say() {
        let accounts = [account(1, &["codex", "claude"])];
        let states: [ProviderQuotaState; 0] = [];
        let placement = resolve(
            &chain(),
            &accounts,
            &states,
            &config(),
            SeatClass::Delivery,
            now(),
            |provider| provider != "codex",
        )
        .expect("a non-empty chain");
        assert_eq!(
            placement,
            Placement::Admit {
                rung: chain()[1].clone(),
                account: profile(1),
            }
        );
    }

    /// Admission is the only decision here, and it is the only decision this
    /// type can express.
    ///
    /// The mutant this kills is not a line of arithmetic — it is a *feature*
    /// somebody adds: "the account is over threshold, so stop the seat that is
    /// using it." Moving a live seat discards the context it has built and
    /// restarts work that was progressing, so an account over its threshold must
    /// simply stop taking new work while the seats already on it finish.
    ///
    /// The guarantee is structural. Every outcome is an instruction about a
    /// launch that has not happened; none of them names a run, a seat, a session
    /// or a binding, so there is no way to spell "cancel that one" here at all.
    #[test]
    fn resolution_can_never_name_a_running_seat_let_alone_pre_empt_one() {
        let accounts = [account(1, &["codex"])];
        // An account far past every threshold, with live work implied.
        let states = [state(profile(1), "codex", ProviderQuotaKind::Available)
            .window(QuotaWindowKind::Weekly, 100, NOW + 500_000)
            .build()];

        // Whatever the pressure, the answer is about the next launch only.
        for seat in [SeatClass::Delivery, SeatClass::ControlPlane] {
            match place(&accounts, &states, seat) {
                Placement::Admit { rung, account } => {
                    // A placement names a route and an account to *start* on.
                    assert!(!rung.provider.0.is_empty());
                    assert_eq!(account, profile(1));
                }
                Placement::Wait { .. } | Placement::NeedsHuman { .. } => {}
            }
        }

        // And the refusal at full consumption is a refusal to admit, carrying
        // the instant the account frees up — not an instruction to reclaim it.
        assert_needs_human(
            &place(&accounts, &states, SeatClass::Delivery),
            Some(at(NOW + 500_000)),
        );
    }

    /// A seat already running is unaffected by the threshold that stopped the
    /// next one, at every level of consumption.
    ///
    /// Stated as a sweep rather than one case because "never pre-empts" has to
    /// hold at the boundary too: 69, 70 and 71 percent of a 70-percent threshold
    /// must all leave running work alone, and only the *admission* answer moves.
    #[test]
    fn crossing_a_threshold_changes_only_whether_a_new_seat_is_admitted() {
        let accounts = [account(1, &["codex"])];
        let mut admitted = Vec::new();
        for used in [58_u8, 59, 60, 61] {
            let states = [state(profile(1), "codex", ProviderQuotaKind::Available)
                .window(QuotaWindowKind::Weekly, used, NOW + 500_000)
                .build()];
            admitted.push(matches!(
                place(&accounts, &states, SeatClass::Delivery),
                Placement::Admit { .. }
            ));
        }
        // The delivery threshold is 70 less the 10-point reserve, so admission
        // stops at 60. Nothing else about the account changes across that line.
        assert_eq!(
            admitted,
            vec![true, true, false, false],
            "the threshold is a line about admission and about nothing else"
        );
    }

    #[test]
    fn an_empty_chain_is_refused_rather_than_silently_placed() {
        assert!(
            resolve(
                &[],
                &[account(1, &["codex"])],
                &[],
                &config(),
                SeatClass::Delivery,
                now(),
                |_| true,
            )
            .is_err()
        );
    }

    #[test]
    fn a_reserve_that_would_starve_delivery_seats_is_refused() {
        let mut config = config();
        config.control_plane_reserve_percent = 70;
        assert!(
            config.validate().is_err(),
            "a reserve at or above the smallest threshold leaves delivery no room"
        );
    }

    #[test]
    fn a_short_horizon_outlasting_the_escalation_horizon_is_refused() {
        let mut stretched = config();
        stretched.short_horizon_seconds = stretched.escalation_horizon_seconds + 1;
        assert!(stretched.validate().is_err());
        assert!(config().validate().is_ok(), "the fixture must be valid");
    }
}
