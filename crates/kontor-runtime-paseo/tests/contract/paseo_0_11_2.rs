//! KR-08 / TEST-004: the adapter against the Paseo 0.11.2 contract baseline.
//!
//! Paseo 0.11 moved three facts this adapter depends on: keyed creation with a
//! durable lifecycle (`agent.create.request` + `creation.subscribe.request`),
//! caller-journaled message delivery (`agentRequestReceipts`), and the
//! permission lifecycle on the agent snapshot rather than in the transcript.
//! These tests pin the adapter to the shapes and guarantees the 0.11.2 source
//! defines; the host may run a later build, but this is the contract baseline
//! DEP-001 keeps separate from it.

use super::*;
use kontor_runtime::admission::ReplacedBinding;

const SERVER_INFO_0_11_2: &str = include_str!("../fixtures/paseo-0.11.2/protocol/server-info.json");
const AGENT_CREATE_RESPONSE: &str =
    include_str!("../fixtures/paseo-0.11.2/protocol/agent-create-response.json");
const CREATION_SUBSCRIBE_RESPONSE: &str =
    include_str!("../fixtures/paseo-0.11.2/protocol/creation-subscribe-response.json");
const CREATION_SUBSCRIBE_OUTCOME_UNKNOWN: &str =
    include_str!("../fixtures/paseo-0.11.2/protocol/creation-subscribe-outcome-unknown.json");

fn keyed(raw: &str, key: &str) -> serde_json::Value {
    v(&raw.replace("__IDEMPOTENCY_KEY__", key))
}

/// A daemon on the 0.11.2 baseline that can also carry an OpenCode posture.
fn paseo_0_11_2_daemon() -> RecordedPaseo {
    opencode_capable_daemon().announcing(&v(SERVER_INFO_0_11_2))
}

/// The 0.11.2 identity advertises the features this suite depends on.
#[test]
fn the_0_11_2_baseline_advertises_keyed_creation_and_delivery_receipts() {
    let identity = v(SERVER_INFO_0_11_2);
    let parsed: kontor_runtime_paseo::wire::PaseoServerInfo =
        serde_json::from_value(identity).expect("recorded server_info is the pinned shape");
    assert!(parsed.supports_creation_lifecycle());
    assert!(parsed.supports(kontor_runtime_paseo::wire::PaseoFeature::AgentRequestReceipts));
    assert!(
        kontor_runtime_paseo::wire::REQUIRED_FEATURES
            .contains(&kontor_runtime_paseo::wire::PaseoFeature::AgentRequestReceipts),
        "a daemon without caller-journaled receipts cannot be driven autonomously"
    );
    assert_eq!(parsed.version.as_deref(), Some("0.11.2"));
}

/// One OpenCode launch on a caller-chosen binding, so the test can name the
/// native id the keyed create must carry.
async fn keyed_opencode_launch(
    plane: &Plane,
    workspace: &WorkspaceBindingSnapshot,
    binding_id: RuntimeBindingId,
) -> LaunchRequest {
    plane
        .adapter
        .admit_launch(&AdmissionRequest {
            slot: RoleSlotKey::new(team_run(), slot("implement-a")),
            agent_run_id: run(RUN_IMPLEMENT),
            binding_id,
            replaces: None,
            requested_at: at("2026-08-10T09:00:00Z"),
        })
        .await
        .expect("admission")
        .into_authority()
        .expect("an authority")
        .into_request(LaunchParts {
            scope: execution_scope(),
            display_name: name("Implement • KON-19"),
            agent_run_id: run(RUN_IMPLEMENT),
            team_run_id: team_run(),
            role_slot_id: slot("implement-a"),
            task_id: task(),
            binding_id,
            placement: Some(LaunchPlacement::Workspace(workspace.clone())),
            cwd: root(),
            account_profile_id: None,
            prompt: text("bootstrap the role"),
            model_rung: opencode_rung(),
            context_policy: standard_context_policy(),
            autonomy: SeatAutonomy::Bounded,
            requested_at: at("2026-08-10T09:00:00Z"),
        })
}

fn agent_for(intent: &str, native_id: &str) -> serde_json::Value {
    let mut agent = opencode_agent(intent, Some(true), false);
    agent["agent"]["id"] = serde_json::json!(native_id);
    agent
}

