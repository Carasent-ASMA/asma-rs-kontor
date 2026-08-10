//! The values the control-plane event log accepts and returns.
//!
//! Three cursor spaces meet here and are deliberately never mixed:
//!
//! | Space | Owner | Type |
//! | --- | --- | --- |
//! | control-plane cursor | this store | [`EventCursor`] |
//! | native control sequence | the runtime | `u64` on [`ControlObservation`] |
//! | session-content epoch/sequence | the runtime | `u64` on [`ContentDiscontinuity`] |
//!
//! A hole in the first is a local paging question. A hole in the second is a
//! [`ControlGap`] — evidence that a *control-plane fact* never arrived. A hole in
//! the third is a [`ContentGapOutcome::TimelineRefetchRequired`] — a statement
//! that some transcript must be fetched again from the runtime, and nothing
//! else. None of the three is an outcome, and none of them closes a run.

use kontor_core::DomainError;
use kontor_core::id::{
    AgentRunId, AggregateRevision, CanonicalDocument, EventCursor, ExternalId, ProjectId, Timestamp,
};
use kontor_core::repository::{RepositoryError, RepositoryResult, RuntimeEvent};
use kontor_core::state::{
    Freshness, NativeRuntimeIdentity, ObservedRunState, RunProjection, RuntimeContact,
};

/// The control-metadata fields the durable control-plane log accepts, normalized
/// (lowercased, `-`/`_`/space removed) exactly as
/// [`kontor_core::id::reject_sensitive_material`] normalizes keys, so
/// `native_sequence`, `nativeSequence` and `native-sequence` are one field.
///
/// This is a **positive** vocabulary, and that is the whole point. A denylist of
/// the runtime's own words can only refuse what it has been taught to name, so a
/// single unlisted alias — `assistant_response`, `agent_reply`, `model_output` —
/// walks an entire transcript into the log while every listed word stays dutifully
/// blocked. An allowlist fails the other way: an unrecognized field is refused,
/// and the cost of that is a known adapter field being added here deliberately.
///
/// Every field below is a fact the control plane already owns — an id, a
/// sequence, a state, an instant, or an opaque reference — and each one is
/// traceable to a `runtime_events` column or to a typed field of
/// [`ControlObservation`]. None of them has room for the work itself.
const CONTROL_FIELDS: &[&str] = &[
    // Envelope.
    "schemaversion",
    // Native session identity and continuity.
    "runtimekind",
    "host",
    "generation",
    "nativeid",
    "nativeeventid",
    "nativesequence",
    "expectedsequence",
    "contentepoch",
    "contentsequence",
    // State, as closed vocabulary and never as prose.
    "sessionstate",
    "observedstate",
    "derivedstate",
    "desiredstate",
    "lifecycle",
    "contact",
    "freshness",
    "exitcode",
    // Instants.
    "observedat",
    "recordedat",
    "startedat",
    "endedat",
    "detectedat",
    // Opaque references into the runtime's own records.
    "auditref",
    "correlation",
    "marker",
    "reconciliationkey",
];

/// The longest an opaque control value may be, matching the bound
/// `runtime_events` puts on the reference columns it stores.
const MAX_CONTROL_TEXT: usize = 256;

/// Prove a document is control metadata and nothing else.
///
/// The shape is positive and deliberately narrow: a flat object whose every
/// member is a [`CONTROL_FIELDS`] field holding one scalar, and whose every
/// string is one short whitespace-free token. Each half of that carries its own
/// weight. The vocabulary refuses an unknown field however it is spelled, which
/// is what a denylist cannot do. The flat-scalar rule refuses the *shapes*
/// session content arrives in — a list of messages, a nested tool result, a
/// stream of deltas — so a known field name cannot be used as a lid on an
/// arbitrary subtree. The token rule refuses prose in an accepted field, the same
/// way the `audit_ref` column already refuses it in SQL.
///
/// # Errors
/// Returns [`DomainError::InvalidAt`] carrying only the offending field's name,
/// or [`DomainError::Invalid`] when the document is not an object at all. No
/// value is ever echoed — a rejected transcript must not reach a log or a test
/// assertion any more than a credential may.
pub(crate) fn ensure_control_metadata(value: &serde_json::Value) -> Result<(), DomainError> {
    let serde_json::Value::Object(members) = value else {
        return Err(DomainError::invalid(
            "ControlObservation",
            "control metadata is an object of control fields",
        ));
    };
    let refuse = |key: &str, rule: &'static str| {
        Err(DomainError::invalid_at(
            "ControlObservation",
            key.to_owned(),
            rule,
        ))
    };
    for (key, member) in members {
        let normalized: String = key
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' '))
            .flat_map(char::to_lowercase)
            .collect();
        if !CONTROL_FIELDS.contains(&normalized.as_str()) {
            return refuse(
                key,
                "the durable control-plane log stores only known control-metadata fields",
            );
        }
        match member {
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
            serde_json::Value::String(text) => {
                if text.is_empty()
                    || text.len() > MAX_CONTROL_TEXT
                    || text.chars().any(char::is_whitespace)
                {
                    return refuse(
                        key,
                        "a control field's text is one short opaque token, never prose",
                    );
                }
            }
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                return refuse(
                    key,
                    "a control field is one scalar; a nested shape is how session content arrives",
                );
            }
        }
    }
    Ok(())
}

