//! Section 3 — the runtime negative-case matrix, two coding accounts and the
//! cross-engine handoff.
//!
//! Every case here is driven through `kontor_runtime::fake::ScriptedFakeRuntime`,
//! which has no clock, no randomness, no network and no child process: a lost
//! acknowledgement, a renumbered stream and a restart are all things a fixture
//! *states*, so the answers are reproducible rather than timing-dependent.
//!
//! Two instances of that fake are identity-indistinguishable but each vouches
//! only for the bindings it issued, which is exactly what "runtime A" and
//! "runtime B" have to mean for a cross-engine handoff to be provable offline.

use std::collections::{BTreeMap, BTreeSet};

use jiff::SignedDuration;
use kontor_accounts::{
    AccountEnvironmentMap, AccountResolver, ResolvedAccountEnvironment, ResolverPolicy,
    SystemKeychain,
};
use kontor_context::handoff::{
    ContinuationMode, HandoffCapsule, TestAttempt, TestResult, acknowledge,
};
use kontor_context::model::WorkspaceRef;
use kontor_core::calendar::WorkScope;
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, BoundedText, CanonicalDocument,
    CommandReceiptId, ContentHash, ContextPackId, CredentialAlias, EnvironmentVariableName,
    ExecutionAuthorizationId, ExternalId, ExternalName, HandoffId, ModuleKey, ProjectId, RealmId,
    RoleSlotId, RuntimeBindingId, RuntimeKindKey, SCHEMA_VERSION, TaskId, TaskWorkflowId,
    TeamRunId,
};
use kontor_core::repository::{AccountProfile, CredentialReference, CredentialReferenceKind};
use kontor_core::state::TaskState;
use kontor_core::state::{
    DerivedRunState, NativeRuntimeIdentity, ObservedRunState, RuntimeContact,
};
use kontor_runtime::adapter::{LaunchOutcome, RuntimeAdapter, RuntimeError, RuntimeResult};
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{
    RuntimeCapabilities, RuntimeCapability, RuntimeLimits, TrustGrade,
};
use kontor_runtime::fake::{AdapterCall, RuntimeScript, ScriptStep, ScriptedFakeRuntime};
use kontor_runtime::observation::{
    ObservationSource, ReconciliationAction, ReconciliationFinding, reconcile,
};
use kontor_runtime::request::{
    AdoptRequest, CorrelationLabel, LaunchParts, LiveSubscribeRequest, MessageId,
    SendMessageRequest,
};
use kontor_runtime::timeline::{HistoryCursor, TimelineBreak, TimelinePosition};
use kontor_runtime::workspace::{
    WorkspaceBindingId, WorkspaceBindingSnapshot, WorkspacePrepareRequest, WorkspaceRoot,
};
use kontor_scheduler::model::{
    AccountAdmissionEvidence, AdaptiveWindow, AdaptiveWindowConfig, AuthorizationEvidence,
    CalendarAdmission, Candidate, CandidateDecision, CapacityConfig, CapacityUsage,
    ExternalWorkEvidence, ReconciliationEvidence, ReconciliationScope, RejectionCode,
    RuntimeAdmissionEvidence, RuntimeHealth, SchedulingSnapshot, TaskOrigin,
};
use kontor_scheduler::ready::{minimum_launch_capabilities, plan};
use kontor_tests_contract::{SESSION_KINDS, closes, drain_history, sequences, text};
use kontor_tests_e2e::{Bundle, scan_for_canaries};
use serde_json::json;

use crate::at;

/// When every pilot workspace in this section was prepared.
const PREPARED_AT: &str = "2026-08-12T08:59:00Z";
/// When every pilot seat in this section was launched.
const LAUNCHED_AT: &str = "2026-08-12T09:00:00Z";
/// The instant every later runtime call in this section declares.
const ACTED_AT: &str = "2026-08-12T09:01:00Z";

/// The environment variable the pilot's coding accounts deliver their home in.
const ACCOUNT_HOME_VARIABLE: &str = "CODEX_HOME";
/// Material planted inside the first account's approved credential home.
const CANARY_A: &str = "PILOT-CANARY-TOKEN-A";
/// Material planted inside the second account's approved credential home.
const CANARY_B: &str = "PILOT-CANARY-TOKEN-B";
/// A string this section deliberately *does* write into the bundle.
///
/// Without it an empty scan result would be indistinguishable from a scanner
/// that walked nothing, which is the way a secrecy proof passes by accident.
const SCAN_CONTROL: &str = "PILOT-SCAN-CONTROL-MARKER";

/// Answer the nine runtime, account and handoff criteria.
pub(crate) async fn run(bundle: &mut Bundle) {
    ambiguous_command(bundle).await;
    event_disorder(bundle).await;
    restart(bundle).await;
    lost_contact(bundle).await;
    adoption_inbox(bundle).await;
    cross_engine(bundle).await;
    // Last, so the credential scan covers every artifact this section wrote.
    accounts(bundle).await;
}

// ---------------------------------------------------------------------------
// A committed effect whose acknowledgement was lost
// ---------------------------------------------------------------------------

