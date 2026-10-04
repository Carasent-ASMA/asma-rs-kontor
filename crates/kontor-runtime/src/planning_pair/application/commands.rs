//! Shared planning-pair command coordination (ASMA-8282 W3b).
//!
//! Owners retain authentication, persistence, compare-and-swap, runtime
//! effects and rendering. These sequences neither mint authority nor create
//! another owner; the daemon remains the sole production implementation.

#[cfg(test)]
mod tests;

pub mod contribution;
pub mod recovery;

use kontor_core::DomainError;
use kontor_core::id::{
    AggregateRevision, CanonicalDocument, CommandReceiptId, IdempotencyKey, PlanningPairRunId,
    ProjectId, Timestamp,
};
use kontor_core::planning_pair::PlanningPairSlot;
use kontor_core::receipt::AggregateRef;
use kontor_core::repository::StoredConsultationRun;

use super::PairState;
use crate::planning_pair::intent::SeatGeneration;

/// An existing receipt, including the revision that its command wrote.
#[derive(Debug, Clone, Copy)]
pub struct Replay {
    /// The receipt's immutable identity.
    pub id: CommandReceiptId,
    /// The revision the receipt originally named.
    pub revision: AggregateRevision,
}

/// The authenticated viewer of a command result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewer {
    /// The frozen caller, seeing only domain-released contributions.
    Caller,
    /// One member, seeing only that member's own contributions.
    Member(PlanningPairSlot),
}

/// The unchanged reads and replay boundary shared by contribution/recovery.
/// An `Actor` is supplied by the owner's authenticated boundary; constructing
/// a generation fingerprint from it never establishes its authority.
pub trait CommandOwner: Sync {
    /// The owner's refusal.
    type Error: Send;
    /// The owner's already authenticated actor.
    type Actor: Copy + Send + Sync;

    /// Answer a domain refusal.
    fn domain_error(&self, error: &DomainError) -> Self::Error;
    /// The owner's clock.
    fn now(&self) -> Timestamp;
    /// The requested durable run.
    fn stored_run(
        &self,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
    ) -> Result<StoredConsultationRun, Self::Error>;
    /// Restore the run through the domain transitions.
    fn pair_state(&self, run: &StoredConsultationRun) -> Result<PairState, Self::Error>;
    /// Fingerprint an actor which the sequence has already authenticated.
    fn presented(&self, actor: Self::Actor) -> SeatGeneration;
    /// Canonicalize the command's existing intent document.
    fn canonical(&self, document: &serde_json::Value) -> Result<CanonicalDocument, Self::Error>;
    /// Read an exact receipt replay, with the owner's original key checks.
    fn replayed(
        &self,
        key: &IdempotencyKey,
        intent: &CanonicalDocument,
        target: &AggregateRef,
    ) -> Result<Option<Replay>, Self::Error>;
    /// Hold a new command to the run revision read by its caller.
    fn expect_revision(
        &self,
        run: &StoredConsultationRun,
        expected: AggregateRevision,
    ) -> Result<(), Self::Error>;
}
