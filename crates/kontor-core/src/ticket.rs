//! External-ticket domain: projection, workflow mapping and reconciliation.
//!
//! Three rules are structural here, not conventional:
//!
//! 1. **The projection is a closed set of fields.** [`TicketFieldKey`] is an
//!    enum; an unknown key cannot be constructed, and an absent field means
//!    *no write* — never "clear the field".
//! 2. **Comments are inbound only.** [`CommentPolicy`] has one variant and there
//!    is no outbound comment payload type, column or API. Adding outbound
//!    comments is a type change, not a configuration change.
//! 3. **Status and assignee are never chosen by prose.** A target status is
//!    selected by *data* ([`StatusSelector`]) and matched against the
//!    destination of a live transition; an assignee can only come from an
//!    external account id ([`AssigneeIdentitySource`] has exactly one variant),
//!    so a model, an email, a display name or a team name cannot be one.
//!
//! No external status name or id appears anywhere in this module. Two projects
//! with entirely different status vocabularies use the same code path.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::id::{
    AggregateRevision, BoundedText, CanonicalDocument, ConnectorKey, ContentHash, ExternalId,
    ExternalIssueTypeKey, ExternalName, ExternalProjectKey, GateKey, IdempotencyKey, PhaseKey,
    SchemaVersion, SemanticMilestoneKey, SpecVersion, StatusConflictId, TaskId, TicketLinkId,
    TicketObservationId, TicketProjectionId, Timestamp, WorkProfileKey,
};
use crate::state::{Freshness, GateState, TaskState, TerminalOutcome};
use crate::{DomainError, DomainResult};

closed_enum! {
    /// The closed set of ticket fields Kontor is able to project.
    ///
    /// It is an enum precisely so that a connector cannot invent a field and a
    /// model cannot name one.
    TicketFieldKey, "TicketFieldKey" {
        /// Short title.
        Summary => "summary",
        /// Long description.
        Description => "description",
        /// Acceptance criteria.
        AcceptanceCriteria => "acceptance_criteria",
        /// Product classification.
        Product => "product",
        /// Service or component name.
        ServiceName => "service_name",
        /// Reproduction steps.
        ReproSteps => "repro_steps",
        /// Severity classification.
        Severity => "severity",
        /// Kontor's own agent status field.
        AgentStatus => "agent_status",
    }
}

closed_enum! {
    /// Who owns a field's value.
    FieldOwner, "FieldOwner" {
        /// Kontor owns it and writes it outward.
        Kontor => "kontor",
        /// The external system owns it and Kontor reads it.
        Jira => "jira",
        /// Kontor mirrors a value it does not own.
        MirrorOnly => "mirror_only",
        /// Kontor-internal; never leaves Kontor and has no external mapping.
        Private => "private",
    }
}

closed_enum! {
    /// Which way a field's value flows.
    FieldDirection, "FieldDirection" {
        /// Kontor writes it to the external system.
        Outbound => "outbound",
        /// Kontor reads it from the external system.
        Inbound => "inbound",
        /// Both, with the owner deciding conflicts.
        Bidirectional => "bidirectional",
    }
}

impl FieldOwner {
    /// Whether this owner may be combined with `direction`.
    ///
    /// | owner | allowed |
    /// | --- | --- |
    /// | `kontor` | `outbound`, `bidirectional` |
    /// | `jira` | `inbound`, `bidirectional` |
    /// | `mirror_only` | `outbound` |
    /// | `private` | none — a private field has no direction and no mapping |
    #[must_use]
    pub const fn allows(self, direction: FieldDirection) -> bool {
        matches!(
            (self, direction),
            (
                Self::Kontor,
                FieldDirection::Outbound | FieldDirection::Bidirectional
            ) | (
                Self::Jira,
                FieldDirection::Inbound | FieldDirection::Bidirectional
            ) | (Self::MirrorOnly, FieldDirection::Outbound)
        )
    }
}

/// The external representation of one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalFieldType {
    /// Single-line text.
    Text,
    /// Multi-line rich text.
    RichText,
    /// One option from a fixed list.
    SingleSelect,
    /// Several options from a fixed list.
    MultiSelect,
    /// Integer number.
    Number,
    /// Calendar date.
    Date,
    /// A user reference.
    User,
    /// Free labels.
    Labels,
}

impl ExternalFieldType {
    /// Whether the type requires a non-empty option list.
    #[must_use]
    pub const fn requires_options(self) -> bool {
        matches!(self, Self::SingleSelect | Self::MultiSelect)
    }
}

/// How a field's text is encoded in the external system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldEncoding {
    /// Plain UTF-8 text.
    PlainText,
    /// The connector's structured document format.
    StructuredDocument,
    /// Markdown source.
    Markdown,
}

/// One selectable option of an external field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalFieldOption {
    /// The option's opaque external id.
    pub id: ExternalId,
    /// The option's display name.
    pub name: ExternalName,
}

/// How one closed field key maps to the external system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalFieldMapping {
    /// The external field id.
    pub field_id: ExternalId,
    /// Its external type.
    pub field_type: ExternalFieldType,
    /// Its text encoding.
    pub encoding: FieldEncoding,
    /// Allowed options, for select types.
    pub options: Vec<ExternalFieldOption>,
}

/// One field's ownership, direction and external mapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketFieldMapping {
    /// Which closed field this describes.
    pub key: TicketFieldKey,
    /// Who owns it.
    pub owner: FieldOwner,
    /// Which way it flows. `None` only for a private field.
    pub direction: Option<FieldDirection>,
    /// Its external representation. `None` only for a private field.
    pub external: Option<ExternalFieldMapping>,
    /// Whether the external system requires it.
    pub required: bool,
}

