//! Jira convergence: pinned policy in, delegated effects out.
//!
//! One ticket's convergence is four operations across the executable boundary,
//! in a fixed order, and the order is the safety property:
//!
//! ```text
//! observe -> reconcile (pure) -> dry_run -> apply (authorized) -> refetch
//! ```
//!
//! * **observe** reads the live issue, its assignee, its legal transitions and
//!   the authenticated principal. Nothing is decided here.
//! * **reconcile** is [`kontor_core::ticket::reconcile`] — a pure function over
//!   the pinned specification, the fresh observation and Kontor's own facts. This
//!   module contributes no branch to it, which is why a project with entirely
//!   different status names needs no code change.
//! * **dry_run** sends the exact request that an apply would send and writes
//!   nothing, so the request that gets reviewed is the request that runs.
//! * **apply** happens only with a named [`ApplyAuthority`], only against the
//!   transition the immediately preceding observation offered, and is only
//!   believed once a **refetch** confirms it.
//!
//! ## What is never on the wire
//!
//! A model choice, an email address, a display name as an assignee, an arbitrary
//! status, a raw AgentsRoom body, Zone C, a null field value, and an outbound
//! comment. An assignee value has exactly one source: the account id the
//! boundary read from the connector's own identity endpoint.
//!
//! A transition id is never configured and never remembered across calls. It is
//! matched on its complete destination selector from the live routes of the
//! observation the plan was computed from, so an id rewire or status rename is
//! detected instead of replayed.

use std::fmt;

use async_trait::async_trait;
use kontor_core::id::{
    AggregateRevision, BoundedText, CanonicalDocument, CommandReceiptId, ConnectorKey, ContentHash,
    ExternalId, ExternalIssueTypeKey, ExternalName, ExternalProjectKey, IdempotencyKey,
    SchemaVersion, SemanticMilestoneKey, SpecVersion, StatusTransitionReceiptId, TicketLinkId,
    TicketObservationId, WorkProfileKey,
};
use kontor_core::state::Freshness;
use kontor_core::ticket::{
    AssignmentResult, ExternalTicketObservation, ExternalWorkflowSpec, FieldEncoding, FieldOwner,
    FieldValue, InternalTaskFacts, LiveTransition, OwnershipAction, ReconciliationInput,
    ReconciliationOutcome, SelectedTransition, StatusConflictKind, StatusSelector,
    StatusTransitionReceipt, TicketFieldSpec, TicketPrincipal, TicketSyncProjection,
    TransitionPlan, reconcile,
};
use kontor_core::{DomainError, DomainResult};
use serde::{Deserialize, Serialize};

use crate::{
    JiraError as AsmaError, SelectionConflict, WIRE_SCHEMA_VERSION, WireTimestamp,
    ensure_wire_schema,
};

/// The ASMA field specifications this build ships, as data.
const BUNDLED_FIELD_SPECS: [&str; 2] = [
    include_str!("../fixtures/ticket-fields-asma.json"),
    include_str!("../fixtures/ticket-fields-asma-epic.json"),
];

/// The ASMA workflow specifications this build ships, as data.
const BUNDLED_WORKFLOW_SPECS: [&str; 4] = [
    include_str!("../fixtures/external-workflow-asma.json"),
    include_str!("../fixtures/external-workflow-asma-high-stakes.json"),
    include_str!("../fixtures/external-workflow-asma-docs.json"),
    include_str!("../fixtures/external-workflow-asma-epic-v2.json"),
];

// ---------------------------------------------------------------------------
// Specification selection
// ---------------------------------------------------------------------------

/// One field specification, validated, with the canonical bytes it hashes to.
///
/// The bytes are retained rather than recomputed on demand: a receipt that cites
/// a hash must cite the hash of the bytes that were actually used, not of a
/// re-serialization that might differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledFieldSpec {
    spec: TicketFieldSpec,
    document: CanonicalDocument,
}

/// One workflow specification, validated, with the canonical bytes it hashes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledWorkflowSpec {
    spec: ExternalWorkflowSpec,
    document: CanonicalDocument,
}

macro_rules! compiled_accessors {
    ($name:ident, $spec:ty) => {
        impl $name {
            /// The validated specification.
            #[must_use]
            pub const fn spec(&self) -> &$spec {
                &self.spec
            }

            /// The canonical bytes and digest this revision was admitted as.
            #[must_use]
            pub const fn document(&self) -> &CanonicalDocument {
                &self.document
            }

            /// The digest of [`Self::document`].
            #[must_use]
            pub const fn hash(&self) -> &ContentHash {
                self.document.hash()
            }
        }
    };
}

compiled_accessors!(CompiledFieldSpec, TicketFieldSpec);
compiled_accessors!(CompiledWorkflowSpec, ExternalWorkflowSpec);

/// The three external keys plus the exact revision a field specification is
/// selected by.
///
/// There is no "latest" selector. A work item that pinned a revision must get
/// that revision or a typed conflict; following the specification as it changes
/// underneath a running plan is the failure this type exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpecKey {
    /// The connector implementation.
    pub connector: ConnectorKey,
    /// The external project.
    pub project: ExternalProjectKey,
    /// The external issue type.
    pub issue_type: ExternalIssueTypeKey,
    /// The exact revision.
    pub version: SpecVersion,
}

/// A pinned work profile and the exact revision of it.
///
/// Both halves are mandatory, mirroring
/// [`ExternalWorkflowSpec::work_profile_version`]: naming a profile without its
/// revision would silently follow that profile as it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedProfile {
    /// The profile.
    pub key: WorkProfileKey,
    /// Its revision.
    pub version: SpecVersion,
}

/// What a workflow specification is selected by.
///
/// `work_profile` present selects the specification written for exactly that
/// profile revision. `work_profile` absent selects a *generic* specification —
/// one that declares no profile at all. A profile-specific specification is
/// never handed to an unpinned work item and vice versa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowSpecKey {
    /// The connector implementation.
    pub connector: ConnectorKey,
    /// The external project.
    pub project: ExternalProjectKey,
    /// The external issue type.
    pub issue_type: ExternalIssueTypeKey,
    /// The exact revision.
    pub version: SpecVersion,
    /// The pinned profile revision, when the work item has one.
    pub work_profile: Option<PinnedProfile>,
}

/// The loaded, validated specifications one deployment can select from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecCatalog {
    field_specs: Vec<CompiledFieldSpec>,
    workflow_specs: Vec<CompiledWorkflowSpec>,
}

impl SpecCatalog {
    /// An empty catalogue.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// The specifications bundled with this build.
    ///
    /// The bundled data has no privileged path: it goes through the same loader a
    /// deployment uses for specifications of its own.
    ///
    /// # Errors
    /// Returns [`AsmaError::Domain`] when the bundled data does not parse or does
    /// not validate — a build-time defect in the data file, not a runtime
    /// condition.
    pub fn bundled() -> Result<Self, AsmaError> {
        let mut catalog = Self::empty();
        for specification in BUNDLED_FIELD_SPECS {
            catalog.load_field_spec(specification)?;
        }
        for specification in BUNDLED_WORKFLOW_SPECS {
            catalog.load_workflow_spec(specification)?;
        }
        Ok(catalog)
    }

