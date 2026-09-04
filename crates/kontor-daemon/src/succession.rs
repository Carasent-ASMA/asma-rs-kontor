//! Provider-neutral handoff production for quota-driven seat succession.
//!
//! Session content crosses this boundary only through the runtime-owned
//! [`BindingMessageTimeline`] projection. The producer applies its configured
//! redaction policy before the summarizer transport is invoked, applies the
//! same policy to the derived output, and persists neither source messages nor
//! a compaction receipt. When no governed summarizer can run, a typed degraded
//! handoff is still produced so retirement never depends on invented context.

use async_trait::async_trait;
use kontor_core::id::{
    AgentRunId, BoundedText, CanonicalDocument, ContentHash, RuntimeBindingId, SuccessionAttemptId,
    Timestamp, reject_sensitive_material, reject_sensitive_text,
};
use kontor_core::spec::ModelRung;
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::succession::{
    SuccessionHandoff, SuccessionHandoffDegradedReason, SuccessionHandoffOutcome,
    SuccessionRedactionPass, SuccessionRedactionReceipt, SuccessionTimelineRange,
};
use kontor_runtime::{BindingMessageTimeline, TimelinePosition};
use serde::Serialize;

/// Stable replacement for content removed by the succession redaction pass.
pub const REDACTED_SUCCESSION_CONTENT: &str = "<redacted>";

/// Maximum message events one summarizer request may carry.
pub const MAX_SUCCESSION_SUMMARY_MESSAGES: usize = 256;

/// Maximum total UTF-8 bytes of redacted message content dispatched once.
pub const MAX_SUCCESSION_SUMMARY_INPUT_BYTES: usize = 256 * 1024;

/// Maximum characters a summarizer may return for one durable handoff.
pub const MAX_SUCCESSION_SUMMARY_OUTPUT_CHARS: usize = 16 * 1024;

/// Exact predecessor and transient timeline inputs for one handoff.
pub struct SuccessionHandoffRequest {
    /// Succession attempt receiving this handoff.
    pub attempt_id: SuccessionAttemptId,
    /// Exact predecessor logical run.
    pub predecessor_agent_run_id: AgentRunId,
    /// Exact immutable predecessor runtime binding.
    pub predecessor_runtime_binding_id: RuntimeBindingId,
    /// Exact native identity and runtime generation behind that binding.
    pub predecessor_native_identity: NativeRuntimeIdentity,
    /// Runtime-owned, binding-scoped message projection. `None` means the
    /// runtime could not provide one stable range.
    pub timeline: Option<BindingMessageTimeline>,
    /// Governed route admitted for summarization. `None` is an explicit
    /// unplaceable decision and never causes an implicit seat launch.
    pub summarizer_model_rung: Option<ModelRung>,
    /// Instant at which the handoff evidence is produced.
    pub produced_at: Timestamp,
}

/// One already-redacted message made available to a summarizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RedactedSuccessionMessage {
    position: TimelinePosition,
    content: BoundedText,
}

impl RedactedSuccessionMessage {
    /// Exact native position of this message.
    #[must_use]
    pub const fn position(&self) -> TimelinePosition {
        self.position
    }

    /// Redacted canonical message payload.
    #[must_use]
    pub const fn content(&self) -> &BoundedText {
        &self.content
    }
}

/// Bounded provider-neutral request crossing the summarizer transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuccessionSummaryRequest {
    schema_version: u32,
    timeline: SuccessionTimelineRange,
    messages: Vec<RedactedSuccessionMessage>,
    maximum_output_characters: usize,
}

impl SuccessionSummaryRequest {
    /// Exact native range represented by the redacted message projection.
    #[must_use]
    pub const fn timeline(&self) -> SuccessionTimelineRange {
        self.timeline
    }

    /// Redacted message payloads in native position order.
    #[must_use]
    pub fn messages(&self) -> &[RedactedSuccessionMessage] {
        &self.messages
    }

    /// Hard output bound communicated to every transport implementation.
    #[must_use]
    pub const fn maximum_output_characters(&self) -> usize {
        self.maximum_output_characters
    }
}

/// Raw provider-neutral response. It is never durable until the output
/// redaction and bounded-text validation pass.
pub struct SuccessionSummaryResponse {
    summary: String,
}

impl SuccessionSummaryResponse {
    /// Wrap the transport's raw derived output for mandatory post-processing.
    #[must_use]
    pub fn new(summary: String) -> Self {
        Self { summary }
    }

