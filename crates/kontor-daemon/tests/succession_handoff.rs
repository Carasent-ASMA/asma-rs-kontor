//! Contract tests for the daemon's provider-neutral succession handoff seam.

use std::sync::Mutex;

use async_trait::async_trait;
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CanonicalDocument, ContentHash, ExternalId,
    ExternalName, ProjectId, RuntimeBindingId, RuntimeKindKey, SuccessionAttemptId, Timestamp,
    parse_utc_timestamp,
};
use kontor_core::quota::HeadroomThresholds;
use kontor_core::repository::ProviderQuotaState;
use kontor_core::spec::{
    EffortLevel, ModelRef, ModelRung, ProviderQuotaKind, ProviderQuotaSource, ProviderRef,
};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::succession::{
    SuccessionHandoff, SuccessionHandoffDegradedReason, SuccessionHandoffOutcome,
    SuccessionRedactionPass, SuccessionTimelineRange,
};
use kontor_daemon::succession::{
    GovernedSuccessionSummarizer, MAX_SUCCESSION_SUMMARY_OUTPUT_CHARS, REDACTED_SUCCESSION_CONTENT,
    SuccessionHandoffRequest, SuccessionRedactionPolicy, SuccessionSummarizerError,
    SuccessionSummarizerTransport, SuccessionSummaryRequest, SuccessionSummaryResponse,
    UnavailableSuccessionSummarizer, produce_succession_handoff,
};
use kontor_runtime::timeline::EventSubject;
use kontor_runtime::{
    BindingMessageTimeline, BindingTimelineEvent, SessionEvent, SessionEventKind, TimelinePosition,
};
use kontor_scheduler::headroom::{EligibleAccount, HeadroomConfig};

const CREDENTIAL_CANARY: &str = "INTERNAL-CREDENTIAL-CANARY-4829";

#[derive(Clone)]
enum Reply {
    Summary(String),
    Rejected,
}

struct RecordingTransport {
    calls: Mutex<Vec<(ModelRung, SuccessionSummaryRequest)>>,
    reply: Reply,
}

impl RecordingTransport {
    fn returning(summary: impl Into<String>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            reply: Reply::Summary(summary.into()),
        }
    }

    fn rejecting() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            reply: Reply::Rejected,
        }
    }

    fn calls(&self) -> std::sync::MutexGuard<'_, Vec<(ModelRung, SuccessionSummaryRequest)>> {
        self.calls.lock().expect("recording transport lock")
    }
}

#[async_trait]
impl SuccessionSummarizerTransport for RecordingTransport {
    async fn summarize(
        &self,
        model_rung: &ModelRung,
        request: &SuccessionSummaryRequest,
    ) -> Result<SuccessionSummaryResponse, SuccessionSummarizerError> {
        self.calls
            .lock()
            .expect("recording transport lock")
            .push((model_rung.clone(), request.clone()));
        match &self.reply {
            Reply::Summary(summary) => Ok(SuccessionSummaryResponse::new(summary.clone())),
            Reply::Rejected => Err(SuccessionSummarizerError::Rejected),
        }
    }
}

fn at() -> Timestamp {
    parse_utc_timestamp("2026-09-04T12:30:00Z").expect("canonical timestamp")
}

fn native_identity() -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo").expect("runtime kind"),
        host: ExternalName::parse("local").expect("runtime host"),
        generation: 7,
        native_id: ExternalId::parse("native-seat-42").expect("native session id"),
    }
}

fn model_rung() -> ModelRung {
    ModelRung {
        provider: ProviderRef("codex".to_owned()),
        model: ModelRef("gpt-5.6-sol".to_owned()),
        effort: Some(EffortLevel::Xhigh),
    }
}

fn event(
    runtime_binding_id: RuntimeBindingId,
    native: &NativeRuntimeIdentity,
    kind: SessionEventKind,
    sequence: u64,
    body: &str,
) -> BindingTimelineEvent {
    BindingTimelineEvent::new(
        runtime_binding_id,
        native.clone(),
        SessionEvent {
            kind,
            position: TimelinePosition { epoch: 4, sequence },
            subject: EventSubject::None,
            native_event_id: None,
            emitted_at: at(),
            payload: CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
                "body": body,
            }))
            .expect("admissible runtime-owned event content"),
        },
    )
}

