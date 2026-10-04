//! The intent fingerprints of the planning pair commands (ASMA-8282 B1b).
//!
//! Each input renders the exact document the service canonicalizes into the
//! intent it records with a command's receipt and compares on a replay. The
//! service fills each input from the request it parsed and the facts it read
//! and authenticated, then canonicalizes, replays, writes and answers exactly
//! as before. Nothing here reads state, authenticates, canonicalizes or
//! records. A seat and generation named here are fingerprint inputs only:
//! naming one authenticates nothing.

use std::collections::BTreeSet;

use kontor_core::consultation::ConsultationRunId;
use kontor_core::id::{
    AggregateRevision, BoundedText, ContentHash, ExternalId, ExternalName, MiniProjectId,
    ProjectId, SeatBindingId, SpecVersion, TaskId,
};
use kontor_core::planning_pair::{
    ConsultationProtocol, PlanningPairDisposition, PlanningPairRound, PlanningPairSlot,
};
use kontor_core::state::NativeRuntimeIdentity;

/// One seat at one occupancy generation, as a fingerprint names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeatGeneration {
    /// The SeatBinding.
    pub seat_binding_id: SeatBindingId,
    /// The occupancy generation the command was presented at.
    pub occupancy_generation: u64,
}

/// One member placement an invocation requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvokeMember<'a> {
    /// The slot it fills.
    pub slot: PlanningPairSlot,
    /// The binding key the caller named.
    pub binding_key: &'a str,
    /// The accounts the caller marked unavailable, as requested.
    pub unavailable_accounts: &'a [String],
    /// The vendors the caller excluded, as requested.
    pub excluded_vendors: &'a [String],
}

/// An invocation of one planning pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invoke<'a> {
    /// The project.
    pub project_id: ProjectId,
    /// The epic it is invoked on.
    pub epic_id: MiniProjectId,
    /// The pinned document's id, as requested.
    pub profile_id: &'a str,
    /// The pinned document's revision.
    pub profile_version: SpecVersion,
    /// The pinned document's hash.
    pub definition_hash: &'a ContentHash,
    /// The topic.
    pub topic: &'a ExternalName,
    /// The question.
    pub question: &'a BoundedText,
    /// The ticket it is scoped to, if any.
    pub task_id: Option<TaskId>,
    /// The requested member placements, in request order.
    pub members: &'a [InvokeMember<'a>],
    /// The caller's seat and generation.
    pub caller: SeatGeneration,
}

impl Invoke<'_> {
    /// The fingerprint document.
    #[must_use]
    pub fn document(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "operation": "invoke_planning_pair_run",
            "project": self.project_id.to_string(),
            "epic": self.epic_id.to_string(),
            "protocol": ConsultationProtocol::PlanningPair.as_str(),
            "profile": [
                self.profile_id,
                self.profile_version.get(),
                self.definition_hash.as_str(),
            ],
            "topic": self.topic.as_str(),
            "question": self.question.as_str(),
            "task_id": self.task_id.map(|id| id.to_string()),
            "members": self.members.iter().map(|member| serde_json::json!({
                "slot": member.slot.as_str(),
                "binding_key": member.binding_key,
                "unavailable_accounts": member.unavailable_accounts.iter().collect::<BTreeSet<_>>(),
                "excluded_vendors": member.excluded_vendors.iter().collect::<BTreeSet<_>>(),
            })).collect::<Vec<_>>(),
            "caller_seat_binding_id": self.caller.seat_binding_id.to_string(),
            "caller_occupancy_generation": self.caller.occupancy_generation,
        })
    }
}

/// One member's finding or answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Contribution<'a> {
    /// The project.
    pub project_id: ProjectId,
    /// The run.
    pub run_id: ConsultationRunId,
    /// Which round it contributes to.
    pub round: PlanningPairRound,
    /// The member's own slot.
    pub slot: PlanningPairSlot,
    /// The advice.
    pub advice: &'a BoundedText,
    /// The member's seat and generation.
    pub member: SeatGeneration,
}