    fn into_summary(self) -> String {
        self.summary
    }
}

/// Content-free refusal from the provider-neutral summarizer boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SuccessionSummarizerError {
    /// No governed provider/model/account route can accept this request.
    #[error("no governed summarizer route is placeable")]
    Unplaceable,
    /// The transport rejected the request or returned no usable answer.
    #[error("the summarizer did not produce an admissible answer")]
    Rejected,
}

/// Provider-neutral transport port for one governed summarization call.
///
/// The request has already passed input redaction and contains no raw binding,
/// account, credential, tool, permission or diagnostic material.
#[async_trait]
pub trait SuccessionSummarizerTransport: Send + Sync {
    /// Produce a derived summary from only the redacted message request.
    async fn summarize(
        &self,
        model_rung: &ModelRung,
        request: &SuccessionSummaryRequest,
    ) -> Result<SuccessionSummaryResponse, SuccessionSummarizerError>;
}

/// Honest production fallback until a configured summarizer transport is
/// composed. It launches nothing and always yields a degraded handoff.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableSuccessionSummarizer;

#[async_trait]
impl SuccessionSummarizerTransport for UnavailableSuccessionSummarizer {
    async fn summarize(
        &self,
        _model_rung: &ModelRung,
        _request: &SuccessionSummaryRequest,
    ) -> Result<SuccessionSummaryResponse, SuccessionSummarizerError> {
        Err(SuccessionSummarizerError::Unplaceable)
    }
}

/// Transient redaction policy used on both sides of the summarizer boundary.
///
/// `sensitive_literals` are runtime-resolved values such as credential
/// canaries. They are deliberately held only in memory and are never exposed
/// through `Debug`, serialization, receipts or errors.
pub struct SuccessionRedactionPolicy {
    policy_hash: ContentHash,
    sensitive_literals: Vec<String>,
}

impl SuccessionRedactionPolicy {
    /// Construct one exact policy revision with transient literal values.
    #[must_use]
    pub fn new(policy_hash: ContentHash, sensitive_literals: Vec<String>) -> Self {
        let mut sensitive_literals = sensitive_literals
            .into_iter()
            .filter(|literal| !literal.is_empty())
            .collect::<Vec<_>>();
        sensitive_literals.sort_by_key(|literal| std::cmp::Reverse(literal.len()));
        sensitive_literals.dedup();
        Self {
            policy_hash,
            sensitive_literals,
        }
    }

    fn redact_text(&self, source: &str) -> String {
        let mut redacted = source.to_owned();
        for literal in &self.sensitive_literals {
            redacted = redacted.replace(literal, REDACTED_SUCCESSION_CONTENT);
        }
        if reject_sensitive_text("succession redaction output", &redacted).is_err() {
            REDACTED_SUCCESSION_CONTENT.to_owned()
        } else {
            redacted
        }
    }

    fn redact_value(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(members) => {
                let mut redacted = serde_json::Map::with_capacity(members.len());
                for (index, (key, value)) in members.iter().enumerate() {
                    let candidate = self.redact_text(key);
                    let key = if candidate == REDACTED_SUCCESSION_CONTENT {
                        format!("redacted_field_{index}")
                    } else {
                        candidate
                    };
                    redacted.insert(key, self.redact_value(value));
                }
                serde_json::Value::Object(redacted)
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(|item| self.redact_value(item)).collect())
            }
            serde_json::Value::String(text) => serde_json::Value::String(self.redact_text(text)),
            scalar => scalar.clone(),
        }
    }
}

struct PreparedSummaryInput {
    request: Option<SuccessionSummaryRequest>,
    receipt: SuccessionRedactionReceipt,
}