fn timeline(
    runtime_binding_id: RuntimeBindingId,
    native: &NativeRuntimeIdentity,
) -> BindingMessageTimeline {
    BindingMessageTimeline::project(
        runtime_binding_id,
        native.clone(),
        [
            event(
                runtime_binding_id,
                native,
                SessionEventKind::Message,
                1,
                &format!("Preserve the exact decision from {CREDENTIAL_CANARY}"),
            ),
            event(
                runtime_binding_id,
                native,
                SessionEventKind::ToolCall,
                2,
                "tool arguments must never cross the summarizer boundary",
            ),
            event(
                runtime_binding_id,
                native,
                SessionEventKind::Message,
                3,
                "The successor must continue from the accepted checkpoint",
            ),
        ],
    )
    .expect("one exact contiguous binding timeline")
}

fn policy() -> SuccessionRedactionPolicy {
    SuccessionRedactionPolicy::new(
        ContentHash::of(b"succession-redaction-policy-v1"),
        vec![CREDENTIAL_CANARY.to_owned()],
    )
}

fn request(
    runtime_binding_id: RuntimeBindingId,
    native: NativeRuntimeIdentity,
    timeline: Option<BindingMessageTimeline>,
    summarizer_model_rung: Option<ModelRung>,
) -> SuccessionHandoffRequest {
    SuccessionHandoffRequest {
        attempt_id: SuccessionAttemptId::generate(),
        predecessor_agent_run_id: AgentRunId::generate(),
        predecessor_runtime_binding_id: runtime_binding_id,
        predecessor_native_identity: native,
        timeline,
        summarizer_model_rung,
        produced_at: at(),
    }
}

fn assert_no_source_or_compaction(handoff: &SuccessionHandoff) {
    let canonical = handoff
        .canonicalize()
        .expect("a handoff is valid canonical evidence");
    let json = canonical.json();
    assert!(!json.contains(CREDENTIAL_CANARY));
    assert!(!json.to_ascii_lowercase().contains("compaction"));
    let readback: SuccessionHandoff = canonical.deserialize().expect("exact handoff readback");
    assert_eq!(&readback, handoff);
    assert_eq!(
        handoff.hash().expect("handoff digest"),
        canonical.hash().clone()
    );
}

