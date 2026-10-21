//! A typed record of a Kontor capability gap a seat had to contain.
//!
//! The transition rule requires a fallback to be evidence rather than folklore:
//! a seat that cannot perform an intended Kontor operation records the intended
//! operation, its failure class, the exact identities around it, the checkpoint
//! it resumed from, the bounded fallback it used, and its owner. A repository
//! document was the only durable place for that record before this ledger, which
//! meant a control-plane fact lived outside the control plane.
//!
//! The record is append-only. A gap is a fact about a moment, and a later
//! resolution is a new fact about that same identity rather than an edit to what
//! was observed. Its `status` is therefore the state it was recorded in, not a
//! mutable projection.
//!
//! Nothing here scans, notifies or escalates. Recording a gap is bookkeeping; the
//! repair remains the owner's work under the transition rule.

use crate::id::{
    AgentRunId, BoundedText, ContentHash, GapFailureClass, MiniProjectId, OperationalGapId,
    ProjectId, SeatBindingId, TaskId, Timestamp,
};
use serde::{Deserialize, Serialize};

crate::closed_enum! {
    /// The lifecycle state a gap record was written in.
    ///
    /// Recording always writes [`Self::Open`]. Closure is deliberately a later,
    /// separately authorized operation: a record that could be closed by the
    /// same call that opened it would lose the distinction between the observed
    /// gap and its repair.
    OperationalGapStatus, "OperationalGapStatus" {
        /// The gap is recorded and awaits its owner's repair.
        Open => "open",
    }
}

/// One durable operational-gap record.
///
/// Every identity is stored rather than inferred at read time. The realm is the
/// store's own boundary, the project and task are the route's scope, the epic is
/// the task's epic at record time, and the run and seat are the exact subjects
/// the fallback was observed around. `None` means the gap was not observed
/// around that subject, never that the subject is unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationalGap {
    /// The durable gap identity. The caller chooses it once and reuses it on
    /// every retry, so a replay cannot mint a second record for one observation.
    pub gap_id: OperationalGapId,
    /// The project the gap was recorded in.
    pub project_id: ProjectId,
    /// The epic the owning task belonged to, when it belongs to one.
    pub mini_project_id: Option<MiniProjectId>,
    /// The task the gap evidence is attached to.
    pub task_id: TaskId,
    /// The exact agent run the fallback was taken around, when there was one.
    pub agent_run_id: Option<AgentRunId>,
    /// The exact persistent seat the fallback was taken around, when there was
    /// one.
    pub seat_binding_id: Option<SeatBindingId>,
    /// The operating class of the failure, as an open key rather than prose.
    pub failure_class: GapFailureClass,
    /// The Kontor operation the seat intended to perform.
    pub intended_operation: BoundedText,
    /// The recorded checkpoint the seat resumed from after containing it.
    pub checkpoint: BoundedText,
    /// The bounded fallback action actually taken.
    pub fallback_action: BoundedText,
    /// Who owns the repair.
    pub owner: BoundedText,
    /// Optional digest of the evidence document the record cites.
    pub evidence: Option<ContentHash>,
    /// The state at record time.
    pub status: OperationalGapStatus,
    /// When the record was written.
    pub recorded_at: Timestamp,
}