/// Produce a content-bearing or explicitly degraded handoff without compacting
/// or mutating the predecessor session.
#[must_use]
pub async fn produce_succession_handoff(
    request: SuccessionHandoffRequest,
    redaction: &SuccessionRedactionPolicy,
    transport: &dyn SuccessionSummarizerTransport,
) -> SuccessionHandoff {
    let SuccessionHandoffRequest {
        attempt_id,
        predecessor_agent_run_id,
        predecessor_runtime_binding_id,
        predecessor_native_identity,
        timeline,
        summarizer_model_rung,
        produced_at,
    } = request;

    let identity_matches = timeline.as_ref().is_some_and(|timeline| {
        timeline.runtime_binding_id() == predecessor_runtime_binding_id
            && timeline.native_identity() == &predecessor_native_identity
    });
    let Some(timeline) = timeline.filter(|_| identity_matches) else {
        return handoff(
            attempt_id,
            predecessor_agent_run_id,
            predecessor_runtime_binding_id,
            predecessor_native_identity,
            SuccessionHandoffOutcome::Degraded {
                timeline: None,
                reason: SuccessionHandoffDegradedReason::TimelineUnavailable,
                input_redaction: empty_receipt(
                    SuccessionRedactionPass::Input,
                    &redaction.policy_hash,
                    produced_at,
                ),
                output_redaction: empty_receipt(
                    SuccessionRedactionPass::Output,
                    &redaction.policy_hash,
                    produced_at,
                ),
            },
            produced_at,
        );
    };
    let Some(timeline_range) = timeline.covered_range() else {
        return handoff(
            attempt_id,
            predecessor_agent_run_id,
            predecessor_runtime_binding_id,
            predecessor_native_identity,
            SuccessionHandoffOutcome::Degraded {
                timeline: None,
                reason: SuccessionHandoffDegradedReason::TimelineUnavailable,
                input_redaction: empty_receipt(
                    SuccessionRedactionPass::Input,
                    &redaction.policy_hash,
                    produced_at,
                ),
                output_redaction: empty_receipt(
                    SuccessionRedactionPass::Output,
                    &redaction.policy_hash,
                    produced_at,
                ),
            },
            produced_at,
        );
    };

    let prepared = prepare_summary_input(&timeline, timeline_range, redaction, produced_at);
    let degraded = |reason, output_redaction| {
        handoff(
            attempt_id,
            predecessor_agent_run_id,
            predecessor_runtime_binding_id,
            predecessor_native_identity.clone(),
            SuccessionHandoffOutcome::Degraded {
                timeline: Some(timeline_range),
                reason,
                input_redaction: prepared.receipt.clone(),
                output_redaction,
            },
            produced_at,
        )
    };

    let Some(summary_request) = prepared.request.as_ref() else {
        return degraded(
            SuccessionHandoffDegradedReason::SummaryRejected,
            empty_receipt(
                SuccessionRedactionPass::Output,
                &redaction.policy_hash,
                produced_at,
            ),
        );
    };
    let Some(summarizer_model_rung) = summarizer_model_rung.filter(|rung| rung.validate().is_ok())
    else {
        return degraded(
            SuccessionHandoffDegradedReason::SummarizerUnplaceable,
            empty_receipt(
                SuccessionRedactionPass::Output,
                &redaction.policy_hash,
                produced_at,
            ),
        );
    };
    let response = match transport
        .summarize(&summarizer_model_rung, summary_request)
        .await
    {
        Ok(response) => response,
        Err(SuccessionSummarizerError::Unplaceable) => {
            return degraded(
                SuccessionHandoffDegradedReason::SummarizerUnplaceable,
                empty_receipt(
                    SuccessionRedactionPass::Output,
                    &redaction.policy_hash,
                    produced_at,
                ),
            );
        }
        Err(SuccessionSummarizerError::Rejected) => {
            return degraded(
                SuccessionHandoffDegradedReason::SummaryRejected,
                empty_receipt(
                    SuccessionRedactionPass::Output,
                    &redaction.policy_hash,
                    produced_at,
                ),
            );
        }
    };

    let raw_summary = response.into_summary();
    let source_hash = ContentHash::of(raw_summary.as_bytes());
    let redacted_summary = redaction.redact_text(raw_summary.trim());
    let summary = BoundedText::parse(&redacted_summary)
        .ok()
        .filter(|summary| {
            !summary.as_str().trim().is_empty()
                && summary.as_str().chars().count() <= MAX_SUCCESSION_SUMMARY_OUTPUT_CHARS
        });
    let redacted_hash = ContentHash::of(
        summary
            .as_ref()
            .map_or(redacted_summary.as_bytes(), |summary| {
                summary.as_str().as_bytes()
            }),
    );
    let output_redaction = SuccessionRedactionReceipt {
        schema_version: 1,
        pass: SuccessionRedactionPass::Output,
        source_hash,
        redacted_hash: redacted_hash.clone(),
        policy_hash: redaction.policy_hash.clone(),
        redacted_at: produced_at,
    };
    let Some(summary) = summary else {
        return degraded(
            SuccessionHandoffDegradedReason::SummaryRejected,
            output_redaction,
        );
    };
    let summary_hash = ContentHash::of(summary.as_str().as_bytes());

    handoff(
        attempt_id,
        predecessor_agent_run_id,
        predecessor_runtime_binding_id,
        predecessor_native_identity,
        SuccessionHandoffOutcome::Summary {
            timeline: timeline_range,
            summarizer_model_rung,
            summary,
            summary_hash,
            input_redaction: prepared.receipt.clone(),
            output_redaction,
        },
        produced_at,
    )
}