impl TicketFieldMapping {
    /// Validate one mapping.
    ///
    /// # Errors
    /// Rejects an owner/direction pair outside the matrix, a private field with
    /// an external mapping or a direction, a non-private field without one, a
    /// select type without options and duplicate option ids.
    pub fn validate(&self) -> DomainResult<()> {
        match self.owner {
            FieldOwner::Private => {
                if self.direction.is_some() || self.external.is_some() {
                    return Err(DomainError::invalid(
                        "TicketFieldMapping",
                        "a private field has no direction and no external mapping",
                    ));
                }
                if self.required {
                    return Err(DomainError::invalid(
                        "TicketFieldMapping",
                        "a private field cannot be externally required",
                    ));
                }
                return Ok(());
            }
            _ => {
                let direction = self.direction.ok_or(DomainError::Invalid {
                    subject: "TicketFieldMapping",
                    rule: "a non-private field must declare a direction",
                })?;
                if !self.owner.allows(direction) {
                    return Err(DomainError::invalid(
                        "TicketFieldMapping",
                        "owner and direction are not a permitted combination",
                    ));
                }
            }
        }
        let external = self.external.as_ref().ok_or(DomainError::Invalid {
            subject: "TicketFieldMapping",
            rule: "a non-private field must declare an external mapping",
        })?;
        if external.field_type.requires_options() {
            if external.options.is_empty() {
                return Err(DomainError::invalid(
                    "TicketFieldMapping",
                    "a select field must declare its options",
                ));
            }
            let unique: BTreeSet<&ExternalId> = external.options.iter().map(|o| &o.id).collect();
            if unique.len() != external.options.len() {
                return Err(DomainError::invalid(
                    "TicketFieldMapping",
                    "a select field lists a duplicate option id",
                ));
            }
        } else if !external.options.is_empty() {
            return Err(DomainError::invalid(
                "TicketFieldMapping",
                "only a select field may declare options",
            ));
        }
        Ok(())
    }
}

/// A versioned, immutable field-mapping specification for one
/// connector/project/issue-type triple.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketFieldSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The connector implementation.
    pub connector: ConnectorKey,
    /// The external project.
    pub project: ExternalProjectKey,
    /// The external issue type.
    pub issue_type: ExternalIssueTypeKey,
    /// This revision.
    pub version: SpecVersion,
    /// The mappings.
    pub mappings: Vec<TicketFieldMapping>,
}

impl TicketFieldSpec {
    /// Validate the whole specification atomically.
    ///
    /// # Errors
    /// Rejects duplicate keys, an empty mapping set and any invalid mapping. A
    /// specification is never partially accepted.
    pub fn validate(&self) -> DomainResult<()> {
        if self.mappings.is_empty() {
            return Err(DomainError::invalid(
                "TicketFieldSpec",
                "must declare at least one field mapping",
            ));
        }
        let mut seen = BTreeSet::new();
        for mapping in &self.mappings {
            if !seen.insert(mapping.key) {
                return Err(DomainError::invalid(
                    "TicketFieldSpec",
                    "declares a duplicate field key",
                ));
            }
            mapping.validate()?;
        }
        Ok(())
    }

