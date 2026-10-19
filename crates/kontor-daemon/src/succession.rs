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
    AccountProfileId, AgentRunId, BoundedText, CanonicalDocument, ContentHash, RuntimeBindingId,
    RuntimeKindKey, SuccessionAttemptId, Timestamp, reject_sensitive_material,
    reject_sensitive_text,
};
use kontor_core::repository::ProviderQuotaState;
use kontor_core::spec::ModelRung;
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::succession::{
    SuccessionHandoff, SuccessionHandoffDegradedReason, SuccessionHandoffOutcome,
    SuccessionRedactionPass, SuccessionRedactionReceipt, SuccessionTimelineRange,
};
use kontor_runtime::{BindingMessageTimeline, TimelinePosition};
use kontor_scheduler::headroom::{EligibleAccount, HeadroomConfig, Placement, SeatClass};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

/// Stable replacement for content removed by the succession redaction pass.
pub const REDACTED_SUCCESSION_CONTENT: &str = "<redacted>";

/// Maximum message events one summarizer request may carry.
pub const MAX_SUCCESSION_SUMMARY_MESSAGES: usize = 256;

/// Maximum total UTF-8 bytes of redacted message content dispatched once.
pub const MAX_SUCCESSION_SUMMARY_INPUT_BYTES: usize = 256 * 1024;

/// Maximum characters a summarizer may return for one durable handoff.
pub const MAX_SUCCESSION_SUMMARY_OUTPUT_CHARS: usize = 16 * 1024;

/// How long one governed summary may run before the handoff degrades.
///
/// A summary is produced while the realm-wide succession guard is held, so a
/// stalled provider call must not block every other succession in the realm.
pub const SUCCESSION_SUMMARY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

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
    /// Account admitted together with that route. It is the account the
    /// runtime must execute the summary as, so the call never falls back to an
    /// ambient account such as the exhausted predecessor's.
    pub summarizer_account: Option<AccountProfileId>,
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
/// account, credential, tool, permission or diagnostic material. The admitted
/// account travels beside the request rather than inside it: the transport must
/// execute the summary as that account and must not substitute the ambient one.
#[async_trait]
pub trait SuccessionSummarizerTransport: Send + Sync {
    /// Produce a derived summary from only the redacted message request, on the
    /// exact admitted route and account.
    async fn summarize(
        &self,
        model_rung: &ModelRung,
        account: AccountProfileId,
        request: &SuccessionSummaryRequest,
    ) -> Result<SuccessionSummaryResponse, SuccessionSummarizerError>;

    /// The configured runtime family that executes an admitted route, when this
    /// transport is backed by one.
    ///
    /// Placement uses the answer to ask *that* runtime whether a provider is
    /// enabled, exactly as a launch on it would. A transport with no runtime of
    /// its own keeps the predecessor's family, which is the only one an
    /// unconfigured realm could have observed.
    fn runtime_kind(&self) -> Option<&RuntimeKindKey> {
        None
    }
}

/// Honest fallback for a Realm that configured no summarizer runtime. It
/// launches nothing and always yields a degraded handoff.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableSuccessionSummarizer;

#[async_trait]
impl SuccessionSummarizerTransport for UnavailableSuccessionSummarizer {
    async fn summarize(
        &self,
        _model_rung: &ModelRung,
        _account: AccountProfileId,
        _request: &SuccessionSummaryRequest,
    ) -> Result<SuccessionSummaryResponse, SuccessionSummarizerError> {
        Err(SuccessionSummarizerError::Unplaceable)
    }
}

/// Production composition of the governed summarizer seam.
///
/// The exact declared [`ModelRung`] a summary runs on is resolved through the
/// shared headroom walk ([`kontor_scheduler::headroom::resolve`]) before any
/// dispatch, from the deployment's configured data: the seat's declared chain,
/// its enabled accounts, their recorded quota states and the effective headroom
/// policy. The walk is account-before-rung, so the act of summarizing can never
/// be placed on the account that just refused.
///
/// This type launches nothing and falls back to nothing. [`Placement::Wait`]
/// and [`Placement::NeedsHuman`] compose as unplaceable, and every dispatch is
/// guarded so only the exact admitted route and account can cross the transport
/// boundary; a different route is refused rather than retried or re-resolved.
pub struct GovernedSuccessionSummarizer<'a> {
    admitted: Option<AdmittedSummaryRoute>,
    transport: &'a dyn SuccessionSummarizerTransport,
}

