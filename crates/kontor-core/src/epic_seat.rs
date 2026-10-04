//! What an epic-level seat certifiably is, and why succession cannot reach it.
//!
//! An ECP seat is not task-scoped: it holds no `task_id` and no `team_run_id`,
//! because it serves the epic rather than one task. Two consequences follow,
//! and both were being read wrongly.
//!
//! First, `last_attached_at` does not mean *bound*. It is written by
//! `observe_seat_binding` from a [`SeatLivenessObservation`], and the observe
//! act fills `attached_at` from a successful readback while deliberately
//! leaving `activity_at` alone — a readback proves the seat answered, never
//! that it is working. A seat that was never bound can therefore carry an
//! attach instant, with no runtime self-report and no activity behind it, and
//! read as "attached at T" to anything that looks only at that column.
//!
//! Second, `seat_replace` fences on an expected task revision and a predecessor
//! AgentRun. An ECP seat has neither to name, so succession is structurally
//! unreachable for it rather than merely unexposed. Naming that is the honest
//! answer; synthesizing a task revision or an AgentRun to satisfy the request
//! shape would be inventing the exact evidence the fence exists to require.
//!
//! This module only classifies. It holds no store and no clock, so it cannot
//! write, and it never manufactures an identity it was not given.

use crate::id::{TaskId, TeamRunId, Timestamp};
use crate::state::ObservedRunState;

/// The authoritative columns of one epic-level seat, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedEpicSeat {
    /// Whether the seat is still active, as opposed to retired or archived.
    pub active: bool,
    /// The task it serves. `None` for an epic-level seat.
    pub task_id: Option<TaskId>,
    /// The TeamRun it serves. `None` for an epic-level seat.
    pub team_run_id: Option<TeamRunId>,
    /// The runtime's own latest self-report, when it ever made one.
    pub runtime_reported: Option<ObservedRunState>,
    /// When a readback last confirmed the seat answered.
    pub last_attached_at: Option<Timestamp>,
    /// When an observed runtime event or turn position last proved work.
    pub last_activity_at: Option<Timestamp>,
    /// When the seat was released, if it was.
    pub released_at: Option<Timestamp>,
}

/// What the seat's own evidence certifies, and nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpicSeatState {
    /// Released. Its evidence stands; it holds no slot.
    Released,
    /// Nothing has been observed of it at all.
    Unobserved,
    /// A readback confirmed it answered, and that is *all* it certifies: no
    /// runtime self-report and no observed activity stand behind it.
    ///
    /// This is the certified pre-native state. A seat here looks attached to
    /// any reader of `last_attached_at` alone, and is not bound.
    AttachObservedOnly,
    /// The runtime reports a state for it, but nothing has proved its work.
    RuntimeReported,
    /// An observed runtime event or turn position proves it worked.
    Working,
}

/// Why a certified successor cannot be requested for this seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuccessionReachability {
    /// The seat is task-scoped, so the supported request can name its fences.
    Reachable,
    /// The seat serves an epic, not a task. `seat_replace` fences on an
    /// expected task revision and a predecessor AgentRun; this seat has
    /// neither, and inventing them would forge the fence.
    NoTaskScope,
    /// Task-scoped, but nothing has ever run on it to name as predecessor.
    NoPredecessorEvidence,
}

impl EpicSeatState {
    /// The closed vocabulary, for a typed projection.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Released => "released",
            Self::Unobserved => "unobserved",
            Self::AttachObservedOnly => "attach_observed_only",
            Self::RuntimeReported => "runtime_reported",
            Self::Working => "working",
        }
    }

    /// Whether this state certifies a native ever stood behind the seat.
    ///
    /// `false` for [`Self::AttachObservedOnly`] on purpose: an answered
    /// readback is not a binding.
    #[must_use]
    pub const fn certifies_native(self) -> bool {
        matches!(self, Self::RuntimeReported | Self::Working)
    }
}

impl SuccessionReachability {
    /// The closed vocabulary.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reachable => "reachable",
            Self::NoTaskScope => "no_task_scope",
            Self::NoPredecessorEvidence => "no_predecessor_evidence",
        }
    }
}

/// One seat's certified state and succession reachability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpicSeatProjection {
    /// What the evidence certifies.
    pub state: EpicSeatState,
    /// Why a certified successor can or cannot be requested.
    pub succession: SuccessionReachability,
}

