//! Hypothetical facts, as if the service had read them. The trusted path is
//! proved by the daemon's loopback tests; these prove the pure decisions.

use super::*;
use crate::planning_pair::tests::{identity, matched_of, request};
use kontor_core::consultation::ConsultationRunId;
use kontor_core::id::{
    AggregateRevision, BoundedText, IdempotencyKey, MiniProjectId, ProjectId, RoleKey, RoleSlotId,
    RuntimeKindKey, SeatBindingId, Timestamp, TopologyNodeId,
};

fn at(text: &str) -> Timestamp {
    text.parse().expect("an instant")
}

fn session(text: &str) -> ExternalId {
    ExternalId::parse(text).expect("a provider session")
}

/// One member, bound to `native-member-b` with conversation `provider-1`, of
/// the run [`request`] names, and its claim launched under `member context`.
struct Fixture {
    request: PlanningPairMemberReconcileRequest,
    run: StoredConsultationRun,
    seat: StoredConsultationSeat,
    claim: StoredPlanningPairKnownNative,
    context_hash: ContentHash,
    placement_hash: ContentHash,
}

fn fixture() -> Fixture {
    let request = request();
    let context = &request.context;
    let run_id = ConsultationRunId::PlanningPair(context.run_id);
    let project_id = ProjectId::generate();
    let run = StoredConsultationRun {
        id: run_id,
        project_id,
        mini_project_id: MiniProjectId::generate(),
        profile_id: context.profile.profile_id.to_string(),
        profile_version: context.profile.version,
        definition_hash: context.profile.definition_hash.clone(),
        semantic_identity_hash: None,
        subject: None,
        topic: None,
        question: BoundedText::parse("Is this plan sound?").expect("a question"),
        question_hash: ContentHash::of(b"question"),
        context: serde_json::json!({}),
        context_hash: ContentHash::of(b"run context"),
        caller_seat_binding_id: SeatBindingId::generate(),
        topology_node_id: TopologyNodeId::generate(),
        invoke_key: IdempotencyKey::parse("pair-invoke").expect("a key"),
        invoke_intent_hash: ContentHash::of(b"intent"),
        state: ConsultationRunState::Running,
        round: 1,
        result: None,
        result_hash: None,
        revision: AggregateRevision::INITIAL,
        created_at: at("2026-10-02T09:00:00Z"),
        updated_at: at("2026-10-02T09:01:00Z"),
        settled_at: None,
    };
    let seat = StoredConsultationSeat {
        run_id,
        role_slot_id: RoleSlotId::parse("seat-b").expect("a slot"),
        committee_role: None,
        logical_role: RoleKey::parse("planning_pair_member").expect("a role"),
        seat_binding_id: context.seat_binding_id,
        model_rung: context.route.clone(),
        occupancy_generation: context.occupancy_generation,
        native_identity: Some(identity("native-member-b")),
        provider_session_id: Some(session("provider-1")),
        observed_at: Some(at("2026-10-02T09:05:00Z")),
    };
    let context_hash = ContentHash::of(b"member context");
    let claim = StoredPlanningPairKnownNative {
        run_id,
        project_id,
        seat_binding_id: seat.seat_binding_id,
        occupancy_generation: seat.occupancy_generation,
        identity: identity("native-member-b"),
        provider_session_id: Some(session("provider-1")),
        context_hash: context_hash.clone(),
        placement_hash: context.placement_hash.clone(),
        readback_refusal: Some(PlanningPairReadbackRefusal::CorrelationUnobserved),
        observed_at: at("2026-10-02T09:03:00Z"),
    };
    let placement_hash = context.placement_hash.clone();
    Fixture {
        request,
        run,
        seat,
        claim,
        context_hash,
        placement_hash,
    }
}

