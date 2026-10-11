//! Durable desk seats: dated declarations and the typed reconciliation decision.
//!
//! A durable desk (ASMA-8450) is a project-level native project that no epic
//! owns. This module describes the long-lived seats an operator decided such a
//! desk should hold, and the one question reconciliation is allowed to ask
//! about each of them: is this seat *positively* missing, and is it Kontor's to
//! restart?
//!
//! Two rules shape everything here.
//!
//! 1. **Absence must be proved.** A runtime that could not be reached, a census
//!    that failed, a busy or archived native and a launch whose answer was lost
//!    are all different from a missing seat. Each has its own status, and only
//!    [`DurableSeatStatus::Missing`] can ever produce a launch.
//! 2. **Recovery has one owner.** A seat declared
//!    [`DurableSeatRecoveryOwner::PaseoDirect`] is run outside Kontor. The
//!    daemon reports it as unchecked without asking any runtime about it and
//!    never adopts, launches or restarts it.
//!
//! The module holds no store, runtime or clock. [`classify`] and [`decide`]
//! are pure functions over facts the daemon has already read, so the restart
//! decision is typed state that a test can examine without a runtime.

use std::collections::BTreeSet;

use jiff::civil;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::id::{DeskKey, DurableSeatKey, ExternalId, ExternalName, ProjectId, RoleCode};
use crate::id::{SeatBindingId, Timestamp};
use crate::naming::{
    NameSeparator, NativeNameSegment, NativeNameTemplate, NativeNameToken, NativeNameValues,
};
use crate::spec::ModelRung;
use crate::{DomainError, DomainResult};

closed_enum! {
    /// Who may restart a declared durable seat.
    ///
    /// There is no default. A declaration names its owner explicitly, so a
    /// seat that is already running outside Kontor can never become the
    /// daemon's by omission.
    DurableSeatRecoveryOwner, "DurableSeatRecoveryOwner" {
        /// Kontor launches, binds and restarts this seat in its desk.
        Daemon => "daemon",
        /// The seat runs outside Kontor. Kontor records the declaration and
        /// reports it, and never adopts, launches or restarts it.
        PaseoDirect => "paseo_direct",
    }
}

/// One dated decision that a desk should hold one long-lived seat.
///
/// Immutable once stored. Re-declaring the identical seat is a no-op;
/// declaring something different under the same key is a conflict, because
/// amending a decision is a new decision that this revision cannot record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableSeatDeclaration {
    /// Owning project.
    pub project_id: ProjectId,
    /// The desk the seat belongs to.
    pub desk_key: DeskKey,
    /// The seat's identity inside its desk.
    pub seat_key: DurableSeatKey,
    /// The exact runtime-facing title, `ROLE_CODE` and `TOPIC` joined by the
    /// desk's pinned separator.
    pub title: ExternalName,
    /// Registered role code the seat is opened under.
    pub role_code: RoleCode,
    /// The value of the title's `TOPIC` segment.
    pub seat_label: ExternalName,
    /// The exact route a daemon-owned seat is launched under. Optional for a
    /// Paseo-direct seat, whose route is not Kontor's to choose.
    pub model_rung: Option<ModelRung>,
    /// Repository-relative procedure the seat follows, recorded as evidence.
    /// Kontor never reads or runs it.
    pub procedure: Option<ExternalName>,
    /// Who may restart the seat.
    pub recovery_owner: DurableSeatRecoveryOwner,
    /// The person who decided the seat should exist.
    pub decided_by: ExternalName,
    /// The civil date of that decision.
    pub decided_on: civil::Date,
    /// The logical seat a daemon-owned declaration will be bound through,
    /// planned when it is declared. Its id is the runtime correlation label,
    /// so every later attempt addresses the same seat. A Paseo-direct
    /// declaration has none.
    pub seat_binding_id: Option<SeatBindingId>,
    /// When the declaration was recorded.
    pub declared_at: Timestamp,
}

impl DurableSeatDeclaration {
    /// The title the declared role code and label render to.
    ///
    /// # Errors
    /// Refuses a label carrying a separator glyph, as every native name does.
    pub fn expected_title(&self, separator: &NameSeparator) -> DomainResult<ExternalName> {
        render_durable_seat_title(separator, &self.role_code, &self.seat_label)
    }