    /// Look up one field's mapping.
    #[must_use]
    pub fn mapping(&self, key: TicketFieldKey) -> Option<&TicketFieldMapping> {
        self.mappings.iter().find(|m| m.key == key)
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`TicketFieldSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }
}

/// A typed field value in a projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldValue {
    /// Text content.
    Text {
        /// The bounded, line-ending-normalized body.
        body: BoundedText,
    },
    /// One selected option.
    Select {
        /// The option's external id.
        option: ExternalId,
    },
    /// Several selected options.
    MultiSelect {
        /// The options' external ids.
        options: Vec<ExternalId>,
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

/// One field of a projection revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectedField {
    /// Which closed field.
    pub key: TicketFieldKey,
    /// Its value. `None` means *absent*, which means *do not write* — never
    /// "clear the external field".
    pub value: Option<FieldValue>,
}

closed_enum! {
    /// What Kontor may do with external comments.
    ///
    /// Schema v1 has exactly one policy. There is no outbound comment payload
    /// type anywhere in this crate, so an outbound comment is unrepresentable
    /// rather than merely disabled.
    CommentPolicy, "CommentPolicy" {
        /// Read external comments; never write one.
        InboundOnly => "inbound_only",
    }
}

/// One immutable revision of the projection Kontor would write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketSyncProjection {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// This projection revision's id.
    pub id: TicketProjectionId,
    /// The ticket link it belongs to.
    pub link_id: TicketLinkId,
    /// The link's aggregate revision when this projection was computed.
    pub link_revision: AggregateRevision,
    /// The connector implementation.
    pub connector: ConnectorKey,
    /// The external project of the ticket-field specification this projection
    /// was computed against.
    pub field_spec_project: ExternalProjectKey,
    /// The external issue type of that specification.
    pub field_spec_issue_type: ExternalIssueTypeKey,
    /// The pinned revision of that specification.
    ///
    /// Stored so a persisted projection can be re-checked against the exact
    /// mapping that produced it rather than against whatever is current.
    pub field_spec_version: SpecVersion,
    /// The external issue key.
    pub external_issue_key: ExternalId,
    /// Ordered typed fields.
    pub fields: Vec<ProjectedField>,
    /// The comment policy in force.
    pub comment_policy: CommentPolicy,
    /// Cursor of the newest external comment already mirrored.
    pub external_comment_cursor: Option<ExternalId>,
    /// When this revision was computed.
    pub computed_at: Timestamp,
}

impl TicketSyncProjection {
    /// Validate the projection against its pinned field specification.
    ///
    /// # Errors
    /// Rejects duplicate fields, a field the specification does not map, a value
    /// whose type contradicts the mapping, a select value outside the declared
    /// options, and any attempt to write a field the specification says Kontor
    /// does not own.
    pub fn validate(&self, spec: &TicketFieldSpec) -> DomainResult<()> {
        let mut seen = BTreeSet::new();
        for field in &self.fields {
            if !seen.insert(field.key) {
                return Err(DomainError::invalid(
                    "TicketSyncProjection",
                    "projects the same field twice",
                ));
            }
            let mapping = spec.mapping(field.key).ok_or(DomainError::Invalid {
                subject: "TicketSyncProjection",
                rule: "projects a field the pinned specification does not map",
            })?;
            let Some(value) = &field.value else {
                // Absent is always legal: it means "do not write".
                continue;
            };
            if mapping.owner == FieldOwner::Private {
                return Err(DomainError::invalid(
                    "TicketSyncProjection",
                    "projects a private field outward",
                ));
            }
            let direction = mapping.direction.ok_or(DomainError::Invalid {
                subject: "TicketSyncProjection",
                rule: "projects a field with no direction",
            })?;
            if direction == FieldDirection::Inbound {
                return Err(DomainError::invalid(
                    "TicketSyncProjection",
                    "projects an inbound-only field outward",
                ));
            }
            let external = mapping.external.as_ref().ok_or(DomainError::Invalid {
                subject: "TicketSyncProjection",
                rule: "projects a field with no external mapping",
            })?;
            validate_field_value(value, external)?;
        }
        for mapping in &spec.mappings {
            if mapping.required
                && !self
                    .fields
                    .iter()
                    .any(|f| f.key == mapping.key && f.value.is_some())
            {
                return Err(DomainError::invalid(
                    "TicketSyncProjection",
                    "omits a field the external system requires",
                ));
            }
        }
        Ok(())
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`TicketSyncProjection::validate`], plus canonicalization failures.
    pub fn canonicalize(&self, spec: &TicketFieldSpec) -> DomainResult<CanonicalDocument> {
        self.validate(spec)?;
        CanonicalDocument::from_serializable(self)
    }
}

fn validate_field_value(value: &FieldValue, external: &ExternalFieldMapping) -> DomainResult<()> {
    let compatible = matches!(
        (value, external.field_type),
        (
            FieldValue::Text { .. },
            ExternalFieldType::Text | ExternalFieldType::RichText
        ) | (FieldValue::Select { .. }, ExternalFieldType::SingleSelect)
            | (
                FieldValue::MultiSelect { .. },
                ExternalFieldType::MultiSelect
            )
            | (FieldValue::Number { .. }, ExternalFieldType::Number)
            | (FieldValue::Date { .. }, ExternalFieldType::Date)
            | (FieldValue::Labels { .. }, ExternalFieldType::Labels)
    );
    if !compatible {
        return Err(DomainError::invalid(
            "TicketSyncProjection",
            "field value type contradicts the pinned mapping",
        ));
    }
    let allowed: BTreeSet<&ExternalId> = external.options.iter().map(|o| &o.id).collect();
    match value {
        FieldValue::Select { option } if !allowed.contains(option) => Err(DomainError::invalid(
            "TicketSyncProjection",
            "selects an option the pinned mapping does not declare",
        )),
        FieldValue::MultiSelect { options } if !options.iter().all(|o| allowed.contains(o)) => {
            Err(DomainError::invalid(
                "TicketSyncProjection",
                "selects an option the pinned mapping does not declare",
            ))
        }
        _ => Ok(()),
    }
}

/// One append-only revision of an external comment.
///
/// Identity is `(link, external comment id, body hash)`: the same comment seen
/// twice deduplicates, while an edit keeps both revisions and their provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalCommentRevision {
    /// The ticket link.
    pub link_id: TicketLinkId,
    /// The comment's external id.
    pub external_comment_id: ExternalId,
    /// The author's external account id.
    pub author_account_id: ExternalId,
    /// The author's display name, as the external system rendered it.
    pub author_display: Option<ExternalName>,
    /// When the external system created it.
    pub external_created_at: Timestamp,
    /// When the external system last updated it.
    pub external_updated_at: Timestamp,
    /// The normalized body.
    pub body: BoundedText,
    /// Digest of the normalized body.
    pub body_hash: ContentHash,
    /// When Kontor observed this revision.
    pub observed_at: Timestamp,
    /// The digest of the revision this one supersedes, for edits.
    pub supersedes: Option<ContentHash>,
}

impl ExternalCommentRevision {
    /// Whether `other` is the same stored revision as this one.
    #[must_use]
    pub fn is_same_revision(&self, other: &Self) -> bool {
        self.link_id == other.link_id
            && self.external_comment_id == other.external_comment_id
            && self.body_hash == other.body_hash
    }

    /// Verify that the recorded digest matches the recorded body.
    ///
    /// # Errors
    /// Returns [`DomainError`] when provenance was lost or rewritten.
    pub fn verify(&self) -> DomainResult<()> {
        if ContentHash::of(self.body.as_str().as_bytes()) != self.body_hash {
            return Err(DomainError::invalid(
                "ExternalCommentRevision",
                "body digest does not match the stored body",
            ));
        }
        Ok(())
    }
}

closed_enum! {
    /// The semantic class of an external status.
    ///
    /// Classes are how Kontor reasons; the status names and ids that map to them
    /// are pure data supplied per project.
    SemanticStatusClass, "SemanticStatusClass" {
        /// Work is progressing.
        Active => "active",
        /// Work is paused externally.
        Hold => "hold",
        /// Closed as successful.
        TerminalSuccess => "terminal_success",
        /// Closed as cancelled.
        TerminalCancelled => "terminal_cancelled",
        /// Closed as rejected.
        TerminalRejected => "terminal_rejected",
    }
}

impl SemanticStatusClass {
    /// Whether the class closes the external ticket.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::TerminalSuccess | Self::TerminalCancelled | Self::TerminalRejected
        )
    }
}

/// A data-valued reference to one external status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusSelector {
    /// The status's opaque external id.
    pub status_id: ExternalId,
    /// Its display name, kept for evidence only. Never matched on.
    pub status_name: ExternalName,
}

/// One external status and the semantic class it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalStatusClass {
    /// The status.
    pub selector: StatusSelector,
    /// Its semantic class.
    pub class: SemanticStatusClass,
}

/// A typed predicate over Kontor's own state.
///
/// Predicates are the only way a milestone can be chosen. They read Kontor's
/// dimensions directly, so no external status name can influence the decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InternalPredicate {
    /// The task is in exactly this state.
    TaskStateIs {
        /// Required task state.
        state: TaskState,
    },
    /// The workflow has completed this phase.
    PhaseCompleted {
        /// Required phase.
        phase: PhaseKey,
    },
    /// This gate is in exactly this state.
    GateStateIs {
        /// The gate.
        gate: GateKey,
        /// Required gate state.
        state: GateState,
    },
    /// Every gate the profile requires has passed or been waived.
    AllRequiredGatesPassed,
    /// The task's run closed with this outcome.
    RunTerminal {
        /// Required run outcome.
        outcome: TerminalOutcome,
    },
    /// Every nested predicate holds.
    All {
        /// Nested predicates.
        of: Vec<InternalPredicate>,
    },
    /// At least one nested predicate holds.
    Any {
        /// Nested predicates.
        of: Vec<InternalPredicate>,
    },
}

/// Maximum nesting depth of an [`InternalPredicate`].
pub const MAX_PREDICATE_DEPTH: usize = 8;

impl InternalPredicate {
    /// Evaluate against Kontor's facts.
    #[must_use]
    pub fn evaluate(&self, facts: &InternalTaskFacts) -> bool {
        self.evaluate_at(facts, 0)
    }

    fn evaluate_at(&self, facts: &InternalTaskFacts, depth: usize) -> bool {
        if depth > MAX_PREDICATE_DEPTH {
            return false;
        }
        match self {
            Self::TaskStateIs { state } => facts.task_state == *state,
            Self::PhaseCompleted { phase } => facts.completed_phases.contains(phase),
            Self::GateStateIs { gate, state } => facts
                .gate_states
                .iter()
                .any(|(key, value)| key == gate && value == state),
            Self::AllRequiredGatesPassed => facts.all_required_gates_passed,
            Self::RunTerminal { outcome } => facts.run_outcome == Some(*outcome),
            Self::All { of } => of.iter().all(|p| p.evaluate_at(facts, depth + 1)),
            Self::Any { of } => of.iter().any(|p| p.evaluate_at(facts, depth + 1)),
        }
    }

    /// Validate structural bounds.
    ///
    /// # Errors
    /// Rejects predicates nested deeper than [`MAX_PREDICATE_DEPTH`] and empty
    /// `all`/`any` groups, which would otherwise be silently true or false.
    pub fn validate(&self) -> DomainResult<()> {
        self.validate_at(0)
    }

    fn validate_at(&self, depth: usize) -> DomainResult<()> {
        if depth > MAX_PREDICATE_DEPTH {
            return Err(DomainError::invalid(
                "InternalPredicate",
                "nested deeper than the bound allows",
            ));
        }
        match self {
            Self::All { of } | Self::Any { of } => {
                if of.is_empty() {
                    return Err(DomainError::invalid(
                        "InternalPredicate",
                        "an `all` or `any` group must not be empty",
                    ));
                }
                for nested in of {
                    nested.validate_at(depth + 1)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// One semantic milestone and the external status it converges to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MilestoneRule {
    /// The internal milestone.
    pub milestone: SemanticMilestoneKey,
    /// When it applies.
    pub predicate: InternalPredicate,
    /// The external status it converges to.
    pub target: StatusSelector,
}

/// What Kontor does about the ticket's assignee at a terminal status.
///
/// Every variant is explicit; there is no "whatever the connector does" value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipAction {
    /// Leave the assignee exactly as it is. Never clears it.
    Preserve,
    /// Clear the assignee.
    Unassign,
    /// Set the assignee to the authenticated principal.
    ReassignToPrincipal,
}

/// The only source an assignee may come from.
///
/// The enum has one variant on purpose: a model, an email address, a display
/// name, an AgentsRoom team or a coding account is not representable here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssigneeIdentitySource {
    /// The authenticated principal's external account id.
    ExternalAccountId,
}

/// What to do when the external assignee is not the expected principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipMismatchBehavior {
    /// Record a conflict for a human.
    RaiseConflict,
    /// Accept the external value and continue.
    AcceptExternal,
}

/// How ownership of an external ticket is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnershipPolicy {
    /// From which milestone onward Kontor expects to hold the ticket.
    pub identity_source: AssigneeIdentitySource,
    /// What to do on mismatch.
    pub mismatch: OwnershipMismatchBehavior,
    /// What to do with the assignee once the ticket is terminal.
    pub terminal_action: OwnershipAction,
}

/// A versioned, immutable external workflow specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalWorkflowSpec {
    /// Schema generation of this document.
    pub schema_version: SchemaVersion,
    /// The connector implementation.
    pub connector: ConnectorKey,
    /// The external project.
    pub project: ExternalProjectKey,
    /// The external issue type.
    pub issue_type: ExternalIssueTypeKey,
    /// The work profile this mapping is written for, if it is profile-specific.
    ///
    /// Paired with [`Self::work_profile_version`]: both absent, or both present.
    pub work_profile: Option<WorkProfileKey>,
    /// The pinned revision of that work profile.
    ///
    /// A mapping written for "the delivery profile" without saying *which*
    /// revision would silently follow the profile as it changes underneath it.
    pub work_profile_version: Option<SpecVersion>,
    /// This revision.
    pub version: SpecVersion,
    /// Milestone rules, evaluated in order.
    pub milestones: Vec<MilestoneRule>,
    /// Every external status and its class.
    pub statuses: Vec<ExternalStatusClass>,
    /// Statuses Kontor accepts as a starting point without raising a conflict.
    pub inbound_compatible: Vec<StatusSelector>,
    /// The milestone at which Kontor takes ownership of the ticket.
    pub ownership_milestone: SemanticMilestoneKey,
    /// Ownership policy.
    pub ownership: OwnershipPolicy,
    /// Where an externally held ticket goes when work pauses.
    pub hold: Option<StatusSelector>,
    /// Where a reopened ticket goes.
    pub reopen: Option<StatusSelector>,
}

impl ExternalWorkflowSpec {
    /// Validate the specification.
    ///
    /// # Errors
    /// Rejects duplicate statuses or milestones, a milestone target that is not
    /// a declared status, an unknown hold/reopen selector, an invalid predicate
    /// and an ownership milestone with no rule.
    pub fn validate(&self) -> DomainResult<()> {
        // The profile pin is a pair. Naming a profile without its revision would
        // silently follow that profile as it changes underneath the mapping.
        if self.work_profile.is_some() != self.work_profile_version.is_some() {
            return Err(DomainError::invalid(
                "ExternalWorkflowSpec",
                "a work-profile pin needs both the profile and its revision",
            ));
        }
        if self.statuses.is_empty() {
            return Err(DomainError::invalid(
                "ExternalWorkflowSpec",
                "must declare at least one status",
            ));
        }
        let mut status_ids = BTreeSet::new();
        for status in &self.statuses {
            if !status_ids.insert(status.selector.status_id.clone()) {
                return Err(DomainError::invalid(
                    "ExternalWorkflowSpec",
                    "declares a duplicate external status",
                ));
            }
        }
        let mut milestones = BTreeSet::new();
        for rule in &self.milestones {
            if !milestones.insert(rule.milestone.clone()) {
                return Err(DomainError::invalid(
                    "ExternalWorkflowSpec",
                    "declares a duplicate milestone",
                ));
            }
            rule.predicate.validate()?;
            if !status_ids.contains(&rule.target.status_id) {
                return Err(DomainError::invalid(
                    "ExternalWorkflowSpec",
                    "a milestone targets an undeclared status",
                ));
            }
        }
        if !milestones.contains(&self.ownership_milestone) {
            return Err(DomainError::invalid(
                "ExternalWorkflowSpec",
                "the ownership milestone has no rule",
            ));
        }
        for selector in self
            .inbound_compatible
            .iter()
            .chain(self.hold.iter())
            .chain(self.reopen.iter())
        {
            if !status_ids.contains(&selector.status_id) {
                return Err(DomainError::invalid(
                    "ExternalWorkflowSpec",
                    "references an undeclared status",
                ));
            }
        }
        Ok(())
    }

    /// Validate, canonicalize and hash in one step.
    ///
    /// # Errors
    /// As [`ExternalWorkflowSpec::validate`], plus canonicalization failures.
    pub fn canonicalize(&self) -> DomainResult<CanonicalDocument> {
        self.validate()?;
        CanonicalDocument::from_serializable(self)
    }

    /// The semantic class of an external status id, if the specification
    /// declares it.
    #[must_use]
    pub fn class_of(&self, status_id: &ExternalId) -> Option<SemanticStatusClass> {
        self.statuses
            .iter()
            .find(|s| &s.selector.status_id == status_id)
            .map(|s| s.class)
    }
}

/// One append-only observation of an external ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalTicketObservation {
    /// This observation's id.
    pub id: TicketObservationId,
    /// The ticket link it belongs to.
    pub link_id: TicketLinkId,
    /// The observed status.
    pub status: StatusSelector,
    /// The status category as the external system reports it, for evidence.
    pub status_category: ExternalName,
    /// The observed issue type.
    pub issue_type: ExternalIssueTypeKey,
    /// The observed assignee's external account id, if any.
    pub assignee_account_id: Option<ExternalId>,
    /// The observed assignee's display name, for evidence only.
    pub assignee_display: Option<ExternalName>,
    /// The external system's own version/update token.
    pub external_version: Option<ExternalId>,
    /// When Kontor observed it.
    pub observed_at: Timestamp,
    /// Digest of the canonical observation payload.
    pub payload_hash: ContentHash,
}

/// Kontor's own facts about a task, as reconciliation reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalTaskFacts {
    /// The task.
    pub task_id: TaskId,
    /// Its lifecycle state.
    pub task_state: TaskState,
    /// Its aggregate revision.
    pub task_revision: AggregateRevision,
    /// The workflow's aggregate revision.
    pub workflow_revision: AggregateRevision,
    /// The projection's aggregate revision.
    pub projection_revision: AggregateRevision,
    /// Phases recorded complete.
    pub completed_phases: BTreeSet<PhaseKey>,
    /// Gate states.
    pub gate_states: Vec<(GateKey, GateState)>,
    /// Whether every required gate passed or was waived.
    pub all_required_gates_passed: bool,
    /// How the task's run closed, if it closed.
    pub run_outcome: Option<TerminalOutcome>,
}

/// One transition the external system currently offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveTransition {
    /// The transition's opaque external id.
    pub transition_id: ExternalId,
    /// Where it leads. Matching is on this destination, never on a remembered
    /// transition id, so workflow drift is detected instead of replayed.
    pub to: StatusSelector,
}

/// The authenticated principal Kontor acts as in the external system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketPrincipal {
    /// The principal's external account id. The only representable identity.
    pub account_id: ExternalId,
}

/// The assignment step of a convergence plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentPlan {
    /// Which account to assign. `None` is only produced for an explicit
    /// [`OwnershipAction::Unassign`], never for [`OwnershipAction::Preserve`].
    pub assign_to: Option<ExternalId>,
    /// Why the assignment is part of this plan.
    pub action: OwnershipAction,
}

/// The selected live transition of a convergence plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedTransition {
    /// The transition id the connector must invoke.
    pub transition_id: ExternalId,
    /// The destination status it was selected for.
    pub to: StatusSelector,
}

/// A plan to converge one external ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransitionPlan {
    /// The milestone being converged to.
    pub milestone: SemanticMilestoneKey,
    /// The target status.
    pub target: StatusSelector,
    /// The transition to invoke. `None` only for assignee-only convergence.
    pub transition: Option<SelectedTransition>,
    /// The assignment to apply, if any.
    pub assignment: Option<AssignmentPlan>,
    /// Whether the assignment must be confirmed before the transition may run.
    pub assignment_prerequisite: bool,
}

impl TransitionPlan {
    /// The status *this attempt* lands on.
    ///
    /// Equal to [`Self::target`] for a direct convergence. When the pinned
    /// specification had to route through its declared intermediate, this is that
    /// intermediate — so every check about the attempt (was the route still
    /// offered, did the status arrive) is stated against where the attempt was
    /// actually going, rather than against a milestone it was never going to
    /// reach in one hop.
    #[must_use]
    pub fn destination(&self) -> &StatusSelector {
        self.transition
            .as_ref()
            .map_or(&self.target, |transition| &transition.to)
    }

    /// Whether this attempt stops short of the milestone on purpose.
    ///
    /// A staged hop is progress, not convergence: the milestone is reached by the
    /// observation that follows it.
    #[must_use]
    pub fn is_staged_hop(&self) -> bool {
        self.transition
            .as_ref()
            .is_some_and(|transition| transition.to.status_id != self.target.status_id)
    }
}

closed_enum! {
    /// Why reconciliation could not produce a plan.
    StatusConflictKind, "StatusConflictKind" {
        /// The newest observation is too old to act on.
        StaleObservation => "stale_observation",
        /// No live transition leads to the target status.
        NoLiveTransition => "no_live_transition",
        /// Several live transitions lead to the target status.
        MultipleLiveTransitions => "multiple_live_transitions",
        /// A human moved the ticket somewhere Kontor cannot start from.
        IncompatibleHumanMove => "incompatible_human_move",
        /// The ticket is closed externally while Kontor has no closure evidence.
        ExternalTerminalBeforeInternalEvidence => "external_terminal_before_internal_evidence",
        /// The observed status is not declared by the pinned specification.
        UnknownStatusClass => "unknown_status_class",
        /// The target status is not declared by the pinned specification.
        UnknownTransitionPath => "unknown_transition_path",
        /// Kontor should hold the ticket but no assignee could be resolved.
        OwnershipUnresolved => "ownership_unresolved",
        /// Someone else holds the ticket.
        OwnershipMismatch => "ownership_mismatch",
        /// A terminal ticket's ownership changed while the policy preserves it.
        ///
        /// [`reconcile`] never produces this under
        /// [`OwnershipAction::Preserve`] — preserving an owner and reporting
        /// that owner as a violation are contradictory. The value stays in the
        /// closed set because a stricter terminal action recorded it before, and
        /// a persisted conflict must still be readable.
        TerminalOwnershipViolation => "terminal_ownership_violation",
    }
}

/// A recorded reconciliation conflict.
///
/// A conflict keeps the exact inputs that produced it. Resolving one appends
/// evidence; it never rewrites the observation or the revisions that caused it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusConflict {
    /// This conflict's id.
    pub id: StatusConflictId,
    /// The ticket link.
    pub link_id: TicketLinkId,
    /// Why it happened.
    pub kind: StatusConflictKind,
    /// The observation that caused it.
    pub observation_id: TicketObservationId,
    /// The task revision at that moment.
    pub task_revision: AggregateRevision,
    /// The workflow specification revision at that moment.
    pub spec_version: SpecVersion,
    /// The milestone that was being converged to, if one was chosen.
    pub milestone: Option<SemanticMilestoneKey>,
    /// When it was detected.
    pub detected_at: Timestamp,
}

/// The complete, pure result of reconciling one external ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationOutcome {
    /// Nothing to do.
    NoOp,
    /// Exactly one convergence plan.
    Transition(Box<TransitionPlan>),
    /// A typed conflict for a human.
    Conflict(StatusConflictKind),
}

/// Everything reconciliation is allowed to read.
#[derive(Debug, Clone)]
pub struct ReconciliationInput<'a> {
    /// The pinned workflow specification.
    pub spec: &'a ExternalWorkflowSpec,
    /// The newest observation.
    pub observation: &'a ExternalTicketObservation,
    /// How old that observation is.
    pub freshness: Freshness,
    /// Kontor's own facts.
    pub facts: &'a InternalTaskFacts,
    /// The transitions the external system currently offers.
    pub live_transitions: &'a [LiveTransition],
    /// The principal Kontor acts as.
    pub principal: &'a TicketPrincipal,
}

/// The one intermediate status Kontor may route through, and only when the
/// pinned specification already named it.
///
/// Deliberately **not** a path search. A shortest-path walk over whatever
/// transitions happen to be live would let the evaluator invent a route nobody
/// declared, and route a ticket through a status the workflow owner never
/// approved. The only status this will route through is the one the
/// specification declares as its reopen selector, and only when that status is
/// directly reachable from where the ticket is standing right now.
///
/// Every other shape is fail-closed — an absent selector, a selector that is not
/// a declared status, a selector that is not currently offered, or the ticket
/// already standing on it — and the caller raises the ordinary typed conflict.
///
/// # Errors
/// [`StatusConflictKind::MultipleLiveTransitions`] when several live transitions
/// reach the hop: which one runs would not be determined by the specification.
fn staged_hop<'live>(
    input: &'live ReconciliationInput<'_>,
    target: &StatusSelector,
) -> Result<Option<&'live LiveTransition>, StatusConflictKind> {
    let Some(hop) = input.spec.reopen.as_ref() else {
        return Ok(None);
    };
    // Standing on the hop already, or a hop that *is* the target, cannot make
    // progress — and re-planning it after the next observation is exactly how a
    // hop would become a loop between two statuses.
    if hop.status_id == input.observation.status.status_id || hop.status_id == target.status_id {
        return Ok(None);
    }
    // Routing through a status the pinned specification does not classify would
    // leave the next observation unable to say what the ticket now means.
    if input.spec.class_of(&hop.status_id).is_none() {
        return Ok(None);
    }
    let mut matching = input
        .live_transitions
        .iter()
        .filter(|live| live.to.status_id == hop.status_id);
    let Some(selected) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Err(StatusConflictKind::MultipleLiveTransitions);
    }
    Ok(Some(selected))
}

/// Reconcile Kontor's state with an external ticket.
///
/// The function is pure and total: it returns exactly one of
/// [`ReconciliationOutcome::NoOp`], one [`TransitionPlan`] or one typed
/// conflict. It never invents a status, never reuses a remembered transition id
/// and never clears an assignee under [`OwnershipAction::Preserve`].
#[must_use]
pub fn reconcile(input: &ReconciliationInput<'_>) -> ReconciliationOutcome {
    use ReconciliationOutcome::{Conflict, NoOp, Transition};

    if input.freshness != Freshness::Fresh {
        return Conflict(StatusConflictKind::StaleObservation);
    }

    let Some(current_class) = input.spec.class_of(&input.observation.status.status_id) else {
        return Conflict(StatusConflictKind::UnknownStatusClass);
    };

    // A ticket closed externally is checked against Kontor's *own* terminal
    // evidence before any no-op, preserve, hold or assignment branch can
    // short-circuit the decision. Getting this order wrong is precisely how an
    // externally closed ticket with unfinished gates becomes a silent `NoOp`.
    if current_class.is_terminal() && !internal_evidence_supports(current_class, input.facts) {
        return Conflict(StatusConflictKind::ExternalTerminalBeforeInternalEvidence);
    }

    if current_class.is_terminal()
        && input.spec.ownership.terminal_action == OwnershipAction::Preserve
    {
        // `preserve` preserves *every* holder, not only the principal. An
        // unassigned, self-held and other-held closed ticket all converge to
        // the same nothing: no assignment, no clear, and no conflict about who
        // holds a ticket the policy has already promised not to touch.
        return NoOp;
    }

    let Some(rule) = input
        .spec
        .milestones
        .iter()
        .find(|rule| rule.predicate.evaluate(input.facts))
    else {
        return NoOp;
    };

    if input.spec.class_of(&rule.target.status_id).is_none() {
        return Conflict(StatusConflictKind::UnknownTransitionPath);
    }

    let takes_ownership = rule.milestone == input.spec.ownership_milestone;
    let assignee_matches =
        input.observation.assignee_account_id.as_ref() == Some(&input.principal.account_id);

    if takes_ownership && !assignee_matches {
        if input.observation.assignee_account_id.is_some() {
            // Somebody else holds the ticket. Under `raise_conflict` that is a
            // conflict for a human; under `accept_external` the existing owner
            // is *preserved* and the status still converges — replanning the
            // assignee to the principal here would be a takeover, which is the
            // one thing `accept_external` says not to do.
            if input.spec.ownership.mismatch == OwnershipMismatchBehavior::RaiseConflict {
                return Conflict(StatusConflictKind::OwnershipMismatch);
            }
        } else {
            // Nobody holds it. Assignment is a prerequisite: converge the
            // assignee first and let the next observation decide whether the
            // status still needs to move. This is also the assignee-only
            // convergence path, so an already-applied status transition is
            // never retried.
            return Transition(Box::new(TransitionPlan {
                milestone: rule.milestone.clone(),
                target: rule.target.clone(),
                transition: None,
                assignment: Some(AssignmentPlan {
                    assign_to: Some(input.principal.account_id.clone()),
                    action: OwnershipAction::ReassignToPrincipal,
                }),
                assignment_prerequisite: true,
            }));
        }
    }

    if input.observation.status.status_id == rule.target.status_id {
        // Already where it should be, and ownership already agrees.
        return NoOp;
    }

    if !input
        .spec
        .inbound_compatible
        .iter()
        .any(|s| s.status_id == input.observation.status.status_id)
    {
        return Conflict(StatusConflictKind::IncompatibleHumanMove);
    }

    let mut matching = input
        .live_transitions
        .iter()
        .filter(|t| t.to.status_id == rule.target.status_id);
    let selected = if let Some(direct) = matching.next() {
        if matching.next().is_some() {
            return Conflict(StatusConflictKind::MultipleLiveTransitions);
        }
        direct
    } else {
        // The target is not reachable in one move. A real Jira workflow routinely
        // refuses `DRAFT -> In Development` while offering
        // `DRAFT -> Ready for Development`, and the honest answer is neither to
        // force the move nor to call an unconverged ticket converged.
        match staged_hop(input, &rule.target) {
            Ok(Some(hop)) => hop,
            Ok(None) => return Conflict(StatusConflictKind::NoLiveTransition),
            Err(kind) => return Conflict(kind),
        }
    };

    Transition(Box::new(TransitionPlan {
        milestone: rule.milestone.clone(),
        target: rule.target.clone(),
        transition: Some(SelectedTransition {
            transition_id: selected.transition_id.clone(),
            to: selected.to.clone(),
        }),
        assignment: None,
        assignment_prerequisite: false,
    }))
}

/// Whether Kontor's own state actually evidences an externally terminal status.
///
/// Success is the strict case: the run must have succeeded **and** every
/// required gate must have passed or been authorizedly waived. A ticket closed
/// as successful while a gate is still outstanding is a conflict, not agreement
/// — even though a run outcome exists. Cancellation and rejection each require
/// their own matching internal outcome; neither may masquerade as completed
/// work.
///
/// Each external terminal class maps to exactly *one* internal outcome. In
/// particular `abandoned` is not cancellation: an operator closing a run without
/// a runtime verdict has produced no evidence about what the external system
/// should say, so an externally cancelled ticket over an abandoned run is a
/// conflict for a human rather than agreement. `parked` likewise evidences
/// nothing external.
fn internal_evidence_supports(class: SemanticStatusClass, facts: &InternalTaskFacts) -> bool {
    match class {
        SemanticStatusClass::TerminalSuccess => {
            facts.run_outcome == Some(TerminalOutcome::Succeeded) && facts.all_required_gates_passed
        }
        SemanticStatusClass::TerminalCancelled => {
            facts.run_outcome == Some(TerminalOutcome::Cancelled)
        }
        SemanticStatusClass::TerminalRejected => facts.run_outcome == Some(TerminalOutcome::Failed),
        SemanticStatusClass::Active | SemanticStatusClass::Hold => true,
    }
}

/// The result of applying an assignment step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentResult {
    /// Who holds the ticket after the step.
    pub assignee_account_id: Option<ExternalId>,
    /// When the connector confirmed it.
    pub confirmed_at: Timestamp,
}

/// The immutable record of one convergence attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusTransitionReceipt {
    /// This receipt's id.
    pub id: crate::id::StatusTransitionReceiptId,
    /// The ticket link.
    pub link_id: TicketLinkId,
    /// The task.
    pub task_id: TaskId,
    /// Task revision at dispatch.
    pub task_revision: AggregateRevision,
    /// Workflow revision at dispatch.
    pub workflow_revision: AggregateRevision,
    /// Projection revision at dispatch.
    pub projection_revision: AggregateRevision,
    /// The pinned workflow specification revision.
    pub spec_version: SpecVersion,
    /// The observation the plan was computed from.
    pub prior_observation_id: TicketObservationId,
    /// The plan that was dispatched.
    pub plan: TransitionPlan,
    /// The principal that acted.
    pub principal: TicketPrincipal,
    /// The assignment result, if the plan had one.
    pub assignment_result: Option<AssignmentResult>,
    /// Idempotency key of the dispatch.
    pub idempotency_key: IdempotencyKey,
    /// When it was dispatched.
    pub dispatched_at: Timestamp,
    /// When the connector acknowledged it.
    pub acknowledged_at: Option<Timestamp>,
    /// When a refetch confirmed it.
    pub confirmed_at: Option<Timestamp>,
    /// The observation that confirmed it.
    pub refetched_observation_id: Option<TicketObservationId>,
}

impl StatusTransitionReceipt {
    /// Validate the receipt's internal consistency.
    ///
    /// # Errors
    /// Rejects a receipt with neither a transition nor an assignment, and one
    /// that claims confirmation without a refetched observation.
    pub fn validate(&self) -> DomainResult<()> {
        if self.plan.transition.is_none() && self.plan.assignment.is_none() {
            return Err(DomainError::invalid(
                "StatusTransitionReceipt",
                "a transition may be absent only for assignee-only convergence",
            ));
        }
        if self.confirmed_at.is_some() && self.refetched_observation_id.is_none() {
            return Err(DomainError::MissingEvidence {
                subject: "status transition",
                rule: "confirmation requires a refetched observation",
            });
        }
        Ok(())
    }
}