impl Fixture {
    /// The plan for `seat` and `known`, asserting `generation`, `native` and
    /// `conversation`.
    fn plan_for(
        &self,
        seat: &StoredConsultationSeat,
        known: &[StoredPlanningPairKnownNative],
        generation: u64,
        native: &NativeRuntimeIdentity,
        conversation: Option<&ExternalId>,
    ) -> Result<RecoveryPlan, RecoveryRefusal> {
        plan(
            RecoveryFacts {
                run: &self.run,
                seat,
                known,
                context_hash: &self.context_hash,
                placement_hash: &self.placement_hash,
            },
            RecoveryAssertion {
                member_occupancy_generation: generation,
                native_identity: native,
                provider_session_id: conversation,
            },
        )
    }

    /// The exact assertion of the fixture's own known session.
    fn exact(
        &self,
        known: &[StoredPlanningPairKnownNative],
    ) -> Result<RecoveryPlan, RecoveryRefusal> {
        let native = identity("native-member-b");
        let conversation = session("provider-1");
        self.plan_for(&self.seat, known, 2, &native, Some(&conversation))
    }

    fn unbound(&self) -> StoredConsultationSeat {
        StoredConsultationSeat {
            native_identity: None,
            provider_session_id: None,
            ..self.seat.clone()
        }
    }
}

#[test]
fn only_a_disposed_pair_refuses_a_recovery() {
    for state in [
        ConsultationRunState::Materializing,
        ConsultationRunState::Running,
        ConsultationRunState::AwaitingJudge,
        ConsultationRunState::Settled,
        ConsultationRunState::NeedsHuman,
    ] {
        assert_eq!(require_recoverable(state), Ok(()), "{state:?}");
    }
    assert_eq!(
        require_recoverable(ConsultationRunState::Disposed),
        Err(DomainError::Terminal {
            subject: "PlanningPairRun"
        })
    );
}

#[test]
fn a_request_is_held_to_the_known_session_in_order() {
    let f = fixture();
    let claims = [f.claim.clone()];
    let native = identity("native-member-b");
    let conversation = session("provider-1");
    assert!(f.exact(&claims).is_ok());

    let elsewhere = identity("native-other");
    assert_eq!(
        f.plan_for(&f.seat, &claims, 3, &elsewhere, None),
        Err(RecoveryRefusal::MemberGenerationMoved),
        "the generation comes first"
    );
    assert_eq!(
        f.plan_for(&f.unbound(), &[], 2, &elsewhere, None),
        Err(RecoveryRefusal::NoKnownSession),
        "no known session comes before its identity"
    );

    let mut kind = native.clone();
    kind.runtime_kind = RuntimeKindKey::parse("paseo").expect("a runtime kind");
    let mut host = native.clone();
    host.host = kontor_core::id::ExternalName::parse("another-host").expect("a host");
    let mut generation = native.clone();
    generation.generation += 1;
    for other in [elsewhere, kind, host, generation] {
        assert_eq!(
            f.plan_for(&f.seat, &claims, 2, &other, None),
            Err(RecoveryRefusal::NativeSessionDiffers),
            "{other:?}: the session comes before its conversation"
        );
    }

    let mut drifted = f.claim.clone();
    drifted.context_hash = ContentHash::of(b"another context");
    let other_conversation = session("provider-2");
    for asserted in [None, Some(&other_conversation)] {
        assert_eq!(
            f.plan_for(&f.seat, &[drifted.clone()], 2, &native, asserted),
            Err(RecoveryRefusal::ProviderConversationDiffers),
            "{asserted:?}: the conversation comes before the context"
        );
    }
    let mut silent = f.unbound();
    silent.native_identity = Some(native.clone());
    assert_eq!(
        f.plan_for(&silent, &claims, 2, &native, Some(&conversation)),
        Err(RecoveryRefusal::ProviderConversationDiffers),
        "a known session without a conversation is not asserted with one"
    );
    assert_eq!(
        f.plan_for(&f.seat, &[drifted], 2, &native, Some(&conversation)),
        Err(RecoveryRefusal::ContextDiffers)
    );
}