    /// Prove the declaration is internally consistent.
    ///
    /// # Errors
    /// Refuses a title that is not exactly the rendered role code and label, a
    /// daemon-owned seat without a route or planned binding, a Paseo-direct
    /// seat carrying a binding, a decision dated more than one day after it was
    /// recorded, and a procedure that is not a plain relative path.
    pub fn validate(&self, separator: &NameSeparator) -> DomainResult<()> {
        if self.expected_title(separator)? != self.title {
            return Err(DomainError::invalid(
                "DurableSeatDeclaration",
                "the title must be the role code and label joined by the desk's separator",
            ));
        }
        match self.recovery_owner {
            DurableSeatRecoveryOwner::Daemon => {
                if self.model_rung.is_none() {
                    return Err(DomainError::invalid(
                        "DurableSeatDeclaration",
                        "a daemon-owned seat must declare the exact route it is launched under",
                    ));
                }
                if self.seat_binding_id.is_none() {
                    return Err(DomainError::invalid(
                        "DurableSeatDeclaration",
                        "a daemon-owned seat must plan its seat binding",
                    ));
                }
            }
            DurableSeatRecoveryOwner::PaseoDirect => {
                if self.seat_binding_id.is_some() {
                    return Err(DomainError::invalid(
                        "DurableSeatDeclaration",
                        "a Paseo-direct seat is never bound by Kontor",
                    ));
                }
            }
        }
        if let Some(rung) = &self.model_rung {
            rung.validate()?;
        }
        if let Some(procedure) = &self.procedure {
            validate_procedure(procedure.as_str())?;
        }
        let recorded_on = self.declared_at.to_zoned(TimeZone::UTC).date();
        // One day of slack, because the decision was dated in the decider's
        // own calendar and the record is dated in UTC.
        if self.decided_on > recorded_on.tomorrow().unwrap_or(recorded_on) {
            return Err(DomainError::invalid(
                "DurableSeatDeclaration",
                "a decision cannot be dated after the day it was recorded",
            ));
        }
        Ok(())
    }

    /// Whether `other` declares the same seat, ignoring only when it was
    /// recorded and which binding it planned.
    ///
    /// A retry of a declaration whose answer was lost mints a fresh binding id
    /// and instant; it is still the same decision.
    #[must_use]
    pub fn declares_same_seat(&self, other: &Self) -> bool {
        self.project_id == other.project_id
            && self.desk_key == other.desk_key
            && self.seat_key == other.seat_key
            && self.title == other.title
            && self.role_code == other.role_code
            && self.seat_label == other.seat_label
            && self.model_rung == other.model_rung
            && self.procedure == other.procedure
            && self.recovery_owner == other.recovery_owner
            && self.decided_by == other.decided_by
            && self.decided_on == other.decided_on
    }
}

/// Render a durable seat title: `ROLE_CODE`, then `TOPIC`, joined by the
/// desk's pinned separator. `LSA • GENERAL` is the label `GENERAL` on an `LSA`
/// seat under the default separator.
///
/// # Errors
/// Refuses a label carrying a separator glyph.
pub fn render_durable_seat_title(
    separator: &NameSeparator,
    role_code: &RoleCode,
    seat_label: &ExternalName,
) -> DomainResult<ExternalName> {
    NativeNameTemplate::from_segments(vec![
        NativeNameSegment::Token(NativeNameToken::RoleCode),
        NativeNameSegment::Token(NativeNameToken::Topic),
    ])?
    .render(
        separator,
        &NativeNameValues::new()
            .with_role_code(role_code.as_str())
            .with_topic(seat_label.as_str()),
    )
}

fn validate_procedure(text: &str) -> DomainResult<()> {
    let plain = !text.starts_with('/')
        && !text.contains('\\')
        && text
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..");
    if plain {
        Ok(())
    } else {
        Err(DomainError::invalid(
            "DurableSeatDeclaration",
            "the procedure must be a plain repository-relative path",
        ))
    }
}

closed_enum! {
    /// What a live native carrying a seat's binding is doing.
    DurableSeatActivity, "DurableSeatActivity" {
        /// Live and waiting for work.
        Idle => "idle",
        /// Starting up or running a turn.
        Working => "working",
        /// Blocked on a permission only a person can grant.
        AwaitingPermission => "awaiting_permission",
        /// Live, but its runtime reports an error.
        Errored => "errored",
        /// Live, in a state the runtime did not name.
        Unknown => "unknown",
    }
}