fn completed_answer(intent: &str, native_id: &str) -> serde_json::Value {
    let mut answer = keyed(AGENT_CREATE_RESPONSE, native_id);
    answer["agent"] = agent_for(intent, native_id)["agent"].clone();
    answer["creation"]["agentId"] = serde_json::json!(native_id);
    answer
}

fn completed_subscription(native_id: &str) -> serde_json::Value {
    let mut answer = keyed(CREATION_SUBSCRIBE_RESPONSE, native_id);
    answer["snapshot"]["agentId"] = serde_json::json!(native_id);
    answer
}

/// A1: the create carries the launch key and a caller-supplied native id, and
/// the prompt still rides on that one effect.
#[tokio::test]
async fn a_keyed_create_carries_the_launch_key_and_a_caller_supplied_agent_id() {
    let binding_id = RuntimeBindingId::generate();
    let native_id = binding_id.to_string();
    let recorded = paseo_0_11_2_daemon().journaling(&native_id, EPOCH_RAW, Vec::new());
    let (plane, workspace) = Plane::prepared(recorded).await;
    let request = keyed_opencode_launch(&plane, &workspace, binding_id).await;
    let intent = standard_intent(binding_id, run(RUN_IMPLEMENT));
    plane.daemon.set_answer_rpc(
        "agent.create.request",
        completed_answer(&intent, &native_id),
    );

    plane
        .adapter
        .launch(&request)
        .await
        .expect("a keyed create is admitted and bound");

    let sent = plane.daemon.sent_messages("agent.create.request");
    let [create] = sent.as_slice() else {
        panic!("exactly one keyed create was sent: {sent:?}")
    };
    assert_eq!(create["type"], "agent.create.request");
    assert_eq!(
        create["idempotencyKey"], native_id,
        "the launch key is the idempotency key, so a retry is the same operation"
    );
    assert_eq!(
        create["agentId"], native_id,
        "the caller supplies the native id the daemon will claim"
    );
    assert_eq!(
        create["initialPrompt"], "bootstrap the role",
        "the prompt still rides on the create rather than becoming a second effect"
    );
    assert_eq!(create["clientMessageId"], native_id);
    assert_eq!(
        plane.daemon.count("rpc create_agent_request"),
        0,
        "0.11.2 uses the lifecycle create, not the legacy spelling"
    );
    assert_eq!(
        plane.daemon.count("rpc send_agent_message_request"),
        0,
        "and no separate first turn"
    );
}

/// A1: a lost create acknowledgement is settled from the durable creation
/// journal, so the same agent is adopted without a second create.
#[tokio::test]
async fn a_lost_keyed_create_acknowledgement_adopts_the_journaled_agent() {
    let binding_id = RuntimeBindingId::generate();
    let native_id = binding_id.to_string();
    let recorded = paseo_0_11_2_daemon().journaling(&native_id, EPOCH_RAW, Vec::new());
    let (plane, workspace) = Plane::prepared(recorded).await;
    let request = keyed_opencode_launch(&plane, &workspace, binding_id).await;
    let intent = standard_intent(binding_id, run(RUN_IMPLEMENT));
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", agent_for(&intent, &native_id));
    plane.daemon.set_answer_rpc(
        "creation.subscribe.request",
        completed_subscription(&native_id),
    );
    plane.daemon.append_journal_item(
        &native_id,
        serde_json::json!({
            "type": "user_message",
            "text": "bootstrap the role",
            "clientMessageId": native_id,
        }),
    );

    plane.daemon.lose_next_rpc("agent.create.request");
    plane
        .adapter
        .launch(&request)
        .await
        .expect("the journal settles the lost create");

    assert_eq!(
        plane.daemon.count("rpc agent.create.request"),
        1,
        "the create is attempted once; the retry is a read"
    );
    assert_eq!(
        plane.daemon.count("rpc creation.subscribe.request"),
        1,
        "the durable journal is the settling evidence"
    );
    assert_eq!(
        plane.daemon.count("rpc create_agent_request"),
        0,
        "never a legacy create as a fallback"
    );
}