    /// Every field specification this catalogue holds, in load order.
    ///
    /// A read, for a caller that needs to *see* what a deployment can map rather
    /// than select one revision of it. Selection stays with
    /// [`SpecCatalog::select_field_spec`], which is the only path that decides
    /// which revision applies.
    #[must_use]
    pub fn field_specs(&self) -> &[CompiledFieldSpec] {
        &self.field_specs
    }

    /// Every workflow specification this catalogue holds, in load order.
    ///
    /// As [`SpecCatalog::field_specs`].
    #[must_use]
    pub fn workflow_specs(&self) -> &[CompiledWorkflowSpec] {
        &self.workflow_specs
    }

    /// Parse, validate, canonicalize and retain one field specification.
    ///
    /// # Errors
    /// Returns [`AsmaError::Domain`] for unparsable or invalid data.
    pub fn load_field_spec(&mut self, json: &str) -> Result<(), AsmaError> {
        let spec: TicketFieldSpec = parse_spec("TicketFieldSpec", json)?;
        let document = spec.canonicalize()?;
        self.field_specs.push(CompiledFieldSpec { spec, document });
        Ok(())
    }

    /// Parse, validate, canonicalize and retain one workflow specification.
    ///
    /// # Errors
    /// Returns [`AsmaError::Domain`] for unparsable or invalid data.
    pub fn load_workflow_spec(&mut self, json: &str) -> Result<(), AsmaError> {
        let spec: ExternalWorkflowSpec = parse_spec("ExternalWorkflowSpec", json)?;
        let document = spec.canonicalize()?;
        self.workflow_specs
            .push(CompiledWorkflowSpec { spec, document });
        Ok(())
    }

    /// Select the one field specification matching `key`.
    ///
    /// # Errors
    /// Returns [`AsmaError::Selection`] when nothing matches
    /// ([`SelectionConflict::NoMatch`]) or more than one does
    /// ([`SelectionConflict::Ambiguous`]).
    pub fn select_field_spec(&self, key: &FieldSpecKey) -> Result<&CompiledFieldSpec, AsmaError> {
        let matched: Vec<&CompiledFieldSpec> = self
            .field_specs
            .iter()
            .filter(|candidate| {
                let spec = &candidate.spec;
                spec.connector == key.connector
                    && spec.project == key.project
                    && spec.issue_type == key.issue_type
                    && spec.version == key.version
            })
            .collect();
        exactly_one("TicketFieldSpec", matched, SelectionConflict::NoMatch)
    }

    /// Select the one workflow specification matching `key`.
    ///
    /// # Errors
    /// Returns [`AsmaError::Selection`] for no match, several matches, and — when
    /// specifications exist for these keys but none at the pinned profile
    /// revision — [`SelectionConflict::ProfileRevisionMismatch`], which tells a
    /// human "your pin is stale" rather than "this project is unconfigured".
    pub fn select_workflow_spec(
        &self,
        key: &WorkflowSpecKey,
    ) -> Result<&CompiledWorkflowSpec, AsmaError> {
        let by_external_keys: Vec<&CompiledWorkflowSpec> = self
            .workflow_specs
            .iter()
            .filter(|candidate| {
                let spec = &candidate.spec;
                spec.connector == key.connector
                    && spec.project == key.project
                    && spec.issue_type == key.issue_type
                    && spec.version == key.version
            })
            .collect();
        let profile_exists = !by_external_keys.is_empty();
        let matched: Vec<&CompiledWorkflowSpec> = by_external_keys
            .into_iter()
            .filter(|candidate| profile_matches(&candidate.spec, key.work_profile.as_ref()))
            .collect();
        let absent = if profile_exists && key.work_profile.is_some() {
            SelectionConflict::ProfileRevisionMismatch
        } else {
            SelectionConflict::NoMatch
        };
        exactly_one("ExternalWorkflowSpec", matched, absent)
    }
}

/// Whether a specification's profile pin is exactly the one requested.
fn profile_matches(spec: &ExternalWorkflowSpec, requested: Option<&PinnedProfile>) -> bool {
    match (
        spec.work_profile.as_ref(),
        spec.work_profile_version,
        requested,
    ) {
        // A generic specification is eligible only for an unpinned work item.
        (None, None, None) => true,
        (Some(profile), Some(version), Some(requested)) => {
            profile == &requested.key && version == requested.version
        }
        _ => false,
    }
}

fn exactly_one<T>(
    subject: &'static str,
    mut matched: Vec<T>,
    absent: SelectionConflict,
) -> Result<T, AsmaError> {
    if matched.len() > 1 {
        return Err(AsmaError::Selection {
            subject,
            conflict: SelectionConflict::Ambiguous,
        });
    }
    matched.pop().ok_or(AsmaError::Selection {
        subject,
        conflict: absent,
    })
}

fn parse_spec<T: for<'de> Deserialize<'de>>(
    subject: &'static str,
    json: &str,
) -> Result<T, AsmaError> {
    serde_json::from_str(json).map_err(|_| {
        AsmaError::Domain(DomainError::invalid(
            subject,
            "is not a parsable specification document",
        ))
    })
}

// ---------------------------------------------------------------------------
// Projection compilation
// ---------------------------------------------------------------------------

/// One field value, resolved to what the external system means.
///
/// The *semantics* are settled here: which option id, which bounded text, which
/// number. The *serialization* into the connector's document format is the
/// boundary's job, because it already owns one verified encoder and a second
/// producer of the same document would give identical content two canonical
/// forms.
///
/// There is no variant for "clear this field". An absent projection field is
/// omitted from the request entirely, so a null field write is unrepresentable
/// rather than merely discouraged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireFieldValue {
    /// Text content, to be encoded per [`FieldWrite::encoding`].
    Text {
        /// The bounded, line-ending-normalized body.
        text: BoundedText,
    },
    /// One selected option, already resolved to its external option id.
    Select {
        /// The option id.
        option_id: ExternalId,
    },
    /// Several selected options, already resolved to their option ids.
    MultiSelect {
        /// The option ids.
        option_ids: Vec<ExternalId>,
    },
    /// An integer.
    Number {
        /// The value.
        value: i64,
    },
    /// A calendar date, as canonical `YYYY-MM-DD`.
    Date {
        /// The date text.
        value: ExternalId,
    },
    /// Free labels.
    Labels {
        /// The label texts.
        values: Vec<ExternalName>,
    },
}

impl WireFieldValue {
    fn from_core(value: &FieldValue) -> Self {
        match value {
            FieldValue::Text { body } => Self::Text { text: body.clone() },
            FieldValue::Select { option } => Self::Select {
                option_id: option.clone(),
            },
            FieldValue::MultiSelect { options } => Self::MultiSelect {
                option_ids: options.clone(),
            },
            FieldValue::Number { value } => Self::Number { value: *value },
            FieldValue::Date { value } => Self::Date {
                value: value.clone(),
            },
            FieldValue::Labels { values } => Self::Labels {
                values: values.clone(),
            },
        }
    }
}

/// One resolved outbound field write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldWrite {
    /// The external field id, from the pinned specification.
    pub field_id: ExternalId,
    /// How the boundary must encode [`Self::value`].
    pub encoding: FieldEncoding,
    /// The resolved value.
    pub value: WireFieldValue,
}