#[test]
fn the_known_session_is_the_bound_seats_and_else_its_claim() {
    let f = fixture();
    let native = identity("native-member-b");
    let conversation = session("provider-1");

    // A bound seat is its own session; the claim only holds the context.
    let mut stale_claim = f.claim.clone();
    stale_claim.identity = identity("native-claimed");
    let bound = f
        .exact(&[stale_claim.clone()])
        .expect("the bound seat's session");
    assert_eq!(bound.identity(), &native);
    assert!(bound.holds_qualification());
    let claimed = identity("native-claimed");
    assert_eq!(
        f.plan_for(&f.seat, &[stale_claim], 2, &claimed, Some(&conversation)),
        Err(RecoveryRefusal::NativeSessionDiffers)
    );

    // A bound seat's conversation is its own, even beside another claimed one.
    let mut other_claimed = f.claim.clone();
    other_claimed.provider_session_id = Some(session("provider-claimed"));
    let own = f
        .exact(std::slice::from_ref(&other_claimed))
        .expect("the bound seat's conversation");
    assert_eq!(own.provider_session_id(), Some(&conversation));

    // An unbound seat is its kept claim's session and conversation.
    let claimed = f
        .plan_for(
            &f.unbound(),
            std::slice::from_ref(&f.claim),
            2,
            &native,
            Some(&conversation),
        )
        .expect("the claim's session and conversation");
    assert_eq!(claimed.identity(), &native);
    assert_eq!(claimed.provider_session_id(), Some(&conversation));
    // An unbound seat is its kept claim's session and conversation.
    let mut quiet_claim = f.claim.clone();
    quiet_claim.provider_session_id = None;
    let kept = f
        .plan_for(&f.unbound(), &[quiet_claim], 2, &native, None)
        .expect("the claim's session");
    assert_eq!(kept.identity(), &native);
    assert_eq!(kept.provider_session_id(), None);
    assert!(!kept.holds_qualification());

    // Another member's claim is not this member's, and is never checked.
    let mut peer = f.claim.clone();
    peer.seat_binding_id = SeatBindingId::generate();
    peer.context_hash = ContentHash::of(b"the peer's context");
    let without = f.exact(&[peer.clone()]).expect("no claim of its own");
    assert_eq!(without.verified().readback_refusal, None);
    assert_eq!(
        f.plan_for(&f.unbound(), &[peer], 2, &native, None),
        Err(RecoveryRefusal::NoKnownSession)
    );
}

#[test]
fn the_verified_claim_is_built_from_the_facts_read() {
    let f = fixture();
    let plan = f.exact(std::slice::from_ref(&f.claim)).expect("a plan");
    assert_eq!(
        plan.verified(),
        &StoredPlanningPairKnownNative {
            run_id: f.run.id,
            project_id: f.run.project_id,
            seat_binding_id: f.seat.seat_binding_id,
            occupancy_generation: 2,
            identity: identity("native-member-b"),
            provider_session_id: Some(session("provider-1")),
            context_hash: f.context_hash.clone(),
            placement_hash: f.placement_hash.clone(),
            readback_refusal: Some(PlanningPairReadbackRefusal::CorrelationUnobserved),
            observed_at: f.claim.observed_at,
        }
    );
    assert_eq!(plan.provider_session_id(), Some(&session("provider-1")));

    // Observed when the claim was, else when the seat was, else at the run.
    let unclaimed = f.exact(&[]).expect("a plan");
    assert_eq!(unclaimed.verified().observed_at, at("2026-10-02T09:05:00Z"));
    assert_eq!(unclaimed.verified().readback_refusal, None);
    let mut unobserved = f.seat.clone();
    unobserved.observed_at = None;
    let native = identity("native-member-b");
    let conversation = session("provider-1");
    let at_run = f
        .plan_for(&unobserved, &[], 2, &native, Some(&conversation))
        .expect("a plan");
    assert_eq!(at_run.verified().observed_at, f.run.updated_at);
}