impl Contribution<'_> {
    /// The fingerprint document.
    #[must_use]
    pub fn document(&self) -> serde_json::Value {
        let operation = match self.round {
            PlanningPairRound::Findings => "record_planning_pair_finding",
            PlanningPairRound::Clarification => "record_planning_pair_answer",
        };
        serde_json::json!({
            "schema_version": 1,
            "operation": operation,
            "project": self.project_id.to_string(),
            "run": self.run_id.as_text(),
            "slot": self.slot.as_str(),
            "advice": self.advice.as_str(),
            "member_seat_binding_id": self.member.seat_binding_id.to_string(),
            "member_occupancy_generation": self.member.occupancy_generation,
        })
    }
}

/// The caller's one clarification request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clarification<'a> {
    /// The project.
    pub project_id: ProjectId,
    /// The run.
    pub run_id: ConsultationRunId,
    /// The question.
    pub question: &'a BoundedText,
    /// The members it is addressed to, in request order.
    pub addressed: &'a [PlanningPairSlot],
    /// The caller's seat and generation.
    pub caller: SeatGeneration,
}

impl Clarification<'_> {
    /// The fingerprint document.
    #[must_use]
    pub fn document(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "operation": "request_planning_pair_clarification",
            "project": self.project_id.to_string(),
            "run": self.run_id.as_text(),
            "question": self.question.as_str(),
            "addressed": self.addressed.iter().map(|slot| slot.as_str()).collect::<Vec<_>>(),
            "caller_seat_binding_id": self.caller.seat_binding_id.to_string(),
            "caller_occupancy_generation": self.caller.occupancy_generation,
        })
    }
}

/// The caller's disposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Disposition<'a> {
    /// The project.
    pub project_id: ProjectId,
    /// The run.
    pub run_id: ConsultationRunId,
    /// The disposition, as the domain records it.
    pub disposition: &'a PlanningPairDisposition,
    /// The caller's seat and generation.
    pub caller: SeatGeneration,
}

impl Disposition<'_> {
    /// The fingerprint document.
    #[must_use]
    pub fn document(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "operation": "record_planning_pair_disposition",
            "project": self.project_id.to_string(),
            "run": self.run_id.as_text(),
            "disposition": self.disposition,
            "caller_seat_binding_id": self.caller.seat_binding_id.to_string(),
            "caller_occupancy_generation": self.caller.occupancy_generation,
        })
    }
}

/// The caller's same-native recovery of one member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recovery<'a> {
    /// The project.
    pub project_id: ProjectId,
    /// The run.
    pub run_id: ConsultationRunId,
    /// The member seat being recovered.
    pub member_seat_binding_id: SeatBindingId,
    /// That member's slot.
    pub slot: PlanningPairSlot,
    /// The caller's seat and generation.
    pub caller: SeatGeneration,
    /// The run revision the caller read.
    pub expected_run_revision: AggregateRevision,
    /// The member generation the caller read.
    pub expected_member_occupancy_generation: u64,
    /// The native session the caller asserts.
    pub expected_native_identity: &'a NativeRuntimeIdentity,
    /// The provider conversation the caller asserts, if any.
    pub expected_provider_session_id: Option<&'a ExternalId>,
    /// The member's frozen context hash, as derived now.
    pub member_context_hash: &'a ContentHash,
    /// The frozen placement's hash.
    pub placement_hash: &'a ContentHash,
}

impl Recovery<'_> {
    /// The fingerprint document.
    #[must_use]
    pub fn document(&self) -> serde_json::Value {
        let expected = self.expected_native_identity;
        serde_json::json!({
            "schema_version": 1,
            "operation": "recover_planning_pair_seat",
            "project": self.project_id.to_string(),
            "run": self.run_id.as_text(),
            "member_seat_binding_id": self.member_seat_binding_id.to_string(),
            "slot": self.slot.as_str(),
            "caller_seat_binding_id": self.caller.seat_binding_id.to_string(),
            "caller_occupancy_generation": self.caller.occupancy_generation,
            "expected_run_revision": self.expected_run_revision.get(),
            "expected_member_occupancy_generation": self.expected_member_occupancy_generation,
            "expected_native_identity": {
                "runtime_kind": expected.runtime_kind.as_str(),
                "host": expected.host.as_str(),
                "generation": expected.generation,
                "native_id": expected.native_id.as_str(),
            },
            "expected_provider_session_id": self
                .expected_provider_session_id
                .map(ExternalId::as_str),
            "member_context_hash": self.member_context_hash.as_str(),
            "placement_hash": self.placement_hash.as_str(),
        })
    }
}

#[cfg(test)]
mod tests;