/// The exact route and account one headroom walk admitted.
struct AdmittedSummaryRoute {
    rung: ModelRung,
    account: AccountProfileId,
}

impl<'a> GovernedSuccessionSummarizer<'a> {
    /// Resolve one exact declared route through the existing headroom walk.
    ///
    /// # Errors
    /// Returns the domain's refusal for an empty declared chain. A walk that
    /// admits no route is not an error: it composes as unplaceable so the
    /// caller keeps the typed degraded handoff.
    #[allow(clippy::too_many_arguments)]
    pub fn resolve<F>(
        declared_rungs: &[ModelRung],
        accounts: &[EligibleAccount],
        states: &[ProviderQuotaState],
        headroom: &HeadroomConfig,
        now: Timestamp,
        freshness: jiff::SignedDuration,
        provider_enabled: F,
        transport: &'a dyn SuccessionSummarizerTransport,
    ) -> kontor_core::DomainResult<Self>
    where
        F: Fn(&str) -> bool,
    {
        let placement = kontor_scheduler::headroom::resolve(
            declared_rungs,
            accounts,
            states,
            headroom,
            SeatClass::Delivery,
            now,
            freshness,
            provider_enabled,
        )?;
        let admitted = match placement {
            Placement::Admit { rung, account } => Some(AdmittedSummaryRoute { rung, account }),
            Placement::Wait { .. } | Placement::NeedsHuman { .. } => None,
        };
        Ok(Self {
            admitted,
            transport,
        })
    }

    /// Compose an explicitly unplaceable decision over one transport.
    ///
    /// This is the honest configuration root: no declared summarizer route is
    /// admitted, so no dispatch happens and the handoff stays degraded.
    #[must_use]
    pub const fn unplaceable(transport: &'a dyn SuccessionSummarizerTransport) -> Self {
        Self {
            admitted: None,
            transport,
        }
    }

    /// The exact route the walk admitted, when it admitted one.
    #[must_use]
    pub fn admitted_rung(&self) -> Option<&ModelRung> {
        self.admitted.as_ref().map(|admitted| &admitted.rung)
    }

    /// The exact account the walk admitted with that route, when it admitted one.
    #[must_use]
    pub fn admitted_account(&self) -> Option<AccountProfileId> {
        self.admitted.as_ref().map(|admitted| admitted.account)
    }
}

#[async_trait]
impl SuccessionSummarizerTransport for GovernedSuccessionSummarizer<'_> {
    async fn summarize(
        &self,
        model_rung: &ModelRung,
        account: AccountProfileId,
        request: &SuccessionSummaryRequest,
    ) -> Result<SuccessionSummaryResponse, SuccessionSummarizerError> {
        match self.admitted.as_ref() {
            Some(admitted) if admitted.rung == *model_rung && admitted.account == account => {
                // The succession guard is held across this call. A transport
                // that never answers must become a typed refusal rather than
                // holding the realm's other successions behind it.
                match tokio::time::timeout(
                    SUCCESSION_SUMMARY_TIMEOUT,
                    self.transport
                        .summarize(&admitted.rung, admitted.account, request),
                )
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err(SuccessionSummarizerError::Rejected),
                }
            }
            // No admitted route, or a route or account the declared walk did
            // not admit. Either way nothing is dispatched and no provenance is
            // invented.
            _ => Err(SuccessionSummarizerError::Unplaceable),
        }
    }

    fn runtime_kind(&self) -> Option<&RuntimeKindKey> {
        self.transport.runtime_kind()
    }
}