/// Classify one epic-level seat from its own stored evidence.
#[must_use]
pub fn classify(seat: &ObservedEpicSeat) -> EpicSeatProjection {
    let state = if seat.released_at.is_some() || !seat.active {
        EpicSeatState::Released
    } else if seat.last_activity_at.is_some() {
        EpicSeatState::Working
    } else if seat.runtime_reported.is_some() {
        EpicSeatState::RuntimeReported
    } else if seat.last_attached_at.is_some() {
        // The whole point. An attach instant with nothing behind it certifies
        // that the seat answered once, not that anything was ever bound to it.
        EpicSeatState::AttachObservedOnly
    } else {
        EpicSeatState::Unobserved
    };
    let succession = if seat.task_id.is_none() || seat.team_run_id.is_none() {
        SuccessionReachability::NoTaskScope
    } else if state.certifies_native() {
        SuccessionReachability::Reachable
    } else {
        SuccessionReachability::NoPredecessorEvidence
    };
    EpicSeatProjection { state, succession }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::parse_utc_timestamp;

    fn at(text: &str) -> Timestamp {
        parse_utc_timestamp(text).expect("canonical UTC")
    }

    fn epic_seat() -> ObservedEpicSeat {
        ObservedEpicSeat {
            active: true,
            task_id: None,
            team_run_id: None,
            runtime_reported: None,
            last_attached_at: None,
            last_activity_at: None,
            released_at: None,
        }
    }

    /// ASMA-7869 TPM seat 01a02b8e-8f63-7161-b043-cf8cc6d1297e, exactly as
    /// stored: an attach instant, no runtime self-report, no activity.
    #[test]
    fn an_attach_instant_alone_does_not_certify_a_native() {
        let seat = ObservedEpicSeat {
            last_attached_at: Some(at("2026-09-19T02:13:45Z")),
            ..epic_seat()
        };
        let projection = classify(&seat);
        assert_eq!(projection.state, EpicSeatState::AttachObservedOnly);
        assert!(
            !projection.state.certifies_native(),
            "a readback that was answered is not a binding"
        );
        assert_eq!(projection.succession, SuccessionReachability::NoTaskScope);
    }

    /// ASMA-8098 LSA seat 01a070e7-bd07-7a03-84fe-cb9f41f29fd6: the runtime
    /// reports it running, and still nothing has proved its work.
    #[test]
    fn a_runtime_self_report_certifies_a_native_but_not_activity() {
        let seat = ObservedEpicSeat {
            runtime_reported: Some(ObservedRunState::Running),
            last_attached_at: Some(at("2026-09-18T22:07:18Z")),
            ..epic_seat()
        };
        let projection = classify(&seat);
        assert_eq!(projection.state, EpicSeatState::RuntimeReported);
        assert!(projection.state.certifies_native());
        assert_eq!(
            projection.succession,
            SuccessionReachability::NoTaskScope,
            "an epic seat has no task revision or predecessor run to fence on, \
             however live it is"
        );
    }

    #[test]
    fn observed_activity_is_the_only_thing_that_certifies_work() {
        let seat = ObservedEpicSeat {
            runtime_reported: Some(ObservedRunState::Running),
            last_attached_at: Some(at("2026-09-18T22:07:18Z")),
            last_activity_at: Some(at("2026-09-18T22:09:00Z")),
            ..epic_seat()
        };
        assert_eq!(classify(&seat).state, EpicSeatState::Working);
    }

    #[test]
    fn a_seat_nothing_was_ever_observed_of_is_unobserved() {
        assert_eq!(classify(&epic_seat()).state, EpicSeatState::Unobserved);
    }

    #[test]
    fn a_released_seat_is_released_whatever_else_it_carries() {
        let seat = ObservedEpicSeat {
            runtime_reported: Some(ObservedRunState::Running),
            last_activity_at: Some(at("2026-09-18T22:09:00Z")),
            released_at: Some(at("2026-09-19T01:00:00Z")),
            ..epic_seat()
        };
        assert_eq!(classify(&seat).state, EpicSeatState::Released);
    }

    #[test]
    fn a_task_scoped_seat_with_a_native_can_be_succeeded() {
        let seat = ObservedEpicSeat {
            task_id: Some(TaskId::parse("01a075dc-74e4-71f0-9e63-775bd79391c9").expect("a task")),
            team_run_id: Some(
                TeamRunId::parse("01a07636-ce37-7b61-91a8-8e6faf7e01eb").expect("a team run"),
            ),
            runtime_reported: Some(ObservedRunState::Running),
            ..epic_seat()
        };
        assert_eq!(
            classify(&seat).succession,
            SuccessionReachability::Reachable
        );
    }

    #[test]
    fn a_task_scoped_seat_with_no_native_has_no_predecessor_to_cite() {
        let seat = ObservedEpicSeat {
            task_id: Some(TaskId::parse("01a075dc-74e4-71f0-9e63-775bd79391c9").expect("a task")),
            team_run_id: Some(
                TeamRunId::parse("01a07636-ce37-7b61-91a8-8e6faf7e01eb").expect("a team run"),
            ),
            last_attached_at: Some(at("2026-09-19T02:13:45Z")),
            ..epic_seat()
        };
        assert_eq!(
            classify(&seat).succession,
            SuccessionReachability::NoPredecessorEvidence
        );
    }
}
