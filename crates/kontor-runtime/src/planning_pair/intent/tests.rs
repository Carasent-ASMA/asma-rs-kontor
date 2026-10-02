//! Hypothetical fixed inputs. Each baseline is the daemon's document at
//! `8cd2305f`, copied verbatim except for where its inputs are read from. The
//! moved document must be exactly that document, and its canonical hash is
//! pinned as a golden.

use super::*;
use kontor_core::consultation::AdviceDisposition;
use kontor_core::id::{CanonicalDocument, PlanningPairRunId, RuntimeKindKey};
use kontor_core::planning_pair::MemberDisposition;

fn id(tail: &str) -> String {
    format!("01991c00-0000-7000-8000-0000000000{tail}")
}

fn project() -> ProjectId {
    ProjectId::parse(&id("f1")).expect("a project id")
}

fn epic() -> MiniProjectId {
    MiniProjectId::parse(&id("e1")).expect("an epic id")
}

fn run() -> ConsultationRunId {
    ConsultationRunId::PlanningPair(PlanningPairRunId::parse(&id("a9")).expect("a run id"))
}

fn caller() -> SeatGeneration {
    SeatGeneration {
        seat_binding_id: SeatBindingId::parse(&id("a1")).expect("a caller"),
        occupancy_generation: 3,
    }
}

fn member() -> SeatGeneration {
    SeatGeneration {
        seat_binding_id: SeatBindingId::parse(&id("a3")).expect("a member"),
        occupancy_generation: 2,
    }
}

fn hash_of(document: &serde_json::Value) -> String {
    CanonicalDocument::from_value(document)
        .expect("a canonical document")
        .hash()
        .as_str()
        .to_owned()
}

fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("a bounded text")
}

/// One requested placement, as the daemon's request carried it.
struct Requested {
    slot: PlanningPairSlot,
    binding_key: String,
    unavailable_accounts: Vec<String>,
    excluded_vendors: Vec<String>,
}

fn requested() -> Vec<Requested> {
    let strings = |values: &[&str]| values.iter().map(|&value| value.to_owned()).collect();
    vec![
        Requested {
            slot: PlanningPairSlot::SeatA,
            binding_key: "committee/reviewer-a".to_owned(),
            unavailable_accounts: strings(&["zz-busy", "aa-busy", "zz-busy"]),
            excluded_vendors: strings(&["xai", "anthropic"]),
        },
        Requested {
            slot: PlanningPairSlot::SeatB,
            binding_key: "advisor/pair".to_owned(),
            unavailable_accounts: Vec::new(),
            excluded_vendors: strings(&["openai"]),
        },
    ]
}

/// The invocation document at `8cd2305f`.
#[allow(
    clippy::too_many_arguments,
    reason = "the baseline reads each input where the daemon read it"
)]
fn baseline_invoke(
    project_id: ProjectId,
    epic_id: MiniProjectId,
    profile_id: &str,
    version: SpecVersion,
    definition_hash: &ContentHash,
    topic: &ExternalName,
    question: &BoundedText,
    task_id: Option<TaskId>,
    members: &[Requested],
    caller: SeatGeneration,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "operation": "invoke_planning_pair_run",
        "project": project_id.to_string(),
        "epic": epic_id.to_string(),
        "protocol": ConsultationProtocol::PlanningPair.as_str(),
        "profile": [
            profile_id,
            version.get(),
            definition_hash.as_str(),
        ],
        "topic": topic.as_str(),
        "question": question.as_str(),
        "task_id": task_id.map(|id| id.to_string()),
        "members": members.iter().map(|member| serde_json::json!({
            "slot": member.slot.as_str(),
            "binding_key": member.binding_key,
            "unavailable_accounts": member.unavailable_accounts.iter().collect::<BTreeSet<_>>(),
            "excluded_vendors": member.excluded_vendors.iter().collect::<BTreeSet<_>>(),
        })).collect::<Vec<_>>(),
        "caller_seat_binding_id": caller.seat_binding_id.to_string(),
        "caller_occupancy_generation": caller.occupancy_generation,
    })
}