/// A1/A2: a keyed create the daemon cannot settle is a typed unknown, not a
/// second create and not a plain channel refusal.
#[tokio::test]
async fn an_unsettled_keyed_create_keeps_the_claim_and_types_the_refusal() {
    let binding_id = RuntimeBindingId::generate();
    let native_id = binding_id.to_string();
    let recorded = paseo_0_11_2_daemon().journaling(&native_id, EPOCH_RAW, Vec::new());
    let (plane, workspace) = Plane::prepared(recorded).await;
    let request = keyed_opencode_launch(&plane, &workspace, binding_id).await;
    plane.daemon.set_answer_rpc(
        "creation.subscribe.request",
        keyed(CREATION_SUBSCRIBE_OUTCOME_UNKNOWN, &native_id),
    );
    plane.daemon.lose_next_rpc("agent.create.request");

    let error = plane
        .adapter
        .launch(&request)
        .await
        .expect_err("an outcome the daemon cannot prove is never bound");
    assert!(
        matches!(error, RuntimeError::DeliveryConfirmationUnknown { .. }),
        "typed unknown, so the seat claim is kept: {error:?}"
    );
    assert_eq!(plane.daemon.count("rpc agent.create.request"), 1);
    assert_eq!(
        plane.daemon.count("rpc create_agent_request"),
        0,
        "an unknown outcome never licences a second create"
    );
}

/// A2: every send returns a typed receipt, and a replay of the same id is the
/// receipt rather than a second delivery.
#[tokio::test]
async fn a_send_on_the_0_11_2_baseline_returns_one_typed_receipt() {
    let (plane, workspace) = Plane::prepared(paseo_0_11_2_daemon()).await;
    let outcome = plane
        .launch(run(RUN_IMPLEMENT), &slot("implement-a"), &workspace)
        .await
        .expect("the seat launches");
    let request = message(&outcome.snapshot, "the next turn");

    let first = plane
        .adapter
        .send(&request)
        .await
        .expect("the delivered message has an acknowledgement");
    let replay = plane
        .adapter
        .send(&request)
        .await
        .expect("the same id replays its own receipt");
    assert_eq!(first, replay);
    assert_eq!(first.message_id, request.message_id);
    assert_eq!(
        plane.daemon.count("rpc send_agent_message_request"),
        1,
        "a replay is answered from the ledger, never put on the wire twice"
    );
    assert_eq!(plane.daemon.journal_len(AGENT_ID), 1);
}

/// A4: a send addressed at an archived native is refused before the wire, so
/// Paseo's unarchive-on-message behaviour cannot reactivate a retired seat.
#[tokio::test]
async fn a_send_cannot_reactivate_an_archived_agent() {
    let (plane, binding) = launched().await;
    plane.daemon.take_calls();
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT_ARCHIVED));

    let refused = plane
        .adapter
        .send(&message(&binding, "a turn for the successor"))
        .await
        .expect_err("an archived seat is not driveable");
    assert!(
        matches!(&refused, RuntimeError::StaleBinding { rule } if rule.contains("retired")),
        "refused as a retired seat: {refused:?}"
    );
    assert_eq!(
        plane.daemon.count("rpc send_agent_message_request"),
        0,
        "nothing was put on the wire, so Paseo cannot unarchive it"
    );
}

/// A4: the same refusal holds when the exact lookup hides the archived agent
/// and only the paginated archive listing proves it.
#[tokio::test]
async fn a_send_cannot_reactivate_an_agent_only_the_archive_listing_shows() {
    let (plane, binding) = launched().await;
    plane.daemon.take_calls();
    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT_NOT_FOUND));
    plane
        .daemon
        .set_answer_rpc("fetch_agents_request", v(AGENT_LIST_ARCHIVED_ONLY));

    let refused = plane
        .adapter
        .send(&message(&binding, "a turn for the successor"))
        .await
        .expect_err("the archive listing proves the exact native is retired");
    assert!(
        matches!(&refused, RuntimeError::StaleBinding { rule } if rule.contains("retired")),
        "refused as a retired seat: {refused:?}"
    );
    assert_eq!(plane.daemon.count("rpc send_agent_message_request"), 0);
}