#[tokio::test]
async fn transport_receives_only_redacted_messages_and_summary_readback_is_exact() {
    let binding_id = RuntimeBindingId::generate();
    let native = native_identity();
    let route = model_rung();
    let raw_summary = format!("Retain constraints from {CREDENTIAL_CANARY} and continue.");
    let transport = RecordingTransport::returning(raw_summary.clone());

    let handoff = produce_succession_handoff(
        request(
            binding_id,
            native.clone(),
            Some(timeline(binding_id, &native)),
            Some(route.clone()),
        ),
        &policy(),
        &transport,
    )
    .await;

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    let (dispatched_route, outbound) = &calls[0];
    assert_eq!(dispatched_route, &route);
    assert_eq!(
        outbound.timeline(),
        SuccessionTimelineRange {
            epoch: 4,
            start_sequence: 1,
            end_sequence: 3,
        }
    );
    assert_eq!(
        outbound.maximum_output_characters(),
        MAX_SUCCESSION_SUMMARY_OUTPUT_CHARS
    );
    assert_eq!(
        outbound
            .messages()
            .iter()
            .map(|message| message.position().sequence)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    let outbound_json = serde_json::to_string(outbound).expect("serializable outbound request");
    assert!(!outbound_json.contains(CREDENTIAL_CANARY));
    assert!(outbound_json.contains(REDACTED_SUCCESSION_CONTENT));
    assert!(!outbound_json.contains("tool arguments"));
    drop(calls);

    let expected_summary =
        format!("Retain constraints from {REDACTED_SUCCESSION_CONTENT} and continue.");
    match &handoff.outcome {
        SuccessionHandoffOutcome::Summary {
            timeline,
            summarizer_model_rung,
            summary,
            summary_hash,
            input_redaction,
            output_redaction,
        } => {
            assert_eq!(timeline.start_sequence, 1);
            assert_eq!(timeline.end_sequence, 3);
            assert_eq!(summarizer_model_rung, &route);
            assert_eq!(summary.as_str(), expected_summary);
            assert_eq!(summary_hash, &ContentHash::of(expected_summary.as_bytes()));
            assert_eq!(input_redaction.pass, SuccessionRedactionPass::Input);
            assert_ne!(input_redaction.source_hash, input_redaction.redacted_hash);
            assert_eq!(output_redaction.pass, SuccessionRedactionPass::Output);
            assert_eq!(
                output_redaction.source_hash,
                ContentHash::of(raw_summary.as_bytes())
            );
            assert_eq!(output_redaction.redacted_hash, *summary_hash);
            assert_eq!(input_redaction.policy_hash, output_redaction.policy_hash);
        }
        outcome => panic!("expected a content-bearing summary handoff, got {outcome:?}"),
    }
    assert_no_source_or_compaction(&handoff);
}

#[tokio::test]
async fn unavailable_timeline_or_route_and_rejected_summary_are_explicitly_degraded() {
    let binding_id = RuntimeBindingId::generate();
    let native = native_identity();
    let transport = RecordingTransport::returning("must not run");

    let no_timeline = produce_succession_handoff(
        request(binding_id, native.clone(), None, Some(model_rung())),
        &policy(),
        &transport,
    )
    .await;
    assert!(matches!(
        no_timeline.outcome,
        SuccessionHandoffOutcome::Degraded {
            timeline: None,
            reason: SuccessionHandoffDegradedReason::TimelineUnavailable,
            ..
        }
    ));
    assert_eq!(transport.calls().len(), 0);
    assert_no_source_or_compaction(&no_timeline);

    let no_route = produce_succession_handoff(
        request(
            binding_id,
            native.clone(),
            Some(timeline(binding_id, &native)),
            None,
        ),
        &policy(),
        &transport,
    )
    .await;
    assert!(matches!(
        no_route.outcome,
        SuccessionHandoffOutcome::Degraded {
            timeline: Some(_),
            reason: SuccessionHandoffDegradedReason::SummarizerUnplaceable,
            ..
        }
    ));
    assert_eq!(transport.calls().len(), 0);
    assert_no_source_or_compaction(&no_route);

    let rejected_transport = RecordingTransport::rejecting();
    let rejected = produce_succession_handoff(
        request(
            binding_id,
            native.clone(),
            Some(timeline(binding_id, &native)),
            Some(model_rung()),
        ),
        &policy(),
        &rejected_transport,
    )
    .await;
    assert!(matches!(
        rejected.outcome,
        SuccessionHandoffOutcome::Degraded {
            timeline: Some(_),
            reason: SuccessionHandoffDegradedReason::SummaryRejected,
            ..
        }
    ));
    assert_eq!(rejected_transport.calls().len(), 1);
    assert_no_source_or_compaction(&rejected);
}

#[tokio::test]
async fn unavailable_production_transport_never_invents_a_summarizer_session() {
    let binding_id = RuntimeBindingId::generate();
    let native = native_identity();
    let handoff = produce_succession_handoff(
        request(
            binding_id,
            native.clone(),
            Some(timeline(binding_id, &native)),
            Some(model_rung()),
        ),
        &policy(),
        &UnavailableSuccessionSummarizer,
    )
    .await;

    assert!(matches!(
        handoff.outcome,
        SuccessionHandoffOutcome::Degraded {
            timeline: Some(_),
            reason: SuccessionHandoffDegradedReason::SummarizerUnplaceable,
            ..
        }
    ));
    assert_no_source_or_compaction(&handoff);
}

// ---------------------------------------------------------------------------
// Production composition: exact declared rung through the headroom walk
// ---------------------------------------------------------------------------

fn summarizer_account() -> AccountProfileId {
    AccountProfileId::parse("01890000-0000-7000-8000-000000000001").expect("a valid profile id")
}

fn eligible_account(providers: &[&str]) -> EligibleAccount {
    EligibleAccount {
        account_profile_id: summarizer_account(),
        selectable_providers: providers
            .iter()
            .map(|provider| (*provider).to_owned())
            .collect(),
    }
}

fn quota_state(provider: &str, kind: ProviderQuotaKind) -> ProviderQuotaState {
    ProviderQuotaState {
        project_id: ProjectId::parse("01890000-0000-7000-8000-00000000f001")
            .expect("a valid project id"),
        account_profile_id: summarizer_account(),
        provider: provider.to_owned(),
        state: kind,
        resets_at: None,
        windows: Vec::new(),
        credit: None,
        evidence_hash: ContentHash::of(b"quota evidence"),
        source: ProviderQuotaSource::RuntimeObservation,
        observed_at: at(),
        revision: AggregateRevision::INITIAL,
        updated_at: at(),
        provenance_id: None,
    }
}

fn headroom() -> HeadroomConfig {
    HeadroomConfig {
        thresholds: HeadroomThresholds {
            session_percent: 90,
            daily_percent: 85,
            weekly_percent: 70,
            monthly_percent: 80,
        },
        control_plane_reserve_percent: 10,
        short_horizon_seconds: 900,
        escalation_horizon_seconds: 7_200,
    }
}

/// One contiguous timeline mixing every excluded event kind with the messages.
///
/// REQ-005 is structural: only [`SessionEventKind::Message`] may reach the
/// summarizer, and the non-message payloads here carry distinct sentinels so
/// their absence is asserted at the transport boundary.
fn mixed_timeline(
    runtime_binding_id: RuntimeBindingId,
    native: &NativeRuntimeIdentity,
) -> BindingMessageTimeline {
    BindingMessageTimeline::project(
        runtime_binding_id,
        native.clone(),
        [
            event(
                runtime_binding_id,
                native,
                SessionEventKind::Message,
                1,
                &format!("Preserve the exact decision from {CREDENTIAL_CANARY}"),
            ),
            event(
                runtime_binding_id,
                native,
                SessionEventKind::ToolCall,
                2,
                "tool arguments must never cross the summarizer boundary",
            ),
            event(
                runtime_binding_id,
                native,
                SessionEventKind::PermissionRequest,
                3,
                "permission request must never cross the summarizer boundary",
            ),
            event(
                runtime_binding_id,
                native,
                SessionEventKind::Message,
                4,
                "The successor must continue from the accepted checkpoint",
            ),
            event(
                runtime_binding_id,
                native,
                SessionEventKind::Log,
                5,
                "diagnostic log must never cross the summarizer boundary",
            ),
            event(
                runtime_binding_id,
                native,
                SessionEventKind::PermissionResolved,
                6,
                "permission resolution must never cross the summarizer boundary",
            ),
            event(
                runtime_binding_id,
                native,
                SessionEventKind::Message,
                7,
                "Keep the open question duty armed",
            ),
        ],
    )
    .expect("one exact contiguous binding timeline")
}

fn governed_summarizer<'a>(
    declared: &[ModelRung],
    accounts: &[EligibleAccount],
    states: &[ProviderQuotaState],
    transport: &'a RecordingTransport,
) -> GovernedSuccessionSummarizer<'a> {
    GovernedSuccessionSummarizer::resolve(
        declared,
        accounts,
        states,
        &headroom(),
        at(),
        jiff::SignedDuration::from_secs(60),
        |_| true,
        transport,
    )
    .expect("a non-empty declared chain resolves")
}