/// Transient redaction policy used on both sides of the summarizer boundary.
///
/// `sensitive_literals` are runtime-resolved values such as credential
/// canaries. They are held in [`SecretString`]s so these copies are zeroized on
/// drop, and they are never exposed through `Debug`, serialization, receipts or
/// errors. Each value is read only where it is substituted out of text.
pub struct SuccessionRedactionPolicy {
    policy_hash: ContentHash,
    sensitive_literals: Vec<SecretString>,
}

impl SuccessionRedactionPolicy {
    /// Construct one exact policy revision with transient literal values.
    #[must_use]
    pub fn new(policy_hash: ContentHash, sensitive_literals: Vec<SecretString>) -> Self {
        let mut sensitive_literals = sensitive_literals
            .into_iter()
            .filter(|literal| !literal.expose_secret().is_empty())
            .collect::<Vec<_>>();
        sensitive_literals.sort_by_key(|literal| std::cmp::Reverse(literal.expose_secret().len()));
        sensitive_literals.dedup_by(|left, right| left.expose_secret() == right.expose_secret());
        Self {
            policy_hash,
            sensitive_literals,
        }
    }

    fn redact_text(&self, source: &str) -> String {
        let mut redacted = redact_json_web_tokens(source);
        for literal in &self.sensitive_literals {
            redacted = redacted.replace(literal.expose_secret(), REDACTED_SUCCESSION_CONTENT);
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

/// Replace bare JSON Web Tokens with the redaction placeholder.
///
/// The masking set holds the Realm's tier secrets, not the predecessor seat's
/// own account credential — for example the Codex OAuth JWT inside
/// `CODEX_HOME/auth.json`. That value carries no `Bearer` prefix and no marker
/// [`reject_sensitive_text`] knows, so a quoted one would otherwise reach the
/// summarizer and the persisted handoff. A JWT is exactly three unpadded
/// base64url segments, and its header begins `eyJ` because the first bytes are
/// `{"`. Matching that shape removes the token without touching ordinary
/// dotted text. The rule lives here rather than in the shared persisted-string
/// scan: that scan also admits runtime evidence, and refusing a whole frame
/// because it quotes a token would break a live turn rather than mask a copy.
fn redact_json_web_tokens(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if !is_json_web_token_byte(bytes[index]) {
            index += 1;
            continue;
        }
        let mut start = index;
        while index < bytes.len() && is_json_web_token_byte(bytes[index]) {
            index += 1;
        }
        let mut end = index;
        // A full stop is both a separator and the segment character, so a JWT
        // at the end of a sentence reads as one run with an empty tail. The
        // punctuation stays in the text; only the credential is replaced.
        while start < end && bytes[start] == b'.' {
            start += 1;
        }
        while end > start && bytes[end - 1] == b'.' {
            end -= 1;
        }
        if start < end && is_json_web_token(&source[start..end]) {
            ranges.push((start, end));
        }
    }
    if ranges.is_empty() {
        return source.to_owned();
    }
    let mut redacted = String::with_capacity(source.len());
    let mut cursor = 0;
    for (start, end) in ranges {
        redacted.push_str(&source[cursor..start]);
        redacted.push_str(REDACTED_SUCCESSION_CONTENT);
        cursor = end;
    }
    redacted.push_str(&source[cursor..]);
    redacted
}

/// The unpadded base64url alphabet plus the segment separator.
const fn is_json_web_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
}

fn is_json_web_token(candidate: &str) -> bool {
    let mut segments = candidate.split('.');
    let (Some(header), Some(payload), Some(signature), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return false;
    };
    header.len() >= 16
        && payload.len() >= 8
        && signature.len() >= 16
        && header
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("eyj"))
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
        summarizer_account,
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
    let Some((summarizer_model_rung, summarizer_account)) = summarizer_model_rung
        .filter(|rung| rung.validate().is_ok())
        .zip(summarizer_account)
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
        .summarize(&summarizer_model_rung, summarizer_account, summary_request)
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
            summarizer_account: Some(summarizer_account),
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
