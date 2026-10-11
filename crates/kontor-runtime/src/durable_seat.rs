//! The runtime half of durable desk seats (ASMA-8451).
//!
//! A durable desk seat is a persistent hosted seat that belongs to no epic. It
//! reuses the hosted-seat launch, its SeatBinding correlation label and its
//! prepared launch intents unchanged. Two things are new here: whose seat a
//! hosted launch fills, so a desk launch can name its desk's exact native
//! project instead of an epic's, and a read-only census of every native that
//! carries one seat's binding label, so reconciliation can tell a missing seat
//! from a busy, archived or duplicated one.

use kontor_core::durable_seat::DurableSeatCarrier;
use kontor_core::id::{SeatBindingId, Timestamp};
use kontor_core::state::NativeRuntimeIdentity;

use crate::scope::ExecutionScope;

/// Whose seat a hosted launch fills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostedSeatScope {
    /// An epic's persistent leadership seat, placed inside the epic's prepared
    /// project and labelled with its epic.
    Epic(ExecutionScope),
    /// A durable desk seat. It belongs to no epic, so it carries no epic label
    /// and its native project is the desk workspace's exact persisted parent:
    /// never an epic's project, and never found by title or path.
    Desk {
        /// The desk's own native container, the workspace's parent.
        project: NativeRuntimeIdentity,
    },
}

impl HostedSeatScope {
    /// The epic scope, when the seat is an epic's.
    #[must_use]
    pub const fn epic(&self) -> Option<&ExecutionScope> {
        match self {
            Self::Epic(scope) => Some(scope),
            Self::Desk { .. } => None,
        }
    }
}

/// Ask which natives carry one hosted seat's binding label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedSeatCensusRequest {
    /// The SeatBinding whose correlation label is counted.
    pub seat_binding_id: SeatBindingId,
    /// Census instant.
    pub requested_at: Timestamp,
}

/// An authoritative answer: every native carrying the binding label, archived
/// ones included.
///
/// An empty list is evidence of absence. A runtime that cannot give a complete
/// answer must refuse instead, because the caller may act on an empty one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedSeatCensus {
    /// Every carrier, in the runtime's order.
    pub carriers: Vec<DurableSeatCarrier>,
    /// When the census was taken.
    pub observed_at: Timestamp,
}