#[tokio::test]
async fn governed_summarizer_dispatches_the_admitted_declared_rung_over_the_redacted_projection() {
    let binding_id = RuntimeBindingId::generate();
    let native = native_identity();
    let declared = vec![model_rung()];
    let accounts = [eligible_account(&["codex"])];
    let states = [quota_state("codex", ProviderQuotaKind::Available)];
    let raw_summary = format!("Retain constraints from {CREDENTIAL_CANARY} and continue.");
    let transport = RecordingTransport::returning(raw_summary.clone());
    let summarizer = governed_summarizer(&declared, &accounts, &states, &transport);

    assert_eq!(
        summarizer.admitted_rung(),
        Some(&declared[0]),
        "the walk must bind the exact declared rung it admitted"
    );

    let handoff = produce_succession_handoff(
        request(
            binding_id,
            native.clone(),
            Some(mixed_timeline(binding_id, &native)),
            summarizer.admitted_rung().cloned(),
        ),
        &policy(),
        &summarizer,
    )
    .await;

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    let (dispatched_route, outbound) = &calls[0];
    assert_eq!(dispatched_route, &declared[0]);
    assert_eq!(
        outbound.timeline(),
        SuccessionTimelineRange {
            epoch: 4,
            start_sequence: 1,
            end_sequence: 7,
        }
    );
    assert_eq!(
        outbound
            .messages()
            .iter()
            .map(|message| message.position().sequence)
            .collect::<Vec<_>>(),
        vec![1, 4, 7],
        "only message events may cross the summarizer boundary"
    );
    let outbound_json = serde_json::to_string(outbound).expect("serializable outbound request");
    assert!(!outbound_json.contains(CREDENTIAL_CANARY));
    assert!(outbound_json.contains(REDACTED_SUCCESSION_CONTENT));
    assert!(!outbound_json.contains("tool arguments"));
    assert!(!outbound_json.contains("permission request"));
    assert!(!outbound_json.contains("diagnostic log"));
    assert!(!outbound_json.contains("permission resolution"));
    drop(calls);

    let expected_summary =
        format!("Retain constraints from {REDACTED_SUCCESSION_CONTENT} and continue.");
    match &handoff.outcome {
        SuccessionHandoffOutcome::Summary {
            timeline,
            summarizer_model_rung,
            summary,
            summary_hash,
            input_redaction,
            output_redaction,
        } => {
            assert_eq!(timeline.start_sequence, 1);
            assert_eq!(timeline.end_sequence, 7);
            assert_eq!(summarizer_model_rung, &declared[0]);
            assert_eq!(summary.as_str(), expected_summary);
            assert_eq!(summary_hash, &ContentHash::of(expected_summary.as_bytes()));
            assert_eq!(input_redaction.pass, SuccessionRedactionPass::Input);
            assert_ne!(input_redaction.source_hash, input_redaction.redacted_hash);
            assert_eq!(output_redaction.pass, SuccessionRedactionPass::Output);
            assert_eq!(
                output_redaction.source_hash,
                ContentHash::of(raw_summary.as_bytes())
            );
            assert_eq!(output_redaction.redacted_hash, *summary_hash);
            assert_eq!(input_redaction.policy_hash, output_redaction.policy_hash);
        }
        outcome => panic!("expected a content-bearing summary handoff, got {outcome:?}"),
    }
    assert_no_source_or_compaction(&handoff);
}