fn prepare_summary_input(
    timeline: &BindingMessageTimeline,
    timeline_range: SuccessionTimelineRange,
    redaction: &SuccessionRedactionPolicy,
    at: Timestamp,
) -> PreparedSummaryInput {
    let mut source_manifest = String::new();
    let mut redacted_manifest = String::new();
    let mut messages = Vec::with_capacity(timeline.messages().len());
    let mut redacted_bytes = 0_usize;
    let mut admissible = !timeline.messages().is_empty()
        && timeline.messages().len() <= MAX_SUCCESSION_SUMMARY_MESSAGES;

    for event in timeline.messages() {
        push_manifest_entry(&mut source_manifest, event.position, event.payload.hash());
        let source: serde_json::Value = match event.payload.deserialize() {
            Ok(source) => source,
            Err(_) => {
                admissible = false;
                serde_json::json!({"schema_version": 1, "redacted": REDACTED_SUCCESSION_CONTENT})
            }
        };
        let mut redacted = redaction.redact_value(&source);
        if reject_sensitive_material(&redacted).is_err() {
            redacted =
                serde_json::json!({"schema_version": 1, "redacted": REDACTED_SUCCESSION_CONTENT});
        }
        let document = CanonicalDocument::from_value(&redacted).or_else(|_| {
            CanonicalDocument::from_value(
                &serde_json::json!({"schema_version": 1, "redacted": REDACTED_SUCCESSION_CONTENT}),
            )
        });
        let Ok(document) = document else {
            admissible = false;
            continue;
        };
        push_manifest_entry(&mut redacted_manifest, event.position, document.hash());
        redacted_bytes = redacted_bytes.saturating_add(document.json().len());
        let content = BoundedText::parse(document.json());
        match content {
            Ok(content) => messages.push(RedactedSuccessionMessage {
                position: event.position,
                content,
            }),
            Err(_) => admissible = false,
        }
    }
    admissible &= redacted_bytes <= MAX_SUCCESSION_SUMMARY_INPUT_BYTES;
    let receipt = SuccessionRedactionReceipt {
        schema_version: 1,
        pass: SuccessionRedactionPass::Input,
        source_hash: ContentHash::of(source_manifest.as_bytes()),
        redacted_hash: ContentHash::of(redacted_manifest.as_bytes()),
        policy_hash: redaction.policy_hash.clone(),
        redacted_at: at,
    };
    let request = admissible.then_some(SuccessionSummaryRequest {
        schema_version: 1,
        timeline: timeline_range,
        messages,
        maximum_output_characters: MAX_SUCCESSION_SUMMARY_OUTPUT_CHARS,
    });
    PreparedSummaryInput { request, receipt }
}

fn push_manifest_entry(
    manifest: &mut String,
    position: TimelinePosition,
    content_hash: &ContentHash,
) {
    use std::fmt::Write as _;
    let _ = writeln!(
        manifest,
        "{}:{}:{}",
        position.epoch,
        position.sequence,
        content_hash.as_str()
    );
}

fn empty_receipt(
    pass: SuccessionRedactionPass,
    policy_hash: &ContentHash,
    at: Timestamp,
) -> SuccessionRedactionReceipt {
    let empty_hash = ContentHash::of(b"");
    SuccessionRedactionReceipt {
        schema_version: 1,
        pass,
        source_hash: empty_hash.clone(),
        redacted_hash: empty_hash,
        policy_hash: policy_hash.clone(),
        redacted_at: at,
    }
}

fn handoff(
    attempt_id: SuccessionAttemptId,
    predecessor_agent_run_id: AgentRunId,
    predecessor_runtime_binding_id: RuntimeBindingId,
    predecessor_native_identity: NativeRuntimeIdentity,
    outcome: SuccessionHandoffOutcome,
    produced_at: Timestamp,
) -> SuccessionHandoff {
    SuccessionHandoff {
        schema_version: 1,
        attempt_id,
        predecessor_agent_run_id,
        predecessor_runtime_binding_id,
        predecessor_native_identity,
        outcome,
        produced_at,
    }
}