/// One native the runtime reports as carrying a seat's binding label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableSeatCarrier {
    /// Exact native identity.
    pub native_id: ExternalId,
    /// Whether it exists only as archive evidence.
    pub archived: bool,
    /// What it is doing. Meaningless for an archived native.
    pub activity: DurableSeatActivity,
}

/// What a runtime said when asked which natives carry one seat's binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurableSeatCensus {
    /// No runtime was asked. A Paseo-direct seat is never censused.
    NotTaken,
    /// No runtime able to answer is configured or it does not support the
    /// question.
    Unavailable,
    /// The runtime was asked and gave no authoritative answer.
    Failed,
    /// The runtime answered, and the answer does not correlate with what
    /// Kontor recorded.
    Uncorrelated,
    /// An authoritative census, archived natives included.
    Taken(Vec<DurableSeatCarrier>),
}

/// The state of a declaration's planned seat binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableSeatBindingFact {
    /// Not created yet. Nothing has ever been launched for it.
    Absent,
    /// Created and active.
    Active,
    /// Created and since retired or archived. Kontor will not reopen it.
    Retired,
}

/// Everything [`classify`] needs, read before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableSeatFacts {
    /// Who may restart the seat.
    pub owner: DurableSeatRecoveryOwner,
    /// The planned binding's state.
    pub binding: DurableSeatBindingFact,
    /// The native Kontor recorded as the current occupant, when it has one.
    pub occupancy: Option<ExternalId>,
    /// Natives already moved to the seat's append-only route history.
    pub fenced: BTreeSet<ExternalId>,
    /// Whether a launch for the next occupancy was prepared and never
    /// installed: a previous attempt may have created a native whose answer
    /// it never heard.
    pub launch_prepared: bool,
    /// What the runtime reports carrying the binding.
    pub census: DurableSeatCensus,
}

closed_enum! {
    /// What reconciliation concluded about one declared seat.
    DurableSeatStatus, "DurableSeatStatus" {
        /// The recorded native is live and idle.
        Healthy => "healthy",
        /// The recorded native is live and not idle.
        Busy => "busy",
        /// Proved absent: the only status that may lead to a launch.
        Missing => "missing",
        /// More than one live native carries the binding.
        Duplicate => "duplicate",
        /// The native was stopped on purpose and is kept as archive evidence.
        Archived => "archived",
        /// What the runtime holds disagrees with what Kontor recorded.
        Mismatched => "mismatched",
        /// Not checked, or not checkable. Never evidence of absence.
        Unchecked => "unchecked",
    }
}

closed_enum! {
    /// Why one seat has the status it has.
    DurableSeatReason, "DurableSeatReason" {
        /// Healthy: live and idle.
        Idle => "idle",
        /// Busy: starting up or running a turn.
        Working => "working",
        /// Busy: blocked on a person's permission.
        AwaitingPermission => "awaiting_permission",
        /// Busy: live with an error reported.
        Errored => "errored",
        /// Busy: live in an unnamed state.
        ActivityUnknown => "activity_unknown",
        /// Missing: nothing was ever launched for this seat.
        NeverLaunched => "never_launched",
        /// Missing: a launch was prepared and stopped before any native
        /// existed.
        LaunchNotEffected => "launch_not_effected",
        /// Missing: the recorded native is absent from an authoritative
        /// census.
        NativeAbsent => "native_absent",
        /// Duplicate: several live natives carry the binding.
        SeveralLiveCarriers => "several_live_carriers",
        /// Archived: the native exists only as archive evidence.
        NativeArchived => "native_archived",
        /// Mismatched: the planned binding was retired.
        BindingRetired => "binding_retired",
        /// Mismatched: the runtime's answer does not correlate.
        Uncorrelated => "uncorrelated",
        /// Mismatched: a live native carries the binding and Kontor never
        /// recorded launching it.
        UnrecordedCarrier => "unrecorded_carrier",
        /// Mismatched: a native Kontor already retired is live again.
        PredecessorLive => "predecessor_live",
        /// Unchecked: recovery belongs to Paseo-direct operation, so no
        /// runtime was asked.
        PaseoDirectOwner => "paseo_direct_owner",
        /// Unchecked: no runtime able to answer is available.
        RuntimeUnavailable => "runtime_unavailable",
        /// Unchecked: the census failed.
        CensusFailed => "census_failed",
        /// Unchecked: one live native carries the binding and a launch was
        /// prepared, so it is most likely that launch's native whose answer
        /// was lost. Binding it is a recovery, not a restart.
        LaunchUnconfirmed => "launch_unconfirmed",
    }
}

