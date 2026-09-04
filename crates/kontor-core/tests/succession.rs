//! Pure succession state-machine and handoff evidence behavior.

use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ContentHash, EventCursor, ExternalId,
    ExternalName, IdempotencyKey, ProjectId, QuotaObservationProvenanceId, RoleKey,
    RuntimeBindingId, RuntimeKindKey, SuccessionAttemptId, TaskId, TeamRunId, Timestamp,
};
use kontor_core::spec::{ModelRef, ModelRung, ProviderRef};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::succession::{
    NewSuccessionAttempt, SuccessionAttemptState, SuccessionHandoff,
    SuccessionHandoffDegradedReason, SuccessionHandoffOutcome, SuccessionRedactionPass,
    SuccessionRedactionReceipt, SuccessionTimelineRange,
};

fn at(text: &str) -> Timestamp {
    text.parse().expect("valid timestamp")
}

fn identity(native_id: &str, generation: u64) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo").expect("runtime kind"),
        host: ExternalName::parse("local").expect("host"),
        generation,
        native_id: ExternalId::parse(native_id).expect("native id"),
    }
}

fn request(deferred_until: Option<Timestamp>) -> NewSuccessionAttempt {
    NewSuccessionAttempt {
        id: SuccessionAttemptId::generate(),
        project_id: ProjectId::generate(),
        task_id: TaskId::generate(),
        team_run_id: TeamRunId::generate(),
        role: RoleKey::parse("aud").expect("role"),
        predecessor_agent_run_id: AgentRunId::generate(),
        predecessor_runtime_binding_id: RuntimeBindingId::generate(),
        predecessor_native_identity: identity("predecessor", 4),
        expected_task_revision: AggregateRevision::INITIAL,
        expected_team_revision: AggregateRevision::INITIAL,
        expected_predecessor_revision: AggregateRevision::INITIAL,
        runtime_observation_cursor: EventCursor::parse(41).expect("cursor"),
        quota_provenance_id: QuotaObservationProvenanceId::generate(),
        quota_state_revision: AggregateRevision::INITIAL,
        quota_evidence_hash: ContentHash::of(b"quota evidence"),
        quota_provider: "openai".to_owned(),
        successor_model_rung: deferred_until.is_none().then(|| ModelRung {
            provider: ProviderRef("anthropic".to_owned()),
            model: ModelRef("claude-sonnet".to_owned()),
            effort: None,
        }),
        successor_account_profile_id: deferred_until.is_none().then(AccountProfileId::generate),
        idempotency_key: IdempotencyKey::parse("succession:one").expect("key"),
        intent_hash: ContentHash::of(b"intent"),
        deferred_until,
        created_at: at("2026-09-04T08:00:00Z"),
    }
}

fn redaction(pass: SuccessionRedactionPass) -> SuccessionRedactionReceipt {
    SuccessionRedactionReceipt {
        schema_version: 1,
        pass,
        source_hash: ContentHash::of(b"source"),
        redacted_hash: ContentHash::of(b"redacted"),
        policy_hash: ContentHash::of(b"policy"),
        redacted_at: at("2026-09-04T08:01:00Z"),
    }
}

#[test]
fn placement_wait_is_the_only_way_an_attempt_starts_deferred() {
    let planned = request(None);
    assert_eq!(
        planned.initial_state().expect("planned request"),
        SuccessionAttemptState::Planned
    );

    let deferred = request(Some(at("2026-09-04T09:00:00Z")));
    assert_eq!(
        deferred.initial_state().expect("deferred request"),
        SuccessionAttemptState::Deferred
    );
    assert!(deferred.successor_model_rung.is_none());
    assert!(deferred.successor_account_profile_id.is_none());

    let invalid = request(Some(at("2026-09-04T08:00:00Z")));
    assert!(invalid.initial_state().is_err());
}

#[test]
fn the_succession_state_machine_is_forward_only() {
    use SuccessionAttemptState::{
        Confirmed, Deferred, Planned, PredecessorRetired, Refused, SuccessorObserved,
    };

    assert!(Planned.can_advance_to(PredecessorRetired));
    assert!(Deferred.can_advance_to(Planned));
    assert!(!Deferred.can_advance_to(PredecessorRetired));
    assert!(PredecessorRetired.can_advance_to(SuccessorObserved));
    assert!(SuccessorObserved.can_advance_to(Confirmed));
    assert!(Planned.can_advance_to(Refused));
    assert!(!SuccessorObserved.can_advance_to(PredecessorRetired));
    assert!(!Confirmed.can_advance_to(Refused));
    assert!(!Refused.can_advance_to(Planned));
}

#[test]
fn a_degraded_handoff_is_durable_evidence_not_a_release_refusal() {
    let request = request(None);
    let handoff = SuccessionHandoff {
        schema_version: 1,
        attempt_id: request.id,
        predecessor_agent_run_id: request.predecessor_agent_run_id,
        predecessor_runtime_binding_id: request.predecessor_runtime_binding_id,
        predecessor_native_identity: request.predecessor_native_identity.clone(),
        outcome: SuccessionHandoffOutcome::Degraded {
            timeline: Some(SuccessionTimelineRange {
                epoch: 2,
                start_sequence: 7,
                end_sequence: 12,
            }),
            reason: SuccessionHandoffDegradedReason::SummarizerUnplaceable,
            input_redaction: redaction(SuccessionRedactionPass::Input),
            output_redaction: redaction(SuccessionRedactionPass::Output),
        },
        produced_at: at("2026-09-04T08:02:00Z"),
    };

    let document = handoff.canonicalize().expect("valid degraded handoff");
    assert_eq!(document.hash(), &handoff.hash().expect("handoff hash"));
    assert_eq!(handoff.summary_hash(), None);
}
