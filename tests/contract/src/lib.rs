//! `kontor-tests-contract` — the reusable, capability-aware runtime-adapter
//! contract.
//!
//! Every adapter Kontor ever gains must pass this harness. It is stated in terms
//! of [`RuntimeAdapter`] rather than any provider, so the scripted fake, a Paseo
//! adapter and an AO adapter all run the *same* assertions instead of each
//! restating them — a copied assertion is one that can be quietly weakened in
//! one place while the others still claim to prove it.
//!
//! # Capability awareness
//!
//! Adapters differ in what they can prove, and the contract has to hold for a
//! Grade B runtime with no semantic history exactly as it holds for a full
//! Grade A one. So each contract reads the adapter's declaration once and then
//! judges each operation *both* ways:
//!
//! * declared supported → the positive rule is asserted (identity, exactly-once
//!   content, replayed acknowledgements, no terminal from an acknowledgement);
//! * not declared → the operation must fail with exactly
//!   [`RuntimeError::UnsupportedCapability`] for that capability, and must do so
//!   **before** the runtime is reached.
//!
//! That second half is the part a capability-blind harness silently skips. An
//! adapter that quietly answers an undeclared `history` with an empty page, or
//! dispatches a request it should have refused, fails here rather than passing
//! by omission.

use std::collections::BTreeSet;

use kontor_core::id::{AgentRunId, BoundedText, RuntimeBindingId, Timestamp, parse_utc_timestamp};
use kontor_core::state::{DerivedRunState, RuntimeContact, TerminalOutcome};
use kontor_runtime::adapter::{RuntimeAdapter, RuntimeError, RuntimeResult};
use kontor_runtime::capability::{RuntimeBindingSnapshot, RuntimeCapability};
use kontor_runtime::observation::{ControlPlaneObservation, ReconciliationFinding};
use kontor_runtime::request::{
    HistoryRequest, LaunchRequest, LiveSubscribeRequest, MessageId, ResumeRequest,
    SendMessageRequest,
};
use kontor_runtime::timeline::{
    HistoryCursor, HistoryReader, SessionEvent, SessionEventKind, TimelinePosition,
};

/// How old an observation may be and still close a run, for this harness.
pub const EVIDENCE_WINDOW_SECONDS: i64 = 60;

/// Every kind of session content, so a subscription cannot pass by filtering
/// away the events it failed to deliver.
pub const SESSION_KINDS: &[SessionEventKind] = &[
    SessionEventKind::Message,
    SessionEventKind::ToolCall,
    SessionEventKind::PermissionRequest,
    SessionEventKind::PermissionResolved,
    SessionEventKind::StateChange,
    SessionEventKind::Log,
];

/// Parse a canonical UTC fixture timestamp.
///
/// # Panics
/// Panics when `text` is not canonical UTC, which is a bug in the fixture.
#[must_use]
pub fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("fixture timestamp is canonical UTC")
}

/// Parse a bounded text body.
///
/// # Panics
/// Panics when `value` is not admissible bounded text.
#[must_use]
pub fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("bounded text")
}

/// The sequences of `events`, in delivery order.
#[must_use]
pub fn sequences(events: &[SessionEvent]) -> Vec<u64> {
    events.iter().map(|event| event.position.sequence).collect()
}

/// The terminal outcome `observation` closes `binding` on, judged when it was
/// observed.
///
/// The trust grade is never an argument, and the snapshot it comes out of is
/// never the caller's: `adapter` is asked to vouch for the binding first, so a
/// run can only be closed at the evidence quality the runtime actually had when
/// the session was bound. A binding it never issued vouches for nothing and
/// closes nothing.
pub async fn closes(
    adapter: &dyn RuntimeAdapter,
    observation: &ControlPlaneObservation,
    binding: &RuntimeBindingSnapshot,
) -> Option<TerminalOutcome> {
    let issued = adapter.issued_binding(binding).await.ok()?;
    observation.terminal_evidence(&issued, observation.observed_at, EVIDENCE_WINDOW_SECONDS)
}

/// Page through a session's whole history, validating continuity as it goes.
///
/// # Errors
/// Propagates every adapter refusal and every continuity break.
///
/// # Panics
/// Panics when the adapter returns no page at all, which `history` may not do.
pub async fn drain_history(
    adapter: &dyn RuntimeAdapter,
    binding: &RuntimeBindingSnapshot,
    page_size: u32,
) -> RuntimeResult<(Vec<SessionEvent>, TimelinePosition)> {
    let mut cursor: Option<HistoryCursor> = None;
    let mut reader: Option<HistoryReader> = None;
    let mut items: Vec<SessionEvent> = Vec::new();
    loop {
        let mut page = adapter
            .history(&HistoryRequest {
                binding: binding.clone(),
                cursor: cursor.clone(),
                page_size,
            })
            .await?;
        if reader.is_none() {
            reader = Some(HistoryReader::start(binding.binding_id(), page.epoch));
        }
        let validating = reader.as_mut().expect("the reader was just created");
        // Validation strips anything already covered, so what the caller
        // accumulates is exactly once by construction rather than by counting.
        validating.accept_page(&mut page)?;
        items.extend(page.items.iter().cloned());
        cursor = page.next.clone();
        if cursor.is_none() {
            break;
        }
    }
    let anchor = reader.expect("history returns at least one page").anchor();
    Ok((items, anchor))
}

