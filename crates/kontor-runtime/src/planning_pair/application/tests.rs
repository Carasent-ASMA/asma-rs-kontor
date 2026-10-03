//! Ordered owner-port traces of the invocation coordinator, over a
//! hypothetical owner. These prove the sequence and its decisions; they
//! qualify nothing. The daemon's loopback suite is the trusted path.

mod fake;

use super::*;
use fake::{Fake, Scenario};
use kontor_core::spec::ModelRung;

fn id(tail: &str) -> String {
    format!("01991c00-0000-7000-8000-0000000000{tail}")
}

fn at() -> Timestamp {
    "2026-10-02T10:00:00Z".parse().expect("an instant")
}

fn project() -> ProjectId {
    ProjectId::parse(&id("f1")).expect("a project")
}

fn epic() -> MiniProjectId {
    MiniProjectId::parse(&id("e1")).expect("an epic")
}

fn spec() -> PlanningPairSpec {
    let member = |slot: &str| {
        serde_json::json!({
            "slot": slot, "role_code": "SA", "specialty": "planning",
            "behavior": "Give one finding; change nothing.",
            "context": {"skills": [], "files": [], "memory": "none"},
        })
    };
    serde_json::from_value(serde_json::json!({
        "schema_version": 1, "protocol": "planning_pair@1", "profile_id": id("b1"),
        "version": 1, "name": "Planning pair",
        "charter": "Is this plan the smallest sound next step?", "container_kind": "PPW",
        "members": [member("seat-a"), member("seat-b")],
        "allowed_caller_roles": ["lsa"], "allowed_scopes": ["epic"],
        "budget": {"max_tokens": 1000, "max_commands": 4, "max_duration_seconds": 60,
                   "max_cost": {"minor_units": 100, "currency": "NOK"}},
    }))
    .expect("a planning pair document")
}

fn members() -> PlanningPairMembers {
    let member = |slot: PlanningPairSlot, model: &str, vendor: &str| PlanningPairMember {
        slot,
        binding_key: format!("pair/{}", slot.as_str()),
        route: ModelRung {
            provider: kontor_core::spec::ProviderRef("fake".to_owned()),
            model: kontor_core::spec::ModelRef(model.to_owned()),
            effort: None,
        },
        vendor: vendor.to_owned(),
    };
    PlanningPairMembers::freeze(
        ContentHash::of(b"placement"),
        vec![
            member(PlanningPairSlot::SeatA, "model-a", "openai"),
            member(PlanningPairSlot::SeatB, "model-b", "anthropic"),
        ],
    )
    .expect("two members")
}

/// A run this key froze, materializing, under `intent_hash`.
fn run(intent_hash: &ContentHash, state: ConsultationRunState) -> StoredConsultationRun {
    StoredConsultationRun {
        id: ConsultationRunId::PlanningPair(PlanningPairRunId::parse(&id("a9")).expect("a run")),
        project_id: project(),
        mini_project_id: epic(),
        profile_id: id("b1"),
        profile_version: SpecVersion::FIRST,
        definition_hash: ContentHash::of(b"document"),
        semantic_identity_hash: None,
        subject: None,
        topic: None,
        question: BoundedText::parse("Is the plan sound?").expect("a question"),
        question_hash: ContentHash::of(b"question"),
        context: serde_json::json!({}),
        context_hash: ContentHash::of(b"context"),
        caller_seat_binding_id: SeatBindingId::parse(&id("a1")).expect("a caller"),
        topology_node_id: TopologyNodeId::parse(&id("c1")).expect("a node"),
        invoke_key: IdempotencyKey::parse("pair-invoke").expect("a key"),
        invoke_intent_hash: intent_hash.clone(),
        state,
        round: 1,
        result: None,
        result_hash: None,
        revision: AggregateRevision::INITIAL,
        created_at: at(),
        updated_at: at(),
        settled_at: None,
    }
}