/// Resolve the outbound field writes one projection asks for.
///
/// The projection is validated against its pinned specification first, so a
/// value whose type contradicts the mapping, an option the specification does not
/// declare, an inbound-only field written outward and a private field written
/// outward are all refused before anything reaches the wire.
///
/// Only [`FieldOwner::Kontor`] and [`FieldOwner::MirrorOnly`] fields are emitted.
/// A `jira`-owned bidirectional field is readable by Kontor but is not Kontor's
/// to overwrite, so it is skipped rather than pushed.
///
/// # Errors
/// Returns [`AsmaError::Domain`] when the projection contradicts the pinned
/// specification.
pub fn compile_field_writes(
    projection: &TicketSyncProjection,
    field_spec: &CompiledFieldSpec,
) -> Result<Vec<FieldWrite>, AsmaError> {
    projection.validate(field_spec.spec())?;
    let mut writes = Vec::new();
    for field in &projection.fields {
        // Absent means "do not write". Not "clear", and not "send null".
        let Some(value) = &field.value else {
            continue;
        };
        let Some(mapping) = field_spec.spec().mapping(field.key) else {
            continue;
        };
        if !matches!(mapping.owner, FieldOwner::Kontor | FieldOwner::MirrorOnly) {
            continue;
        }
        let Some(external) = mapping.external.as_ref() else {
            continue;
        };
        writes.push(FieldWrite {
            field_id: external.field_id.clone(),
            encoding: external.encoding,
            value: WireFieldValue::from_core(value),
        });
    }
    Ok(writes)
}

// ---------------------------------------------------------------------------
// The wire
// ---------------------------------------------------------------------------

/// Which of the four operations a request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraOperation {
    /// Read the issue, its assignee and its legal transitions.
    Observe,
    /// Validate the exact request an apply would send, writing nothing.
    DryRun,
    /// Perform the validated effects, then confirm them.
    Apply,
    /// Re-read the issue to confirm or reconcile.
    Refetch,
}

impl JiraOperation {
    /// The stable spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::DryRun => "dry_run",
            Self::Apply => "apply",
            Self::Refetch => "refetch",
        }
    }
}

impl fmt::Display for JiraOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What actually happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JiraOutcome {
    /// The issue was read.
    Observed,
    /// The request validated and nothing was written.
    Planned,
    /// The effects were written and confirmed.
    Applied,
    /// There was nothing to do.
    NoOp,
    /// A typed conflict for a human.
    Conflict,
    /// The boundary could not act.
    Unavailable,
}

/// The state a request asserts the issue is still in.
///
/// The boundary revalidates this immediately before writing. Without it, a plan
/// computed against one observation could be applied to a ticket a human moved in
/// between.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpectedObservation {
    /// The status the plan was computed against.
    pub status_id: ExternalId,
    /// The assignee the plan was computed against.
    pub assignee_account_id: Option<ExternalId>,
    /// The connector's own update token, when it reports one.
    pub update_token: Option<ExternalId>,
    /// Digest of the observation the plan was computed from.
    pub observation_hash: Option<ContentHash>,
}

/// The transition an apply asks for, plus the destination it was selected for.
///
/// Both halves travel because the boundary must re-prove the route still leads
/// where the plan intended. A transition id alone would be replayable into a
/// workflow that has since been rewired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestedTransition {
    /// The opaque transition id, from the immediately preceding observation.
    pub transition_id: ExternalId,
    /// The destination status id it must still reach.
    pub to_status_id: ExternalId,
}

/// One schema-versioned machine request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JiraRequest {
    /// Wire schema generation.
    pub schema_version: SchemaVersion,
    /// The operation asked for.
    pub operation: JiraOperation,
    /// The external issue key.
    pub issue_key: ExternalId,
    /// The caller's durable idempotency key.
    pub idempotency_key: IdempotencyKey,
    /// Digest of the canonical intent this request implements.
    pub intent_hash: Option<ContentHash>,
    /// Digest of the pinned field specification.
    pub field_spec_hash: Option<ContentHash>,
    /// Digest of the pinned workflow specification.
    pub workflow_spec_hash: Option<ContentHash>,
    /// The state the plan was computed against.
    pub expected: Option<ExpectedObservation>,
    /// The resolved, non-null field writes.
    pub field_writes: Vec<FieldWrite>,
    /// The spec-derived destination selector.
    pub destination: Option<StatusSelector>,
    /// The spec-derived ownership action.
    pub ownership_action: OwnershipAction,
    /// The selected live transition, for an apply.
    pub transition: Option<RequestedTransition>,
    /// Explicit permission to write. Absent or `false` stays a dry run.
    pub authorized_apply: bool,
}

/// One observed state of the external issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireObservation {
    /// The status's opaque id.
    pub status_id: ExternalId,
    /// The status's display name; configured routes bind it together with the id.
    pub status_name: ExternalName,
    /// The status category the connector reports, evidence only.
    pub status_category: ExternalName,
    /// The issue type's display name, as the connector spells it.
    pub issue_type: ExternalName,
    /// The assignee's account id, when there is one.
    pub assignee_account_id: Option<ExternalId>,
    /// The assignee's display name, evidence only.
    pub assignee_display: Option<ExternalName>,
    /// The connector's own update token.
    pub update_token: Option<ExternalId>,
    /// Digest of the canonical observation payload.
    pub observation_hash: ContentHash,
}

impl WireObservation {
    /// Convert to the domain observation, minting its append-only identity.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the connector's issue-type name does not
    /// normalize to a legal key.
    pub fn to_core(
        &self,
        link_id: TicketLinkId,
        observed_at: WireTimestamp,
    ) -> DomainResult<ExternalTicketObservation> {
        Ok(ExternalTicketObservation {
            id: TicketObservationId::generate(),
            link_id,
            status: StatusSelector {
                status_id: self.status_id.clone(),
                status_name: self.status_name.clone(),
            },
            status_category: self.status_category.clone(),
            issue_type: normalize_issue_type(self.issue_type.as_str())?,
            assignee_account_id: self.assignee_account_id.clone(),
            assignee_display: self.assignee_display.clone(),
            external_version: self.update_token.clone(),
            observed_at: observed_at.get(),
            payload_hash: self.observation_hash.clone(),
        })
    }
}

/// Normalize a connector's issue-type display name into a domain key.
///
/// The connector reports what a human sees (`Bug`, `User Story`); the domain
/// keys are lowercase. Doing the fold here rather than on the far side keeps one
/// rule in one place: the boundary reports raw evidence and never guesses at
/// Kontor's vocabulary.
fn normalize_issue_type(display: &str) -> DomainResult<ExternalIssueTypeKey> {
    let mut key = String::with_capacity(display.len());
    for character in display.chars() {
        if character.is_ascii_alphanumeric() {
            key.extend(character.to_lowercase());
        } else if !key.ends_with('-') {
            key.push('-');
        }
    }
    ExternalIssueTypeKey::parse(key.trim_matches('-'))
}

/// One transition the connector currently offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireTransition {
    /// The opaque transition id.
    pub transition_id: ExternalId,
    /// The destination status id.
    pub to_status_id: ExternalId,
    /// The destination status name; configured routes match it with the id.
    pub to_status_name: ExternalName,
    /// The destination status category, evidence only.
    pub to_status_category: Option<ExternalName>,
}

impl WireTransition {
    /// Convert to the domain transition.
    #[must_use]
    pub fn to_core(&self) -> LiveTransition {
        LiveTransition {
            transition_id: self.transition_id.clone(),
            to: StatusSelector {
                status_id: self.to_status_id.clone(),
                status_name: self.to_status_name.clone(),
            },
        }
    }
}