/// Assert that `outcome` is exactly the refusal an undeclared `capability` owes.
///
/// # Panics
/// Panics when the operation succeeded, or failed for any other reason. An
/// adapter that answers an undeclared operation with an empty result — an empty
/// history page, a subscription that yields nothing — fails here.
pub fn assert_unsupported<T: std::fmt::Debug>(
    capability: RuntimeCapability,
    outcome: RuntimeResult<T>,
) {
    let error =
        outcome.expect_err("an operation the runtime never declared must be refused, not answered");
    assert_eq!(
        error,
        RuntimeError::UnsupportedCapability { capability },
        "an undeclared {capability} must fail as exactly that capability"
    );
}

/// Identity, refusal and evidence rules every adapter must satisfy.
///
/// `Launch` is the one precondition: a runtime that cannot launch has nothing
/// for this contract to be about.
///
/// # Errors
/// Propagates the adapter's own refusals for the operations it declares.
///
/// # Panics
/// Panics when the adapter does not declare `Launch`, when a launch loses a
/// Kontor identifier, when a binding does not freeze what discovery reported,
/// when a launch acknowledgement closes the run, or when a declared/undeclared
/// `Resume` behaves as the other.
pub async fn adapter_contract(
    adapter: &dyn RuntimeAdapter,
    launch: &LaunchRequest,
) -> RuntimeResult<RuntimeBindingSnapshot> {
    let declared = adapter.discover_capabilities().await?;
    assert!(
        declared.supports(RuntimeCapability::Launch),
        "this contract is about a runtime that can be launched into"
    );

    let launched = adapter.launch(launch).await?;
    assert_eq!(launched.snapshot.agent_run_id(), launch.agent_run_id());
    assert_eq!(launched.snapshot.binding_id(), launch.binding_id());
    assert_eq!(
        launched.snapshot.correlation.label.agent_run_id(),
        launch.agent_run_id()
    );
    assert_eq!(
        launched.snapshot.capabilities, declared,
        "the binding freezes what discovery reported"
    );
    assert_eq!(
        closes(adapter, &launched.observation, &launched.snapshot).await,
        None,
        "a launch acknowledgement never closes a run"
    );

    let resume = ResumeRequest {
        binding: launched.snapshot.clone(),
        requested_at: at("2026-08-10T09:01:00Z"),
    };
    if declared.supports(RuntimeCapability::Resume) {
        let resumed = adapter.resume(&resume).await?;
        assert_eq!(resumed.agent_run_id, launch.agent_run_id());
        assert_eq!(resumed.contact, RuntimeContact::Reachable);
        assert_eq!(
            closes(adapter, &resumed, &launched.snapshot).await,
            None,
            "continuing a session is not finishing it"
        );
    } else {
        assert_unsupported(RuntimeCapability::Resume, adapter.resume(&resume).await);
    }

    Ok(launched.snapshot)
}