/// A4: an archived predecessor is replaceable; a live one is not, even though
/// the citation itself is complete.
#[tokio::test]
async fn an_archived_predecessor_is_replaceable_and_a_live_one_is_refused() {
    let (plane, binding) = launched().await;
    let predecessor = ReplacedBinding {
        binding_id: binding.binding_id(),
        agent_run_id: binding.agent_run_id(),
        successor_agent_run_id: run(RUN_QA),
    };
    let admission = || AdmissionRequest {
        slot: RoleSlotKey::new(team_run(), slot("implement-a")),
        agent_run_id: run(RUN_QA),
        binding_id: RuntimeBindingId::generate(),
        replaces: Some(predecessor.clone()),
        requested_at: at("2026-08-10T09:05:00Z"),
    };

    plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT_ARCHIVED));
    let admitted = plane
        .adapter
        .admit_launch(&admission())
        .await
        .expect("the archive stamp is the runtime's own terminal evidence");
    assert!(admitted.into_authority().is_ok());

    // A separate plane, because the successful admission reserved the slot for
    // the successor run.
    let (live_plane, live_binding) = launched().await;
    let live_predecessor = ReplacedBinding {
        binding_id: live_binding.binding_id(),
        agent_run_id: live_binding.agent_run_id(),
        successor_agent_run_id: run(RUN_QA),
    };
    live_plane
        .daemon
        .set_answer_rpc("fetch_agent_request", v(AGENT));
    let refused = live_plane
        .adapter
        .admit_launch(&AdmissionRequest {
            slot: RoleSlotKey::new(team_run(), slot("implement-a")),
            agent_run_id: run(RUN_QA),
            binding_id: RuntimeBindingId::generate(),
            replaces: Some(live_predecessor),
            requested_at: at("2026-08-10T09:05:00Z"),
        })
        .await
        .expect_err("a live holder is not terminal evidence");
    assert!(
        matches!(refused, RuntimeError::ReplacementNotEvidenced { .. }),
        "refused as unevidenced: {refused:?}"
    );
}

/// A3: a native permission prompt is addressable through the adapter's typed
/// snapshot read, and answering it is a single correlated effect.
#[tokio::test]
async fn a_native_permission_prompt_is_typed_attention_and_answerable_once() {
    let (plane, binding) = with_permission().await;
    let pending = plane
        .adapter
        .pending_permissions(&binding)
        .await
        .expect("the open request is read from the agent snapshot");
    assert_eq!(pending, vec![external("perm_1")]);

    resolve_permission(&plane);
    let acknowledged = plane
        .adapter
        .respond_permission(&permission(&binding, PermissionDecision::Allow))
        .await
        .expect("the prompt this runtime raised can be answered");
    assert_eq!(acknowledged.decision, PermissionDecision::Allow);
    assert_eq!(
        plane.daemon.count("rpc agent_permission_response"),
        1,
        "one answer, correlated by the permission id"
    );
}

/// N-05: a settled consultation seat is archived and read back on 0.11.2, and
/// a replay is read-only.
#[tokio::test]
async fn a_settled_consultation_session_releases_on_the_0_11_2_baseline() {
    use kontor_runtime::adapter::ConsultationSessionReleaseRequest;
    let run_id = ConsultationRunId::Committee(CommitteeRunId::parse(MINI_PROJECT).unwrap());
    let seat_binding_id = SeatBindingId::parse(RUN_QA).unwrap();
    let archived = consultation_agent(AGENT_ARCHIVED, run_id, seat_binding_id);
    let mut archived_list = v(AGENT_LIST_ARCHIVED_ONLY);
    archived_list["entries"][0]["agent"] = archived["agent"].clone();
    let recorded = RecordedPaseo::new()
        .answering(&PaseoCommand::version(), VERSION)
        .answering(&PaseoCommand::agent_archive(AGENT_ID), CLI_AGENT_ARCHIVED)
        .then_answering_rpc(
            "fetch_agent_request",
            consultation_agent(AGENT_PERMISSION_OPEN, run_id, seat_binding_id),
        )
        .answering_rpc("fetch_agent_request", v(AGENT_NOT_FOUND))
        .answering_rpc("fetch_agents_request", archived_list)
        .announcing(&v(SERVER_INFO_0_11_2));
    let plane = Plane::fresh(recorded);
    let request = ConsultationSessionReleaseRequest {
        run_id,
        seat_binding_id,
        identity: NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse(RUNTIME_KIND).unwrap(),
            host: name(HOST_KEY),
            generation: 1,
            native_id: external(AGENT_ID),
        },
        requested_at: kontor_core::id::Timestamp::now(),
    };
    plane
        .adapter
        .release_consultation_session(&request)
        .await
        .expect("the settled seat is archived and read back");
    assert_eq!(plane.daemon.count(&format!("agent archive {AGENT_ID}")), 1);
    plane
        .adapter
        .release_consultation_session(&request)
        .await
        .expect("a replay proves the archive without a second effect");
    assert_eq!(
        plane.daemon.count(&format!("agent archive {AGENT_ID}")),
        1,
        "release is exactly once"
    );
}