/// The assignment a response planned or performed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireAssignment {
    /// Which ownership action it implements.
    pub action: OwnershipAction,
    /// The account id written, when one was.
    pub account_id: Option<ExternalId>,
}

/// Exactly what a response planned or performed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireEffects {
    /// The field ids written. Values are not echoed back.
    #[serde(default)]
    pub field_ids: Vec<ExternalId>,
    /// The assignment, when there was one.
    #[serde(default)]
    pub assignment: Option<WireAssignment>,
    /// The transition, when there was one.
    #[serde(default)]
    pub transition: Option<RequestedTransition>,
}

/// The refetched evidence that an apply actually landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireConfirmation {
    /// The observation read after the writes.
    pub observation: WireObservation,
    /// When it was read.
    pub confirmed_at: WireTimestamp,
}

/// A typed failure a response reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFailure {
    /// Short, stable reason token.
    pub reason: String,
    /// Human-readable detail.
    pub detail: String,
}

/// One schema-versioned machine response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JiraResponse {
    /// Wire schema generation.
    pub schema_version: SchemaVersion,
    /// The operation that was asked for.
    pub operation: JiraOperation,
    /// The operation that actually ran. An apply without authority runs as a
    /// dry run and says so here.
    pub effective_operation: JiraOperation,
    /// The external issue key.
    pub issue_key: ExternalId,
    /// The idempotency key the request carried.
    pub idempotency_key: IdempotencyKey,
    /// The intent digest the request carried.
    #[serde(default)]
    pub intent_hash: Option<ContentHash>,
    /// When the boundary received the request.
    pub requested_at: WireTimestamp,
    /// When it answered.
    pub completed_at: WireTimestamp,
    /// What happened.
    pub outcome: JiraOutcome,
    /// The state read at the start of the operation.
    #[serde(default)]
    pub observation: Option<WireObservation>,
    /// The authenticated principal's account id, when the boundary resolved one.
    #[serde(default)]
    pub principal_account_id: Option<ExternalId>,
    /// The routes the connector currently offers.
    #[serde(default)]
    pub live_transitions: Vec<WireTransition>,
    /// Exactly what was planned or performed.
    #[serde(default)]
    pub effects: WireEffects,
    /// The refetched evidence, for an apply.
    #[serde(default)]
    pub confirmation: Option<WireConfirmation>,
    /// The typed conflict, when the outcome is a conflict.
    #[serde(default)]
    pub conflict: Option<WireFailure>,
    /// The typed unavailable reason, when the outcome is unavailable.
    #[serde(default)]
    pub unavailable: Option<WireFailure>,
    /// Non-failure observations.
    #[serde(default)]
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Delegation
// ---------------------------------------------------------------------------

/// One observation, in both its wire and domain forms.
#[derive(Debug, Clone)]
pub struct Observed {
    /// The whole answer, kept as evidence.
    pub response: JiraResponse,
    /// The domain observation the evaluator reads.
    pub observation: ExternalTicketObservation,
    /// The routes the evaluator may select from.
    pub live_transitions: Vec<LiveTransition>,
    /// The principal Kontor acts as.
    pub principal: TicketPrincipal,
}

/// Named permission to write to the external system.
///
/// It carries the receipt that granted it rather than being a boolean, because a
/// boolean is exactly the argument that gets passed wrongly, and because "who
/// authorized this" is the first question asked after an unexpected write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyAuthority {
    /// The command receipt this delegation acts under.
    pub authorized_by: CommandReceiptId,
}

/// What a refetch proves about an apply whose result was never seen.
#[derive(Debug, Clone)]
pub enum AmbiguityVerdict {
    /// The intended effects are already in place. Return the prior result; a
    /// retry here would be a second transition.
    AlreadyConfirmed(Box<Observed>),
    /// Fresh evidence proves the first attempt had no effect. One retry under
    /// the same idempotency key is permitted.
    NoEffect(Box<Observed>),
    /// The issue is in neither the old state nor the intended one. Somebody else
    /// acted, or the effects landed partially: a human decides.
    Contradictory(Box<Observed>),
}

/// One Jira issue observation without a task or ticket-link identity.
///
/// The connector evidence is kept in its wire form because the domain ticket
/// observation requires a [`TicketLinkId`]. Epic callers persist the evidence
/// against their own binding instead of manufacturing that task-scoped id.
#[derive(Debug, Clone)]
pub struct ObservedIssue {
    /// The whole connector answer, kept as evidence.
    pub response: JiraResponse,
    /// The observed issue state.
    pub observation: WireObservation,
    /// The transitions the connector offered with this observation.
    pub live_transitions: Vec<LiveTransition>,
    /// The authenticated principal Kontor may act as.
    pub principal: TicketPrincipal,
}

/// What a refetch proves about an entity-neutral apply whose result was lost.
#[derive(Debug, Clone)]
pub enum IssueAmbiguityVerdict {
    /// The intended status and ownership effects are already present.
    AlreadyConfirmed(Box<ObservedIssue>),
    /// The issue is byte-for-byte unchanged in the decision-relevant fields.
    NoEffect(Box<ObservedIssue>),
    /// The issue is in neither the old nor intended state.
    Contradictory(Box<ObservedIssue>),
}

/// The safe transport boundary shared by task and epic convergence.
///
/// Policy stays outside this type. Its caller supplies a typed
/// [`TransitionPlan`], while this boundary proves the plan against the exact
/// observation and live route, hashes the exact pinned specifications and
/// intent, requires explicit apply authority, and believes an applied effect
/// only with connector-confirmed refetch evidence.
#[derive(Clone, Copy)]
pub struct JiraIssueDelegation<'a> {
    /// The configured connector transport.
    pub exchange: &'a dyn JiraExchange,
    /// The exact field specification selected for this entity kind.
    pub field_spec: &'a CompiledFieldSpec,
    /// The exact workflow specification selected for this entity and profile.
    pub workflow_spec: &'a CompiledWorkflowSpec,
    /// The external issue being converged.
    pub issue_key: &'a ExternalId,
    /// The internal projection revision represented by this attempt.
    pub projection_revision: AggregateRevision,
    /// Already-validated, non-null field writes for this entity.
    pub field_writes: &'a [FieldWrite],
    /// The caller's durable idempotency key.
    pub idempotency_key: &'a IdempotencyKey,
}