fn pair_state(run: &StoredConsultationRun, bound: bool) -> PairState {
    let spec = spec();
    let pair = PlanningPairRun::admit(&spec, members(), run.question.clone()).expect("a pair");
    let record = PlanningPairRecord::admitted(&pair);
    let seat = |slot: &str, tail: &str| StoredConsultationSeat {
        run_id: run.id,
        role_slot_id: RoleSlotId::parse(slot).expect("a slot"),
        committee_role: None,
        logical_role: kontor_core::id::RoleKey::parse("planning_pair_member").expect("a role"),
        seat_binding_id: SeatBindingId::parse(&id(tail)).expect("a seat"),
        model_rung: members().members()[0].route.clone(),
        occupancy_generation: 1,
        native_identity: bound.then(|| NativeRuntimeIdentity {
            runtime_kind: kontor_core::id::RuntimeKindKey::parse("fake.runtime").expect("a kind"),
            host: ExternalName::parse("fake-host").expect("a host"),
            generation: 1,
            native_id: ExternalId::parse(&format!("native-{tail}")).expect("a native"),
        }),
        provider_session_id: None,
        observed_at: None,
    };
    PairState {
        run: run.clone(),
        revision: StoredConsultationProfileRevision {
            project_id: project(),
            family: ConsultationFamily::PlanningPair,
            profile_id: id("b1"),
            version: SpecVersion::FIRST,
            name: ExternalName::parse("Planning pair").expect("a name"),
            definition: "{}".to_owned(),
            definition_hash: ContentHash::of(b"document"),
            published_at: at(),
        },
        spec,
        placement: StoredPlanningPairPlacement {
            run_id: run.id,
            project_id: project(),
            placement: CanonicalDocument::from_value(
                &serde_json::json!({"schema_version": 1, "selection": {}}),
            )
            .expect("a placement"),
            created_at: at(),
        },
        record_revision: AggregateRevision::INITIAL,
        record,
        pair,
        seats: vec![seat("seat-a", "a3"), seat("seat-b", "a4")],
        known: Vec::new(),
    }
}

async fn invoked(scenario: Scenario) -> (Result<(CommandReceiptId, Applied), String>, Vec<String>) {
    let fake = Fake::new(scenario);
    let question = BoundedText::parse("Is the plan sound?").expect("a question");
    let topic = ExternalName::parse("Pair plan").expect("a topic");
    let hash = ContentHash::of(b"document");
    let input = InvokeInput {
        profile_id: &id("b1"),
        profile_version: SpecVersion::FIRST,
        definition_hash: &hash,
        topic: &topic,
        question: &question,
        task_id: None,
        members: &[],
        expected_revision: AggregateRevision::INITIAL,
    };
    let key = IdempotencyKey::parse("pair-invoke").expect("a key");
    let result = invoke(&fake, &key, project(), epic(), 1, &input).await;
    (result, fake.trace())
}

/// The prefix every invocation shares: the guard, the document, the caller
/// authorized before anything else, then the intent and its replay.
const PREFIX: [&str; 6] = [
    "begin_native_activity",
    "pinned_document",
    "authorize_caller",
    "presented",
    "canonical",
    "replayed",
];

fn expect(trace: &[String], tail: &[&str]) {
    let wanted: Vec<&str> = PREFIX.iter().copied().chain(tail.iter().copied()).collect();
    assert_eq!(trace, wanted.as_slice());
}

#[tokio::test]
async fn an_exact_replay_is_authorized_first_and_answers_its_receipt_unchanged() {
    let (result, trace) = invoked(Scenario {
        replay: true,
        existing: Some(false),
        ..Scenario::default()
    })
    .await;
    expect(&trace, &["run_by_key", "pair_state", "render:Unchanged"]);
    assert_eq!(result.expect("a replay").1, Applied::Unchanged);
}

#[tokio::test]
async fn an_unauthorized_caller_never_reaches_the_intent_or_the_replay() {
    let (result, trace) = invoked(Scenario {
        unauthorized: true,
        replay: true,
        ..Scenario::default()
    })
    .await;
    assert_eq!(result, Err("unauthorized".to_owned()));
    assert_eq!(
        trace,
        [
            "begin_native_activity",
            "pinned_document",
            "authorize_caller"
        ]
    );
}

#[tokio::test]
async fn a_moved_document_refuses_before_the_caller_is_authorized() {
    let (result, trace) = invoked(Scenario {
        hash_moved: true,
        ..Scenario::default()
    })
    .await;
    assert_eq!(result, Err("refuse:DocumentHashDiffers".to_owned()));
    assert_eq!(
        trace,
        [
            "begin_native_activity",
            "pinned_document",
            "refuse:DocumentHashDiffers"
        ]
    );
}