/// A lost acknowledgement is reconciled by message id, never by resending.
///
/// The interesting half is the counterfactual at the end: the same body under a
/// *fresh* id does commit a second effect. That is what makes the identity —
/// rather than luck or a quiet dedup on content — the thing holding
/// exactly-once.
async fn ambiguous_command(bundle: &mut Bundle) {
    let engine = Engine::start("/w/pilot-ambiguous", TaskId::generate()).await;
    let launched = engine
        .launch(AgentRunId::generate(), "ambiguous-a", None)
        .await
        .expect("the pilot seat launches");
    let binding = launched.snapshot;

    let message_id = MessageId::generate();
    let command = SendMessageRequest {
        binding: binding.clone(),
        message_id,
        body: text("run the pilot step"),
        sent_at: at(ACTED_AT),
    };

    engine.fake.push_step(ScriptStep::LoseSendAck);
    let ambiguous = engine.fake.send(&command).await;
    let committed_after_loss = engine.fake.committed_messages(&binding);

    // The identical request, resubmitted. Same id, same body: a reconciliation,
    // not a retry.
    let reconciled = engine.fake.send(&command).await;
    let committed_after_reconcile = engine.fake.committed_messages(&binding);

    // The counterfactual. A caller that had minted a new id would have been
    // sending the message a second time whatever it called the operation.
    let blind = engine
        .fake
        .send(&SendMessageRequest {
            message_id: MessageId::generate(),
            ..command.clone()
        })
        .await;
    let committed_after_blind = engine.fake.committed_messages(&binding);

    let sends = engine
        .fake
        .calls()
        .iter()
        .filter(|call| matches!(call, AdapterCall::Send(..)))
        .count();

    let lost = matches!(
        ambiguous,
        Err(RuntimeError::Transport {
            rule: "acknowledgement was lost after the message was committed"
        })
    );
    let replayed = reconciled
        .as_ref()
        .is_ok_and(|ack| ack.message_id == message_id && ack.binding_id == binding.binding_id());
    let position = reconciled.as_ref().ok().map(|ack| ack.position.to_string());

    let artifact = bundle
        .artifact(
            "runtime/ambiguous-command.json",
            &json!({
                "binding_id": binding.binding_id().to_string(),
                "message_id": message_id.to_string(),
                "first_attempt": {
                    "outcome": "transport failure after the effect committed",
                    "recognized": lost,
                    "committed_messages": committed_after_loss,
                },
                "reconciled_by_id": {
                    "replayed_original_receipt": replayed,
                    "receipt_position": position,
                    "committed_messages": committed_after_reconcile,
                },
                "control_blind_retry_under_a_fresh_id": {
                    "accepted": blind.is_ok(),
                    "committed_messages": committed_after_blind,
                },
                "adapter_send_calls": sends,
                "rule": "the ledger is written before the acknowledgement leaves, so an \
                         ambiguous command is answered from the ledger rather than by \
                         performing the effect again",
            }),
        )
        .expect("the ambiguous-command evidence is written");

    let one_effect = committed_after_loss == 1 && committed_after_reconcile == 1;
    let control_holds = committed_after_blind == 2 && blind.is_ok();
    if lost && replayed && one_effect && control_holds && sends == 3 {
        bundle.pass(
            "negative.ambiguous-command",
            format!(
                "the send committed and then lost its acknowledgement; resubmitting the identical \
                 message id replayed the original receipt at {} and left the session on one \
                 committed message across {sends} adapter calls — while the same body under a \
                 fresh id committed a second, proving the id and not the content is what makes \
                 the effect exactly-once",
                position.clone().unwrap_or_else(|| "unknown".to_owned())
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "negative.ambiguous-command",
            format!(
                "lost={lost}, replayed={replayed}, committed after loss/reconcile/control = \
                 {committed_after_loss}/{committed_after_reconcile}/{committed_after_blind}, \
                 send calls={sends}"
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// Duplicate, out-of-order and contradictory content
// ---------------------------------------------------------------------------

/// What one scripted stream did to a validated subscription.
struct StreamOutcome {
    /// The sequences history validated, in order.
    history: Vec<u64>,
    /// The anchor history left for live delivery to start strictly after.
    anchor: TimelinePosition,
    /// The sequences live delivery handed out, in order.
    delivered: Vec<u64>,
    /// Where the timeline stood once the stream was drained.
    position: TimelinePosition,
    /// Every typed refusal the stream produced, in order.
    breaks: Vec<TimelineBreak>,
}

impl StreamOutcome {
    /// The evidence shape both the artifact and the verdict read.
    fn to_json(&self) -> serde_json::Value {
        json!({
            "history": self.history,
            "anchor": self.anchor.to_string(),
            "delivered": self.delivered,
            "final_position": self.position.to_string(),
            "breaks": self.breaks.iter().map(|reason| reason.as_str()).collect::<Vec<_>>(),
        })
    }
}

/// Duplicates are no-ops, an older event cannot move the cursor back, and a
/// contradiction or a gap stops the stream instead of leaving a hole in it.
///
/// The three scripts run on three separate runtimes because a script is loaded
/// into a runtime and copied into a session at launch: one runtime could not
/// hold three different versions of the same session's content at once.
async fn event_disorder(bundle: &mut Bundle) {
    // History ends at 1. Live redelivers 4 unchanged and then redelivers 2,
    // three positions behind the cursor, also unchanged.
    let benign = stream(
        "/w/pilot-disorder-duplicates",
        "disorder-duplicates",
        r#"{
            "history": [
                { "kind": "message", "sequence": 1, "emitted_at": "2026-08-12T09:00:01Z", "body": "a" }
            ],
            "live": [
                { "kind": "message", "sequence": 2, "emitted_at": "2026-08-12T09:00:02Z", "body": "b" },
                { "kind": "message", "sequence": 3, "emitted_at": "2026-08-12T09:00:03Z", "body": "c" },
                { "kind": "message", "sequence": 4, "emitted_at": "2026-08-12T09:00:04Z", "body": "d" },
                { "kind": "message", "sequence": 4, "emitted_at": "2026-08-12T09:00:05Z", "body": "d" },
                { "kind": "message", "sequence": 2, "emitted_at": "2026-08-12T09:00:06Z", "body": "b" },
                { "kind": "message", "sequence": 5, "emitted_at": "2026-08-12T09:00:07Z", "body": "e" }
            ]
        }"#,
    )
    .await;

    // The same position, twice, with different content.
    let contradiction = stream(
        "/w/pilot-disorder-contradiction",
        "disorder-contradiction",
        r#"{
            "history": [
                { "kind": "message", "sequence": 1, "emitted_at": "2026-08-12T09:00:01Z", "body": "a" }
            ],
            "live": [
                { "kind": "message", "sequence": 2, "emitted_at": "2026-08-12T09:00:02Z", "body": "b" },
                { "kind": "message", "sequence": 2, "emitted_at": "2026-08-12T09:00:03Z", "body": "rewritten" },
                { "kind": "message", "sequence": 3, "emitted_at": "2026-08-12T09:00:04Z", "body": "c" }
            ]
        }"#,
    )
    .await;

    // Two events are missing, and the one that would have filled a hole arrives
    // after the stream has already been refused.
    let gap = stream(
        "/w/pilot-disorder-gap",
        "disorder-gap",
        r#"{
            "history": [
                { "kind": "message", "sequence": 1, "emitted_at": "2026-08-12T09:00:01Z", "body": "a" },
                { "kind": "message", "sequence": 2, "emitted_at": "2026-08-12T09:00:02Z", "body": "b" }
            ],
            "live": [
                { "kind": "message", "sequence": 5, "emitted_at": "2026-08-12T09:00:05Z", "body": "e" },
                { "kind": "message", "sequence": 3, "emitted_at": "2026-08-12T09:00:03Z", "body": "c" }
            ]
        }"#,
    )
    .await;

    // A refetch is only a block if something downstream refuses to dispatch on
    // it. The ready pass is where that happens, and it reads the open gap off
    // the runtime's reconciliation evidence rather than re-deriving it.
    let blocked = dispatch_decision(true);
    let unblocked = dispatch_decision(false);

    let artifact = bundle
        .artifact(
            "runtime/event-disorder.json",
            &json!({
                "duplicates_and_regression": benign.to_json(),
                "contradiction": contradiction.to_json(),
                "sequence_gap": gap.to_json(),
                "dispatch": {
                    "with_open_replay_gap": {
                        "admitted": blocked.0,
                        "rejection": blocked.1,
                    },
                    "control_without_gap": {
                        "admitted": unblocked.0,
                        "rejection": unblocked.1,
                    },
                },
                "rule": "continuity is validated over every event before any filter, and the \
                         guard latches: once a stream is refused, every later read repeats the \
                         same typed refusal instead of resuming past the hole",
            }),
        )
        .expect("the event-disorder evidence is written");

    let duplicates_no_op = benign.breaks.is_empty()
        && benign.delivered == vec![2, 3, 4, 5]
        && benign.position.sequence == 5;
    let contradiction_typed = contradiction.delivered == vec![2]
        && contradiction.breaks == vec![TimelineBreak::ConflictingDuplicate; 2]
        && contradiction.position.sequence == 2;
    let gap_typed = gap.delivered.is_empty()
        && gap.breaks == vec![TimelineBreak::SequenceGap; 2]
        && gap.position == gap.anchor;
    let dispatch_blocked = blocked.0 == 0
        && blocked.1.as_deref() == Some(RejectionCode::RuntimeReconciliationIncomplete.as_str())
        && unblocked.0 == 1;

    if duplicates_no_op && contradiction_typed && gap_typed && dispatch_blocked {
        bundle.pass(
            "negative.event-disorder",
            format!(
                "one stream redelivered position 4 and then position 2 unchanged: both were \
                 dropped, the cursor never moved back and delivery was exactly {:?}. A second \
                 stream rewrote position 2 and was refused `{}`; a third skipped to 5 and was \
                 refused `{}`, and both refusals latched — the event that would have filled the \
                 hole was refused too. A candidate whose runtime still carries that open gap is \
                 refused `{}` by the ready pass, while the same candidate without it is admitted",
                benign.delivered,
                TimelineBreak::ConflictingDuplicate,
                TimelineBreak::SequenceGap,
                RejectionCode::RuntimeReconciliationIncomplete
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "negative.event-disorder",
            format!(
                "duplicates_no_op={duplicates_no_op}, contradiction_typed={contradiction_typed}, \
                 gap_typed={gap_typed}, dispatch_blocked={dispatch_blocked}; delivered \
                 {:?}/{:?}/{:?}",
                benign.delivered, contradiction.delivered, gap.delivered
            ),
        );
    }
}

/// Drain one scripted session's history and then its live stream.
///
/// History is drained first on purpose: the anchor it leaves is what live
/// delivery starts strictly after, so a redelivery from before the anchor and a
/// redelivery of something the subscription itself accepted are different cases
/// and are judged differently.
async fn stream(root: &str, slot: &str, fixture: &str) -> StreamOutcome {
    let engine = Engine::start(root, TaskId::generate()).await;
    engine
        .fake
        .load_script(&script(fixture), &[])
        .expect("the disorder script loads");
    let launched = engine
        .launch(AgentRunId::generate(), slot, None)
        .await
        .expect("the pilot seat launches");
    let binding = launched.snapshot;

    let (items, anchor) = drain_history(&engine.fake, &binding, 32)
        .await
        .expect("the scripted history pages cleanly");
    let mut subscription = engine
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding,
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: anchor,
        })
        .await
        .expect("the subscription opens");

    let mut delivered = Vec::new();
    let mut breaks = Vec::new();
    while let Some(next) = subscription.next_event() {
        match next {
            Ok(event) => delivered.push(event.position.sequence),
            Err(RuntimeError::TimelineRefetchRequired { reason }) => breaks.push(reason),
            Err(other) => panic!("a disorder script produced an unexpected refusal: {other}"),
        }
    }

    StreamOutcome {
        history: sequences(&items),
        anchor,
        delivered,
        position: subscription.position(),
        breaks,
    }
}

// ---------------------------------------------------------------------------
// A restart between intent and confirmation
// ---------------------------------------------------------------------------

/// A restart makes bindings stale without making anything terminal, and without
/// losing the effect that was already committed.
async fn restart(bundle: &mut Bundle) {
    let engine = Engine::start("/w/pilot-restart", TaskId::generate()).await;
    let run = AgentRunId::generate();
    let launched = engine
        .launch(run, "restart-a", None)
        .await
        .expect("the pilot seat launches");
    let binding = launched.snapshot;

    // The intent: one committed message, and the cursor the control plane would
    // have persisted next to it.
    engine
        .fake
        .send(&SendMessageRequest {
            binding: binding.clone(),
            message_id: MessageId::generate(),
            body: text("carry on after the restart"),
            sent_at: at(ACTED_AT),
        })
        .await
        .expect("the message commits");
    let (before, anchor) = drain_history(&engine.fake, &binding, 32)
        .await
        .expect("history drains before the restart");
    let cursor = HistoryCursor::issue(binding.binding_id(), anchor);
    let committed_before = engine.fake.committed_messages(&binding);
    let generation_before = engine.fake.generation();

    engine.fake.restart();
    let generation_after = engine.fake.generation();
    bundle.event(
        "runtime-restart",
        json!({
            "binding_id": binding.binding_id().to_string(),
            "generation_before": generation_before,
            "generation_after": generation_after,
        }),
    );

    // Nothing bound in the old generation may act.
    let stale_send = engine
        .fake
        .send(&SendMessageRequest {
            binding: binding.clone(),
            message_id: MessageId::generate(),
            body: text("a command the old binding may not issue"),
            sent_at: at(ACTED_AT),
        })
        .await;
    let stale_launch = engine
        .launch(AgentRunId::generate(), "restart-b", None)
        .await;

    // Reconciliation proposes an orphan review. It never proposes a closure.
    let stale_report = engine
        .fake
        .reconcile(std::slice::from_ref(&binding))
        .await
        .expect("reconciliation runs after a restart");
    let stale_finding = stale_report.findings.first();
    let generation_changed = matches!(
        stale_finding,
        Some(ReconciliationFinding::GenerationChanged { .. })
    );
    let proposed = stale_finding.and_then(ReconciliationFinding::proposed_state);

    // The recorded effects are untouched by the generation change.
    let content_survived = engine.fake.content(&binding).len();
    let committed_after = engine.fake.committed_messages(&binding);
    let cursor_resolves = cursor.resolve(binding.binding_id()).ok() == Some(anchor);

    // Convergence: the same run re-adopts the same native session under a new
    // binding. Adoption is the explicit act; nothing rebound on its own.
    let readopted = engine
        .fake
        .adopt(&AdoptRequest {
            agent_run_id: run,
            binding_id: RuntimeBindingId::generate(),
            native: NativeRuntimeIdentity {
                generation: generation_after,
                ..binding.identity().clone()
            },
            adopted_at: at(ACTED_AT),
        })
        .await;
    let (adopted_binding, adoption_closes) = match &readopted {
        Ok(outcome) => (
            Some(outcome.snapshot.clone()),
            closes(&engine.fake, &outcome.observation, &outcome.snapshot).await,
        ),
        Err(_) => (None, None),
    };

    let (after, converged, sessions_for_run) = match &adopted_binding {
        Some(snapshot) => {
            let replayed = drain_history(&engine.fake, snapshot, 32)
                .await
                .map(|(items, _)| sequences(&items))
                .unwrap_or_default();
            let report = engine
                .fake
                .reconcile(std::slice::from_ref(snapshot))
                .await
                .expect("reconciliation runs after the re-adoption");
            let converged = report
                .findings
                .first()
                .is_some_and(|finding| finding.action() == ReconciliationAction::Keep);
            (replayed, converged, engine.fake.sessions_for(run))
        }
        None => (Vec::new(), false, engine.fake.sessions_for(run)),
    };

    let artifact = bundle
        .artifact(
            "runtime/restart.json",
            &json!({
                "generation": { "before": generation_before, "after": generation_after },
                "durable": {
                    "recorded_events": content_survived,
                    "committed_messages_before": committed_before,
                    "committed_messages_after": committed_after,
                    "cursor_still_resolves_to_its_own_position": cursor_resolves,
                    "history_before": sequences(&before),
                    "history_after_readoption": after,
                },
                "stale": {
                    "send_refusal": stale_send.as_ref().err().map(ToString::to_string),
                    "launch_refusal": stale_launch.as_ref().err().map(ToString::to_string),
                    "finding": stale_finding.map(|finding| format!("{finding:?}")),
                    "proposed_state": proposed.map(DerivedRunState::as_str),
                    "proposal_is_terminal": proposed.is_some_and(DerivedRunState::is_terminal),
                    "action": stale_finding.map(|finding| format!("{:?}", finding.action())),
                },
                "convergence": {
                    "readopted": readopted.is_ok(),
                    "native_sessions_for_the_run": sessions_for_run,
                    "reconciles_as_matched": converged,
                    "adoption_closes_the_run": adoption_closes.map(|outcome| format!("{outcome:?}")),
                },
            }),
        )
        .expect("the restart evidence is written");

    let stale_session = matches!(
        stale_send,
        Err(RuntimeError::StaleBinding {
            rule: "the runtime generation changed since this session was bound"
        })
    );
    let stale_workspace = matches!(
        stale_launch,
        Err(RuntimeError::StaleBinding {
            rule: "the runtime generation changed since this workspace was prepared"
        })
    );
    let durable = content_survived == before.len()
        && committed_after == committed_before
        && committed_before == 1
        && cursor_resolves
        && after == sequences(&before);
    let unreconciled = generation_changed
        && proposed == Some(DerivedRunState::Orphaned)
        && stale_finding.map(ReconciliationFinding::action)
            == Some(ReconciliationAction::ProposeOrphanReview);
    let no_false_terminal =
        adoption_closes.is_none() && !proposed.is_some_and(DerivedRunState::is_terminal);

    if stale_session
        && stale_workspace
        && durable
        && unreconciled
        && no_false_terminal
        && sessions_for_run == 1
        && converged
    {
        bundle.pass(
            "negative.restart",
            format!(
                "the runtime restarted from generation {generation_before} into \
                 {generation_after} between the message and its confirmation: the committed \
                 message, its ledger entry and the issued cursor all survived, while the old \
                 binding could neither send nor launch again. Reconciliation classified it \
                 `generation_changed` and proposed `{}` — an uncertainty, never a closure. \
                 Re-adopting the same native session converged it back to `keep` and left the \
                 run with exactly one native session, so nothing was launched twice",
                DerivedRunState::Orphaned.as_str()
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "negative.restart",
            format!(
                "stale_session={stale_session}, stale_workspace={stale_workspace}, \
                 durable={durable}, unreconciled={unreconciled}, \
                 no_false_terminal={no_false_terminal}, sessions={sessions_for_run}, \
                 converged={converged}"
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// A channel that broke, and a session that is not there
// ---------------------------------------------------------------------------

/// A closed stream and a vanished session are facts about the channel. Neither
/// is allowed to read as a finished run.
async fn lost_contact(bundle: &mut Bundle) {
    let engine = Engine::start("/w/pilot-lost-contact", TaskId::generate()).await;
    let launched = engine
        .launch(AgentRunId::generate(), "lost-contact-a", None)
        .await
        .expect("the pilot seat launches");
    let binding = launched.snapshot;

    engine
        .fake
        .push_step(ScriptStep::CloseStreamWithoutTerminal);
    let subscription = engine
        .fake
        .subscribe_live(&LiveSubscribeRequest {
            binding: binding.clone(),
            kinds: SESSION_KINDS.iter().copied().collect(),
            strict_after: TimelinePosition::start_of(1),
        })
        .await
        .expect("the subscription opens");
    let closed_without_verdict = subscription.closed_without_terminal();

    // The strongest possible claim that the work finished, carried over a
    // channel that broke. It still closes nothing.
    let mut over_broken_channel = launched.observation.clone();
    over_broken_channel.contact = RuntimeContact::StreamClosed;
    over_broken_channel.state = ObservedRunState::Succeeded;
    over_broken_channel.source = ObservationSource::AuthoritativeEvent;
    let broken = closes(&engine.fake, &over_broken_channel, &binding).await;

    // The same claim over a channel that answered. This is the control: without
    // it, `None` above would be indistinguishable from an assertion that can
    // never produce anything else.
    let mut over_live_channel = over_broken_channel.clone();
    over_live_channel.contact = RuntimeContact::Reachable;
    let reachable = closes(&engine.fake, &over_live_channel, &binding).await;

    // The process disappeared: discovery reports nothing for a binding the
    // control plane still holds.
    let vanished = reconcile(
        std::slice::from_ref(&binding),
        &[],
        engine.fake.generation(),
    );
    let finding = vanished.findings.first();
    let missing = matches!(finding, Some(ReconciliationFinding::MissingSession { .. }));
    let proposed = finding.and_then(ReconciliationFinding::proposed_state);

    let artifact = bundle
        .artifact(
            "runtime/lost-contact.json",
            &json!({
                "binding_id": binding.binding_id().to_string(),
                "stream_closed_without_terminal": closed_without_verdict,
                "terminal_over_a_closed_stream": broken.map(|outcome| format!("{outcome:?}")),
                "control_terminal_over_a_live_channel": reachable.map(|outcome| format!("{outcome:?}")),
                "vanished_session": {
                    "finding": finding.map(|finding| format!("{finding:?}")),
                    "action": finding.map(|finding| format!("{:?}", finding.action())),
                    "proposed_state": proposed.map(DerivedRunState::as_str),
                },
                "lost_contact_is_terminal": DerivedRunState::LostContact.is_terminal(),
                "lost_contact_is_uncertain": DerivedRunState::LostContact.is_uncertain(),
                "rule": "an observation closes a run only when the channel was reachable, the \
                         evidence class is allowed to prove state at the binding's frozen trust \
                         grade, and the observation is fresh",
            }),
        )
        .expect("the lost-contact evidence is written");

    let never_terminal = broken.is_none() && reachable.is_some();
    let vanished_is_lost = missing
        && proposed == Some(DerivedRunState::LostContact)
        && finding.map(ReconciliationFinding::action)
            == Some(ReconciliationAction::ProposeLostContactReview);
    let uncertain =
        !DerivedRunState::LostContact.is_terminal() && DerivedRunState::LostContact.is_uncertain();

    if closed_without_verdict && never_terminal && vanished_is_lost && uncertain {
        bundle.pass(
            "negative.lost-contact",
            "a live stream ended without the session reaching a terminal state, and a `succeeded` \
             authoritative event carried over that closed channel still closed nothing — while the \
             identical event over a reachable channel did close the run, so the refusal is about \
             the broken channel and not about an assertion that never fires. A binding whose \
             session discovery can no longer find is classified `missing_session` and proposed as \
             `lost_contact`, which is uncertain and never terminal",
            &[artifact],
        );
    } else {
        bundle.fail(
            "negative.lost-contact",
            format!(
                "closed_without_verdict={closed_without_verdict}, never_terminal={never_terminal}, \
                 vanished_is_lost={vanished_is_lost}, uncertain={uncertain}"
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// A native session Kontor did not launch
// ---------------------------------------------------------------------------

/// A foreign session is offered, not taken.
///
/// Reconciliation's whole output is a proposal: both the orphan and the
/// adoptable case return `None` for a proposed state, because a state would be
/// a decision about a run that reconciliation is not allowed to make.
async fn adoption_inbox(bundle: &mut Bundle) {
    let fake = ScriptedFakeRuntime::new(capabilities(true));

    // A session with no Kontor label at all.
    fake.load_script(
        &script(
            r#"{
                "sessions": [
                    {
                        "native_id": "native-foreign-1",
                        "state": "running",
                        "observed_at": "2026-08-12T09:00:00Z"
                    }
                ]
            }"#,
        ),
        &[],
    )
    .expect("the orphan script loads");
    let discovered = fake
        .discover_sessions()
        .await
        .expect("discovery enumerates native sessions");
    let orphan_report = fake
        .reconcile(&[])
        .await
        .expect("reconciliation runs against no bindings");
    let orphan = orphan_report.findings.first();

    // The same session, now carrying the label of a run that has no binding.
    let run = AgentRunId::generate();
    fake.load_script(
        &script(
            r#"{
                "sessions": [
                    {
                        "native_id": "native-foreign-1",
                        "correlation_slot": 0,
                        "state": "running",
                        "observed_at": "2026-08-12T09:00:00Z"
                    }
                ]
            }"#,
        ),
        &[CorrelationLabel::for_run(run)],
    )
    .expect("the adoptable script loads");
    let adoptable_report = fake
        .reconcile(&[])
        .await
        .expect("reconciliation runs against no bindings");
    let adoptable = adoptable_report.findings.first();

    // Nothing was bound by any of that.
    let bound_before = fake.sessions_for(run);
    let adopt_calls_before = fake
        .calls()
        .iter()
        .filter(|call| matches!(call, AdapterCall::Adopt(_)))
        .count();

    // Binding is a separate, explicit act with its own adapter call.
    let adopted = fake
        .adopt(&AdoptRequest {
            agent_run_id: run,
            binding_id: RuntimeBindingId::generate(),
            native: NativeRuntimeIdentity {
                runtime_kind: runtime_kind(),
                host: name("fake-host"),
                generation: fake.generation(),
                native_id: ExternalId::parse("native-foreign-1").expect("a legal native id"),
            },
            adopted_at: at(ACTED_AT),
        })
        .await;
    let bound_after = fake.sessions_for(run);
    let adopt_calls_after = fake
        .calls()
        .iter()
        .filter(|call| matches!(call, AdapterCall::Adopt(_)))
        .count();

    let durable_inbox = Some(
        "session adoption remains staged until one public command can atomically record the \
         run, binding, and frozen capability snapshot"
            .to_owned(),
    );

    let artifact = bundle
        .artifact(
            "runtime/adoption-inbox.json",
            &json!({
                "discovered": discovered
                    .iter()
                    .map(|session| json!({
                        "native_id": session.identity.native_id.to_string(),
                        "generation": session.identity.generation,
                        "carries_a_kontor_label": session.correlation.is_some(),
                        "state": session.state.as_str(),
                    }))
                    .collect::<Vec<_>>(),
                "unlabelled": {
                    "finding": orphan.map(|finding| format!("{finding:?}")),
                    "action": orphan.map(|finding| format!("{:?}", finding.action())),
                    "proposed_state": orphan
                        .and_then(ReconciliationFinding::proposed_state)
                        .map(DerivedRunState::as_str),
                },
                "labelled_for_an_unbound_run": {
                    "finding": adoptable.map(|finding| format!("{finding:?}")),
                    "action": adoptable.map(|finding| format!("{:?}", finding.action())),
                    "proposed_state": adoptable
                        .and_then(ReconciliationFinding::proposed_state)
                        .map(DerivedRunState::as_str),
                },
                "binding": {
                    "sessions_for_the_run_before": bound_before,
                    "adapter_adopt_calls_before": adopt_calls_before,
                    "explicit_adoption_accepted": adopted.is_ok(),
                    "sessions_for_the_run_after": bound_after,
                    "adapter_adopt_calls_after": adopt_calls_after,
                },
                "durable_inbox_gap": durable_inbox,
            }),
        )
        .expect("the adoption-inbox evidence is written");

    let proposed_as_inbox = matches!(orphan, Some(ReconciliationFinding::Orphan { .. }))
        && orphan.map(ReconciliationFinding::action)
            == Some(ReconciliationAction::ProposeInboxEntry)
        && orphan
            .and_then(ReconciliationFinding::proposed_state)
            .is_none();
    let proposed_for_adoption = matches!(adoptable, Some(ReconciliationFinding::Adoptable { .. }))
        && adoptable.map(ReconciliationFinding::action)
            == Some(ReconciliationAction::ProposeAdoption)
        && adoptable
            .and_then(ReconciliationFinding::proposed_state)
            .is_none();
    let never_auto_bound = bound_before == 0 && adopt_calls_before == 0;
    let explicit_binds = adopted.is_ok() && bound_after == 1 && adopt_calls_after == 1;

    if proposed_as_inbox && proposed_for_adoption && never_auto_bound && explicit_binds {
        bundle.pass(
            "negative.adoption-inbox",
            format!(
                "a native session this control plane never launched was reported by discovery and \
                 classified `orphan` → `{:?}` while unlabelled and `adoptable` → `{:?}` once it \
                 carried an unbound run's label. Both propose no run state at all, and neither \
                 reached the adapter's `adopt`: the run held zero native sessions until an \
                 explicit adoption call bound one. The *durable* inbox is not wired — {}",
                ReconciliationAction::ProposeInboxEntry,
                ReconciliationAction::ProposeAdoption,
                durable_inbox
                    .as_deref()
                    .unwrap_or("no staged entry names it")
            ),
            &[artifact],
        );
    } else {
        bundle.fail(
            "negative.adoption-inbox",
            format!(
                "proposed_as_inbox={proposed_as_inbox}, \
                 proposed_for_adoption={proposed_for_adoption}, \
                 never_auto_bound={never_auto_bound}, explicit_binds={explicit_binds}"
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// Two coding accounts, and what never leaves them
// ---------------------------------------------------------------------------

/// Two accounts run side by side on separate credential homes, and nothing they
/// resolve reaches an artifact.
///
/// The two criteria are answered by one body because they are two halves of the
/// same setup: the secrecy scan is only meaningful over material that was
/// actually resolved and actually used to launch something.
async fn accounts(bundle: &mut Bundle) {
    let home_a = tempfile::TempDir::new().expect("a temporary credential home");
    let home_b = tempfile::TempDir::new().expect("a temporary credential home");
    for (home, canary) in [(&home_a, CANARY_A), (&home_b, CANARY_B)] {
        std::fs::write(
            home.path().join("auth.json"),
            format!("{{\"pilot\":\"{canary}\"}}\n"),
        )
        .expect("the canary is planted inside the approved home");
    }
    // The policy canonicalizes each approved directory once, at build time, so
    // these are the exact strings a resolution can hand to a child.
    let canonical_a = std::fs::canonicalize(home_a.path()).expect("the home canonicalizes");
    let canonical_b = std::fs::canonicalize(home_b.path()).expect("the home canonicalizes");

    let alias_a = alias("pilot-account-a");
    let alias_b = alias("pilot-account-b");
    let variable = EnvironmentVariableName::parse(ACCOUNT_HOME_VARIABLE)
        .expect("a POSIX-portable variable name");
    let policy = ResolverPolicy::builder()
        .harness(runtime_kind())
        .config_home(alias_a.clone(), home_a.path())
        .expect("the first home is approvable")
        .config_home(alias_b.clone(), home_b.path())
        .expect("the second home is approvable")
        .environment(variable.clone())
        .build();

    let project = ProjectId::generate();
    let profile_a = profile(project, "pilot-account-a", &alias_a, &variable);
    let profile_b = profile(project, "pilot-account-b", &alias_b, &variable);

    // A config-home-only policy never reaches a keychain backend: `resolve`
    // consults one only for `CredentialReferenceKind::Keychain`, and neither
    // profile names one. The production backend is therefore inert here, which
    // keeps this offline and prompt-free without a second implementation of the
    // port to keep honest.
    let keychain = SystemKeychain;
    let resolver = AccountResolver::new(&policy, &keychain);
    let resolved_a = resolver
        .resolve(&profile_a)
        .expect("the first account resolves");
    let resolved_b = resolver
        .resolve(&profile_b)
        .expect("the second account resolves");

    let applied_a = applied(&resolved_a);
    let applied_b = applied(&resolved_b);
    let delivered = |entries: &BTreeMap<String, String>, home: &std::path::Path| {
        entries
            .get(ACCOUNT_HOME_VARIABLE)
            .is_some_and(|value| std::path::Path::new(value) == home)
    };
    let separated = delivered(&applied_a, &canonical_a)
        && delivered(&applied_b, &canonical_b)
        && !applied_a.values().any(|value| value.contains(CANARY_B))
        && !applied_b.values().any(|value| value.contains(CANARY_A))
        && applied_a != applied_b;

    // Both accounts hold a live seat at the same time, each pinned to its own
    // profile. The runtime is the thing that has to be able to prove a per-run
    // account environment, so the pin is refused by a runtime that cannot.
    let engine = Engine::start("/w/pilot-accounts", TaskId::generate()).await;
    let run_a = AgentRunId::generate();
    let run_b = AgentRunId::generate();
    let launched_a = engine.launch(run_a, "account-a", Some(profile_a.id)).await;
    let launched_b = engine.launch(run_b, "account-b", Some(profile_b.id)).await;
    let concurrent = engine
        .fake
        .sessions_in(&RoleSlotKey::new(engine.team_run_id, slot("account-a")))
        == 1
        && engine
            .fake
            .sessions_in(&RoleSlotKey::new(engine.team_run_id, slot("account-b")))
            == 1;

    let blind = Engine::with(
        capabilities(false),
        "/w/pilot-accounts-blind",
        TaskId::generate(),
    )
    .await;
    let refused = blind
        .launch(AgentRunId::generate(), "account-blind", Some(profile_a.id))
        .await;

    let attribution = |run: AgentRunId,
                       profile: &AccountProfile,
                       launched: &RuntimeResult<LaunchOutcome>| {
        json!({
            "agent_run_id": run.to_string(),
            "account_profile_id": profile.id.to_string(),
            "label": profile.label.as_str(),
            "reference_kind": profile.credential_ref.kind.as_str(),
            "reference_alias": profile.credential_ref.alias.as_str(),
            "binding_id": launched.as_ref().ok().map(|outcome| outcome.snapshot.binding_id().to_string()),
            "native_id": launched
                .as_ref()
                .ok()
                .map(|outcome| outcome.snapshot.identity().native_id.to_string()),
        })
    };

    let accounts_artifact = bundle
        .artifact(
            "runtime/accounts.json",
            &json!({
                "canary_scan_control": SCAN_CONTROL,
                "harness": runtime_kind().as_str(),
                "approved_environment": [variable.as_str()],
                "seats": [
                    attribution(run_a, &profile_a, &launched_a),
                    attribution(run_b, &profile_b, &launched_b),
                ],
                "resolution": {
                    "first": {
                        "profile_id": resolved_a.profile_id().to_string(),
                        "revision": resolved_a.revision().get(),
                        "variables": resolved_a.names().iter().map(EnvironmentVariableName::as_str).collect::<Vec<_>>(),
                    },
                    "second": {
                        "profile_id": resolved_b.profile_id().to_string(),
                        "revision": resolved_b.revision().get(),
                        "variables": resolved_b.names().iter().map(EnvironmentVariableName::as_str).collect::<Vec<_>>(),
                    },
                    "homes_are_distinct_and_not_crossed": separated,
                },
                "both_seats_live_at_once": concurrent,
                "control_runtime_without_account_environment": refused
                    .as_ref()
                    .err()
                    .map(ToString::to_string),
                "rule": "a profile stores a closed reference kind and an opaque alias; only an \
                         in-memory operator policy maps an alias to a real place, and the resolved \
                         value's single exit is a child process environment block",
            }),
        )
        .expect("the accounts evidence is written");

    let attributed = launched_a
        .as_ref()
        .is_ok_and(|outcome| outcome.snapshot.agent_run_id() == run_a)
        && launched_b
            .as_ref()
            .is_ok_and(|outcome| outcome.snapshot.agent_run_id() == run_b)
        && resolved_a.profile_id() == profile_a.id
        && resolved_b.profile_id() == profile_b.id
        && profile_a.id != profile_b.id;
    let pin_needs_proof = matches!(refused, Err(RuntimeError::AccountEnvironmentUnavailable));

    if separated && concurrent && attributed && pin_needs_proof {
        bundle.pass(
            "project.two-accounts",
            format!(
                "two profiles storing nothing but `{}` aliases resolved through separate approved \
                 homes into separate child environments — neither carrying the other's material — \
                 and both held a live seat on one runtime at the same time, each attributed to its \
                 own account-profile id. A runtime that cannot prove a per-run account environment \
                 refused the same pin with `{}`",
                CredentialReferenceKind::ConfigHome.as_str(),
                RuntimeError::AccountEnvironmentUnavailable
            ),
            std::slice::from_ref(&accounts_artifact),
        );
    } else {
        bundle.fail(
            "project.two-accounts",
            format!(
                "separated={separated}, concurrent={concurrent}, attributed={attributed}, \
                 pin_needs_proof={pin_needs_proof}"
            ),
        );
    }

    // --- secrecy -----------------------------------------------------------

    // Everything this section can render: the approvals, the resolver, both
    // resolved environments in Debug and Display, both stored profiles, and the
    // runtime's own call log and bindings.
    let rendered = format!(
        "{policy:?} {resolver:?} {resolved_a:?} {resolved_a} {resolved_b:?} {resolved_b} \
         {profile_a:?} {profile_b:?} {:?} {:?} {:?}",
        engine.fake.calls(),
        launched_a.as_ref().ok().map(|outcome| &outcome.snapshot),
        launched_b.as_ref().ok().map(|outcome| &outcome.snapshot),
    );
    let home_a_text = canonical_a.to_string_lossy().into_owned();
    let home_b_text = canonical_b.to_string_lossy().into_owned();
    let needles = [
        CANARY_A,
        CANARY_B,
        home_a_text.as_str(),
        home_b_text.as_str(),
        SCAN_CONTROL,
    ];
    let leaked_in_renderings: Vec<&str> = needles
        .iter()
        .filter(|needle| **needle != SCAN_CONTROL && rendered.contains(*needle))
        .copied()
        .collect();
    // The renderings are not merely empty: the non-secret half is still there.
    let renderings_are_useful =
        rendered.contains(ACCOUNT_HOME_VARIABLE) && rendered.contains(&profile_a.id.to_string());

    let mut hits: Vec<(String, String)> = Vec::new();
    for root in [bundle.ephemeral(), bundle.retained()] {
        hits.extend(scan_for_canaries(root, &needles).expect("the bundle scan walks both roots"));
    }
    let control_found = hits.iter().any(|(needle, _)| needle == SCAN_CONTROL);
    let leaked_in_bundle: Vec<&(String, String)> = hits
        .iter()
        .filter(|(needle, _)| needle != SCAN_CONTROL)
        .collect();

    let secrecy_artifact = bundle
        .artifact(
            "runtime/account-secrecy.json",
            &json!({
                "scanned_roots": ["target/kontor-pilot", "docs/evidence"],
                "needle_classes": [
                    "credential material planted inside each approved home",
                    "the canonical path of each approved home",
                    "a deliberate control string this section wrote into the bundle",
                ],
                "control_string_found": control_found,
                "control_hits": hits
                    .iter()
                    .filter(|(needle, _)| needle == SCAN_CONTROL)
                    .map(|(_, path)| path.clone())
                    .collect::<Vec<_>>(),
                "leaked_in_bundle": leaked_in_bundle
                    .iter()
                    .map(|(_, path)| path.clone())
                    .collect::<Vec<_>>(),
                "leaked_in_renderings": leaked_in_renderings.len(),
                "renderings_still_name_the_non_secret_half": renderings_are_useful,
            }),
        )
        .expect("the secrecy evidence is written");

    if leaked_in_bundle.is_empty()
        && leaked_in_renderings.is_empty()
        && control_found
        && renderings_are_useful
    {
        bundle.pass(
            "project.account-secrecy",
            "neither account's planted credential material nor either approved home's canonical \
             path appears in the redacted policy, resolver, resolved-environment, profile, binding \
             or adapter-call renderings, nor anywhere under either bundle root — while a control \
             string this section deliberately wrote *was* found by the same scan and the same \
             renderings still name the variable and the profile ids, so the absence is a result \
             rather than an empty search",
            &[secrecy_artifact, accounts_artifact],
        );
    } else {
        bundle.fail(
            "project.account-secrecy",
            format!(
                "bundle leaks={:?}, rendering leaks={leaked_in_renderings:?}, \
                 control_found={control_found}, renderings_are_useful={renderings_are_useful}",
                leaked_in_bundle
                    .iter()
                    .map(|(_, path)| path.clone())
                    .collect::<Vec<_>>()
            ),
        );
    }
}

/// Apply a resolved environment to a command that is never spawned, and read
/// the block back.
///
/// `Command::get_envs` is the only way this section observes a resolved value,
/// and it never prints one: the comparisons above are made against paths this
/// function's caller already holds.
fn applied(environment: &ResolvedAccountEnvironment) -> BTreeMap<String, String> {
    let mut command = std::process::Command::new("/nonexistent/kontor-pilot-launcher");
    environment.apply(&mut command);
    command
        .get_envs()
        .filter_map(|(key, value)| {
            Some((
                key.to_string_lossy().into_owned(),
                value?.to_string_lossy().into_owned(),
            ))
        })
        .collect()
}

/// One stored account profile: a closed reference kind, an opaque alias, and
/// nothing that resolves to anywhere.
fn profile(
    project_id: ProjectId,
    label: &str,
    credential_alias: &CredentialAlias,
    variable: &EnvironmentVariableName,
) -> AccountProfile {
    let reference = CredentialReference {
        kind: CredentialReferenceKind::ConfigHome,
        alias: credential_alias.clone(),
    };
    let created_at = at(PREPARED_AT);
    AccountProfile {
        id: AccountProfileId::generate(),
        project_id,
        label: name(label),
        external_account_id: None,
        harness: runtime_kind(),
        credential_ref: reference.clone(),
        environment: AccountEnvironmentMap::new()
            .with(variable.clone(), reference)
            .to_document()
            .expect("the environment map canonicalizes"),
        routing: document(&json!({ "schema_version": 1, "provider": "pilot" })),
        capability: document(&json!({ "schema_version": 1, "declared": ["code"] })),
        provider_identity: None,
        enabled: true,
        revision: AggregateRevision::INITIAL,
        created_at,
        updated_at: created_at,
    }
}

// ---------------------------------------------------------------------------
// A handoff between two engines
// ---------------------------------------------------------------------------

/// A sealed capsule moves the work; the binding it came from stays where it was.
///
/// Both criteria are answered here because the workspace claim only means
/// anything against a capsule that was actually sealed and actually
/// acknowledged: the successor has to be reading the same document the
/// predecessor wrote.
async fn cross_engine(bundle: &mut Bundle) {
    // One task, two engines, the same verified place.
    let task_id = TaskId::generate();
    let engine_a = Engine::start("/w/pilot-handoff", task_id).await;
    let engine_b = Engine::start("/w/pilot-handoff", task_id).await;

    let source_run = AgentRunId::generate();
    let launched_a = engine_a
        .launch(source_run, "handoff-source", None)
        .await
        .expect("the predecessor launches on runtime A");
    let binding_a = launched_a.snapshot;

    let realm = RealmId::generate();
    let successor_run = AgentRunId::generate();
    let workspace = WorkspaceRef {
        root: BoundedText::parse(engine_a.workspace.root().as_str()).expect("a bounded root"),
        branch: name("feat/pilot-cross-engine"),
        baseline_commit: ExternalId::parse("45126a6").expect("a legal commit reference"),
    };
    let capsule = handoff_capsule(realm, source_run, Some(successor_run), &workspace);

    let sealed = match capsule.canonical(realm) {
        Ok(document) => document,
        Err(error) => {
            let detail = format!("the pilot handoff capsule did not canonicalize: {error}");
            bundle.fail("project.cross-engine", detail.clone());
            bundle.fail("project.workspace-identity", detail);
            return;
        }
    };

    // The seal is the digest. Any edit produces a different one.
    let mut edited = capsule.clone();
    edited
        .risks
        .push(text("a risk appended after the capsule was sealed"));
    let resealed = edited
        .canonical(realm)
        .expect("an edited capsule is still a valid capsule");
    let immutable = resealed.hash() != sealed.hash();

    // The successor starts on the other engine, in the same place.
    let launched_b = engine_b
        .launch(successor_run, "handoff-successor", None)
        .await
        .expect("the successor launches on runtime B");
    let binding_b = launched_b.snapshot;

    let acknowledgement = acknowledge(
        realm,
        &sealed,
        successor_run,
        CommandReceiptId::generate(),
        ContentHash::of(b"kontor-pilot-handoff-acknowledgement"),
        at(ACTED_AT),
    );
    let bound_to_this_capsule = acknowledgement
        .as_ref()
        .is_ok_and(|ack| ack.ensure_acknowledges(realm, &sealed).is_ok());
    let refuses_another_capsule = acknowledgement
        .as_ref()
        .is_ok_and(|ack| ack.ensure_acknowledges(realm, &resealed).is_err());
    // The producer cannot sign off its own handover.
    let refuses_the_source = acknowledge(
        realm,
        &sealed,
        source_run,
        CommandReceiptId::generate(),
        ContentHash::of(b"kontor-pilot-handoff-acknowledgement"),
        at(ACTED_AT),
    )
    .is_err();

    // Neither runtime will vouch for the other's binding, which is the whole of
    // what "a different engine" means to the control plane.
    let a_owns_a = engine_a.fake.issued_binding(&binding_a).await.is_ok();
    let b_owns_b = engine_b.fake.issued_binding(&binding_b).await.is_ok();
    let b_disowns_a = matches!(
        engine_b.fake.issued_binding(&binding_a).await,
        Err(RuntimeError::StaleBinding {
            rule: "this runtime never issued this binding"
        })
    );
    let a_disowns_b = matches!(
        engine_a.fake.issued_binding(&binding_b).await,
        Err(RuntimeError::StaleBinding {
            rule: "this runtime never issued this binding"
        })
    );
    let original_unchanged = engine_a
        .fake
        .issued_binding(&binding_a)
        .await
        .is_ok_and(|issued| issued.snapshot() == &binding_a);

    let capsule_artifact = bundle
        .artifact(
            "snapshots/handoff-capsule.json",
            &json!({
                "realm_id": realm.to_string(),
                "handoff_id": capsule.handoff_id.to_string(),
                "continuation_mode": "cross_engine_handoff",
                "source": {
                    "agent_run_id": source_run.to_string(),
                    "runtime_binding_id": binding_a.binding_id().to_string(),
                    "native_id": binding_a.identity().native_id.to_string(),
                    "workspace_root": engine_a.workspace.root().as_str(),
                },
                "target": {
                    "agent_run_id": successor_run.to_string(),
                    "runtime_binding_id": binding_b.binding_id().to_string(),
                    "native_id": binding_b.identity().native_id.to_string(),
                    "workspace_root": engine_b.workspace.root().as_str(),
                },
                "capsule_hash": sealed.hash().as_str(),
                "edited_capsule_hash": resealed.hash().as_str(),
                "seal_changes_when_the_capsule_does": immutable,
                "carries": {
                    "attempted_work": capsule.attempted_work.len(),
                    "touched_files": capsule.touched_files.len(),
                    "commits": capsule.commits.len(),
                    "tests": capsule.tests.len(),
                    "decisions": capsule.decisions.len(),
                    "evidence": capsule.evidence.len(),
                    "remaining_work": capsule.remaining_work.len(),
                    "risks": capsule.risks.len(),
                },
                "carries_no_session_locator": true,
            }),
        )
        .expect("the handoff capsule evidence is written");

    let receipt_artifact = bundle
        .artifact(
            "receipts/handoff-acknowledgement.json",
            &json!({
                "realm_id": realm.to_string(),
                "capsule_hash": acknowledgement
                    .as_ref()
                    .ok()
                    .map(|ack| ack.capsule_hash.as_str().to_owned()),
                "receiver_run_id": acknowledgement
                    .as_ref()
                    .ok()
                    .map(|ack| ack.receiver_run_id.to_string()),
                "receipt_id": acknowledgement
                    .as_ref()
                    .ok()
                    .map(|ack| ack.receipt_id.to_string()),
                "acknowledged_at": acknowledgement
                    .as_ref()
                    .ok()
                    .map(|ack| ack.acknowledged_at.to_string()),
                "bound_to_this_capsule": bound_to_this_capsule,
                "refuses_an_edited_capsule": refuses_another_capsule,
                "refuses_the_producing_run": refuses_the_source,
                "bindings": {
                    "runtime_a_vouches_for_its_own": a_owns_a,
                    "runtime_b_vouches_for_its_own": b_owns_b,
                    "runtime_b_disowns_a": b_disowns_a,
                    "runtime_a_disowns_b": a_disowns_b,
                    "original_binding_unchanged": original_unchanged,
                },
            }),
        )
        .expect("the handoff acknowledgement evidence is written");

    let linked = acknowledgement.is_ok()
        && bound_to_this_capsule
        && refuses_another_capsule
        && refuses_the_source;
    let engines_are_distinct = a_owns_a && b_owns_b && b_disowns_a && a_disowns_b;

    if immutable && linked && engines_are_distinct && original_unchanged {
        bundle.pass(
            "project.cross-engine",
            format!(
                "the predecessor on runtime A sealed a `{:?}` capsule at `{}`; the successor on \
                 runtime B acknowledged that exact digest, and the same acknowledgement refuses an \
                 edited capsule and refuses the run that produced it. Appending one risk changed \
                 the seal, and neither runtime will vouch for the other's binding — so the \
                 original binding stayed with A while the linkage travelled in the document",
                ContinuationMode::CrossEngineHandoff,
                &sealed.hash().as_str()[..16]
            ),
            &[capsule_artifact.clone(), receipt_artifact.clone()],
        );
    } else {
        bundle.fail(
            "project.cross-engine",
            format!(
                "immutable={immutable}, linked={linked}, \
                 engines_are_distinct={engines_are_distinct}, \
                 original_unchanged={original_unchanged}"
            ),
        );
    }

    // --- the same verified workspace, on both planes ------------------------

    // What the successor actually reads is the capsule's own bytes, so the
    // workspace claim is re-derived from them rather than from the value that
    // was serialized.
    let imported: Option<HandoffCapsule> = sealed.deserialize().ok();
    let capsule_agrees = imported
        .as_ref()
        .is_some_and(|parsed| parsed.workspace == workspace);
    // The successor's own onward capsule carries the same reference.
    let onward = handoff_capsule(realm, successor_run, None, &workspace);
    let successor_agrees = onward.workspace == capsule.workspace;
    let runtime_agrees = engine_a.workspace.root() == engine_b.workspace.root()
        && engine_a.workspace.root().as_str() == workspace.root.as_str()
        && engine_a.workspace.binding.task_id == engine_b.workspace.binding.task_id;

    let workspace_artifact = bundle
        .artifact(
            "snapshots/handoff-workspace.json",
            &json!({
                "task_id": task_id.to_string(),
                "capsule_workspace": {
                    "root": workspace.root.as_str(),
                    "branch": workspace.branch.as_str(),
                    "baseline_commit": workspace.baseline_commit.to_string(),
                },
                "predecessor_runtime_workspace": {
                    "root": engine_a.workspace.root().as_str(),
                    "workspace_binding_id": engine_a.workspace.binding_id().to_string(),
                    "team_run_id": engine_a.team_run_id.to_string(),
                },
                "successor_runtime_workspace": {
                    "root": engine_b.workspace.root().as_str(),
                    "workspace_binding_id": engine_b.workspace.binding_id().to_string(),
                    "team_run_id": engine_b.team_run_id.to_string(),
                },
                "imported_capsule_agrees": capsule_agrees,
                "successor_capsule_agrees": successor_agrees,
                "runtime_planes_agree": runtime_agrees,
                "rule": "the workspace binding is per-runtime because only the runtime that \
                         prepared it can vouch for it; the *place* and the task it serves are the \
                         thing both runs share",
            }),
        )
        .expect("the workspace-identity evidence is written");

    if capsule_agrees && successor_agrees && runtime_agrees {
        bundle.pass(
            "project.workspace-identity",
            format!(
                "the capsule read back out of its own sealed bytes names `{}` on branch `{}` at \
                 baseline `{}`, the successor's onward capsule names the identical reference, and \
                 both runtimes prepared that same root for the same task — each under its own \
                 workspace binding, because a binding is a runtime's own attestation and does not \
                 travel",
                workspace.root.as_str(),
                workspace.branch.as_str(),
                workspace.baseline_commit
            ),
            &[workspace_artifact, capsule_artifact, receipt_artifact],
        );
    } else {
        bundle.fail(
            "project.workspace-identity",
            format!(
                "capsule_agrees={capsule_agrees}, successor_agrees={successor_agrees}, \
                 runtime_agrees={runtime_agrees}"
            ),
        );
    }
}

/// One portable capsule for the pilot's cross-engine handoff.
///
/// Every list is spelled out because the document refuses an omitted category:
/// "no risks" and "forgot the risks" are deliberately not the same capsule.
fn handoff_capsule(
    realm_id: RealmId,
    source_run_id: AgentRunId,
    target_run_id: Option<AgentRunId>,
    workspace: &WorkspaceRef,
) -> HandoffCapsule {
    HandoffCapsule {
        schema_version: SCHEMA_VERSION,
        realm_id,
        handoff_id: HandoffId::generate(),
        continuation_mode: ContinuationMode::CrossEngineHandoff,
        source_run_id,
        target_run_id,
        context_pack_id: ContextPackId::generate(),
        context_pack_hash: ContentHash::of(b"kontor-pilot-context-pack"),
        workspace: workspace.clone(),
        attempted_work: vec![
            text("prepared the pilot task workspace"),
            text("ran the pilot code step to its first checkpoint"),
        ],
        touched_files: vec![text("tests/e2e/pilot_sections/runtime.rs")],
        commits: vec![ExternalId::parse("9f2c1ab").expect("a legal commit reference")],
        tests: vec![TestAttempt {
            command: text("cargo test -p kontor-tests-e2e --test pilot"),
            result: TestResult::Passed,
        }],
        decisions: vec![text(
            "the successor continues from the capsule, never from a provider session locator",
        )],
        evidence: vec![
            ExternalId::parse("evidence.kontor.pilot-0001").expect("a legal evidence reference"),
        ],
        remaining_work: vec![text("finish the pilot code step on the second engine")],
        risks: vec![text(
            "the baseline commit must still be the branch's baseline",
        )],
        recommended_next_action: text("resume the pilot code step from the recorded baseline"),
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

/// How many candidates the ready pass admits, and why it refused, when the
/// runtime's reconciliation still has an open replay gap.
///
/// The gap is not re-derived here: the scheduler consumes the runtime's own
/// reconciliation evidence, which is what makes a refused stream and a refused
/// dispatch the same fact rather than two opinions about it.
fn dispatch_decision(open_replay_gap: bool) -> (usize, Option<String>) {
    let project_id = ProjectId::generate();
    let taken_at = at(LAUNCHED_AT);
    let host = name("fake-host");
    let candidate = Candidate {
        project_id,
        task_id: TaskId::generate(),
        mini_project_id: None,
        workflow_id: TaskWorkflowId::generate(),
        state: TaskState::Ready,
        revision: AggregateRevision::INITIAL,
        created_at: taken_at,
        priority: 500,
        module: Some(ModuleKey::parse("pilot.code").expect("a legal module key")),
        worktree: None,
        depends_on: BTreeSet::new(),
        serializes_with: BTreeSet::new(),
        origin: TaskOrigin::Manual,
        authorization: Some(AuthorizationEvidence {
            id: ExecutionAuthorizationId::generate(),
            project_id,
            scope: WorkScope::Project,
            selected_tasks: BTreeSet::new(),
            allowed_start: at("2026-08-12T00:00:00Z"),
            allowed_end: at("2026-08-20T00:00:00Z"),
            max_concurrency: 8,
        }),
        calendar: CalendarAdmission::unrestricted(),
        runtime: RuntimeAdmissionEvidence {
            runtime_kind: runtime_kind(),
            host: host.clone(),
            generation: 1,
            capabilities: capabilities(true),
            required: minimum_launch_capabilities(),
            health: RuntimeHealth::Healthy,
            reconciliation: ReconciliationEvidence {
                epoch_completed: true,
                scope: ReconciliationScope {
                    project_id,
                    runtime_kind: runtime_kind(),
                    host,
                    generation: 1,
                },
                open_replay_gap,
                divergence: false,
                orphan_ambiguity: false,
                stale_lost_contact: false,
            },
            last_confirmed_at: Some(taken_at),
        },
        account: AccountAdmissionEvidence {
            pin: None,
            required_capabilities: BTreeSet::new(),
        },
        external: ExternalWorkEvidence::default(),
    };
    let window = AdaptiveWindowConfig {
        initial: 16,
        floor: 2,
        ceiling: 16,
        growth_step: 1,
    };
    let snapshot = SchedulingSnapshot {
        schema_version: kontor_core::id::SCHEMA_VERSION,
        taken_at,
        candidates: vec![candidate],
        in_flight_tasks: BTreeSet::new(),
        completed_tasks: BTreeSet::new(),
        module_leases: Vec::new(),
        worktree_leases: BTreeSet::new(),
        usage: CapacityUsage {
            global_in_flight: 0,
            project_in_flight: BTreeMap::new(),
            mission_in_flight: BTreeMap::new(),
            account_in_flight: BTreeMap::new(),
            provider_in_flight: BTreeMap::new(),
            runtime_in_flight: BTreeMap::new(),
        },
        capacity: CapacityConfig {
            global_max_in_flight: 16,
            project_max_in_flight: 16,
            mission_max_in_flight: 16,
            account_max_in_flight: 16,
            provider_max_in_flight: 16,
            runtime_max_in_flight: 16,
            adaptive: window,
        },
        adaptive_window: AdaptiveWindow::start(window),
        freshness: SignedDuration::from_secs(120),
    };
    let outcome = plan(&snapshot).expect("the ready pass runs");
    let code = outcome
        .decisions
        .first()
        .and_then(CandidateDecision::rejection_code)
        .map(|code| code.as_str().to_owned());
    (outcome.admitted_count(), code)
}

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

/// One fake runtime with one team run's task workspace already prepared.
struct Engine {
    /// The runtime under test.
    fake: ScriptedFakeRuntime,
    /// The team run every seat below belongs to.
    team_run_id: TeamRunId,
    /// The task the workspace serves.
    task_id: TaskId,
    /// The verified place every seat launches through.
    workspace: WorkspaceBindingSnapshot,
}

impl Engine {
    /// Bring up a fully capable runtime and prepare `root`.
    async fn start(root: &str, task_id: TaskId) -> Self {
        Self::with(capabilities(true), root, task_id).await
    }

    /// Bring up a runtime declaring exactly `declared`.
    ///
    /// # Panics
    /// Panics when the root is not an absolute path or the runtime refuses to
    /// prepare it, both of which are fixture bugs rather than findings.
    async fn with(declared: RuntimeCapabilities, root: &str, task_id: TaskId) -> Self {
        let fake = ScriptedFakeRuntime::new(declared);
        let team_run_id = TeamRunId::generate();
        let workspace = fake
            .prepare_workspace(&WorkspacePrepareRequest {
                team_run_id,
                task_id,
                workspace_binding_id: WorkspaceBindingId::generate(),
                root: WorkspaceRoot::parse(root).expect("an absolute pilot workspace root"),
                requested_at: at(PREPARED_AT),
            })
            .await
            .expect("the runtime prepares the pilot task workspace")
            .snapshot;
        Self {
            fake,
            team_run_id,
            task_id,
            workspace,
        }
    }

    /// Launch one seat of this team run, optionally pinned to a coding account.
    ///
    /// Admission is a separate runtime call on purpose: there is no other way to
    /// obtain a launch request, so a seat cannot be filled without the runtime
    /// having said yes to exactly that seat first.
    async fn launch(
        &self,
        agent_run_id: AgentRunId,
        role_slot: &str,
        account_profile_id: Option<AccountProfileId>,
    ) -> RuntimeResult<LaunchOutcome> {
        let parts = LaunchParts {
            agent_run_id,
            team_run_id: self.team_run_id,
            role_slot_id: slot(role_slot),
            task_id: self.task_id,
            binding_id: RuntimeBindingId::generate(),
            workspace: Some(self.workspace.clone()),
            cwd: self.workspace.root().clone(),
            account_profile_id,
            prompt: text("carry out the pilot step"),
            requested_at: at(LAUNCHED_AT),
        };
        let request = self
            .fake
            .admit_launch(&AdmissionRequest {
                slot: RoleSlotKey::new(parts.team_run_id, parts.role_slot_id.clone()),
                agent_run_id: parts.agent_run_id,
                binding_id: parts.binding_id,
                replaces: None,
                requested_at: parts.requested_at,
            })
            .await?
            .into_authority()?
            .into_request(parts);
        self.fake.launch(&request).await
    }
}

/// Every capability the fake declares, so nothing in this section is refused by
/// a capability that is not what the case is about.
fn capabilities(account_env: bool) -> RuntimeCapabilities {
    RuntimeCapabilities {
        trust_grade: TrustGrade::A,
        supported: RuntimeCapability::ALL.iter().copied().collect(),
        account_env,
        limits: RuntimeLimits {
            max_message_bytes: 4_096,
            max_history_page: 64,
            max_concurrent_sessions: 8,
        },
    }
}

/// Parse one inline runtime script.
///
/// # Panics
/// Panics on a fixture this file itself wrote and that will not parse, which is
/// a driver bug.
fn script(fixture: &str) -> RuntimeScript {
    serde_json::from_str(fixture).expect("the inline runtime script parses")
}

/// The runtime family the fake hard-codes.
///
/// # Panics
/// Panics on a key the domain refuses, which would be a bug in this crate.
fn runtime_kind() -> RuntimeKindKey {
    RuntimeKindKey::parse("fake.runtime").expect("a legal runtime kind")
}

/// A bounded external name.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a legal external name")
}

/// A role slot address.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn slot(text: &str) -> RoleSlotId {
    RoleSlotId::parse(text).expect("a legal role slot id")
}

/// An approved credential alias.
///
/// # Panics
/// Panics on text the domain refuses, which is a fixture bug.
fn alias(text: &str) -> CredentialAlias {
    CredentialAlias::parse(text).expect("a legal credential alias")
}

/// A canonical non-secret document.
///
/// # Panics
/// Panics on a value the core's sensitive-material scanner refuses, which is a
/// fixture bug.
fn document(value: &serde_json::Value) -> CanonicalDocument {
    CanonicalDocument::from_serializable(value).expect("a non-secret canonical document")
}