impl JiraIssueDelegation<'_> {
    /// Read the live issue, routes and authenticated principal.
    pub async fn observe(&self) -> Result<ObservedIssue, AsmaError> {
        self.read("jira observe", JiraOperation::Observe).await
    }

    /// Re-read the live issue for confirmation or ambiguity recovery.
    pub async fn refetch(&self) -> Result<ObservedIssue, AsmaError> {
        self.read("jira refetch", JiraOperation::Refetch).await
    }

    async fn read(
        &self,
        operation: &'static str,
        wire_operation: JiraOperation,
    ) -> Result<ObservedIssue, AsmaError> {
        let request = JiraRequest {
            schema_version: WIRE_SCHEMA_VERSION,
            operation: wire_operation,
            issue_key: self.issue_key.clone(),
            idempotency_key: self.idempotency_key.clone(),
            intent_hash: None,
            field_spec_hash: Some(self.field_spec.hash().clone()),
            workflow_spec_hash: Some(self.workflow_spec.hash().clone()),
            expected: None,
            field_writes: Vec::new(),
            destination: None,
            ownership_action: OwnershipAction::Preserve,
            transition: None,
            authorized_apply: false,
        };
        let response = self.exchange(operation, &request).await?;
        self.interpret(operation, response)
    }

    /// Validate the exact request an apply would send without writing.
    pub async fn dry_run(
        &self,
        observed: &ObservedIssue,
        plan: &TransitionPlan,
    ) -> Result<JiraResponse, AsmaError> {
        let request = self.build_write_request("jira dry-run", observed, plan, None)?;
        self.exchange("jira dry-run", &request).await
    }

    /// Apply a plan under explicit authority and require confirmed readback.
    pub async fn apply(
        &self,
        observed: &ObservedIssue,
        plan: &TransitionPlan,
        authority: ApplyAuthority,
    ) -> Result<JiraResponse, AsmaError> {
        let request = self.build_write_request("jira apply", observed, plan, Some(authority))?;
        let response = self.exchange("jira apply", &request).await?;
        if response.effective_operation != JiraOperation::Apply {
            return Err(AsmaError::unavailable(
                "jira apply",
                crate::UnavailableReason::MalformedResponse,
                format!(
                    "an authorized apply ran as {}",
                    response.effective_operation.as_str()
                ),
            ));
        }
        if response.outcome == JiraOutcome::Applied && response.confirmation.is_none() {
            return Err(AsmaError::unavailable(
                "jira apply",
                crate::UnavailableReason::MalformedResponse,
                "reported applied without a refetched observation",
            ));
        }
        self.raise_reported_failure("jira apply", &response)?;
        Ok(response)
    }

    /// Re-read an apply with an unknown result before authorizing any retry.
    pub async fn reconcile_after_ambiguity(
        &self,
        before: &ObservedIssue,
        plan: &TransitionPlan,
    ) -> Result<IssueAmbiguityVerdict, AsmaError> {
        let after = Box::new(self.refetch().await?);
        let expected_holder = issue_planned_holder(before, plan);
        let status_arrived = plan.transition.is_none()
            || (after.observation.status_id == plan.destination().status_id
                && after.observation.status_name == plan.destination().status_name);
        let holder_arrived = after.observation.assignee_account_id == expected_holder;
        if status_arrived && holder_arrived {
            return Ok(IssueAmbiguityVerdict::AlreadyConfirmed(after));
        }
        let unchanged = after.observation.status_id == before.observation.status_id
            && after.observation.status_name == before.observation.status_name
            && after.observation.assignee_account_id == before.observation.assignee_account_id;
        if unchanged {
            return Ok(IssueAmbiguityVerdict::NoEffect(after));
        }
        Ok(IssueAmbiguityVerdict::Contradictory(after))
    }

    /// Canonical, retry-stable intent for this issue attempt.
    pub fn intent(
        &self,
        observed: &ObservedIssue,
        plan: &TransitionPlan,
    ) -> Result<CanonicalDocument, AsmaError> {
        let intent = DelegationIntent {
            schema_version: WIRE_SCHEMA_VERSION,
            connector: &self.workflow_spec.spec().connector,
            project: &self.workflow_spec.spec().project,
            issue_type: &self.workflow_spec.spec().issue_type,
            external_issue_key: self.issue_key,
            field_spec_version: self.field_spec.spec().version,
            field_spec_hash: self.field_spec.hash(),
            workflow_spec_version: self.workflow_spec.spec().version,
            workflow_spec_hash: self.workflow_spec.hash(),
            projection_revision: self.projection_revision,
            prior_observation_hash: &observed.observation.observation_hash,
            prior_status_id: &observed.observation.status_id,
            prior_assignee_account_id: observed.observation.assignee_account_id.as_ref(),
            milestone: &plan.milestone,
            destination: plan.destination(),
            ownership_action: plan
                .assignment
                .as_ref()
                .map_or(OwnershipAction::Preserve, |assignment| assignment.action),
            field_writes: self.field_writes,
            live_routes: observed
                .live_transitions
                .iter()
                .map(|transition| RequestedTransition {
                    transition_id: transition.transition_id.clone(),
                    to_status_id: transition.to.status_id.clone(),
                })
                .collect(),
        };
        Ok(CanonicalDocument::from_serializable(&intent)?)
    }

    fn build_write_request(
        &self,
        operation: &'static str,
        observed: &ObservedIssue,
        plan: &TransitionPlan,
        authority: Option<ApplyAuthority>,
    ) -> Result<JiraRequest, AsmaError> {
        let ownership_action = issue_ownership_action(operation, observed, plan)?;
        let transition = plan
            .transition
            .as_ref()
            .map(|selected| prove_issue_live_route(operation, observed, plan, selected))
            .transpose()?;
        let intent = self.intent(observed, plan)?;
        Ok(JiraRequest {
            schema_version: WIRE_SCHEMA_VERSION,
            operation: if authority.is_some() {
                JiraOperation::Apply
            } else {
                JiraOperation::DryRun
            },
            issue_key: self.issue_key.clone(),
            idempotency_key: self.idempotency_key.clone(),
            intent_hash: Some(intent.hash().clone()),
            field_spec_hash: Some(self.field_spec.hash().clone()),
            workflow_spec_hash: Some(self.workflow_spec.hash().clone()),
            expected: Some(ExpectedObservation {
                status_id: observed.observation.status_id.clone(),
                assignee_account_id: observed.observation.assignee_account_id.clone(),
                update_token: observed.observation.update_token.clone(),
                observation_hash: Some(observed.observation.observation_hash.clone()),
            }),
            field_writes: self.field_writes.to_vec(),
            destination: Some(plan.destination().clone()),
            ownership_action,
            transition,
            authorized_apply: authority.is_some(),
        })
    }

    async fn exchange(
        &self,
        operation: &'static str,
        request: &JiraRequest,
    ) -> Result<JiraResponse, AsmaError> {
        let response = self.exchange.execute(operation, request).await?;
        ensure_wire_schema(operation, response.schema_version)?;
        if response.issue_key != request.issue_key
            || response.idempotency_key != request.idempotency_key
        {
            return Err(AsmaError::unavailable(
                operation,
                crate::UnavailableReason::MalformedResponse,
                "answered about a different issue or idempotency key",
            ));
        }
        if !request.authorized_apply && response.effective_operation == JiraOperation::Apply {
            return Err(AsmaError::refused(
                operation,
                "the boundary applied an unauthorized request",
            ));
        }
        Ok(response)
    }

    fn raise_reported_failure(
        &self,
        operation: &'static str,
        response: &JiraResponse,
    ) -> Result<(), AsmaError> {
        match response.outcome {
            JiraOutcome::Conflict => {
                let reason = response
                    .conflict
                    .as_ref()
                    .map_or("", |failure| failure.reason.as_str());
                Err(AsmaError::Conflict {
                    operation,
                    kind: StatusConflictKind::parse(reason)
                        .unwrap_or(StatusConflictKind::IncompatibleHumanMove),
                })
            }
            JiraOutcome::Unavailable => Err(AsmaError::unavailable(
                operation,
                crate::UnavailableReason::Transport,
                response
                    .unavailable
                    .as_ref()
                    .map_or("no reason given", |failure| failure.detail.as_str()),
            )),
            _ => Ok(()),
        }
    }

    fn interpret(
        &self,
        operation: &'static str,
        response: JiraResponse,
    ) -> Result<ObservedIssue, AsmaError> {
        self.raise_reported_failure(operation, &response)?;
        let observation = response.observation.clone().ok_or_else(|| {
            AsmaError::unavailable(
                operation,
                crate::UnavailableReason::MalformedResponse,
                "answered without an observation",
            )
        })?;
        let account_id = response
            .principal_account_id
            .clone()
            .ok_or(AsmaError::Conflict {
                operation,
                kind: StatusConflictKind::OwnershipUnresolved,
            })?;
        let live_transitions = response
            .live_transitions
            .iter()
            .map(WireTransition::to_core)
            .collect();
        Ok(ObservedIssue {
            response,
            observation,
            live_transitions,
            principal: TicketPrincipal { account_id },
        })
    }
}