/// Hold a canonical document that is about to enter the control-plane log to the
/// control-metadata shape.
///
/// The boundary belongs to the log, not to one entry point: every path that
/// appends a payload calls this, so no public method — evidence-complete or
/// legacy — can persist a transcript by taking a different route.
///
/// # Errors
/// Returns [`DomainError::InvalidAt`] naming only the offending field, or
/// [`DomainError::Invalid`] when the payload is not a JSON object.
pub(crate) fn ensure_no_session_content(payload: &CanonicalDocument) -> Result<(), DomainError> {
    let value: serde_json::Value = serde_json::from_str(payload.json())
        .map_err(|_| DomainError::invalid("ControlObservation", "raw payload is not JSON"))?;
    ensure_control_metadata(&value)
}

/// One control-plane observation of one native runtime session.
///
/// It carries both halves at once: the immutable `raw` document exactly as the
/// adapter canonicalized it, and the normalized fields a reduction consumes.
/// They are inserted in the same statement, so a projection effect can always
/// name the row it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlObservation {
    /// Owning project.
    pub project_id: ProjectId,
    /// The run this observation concerns.
    pub agent_run_id: AgentRunId,
    /// The native session that emitted it.
    pub identity: NativeRuntimeIdentity,
    /// The runtime's own event id, when it provides one.
    pub native_event_id: Option<ExternalId>,
    /// The runtime's own ordering for this observation.
    pub native_sequence: u64,
    /// The sequence the adapter expected next, when it tracks continuity.
    ///
    /// A received sequence beyond it records a [`ControlGap`]. `None` means the
    /// adapter makes no continuity claim, which is honest rather than a gap.
    pub expected_sequence: Option<u64>,
    /// What the runtime reported.
    pub observed: ObservedRunState,
    /// The transport result of the contact that produced it.
    pub contact: RuntimeContact,
    /// How old the newest confirmation is.
    pub freshness: Freshness,
    /// The immutable canonical control metadata, free of session content.
    pub raw: CanonicalDocument,
    /// An opaque reference to the runtime's own record of this observation.
    pub audit_ref: ExternalId,
    /// When the runtime emitted it.
    pub observed_at: Timestamp,
    /// The run revision the caller believes is current.
    pub expected_revision: AggregateRevision,
}

impl ControlObservation {
    /// Refuse runtime-owned session content before a transaction opens.
    ///
    /// # Errors
    /// Returns [`DomainError::InvalidAt`] naming only the structural path of the
    /// offending node.
    pub fn ensure_no_session_content(&self) -> Result<(), DomainError> {
        ensure_no_session_content(&self.raw)
    }
}

/// A missing stretch of the runtime's own control sequence.
///
/// It records that facts are missing. It does not record what they said, and it
/// never becomes a lifecycle or an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlGap {
    /// The sequence the adapter expected next.
    pub expected_sequence: u64,
    /// The sequence that actually arrived.
    pub received_sequence: u64,
    /// The control-plane cursor of the observation that revealed the jump.
    pub detected_cursor: EventCursor,
}

/// What appending one control-plane observation did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlObservationOutcome {
    /// The control-plane cursor of the stored row. A duplicate returns the
    /// cursor the *original* row already has.
    pub cursor: EventCursor,
    /// Whether this call appended a new row.
    pub appended: bool,
    /// Whether the projection was reduced from it.
    ///
    /// A duplicate, an older sequence and an equal sequence all leave the
    /// projection exactly as it was, and say so here rather than by looking
    /// identical to progress.
    pub reduced: bool,
    /// The projection as it stands after the call.
    pub projection: RunProjection,
    /// The continuity gap this observation revealed, if any.
    pub control_gap: Option<ControlGap>,
}

/// A discontinuity in the runtime's own **session content**, reported by an
/// adapter that noticed its transcript epoch or sequence skip.
///
/// Every field is a number, an id or an opaque reference. There is no room here
/// for the content itself, by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentDiscontinuity {
    /// Owning project.
    pub project_id: ProjectId,
    /// The run whose timeline is incomplete.
    pub agent_run_id: AgentRunId,
    /// The runtime's content epoch.
    pub content_epoch: u64,
    /// The content sequence that was expected next.
    pub expected_sequence: u64,
    /// The content sequence that actually arrived.
    pub received_sequence: u64,
    /// An opaque reference the caller can refetch the timeline by.
    pub audit_ref: ExternalId,
    /// When the discontinuity was noticed.
    pub detected_at: Timestamp,
}

/// What recording a content discontinuity requires of the caller.
///
/// There is exactly one variant, and it is not an error: a hole in a transcript
/// is a fetch obligation, not a state change. Nothing here touches desired,
/// observed, derived or lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContentGapOutcome {
    /// Refetch this run's timeline for this epoch; no lifecycle changed.
    TimelineRefetchRequired {
        /// The run whose timeline is incomplete.
        run: AgentRunId,
        /// The runtime's content epoch.
        content_epoch: u64,
        /// The content sequence that was expected next.
        expected_sequence: u64,
        /// The content sequence that actually arrived.
        received_sequence: u64,
        /// The opaque reference to refetch by.
        audit_ref: ExternalId,
    },
}

/// One page of control-plane events delivered to a persisted consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumerPage {
    /// The events, ascending, all strictly after the consumer's previous
    /// checkpoint.
    pub events: Vec<RuntimeEvent>,
    /// The checkpoint the consumer now stands at. An empty page leaves it
    /// exactly where it was.
    pub last_cursor: EventCursor,
}

/// Refuse a page size of zero.
///
/// A `limit` of 0 would return an empty page forever while looking like a
/// consumer that has caught up, so it is a caller error rather than a no-op.
///
/// # Errors
/// Returns [`RepositoryError::Domain`] when `limit` is zero.
pub(crate) fn page_limit(limit: u32) -> RepositoryResult<i64> {
    if limit == 0 {
        return Err(RepositoryError::Domain(DomainError::invalid(
            "replay page",
            "a page limit of zero can never make progress",
        )));
    }
    Ok(i64::from(limit))
}