#[tokio::test]
async fn a_receipt_without_its_run_refuses() {
    let (result, trace) = invoked(Scenario {
        replay: true,
        receipt_without_run: true,
        ..Scenario::default()
    })
    .await;
    assert_eq!(result, Err("refuse:ReceiptWithoutRun".to_owned()));
    expect(&trace, &["run_by_key", "refuse:ReceiptWithoutRun"]);
}

#[tokio::test]
async fn a_key_reused_for_another_consultation_refuses_before_any_freeze_or_launch() {
    let (result, trace) = invoked(Scenario {
        existing: Some(true),
        ..Scenario::default()
    })
    .await;
    assert_eq!(result, Err("refuse:KeyReused".to_owned()));
    expect(&trace, &["run_by_key", "refuse:KeyReused"]);
}

#[tokio::test]
async fn a_moved_epic_refuses_before_any_freeze() {
    let (result, trace) = invoked(Scenario {
        epic_moved: true,
        ..Scenario::default()
    })
    .await;
    assert!(result.expect_err("refused").starts_with("refuse:EpicMoved"));
    expect(
        &trace,
        &[
            "run_by_key",
            "epic",
            "refuse:EpicMoved { current: AggregateRevision(2) }",
        ],
    );
}

#[tokio::test]
async fn a_new_pair_freezes_only_after_the_epic_check() {
    let (result, trace) = invoked(Scenario::default()).await;
    assert_eq!(result, Err("refuse:NoPinnedTeamDefinition".to_owned()));
    expect(
        &trace,
        &[
            "run_by_key",
            "epic",
            "pinned_team_definition",
            "refuse:NoPinnedTeamDefinition",
        ],
    );
}

const RESUMED: [&str; 8] = [
    "run_by_key",
    "pair_state",
    "project",
    "topology_node",
    "runtime",
    "require_member_routes",
    "epic_tracker_key",
    "consultation_root",
];

#[tokio::test]
async fn a_resumed_pair_with_both_members_bound_launches_nothing_and_classifies_its_receipt() {
    for (inserted, applied) in [(true, "render:Created"), (false, "render:Unchanged")] {
        let (result, trace) = invoked(Scenario {
            existing: Some(false),
            inserted,
            ..Scenario::default()
        })
        .await;
        let tail: Vec<&str> = RESUMED
            .iter()
            .copied()
            .chain([
                "advance_to_running",
                "receipt_hold_point",
                "record:invoke_planning_pair_run",
                "pair_state",
                applied,
            ])
            .collect();
        expect(&trace, &tail);
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn a_route_the_runtime_cannot_compose_refuses_before_any_container_or_launch() {
    let (result, trace) = invoked(Scenario {
        existing: Some(false),
        routes_refused: true,
        ..Scenario::default()
    })
    .await;
    assert_eq!(result, Err("route refused".to_owned()));
    expect(&trace, &RESUMED[..6]);
}

#[tokio::test]
async fn a_lost_advance_answers_the_winners_row_and_a_still_materializing_run_refuses() {
    let (result, trace) = invoked(Scenario {
        existing: Some(false),
        advance_lost: Some(ConsultationRunState::Running),
        inserted: false,
        ..Scenario::default()
    })
    .await;
    assert!(result.is_ok());
    let tail: Vec<&str> = RESUMED
        .iter()
        .copied()
        .chain([
            "advance_to_running",
            "stored_run",
            "receipt_hold_point",
            "record:invoke_planning_pair_run",
            "pair_state",
            "render:Unchanged",
        ])
        .collect();
    expect(&trace, &tail);

    let (result, trace) = invoked(Scenario {
        existing: Some(false),
        advance_lost: Some(ConsultationRunState::Materializing),
        ..Scenario::default()
    })
    .await;
    assert_eq!(result, Err("advance lost".to_owned()));
    let tail: Vec<&str> = RESUMED
        .iter()
        .copied()
        .chain(["advance_to_running", "stored_run"])
        .collect();
    expect(&trace, &tail);
}

#[tokio::test]
async fn a_running_pair_is_not_advanced_again() {
    let (result, trace) = invoked(Scenario {
        existing: Some(false),
        running_already: true,
        inserted: true,
        ..Scenario::default()
    })
    .await;
    assert!(result.is_ok());
    let tail: Vec<&str> = RESUMED
        .iter()
        .copied()
        .chain([
            "receipt_hold_point",
            "record:invoke_planning_pair_run",
            "pair_state",
            "render:Created",
        ])
        .collect();
    expect(&trace, &tail);
}