fn issue_ownership_action(
    operation: &'static str,
    observed: &ObservedIssue,
    plan: &TransitionPlan,
) -> Result<OwnershipAction, AsmaError> {
    match plan.assignment.as_ref() {
        None => Ok(OwnershipAction::Preserve),
        Some(assignment) => {
            if assignment.action == OwnershipAction::Preserve {
                return Err(AsmaError::refused(
                    operation,
                    "a preserve action may not carry an assignee mutation",
                ));
            }
            if assignment.action == OwnershipAction::Unassign {
                return Err(AsmaError::refused(
                    operation,
                    "the asma boundary never clears an assignee; \
                     a workflow specification for it must use preserve",
                ));
            }
            if let Some(account_id) = assignment.assign_to.as_ref()
                && account_id != &observed.principal.account_id
            {
                return Err(AsmaError::refused(
                    operation,
                    "an assignee value may only be the authenticated principal's account id",
                ));
            }
            Ok(assignment.action)
        }
    }
}

fn prove_issue_live_route(
    operation: &'static str,
    observed: &ObservedIssue,
    plan: &TransitionPlan,
    selected: &SelectedTransition,
) -> Result<RequestedTransition, AsmaError> {
    let offered = observed
        .live_transitions
        .iter()
        .find(|live| live.transition_id == selected.transition_id)
        .ok_or_else(|| {
            AsmaError::refused(
                operation,
                "the selected transition was not offered by this observation",
            )
        })?;
    if &offered.to != plan.destination() {
        return Err(AsmaError::refused(
            operation,
            "the selected transition no longer reaches the planned destination",
        ));
    }
    Ok(RequestedTransition {
        transition_id: offered.transition_id.clone(),
        to_status_id: offered.to.status_id.clone(),
    })
}

fn issue_planned_holder(before: &ObservedIssue, plan: &TransitionPlan) -> Option<ExternalId> {
    match plan.assignment.as_ref() {
        Some(assignment) => assignment.assign_to.clone(),
        None => before.observation.assignee_account_id.clone(),
    }
}

/// Everything one ticket's convergence needs, in one place.
///
/// It borrows rather than owns: this adapter hands receipts back to its caller
/// and is deliberately not a repository.
#[derive(Clone, Copy)]
pub struct TicketDelegation<'a> {
    /// The one connector transport.
    pub exchange: &'a dyn JiraExchange,
    /// The pinned field specification.
    pub field_spec: &'a CompiledFieldSpec,
    /// The pinned workflow specification.
    pub workflow_spec: &'a CompiledWorkflowSpec,
    /// The projection revision to write.
    pub projection: &'a TicketSyncProjection,
    /// Kontor's own facts about the task.
    pub facts: &'a InternalTaskFacts,
    /// The ticket link.
    pub link_id: TicketLinkId,
    /// The caller's durable idempotency key.
    pub idempotency_key: &'a IdempotencyKey,
}

/// The transport seam below the pure Jira policy.
#[async_trait]
pub trait JiraExchange: Send + Sync {
    /// Execute one already-validated Jira request.
    async fn execute(
        &self,
        operation: &'static str,
        request: &JiraRequest,
    ) -> Result<JiraResponse, AsmaError>;
}