#[tokio::test]
async fn governed_summarizer_never_dispatches_a_route_the_walk_did_not_admit() {
    let binding_id = RuntimeBindingId::generate();
    let native = native_identity();
    let declared = vec![model_rung()];
    let accounts = [eligible_account(&["codex"])];
    let states = [quota_state("codex", ProviderQuotaKind::Available)];
    let transport = RecordingTransport::returning("must not run");
    let summarizer = governed_summarizer(&declared, &accounts, &states, &transport);

    let wrong_route = ModelRung {
        provider: ProviderRef("claude".to_owned()),
        model: ModelRef("claude-opus-5".to_owned()),
        effort: Some(EffortLevel::High),
    };
    let handoff = produce_succession_handoff(
        request(
            binding_id,
            native.clone(),
            Some(timeline(binding_id, &native)),
            Some(wrong_route),
        ),
        &policy(),
        &summarizer,
    )
    .await;

    assert!(matches!(
        handoff.outcome,
        SuccessionHandoffOutcome::Degraded {
            timeline: Some(_),
            reason: SuccessionHandoffDegradedReason::SummarizerUnplaceable,
            ..
        }
    ));
    assert_eq!(
        transport.calls().len(),
        0,
        "a route the declared walk did not admit must never reach the transport"
    );
    assert_no_source_or_compaction(&handoff);
}