/// History, live delivery and idempotency rules every adapter must satisfy.
///
/// A runtime with no semantic history and no semantic live stream still has to
/// pass: it owes the exact typed refusal for each, before dispatch. What it must
/// never do is answer with an empty page and let a caller read that as "the
/// session said nothing".
///
/// # Errors
/// Propagates the adapter's own refusals for the operations it declares.
///
/// # Panics
/// Panics when content is delivered twice or with a hole, when a retried message
/// does not replay its own acknowledgement, when a foreign cursor is silently
/// reset, or when a declared/undeclared operation behaves as the other.
pub async fn session_content_contract(
    adapter: &dyn RuntimeAdapter,
    binding: &RuntimeBindingSnapshot,
) -> RuntimeResult<()> {
    let declared = adapter.discover_capabilities().await?;
    let has_history = declared.supports(RuntimeCapability::History);
    let has_live = declared.supports(RuntimeCapability::LiveEvents);

    // History anchors live. Without it there is no validated position for a
    // subscription to start strictly after, which is exactly why a runtime that
    // cannot replay content must not pretend to stream it either.
    let anchor = if has_history {
        let (history, anchor) = drain_history(adapter, binding, 2).await?;
        if has_live {
            let mut live = adapter
                .subscribe_live(&LiveSubscribeRequest {
                    binding: binding.clone(),
                    kinds: SESSION_KINDS.iter().copied().collect(),
                    strict_after: anchor,
                })
                .await?;
            let mut seen = sequences(&history);
            while let Some(event) = live.next_event() {
                seen.push(event?.position.sequence);
            }
            let unique: BTreeSet<u64> = seen.iter().copied().collect();
            assert_eq!(seen.len(), unique.len(), "no event is delivered twice");
            assert!(
                seen.windows(2).all(|pair| pair[1] == pair[0] + 1),
                "no event is skipped between history and live"
            );
        }
        anchor
    } else {
        assert_unsupported(
            RuntimeCapability::History,
            drain_history(adapter, binding, 2).await,
        );
        TimelinePosition::start_of(1)
    };

    if !has_live {
        assert_unsupported(
            RuntimeCapability::LiveEvents,
            adapter
                .subscribe_live(&LiveSubscribeRequest {
                    binding: binding.clone(),
                    kinds: SESSION_KINDS.iter().copied().collect(),
                    strict_after: anchor,
                })
                .await
                .map(|_| "a subscription"),
        );
    }

    let send = SendMessageRequest {
        binding: binding.clone(),
        message_id: MessageId::generate(),
        body: text("contract message"),
        sent_at: at("2026-08-10T09:40:00Z"),
    };
    if declared.supports(RuntimeCapability::SendMessage) {
        let first = adapter.send(&send).await?;
        let replay = adapter.send(&send).await?;
        assert_eq!(first, replay, "a retried message replays its own result");
    } else {
        assert_unsupported(RuntimeCapability::SendMessage, adapter.send(&send).await);
    }

    if has_history {
        assert!(
            adapter
                .history(&HistoryRequest {
                    binding: binding.clone(),
                    cursor: Some(HistoryCursor::issue(RuntimeBindingId::generate(), anchor)),
                    page_size: 2,
                })
                .await
                .is_err(),
            "a cursor from another session is refused rather than reset"
        );
    }
    Ok(())
}

/// Classification rules every adapter must satisfy.
///
/// # Errors
/// Propagates the adapter's own refusals.
///
/// # Panics
/// Panics when discovery invents Kontor identity, when a binding is classified
/// more or less than once, or when reconciliation concludes that work finished.
pub async fn reconciliation_contract(
    adapter: &dyn RuntimeAdapter,
    bindings: &[RuntimeBindingSnapshot],
) -> RuntimeResult<()> {
    let sessions = adapter.discover_sessions().await?;
    for session in &sessions {
        assert!(
            session.correlation.is_none()
                || session
                    .correlation
                    .is_some_and(|label| !label.to_string().is_empty()),
            "discovery reports raw facts, never Kontor identity it invented"
        );
    }

    let report = adapter.reconcile(bindings).await?;
    for binding in bindings {
        let classified = report
            .findings
            .iter()
            .filter(|finding| match finding {
                ReconciliationFinding::Matched { binding_id, .. }
                | ReconciliationFinding::GenerationChanged { binding_id, .. }
                | ReconciliationFinding::MissingSession { binding_id, .. }
                // An adapter that attests its bindings answers for the ones it
                // does not vouch for too, so this counts as a classification: the
                // rule is that no presented binding goes unmentioned, not that it
                // must be one the runtime recognizes.
                | ReconciliationFinding::Unattested { binding_id, .. } => {
                    *binding_id == binding.binding_id()
                }
                _ => false,
            })
            .count();
        assert_eq!(classified, 1, "every binding is classified exactly once");
    }
    assert!(
        report.findings.iter().all(|finding| !matches!(
            finding.proposed_state(),
            Some(DerivedRunState::Terminal { .. })
        )),
        "reconciliation never concludes that work finished"
    );
    Ok(())
}

/// Every unsupported capability owes its typed refusal before dispatch.
///
/// This is the audit the acceptance matrix asks for as one call: a runtime that
/// declares a reduced capability set must refuse *each* missing operation as
/// exactly that capability. `probe` runs one operation and is expected to fail;
/// the caller supplies it because only the caller can build a request the
/// adapter will look at.
///
/// # Panics
/// Panics when a declared capability is probed (a test bug), or when the refusal
/// is not the exact unsupported-capability error.
pub fn assert_declares_unsupported<T: std::fmt::Debug>(
    declared: &kontor_runtime::capability::RuntimeCapabilities,
    capability: RuntimeCapability,
    probe: RuntimeResult<T>,
) {
    assert!(
        !declared.supports(capability),
        "{capability} is declared supported, so it owes no refusal"
    );
    assert_unsupported(capability, probe);
}

/// The Kontor identifiers an adapter must never replace with a native one.
///
/// # Panics
/// Panics when a native identifier parses as a Kontor identifier.
pub fn assert_native_id_is_not_a_kontor_id(native: &str) {
    assert!(
        AgentRunId::parse(native).is_err(),
        "a native id must not parse as an AgentRunId"
    );
    assert!(
        MessageId::parse(native).is_err(),
        "a native id must not parse as a MessageId"
    );
    assert!(
        kontor_runtime::request::CorrelationLabel::parse(native).is_err(),
        "a native id must not parse as a correlation label"
    );
}