impl TicketDelegation<'_> {
    /// Read the live issue, its routes and the authenticated principal.
    ///
    /// # Errors
    /// Returns [`AsmaError::Unavailable`] when the boundary could not answer, and
    /// [`AsmaError::Conflict`] with [`StatusConflictKind::OwnershipUnresolved`]
    /// when it answered without a principal — reconciliation cannot decide
    /// "is the holder me?" without one, and guessing is how a ticket gets stolen.
    pub async fn observe(&self) -> Result<Observed, AsmaError> {
        self.read("jira observe", JiraOperation::Observe).await
    }

    /// Re-read the live issue, for confirmation or reconciliation.
    ///
    /// # Errors
    /// As [`TicketDelegation::observe`].
    pub async fn refetch(&self) -> Result<Observed, AsmaError> {
        self.read("jira refetch", JiraOperation::Refetch).await
    }

    async fn read(
        &self,
        operation: &'static str,
        wire_operation: JiraOperation,
    ) -> Result<Observed, AsmaError> {
        let request = JiraRequest {
            schema_version: WIRE_SCHEMA_VERSION,
            operation: wire_operation,
            issue_key: self.projection.external_issue_key.clone(),
            idempotency_key: self.idempotency_key.clone(),
            intent_hash: None,
            field_spec_hash: Some(self.field_spec.hash().clone()),
            workflow_spec_hash: Some(self.workflow_spec.hash().clone()),
            expected: None,
            field_writes: Vec::new(),
            destination: None,
            // A read converges nothing, so it asserts the most conservative
            // ownership action there is.
            ownership_action: OwnershipAction::Preserve,
            transition: None,
            authorized_apply: false,
        };
        let response = self.exchange(operation, &request).await?;
        self.interpret(operation, response)
    }

    /// Decide what to do, using only the pinned specification and this evidence.
    ///
    /// This is [`kontor_core::ticket::reconcile`] and nothing else: no ASMA
    /// branch, no status name, no remembered transition.
    #[must_use]
    pub fn plan(&self, observed: &Observed) -> ReconciliationOutcome {
        reconcile(&ReconciliationInput {
            spec: self.workflow_spec.spec(),
            observation: &observed.observation,
            // The observation was read in the same operation that produced this
            // plan, which is the only definition of fresh this adapter accepts.
            freshness: Freshness::Fresh,
            facts: self.facts,
            live_transitions: &observed.live_transitions,
            principal: &observed.principal,
        })
    }

    /// Validate the exact request an apply would send, without writing.
    ///
    /// # Errors
    /// As [`TicketDelegation::apply`], minus the confirmation requirement.
    pub async fn dry_run(
        &self,
        observed: &Observed,
        plan: &TransitionPlan,
    ) -> Result<JiraResponse, AsmaError> {
        let request = self.build_write_request("jira dry-run", observed, plan, None)?;
        self.exchange("jira dry-run", &request).await
    }

    /// Perform the planned effects and confirm them.
    ///
    /// # Errors
    /// * [`AsmaError::Refused`] when the plan contradicts itself or asks for
    ///   something this boundary cannot do — a preserve policy carrying an
    ///   assignee mutation, an [`OwnershipAction::Unassign`] (the boundary never
    ///   clears an assignee), an assignee value that is not the principal's, a
    ///   transition the observation did not offer, or a route that no longer
    ///   reaches the planned destination.
    /// * [`AsmaError::Unavailable`] when the boundary could not answer, or
    ///   answered `applied` without refetched confirmation — an acknowledgement is
    ///   not an effect.
    /// * [`AsmaError::Conflict`] when the boundary reports a typed conflict.
    pub async fn apply(
        &self,
        observed: &Observed,
        plan: &TransitionPlan,
        authority: ApplyAuthority,
    ) -> Result<JiraResponse, AsmaError> {
        let request = self.build_write_request("jira apply", observed, plan, Some(authority))?;
        let response = self.exchange("jira apply", &request).await?;
        if response.effective_operation != JiraOperation::Apply {
            return Err(AsmaError::unavailable(
                "jira apply",
                crate::UnavailableReason::MalformedResponse,
                format!(
                    "an authorized apply ran as {}",
                    response.effective_operation.as_str()
                ),
            ));
        }
        if response.outcome == JiraOutcome::Applied && response.confirmation.is_none() {
            return Err(AsmaError::unavailable(
                "jira apply",
                crate::UnavailableReason::MalformedResponse,
                "reported applied without a refetched observation",
            ));
        }
        self.raise_reported_failure("jira apply", &response)?;
        Ok(response)
    }

    /// Reconcile an apply whose result was never seen.
    ///
    /// Retrying a transition is forbidden until fresh evidence proves the first
    /// attempt had no effect. This is where that evidence is obtained; it never
    /// grants permission on the strength of a timeout alone.
    ///
    /// # Errors
    /// As [`TicketDelegation::refetch`].
    pub async fn reconcile_after_ambiguity(
        &self,
        before: &Observed,
        plan: &TransitionPlan,
    ) -> Result<AmbiguityVerdict, AsmaError> {
        let after = Box::new(self.refetch().await?);
        let now = &after.observation;
        let expected_holder = planned_holder(before, plan);

        // An assignee-only plan deliberately does *not* move the status: its
        // whole point is to converge the owner first and let the next
        // observation decide. Demanding the destination here would report a
        // fully successful assignment as contested state and invite a retry.
        // A staged hop is judged against the status it was going to, not the
        // milestone: demanding the milestone here would report a hop that landed
        // exactly as planned as contested state and invite a retry of a move Jira
        // has already made.
        let status_arrived = plan.transition.is_none() || &now.status == plan.destination();
        let holder_arrived = now.assignee_account_id == expected_holder;
        if status_arrived && holder_arrived {
            return Ok(AmbiguityVerdict::AlreadyConfirmed(after));
        }
        let unchanged = now.status == before.observation.status
            && now.assignee_account_id == before.observation.assignee_account_id;
        if unchanged {
            return Ok(AmbiguityVerdict::NoEffect(after));
        }
        // Anything else — the assignment landed but the transition did not, or a
        // human moved the ticket somewhere new — is partial or contested state.
        Ok(AmbiguityVerdict::Contradictory(after))
    }

    /// The canonical intent this delegation implements.
    ///
    /// Ids, timestamps and attempt counts are deliberately absent: they are
    /// evidence about one attempt, and including them would give the same logical
    /// plan a different digest on every retry, defeating replay detection.
    ///
    /// # Errors
    /// Returns [`AsmaError::Domain`] when the intent cannot be canonicalized.
    pub fn intent(
        &self,
        observed: &Observed,
        plan: &TransitionPlan,
    ) -> Result<CanonicalDocument, AsmaError> {
        let writes = compile_field_writes(self.projection, self.field_spec)?;
        let intent = DelegationIntent {
            schema_version: WIRE_SCHEMA_VERSION,
            connector: &self.workflow_spec.spec().connector,
            project: &self.workflow_spec.spec().project,
            issue_type: &self.workflow_spec.spec().issue_type,
            external_issue_key: &self.projection.external_issue_key,
            field_spec_version: self.field_spec.spec().version,
            field_spec_hash: self.field_spec.hash(),
            workflow_spec_version: self.workflow_spec.spec().version,
            workflow_spec_hash: self.workflow_spec.hash(),
            projection_revision: self.facts.projection_revision,
            prior_observation_hash: &observed.observation.payload_hash,
            prior_status_id: &observed.observation.status.status_id,
            prior_assignee_account_id: observed.observation.assignee_account_id.as_ref(),
            milestone: &plan.milestone,
            destination: plan.destination(),
            ownership_action: plan
                .assignment
                .as_ref()
                .map_or(OwnershipAction::Preserve, |assignment| assignment.action),
            field_writes: &writes,
            live_routes: observed
                .live_transitions
                .iter()
                .map(|transition| RequestedTransition {
                    transition_id: transition.transition_id.clone(),
                    to_status_id: transition.to.status_id.clone(),
                })
                .collect(),
        };
        Ok(CanonicalDocument::from_serializable(&intent)?)
    }

    /// Build the immutable record of one convergence attempt.
    ///
    /// # Errors
    /// Returns [`AsmaError::Domain`] when the receipt is internally inconsistent
    /// — in particular when it claims confirmation without a refetched
    /// observation.
    pub fn receipt(
        &self,
        observed: &Observed,
        plan: &TransitionPlan,
        response: &JiraResponse,
    ) -> Result<StatusTransitionReceipt, AsmaError> {
        let confirmation = response
            .confirmation
            .as_ref()
            .map(|confirmation| {
                confirmation
                    .observation
                    .to_core(self.link_id, confirmation.confirmed_at)
                    .map(|observation| (observation, confirmation.confirmed_at))
            })
            .transpose()?;
        let assignment_result = response.effects.assignment.as_ref().and_then(|assignment| {
            confirmation
                .as_ref()
                .map(|(observation, confirmed_at)| AssignmentResult {
                    // The confirmed holder, not the requested one: what the
                    // connector ended up with is the only fact worth recording.
                    assignee_account_id: observation
                        .assignee_account_id
                        .clone()
                        .or_else(|| assignment.account_id.clone()),
                    confirmed_at: confirmed_at.get(),
                })
        });
        let receipt = StatusTransitionReceipt {
            id: StatusTransitionReceiptId::generate(),
            link_id: self.link_id,
            task_id: self.facts.task_id,
            task_revision: self.facts.task_revision,
            workflow_revision: self.facts.workflow_revision,
            projection_revision: self.facts.projection_revision,
            spec_version: self.workflow_spec.spec().version,
            prior_observation_id: observed.observation.id,
            plan: plan.clone(),
            principal: observed.principal.clone(),
            assignment_result,
            idempotency_key: self.idempotency_key.clone(),
            dispatched_at: response.requested_at.get(),
            acknowledged_at: Some(response.completed_at.get()),
            confirmed_at: confirmation
                .as_ref()
                .map(|(_, confirmed_at)| confirmed_at.get()),
            refetched_observation_id: confirmation.as_ref().map(|(observation, _)| observation.id),
        };
        receipt.validate()?;
        Ok(receipt)
    }

    /// Assemble the one request shape a dry run and an apply share.
    ///
    /// They share it deliberately: a dry run that validated a *different*
    /// document from the one an apply sends would review the wrong thing.
    fn build_write_request(
        &self,
        operation: &'static str,
        observed: &Observed,
        plan: &TransitionPlan,
        authority: Option<ApplyAuthority>,
    ) -> Result<JiraRequest, AsmaError> {
        let ownership_action = match plan.assignment.as_ref() {
            None => OwnershipAction::Preserve,
            Some(assignment) => {
                if assignment.action == OwnershipAction::Preserve {
                    return Err(AsmaError::refused(
                        operation,
                        "a preserve action may not carry an assignee mutation",
                    ));
                }
                if assignment.action == OwnershipAction::Unassign {
                    // The domain models a clear; this boundary cannot perform
                    // one. Refusing here — rather than sending it and reading the
                    // rejection back — keeps the failure a policy answer instead
                    // of a transport error, and means a spec that asks for a
                    // clear is caught before anything is dispatched.
                    return Err(AsmaError::refused(
                        operation,
                        "the asma boundary never clears an assignee; \
                         a workflow specification for it must use preserve",
                    ));
                }
                if let Some(account_id) = assignment.assign_to.as_ref()
                    && account_id != &observed.principal.account_id
                {
                    return Err(AsmaError::refused(
                        operation,
                        "an assignee value may only be the authenticated principal's account id",
                    ));
                }
                assignment.action
            }
        };

        let transition = plan
            .transition
            .as_ref()
            .map(|selected| self.prove_live_route(operation, observed, plan, selected))
            .transpose()?;

        let intent = self.intent(observed, plan)?;
        Ok(JiraRequest {
            schema_version: WIRE_SCHEMA_VERSION,
            operation: if authority.is_some() {
                JiraOperation::Apply
            } else {
                JiraOperation::DryRun
            },
            issue_key: self.projection.external_issue_key.clone(),
            idempotency_key: self.idempotency_key.clone(),
            intent_hash: Some(intent.hash().clone()),
            field_spec_hash: Some(self.field_spec.hash().clone()),
            workflow_spec_hash: Some(self.workflow_spec.hash().clone()),
            expected: Some(ExpectedObservation {
                status_id: observed.observation.status.status_id.clone(),
                assignee_account_id: observed.observation.assignee_account_id.clone(),
                update_token: observed.observation.external_version.clone(),
                observation_hash: Some(observed.observation.payload_hash.clone()),
            }),
            field_writes: compile_field_writes(self.projection, self.field_spec)?,
            // The destination this request declares travels with the transition
            // below it, so it is *this attempt's* destination rather than the
            // milestone. A staged hop that declared the milestone here would hand
            // the connector a route to one status while naming another — the
            // internally inconsistent request that turns a hop into a
            // false-success receipt.
            destination: Some(plan.destination().clone()),
            ownership_action,
            transition,
            authorized_apply: authority.is_some(),
        })
    }

    /// Prove the selected route came from this observation and still leads home.
    fn prove_live_route(
        &self,
        operation: &'static str,
        observed: &Observed,
        plan: &TransitionPlan,
        selected: &SelectedTransition,
    ) -> Result<RequestedTransition, AsmaError> {
        let offered = observed
            .live_transitions
            .iter()
            .find(|live| live.transition_id == selected.transition_id)
            .ok_or_else(|| {
                AsmaError::refused(
                    operation,
                    "the selected transition was not offered by this observation",
                )
            })?;
        if &offered.to != plan.destination() {
            return Err(AsmaError::refused(
                operation,
                "the selected transition no longer reaches the planned destination",
            ));
        }
        Ok(RequestedTransition {
            transition_id: offered.transition_id.clone(),
            to_status_id: offered.to.status_id.clone(),
        })
    }

    async fn exchange(
        &self,
        operation: &'static str,
        request: &JiraRequest,
    ) -> Result<JiraResponse, AsmaError> {
        let response = self.exchange.execute(operation, request).await?;
        ensure_wire_schema(operation, response.schema_version)?;
        if response.issue_key != request.issue_key
            || response.idempotency_key != request.idempotency_key
        {
            return Err(AsmaError::unavailable(
                operation,
                crate::UnavailableReason::MalformedResponse,
                "answered about a different issue or idempotency key",
            ));
        }
        if !request.authorized_apply && response.effective_operation == JiraOperation::Apply {
            return Err(AsmaError::refused(
                operation,
                "the boundary applied an unauthorized request",
            ));
        }
        Ok(response)
    }

    /// Turn a reported conflict or unavailable reason into a typed error.
    fn raise_reported_failure(
        &self,
        operation: &'static str,
        response: &JiraResponse,
    ) -> Result<(), AsmaError> {
        match response.outcome {
            JiraOutcome::Conflict => {
                let reason = response
                    .conflict
                    .as_ref()
                    .map_or("", |failure| failure.reason.as_str());
                Err(AsmaError::Conflict {
                    operation,
                    // An unrecognized reason is still a conflict; it is simply not
                    // one this build has a name for.
                    kind: StatusConflictKind::parse(reason)
                        .unwrap_or(StatusConflictKind::IncompatibleHumanMove),
                })
            }
            JiraOutcome::Unavailable => Err(AsmaError::unavailable(
                operation,
                crate::UnavailableReason::Transport,
                response
                    .unavailable
                    .as_ref()
                    .map_or("no reason given", |failure| failure.detail.as_str()),
            )),
            _ => Ok(()),
        }
    }

    /// Turn a read response into domain values, refusing an unusable answer.
    fn interpret(
        &self,
        operation: &'static str,
        response: JiraResponse,
    ) -> Result<Observed, AsmaError> {
        self.raise_reported_failure(operation, &response)?;
        let wire = response.observation.as_ref().ok_or_else(|| {
            AsmaError::unavailable(
                operation,
                crate::UnavailableReason::MalformedResponse,
                "answered without an observation",
            )
        })?;
        let observation = wire.to_core(self.link_id, response.completed_at)?;
        let account_id = response.principal_account_id.clone().ok_or({
            AsmaError::Conflict {
                operation,
                kind: StatusConflictKind::OwnershipUnresolved,
            }
        })?;
        let live_transitions = response
            .live_transitions
            .iter()
            .map(WireTransition::to_core)
            .collect();
        Ok(Observed {
            observation,
            live_transitions,
            principal: TicketPrincipal { account_id },
            response,
        })
    }
}