#[tokio::test]
async fn governed_summarizer_degrades_without_an_admissible_declared_route() {
    let binding_id = RuntimeBindingId::generate();
    let native = native_identity();
    let declared = vec![model_rung()];
    let accounts = [eligible_account(&["codex"])];
    let transport = RecordingTransport::returning("must not run");

    let mut exhausted = quota_state("codex", ProviderQuotaKind::Exhausted);
    exhausted.resets_at =
        Some(Timestamp::from_second(at().as_second() + 100_000).expect("a representable instant"));
    let summarizer = governed_summarizer(&declared, &accounts, &[exhausted], &transport);
    assert_eq!(
        summarizer.admitted_rung(),
        None,
        "an escalation is unplaceable, never a fabricated route"
    );

    let handoff = produce_succession_handoff(
        request(
            binding_id,
            native.clone(),
            Some(timeline(binding_id, &native)),
            summarizer.admitted_rung().cloned(),
        ),
        &policy(),
        &summarizer,
    )
    .await;
    assert!(matches!(
        handoff.outcome,
        SuccessionHandoffOutcome::Degraded {
            timeline: Some(_),
            reason: SuccessionHandoffDegradedReason::SummarizerUnplaceable,
            ..
        }
    ));
    assert_eq!(transport.calls().len(), 0);
    assert_no_source_or_compaction(&handoff);

    let mut block_soon = quota_state("codex", ProviderQuotaKind::Exhausted);
    block_soon.resets_at =
        Some(Timestamp::from_second(at().as_second() + 300).expect("a representable instant"));
    let waiting = governed_summarizer(&declared, &accounts, &[block_soon], &transport);
    assert_eq!(
        waiting.admitted_rung(),
        None,
        "a wait is unplaceable until the reset, never a fabricated route"
    );
}

#[tokio::test]
async fn an_unconfigured_summarizer_never_dispatches_and_stays_degraded() {
    let binding_id = RuntimeBindingId::generate();
    let native = native_identity();
    let transport = RecordingTransport::returning("must not run");
    let summarizer = GovernedSuccessionSummarizer::unplaceable(&transport);

    let handoff = produce_succession_handoff(
        request(
            binding_id,
            native.clone(),
            Some(timeline(binding_id, &native)),
            summarizer.admitted_rung().cloned(),
        ),
        &policy(),
        &summarizer,
    )
    .await;

    assert!(matches!(
        handoff.outcome,
        SuccessionHandoffOutcome::Degraded {
            timeline: Some(_),
            reason: SuccessionHandoffDegradedReason::SummarizerUnplaceable,
            ..
        }
    ));
    assert_eq!(transport.calls().len(), 0);
    assert_no_source_or_compaction(&handoff);
}

#[tokio::test]
async fn a_summary_handoff_with_a_mismatched_hash_is_refused_on_readback() {
    let binding_id = RuntimeBindingId::generate();
    let native = native_identity();
    let declared = vec![model_rung()];
    let accounts = [eligible_account(&["codex"])];
    let states = [quota_state("codex", ProviderQuotaKind::Available)];
    let transport = RecordingTransport::returning("Keep the accepted checkpoint");
    let summarizer = governed_summarizer(&declared, &accounts, &states, &transport);

    let handoff = produce_succession_handoff(
        request(
            binding_id,
            native.clone(),
            Some(timeline(binding_id, &native)),
            summarizer.admitted_rung().cloned(),
        ),
        &policy(),
        &summarizer,
    )
    .await;
    assert!(matches!(
        handoff.outcome,
        SuccessionHandoffOutcome::Summary { .. }
    ));

    let mut corrupted = handoff.clone();
    match &mut corrupted.outcome {
        SuccessionHandoffOutcome::Summary { summary_hash, .. } => {
            *summary_hash = ContentHash::of(b"a hash that does not digest the summary");
        }
        outcome => panic!("expected a summary handoff, got {outcome:?}"),
    }
    assert!(
        corrupted.validate().is_err(),
        "a summary hash that does not digest the stored summary must be refused"
    );
    assert!(
        corrupted.canonicalize().is_err(),
        "readback must refuse the same mismatched summary hash"
    );
}