#[test]
fn an_invocation_is_the_baseline_document() {
    let requested = requested();
    let members: Vec<InvokeMember<'_>> = requested
        .iter()
        .map(|member| InvokeMember {
            slot: member.slot,
            binding_key: &member.binding_key,
            unavailable_accounts: &member.unavailable_accounts,
            excluded_vendors: &member.excluded_vendors,
        })
        .collect();
    let profile_id = id("b1");
    let definition_hash = ContentHash::of(b"planning pair document");
    let topic = ExternalName::parse("Release plan").expect("a topic");
    let question = text("Is this plan the smallest sound next step?");
    let mut hashes = Vec::new();
    for task_id in [None, Some(TaskId::parse(&id("c7")).expect("a task"))] {
        let moved = Invoke {
            project_id: project(),
            epic_id: epic(),
            profile_id: &profile_id,
            profile_version: SpecVersion::FIRST,
            definition_hash: &definition_hash,
            topic: &topic,
            question: &question,
            task_id,
            members: &members,
            caller: caller(),
        }
        .document();
        assert_eq!(
            moved,
            baseline_invoke(
                project(),
                epic(),
                &profile_id,
                SpecVersion::FIRST,
                &definition_hash,
                &topic,
                &question,
                task_id,
                &requested,
                caller(),
            )
        );
        assert_eq!(
            moved["members"][0]["unavailable_accounts"],
            serde_json::json!(["aa-busy", "zz-busy"]),
            "sorted and deduplicated"
        );
        hashes.push(hash_of(&moved));
    }
    assert_eq!(
        hashes,
        [
            "bad794bb562006c876b97eeadcec8e6dbf5915dd040f8b768da75fbab3199ef7",
            "741844c72fe2d4efe07ae752403ebed8424e2ea2105f892f06bbb3fc2ae5a94d",
        ]
    );
}

/// The contribution document at `8cd2305f`.
fn baseline_contribution(
    project_id: ProjectId,
    run: ConsultationRunId,
    round: PlanningPairRound,
    slot: PlanningPairSlot,
    advice: &BoundedText,
    member: SeatGeneration,
) -> serde_json::Value {
    let operation = match round {
        PlanningPairRound::Findings => "record_planning_pair_finding",
        PlanningPairRound::Clarification => "record_planning_pair_answer",
    };
    serde_json::json!({
        "schema_version": 1,
        "operation": operation,
        "project": project_id.to_string(),
        "run": run.as_text(),
        "slot": slot.as_str(),
        "advice": advice.as_str(),
        "member_seat_binding_id": member.seat_binding_id.to_string(),
        "member_occupancy_generation": member.occupancy_generation,
    })
}

#[test]
fn a_finding_and_an_answer_are_their_baseline_documents() {
    let advice = text("Keep it whole.");
    let mut hashes = Vec::new();
    for round in [
        PlanningPairRound::Findings,
        PlanningPairRound::Clarification,
    ] {
        let moved = Contribution {
            project_id: project(),
            run_id: run(),
            round,
            slot: PlanningPairSlot::SeatB,
            advice: &advice,
            member: member(),
        }
        .document();
        assert_eq!(
            moved,
            baseline_contribution(
                project(),
                run(),
                round,
                PlanningPairSlot::SeatB,
                &advice,
                member()
            )
        );
        hashes.push(hash_of(&moved));
    }
    assert_eq!(
        hashes,
        [
            "feb87520fef0a614c2c16f00525c0be7276fcc2459c39235b0b094a48dddaf23",
            "de5998d669863507f100a1c67a266537b07ba7fcad1de3888eea68997e34b98f",
        ]
    );
}

#[test]
fn a_clarification_is_the_baseline_document_in_request_order() {
    let question = text("Which step reverts alone?");
    let addressed = [PlanningPairSlot::SeatB, PlanningPairSlot::SeatA];
    let moved = Clarification {
        project_id: project(),
        run_id: run(),
        question: &question,
        addressed: &addressed,
        caller: caller(),
    }
    .document();
    let baseline = serde_json::json!({
        "schema_version": 1,
        "operation": "request_planning_pair_clarification",
        "project": project().to_string(),
        "run": run().as_text(),
        "question": question.as_str(),
        "addressed": addressed.iter().map(|slot| slot.as_str()).collect::<Vec<_>>(),
        "caller_seat_binding_id": caller().seat_binding_id.to_string(),
        "caller_occupancy_generation": caller().occupancy_generation,
    });
    assert_eq!(moved, baseline);
    assert_eq!(moved["addressed"], serde_json::json!(["seat-b", "seat-a"]));
    assert_eq!(
        hash_of(&moved),
        "73a8ccd16ed9f81613a037937aebe89be5e600d15bcc21478aeae9387401183b"
    );
}