/// Who should hold the ticket once `plan` has landed.
///
/// A plan with no assignment leaves the holder exactly as it was — which is what
/// makes `preserve` observable in the reconciliation of an ambiguous apply.
fn planned_holder(before: &Observed, plan: &TransitionPlan) -> Option<ExternalId> {
    match plan.assignment.as_ref() {
        Some(assignment) => assignment.assign_to.clone(),
        None => before.observation.assignee_account_id.clone(),
    }
}

/// The deterministic part of one delegation, as it is hashed into a receipt.
#[derive(Debug, Serialize)]
struct DelegationIntent<'a> {
    schema_version: SchemaVersion,
    connector: &'a ConnectorKey,
    project: &'a ExternalProjectKey,
    issue_type: &'a ExternalIssueTypeKey,
    external_issue_key: &'a ExternalId,
    field_spec_version: SpecVersion,
    field_spec_hash: &'a ContentHash,
    workflow_spec_version: SpecVersion,
    workflow_spec_hash: &'a ContentHash,
    projection_revision: AggregateRevision,
    prior_observation_hash: &'a ContentHash,
    prior_status_id: &'a ExternalId,
    prior_assignee_account_id: Option<&'a ExternalId>,
    milestone: &'a SemanticMilestoneKey,
    destination: &'a StatusSelector,
    ownership_action: OwnershipAction,
    field_writes: &'a [FieldWrite],
    live_routes: Vec<RequestedTransition>,
}