/// One seat's typed classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurableSeatObservation {
    /// The conclusion.
    pub status: DurableSeatStatus,
    /// Why.
    pub reason: DurableSeatReason,
}

impl DurableSeatObservation {
    const fn of(status: DurableSeatStatus, reason: DurableSeatReason) -> Self {
        Self { status, reason }
    }
}

/// Classify one seat from facts already read.
///
/// The order is the safety argument. Ownership is settled before any runtime
/// fact is consulted, a retired binding before the census, and an
/// unavailable, failed or uncorrelated census before anything that could
/// read as absence.
#[must_use]
pub fn classify(facts: &DurableSeatFacts) -> DurableSeatObservation {
    use DurableSeatReason as R;
    use DurableSeatStatus as S;
    if facts.owner == DurableSeatRecoveryOwner::PaseoDirect {
        return DurableSeatObservation::of(S::Unchecked, R::PaseoDirectOwner);
    }
    if facts.binding == DurableSeatBindingFact::Retired {
        return DurableSeatObservation::of(S::Mismatched, R::BindingRetired);
    }
    let carriers = match &facts.census {
        DurableSeatCensus::NotTaken | DurableSeatCensus::Unavailable => {
            return DurableSeatObservation::of(S::Unchecked, R::RuntimeUnavailable);
        }
        DurableSeatCensus::Failed => {
            return DurableSeatObservation::of(S::Unchecked, R::CensusFailed);
        }
        DurableSeatCensus::Uncorrelated => {
            return DurableSeatObservation::of(S::Mismatched, R::Uncorrelated);
        }
        DurableSeatCensus::Taken(carriers) => carriers,
    };
    let live: Vec<&DurableSeatCarrier> = carriers.iter().filter(|c| !c.archived).collect();
    if live.len() > 1 {
        return DurableSeatObservation::of(S::Duplicate, R::SeveralLiveCarriers);
    }
    if let Some(carrier) = live.first()
        && facts.fenced.contains(&carrier.native_id)
    {
        return DurableSeatObservation::of(S::Mismatched, R::PredecessorLive);
    }
    match (&facts.occupancy, live.first()) {
        (Some(occupant), Some(carrier)) if &carrier.native_id == occupant => {
            DurableSeatObservation::of(
                if carrier.activity == DurableSeatActivity::Idle {
                    S::Healthy
                } else {
                    S::Busy
                },
                match carrier.activity {
                    DurableSeatActivity::Idle => R::Idle,
                    DurableSeatActivity::Working => R::Working,
                    DurableSeatActivity::AwaitingPermission => R::AwaitingPermission,
                    DurableSeatActivity::Errored => R::Errored,
                    DurableSeatActivity::Unknown => R::ActivityUnknown,
                },
            )
        }
        (Some(_), Some(_)) => DurableSeatObservation::of(S::Mismatched, R::UnrecordedCarrier),
        (Some(occupant), None) => {
            if carriers
                .iter()
                .any(|carrier| carrier.archived && &carrier.native_id == occupant)
            {
                DurableSeatObservation::of(S::Archived, R::NativeArchived)
            } else {
                DurableSeatObservation::of(S::Missing, R::NativeAbsent)
            }
        }
        (None, Some(_)) if facts.launch_prepared => {
            DurableSeatObservation::of(S::Unchecked, R::LaunchUnconfirmed)
        }
        (None, Some(_)) => DurableSeatObservation::of(S::Mismatched, R::UnrecordedCarrier),
        (None, None) => {
            // An archived native nobody fenced was launched for this binding
            // and then stopped: stopped on purpose, not missing.
            if carriers
                .iter()
                .any(|carrier| carrier.archived && !facts.fenced.contains(&carrier.native_id))
            {
                DurableSeatObservation::of(S::Archived, R::NativeArchived)
            } else if facts.launch_prepared {
                DurableSeatObservation::of(S::Missing, R::LaunchNotEffected)
            } else {
                DurableSeatObservation::of(S::Missing, R::NeverLaunched)
            }
        }
    }
}