#[test]
fn a_disposition_is_the_baseline_document() {
    let disposition = PlanningPairDisposition {
        members: vec![
            MemberDisposition {
                slot: PlanningPairSlot::SeatA,
                finding: ContentHash::of(b"finding a"),
                answer: Some(ContentHash::of(b"answer a")),
                disposition: AdviceDisposition::Accepted,
            },
            MemberDisposition {
                slot: PlanningPairSlot::SeatB,
                finding: ContentHash::of(b"finding b"),
                answer: None,
                disposition: AdviceDisposition::Rejected,
            },
        ],
        rationale: text("Keep the dissent."),
    };
    let moved = Disposition {
        project_id: project(),
        run_id: run(),
        disposition: &disposition,
        caller: caller(),
    }
    .document();
    let baseline = serde_json::json!({
        "schema_version": 1,
        "operation": "record_planning_pair_disposition",
        "project": project().to_string(),
        "run": run().as_text(),
        "disposition": disposition,
        "caller_seat_binding_id": caller().seat_binding_id.to_string(),
        "caller_occupancy_generation": caller().occupancy_generation,
    });
    assert_eq!(moved, baseline);
    assert_eq!(
        hash_of(&moved),
        "75156987b470cfa99874240bd37d69abb207e3b050be4c1f3c3274b4d749f09f"
    );
}

#[test]
fn a_recovery_is_the_baseline_document() {
    let expected = NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("fake.runtime").expect("a runtime kind"),
        host: ExternalName::parse("fake-host").expect("a host"),
        generation: 4,
        native_id: ExternalId::parse("native-member-b").expect("a native id"),
    };
    let session = ExternalId::parse("provider-1").expect("a session");
    let context_hash = ContentHash::of(b"member context");
    let placement_hash = ContentHash::of(b"placement");
    let revision = AggregateRevision::parse(7).expect("a revision");
    let mut hashes = Vec::new();
    for provider_session_id in [Some(&session), None] {
        let moved = Recovery {
            project_id: project(),
            run_id: run(),
            member_seat_binding_id: member().seat_binding_id,
            slot: PlanningPairSlot::SeatB,
            caller: caller(),
            expected_run_revision: revision,
            expected_member_occupancy_generation: 2,
            expected_native_identity: &expected,
            expected_provider_session_id: provider_session_id,
            member_context_hash: &context_hash,
            placement_hash: &placement_hash,
        }
        .document();
        let baseline = serde_json::json!({
            "schema_version": 1,
            "operation": "recover_planning_pair_seat",
            "project": project().to_string(),
            "run": run().as_text(),
            "member_seat_binding_id": member().seat_binding_id.to_string(),
            "slot": PlanningPairSlot::SeatB.as_str(),
            "caller_seat_binding_id": caller().seat_binding_id.to_string(),
            "caller_occupancy_generation": caller().occupancy_generation,
            "expected_run_revision": revision.get(),
            "expected_member_occupancy_generation": 2_u64,
            "expected_native_identity": {
                "runtime_kind": expected.runtime_kind.as_str(),
                "host": expected.host.as_str(),
                "generation": expected.generation,
                "native_id": expected.native_id.as_str(),
            },
            "expected_provider_session_id": provider_session_id.map(ExternalId::as_str),
            "member_context_hash": context_hash.as_str(),
            "placement_hash": placement_hash.as_str(),
        });
        assert_eq!(moved, baseline);
        hashes.push(hash_of(&moved));
    }
    assert_eq!(
        hashes,
        [
            "f146be39e22d2232df8eb241e9fb469e155e685c7404ec1897a57de43023f42b",
            "cb1905678c95ef2cd8ee92b8f84be57bf6ae86384cec818ba9a9e95425b9321d",
        ]
    );
}