#[test]
fn the_runtime_answer_is_classified_as_before() {
    let f = fixture();
    let plan = f.exact(std::slice::from_ref(&f.claim)).expect("a plan");
    let context = &f.request.context;
    let matched = || matched_of(context, "native-member-b");
    let outcome = |answer| plan.outcome(&f.request, answer);

    assert!(matches!(
        outcome(Err(RuntimeError::StaleBinding { rule: "stopped" })),
        RecoveryOutcome::Withdraw(WithdrawReason::Runtime("stopped"))
    ));
    assert!(matches!(
        outcome(Err(RuntimeError::CorrelationFailed)),
        RecoveryOutcome::Withdraw(WithdrawReason::CorrelationLost)
    ));
    assert!(matches!(
        outcome(Err(RuntimeError::LaunchNotAdmitted {
            rule: "unsupported"
        })),
        RecoveryOutcome::NoObservation(RuntimeError::LaunchNotAdmitted {
            rule: "unsupported"
        })
    ));
    match outcome(Ok(matched())) {
        RecoveryOutcome::Requalify(requalified) => assert_eq!(*requalified, matched()),
        other => panic!("a matched readback requalifies: {other:?}"),
    }

    let mut created = matched();
    created.created = true;
    created.provider_session_id = None;
    let mut other_native = matched();
    other_native.identity = identity("native-other");
    for answer in [created, other_native] {
        assert!(
            matches!(
                outcome(Ok(answer.clone())),
                RecoveryOutcome::Withdraw(WithdrawReason::NotTheKnownSession)
            ),
            "{answer:?}: the same session comes before its conversation"
        );
    }
    let mut silent = matched();
    silent.provider_session_id = None;
    silent.planning_pair = None;
    let mut moved = matched();
    moved.provider_session_id = Some(session("provider-2"));
    for answer in [silent, moved] {
        assert!(
            matches!(
                outcome(Ok(answer.clone())),
                RecoveryOutcome::Withdraw(WithdrawReason::AnotherProviderConversation)
            ),
            "{answer:?}: the conversation comes before qualification"
        );
    }

    let mut surfaceless = matched();
    surfaceless.planning_pair = None;
    assert!(matches!(
        outcome(Ok(surfaceless)),
        RecoveryOutcome::Withdraw(WithdrawReason::Unqualified(
            PlanningPairReadbackRefusal::NoMemberSurface
        ))
    ));
    let mut unconfirmed = matched();
    unconfirmed.fleet_provenance = crate::provenance::FleetProvenanceObservation::Unsupported {
        surface: "fixture".to_owned(),
        native_id: ExternalId::parse("native-member-b").expect("a native id"),
    };
    assert!(matches!(
        outcome(Ok(unconfirmed)),
        RecoveryOutcome::Withdraw(WithdrawReason::Unqualified(
            PlanningPairReadbackRefusal::ProvenanceUnconfirmed
        ))
    ));
}

#[test]
fn a_session_without_a_known_conversation_takes_the_one_read_back() {
    let f = fixture();
    let mut quiet_claim = f.claim.clone();
    quiet_claim.provider_session_id = None;
    let native = identity("native-member-b");
    let plan = f
        .plan_for(&f.unbound(), &[quiet_claim], 2, &native, None)
        .expect("a plan");
    let readback = matched_of(&f.request.context, "native-member-b");
    match plan.outcome(&f.request, Ok(readback.clone())) {
        RecoveryOutcome::Requalify(requalified) => assert_eq!(*requalified, readback),
        other => panic!("a readback naming a conversation requalifies: {other:?}"),
    }
    let bound = plan.requalified(&readback);
    assert_eq!(
        bound,
        StoredPlanningPairKnownNative {
            provider_session_id: Some(session("provider-1")),
            readback_refusal: None,
            observed_at: readback.observed_at,
            ..plan.verified().clone()
        }
    );
    assert_ne!(bound.observed_at, plan.verified().observed_at);
    assert_eq!(
        plan.verified().readback_refusal,
        Some(PlanningPairReadbackRefusal::CorrelationUnobserved)
    );
}