closed_enum! {
    /// What reconciliation will do about one seat.
    DurableSeatDecision, "DurableSeatDecision" {
        /// Nothing: report only.
        None => "none",
        /// Launch the seat's next occupancy: it is proved missing, it is the
        /// daemon's, and the caller selected it and asked for restarts.
        Launch => "launch",
        /// Bind the native a prepared launch already produced. The launch is
        /// replayed under the same prepared intent, so the runtime returns
        /// that native rather than creating another.
        Recover => "recover",
    }
}

/// Decide what to do about one classified seat.
///
/// `restart` is true only when the caller asked for restarts and explicitly
/// selected this seat. Everything other than a proved-missing or an
/// unconfirmed-launch daemon-owned seat is left alone.
#[must_use]
pub fn decide(
    owner: DurableSeatRecoveryOwner,
    observation: DurableSeatObservation,
    restart: bool,
) -> DurableSeatDecision {
    if !restart || owner != DurableSeatRecoveryOwner::Daemon {
        return DurableSeatDecision::None;
    }
    match (observation.status, observation.reason) {
        (DurableSeatStatus::Missing, _) => DurableSeatDecision::Launch,
        (DurableSeatStatus::Unchecked, DurableSeatReason::LaunchUnconfirmed) => {
            DurableSeatDecision::Recover
        }
        _ => DurableSeatDecision::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native(id: &str) -> ExternalId {
        ExternalId::parse(id).expect("native id")
    }

    fn carrier(id: &str, archived: bool, activity: DurableSeatActivity) -> DurableSeatCarrier {
        DurableSeatCarrier {
            native_id: native(id),
            archived,
            activity,
        }
    }

    fn facts(census: DurableSeatCensus) -> DurableSeatFacts {
        DurableSeatFacts {
            owner: DurableSeatRecoveryOwner::Daemon,
            binding: DurableSeatBindingFact::Active,
            occupancy: Some(native("seat-1")),
            fenced: BTreeSet::new(),
            launch_prepared: false,
            census,
        }
    }

    fn observed(facts: &DurableSeatFacts) -> (DurableSeatStatus, DurableSeatReason) {
        let observation = classify(facts);
        (observation.status, observation.reason)
    }

    #[test]
    fn the_asma_8419_titles_render_from_role_code_and_label() {
        let separator = NameSeparator::default();
        let lsa = RoleCode::parse("LSA").expect("role");
        for (label, title) in [
            ("PR DRAFTS STATUS CHECKER", "LSA • PR DRAFTS STATUS CHECKER"),
            ("GENERAL", "LSA • GENERAL"),
        ] {
            let rendered = render_durable_seat_title(
                &separator,
                &lsa,
                &ExternalName::parse(label).expect("label"),
            )
            .expect("renders");
            assert_eq!(rendered.as_str(), title);
        }
        assert!(
            render_durable_seat_title(
                &separator,
                &lsa,
                &ExternalName::parse("PR • GENERAL").expect("label"),
            )
            .is_err(),
            "a label carrying the separator would render an ambiguous title"
        );
    }

    #[test]
    fn only_an_idle_recorded_native_is_healthy() {
        let mut seat = facts(DurableSeatCensus::Taken(vec![carrier(
            "seat-1",
            false,
            DurableSeatActivity::Idle,
        )]));
        assert_eq!(
            observed(&seat),
            (DurableSeatStatus::Healthy, DurableSeatReason::Idle)
        );
        for (activity, reason) in [
            (DurableSeatActivity::Working, DurableSeatReason::Working),
            (
                DurableSeatActivity::AwaitingPermission,
                DurableSeatReason::AwaitingPermission,
            ),
            (DurableSeatActivity::Errored, DurableSeatReason::Errored),
            (
                DurableSeatActivity::Unknown,
                DurableSeatReason::ActivityUnknown,
            ),
        ] {
            seat.census = DurableSeatCensus::Taken(vec![carrier("seat-1", false, activity)]);
            assert_eq!(observed(&seat), (DurableSeatStatus::Busy, reason));
        }
    }

    #[test]
    fn nothing_that_is_not_proved_absent_is_missing() {
        let mut seat = facts(DurableSeatCensus::Failed);
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Unchecked,
                DurableSeatReason::CensusFailed
            )
        );
        seat.census = DurableSeatCensus::Unavailable;
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Unchecked,
                DurableSeatReason::RuntimeUnavailable
            )
        );
        seat.census = DurableSeatCensus::Uncorrelated;
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Mismatched,
                DurableSeatReason::Uncorrelated
            )
        );
        seat.census = DurableSeatCensus::Taken(vec![
            carrier("seat-1", false, DurableSeatActivity::Idle),
            carrier("seat-2", false, DurableSeatActivity::Idle),
        ]);
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Duplicate,
                DurableSeatReason::SeveralLiveCarriers
            )
        );
        seat.census =
            DurableSeatCensus::Taken(vec![carrier("seat-1", true, DurableSeatActivity::Idle)]);
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Archived,
                DurableSeatReason::NativeArchived
            )
        );
        seat.census =
            DurableSeatCensus::Taken(vec![carrier("other", false, DurableSeatActivity::Idle)]);
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Mismatched,
                DurableSeatReason::UnrecordedCarrier
            )
        );
        seat.binding = DurableSeatBindingFact::Retired;
        seat.census = DurableSeatCensus::Taken(Vec::new());
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Mismatched,
                DurableSeatReason::BindingRetired
            )
        );
    }

    #[test]
    fn a_paseo_direct_seat_is_unchecked_whatever_the_runtime_says() {
        for census in [
            DurableSeatCensus::NotTaken,
            DurableSeatCensus::Taken(Vec::new()),
            DurableSeatCensus::Failed,
        ] {
            let mut seat = facts(census);
            seat.owner = DurableSeatRecoveryOwner::PaseoDirect;
            seat.binding = DurableSeatBindingFact::Absent;
            seat.occupancy = None;
            let observation = classify(&seat);
            assert_eq!(
                (observation.status, observation.reason),
                (
                    DurableSeatStatus::Unchecked,
                    DurableSeatReason::PaseoDirectOwner
                )
            );
            assert_eq!(
                decide(DurableSeatRecoveryOwner::PaseoDirect, observation, true),
                DurableSeatDecision::None
            );
        }
    }

    #[test]
    fn absence_is_proved_three_ways_and_only_those_launch() {
        let mut seat = facts(DurableSeatCensus::Taken(Vec::new()));
        assert_eq!(
            observed(&seat),
            (DurableSeatStatus::Missing, DurableSeatReason::NativeAbsent)
        );
        seat.occupancy = None;
        seat.binding = DurableSeatBindingFact::Absent;
        assert_eq!(
            observed(&seat),
            (DurableSeatStatus::Missing, DurableSeatReason::NeverLaunched)
        );
        seat.launch_prepared = true;
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Missing,
                DurableSeatReason::LaunchNotEffected
            )
        );
        // A fenced predecessor kept as archive evidence does not hide absence.
        seat.fenced.insert(native("seat-0"));
        seat.census =
            DurableSeatCensus::Taken(vec![carrier("seat-0", true, DurableSeatActivity::Idle)]);
        assert_eq!(observed(&seat).0, DurableSeatStatus::Missing);
        // ...but one that is live again is not absence at all.
        seat.census =
            DurableSeatCensus::Taken(vec![carrier("seat-0", false, DurableSeatActivity::Idle)]);
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Mismatched,
                DurableSeatReason::PredecessorLive
            )
        );
    }

    #[test]
    fn a_lost_launch_answer_recovers_and_never_relaunches() {
        let mut seat = facts(DurableSeatCensus::Taken(vec![carrier(
            "seat-1",
            false,
            DurableSeatActivity::Idle,
        )]));
        seat.occupancy = None;
        seat.launch_prepared = true;
        let observation = classify(&seat);
        assert_eq!(
            (observation.status, observation.reason),
            (
                DurableSeatStatus::Unchecked,
                DurableSeatReason::LaunchUnconfirmed
            )
        );
        assert_eq!(
            decide(DurableSeatRecoveryOwner::Daemon, observation, true),
            DurableSeatDecision::Recover
        );
        seat.launch_prepared = false;
        assert_eq!(
            observed(&seat),
            (
                DurableSeatStatus::Mismatched,
                DurableSeatReason::UnrecordedCarrier
            )
        );
    }

    #[test]
    fn only_a_selected_missing_daemon_seat_launches() {
        use DurableSeatReason as R;
        use DurableSeatStatus as S;
        let missing = DurableSeatObservation::of(S::Missing, R::NativeAbsent);
        assert_eq!(
            decide(DurableSeatRecoveryOwner::Daemon, missing, true),
            DurableSeatDecision::Launch
        );
        assert_eq!(
            decide(DurableSeatRecoveryOwner::Daemon, missing, false),
            DurableSeatDecision::None
        );
        // Ownership decides on its own, whatever the observation says: a
        // Paseo-direct seat is never launched or recovered by Kontor.
        for observation in [
            missing,
            DurableSeatObservation::of(S::Unchecked, R::LaunchUnconfirmed),
        ] {
            assert_eq!(
                decide(DurableSeatRecoveryOwner::PaseoDirect, observation, true),
                DurableSeatDecision::None
            );
        }
        for (status, reason) in [
            (S::Healthy, R::Idle),
            (S::Busy, R::Working),
            (S::Duplicate, R::SeveralLiveCarriers),
            (S::Archived, R::NativeArchived),
            (S::Mismatched, R::Uncorrelated),
            (S::Unchecked, R::CensusFailed),
            (S::Unchecked, R::RuntimeUnavailable),
        ] {
            assert_eq!(
                decide(
                    DurableSeatRecoveryOwner::Daemon,
                    DurableSeatObservation::of(status, reason),
                    true
                ),
                DurableSeatDecision::None,
                "{status}/{reason} must not launch"
            );
        }
    }

    fn declaration() -> DurableSeatDeclaration {
        DurableSeatDeclaration {
            project_id: ProjectId::generate(),
            desk_key: DeskKey::parse("pr-review").expect("desk"),
            seat_key: DurableSeatKey::parse("general").expect("seat"),
            title: ExternalName::parse("LSA • GENERAL").expect("title"),
            role_code: RoleCode::parse("LSA").expect("role"),
            seat_label: ExternalName::parse("GENERAL").expect("label"),
            model_rung: None,
            procedure: Some(
                ExternalName::parse(".github/skills/asma-pr-review/SKILL.md").expect("procedure"),
            ),
            recovery_owner: DurableSeatRecoveryOwner::PaseoDirect,
            decided_by: ExternalName::parse("Igor").expect("decider"),
            decided_on: civil::date(2026, 10, 8),
            seat_binding_id: None,
            declared_at: "2026-10-11T10:00:00Z".parse().expect("instant"),
        }
    }

    #[test]
    fn a_declaration_validates_its_title_owner_date_and_procedure() {
        let separator = NameSeparator::default();
        declaration().validate(&separator).expect("valid");

        let mut wrong_title = declaration();
        wrong_title.title = ExternalName::parse("LSA • General").expect("title");
        assert!(wrong_title.validate(&separator).is_err());

        let mut bound_direct = declaration();
        bound_direct.seat_binding_id = Some(SeatBindingId::generate());
        assert!(bound_direct.validate(&separator).is_err());

        let mut unrouted_daemon = declaration();
        unrouted_daemon.recovery_owner = DurableSeatRecoveryOwner::Daemon;
        unrouted_daemon.seat_binding_id = Some(SeatBindingId::generate());
        assert!(unrouted_daemon.validate(&separator).is_err());

        let mut future = declaration();
        future.decided_on = civil::date(2026, 10, 13);
        assert!(future.validate(&separator).is_err());
        let mut tomorrow = declaration();
        tomorrow.decided_on = civil::date(2026, 10, 12);
        tomorrow.validate(&separator).expect("one day of slack");

        for procedure in ["/etc/passwd", "../x/SKILL.md", "a//b", "a\\b"] {
            let mut escaped = declaration();
            escaped.procedure = Some(ExternalName::parse(procedure).expect("text"));
            assert!(escaped.validate(&separator).is_err(), "{procedure}");
        }
    }

    #[test]
    fn a_retry_of_the_same_decision_is_the_same_seat() {
        let first = declaration();
        let mut retry = first.clone();
        retry.seat_binding_id = Some(SeatBindingId::generate());
        retry.declared_at = "2026-10-11T11:00:00Z".parse().expect("instant");
        assert!(first.declares_same_seat(&retry));
        retry.decided_by = ExternalName::parse("Adam").expect("decider");
        assert!(!first.declares_same_seat(&retry));
    }
}
