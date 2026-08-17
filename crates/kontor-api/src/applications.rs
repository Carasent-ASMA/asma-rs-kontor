//! The public application operations an empty Realm is brought to life through.
//!
//! Everything in [`crate::control`] is either a read or a generic *intent*: it
//! records that somebody asked for something and answers with a receipt. That is
//! the right shape for a command whose effect belongs to a dispatcher, and the
//! wrong shape for bootstrapping — a caller that gets a receipt back does not know
//! whether the project exists, and cannot ask for the epic it was about to apply.
//!
//! The operations here are the other kind. A successful answer means the
//! application service *ran*, or idempotently replayed, and the body carries the
//! durable projection rather than a promise about one. Between them they are
//! sufficient: from a Realm whose database has nothing in it, one admin
//! credential can ensure a project, apply a whole epic, arm a bounded scope, ask
//! the scheduler what it would do, start what it admits, and drive the work to a
//! gated close — with no seed script, no direct SQL and no manual session
//! creation anywhere in the sequence.
//!
//! # Where the decisions live
//!
//! Not here. Every handler in this module does the same four things and nothing
//! else: check the caller's tier, read the `Idempotency-Key` and the path ids,
//! call exactly one method on [`ApplicationOperations`], and return what it
//! answered. The service behind that port is composed in `kontor-daemon` out of
//! the store, the profile pack, the team layer, the scheduler and the runtime
//! adapters — which is where those crates already live, and where the choice of
//! which ones a Realm has is already made.
//!
//! # What is deliberately absent
//!
//! There is no route that creates a native session, names a runtime endpoint,
//! carries a credential value, or writes a store row the domain did not decide
//! on. A seat exists because the scheduler admitted work and the runtime agreed
//! to fill it; `scheduler:start` is the only operation in this module that
//! reaches a runtime at all, and it reaches it through the same admission path a
//! background scheduler would.

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use kontor_core::id::{
    AccountProfileId, AdvisorRunId, AgentRunId, AggregateRevision, BoundedText, CommitteeRunId,
    ContentHash, ExternalId, ExternalName, IdempotencyKey, MiniProjectId, ProjectId,
    QuickSessionId, RoleCatalogId, RoleCode, RuntimeKindKey, SeatBindingId, SpecVersion, TaskId,
    Timestamp, TopologyKindKey, TopologyNodeId, TopologySpecId,
};
use kontor_core::spec::{
    CodeCategory, CodeLifecycle, RoleSegment, ShareabilityClass, ShareabilityClassifier,
    ShareabilityProvenance,
};
use kontor_core::state::{PlacementState, TopologyLifecycle};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::Caller;
use crate::auth::CallerCapability;
use crate::control::{idempotency_key, parse_id};
use crate::error::{ApiError, ApiErrorCode};
use crate::state::ApiState;

// ---------------------------------------------------------------------------
// Shared wire vocabulary
// ---------------------------------------------------------------------------

/// Whether an ensure/apply wrote the row or found it already matching.
///
/// It is on every item of every declarative answer, because "it worked" and "it
/// was already like that" are different facts and a caller reconciling a plan
/// needs to tell them apart without diffing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AppliedDto {
    /// This call wrote it.
    Created,
    /// It already existed and matched, so nothing was written.
    Unchanged,
}

/// One immutable specification revision, as a caller pins it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RevisionRefDto {
    /// The specification's stable id.
    pub id: String,
    /// The pinned revision.
    #[schema(value_type = u32)]
    pub version: SpecVersion,
}

/// What a caller supplies when a new seat selects a standard role.
///
/// It carries the catalog revision and the code, and deliberately nothing else.
/// A request that could also state the standard title would be a second source
/// for a fact the catalog already owns, and the two would disagree the first
/// time a title was corrected — so the title is resolved, never accepted.
///
/// The field list is closed. Without that, a caller sending `standard_title`
/// would have it quietly dropped by serde and would believe it had been
/// honoured — which is worse than a refusal, because it looks like agreement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RoleSelectionDto {
    /// The exact catalog revision the code is read from.
    pub catalog_revision: RevisionRefDto,
    /// The stable role code.
    #[schema(value_type = String)]
    pub role_code: RoleCode,
    /// A presentation-only label, when this seat is shown as something more
    /// specific than its standard title.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub custom_display_name: Option<ExternalName>,
}

/// One role as the server resolved it, on every projection and every receipt.
///
/// The extra fields over [`RoleSelectionDto`] are exactly the ones the daemon
/// looked up. A client renders these; it never derives them, and it never keeps
/// its own table of what a code means.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ResolvedRoleRefDto {
    /// The catalog revision this was resolved against.
    pub catalog_revision: RevisionRefDto,
    /// The stable role code.
    #[schema(value_type = String)]
    pub role_code: RoleCode,
    /// The catalog's standard title for that code.
    #[schema(value_type = String)]
    pub standard_title: ExternalName,
    /// The segment the catalog files it under.
    #[schema(value_type = String)]
    pub segment: RoleSegment,
    /// The presentation-only label, when one was selected.
    #[schema(value_type = Option<String>)]
    pub custom_display_name: Option<ExternalName>,
}

/// Server-owned help for one controlled code.
///
/// Keyed by `(category, code)`. Compatibility and retired codes stay present as
/// explicit entries: a client reading old state has to render them honestly, and
/// a projection that dropped them would force every client to keep the private
/// dictionary this projection exists to replace. A code with no entry is
/// rendered as unknown, because the server returned no definition for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CodeHelpEntryDto {
    /// The code itself.
    pub code: String,
    /// Its expanded name.
    #[schema(value_type = String)]
    pub full_name: ExternalName,
    /// One concise sentence saying what it means.
    #[schema(value_type = String)]
    pub meaning: BoundedText,
    /// The family it belongs to.
    #[schema(value_type = String)]
    pub category: CodeCategory,
    /// Whether new state may still use it.
    #[schema(value_type = String)]
    pub lifecycle: CodeLifecycle,
    /// The revision this definition was read from.
    pub source: RevisionRefDto,
}

/// The exact immutable topology specification a projection is pinned to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PinnedSpecDto {
    /// Specification identity.
    #[schema(value_type = String)]
    pub id: TopologySpecId,
    /// The published revision.
    #[schema(value_type = u32)]
    pub version: SpecVersion,
    /// Canonical hash of that exact document.
    #[schema(value_type = String)]
    pub canonical_hash: ContentHash,
}

/// The native shape the server derived for one node.
///
/// Derived, never supplied. It is what the daemon intends to materialize from
/// the pinned specification's declared capabilities — which is why a caller can
/// read it and cannot write it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct DesiredBindingDto {
    /// The runtime family this node's container must come from.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// The projection capabilities the adapter must support, in declared order.
    pub projection_capabilities: Vec<String>,
}

/// What a runtime actually reported for one node, at one instant.
///
/// Present only after an exact-id readback. Its absence is a fact — nothing has
/// been observed — and is never filled in from the desired shape, because a
/// desired value presented as an observation is how drift stops being visible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ObservedBindingDto {
    /// The runtime family that answered.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// The native container identity it reported.
    #[schema(value_type = String)]
    pub native_id: ExternalId,
    /// The native display name it reported.
    #[schema(value_type = Option<String>)]
    pub native_name: Option<ExternalId>,
    /// The working directory it reported.
    #[schema(value_type = Option<String>)]
    pub cwd: Option<ExternalId>,
    /// When the readback happened.
    #[schema(value_type = String, format = DateTime)]
    pub observed_at: Timestamp,
}

/// One seat a topology node hosts, as a projection reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TopologySeatDto {
    /// The exact binding identity, which is what an attention or retirement
    /// addresses. Naming a seat any other way would be a scan.
    #[schema(value_type = String)]
    pub seat_binding_id: SeatBindingId,
    /// The stable role-slot address within the node.
    pub role_slot_id: String,
    /// The role, as the server resolved it.
    pub role: ResolvedRoleRefDto,
    /// Its lifecycle.
    #[schema(value_type = String)]
    pub lifecycle: TopologyLifecycle,
}

/// One node of a topology projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TopologyNodeDto {
    /// Durable node identity. The only topology handle a caller may address
    /// back, and only for the operations that take one.
    #[schema(value_type = String)]
    pub topology_node_id: TopologyNodeId,
    /// Logical parent; absent only for the root.
    #[schema(value_type = Option<String>)]
    pub parent_topology_node_id: Option<TopologyNodeId>,
    /// The data-defined kind the pinned specification declares.
    #[schema(value_type = String)]
    pub kind_key: TopologyKindKey,
    /// Logical lifecycle.
    #[schema(value_type = String)]
    pub lifecycle: TopologyLifecycle,
    /// Derived native-placement condition.
    #[schema(value_type = String)]
    pub placement: PlacementState,
    /// The native shape the server derived.
    pub desired_binding: DesiredBindingDto,
    /// The native identity last read back, when anything has been.
    pub observed_binding: Option<ObservedBindingDto>,
    /// The seats this node hosts, in stable slot order.
    pub seats: Vec<TopologySeatDto>,
}

/// The scope a semantic topology operation acts on.
///
/// A closed tagged union of the semantic ids Kontor already owns. This is the
/// whole of what a model may say about *where* it wants topology: it names a
/// meaning, and the server derives the kind, the parent and the native shape
/// from the pinned specification. Adding a published kind to a specification
/// therefore needs no change here, and inventing a kind per call stays
/// impossible — which is the same rule stated as a type rather than as a
/// validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum SemanticTopologyTargetDto {
    /// The project's own root.
    ProjectRoot,
    /// One Quick session.
    QuickSession {
        /// The session.
        #[schema(value_type = String)]
        quick_session_id: QuickSessionId,
    },
    /// One epic.
    Epic {
        /// The epic.
        #[schema(value_type = String)]
        epic_id: MiniProjectId,
    },
    /// One epic's control plane.
    EpicControl {
        /// The epic whose control plane this is.
        #[schema(value_type = String)]
        epic_id: MiniProjectId,
    },
    /// One ticket.
    Ticket {
        /// The task the ticket is linked to.
        #[schema(value_type = String)]
        task_id: TaskId,
    },
    /// One Advisor consultation.
    AdvisorConsultation {
        /// The consultation.
        #[schema(value_type = String)]
        advisor_run_id: AdvisorRunId,
    },
    /// One Committee consultation.
    CommitteeConsultation {
        /// The consultation.
        #[schema(value_type = String)]
        committee_run_id: CommitteeRunId,
    },
}

/// What every new mutation answers with, whatever else it adds.
///
/// A caller gets the receipt, whether this call was the one that wrote,
/// the revision to present next, and the position the answer is consistent
/// with. `applied` is what makes a replay legible: the same key returns the
/// original receipt with `Unchanged`, so a retry is distinguishable from a
/// second effect without diffing anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct MutationReceiptDto {
    /// The Realm the effect happened in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The receipt this command was committed under.
    pub receipt_id: String,
    /// Whether this call wrote, or replayed one that already had.
    pub applied: AppliedDto,
    /// The affected aggregate's revision after the effect.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// The control-plane position the answer is consistent with.
    #[schema(value_type = i64)]
    pub snapshot_cursor: kontor_core::id::EventCursor,
}

/// How one durable record was classified for leaving Kontor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ShareabilityDto {
    /// Whether it may ever leave.
    #[schema(value_type = String)]
    pub class: ShareabilityClass,
    /// Who classified it.
    #[schema(value_type = String)]
    pub classifier: ShareabilityClassifier,
    /// Default rule versus a human's write-time override.
    #[schema(value_type = String)]
    pub provenance: ShareabilityProvenance,
}

// ---------------------------------------------------------------------------
// Topology specification, catalog and reference
// ---------------------------------------------------------------------------

/// What `topology-specs:draft` is asked for.
///
/// The vocabulary is data, so it arrives as the declared node kinds rather than
/// as a choice between server-known shapes. `base` names a revision to start
/// from; without one the draft is built from nothing.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
pub struct DraftTopologySpecRequest {
    /// The revision to start from, when this is an edit rather than a first draft.
    pub base: Option<RevisionRefDto>,
    /// Human name for the specification.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// The unique logical root kind.
    #[schema(value_type = String)]
    pub root_kind: TopologyKindKey,
    /// The data-defined node-kind vocabulary, in declaration order.
    #[schema(value_type = Vec<Object>)]
    pub node_kinds: Vec<serde_json::Value>,
    /// Codes this vocabulary explains but never declares as usable.
    #[serde(default)]
    #[schema(value_type = Vec<Object>)]
    pub historical_codes: Vec<serde_json::Value>,
}

/// One complete candidate document, built by the server and stored nowhere.
///
/// Draft is deliberately pure. There is no durable draft aggregate to put this
/// in, publication already revalidates the exact candidate it is given, and a
/// store added solely to remember editor scratch state would widen the authority
/// boundary without making anything safer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TopologySpecCandidateDto {
    /// The Realm that built it.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The complete candidate document.
    #[schema(value_type = Object)]
    pub candidate: serde_json::Value,
    /// The canonical hash of that exact candidate.
    #[schema(value_type = String)]
    pub candidate_hash: ContentHash,
}

/// What `topology-specs:validate` is asked for.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
pub struct ValidateTopologySpecRequest {
    /// One complete candidate document.
    #[schema(value_type = Object)]
    pub candidate: serde_json::Value,
}

/// The ordered verdict on one candidate.
///
/// Violations are ordered so two runs over the same candidate produce the same
/// list, which is what lets a client diff them. An empty list is the only thing
/// that makes the candidate publishable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TopologySpecValidationDto {
    /// The Realm that validated it.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// Every violation, in a stable order. Empty means publishable.
    pub violations: Vec<String>,
    /// The canonical hash of the exact candidate that was validated.
    #[schema(value_type = String)]
    pub validation_hash: ContentHash,
}

/// What `topology-specs:publish` is asked for.
///
/// It names the hash validation returned, so the server can prove it is
/// publishing the document that was judged rather than one edited after the
/// verdict — and it revalidates anyway, because a hash proves identity and not
/// currency.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
pub struct PublishTopologySpecRequest {
    /// The complete candidate to publish.
    #[schema(value_type = Object)]
    pub candidate: serde_json::Value,
    /// The hash the validation answered with.
    #[schema(value_type = String)]
    pub validation_hash: ContentHash,
    /// The project revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
}

/// One published, immutable specification revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PublishedTopologySpecDto {
    /// Its identity, revision and canonical hash.
    pub spec: PinnedSpecDto,
    /// How it was classified for leaving Kontor.
    pub shareability: ShareabilityDto,
    /// The receipt this publication was committed under.
    pub receipt: MutationReceiptDto,
}

/// One exact immutable specification document, as a caller reads it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TopologySpecDocumentDto {
    /// The Realm it belongs to.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// Its identity, revision and canonical hash.
    pub spec: PinnedSpecDto,
    /// The exact published document.
    #[schema(value_type = Object)]
    pub document: serde_json::Value,
    /// How it was classified.
    pub shareability: ShareabilityDto,
    /// The position this read is consistent with.
    #[schema(value_type = i64)]
    pub snapshot_cursor: kontor_core::id::EventCursor,
}

/// One resolved role from a catalog revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RoleCatalogEntryDto {
    /// The stable code.
    #[schema(value_type = String)]
    pub role_code: RoleCode,
    /// The standard human title.
    #[schema(value_type = String)]
    pub standard_title: ExternalName,
    /// Where the role may be selected.
    #[schema(value_type = String)]
    pub segment: RoleSegment,
    /// Its bounded responsibility summary.
    #[schema(value_type = String)]
    pub responsibility_summary: BoundedText,
    /// Whether new seats may still select it.
    #[schema(value_type = String)]
    pub lifecycle: CodeLifecycle,
    /// Default capabilities a deployment may narrow later.
    pub capability_defaults: Vec<String>,
}

/// One whole catalog revision, in its declared order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RoleCatalogDto {
    /// The Realm it was read in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The catalog identity and revision.
    #[schema(value_type = String)]
    pub catalog_id: RoleCatalogId,
    /// The revision read.
    #[schema(value_type = u32)]
    pub version: SpecVersion,
    /// Human name.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// Every role, sorted in the catalog's declared order rather than in any
    /// order this projection chose.
    pub roles: Vec<RoleCatalogEntryDto>,
    /// The position this read is consistent with.
    #[schema(value_type = i64)]
    pub snapshot_cursor: kontor_core::id::EventCursor,
}

// ---------------------------------------------------------------------------
// Semantic topology
// ---------------------------------------------------------------------------

/// How far an inspection reaches.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TopologyScopeQuery {
    /// Narrow to one epic's pinned subgraph. Absent means the whole project.
    pub epic_id: Option<String>,
}

/// One project's authoritative topology, as stored.
///
/// The nodes carry the derived native shape and, where anything has been read
/// back, the exact native identity observed. Both are evidence: their presence
/// in an answer does not make them legal in a request, which is what keeps the
/// model-facing boundary semantic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TopologyProjectionDto {
    /// The Realm it was read in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The project whose topology this is.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The exact immutable specification these nodes are pinned to.
    pub pinned_spec: PinnedSpecDto,
    /// Every node in the addressed scope, parents before children.
    pub nodes: Vec<TopologyNodeDto>,
    /// The position this read is consistent with.
    #[schema(value_type = i64)]
    pub snapshot_cursor: kontor_core::id::EventCursor,
}

/// What a semantic topology write is asked for.
///
/// A scope and the revision the caller read it at, and nothing else. There is
/// no field for a node kind, a parent, a native name, a native id or a working
/// directory — not because they are validated away, but because the type has
/// nowhere to put them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticTopologyRequest {
    /// The semantic scope to act on.
    pub target: SemanticTopologyTargetDto,
    /// The project revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
}

/// What addressing one already-returned node is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TopologyNodeRequest {
    /// The node's revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// Why the node is being retired or archived. Recorded, never interpreted.
    #[schema(value_type = String)]
    pub reason: ExternalName,
}

/// What one semantic topology write produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TopologyMutationDto {
    /// The topology as it stands after the effect.
    pub projection: TopologyProjectionDto,
    /// The receipt the effect was committed under.
    pub receipt: MutationReceiptDto,
}

/// What a pinned-specification upgrade is previewed against.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TopologyUpgradePreviewRequest {
    /// The published revision to diff the epic's current pin against.
    pub target_spec: RevisionRefDto,
}

/// One node-, seat- or native-level effect an upgrade would have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TopologyUpgradeEffectDto {
    /// What the effect is about: a node, a seat or a native container.
    pub subject: String,
    /// The node it concerns, when it concerns one.
    #[schema(value_type = Option<String>)]
    pub topology_node_id: Option<TopologyNodeId>,
    /// What would happen, in the server's own vocabulary.
    pub effect: String,
    /// One line a human can read.
    #[schema(value_type = String)]
    pub detail: BoundedText,
}

/// What an upgrade would do, computed and committed nowhere.
///
/// A preview is a read: it takes no idempotency key, and it hands back a hash
/// the apply must name. The apply revalidates anyway — a hash proves the caller
/// is applying the diff it was shown, not that the world still looks that way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TopologyUpgradePreviewDto {
    /// The Realm that computed it.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The epic whose pin would move.
    #[schema(value_type = String)]
    pub epic_id: MiniProjectId,
    /// The pin as it stands.
    pub current_spec: PinnedSpecDto,
    /// The pin it would move to.
    pub target_spec: PinnedSpecDto,
    /// Every effect, in a stable order.
    pub effects: Vec<TopologyUpgradeEffectDto>,
    /// The hash the corresponding apply must name.
    #[schema(value_type = String)]
    pub preview_hash: ContentHash,
    /// The position this preview was computed at.
    #[schema(value_type = i64)]
    pub snapshot_cursor: kontor_core::id::EventCursor,
}

/// What applying a named upgrade preview is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TopologyUpgradeApplyRequest {
    /// The hash the preview answered with.
    #[schema(value_type = String)]
    pub preview_hash: ContentHash,
    /// The epic revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
}

/// One applied upgrade: the new immutable pin and what the topology now is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AppliedTopologyUpgradeDto {
    /// The pin the epic now holds.
    pub pinned_spec: PinnedSpecDto,
    /// The topology as it stands after the upgrade.
    pub projection: TopologyProjectionDto,
    /// The receipt the upgrade was committed under.
    pub receipt: MutationReceiptDto,
}

/// Every controlled code one epic's pinned revisions define.
///
/// One combined projection rather than three, because a client rendering a
/// transcript has one code in hand and does not know which family it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CodeHelpProjectionDto {
    /// The Realm it was read in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The epic whose pins were read.
    #[schema(value_type = String)]
    pub epic_id: MiniProjectId,
    /// Every definition, sorted by `(category, code)`.
    pub entries: Vec<CodeHelpEntryDto>,
    /// The position this read is consistent with.
    #[schema(value_type = i64)]
    pub snapshot_cursor: kontor_core::id::EventCursor,
}

// ---------------------------------------------------------------------------
// Project bootstrap
// ---------------------------------------------------------------------------

/// What `projects:ensure` is asked for.
///
/// `root_path` is the stable caller key: it is unique across the database and
/// immutable once the project exists, so the same request always resolves to the
/// same project without a second identity that could disagree with it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct EnsureProjectRequest {
    /// Human name. Immutable once the project exists.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// Canonical absolute root path. The natural identity.
    #[schema(value_type = String)]
    pub root_path: ExternalName,
}

/// One project, as a bootstrap caller sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProjectDto {
    /// The Realm it belongs to.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The project.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// Its name.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// Its canonical root path.
    #[schema(value_type = String)]
    pub root_path: ExternalName,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// Whether this call created it.
    pub applied: AppliedDto,
    /// When it was created.
    #[schema(value_type = String, format = DateTime)]
    pub created_at: Timestamp,
}

// ---------------------------------------------------------------------------
// Catalogs
// ---------------------------------------------------------------------------

/// One selectable work profile, as the catalog advertises it.
///
/// The revision is what a caller pins in `epics:apply`; the team it prescribes is
/// reported alongside it so a Lead can see the closure it is choosing rather than
/// discovering it at apply time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct WorkProfileCatalogDto {
    /// The pack category this profile is advertised under.
    pub category: String,
    /// Human label.
    #[schema(value_type = String)]
    pub label: ExternalName,
    /// The profile revision the category resolves to.
    pub profile: RevisionRefDto,
    /// The team revision the profile pins, when it prescribes one.
    pub team: Option<RevisionRefDto>,
    /// The digest of the whole resolved bundle. What `epics:apply` freezes.
    pub bundle_hash: String,
}

/// One selectable team template revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TeamTemplateCatalogDto {
    /// The template revision.
    pub template: RevisionRefDto,
    /// Human name.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// The role slots it seats, in declaration order.
    pub slots: Vec<String>,
    /// The digest of its canonical definition.
    pub definition_hash: String,
}

/// The realm-qualified model catalog consumed by the Teams editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ModelCatalogDto {
    /// The Realm that performed discovery.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The control-plane position at which this projection was read.
    pub snapshot_cursor: i64,
    /// Provider rows, including charging-basis provenance.
    #[schema(value_type = Vec<Object>)]
    pub providers: Vec<serde_json::Value>,
    /// Model routes, including effort, window and price provenance.
    #[schema(value_type = Vec<Object>)]
    pub models: Vec<serde_json::Value>,
}

/// One declared seat in a Delivery Team template.
///
/// The role is a [`RoleSelectionDto`] rather than free-form JSON. Before this,
/// a slot's meaning lived in an opaque `id` the server never interpreted, which
/// made "which standard role is this seat?" a string every client answered for
/// itself. A selection names a catalog revision and a code, and the server owns
/// the rest.
///
/// `capabilities` stays a nested document on purpose: it is a chain, a context
/// class and a skill set, none of which is a role fact, and the daemon and the
/// domain validate it once rather than the wire schema validating it twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TeamDraftSlotRequest {
    /// The slot key within this template.
    pub id: String,
    /// The standard role this seat fills.
    pub role: RoleSelectionDto,
    /// What the seat in it may be.
    #[schema(value_type = Object)]
    pub capabilities: serde_json::Value,
}

/// One declared seat, as a projection reports it back.
///
/// The role is echoed as the selection that was stored. It becomes a fully
/// resolved [`ResolvedRoleRefDto`] when the role-catalog service is composed and
/// the daemon can look a code up; until then the honest answer is what was
/// selected, because a standard title invented here would be exactly the second
/// source of truth the selection type exists to remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamDraftSlotDto {
    /// The slot key within this template.
    pub id: String,
    /// The standard role this seat fills, as selected.
    pub role: RoleSelectionDto,
    /// What the seat in it may be.
    #[schema(value_type = Object)]
    pub capabilities: serde_json::Value,
}

/// One mutable draft document accepted by the realm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TeamDraftRequest {
    /// Stable logical template id.
    pub id: String,
    /// Human label.
    pub name: String,
    /// The seats this template declares.
    pub slots: Vec<TeamDraftSlotRequest>,
}

/// One server-held draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct TeamDraftDto {
    /// Stable logical template id.
    pub id: String,
    /// Human label.
    pub name: String,
    /// The seats this template declares.
    pub slots: Vec<TeamDraftSlotDto>,
    /// Server-resolved context policy preview for every slot.
    #[schema(value_type = Vec<Object>)]
    pub resolved_policy: Vec<serde_json::Value>,
}

/// One immutable published team-template revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PublishedTeamRevisionDto {
    /// Stable logical template id.
    pub id: String,
    /// Monotonic version within `id`.
    pub version: u32,
    /// Human label frozen at publish.
    pub name: String,
    /// The seats frozen at publish.
    pub slots: Vec<TeamDraftSlotDto>,
    /// Server-resolved context policy preview frozen from this revision.
    #[schema(value_type = Vec<Object>)]
    pub resolved_policy: Vec<serde_json::Value>,
}

/// Realm-qualified Teams read projection shared by HTTP, CLI and MCP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TeamsProjectionDto {
    /// The Realm that owns the documents.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The Teams projection cursor; every successful write advances it once.
    pub snapshot_cursor: i64,
    /// Current mutable drafts.
    pub drafts: Vec<TeamDraftDto>,
    /// Immutable published revisions.
    pub revisions: Vec<PublishedTeamRevisionDto>,
}

/// One provider-account profile, with nothing a caller could authenticate with.
///
/// The credential reference is an opaque alias whose meaning lives entirely in
/// the resolver's policy: publishing it discloses nothing, and there is no field
/// here that a token, a config home or a keychain target could occupy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AccountProfileDto {
    /// The profile.
    #[schema(value_type = String)]
    pub account_profile_id: AccountProfileId,
    /// Human label.
    #[schema(value_type = String)]
    pub label: ExternalName,
    /// The runtime family it authenticates against.
    #[schema(value_type = String)]
    pub harness: RuntimeKindKey,
    /// Whether launches may select it.
    pub enabled: bool,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// Whether this call created it, for an ensure.
    pub applied: AppliedDto,
}

/// What `provider-account-profiles:ensure` is asked for.
///
/// The label is the stable caller key. Only the two mutable fields a profile has
/// are settable here; everything credential-affecting is immutable for the life
/// of a profile, which is why rotation is a new profile rather than an edit.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct EnsureAccountProfileRequest {
    /// Human label. The natural identity inside the project.
    #[schema(value_type = String)]
    pub label: ExternalName,
    /// The runtime family this account authenticates against.
    #[schema(value_type = String)]
    pub harness: RuntimeKindKey,
    /// The approved credential alias the resolver looks the credential up under.
    /// An alias is not a capability: one the resolver policy does not already
    /// approve resolves to nothing.
    pub credential_alias: String,
    /// Whether launches may select it.
    pub enabled: bool,
}

/// What one configured runtime family can currently prove.
///
/// The family is a *name*, never an endpoint: there is no field here a URL, a
/// port or a client configuration could occupy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RuntimeCapabilityDto {
    /// The runtime family.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// How much of what it reports Kontor may act on.
    pub trust_grade: String,
    /// The operations it declares, in a stable order.
    pub supported: Vec<String>,
    /// Whether the runtime can prove which coding account a run executes as.
    /// When false, a per-run account pin is refused rather than silently ignored.
    pub account_env: bool,
    /// Largest message body it accepts, in bytes.
    pub max_message_bytes: u64,
    /// Largest history page it returns.
    pub max_history_page: u32,
    /// Largest number of simultaneous native sessions.
    pub max_concurrent_sessions: u32,
    /// Whether the family answered discovery at all. A family that could not be
    /// reached reports its absence rather than a plausible capability set.
    pub reachable: bool,
}

// ---------------------------------------------------------------------------
// Epic application
// ---------------------------------------------------------------------------

/// One external ticket an applied task is linked to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct TicketLinkRequest {
    /// The connector implementation. Never its vendor semantics.
    pub connector: String,
    /// The external issue key.
    pub external_issue_key: String,
}

/// One task in a declarative epic.
///
/// `title` is the stable caller key: dependencies name it, a reapply matches on
/// it, and it is immutable for the life of the task.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct EpicTaskRequest {
    /// The title, which is this task's identity inside the epic.
    #[schema(value_type = String)]
    pub title: ExternalName,
    /// The module the task contends for, if any.
    pub module: Option<String>,
    /// The titles of the sibling tasks that must finish first.
    #[serde(default)]
    #[schema(value_type = Vec<String>)]
    pub depends_on: BTreeSet<ExternalName>,
    /// The external tickets to link.
    #[serde(default)]
    pub ticket_links: Vec<TicketLinkRequest>,
    /// The absolute path this task's work happens in.
    ///
    /// It is the task's *placement*, and admission has nowhere else to learn it
    /// from: a seat is a session opened in a directory, and a control plane with
    /// no field for one either refuses to seat or invents a path — which is
    /// deciding where code gets edited by string formatting.
    ///
    /// Omitting it leaves any previously declared worktree alone rather than
    /// clearing it. A task that has never had one cannot be seated, and says so.
    pub worktree: Option<String>,
}

/// What `epics:apply` is asked for.
///
/// One request, one epic, all of it. The profile category is resolved and frozen
/// onto every task in the same transaction the tasks are created in, so there is
/// no window in which a task exists without the workflow it will be judged
/// against.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ApplyEpicRequest {
    /// The revision the caller read the project at.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// The epic's name, which is its identity inside the project.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// The work-profile category to resolve, from `GET /v1/catalog/work-profiles`.
    pub work_profile_category: String,
    /// The team template revision the caller believes the profile pins. Checked
    /// against what it actually pins, so a stale catalog read is refused rather
    /// than silently applied.
    pub team_template: Option<RevisionRefDto>,
    /// The runtime family the epic's work is intended for.
    #[schema(value_type = String)]
    pub runtime_family: RuntimeKindKey,
    /// The provider-account profile to pin, if any.
    #[schema(value_type = Option<String>)]
    pub account_profile_id: Option<AccountProfileId>,
    /// The tasks, in the order they should be created.
    pub tasks: Vec<EpicTaskRequest>,
}

/// One external ticket link after an epic was applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AppliedLinkDto {
    /// The link.
    pub link_id: String,
    /// The connector.
    pub connector: String,
    /// The external issue key.
    pub external_issue_key: String,
    /// Whether this call created it.
    pub applied: AppliedDto,
}

/// One task after an epic was applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AppliedTaskDto {
    /// The title it was addressed by.
    #[schema(value_type = String)]
    pub title: ExternalName,
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// Whether this call created it.
    pub applied: AppliedDto,
    /// Its lifecycle state.
    pub state: String,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// The workflow that froze the epic's profile onto it.
    pub workflow_id: String,
    /// The tasks it depends on.
    #[schema(value_type = Vec<String>)]
    pub depends_on: Vec<TaskId>,
    /// Its external ticket links.
    pub links: Vec<AppliedLinkDto>,
    /// Where its work happens, once declared.
    #[schema(value_type = Option<String>)]
    pub worktree: Option<ExternalName>,
}

/// One epic after it was applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AppliedEpicDto {
    /// The Realm it belongs to.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The project.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The goal that carries the epic.
    #[schema(value_type = String)]
    pub epic_id: MiniProjectId,
    /// Whether this call created it.
    pub applied: AppliedDto,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// The work-profile revision frozen onto every task.
    pub work_profile: RevisionRefDto,
    /// The team revision the profile pins, when it prescribes one.
    pub team_template: Option<RevisionRefDto>,
    /// A stable digest of the graph this call applied.
    ///
    /// It covers the *content* — the epic and its revision, the pinned profile
    /// and team revisions, and every task's identity, title, state, dependency
    /// set and ticket links — and nothing about the call that applied it. So a
    /// byte-identical reapply of an unchanged graph returns the same digest, and
    /// a caller diffing it to detect drift sees drift only when the graph
    /// actually moved.
    ///
    /// It is deliberately *not* the resolved bundle's digest: that one covers the
    /// resolution, including when it happened, and therefore differs on every
    /// call. Reporting it here made drift detection fire on every replay.
    pub bundle_hash: String,
    /// The tasks, in the order they were stated.
    pub tasks: Vec<AppliedTaskDto>,
}

// ---------------------------------------------------------------------------
// Epic projection
// ---------------------------------------------------------------------------

/// One gate the pinned profile declares, and what discharging it needs.
///
/// The declared authority travels with the state on purpose. A Lead driving a
/// task to closure has to know *who* may evaluate each gate and *which* artifacts
/// it requires, and a projection that reported only a state would make that
/// knowable solely by reading the profile pack out of band — which is exactly the
/// out-of-band step the public API exists to remove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct GateProjectionDto {
    /// The gate.
    pub gate: String,
    /// The phase it belongs to.
    pub phase: String,
    /// Its current state, reduced from the append-only evaluations.
    pub state: String,
    /// The roles the pinned profile authorizes to evaluate it.
    pub evaluator_roles: Vec<String>,
    /// The artifacts a pass or a waiver must cite.
    pub required_evidence: Vec<String>,
    /// Whether the profile permits waiving it at all.
    pub waiver_allowed: bool,
    /// The roles the profile authorizes to waive it.
    pub waiver_roles: Vec<String>,
}

/// One task, as the epic projection reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct EpicTaskProjectionDto {
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// Its title.
    #[schema(value_type = String)]
    pub title: ExternalName,
    /// Where its work happens. `None` is why a task cannot be seated, so it is
    /// reported rather than left to be discovered at admission.
    #[schema(value_type = Option<String>)]
    pub worktree: Option<ExternalName>,
    /// Its lifecycle state.
    pub state: String,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// The module it contends for, if any.
    pub module: Option<String>,
    /// The tasks it depends on.
    #[schema(value_type = Vec<String>)]
    pub depends_on: Vec<TaskId>,
    /// The phase its active workflow is in.
    pub current_phase: Option<String>,
    /// The revision a gate recording must present.
    ///
    /// It is the *workflow's* revision, not the task's: a gate verdict is checked
    /// against the workflow that pins the profile declaring the gate, and the two
    /// aggregates move independently. Reporting it here is what makes the gate
    /// list above actionable — a caller that had to guess it would be right only
    /// until the first phase advance.
    ///
    /// `None` when the task has no active workflow, which is also when `gates` is
    /// empty: there is no revision to present because there is nothing to record
    /// a verdict against.
    #[schema(value_type = Option<u64>)]
    pub workflow_revision: Option<AggregateRevision>,
    /// Every gate the pinned profile declares, in declaration order.
    pub gates: Vec<GateProjectionDto>,
    /// Every artifact the pinned profile requires, across all its phases and
    /// gates. What `complete_task` must be able to cite.
    pub required_artifacts: Vec<String>,
    /// Its external ticket links.
    pub links: Vec<AppliedLinkDto>,
    /// The team runs the scheduler created for it, newest last.
    pub team_runs: Vec<TeamRunProjectionDto>,
}

/// One team run and its seats, as the epic projection reports them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TeamRunProjectionDto {
    /// The team run.
    pub team_run_id: String,
    /// Its lifecycle.
    pub lifecycle: String,
    /// One entry per agent run the scheduler admitted into a role slot.
    pub seats: Vec<SeatProjectionDto>,
}

/// One seat: a role slot, the run filling it and the native session behind it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SeatProjectionDto {
    /// The role slot.
    pub role_slot: String,
    /// The agent run filling it.
    pub agent_run_id: String,
    /// The runtime family that owns the session, if the run is bound.
    pub runtime_kind: Option<String>,
    /// The runtime's own session id. Correlation evidence, never identity.
    pub native_id: Option<String>,
    /// Whether this process still holds the frozen capability snapshot for it.
    pub attached: bool,
}

/// One arming decision, as the projection reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AuthorizationProjectionDto {
    /// The authorization.
    pub authorization_id: String,
    /// What it covers.
    pub scope: String,
    /// The tasks it names explicitly, when it is not scope-wide.
    #[schema(value_type = Vec<String>)]
    pub selected_tasks: Vec<TaskId>,
    /// The first instant work may start.
    #[schema(value_type = String, format = DateTime)]
    pub allowed_start: Timestamp,
    /// The last instant work may start.
    #[schema(value_type = String, format = DateTime)]
    pub allowed_end: Timestamp,
    /// Maximum concurrent runs it authorizes.
    pub max_concurrency: u32,
    /// Whether it has been disarmed, and when.
    #[schema(value_type = Option<String>, format = DateTime)]
    pub revoked_at: Option<Timestamp>,
}

/// The whole of one epic, read at one control-plane position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct EpicProjectionDto {
    /// The Realm it belongs to.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The position this projection is consistent with. A subscriber resumes
    /// strictly after it.
    #[schema(value_type = i64)]
    pub snapshot_cursor: kontor_core::id::EventCursor,
    /// The project.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The goal that carries the epic.
    #[schema(value_type = String)]
    pub epic_id: MiniProjectId,
    /// Its name.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// The revision a write must present.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// The work-profile revision every task pins.
    pub work_profile: Option<RevisionRefDto>,
    /// The team revision that profile pins.
    pub team_template: Option<RevisionRefDto>,
    /// The tasks, oldest first.
    pub tasks: Vec<EpicTaskProjectionDto>,
    /// Every arming decision that touches this epic.
    pub authorizations: Vec<AuthorizationProjectionDto>,
    /// Whether startup reconciliation has finished, and therefore whether
    /// anything may be scheduled at all.
    pub scheduling_open: bool,
}

// ---------------------------------------------------------------------------
// Arm and disarm
// ---------------------------------------------------------------------------

/// The budget bounds an arming decision authorizes.
///
/// Every bound is mandatory and positive. There is no "unlimited": a bound that
/// could be omitted would read as "no work allowed" in one place and "no ceiling"
/// in another, and arming is exactly where that ambiguity is unaffordable.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct BudgetBoundsRequest {
    /// Maximum tokens across the armed work.
    pub max_tokens: u64,
    /// Maximum runtime commands across the armed work.
    pub max_commands: u64,
    /// Maximum wall-clock seconds across the armed work.
    pub max_duration_seconds: u64,
    /// Maximum monetary cost, in integer minor units.
    pub max_cost_minor_units: u64,
    /// The currency those minor units are in.
    pub cost_currency: String,
}

/// What `execution:arm` is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ArmRequest {
    /// The revision the caller read the epic at.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// The tasks to arm. Empty arms the whole epic.
    #[serde(default)]
    #[schema(value_type = Vec<String>)]
    pub tasks: Vec<TaskId>,
    /// The first instant work may start.
    #[schema(value_type = String, format = DateTime)]
    pub allowed_start: Timestamp,
    /// The last instant work may start.
    #[schema(value_type = String, format = DateTime)]
    pub allowed_end: Timestamp,
    /// Maximum concurrent runs.
    pub max_concurrency: u32,
    /// The budget ceiling.
    pub budget: BudgetBoundsRequest,
    /// The account profile acting as the granting authority.
    #[schema(value_type = String)]
    pub granted_by: AccountProfileId,
    /// Why the scope is being armed. Recorded, never interpreted.
    #[schema(value_type = String)]
    pub reason: ExternalName,
}

/// What `execution:disarm` is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct DisarmRequest {
    /// The authorization to revoke.
    pub authorization_id: String,
    /// The account profile acting as the revoking authority.
    #[schema(value_type = String)]
    pub revoked_by: AccountProfileId,
    /// Why it is being disarmed.
    #[schema(value_type = String)]
    pub reason: ExternalName,
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

/// One task the planner would admit, and what it would run under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReadyTaskDto {
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The authorization that arms it.
    pub authorization_id: String,
    /// The runtime family it would run on.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// The account profile it is pinned to, if any.
    #[schema(value_type = Option<String>)]
    pub account_profile_id: Option<AccountProfileId>,
}

/// One task the planner refused, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BlockedTaskDto {
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The stable machine-readable reason.
    pub code: String,
    /// The structural evidence behind it. Positions and ids, never values.
    #[schema(value_type = Vec<Object>)]
    pub evidence: Vec<serde_json::Value>,
}

/// What the planner decided, and what it decided against.
///
/// A plan is a dry run in the strongest sense available: it reads rows, it calls
/// no runtime, and it writes nothing. `plan_hash` is what `scheduler:start`
/// applies, so a caller starts the plan it was shown rather than whatever the
/// world looks like by the time it decides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SchedulerPlanDto {
    /// The Realm it was planned in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The digest of the whole plan.
    pub plan_hash: String,
    /// When it was taken.
    #[schema(value_type = String, format = DateTime)]
    pub taken_at: Timestamp,
    /// Whether startup reconciliation has finished. Nothing is admitted until it
    /// has, and a plan taken before it says so.
    pub scheduling_open: bool,
    /// The tasks that would be admitted, in admission order.
    pub ready: Vec<ReadyTaskDto>,
    /// The tasks that would not, with the reason for each.
    pub blocked: Vec<BlockedTaskDto>,
    /// The arming decisions the plan was computed against.
    pub authorizations: Vec<String>,
}

/// What `scheduler:start` is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct StartRequest {
    /// The plan digest a previous `scheduler:plan` returned. A plan that no
    /// longer describes the Realm is refused rather than re-derived, because the
    /// caller authorized *that* batch and not whatever replaced it.
    pub plan_hash: String,
}

/// One seat the start actually produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct StartedSeatDto {
    /// The task the seat is working on.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The team run the scheduler created or reused.
    pub team_run_id: String,
    /// The agent run filling the role slot.
    pub agent_run_id: String,
    /// The role slot.
    pub role_slot: String,
    /// The runtime family that owns the session.
    #[schema(value_type = String)]
    pub runtime_kind: RuntimeKindKey,
    /// The runtime's own session id. Correlation evidence, never identity.
    pub native_id: String,
    /// Whether this call created the seat, or found the runtime already holding
    /// one for that `(team run, role slot)` and reused it.
    pub applied: AppliedDto,
}

/// What starting a plan produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SchedulerStartDto {
    /// The Realm it was started in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The plan that was applied.
    pub plan_hash: String,
    /// The seats that now exist.
    pub started: Vec<StartedSeatDto>,
    /// The tasks the plan named that admission then refused, with the reason.
    pub blocked: Vec<BlockedTaskDto>,
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

/// The closed set of lifecycle transitions a Lead may ask for.
///
/// One endpoint and an enum rather than six routes: the transitions share every
/// input — an expected revision, a reason, the evidence the domain demands — and
/// splitting them would make it possible for one to drift out of agreement with
/// the transition table the others are judged by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleAction {
    /// Hold a task; it stops being eligible for admission.
    Block,
    /// Return a held task to ordinary scheduler eligibility. It never creates or
    /// resumes a native session on its own.
    Resume,
    /// Close a task, against its profile's phases, gates, artifacts and its
    /// team's role slots.
    CompleteTask,
    /// Re-open a closed task as a new, auditable non-terminal revision.
    ReopenTask,
    /// Close the epic, once every task, gate, run and ticket is terminal.
    CloseEpic,
    /// Re-open a closed epic.
    ReopenEpic,
}

/// What `lifecycle` is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct LifecycleRequest {
    /// Which transition.
    pub action: LifecycleAction,
    /// The task it applies to, for the task-scoped actions.
    #[schema(value_type = Option<String>)]
    pub task_id: Option<TaskId>,
    /// The revision the caller read the target at.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// Why. Recorded as evidence, never interpreted.
    #[schema(value_type = String)]
    pub reason: ExternalName,
    /// The artifacts cited as evidence, for a completion.
    #[serde(default)]
    pub evidence: Vec<String>,
}

/// What a lifecycle transition produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct LifecycleOutcomeDto {
    /// The Realm it happened in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The target that moved.
    pub target: String,
    /// The state it is now in.
    pub state: String,
    /// The revision it now stands at.
    #[schema(value_type = u64)]
    pub revision: AggregateRevision,
    /// The command receipt that authorizes it.
    pub receipt_id: String,
}

// ---------------------------------------------------------------------------
// Lead-required control and evidence operations
// ---------------------------------------------------------------------------

/// What `context:resolve` is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ResolveContextRequest {
    /// Whether to freeze the resolution onto the task's live run.
    ///
    /// A preview reads and returns nothing durable. A snapshot needs a run to
    /// belong to, because a frozen context pack is evidence about *what a run was
    /// given* and a pack belonging to no run is evidence about nothing.
    #[serde(default)]
    pub snapshot: bool,
}

/// Where one resolved value came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProvenanceDto {
    /// The JSON pointer the value sits at.
    pub path: String,
    /// The layer that last wrote it.
    pub layer: String,
    /// The source inside that layer.
    pub source_id: String,
    /// That source's revision.
    #[schema(value_type = u32)]
    pub revision: SpecVersion,
}

/// One member the resolver removed, and why.
///
/// It carries the path and the reason and never the value, not even its length:
/// a redaction record that described what it removed would be the disclosure it
/// exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RedactionDto {
    /// The JSON pointer that was removed.
    pub path: String,
    /// The source that declared the redaction.
    pub source_id: String,
    /// Why it was removed.
    pub reason: String,
}

/// What resolving a task's Context Pack produced.
///
/// The resolved document itself is deliberately absent. What a caller needs from
/// this operation is *determinism and accountability* — the same inputs produce
/// the same hash, and every path is attributable — and shipping the merged
/// content back would make this the one route through which everything the
/// resolver was given leaves the process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ResolvedContextDto {
    /// The Realm it was resolved in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The task it was resolved for.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The digest of the canonical resolved document.
    pub context_hash: String,
    /// The frozen pack, when this call snapshotted one.
    pub context_pack_id: Option<String>,
    /// The agent run the snapshot belongs to, when there is one.
    pub agent_run_id: Option<String>,
    /// Where every resolved path came from.
    pub provenance: Vec<ProvenanceDto>,
    /// Every member the resolver removed.
    pub redactions: Vec<RedactionDto>,
}

/// What `gates/{gate_id}:record` is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct RecordGateRequest {
    /// The revision the caller read the task's workflow at.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// The verdict. `waived` is an admin decision; the rest are operator work.
    pub verdict: String,
    /// The role recording it. Checked against the pinned profile's authority.
    pub evaluator_role: String,
    /// The account profile recording it.
    #[schema(value_type = String)]
    pub evaluator_account: AccountProfileId,
    /// The artifacts cited. A pass or a waiver requires the ones the profile
    /// declares.
    #[serde(default)]
    pub evidence: Vec<String>,
    /// The stable authenticated principal recording it.
    ///
    /// Omitting it records the verdict and attributes it to nobody; it never
    /// silently falls back to the run or the display name.
    pub reviewer_principal: Option<String>,
}

/// One recorded gate verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct GateVerdictDto {
    /// The Realm it was recorded in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The gate.
    pub gate: String,
    /// Its position in the gate's append-only history, starting at 1.
    pub sequence: u32,
    /// The verdict that was recorded.
    pub verdict: String,
    /// The gate's state once this verdict is reduced in.
    pub state: String,
    /// The command receipt that authorizes it.
    pub receipt_id: String,
}

/// What a selection-correction operation is asked for.
///
/// One request shape for all three corrections, because they are the same
/// decision about three different pins and splitting them would let one drift out
/// of agreement with the pre-run rule the others obey.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct SelectionRequest {
    /// The revision the caller read the task at.
    #[schema(value_type = u64)]
    pub expected_revision: AggregateRevision,
    /// The work-profile category to pin, for a profile correction.
    pub work_profile_category: Option<String>,
    /// The team revision the caller believes the profile pins, for a team
    /// correction.
    pub team_template: Option<RevisionRefDto>,
    /// The provider-account profile to pin, for an account correction.
    #[schema(value_type = Option<String>)]
    pub account_profile_id: Option<AccountProfileId>,
    /// Why the correction is being made. Recorded, never interpreted.
    #[schema(value_type = String)]
    pub reason: ExternalName,
}

/// What a selection correction produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SelectionDto {
    /// The Realm it happened in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The work-profile revision now pinned.
    pub work_profile: Option<RevisionRefDto>,
    /// The team revision that profile pins.
    pub team_template: Option<RevisionRefDto>,
    /// The provider-account profile now pinned.
    #[schema(value_type = Option<String>)]
    pub account_profile_id: Option<AccountProfileId>,
    /// Whether this call changed anything.
    pub applied: AppliedDto,
    /// The command receipt that authorizes it.
    pub receipt_id: String,
}

/// One typed field difference between Kontor and an external ticket.
///
/// The set of fields is closed by the pinned field specification: there is no
/// member here that could carry an arbitrary status, an assignee or a comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TicketFieldDiffDto {
    /// The semantic milestone the field maps to.
    pub milestone: String,
    /// What Kontor believes, as the pinned mapping spells it.
    pub kontor: String,
    /// What the external system last reported, when there is an observation.
    pub external: Option<String>,
}

/// What reconciling one task's tickets would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TicketReconcilePlanDto {
    /// The Realm it was planned in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The digest `reconcile-apply` must present.
    pub projection_hash: String,
    /// The links this plan covers.
    pub links: Vec<String>,
    /// The typed differences, if any.
    pub diff: Vec<TicketFieldDiffDto>,
    /// Whether every link is already converged.
    pub converged: bool,
}

/// What `ticket:reconcile-apply` is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct TicketReconcileApplyRequest {
    /// The digest a previous `reconcile-plan` returned.
    pub projection_hash: String,
}

/// What applying a ticket reconciliation produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TicketReconcileAppliedDto {
    /// The Realm it happened in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The plan that was applied.
    pub projection_hash: String,
    /// The links that converged.
    pub converged: Vec<String>,
    /// The command receipt that authorizes it.
    pub receipt_id: String,
}

/// What settling one run against its runtime produced.
///
/// Nothing in the request shapes this. There is no field on the way in for a
/// verdict, a terminal state, an evidence hash or a citation, which is the point:
/// settlement is Kontor *asking* a runtime what is true and recording the answer,
/// and an operator who could supply any of those would be closing a run on their
/// own authority while it looked like the runtime's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RuntimeSettlementDto {
    /// The Realm it happened in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The run that was settled.
    pub agent_run_id: String,
    /// What the runtime reported the session is doing, right now.
    pub observed: String,
    /// How the run closed, when the observation qualified to close it.
    pub outcome: Option<String>,
    /// The control-plane position of the observation the closure cites.
    ///
    /// It is a position in this Realm's own log, so the closure points at
    /// evidence the store can re-load and re-prove rather than at a digest the
    /// caller handed over.
    #[schema(value_type = Option<i64>)]
    pub evidence_cursor: Option<kontor_core::id::EventCursor>,
    /// Whether this call closed the run, or found it already closed.
    pub applied: AppliedDto,
    /// The team run, once every declared role slot is terminal and the team's
    /// closure has been certified.
    pub team_run_closed: Option<String>,
    /// Why the team is not closed yet, when it is not. A static rule, never a
    /// stored value.
    pub team_pending: Option<String>,
    /// The command receipt that authorizes the settlement.
    pub receipt_id: String,
}

/// What an operator says when abandoning a run no runtime ever took.
///
/// The revision is the operator's, not a convenience: an abandon decision is
/// made against a specific revision of a specific run, and closing a revision
/// nobody looked at would let a stale decision close work that has moved on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct AbandonRunRequest {
    /// The revision the caller read the run at.
    pub expected_revision: u64,
    /// Why the run is being abandoned. Recorded on the receipt.
    pub reason: String,
}

/// What abandoning one unbound run produced.
///
/// There is no field on the way in for an outcome, and none on the way out that
/// the caller chose. An operator may abandon a run; an operator may not declare
/// it cancelled, failed or succeeded — those are claims about a runtime, and
/// this operation exists precisely because no runtime ever answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct AbandonedRunDto {
    /// The Realm it happened in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The run that was abandoned.
    pub agent_run_id: String,
    /// How the run closed. Always `abandoned`.
    pub outcome: String,
    /// Whether this call closed the run, or found it already closed.
    pub applied: AppliedDto,
    /// The run's revision after the closure.
    #[schema(value_type = u64)]
    pub revision: kontor_core::id::AggregateRevision,
    /// The team run, once every one of its runs is terminal and the team's
    /// closure has been certified.
    pub team_run_closed: Option<String>,
    /// Why the team is not closed yet, when it is not. A static rule, never a
    /// stored value.
    pub team_pending: Option<String>,
    /// The command receipt that authorizes the abandon.
    pub receipt_id: String,
}

// ---------------------------------------------------------------------------
// Work-profile detail and validation
// ---------------------------------------------------------------------------

/// One phase of a work profile, as the catalog spells it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProfilePhaseDto {
    /// The phase.
    pub phase: String,
    /// Human label.
    #[schema(value_type = String)]
    pub label: ExternalName,
    /// Artifacts that must exist before the phase can complete.
    pub required_artifacts: Vec<String>,
    /// Gates evaluated at the end of it.
    pub gates: Vec<String>,
    /// Where rejected work returns to, when the profile routes it.
    pub rejection_route: Option<String>,
}

/// One artifact contract a work profile declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProfileArtifactDto {
    /// The artifact.
    pub artifact: String,
    /// Human label.
    #[schema(value_type = String)]
    pub label: ExternalName,
    /// The phase that produces it.
    pub producer_phase: String,
    /// Whether stored evidence is required, not merely a declaration.
    pub evidence_required: bool,
}

/// One declared handoff of the team a profile prescribes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProfileHandoffDto {
    /// The slot that hands work over.
    pub from_slot: String,
    /// The slot that receives it.
    pub to_slot: String,
    /// The phase after which the handoff may happen.
    pub after_phase: String,
    /// The artifacts the receiving slot needs before it may start.
    pub required_artifacts: Vec<String>,
}

/// The whole of one selectable work profile, resolved.
///
/// It is the catalog entry plus everything a Lead would otherwise have had to
/// read the pack out of band to learn: the phase order, the gate authority, the
/// artifact contracts and the handoff DAG that decides which seat starts first.
/// Nothing here is per-project — a category resolves to the same bundle in every
/// Realm running this build, which is why it is a workspace-level read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct WorkProfileDetailDto {
    /// The pack category this profile is advertised under.
    pub category: String,
    /// Human name.
    #[schema(value_type = String)]
    pub name: ExternalName,
    /// The profile revision the category resolves to.
    pub profile: RevisionRefDto,
    /// The team revision the profile pins, when it prescribes one.
    pub team: Option<RevisionRefDto>,
    /// The phase the profile enters at.
    pub entry_phase: String,
    /// The phases it declares, in declaration order.
    pub phases: Vec<ProfilePhaseDto>,
    /// The phases it may terminate at.
    pub terminal_phases: Vec<String>,
    /// Every gate it declares, with the authority and evidence each one names.
    pub gates: Vec<GateProjectionDto>,
    /// Every artifact contract it declares.
    pub artifacts: Vec<ProfileArtifactDto>,
    /// The declared handoffs of the team it pins.
    pub handoffs: Vec<ProfileHandoffDto>,
    /// The role slots that no handoff feeds, which are the seats that start with
    /// work rather than with an instruction to wait.
    pub eligible_roots: Vec<String>,
    /// The digest of the profile's canonical definition.
    ///
    /// This is the stable one. `bundle_hash` covers the *resolution*, which
    /// records when it happened, so two reads of an unchanged category answer
    /// with two different bundle digests and the same definition digest. A
    /// caller proving the pack has not moved compares this.
    pub definition_hash: String,
    /// The digest of the whole resolved bundle. What `epics:apply` freezes.
    pub bundle_hash: String,
}

/// What validating a work-profile category proved.
///
/// A validation answers about the *pack*, not about a request: it re-runs the
/// pack's own invariants and re-derives the bundle digest. There is no field
/// here a caller could supply a profile through — validating something the
/// deployment does not ship would prove nothing about what it will run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProfileValidationDto {
    /// The category that was validated.
    pub category: String,
    /// Whether the category is backed by a profile revision, or advertises
    /// vocabulary only. A manifest-only category is deliberately not runnable.
    pub availability: String,
    /// Whether the whole pack validates.
    pub pack_valid: bool,
    /// Whether the category resolves to a bundle that verifies against its own
    /// pinned digests.
    pub bundle_verified: bool,
    /// The bundle digest, when it resolved.
    pub bundle_hash: Option<String>,
    /// Why validation failed, when it did. A rule, never a stored value.
    pub refused: Option<String>,
}

// ---------------------------------------------------------------------------
// Bounded role turns
// ---------------------------------------------------------------------------

/// What `turns:settle` is asked for.
///
/// It settles **Kontor's** bounded turn in a persistent seat. There is no field
/// here for a runtime verdict, a terminal state or a native observation, and
/// that is the point: a seat is expected to still be sitting there when this
/// returns, ready for its next turn. Whether the *session* ever ended is a
/// separate question only the runtime can answer, and `runtime:settle` is where
/// it is asked.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct SettleTurnRequest {
    // There is deliberately **no** actor field. A caller-supplied account id
    // proves nothing about who is asking — checking that it exists and is
    // enabled is a fact about the account, not about the caller — so persisting
    // one as attribution would record a claim nothing verified. The settling
    // authority is taken from the credential the caller actually presented.
    /// The role slot whose turn this is.
    pub role_slot: String,
    /// The task revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_task_revision: AggregateRevision,
    /// The artifacts the turn produced.
    #[serde(default)]
    pub artifacts: Vec<String>,
}

/// What the Admin-only late-handoff reconciliation is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct AttestLateHandoffRequest {
    /// The role slot whose completed handoff is being reconciled.
    pub role_slot: String,
    /// The task revision the handoff was produced against.
    #[schema(value_type = u64)]
    pub expected_task_revision: AggregateRevision,
    /// The immutable native binding generation recorded by the run.
    pub binding_generation: u64,
    /// The handoff digest carried by the run's durable compaction receipt.
    pub handoff_hash: String,
    /// Valid artifact keys proving the bounded handoff.
    pub artifacts: Vec<String>,
}

/// What the Admin-only unusable-seat replacement is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ReplaceSeatRequest {
    /// The role slot whose terminal attempt is being replaced.
    pub role_slot: String,
    /// The task revision the replacement is reconciled against.
    #[schema(value_type = u64)]
    pub expected_task_revision: AggregateRevision,
    /// The immutable binding generation of the terminal predecessor.
    pub binding_generation: u64,
}

/// One follow-up a settled turn derived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TurnFollowUpDto {
    /// The slot the work was handed to.
    pub to_role_slot: String,
    /// The seat it reached, when one was materialized for that slot.
    pub target_agent_run_id: Option<String>,
    /// Whether the effect actually reached the seat.
    pub dispatched: bool,
    /// Why it was derived: the handoff's phase and artifact conditions are met.
    pub after_phase: Option<String>,
}

/// Excuse one declared role slot that was never bound to a session.
///
/// What this body deliberately does **not** carry is the design: no
/// `agent_run_id`, no binding or runtime identity, no outcome or lifecycle, no
/// `terminal: true`, no generic disposition kind, no caller credential. A waiver
/// is a statement about a *slot* and the template's own permission to omit it —
/// every one of those omitted fields would turn it into a statement about a
/// session or a run that nothing observed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct WaiveRoleSlotRequest {
    /// The team revision the caller believes is current.
    #[schema(value_type = u64)]
    pub expected_team_revision: AggregateRevision,
    /// The role the waiver is attributed to. Policy attribution, checked against
    /// the frozen slot's own policy — never a person, never the caller.
    pub authorized_by_role: String,
    /// Every evidence reference the frozen policy demands, at least.
    pub evidence: Vec<String>,
}

/// One recorded waiver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct RoleSlotWaiverDto {
    /// The Realm it was recorded in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The owning project.
    #[schema(value_type = String)]
    pub project_id: ProjectId,
    /// The task the team serves.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The team run whose slot was excused.
    pub team_run_id: String,
    /// The excused slot.
    pub role_slot: String,
    /// The waiver.
    pub waiver_id: String,
    /// Always `waived`. Spelled out rather than implied, and closed: there is no
    /// second disposition a caller may select.
    pub disposition: &'static str,
    /// The role it is attributed to.
    pub authorized_by_role: String,
    /// The tier the credential proved.
    pub authority_tier: String,
    /// The evidence cited.
    pub evidence: Vec<String>,
    /// The canonical digest the closure re-derives.
    pub evidence_hash: String,
    /// When it was recorded.
    #[schema(value_type = String)]
    pub recorded_at: Timestamp,
    /// Whether this call recorded it or replayed one.
    pub applied: AppliedDto,
    /// The team run this waiver closed, when it was the last slot outstanding.
    /// `null` while any other declared slot is still unaccounted for.
    pub team_run_closed: Option<String>,
}

/// What settling one bounded role turn produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SettledTurnDto {
    /// The Realm it was settled in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The receipt.
    pub turn_id: String,
    /// The task it served.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The seat's agent run, which stays open.
    pub agent_run_id: String,
    /// The role slot.
    pub role_slot: String,
    /// Its position in that seat's sequence of turns.
    pub turn_ordinal: u32,
    /// The native binding generation the seat was bound under.
    pub binding_generation: u64,
    /// The artifacts it produced.
    pub artifacts: Vec<String>,
    /// The tier the settling caller authenticated at. Not a claimed identity:
    /// it is what the presented credential proved.
    pub settled_by: String,
    /// The provider account the seat runs as, derived from the bound run rather
    /// than supplied by the caller. Operational context, never attribution.
    pub account_profile: Option<String>,
    /// The digest it was settled under.
    pub evidence_hash: String,
    /// Whether this call settled it, or replayed a settlement.
    pub applied: AppliedDto,
    /// Whether the seat's native session is still live. Settling a turn must
    /// never end one, so this is the assertion that it did not.
    pub seat_live: bool,
    /// The team run, once every declared slot has settled its final turn and the
    /// team's closure was certified from those rows. `None` while any slot is
    /// still unaccounted for.
    pub team_run_closed: Option<String>,
    /// The follow-ups this settlement derived, in slot order.
    pub follow_ups: Vec<TurnFollowUpDto>,
}

/// One immutable late-handoff disposition recorded after runtime cancellation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct LateHandoffAttestationDto {
    /// The Realm it was recorded in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The immutable turn evidence row.
    pub turn_id: String,
    /// The task it served.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The terminal run whose handoff was reconciled.
    pub agent_run_id: String,
    /// The reconciled role slot.
    pub role_slot: String,
    /// The immutable binding generation.
    pub binding_generation: u64,
    /// The durable compaction receipt that carried the handoff.
    pub compaction_receipt_id: String,
    /// The attested handoff digest.
    pub handoff_hash: String,
    /// The artifacts recorded on the disposition.
    pub artifacts: Vec<String>,
    /// Always `cancelled`; the attestation cannot change the runtime verdict.
    pub terminal_outcome: String,
    /// Always false; the operation never reopens or restores the native seat.
    pub seat_live: bool,
    /// Whether this call created or replayed the evidence row.
    pub applied: AppliedDto,
    /// The Admin tier proven by the caller credential.
    pub attested_by: String,
    /// The team run if this disposition completed its closure proof.
    pub team_run_closed: Option<String>,
    /// Normal follow-ups derived from the recorded handoff.
    pub follow_ups: Vec<TurnFollowUpDto>,
}

/// One linked successor created for an unusable persistent seat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReplacedSeatDto {
    /// The Realm it was created in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The task whose team owns the seat.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The preserved team run.
    pub team_run_id: String,
    /// The terminal predecessor; it remains immutable.
    pub predecessor_agent_run_id: String,
    /// The linked successor run.
    pub successor_agent_run_id: String,
    /// The role slot retained by the successor.
    pub role_slot: String,
    /// The successor's runtime family.
    pub runtime_kind: String,
    /// The successor's new native identity.
    pub native_id: String,
    /// Whether this call created the successor or replayed it.
    pub applied: AppliedDto,
}

// ---------------------------------------------------------------------------
// Catalogue registration
// ---------------------------------------------------------------------------

/// What `catalog/packs:register` is asked for.
///
/// The whole pack, as a document. It is one operation and not two — "register a
/// profile" and "register a team template" — because a work profile prescribes a
/// team, pins role and skill revisions, and cannot be resolved without them: a
/// profile admitted alone would be a catalogue entry that refuses to resolve, and
/// a team admitted alone would be one nothing can select.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct RegisterPackRequest {
    /// The pack document. Validated in full before anything is stored.
    #[schema(value_type = Object)]
    pub pack: serde_json::Value,
}

/// One profile pack this Realm can resolve a category from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProfilePackDto {
    /// The pack's open id.
    pub pack_id: String,
    /// This revision.
    #[schema(value_type = u32)]
    pub version: SpecVersion,
    /// Whether it is compiled into this build or was registered by an operator.
    pub source: String,
    /// The digest of the canonical document. `None` for the compiled pack, which
    /// is not stored as a document.
    pub document_hash: Option<String>,
    /// The categories it advertises, in manifest order.
    pub categories: Vec<String>,
    /// The team templates it carries.
    pub team_templates: Vec<RevisionRefDto>,
    /// Whether this call registered it, for a register.
    pub applied: AppliedDto,
}

// ---------------------------------------------------------------------------
// Triggers and intake
// ---------------------------------------------------------------------------

/// One pinned trigger revision, as an operator reads it back.
///
/// The filter clauses and the dedup pointers are reported as *pointers*, never
/// with the values a matching event carried: a trigger is configuration, and an
/// event's contents belong to the event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TriggerSpecDto {
    /// The trigger.
    pub trigger: String,
    /// This revision.
    #[schema(value_type = u32)]
    pub version: SpecVersion,
    /// The source kind it listens to.
    pub source_kind: String,
    /// The configured connection of that kind.
    pub source_connection: String,
    /// The event schema it accepts, at its pinned revision.
    pub event_schema: RevisionRefDto,
    /// The envelope pointers its filter constrains.
    pub filter_pointers: Vec<String>,
    /// The envelope pointers its dedup key is derived from.
    pub dedup_pointers: Vec<String>,
    /// The work profile revision the work it proposes would use.
    pub work_profile: RevisionRefDto,
    /// Whether it may arm the work it creates without a human, as the pinned
    /// policy spells it.
    pub auto_arm: bool,
}

/// What `intake:submit` is asked for.
///
/// The envelope is the *canonical* event, already redacted by whoever holds the
/// connection: this operation evaluates it and records the decision, and there is
/// no field here for a verdict, an approval or a work graph. What a matched event
/// becomes is the trigger's decision, not the submitter's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct SubmitIntakeRequest {
    /// The trigger to evaluate under.
    pub trigger: String,
    /// The pinned trigger revision.
    #[schema(value_type = u32)]
    pub trigger_version: SpecVersion,
    /// The event id as the source system spells it.
    #[schema(value_type = String)]
    pub external_event_id: ExternalId,
    /// When the source system observed it.
    #[schema(value_type = String)]
    pub external_observed_at: Timestamp,
    /// The canonical, redacted envelope.
    #[schema(value_type = Object)]
    pub envelope: serde_json::Value,
}

/// One recorded intake decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct IntakeReceiptDto {
    /// The Realm it was decided in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The decision.
    pub receipt_id: String,
    /// The event it decided about.
    pub source_event_id: String,
    /// The digest of that event's canonical envelope.
    pub source_event_hash: String,
    /// The trigger that decided, at the revision it decided under.
    pub trigger: RevisionRefDto,
    /// The deterministic outcome.
    pub result: String,
    /// The deterministic dedup key of the event.
    pub dedup_key: String,
    /// The original decision, when this one repeats it.
    pub duplicate_of: Option<String>,
    /// Whether this call recorded it, or found the event already decided.
    pub applied: AppliedDto,
}

// ---------------------------------------------------------------------------
// Connector specifications, conflicts, comments and ownership
// ---------------------------------------------------------------------------

/// One connector specification revision this build can serve.
///
/// `installed` distinguishes "this deployment ships the mapping" from "this
/// project pinned it": a bundled revision nothing installed is selectable, and a
/// task linked to a ticket it does not cover is exactly the unmapped link
/// `ticket:reconcile-plan` already reports rather than silently converging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ConnectorSpecDto {
    /// The connector implementation.
    pub connector: String,
    /// The external project the mapping is written for.
    pub external_project: String,
    /// The external issue type it covers.
    pub issue_type: String,
    /// The pinned revision.
    #[schema(value_type = u32)]
    pub version: SpecVersion,
    /// The digest of its canonical definition.
    pub definition_hash: String,
    /// What the revision declares, in declaration order: the closed field keys a
    /// field mapping covers, or the semantic milestones a workflow mapping does.
    pub covers: Vec<String>,
    /// Whether this project has this revision installed in its own store.
    pub installed: bool,
}

/// One recorded reconciliation conflict.
///
/// A conflict names its *kind* and the observation it was raised from, and
/// carries neither the external value that disagreed nor the comment that
/// mentioned it. The kind is what a human acts on; the values are what the
/// connector holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TicketConflictDto {
    /// The conflict.
    pub conflict_id: String,
    /// The ticket link it was raised on.
    pub link_id: String,
    /// Which typed conflict it is.
    pub kind: String,
    /// The observation it was raised from.
    pub observation_id: String,
    /// The task revision at the time it was raised.
    #[schema(value_type = u64)]
    pub task_revision: AggregateRevision,
    /// The specification revision it was judged against.
    #[schema(value_type = u32)]
    pub spec_version: SpecVersion,
    /// When it was raised.
    #[schema(value_type = String)]
    pub detected_at: Timestamp,
    /// When it was resolved, when it has been.
    #[schema(value_type = Option<String>)]
    pub resolved_at: Option<Timestamp>,
}

/// What `ticket:resolve-conflict` is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
pub struct ResolveConflictRequest {
    /// The conflict to close.
    pub conflict_id: String,
}

/// One mirrored inbound comment revision.
///
/// The body is deliberately absent and only its digest is reported. A comment is
/// mirrored so Kontor can prove *that* it saw a revision and in what order; a
/// read that also handed the prose back would make this the place ticket content
/// leaves the process, which is the disclosure the mirror exists to avoid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TicketCommentDto {
    /// The ticket link it arrived on.
    pub link_id: String,
    /// The comment id as the external system spells it.
    #[schema(value_type = String)]
    pub external_comment_id: ExternalId,
    /// The digest of the body. Never the body.
    pub body_hash: String,
    /// The author's external account id.
    #[schema(value_type = String)]
    pub author_account_id: ExternalId,
    /// When the external system says it was created.
    #[schema(value_type = String)]
    pub external_created_at: Timestamp,
    /// When the external system says it was last updated.
    #[schema(value_type = String)]
    pub external_updated_at: Timestamp,
    /// When Kontor mirrored it.
    #[schema(value_type = String)]
    pub observed_at: Timestamp,
    /// The revision this one edits, when it is an edit.
    pub supersedes: Option<String>,
}

/// What pulling one task's inbound comments produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TicketCommentPullDto {
    /// The Realm it happened in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The links the pull covered.
    pub links: Vec<String>,
    /// How many revisions this pull mirrored. A replay mirrors none.
    pub mirrored: u32,
    /// How many revisions the task holds after the pull.
    pub held: u32,
    /// The command receipt that authorizes it.
    pub receipt_id: String,
}

/// What claiming one task's tickets would do, and did.
///
/// The action is always [`OwnershipAction::ReassignToPrincipal`] as the domain
/// spells it — there is no field on the way in for an assignee, so a caller can
/// claim a ticket for the principal Kontor authenticates as and for nobody else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TicketClaimDto {
    /// The Realm it happened in.
    #[schema(value_type = String)]
    pub realm_id: kontor_core::id::RealmId,
    /// The task.
    #[schema(value_type = String)]
    pub task_id: TaskId,
    /// The links the claim covers.
    pub links: Vec<String>,
    /// The ownership action, as the domain names it.
    pub action: String,
    /// The command receipt that authorizes it.
    pub receipt_id: String,
}

// ---------------------------------------------------------------------------
// The port
// ---------------------------------------------------------------------------

/// Every public application operation, as one port.
///
/// It is a trait for exactly one reason: `kontor-api` is stated against the
/// domain ports and the runtime contract, and composing the profile pack, the
/// team layer, the scheduler and the connectors is the composition root's job.
/// Making this crate depend on those would put the choice of which services a
/// Realm has in the layer that is supposed to be indifferent to it.
///
/// There is one implementation, in `kontor-daemon`. Every method is expected to
/// be idempotent under its `Idempotency-Key`: replaying the same key with the
/// same canonical request returns the original answer, and reusing it with
/// different bytes is a conflict.
#[async_trait]
pub trait ApplicationOperations: Send + Sync {
    /// Create a project, or return the one already standing at that root.
    async fn ensure_project(
        &self,
        key: &IdempotencyKey,
        request: &EnsureProjectRequest,
    ) -> Result<ProjectDto, ApiError>;

    /// Every work profile a caller may select, with the team each one pins.
    fn work_profiles(&self) -> Result<Vec<WorkProfileCatalogDto>, ApiError>;

    /// Every team template revision a work profile may pin.
    fn team_templates(&self) -> Result<Vec<TeamTemplateCatalogDto>, ApiError>;

    /// The live model catalog for this Realm.
    fn model_catalog(&self) -> Result<ModelCatalogDto, ApiError>;

    /// Current Teams drafts and immutable revisions.
    fn teams(&self) -> Result<TeamsProjectionDto, ApiError>;

    /// Create or replace one server-held draft.
    async fn save_team_draft(
        &self,
        key: &IdempotencyKey,
        request: &TeamDraftRequest,
    ) -> Result<TeamsProjectionDto, ApiError>;

    /// Publish the next immutable revision of one draft.
    async fn publish_team(
        &self,
        key: &IdempotencyKey,
        team_id: &str,
    ) -> Result<TeamsProjectionDto, ApiError>;

    /// Every provider-account profile in a project, with no credential material.
    fn account_profiles(&self, project_id: ProjectId) -> Result<Vec<AccountProfileDto>, ApiError>;

    /// Create a provider-account profile, or return the one with that label.
    async fn ensure_account_profile(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &EnsureAccountProfileRequest,
    ) -> Result<AccountProfileDto, ApiError>;

    /// What every configured runtime family can currently prove.
    async fn runtime_capabilities(&self) -> Result<Vec<RuntimeCapabilityDto>, ApiError>;

    /// Build one complete topology-specification candidate. Persists nothing.
    fn draft_topology_spec(
        &self,
        project_id: ProjectId,
        request: &DraftTopologySpecRequest,
    ) -> Result<TopologySpecCandidateDto, ApiError>;

    /// Judge one complete candidate. Persists nothing.
    fn validate_topology_spec(
        &self,
        project_id: ProjectId,
        request: &ValidateTopologySpecRequest,
    ) -> Result<TopologySpecValidationDto, ApiError>;

    /// Publish one revalidated candidate as an immutable revision.
    async fn publish_topology_spec(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &PublishTopologySpecRequest,
    ) -> Result<PublishedTopologySpecDto, ApiError>;

    /// One exact immutable specification document.
    fn topology_spec(
        &self,
        project_id: ProjectId,
        spec_id: TopologySpecId,
        version: SpecVersion,
    ) -> Result<TopologySpecDocumentDto, ApiError>;

    /// One whole role-catalog revision, in its declared order.
    fn role_catalog(
        &self,
        catalog_id: RoleCatalogId,
        version: SpecVersion,
    ) -> Result<RoleCatalogDto, ApiError>;

    /// One resolved catalog entry. An unknown revision or code is never guessed.
    fn role(
        &self,
        catalog_id: RoleCatalogId,
        version: SpecVersion,
        role_code: &str,
    ) -> Result<RoleCatalogEntryDto, ApiError>;

    /// Every controlled code one epic's pinned revisions define.
    fn code_help(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<CodeHelpProjectionDto, ApiError>;

    /// The stored authoritative topology, optionally narrowed to one epic.
    fn inspect_topology(
        &self,
        project_id: ProjectId,
        epic_id: Option<MiniProjectId>,
    ) -> Result<TopologyProjectionDto, ApiError>;

    /// Read the exact native identities back and record what was observed.
    async fn drift_topology(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &SemanticTopologyRequest,
    ) -> Result<TopologyMutationDto, ApiError>;

    /// Ensure the logical nodes one semantic scope needs. No native effect.
    async fn ensure_topology(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &SemanticTopologyRequest,
    ) -> Result<TopologyMutationDto, ApiError>;

    /// Materialize or reconcile an ensured scope through the admission path.
    async fn materialize_topology(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &SemanticTopologyRequest,
    ) -> Result<TopologyMutationDto, ApiError>;

    /// Retire one already-returned node after child and seat policy checks.
    async fn retire_topology_node(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        request: &TopologyNodeRequest,
    ) -> Result<TopologyMutationDto, ApiError>;

    /// Archive one already-retired node after exact readback.
    async fn archive_topology_node(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        request: &TopologyNodeRequest,
    ) -> Result<TopologyMutationDto, ApiError>;

    /// What moving one epic's pinned specification would do. Commits nothing.
    fn preview_topology_upgrade(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &TopologyUpgradePreviewRequest,
    ) -> Result<TopologyUpgradePreviewDto, ApiError>;

    /// Apply the named preview and return the new immutable pin.
    async fn apply_topology_upgrade(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &TopologyUpgradeApplyRequest,
    ) -> Result<AppliedTopologyUpgradeDto, ApiError>;

    /// Apply one whole epic — graph, links, selections — atomically.
    async fn apply_epic(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &ApplyEpicRequest,
    ) -> Result<AppliedEpicDto, ApiError>;

    /// The whole of one epic, read at one control-plane position.
    fn read_epic(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<EpicProjectionDto, ApiError>;

    /// Arm a bounded scope, or return the authorization the same key granted.
    async fn arm(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &ArmRequest,
    ) -> Result<AuthorizationProjectionDto, ApiError>;

    /// Revoke future admission under one authorization.
    async fn disarm(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &DisarmRequest,
    ) -> Result<AuthorizationProjectionDto, ApiError>;

    /// What the scheduler would admit right now, and what it would refuse.
    async fn plan(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<SchedulerPlanDto, ApiError>;

    /// Apply a named plan through the existing admission path.
    async fn start(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &StartRequest,
    ) -> Result<SchedulerStartDto, ApiError>;

    /// Move a task or the epic through one legal, evidenced transition.
    async fn lifecycle(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &LifecycleRequest,
    ) -> Result<LifecycleOutcomeDto, ApiError>;

    /// Resolve one task's Context Pack, previewing or freezing it.
    async fn resolve_context(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &ResolveContextRequest,
    ) -> Result<ResolvedContextDto, ApiError>;

    /// Append one gate verdict to a task's workflow.
    async fn record_gate(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        gate: &str,
        request: &RecordGateRequest,
    ) -> Result<GateVerdictDto, ApiError>;

    /// Correct one task's pinned work profile before a run snapshots it.
    async fn select_profile(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &SelectionRequest,
    ) -> Result<SelectionDto, ApiError>;

    /// Confirm the team revision a task's pinned profile prescribes.
    async fn select_team(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &SelectionRequest,
    ) -> Result<SelectionDto, ApiError>;

    /// Correct one task's pinned provider account before a run snapshots it.
    async fn select_account(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &SelectionRequest,
    ) -> Result<SelectionDto, ApiError>;

    /// What reconciling one task's external tickets would do. Writes nothing.
    async fn ticket_reconcile_plan(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<TicketReconcilePlanDto, ApiError>;

    /// Apply a named ticket reconciliation plan.
    async fn ticket_reconcile_apply(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &TicketReconcileApplyRequest,
    ) -> Result<TicketReconcileAppliedDto, ApiError>;

    /// Retry every follow-up that was derived and never delivered.
    ///
    /// Called from startup reconciliation. It delivers nothing new — a follow-up
    /// is *derived* only by settling a turn — so a restart cannot invent work;
    /// it only finishes what a previous process decided and did not manage to
    /// hand over. Returns how many reached a seat this time.
    async fn retry_undelivered_dispatches(&self) -> Result<usize, ApiError>;

    /// Excuse one declared role slot that was never bound.
    ///
    /// Admin, matching gate-waiver authority. Every rule that makes the waiver
    /// legal is proved against the run's *frozen* snapshot inside the write
    /// transaction, so nothing here can be true of a state that has since moved.
    async fn waive_role_slot(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        team_run_id: kontor_core::id::TeamRunId,
        role_slot: &str,
        request: &WaiveRoleSlotRequest,
    ) -> Result<RoleSlotWaiverDto, ApiError>;

    /// Settle one bounded Kontor role turn, leaving the seat live.
    async fn settle_turn(
        &self,
        key: &IdempotencyKey,
        authority: CallerCapability,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        request: &SettleTurnRequest,
    ) -> Result<SettledTurnDto, ApiError>;

    /// Record a bounded handoff after runtime cancellation without reopening.
    async fn attest_late_handoff(
        &self,
        key: &IdempotencyKey,
        authority: CallerCapability,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        request: &AttestLateHandoffRequest,
    ) -> Result<LateHandoffAttestationDto, ApiError>;

    /// Replace one runtime-terminal unusable seat with a linked successor.
    async fn replace_seat(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        request: &ReplaceSeatRequest,
    ) -> Result<ReplacedSeatDto, ApiError>;

    /// Settle one run against a fresh reading of its runtime.
    async fn settle_runtime(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> Result<RuntimeSettlementDto, ApiError>;

    /// Abandon one run that was admitted but never bound to a session.
    ///
    /// The recovery path for a launch that was refused. Admission commits the
    /// run before the runtime is asked for a session, so a refused launch leaves
    /// a non-terminal run behind with nothing to settle and nothing to cancel —
    /// and a non-terminal run keeps its task in flight, so the task can never be
    /// scheduled again. Every other exit demands evidence from a runtime that,
    /// in this case, never answered.
    async fn abandon_run(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        request: &AbandonRunRequest,
    ) -> Result<AbandonedRunDto, ApiError>;

    /// Register one profile pack, additively, alongside the compiled seeds.
    async fn register_pack(
        &self,
        key: &IdempotencyKey,
        request: &RegisterPackRequest,
    ) -> Result<ProfilePackDto, ApiError>;

    /// Every pack this Realm can resolve a category from.
    fn profile_packs(&self) -> Result<Vec<ProfilePackDto>, ApiError>;

    /// The whole of one selectable work profile, resolved.
    fn work_profile(&self, category: &str) -> Result<WorkProfileDetailDto, ApiError>;

    /// Re-run the pack's own invariants over one category.
    fn validate_work_profile(&self, category: &str) -> Result<ProfileValidationDto, ApiError>;

    /// One pinned trigger revision.
    fn trigger(
        &self,
        project_id: ProjectId,
        trigger: &str,
        version: SpecVersion,
    ) -> Result<TriggerSpecDto, ApiError>;

    /// Evaluate one canonical source event and record the decision.
    async fn submit_intake(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &SubmitIntakeRequest,
    ) -> Result<IntakeReceiptDto, ApiError>;

    /// Read one recorded intake decision.
    fn intake_receipt(
        &self,
        project_id: ProjectId,
        receipt_id: &str,
    ) -> Result<IntakeReceiptDto, ApiError>;

    /// Every ticket field-mapping revision this build can serve for a connector.
    fn connector_field_specs(
        &self,
        project_id: ProjectId,
        connector: &str,
    ) -> Result<Vec<ConnectorSpecDto>, ApiError>;

    /// Every external-workflow revision this build can serve for a connector.
    fn connector_workflow_specs(
        &self,
        project_id: ProjectId,
        connector: &str,
    ) -> Result<Vec<ConnectorSpecDto>, ApiError>;

    /// Every reconciliation conflict recorded against one task's tickets.
    fn ticket_conflicts(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<Vec<TicketConflictDto>, ApiError>;

    /// Close one reconciliation conflict, citing the receipt that authorizes it.
    async fn resolve_ticket_conflict(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &ResolveConflictRequest,
    ) -> Result<TicketConflictDto, ApiError>;

    /// Mirror one task's inbound external comments.
    async fn pull_ticket_comments(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<TicketCommentPullDto, ApiError>;

    /// The inbound comment revisions one task holds, without their bodies.
    fn ticket_comments(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<Vec<TicketCommentDto>, ApiError>;

    /// Claim one task's external tickets for the principal Kontor acts as.
    async fn claim_ticket(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<TicketClaimDto, ApiError>;
}

/// The application service the composition root handed this process.
pub type Applications = Arc<dyn ApplicationOperations>;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Create a project, or return the one already standing at that root.
#[utoipa::path(
    post, path = "/v1/projects:ensure", tag = "applications",
    params(("Idempotency-Key" = String, Header, description = "The caller's stable key")),
    request_body = EnsureProjectRequest,
    responses(
        (status = 200, body = ProjectDto, description = "Created, or returned unchanged"),
        (status = 401), (status = 403),
        (status = 409, description = "The root exists under a different name, or the key was reused")
    )
)]
pub async fn ensure_project(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
    Json(request): Json<EnsureProjectRequest>,
) -> Result<Json<ProjectDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state.applications().ensure_project(&key, &request).await?,
    ))
}

/// The work profiles a caller may select.
#[utoipa::path(
    get, path = "/v1/catalog/work-profiles", tag = "applications",
    responses((status = 200, body = Vec<WorkProfileCatalogDto>), (status = 401), (status = 403))
)]
pub async fn work_profiles(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<Vec<WorkProfileCatalogDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    Ok(Json(state.applications().work_profiles()?))
}

/// The team template revisions a work profile may pin.
#[utoipa::path(
    get, path = "/v1/catalog/team-templates", tag = "applications",
    responses((status = 200, body = Vec<TeamTemplateCatalogDto>), (status = 401), (status = 403))
)]
pub async fn team_templates(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<Vec<TeamTemplateCatalogDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    Ok(Json(state.applications().team_templates()?))
}

/// The provider/model catalog discovered for this Realm.
#[utoipa::path(
    get, path = "/v1/catalog", tag = "applications",
    responses((status = 200, body = ModelCatalogDto), (status = 401), (status = 403))
)]
pub async fn model_catalog(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<ModelCatalogDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    Ok(Json(state.applications().model_catalog()?))
}

/// Current Teams drafts and immutable published revisions.
#[utoipa::path(
    get, path = "/v1/teams", tag = "applications",
    responses((status = 200, body = TeamsProjectionDto), (status = 401), (status = 403))
)]
pub async fn teams(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<TeamsProjectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    Ok(Json(state.applications().teams()?))
}

/// Create or replace one Teams draft in this Realm.
#[utoipa::path(
    post, path = "/v1/teams/drafts:save", tag = "applications",
    params(("Idempotency-Key" = String, Header, description = "The caller's stable key")),
    request_body = TeamDraftRequest,
    responses((status = 200, body = TeamsProjectionDto), (status = 401), (status = 403), (status = 409))
)]
pub async fn save_team_draft(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
    Json(request): Json<TeamDraftRequest>,
) -> Result<Json<TeamsProjectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state.applications().save_team_draft(&key, &request).await?,
    ))
}

/// Publish the next immutable revision of one Teams draft.
#[utoipa::path(
    post, path = "/v1/teams/{team_id}/publish", tag = "applications",
    params(
        ("team_id" = String, Path, description = "The logical team-template id"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    responses((status = 200, body = TeamsProjectionDto), (status = 401), (status = 403), (status = 404), (status = 409))
)]
pub async fn publish_team(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
    Path(team_id): Path<String>,
) -> Result<Json<TeamsProjectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state.applications().publish_team(&key, &team_id).await?,
    ))
}

/// The provider-account profiles a run may be pinned to.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/provider-account-profiles", tag = "applications",
    params(("project_id" = String, Path, description = "The owning project")),
    responses((status = 200, body = Vec<AccountProfileDto>), (status = 401), (status = 403))
)]
pub async fn account_profiles(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<AccountProfileDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    Ok(Json(state.applications().account_profiles(project_id)?))
}

/// Create a provider-account profile, or return the one with that label.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/provider-account-profiles:ensure", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = EnsureAccountProfileRequest,
    responses(
        (status = 200, body = AccountProfileDto),
        (status = 401), (status = 403), (status = 409)
    )
)]
pub async fn ensure_account_profile(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<EnsureAccountProfileRequest>,
) -> Result<Json<AccountProfileDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .ensure_account_profile(&key, project_id, &request)
            .await?,
    ))
}

/// What every configured runtime family can currently prove.
#[utoipa::path(
    get, path = "/v1/runtime-capabilities", tag = "applications",
    responses((status = 200, body = Vec<RuntimeCapabilityDto>), (status = 401), (status = 403))
)]
pub async fn runtime_capabilities(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<Vec<RuntimeCapabilityDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    Ok(Json(state.applications().runtime_capabilities().await?))
}

/// Build one complete topology-specification candidate.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/topology-specs:draft", tag = "applications",
    params(("project_id" = String, Path, description = "The owning project")),
    request_body = DraftTopologySpecRequest,
    responses(
        (status = 200, body = TopologySpecCandidateDto, description = "A candidate, persisted nowhere"),
        (status = 401), (status = 403), (status = 404)
    )
)]
pub async fn draft_topology_spec(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    Json(request): Json<DraftTopologySpecRequest>,
) -> Result<Json<TopologySpecCandidateDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    Ok(Json(
        state
            .applications()
            .draft_topology_spec(project_id, &request)?,
    ))
}

/// Judge one complete topology-specification candidate.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/topology-specs:validate", tag = "applications",
    params(("project_id" = String, Path, description = "The owning project")),
    request_body = ValidateTopologySpecRequest,
    responses(
        (status = 200, body = TopologySpecValidationDto, description = "Ordered violations, empty when publishable"),
        (status = 401), (status = 403), (status = 404)
    )
)]
pub async fn validate_topology_spec(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    Json(request): Json<ValidateTopologySpecRequest>,
) -> Result<Json<TopologySpecValidationDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    Ok(Json(
        state
            .applications()
            .validate_topology_spec(project_id, &request)?,
    ))
}

/// Publish one revalidated candidate as an immutable revision.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/topology-specs:publish", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = PublishTopologySpecRequest,
    responses(
        (status = 200, body = PublishedTopologySpecDto),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "A stale project revision, or the key was reused for different bytes")
    )
)]
pub async fn publish_topology_spec(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<PublishTopologySpecRequest>,
) -> Result<Json<PublishedTopologySpecDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .publish_topology_spec(&key, project_id, &request)
            .await?,
    ))
}

/// One exact immutable topology-specification document.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/topology-specs/{spec_id}/{version}", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("spec_id" = String, Path, description = "The specification identity"),
        ("version" = u32, Path, description = "The published revision")
    ),
    responses((status = 200, body = TopologySpecDocumentDto), (status = 401), (status = 403), (status = 404))
)]
pub async fn topology_spec(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, spec_id, version)): Path<(String, String, u32)>,
) -> Result<Json<TopologySpecDocumentDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let spec_id = parse_id(&state, TopologySpecId::parse(&spec_id))?;
    let version = parse_id(&state, SpecVersion::parse(version))?;
    Ok(Json(
        state
            .applications()
            .topology_spec(project_id, spec_id, version)?,
    ))
}

/// One whole role-catalog revision.
#[utoipa::path(
    get, path = "/v1/catalog/role-catalogs/{catalog_id}/{version}", tag = "applications",
    params(
        ("catalog_id" = String, Path, description = "The catalog identity"),
        ("version" = u32, Path, description = "The revision")
    ),
    responses((status = 200, body = RoleCatalogDto), (status = 401), (status = 403), (status = 404))
)]
pub async fn role_catalog(
    State(state): State<ApiState>,
    caller: Caller,
    Path((catalog_id, version)): Path<(String, u32)>,
) -> Result<Json<RoleCatalogDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let catalog_id = parse_id(&state, RoleCatalogId::parse(&catalog_id))?;
    let version = parse_id(&state, SpecVersion::parse(version))?;
    Ok(Json(
        state.applications().role_catalog(catalog_id, version)?,
    ))
}

/// One resolved role from one catalog revision.
#[utoipa::path(
    get, path = "/v1/catalog/role-catalogs/{catalog_id}/{version}/roles/{role_code}", tag = "applications",
    params(
        ("catalog_id" = String, Path, description = "The catalog identity"),
        ("version" = u32, Path, description = "The revision"),
        ("role_code" = String, Path, description = "The stable role code")
    ),
    responses(
        (status = 200, body = RoleCatalogEntryDto),
        (status = 401), (status = 403),
        (status = 404, description = "An unknown revision or code, never a guess")
    )
)]
pub async fn role(
    State(state): State<ApiState>,
    caller: Caller,
    Path((catalog_id, version, role_code)): Path<(String, u32, String)>,
) -> Result<Json<RoleCatalogEntryDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let catalog_id = parse_id(&state, RoleCatalogId::parse(&catalog_id))?;
    let version = parse_id(&state, SpecVersion::parse(version))?;
    Ok(Json(
        state.applications().role(catalog_id, version, &role_code)?,
    ))
}

/// Every controlled code one epic's pinned revisions define.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/epics/{epic_id}/code-help", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic whose pins are read")
    ),
    responses((status = 200, body = CodeHelpProjectionDto), (status = 401), (status = 403), (status = 404))
)]
pub async fn code_help(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
) -> Result<Json<CodeHelpProjectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let epic_id = parse_id(&state, MiniProjectId::parse(&epic_id))?;
    Ok(Json(state.applications().code_help(project_id, epic_id)?))
}

/// The stored authoritative topology.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/topology:inspect", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = Option<String>, Query, description = "Narrow to one epic's pinned subgraph")
    ),
    responses((status = 200, body = TopologyProjectionDto), (status = 401), (status = 403), (status = 404))
)]
pub async fn inspect_topology(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    Query(query): Query<TopologyScopeQuery>,
) -> Result<Json<TopologyProjectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let epic_id = query
        .epic_id
        .map(|epic| parse_id(&state, MiniProjectId::parse(&epic)))
        .transpose()?;
    Ok(Json(
        state.applications().inspect_topology(project_id, epic_id)?,
    ))
}

/// Read the exact native identities back and record what was observed.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/topology:drift", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = SemanticTopologyRequest,
    responses((status = 200, body = TopologyMutationDto), (status = 401), (status = 403), (status = 404), (status = 409))
)]
pub async fn drift_topology(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SemanticTopologyRequest>,
) -> Result<Json<TopologyMutationDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .drift_topology(&key, project_id, &request)
            .await?,
    ))
}

/// Ensure the logical nodes one semantic scope needs.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/topology:ensure", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = SemanticTopologyRequest,
    responses((status = 200, body = TopologyMutationDto), (status = 401), (status = 403), (status = 404), (status = 409))
)]
pub async fn ensure_topology(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SemanticTopologyRequest>,
) -> Result<Json<TopologyMutationDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .ensure_topology(&key, project_id, &request)
            .await?,
    ))
}

/// Materialize or reconcile an ensured scope.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/topology:materialize", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = SemanticTopologyRequest,
    responses(
        (status = 200, body = TopologyMutationDto),
        (status = 401), (status = 403), (status = 404), (status = 409),
        (status = 503, description = "A runtime could not be reached")
    )
)]
pub async fn materialize_topology(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SemanticTopologyRequest>,
) -> Result<Json<TopologyMutationDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .materialize_topology(&key, project_id, &request)
            .await?,
    ))
}

/// Retire one already-returned node.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/topology/nodes/{topology_node_id}/retire", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("topology_node_id" = String, Path, description = "The node a projection returned"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = TopologyNodeRequest,
    responses((status = 200, body = TopologyMutationDto), (status = 401), (status = 403), (status = 404), (status = 409))
)]
pub async fn retire_topology_node(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, topology_node_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<TopologyNodeRequest>,
) -> Result<Json<TopologyMutationDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let topology_node_id = parse_id(&state, TopologyNodeId::parse(&topology_node_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .retire_topology_node(&key, project_id, topology_node_id, &request)
            .await?,
    ))
}

/// Archive one already-retired node.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/topology/nodes/{topology_node_id}/archive", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("topology_node_id" = String, Path, description = "The node a projection returned"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = TopologyNodeRequest,
    responses((status = 200, body = TopologyMutationDto), (status = 401), (status = 403), (status = 404), (status = 409))
)]
pub async fn archive_topology_node(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, topology_node_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<TopologyNodeRequest>,
) -> Result<Json<TopologyMutationDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let topology_node_id = parse_id(&state, TopologyNodeId::parse(&topology_node_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .archive_topology_node(&key, project_id, topology_node_id, &request)
            .await?,
    ))
}

/// What moving one epic's pinned specification would do.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics/{epic_id}/topology:upgrade-preview", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic whose pin would move")
    ),
    request_body = TopologyUpgradePreviewRequest,
    responses((status = 200, body = TopologyUpgradePreviewDto), (status = 401), (status = 403), (status = 404))
)]
pub async fn preview_topology_upgrade(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
    Json(request): Json<TopologyUpgradePreviewRequest>,
) -> Result<Json<TopologyUpgradePreviewDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let epic_id = parse_id(&state, MiniProjectId::parse(&epic_id))?;
    Ok(Json(
        state
            .applications()
            .preview_topology_upgrade(project_id, epic_id, &request)?,
    ))
}

/// Apply the named upgrade preview.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics/{epic_id}/topology:upgrade-apply", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic whose pin moves"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = TopologyUpgradeApplyRequest,
    responses((status = 200, body = AppliedTopologyUpgradeDto), (status = 401), (status = 403), (status = 404), (status = 409))
)]
pub async fn apply_topology_upgrade(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<TopologyUpgradeApplyRequest>,
) -> Result<Json<AppliedTopologyUpgradeDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let epic_id = parse_id(&state, MiniProjectId::parse(&epic_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .apply_topology_upgrade(&key, project_id, epic_id, &request)
            .await?,
    ))
}

/// Apply one whole epic atomically.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics:apply", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = ApplyEpicRequest,
    responses(
        (status = 200, body = AppliedEpicDto, description = "Applied, or returned unchanged"),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "Drift, a stale revision, or a reused key")
    )
)]
pub async fn apply_epic(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ApplyEpicRequest>,
) -> Result<Json<AppliedEpicDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .apply_epic(&key, project_id, &request)
            .await?,
    ))
}

/// The whole of one epic, read at one control-plane position.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/epics/{epic_id}", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic")
    ),
    responses((status = 200, body = EpicProjectionDto), (status = 401), (status = 404))
)]
pub async fn read_epic(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
) -> Result<Json<EpicProjectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let epic_id = parse_id(&state, MiniProjectId::parse(&epic_id))?;
    Ok(Json(state.applications().read_epic(project_id, epic_id)?))
}

/// Arm a bounded scope of an epic.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics/{epic_id}/execution:arm", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = ArmRequest,
    responses(
        (status = 200, body = AuthorizationProjectionDto),
        (status = 401), (status = 403), (status = 404), (status = 409)
    )
)]
pub async fn arm(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
    headers: HeaderMap,
    request: Result<Json<ArmRequest>, JsonRejection>,
) -> Result<Json<AuthorizationProjectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let Json(request) = request.map_err(|_| {
        state.refuse(
            ApiErrorCode::InvalidRequest,
            "execution:arm requires a JSON body matching the documented budget contract",
        )
    })?;
    let (project_id, epic_id, key) = scope(&state, &project_id, &epic_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .arm(&key, project_id, epic_id, &request)
            .await?,
    ))
}

/// Revoke future admission under one authorization.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics/{epic_id}/execution:disarm", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = DisarmRequest,
    responses(
        (status = 200, body = AuthorizationProjectionDto),
        (status = 401), (status = 403), (status = 404), (status = 409)
    )
)]
pub async fn disarm(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<DisarmRequest>,
) -> Result<Json<AuthorizationProjectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let (project_id, epic_id, key) = scope(&state, &project_id, &epic_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .disarm(&key, project_id, epic_id, &request)
            .await?,
    ))
}

/// What the scheduler would admit right now, and what it would refuse.
///
/// It carries no `Idempotency-Key` because it commits nothing: a dry run has
/// nothing to replay.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics/{epic_id}/scheduler:plan", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic")
    ),
    responses((status = 200, body = SchedulerPlanDto), (status = 401), (status = 403), (status = 404))
)]
pub async fn plan(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
) -> Result<Json<SchedulerPlanDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let epic_id = parse_id(&state, MiniProjectId::parse(&epic_id))?;
    Ok(Json(state.applications().plan(project_id, epic_id).await?))
}

/// Apply a named plan through the existing admission path.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics/{epic_id}/scheduler:start", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = StartRequest,
    responses(
        (status = 200, body = SchedulerStartDto),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "The plan no longer describes this realm"),
        (status = 503, description = "Startup reconciliation has not finished")
    )
)]
pub async fn start(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<StartRequest>,
) -> Result<Json<SchedulerStartDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let (project_id, epic_id, key) = scope(&state, &project_id, &epic_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .start(&key, project_id, epic_id, &request)
            .await?,
    ))
}

/// Move a task or the epic through one legal, evidenced transition.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/epics/{epic_id}/lifecycle", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("epic_id" = String, Path, description = "The epic"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = LifecycleRequest,
    responses(
        (status = 200, body = LifecycleOutcomeDto),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "An illegal transition, a stale revision, or unmet gates")
    )
)]
pub async fn lifecycle(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, epic_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<LifecycleRequest>,
) -> Result<Json<LifecycleOutcomeDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let (project_id, epic_id, key) = scope(&state, &project_id, &epic_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .lifecycle(&key, project_id, epic_id, &request)
            .await?,
    ))
}

/// Settle one run against what its runtime reports right now.
///
/// The request body is empty and stays empty. Kontor loads the run's immutable
/// binding, asks the runtime that issued it for a fresh `inspect`, persists what
/// came back, and only then asks whether that observation is allowed to close the
/// run — a question answered by the binding's *frozen* trust grade, not by
/// anything the caller said. An operator who could post an outcome would be
/// closing a run on their own authority while it looked like the runtime's, which
/// is the one thing a control plane must never let happen quietly.
///
/// Idempotent: a run that is already closed reports its stored closure and no
/// second observation is taken.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/agent-runs/{agent_run_id}/runtime:settle",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("agent_run_id" = String, Path, description = "The run to settle"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    responses(
        (status = 200, body = RuntimeSettlementDto, description = "Settled, or already settled"),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "The binding no longer names a session this runtime will act on"),
        (status = 422, description = "The runtime does not evidence a terminal state"),
        (status = 503, description = "The runtime could not be reached")
    )
)]
pub async fn settle_runtime(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, agent_run_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<RuntimeSettlementDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let agent_run_id = parse_id(&state, AgentRunId::parse(&agent_run_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .settle_runtime(&key, project_id, agent_run_id)
            .await?,
    ))
}

/// Abandon a run that was admitted but never bound, so its task can be
/// scheduled again.
///
/// Refused for a run that *is* bound: that run has a native session, and closing
/// Kontor's row without cancelling it would leave an agent running that nothing
/// is steering. `runtime:settle` is the path for those.
///
/// Idempotent: a run that is already closed reports its stored closure.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/agent-runs/{agent_run_id}/runtime:abandon",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("agent_run_id" = String, Path, description = "The run to abandon"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = AbandonRunRequest,
    responses(
        (status = 200, body = AbandonedRunDto, description = "Abandoned, or already closed"),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "The run moved, or it holds a session that must be settled"),
        (status = 422, description = "The run is bound, so it cannot be abandoned")
    )
)]
pub async fn abandon_run(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, agent_run_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<AbandonRunRequest>,
) -> Result<Json<AbandonedRunDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let agent_run_id = parse_id(&state, AgentRunId::parse(&agent_run_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .abandon_run(&key, project_id, agent_run_id, &request)
            .await?,
    ))
}

/// Parse the two path ids and the idempotency key every task-scoped mutation
/// carries.
fn task_scope(
    state: &ApiState,
    project_id: &str,
    task_id: &str,
    headers: &HeaderMap,
) -> Result<(ProjectId, TaskId, IdempotencyKey), ApiError> {
    let project_id = parse_id(state, ProjectId::parse(project_id))?;
    let task_id = parse_id(state, TaskId::parse(task_id))?;
    let key = idempotency_key(state, headers)?;
    Ok((project_id, task_id, key))
}

/// Resolve one task's Context Pack.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/context:resolve", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = ResolveContextRequest,
    responses(
        (status = 200, body = ResolvedContextDto),
        (status = 401), (status = 403), (status = 404),
        (status = 422, description = "A snapshot was asked for and the task has no live run")
    )
)]
pub async fn resolve_context(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<ResolveContextRequest>,
) -> Result<Json<ResolvedContextDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let (project_id, task_id, key) = task_scope(&state, &project_id, &task_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .resolve_context(&key, project_id, task_id, &request)
            .await?,
    ))
}

/// Append one gate verdict.
///
/// The gate is the addressed resource and `record` is the action, so the action
/// is its own path segment rather than a `:record` suffix on the identifier. That
/// is also the only encoding a router can express — a segment holds one parameter
/// *or* a literal, never both — so the two agree, and no action is smuggled into
/// a query parameter where it would escape the route contract.
///
/// The tier is decided by the *verdict*, not by the route: an ordinary pass or
/// rejection is operator work, and waiving a gate is a decision about whether the
/// rule applies at all, which is admin authority.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/gates/{gate_id}/record",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task"),
        ("gate_id" = String, Path, description = "The gate the pinned profile declares"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = RecordGateRequest,
    responses(
        (status = 200, body = GateVerdictDto),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "A stale revision or a reused key")
    )
)]
pub async fn record_gate(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id, gate_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(request): Json<RecordGateRequest>,
) -> Result<Json<GateVerdictDto>, ApiError> {
    // Waiving is authority-changing, so it is checked here — before the path ids
    // are parsed and before the service is reached.
    let required = if request.verdict == "waived" {
        CallerCapability::Admin
    } else {
        CallerCapability::Operator
    };
    caller.require(&state, required)?;
    let (project_id, task_id, key) = task_scope(&state, &project_id, &task_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .record_gate(&key, project_id, task_id, &gate_id, &request)
            .await?,
    ))
}

/// Correct one task's pinned work profile before a run snapshots it.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/profile-selection",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = SelectionRequest,
    responses(
        (status = 200, body = SelectionDto),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "A run already snapshotted the selection")
    )
)]
pub async fn select_profile(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<SelectionRequest>,
) -> Result<Json<SelectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let (project_id, task_id, key) = task_scope(&state, &project_id, &task_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .select_profile(&key, project_id, task_id, &request)
            .await?,
    ))
}

/// Confirm the team revision a task's pinned profile prescribes.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/team-selection",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = SelectionRequest,
    responses(
        (status = 200, body = SelectionDto),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "The profile pins a different team revision")
    )
)]
pub async fn select_team(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<SelectionRequest>,
) -> Result<Json<SelectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let (project_id, task_id, key) = task_scope(&state, &project_id, &task_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .select_team(&key, project_id, task_id, &request)
            .await?,
    ))
}

/// Correct one task's pinned provider account before a run snapshots it.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/account-selection",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = SelectionRequest,
    responses(
        (status = 200, body = SelectionDto),
        (status = 401), (status = 403), (status = 404),
        (status = 422, description = "The runtime cannot prove a per-run account environment")
    )
)]
pub async fn select_account(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<SelectionRequest>,
) -> Result<Json<SelectionDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let (project_id, task_id, key) = task_scope(&state, &project_id, &task_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .select_account(&key, project_id, task_id, &request)
            .await?,
    ))
}

/// What reconciling one task's external tickets would do.
///
/// A dry run, so it carries no `Idempotency-Key`: there is nothing to replay.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/ticket:reconcile-plan",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task")
    ),
    responses(
        (status = 200, body = TicketReconcilePlanDto),
        (status = 401), (status = 403), (status = 404)
    )
)]
pub async fn ticket_reconcile_plan(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
) -> Result<Json<TicketReconcilePlanDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let task_id = parse_id(&state, TaskId::parse(&task_id))?;
    Ok(Json(
        state
            .applications()
            .ticket_reconcile_plan(project_id, task_id)
            .await?,
    ))
}

/// Apply a named ticket reconciliation plan.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/ticket:reconcile-apply",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = TicketReconcileApplyRequest,
    responses(
        (status = 200, body = TicketReconcileAppliedDto),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "The named plan no longer describes this realm"),
        (status = 503, description = "The connector this realm would converge through is absent")
    )
)]
pub async fn ticket_reconcile_apply(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<TicketReconcileApplyRequest>,
) -> Result<Json<TicketReconcileAppliedDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let (project_id, task_id, key) = task_scope(&state, &project_id, &task_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .ticket_reconcile_apply(&key, project_id, task_id, &request)
            .await?,
    ))
}

/// Settle one bounded Kontor role turn.
///
/// Operator, because it is a decision about Kontor's own work rather than about
/// the fleet. It closes the **turn**, never the run: the seat's native session
/// stays live and reusable, and nothing here is admissible as evidence that the
/// runtime ended anything.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/agent-runs/{agent_run_id}/turns:settle",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("agent_run_id" = String, Path, description = "The seat's agent run"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = SettleTurnRequest,
    responses(
        (status = 200, body = SettledTurnDto, description = "Settled, or replayed"),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "The task moved, or the key settled a different turn")
    )
)]
pub async fn settle_turn(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, agent_run_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<SettleTurnRequest>,
) -> Result<Json<SettledTurnDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let agent_run_id = parse_id(&state, AgentRunId::parse(&agent_run_id))?;
    let key = idempotency_key(&state, &headers)?;
    // The tier the bearer proved, handed on as the settling authority. This is
    // the only identity in this operation the control plane can vouch for.
    Ok(Json(
        state
            .applications()
            .settle_turn(&key, caller.0, project_id, agent_run_id, &request)
            .await?,
    ))
}

/// Reconcile one durable handoff after its run was cancelled by runtime observation.
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/agent-runs/{agent_run_id}/handoffs:attest-late",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("agent_run_id" = String, Path, description = "The terminal agent run"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = AttestLateHandoffRequest,
    responses(
        (status = 200, body = LateHandoffAttestationDto, description = "Attested, or replayed"),
        (status = 400, description = "Invalid artifact key or handoff digest"),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "The task, binding, terminal state, or disposition moved")
    )
)]
pub async fn attest_late_handoff(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, agent_run_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<AttestLateHandoffRequest>,
) -> Result<Json<LateHandoffAttestationDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let agent_run_id = parse_id(&state, AgentRunId::parse(&agent_run_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .attest_late_handoff(&key, caller.0, project_id, agent_run_id, &request)
            .await?,
    ))
}

/// Replace one runtime-terminal unusable seat with a linked successor.
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/agent-runs/{agent_run_id}/successors:replace",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("agent_run_id" = String, Path, description = "The terminal predecessor run"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = ReplaceSeatRequest,
    responses(
        (status = 200, body = ReplacedSeatDto, description = "Created, or replayed"),
        (status = 400, description = "Invalid role slot"),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "The task, binding, run, team, or successor lineage moved"),
        (status = 422, description = "The predecessor is not runtime-terminal and unusable")
    )
)]
pub async fn replace_seat(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, agent_run_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<ReplaceSeatRequest>,
) -> Result<Json<ReplacedSeatDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let agent_run_id = parse_id(&state, AgentRunId::parse(&agent_run_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .replace_seat(&key, project_id, agent_run_id, &request)
            .await?,
    ))
}

/// Excuse one declared role slot that was never bound to a session.
///
/// Admin, because it is the same kind of act as waiving a gate: it discharges an
/// obligation the template imposed, on the template's own terms.
#[utoipa::path(
    post,
    path = "/v1/projects/{project_id}/team-runs/{team_run_id}/role-slots/{role_slot_id}/waivers",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("team_run_id" = String, Path, description = "The team run"),
        ("role_slot_id" = String, Path, description = "The declared slot being excused"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = WaiveRoleSlotRequest,
    responses(
        (status = 200, body = RoleSlotWaiverDto, description = "Recorded, or replayed"),
        (status = 401), (status = 403),
        (status = 404, description = "No such team run, or the template declares no such slot"),
        (status = 409, description = "Stale revision, already accounted for, or ever bound"),
        (status = 422, description = "The slot cannot be waived on this template's terms")
    )
)]
pub async fn waive_role_slot(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, team_run_id, role_slot_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    Json(request): Json<WaiveRoleSlotRequest>,
) -> Result<Json<RoleSlotWaiverDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let team_run_id = parse_id(&state, kontor_core::id::TeamRunId::parse(&team_run_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .waive_role_slot(&key, project_id, team_run_id, &role_slot_id, &request)
            .await?,
    ))
}

/// Register one profile pack alongside the compiled seeds.
///
/// Admin, because it widens what every later `epics:apply` in this Realm may
/// freeze onto a task.
///
/// # Idempotency
///
/// It takes an `Idempotency-Key` like every other write on this surface. A
/// receipt cannot carry it — a receipt is written against a project and a
/// realm-scoped catalogue has none — so the key is bound, once and permanently,
/// to a **fingerprint of this logical operation**: a digest over the operation
/// name, the pack, its revision and its content, canonicalized exactly the way a
/// command intent is.
///
/// Three answers, and no fourth:
///
/// * same key, same fingerprint → the original answer, `unchanged`;
/// * same key, same `(pack_id, version)`, different bytes → `409`;
/// * same key reused for a *different* pack, revision or content → `409`, the
///   key is already bound to another logical operation.
///
/// Binding to a fingerprint rather than to the pack alone is what makes the
/// third case refusable. Content immutability answers "may these bytes be this
/// revision?" and cannot answer "was this key already used for something else?",
/// because two registrations of two different packs are each independently
/// valid and nothing would be comparing them.
#[utoipa::path(
    post, path = "/v1/catalog/packs:register", tag = "applications",
    params(("Idempotency-Key" = String, Header, description = "The caller's stable key")),
    request_body = RegisterPackRequest,
    responses(
        (status = 200, body = ProfilePackDto, description = "Registered, or already registered"),
        (status = 400, description = "The pack document does not validate"),
        (status = 401), (status = 403),
        (status = 409, description = "This revision is registered with different content")
    )
)]
pub async fn register_pack(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
    Json(request): Json<RegisterPackRequest>,
) -> Result<Json<ProfilePackDto>, ApiError> {
    caller.require(&state, CallerCapability::Admin)?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state.applications().register_pack(&key, &request).await?,
    ))
}

/// Every pack this Realm can resolve a category from.
#[utoipa::path(
    get, path = "/v1/catalog/packs", tag = "applications",
    responses((status = 200, body = Vec<ProfilePackDto>), (status = 401), (status = 403))
)]
pub async fn profile_packs(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<Vec<ProfilePackDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    Ok(Json(state.applications().profile_packs()?))
}

/// The whole of one selectable work profile.
#[utoipa::path(
    get, path = "/v1/catalog/work-profiles/{category}", tag = "applications",
    params(("category" = String, Path, description = "The pack category")),
    responses(
        (status = 200, body = WorkProfileDetailDto),
        (status = 401), (status = 403),
        (status = 404, description = "The pack advertises no such category")
    )
)]
pub async fn work_profile(
    State(state): State<ApiState>,
    caller: Caller,
    Path(category): Path<String>,
) -> Result<Json<WorkProfileDetailDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    Ok(Json(state.applications().work_profile(&category)?))
}

/// Re-run the pack's own invariants over one category.
///
/// It reports rather than refuses: a category that does not validate answers
/// `200` saying so, because "this profile is unrunnable" is the finding a caller
/// asked for, not a transport failure.
#[utoipa::path(
    post, path = "/v1/catalog/work-profiles/{category}/validate", tag = "applications",
    params(("category" = String, Path, description = "The pack category")),
    responses(
        (status = 200, body = ProfileValidationDto),
        (status = 401), (status = 403),
        (status = 404, description = "The pack advertises no such category")
    )
)]
pub async fn validate_work_profile(
    State(state): State<ApiState>,
    caller: Caller,
    Path(category): Path<String>,
) -> Result<Json<ProfileValidationDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    Ok(Json(state.applications().validate_work_profile(&category)?))
}

/// One pinned trigger revision.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/triggers/{trigger}/{version}", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("trigger" = String, Path, description = "The trigger"),
        ("version" = u32, Path, description = "The pinned revision")
    ),
    responses(
        (status = 200, body = TriggerSpecDto),
        (status = 401), (status = 403), (status = 404)
    )
)]
pub async fn trigger(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, trigger, version)): Path<(String, String, u32)>,
) -> Result<Json<TriggerSpecDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let version = parse_id(&state, SpecVersion::parse(version))?;
    Ok(Json(
        state
            .applications()
            .trigger(project_id, &trigger, version)?,
    ))
}

/// Evaluate one canonical source event and record the decision.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/intake:submit", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = SubmitIntakeRequest,
    responses(
        (status = 200, body = IntakeReceiptDto, description = "Decided, or the original decision"),
        (status = 401), (status = 403),
        (status = 404, description = "No such trigger revision"),
        (status = 409, description = "The same source identity carries different bytes")
    )
)]
pub async fn submit_intake(
    State(state): State<ApiState>,
    caller: Caller,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SubmitIntakeRequest>,
) -> Result<Json<IntakeReceiptDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let key = idempotency_key(&state, &headers)?;
    Ok(Json(
        state
            .applications()
            .submit_intake(&key, project_id, &request)
            .await?,
    ))
}

/// One recorded intake decision.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/intake/{receipt_id}", tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("receipt_id" = String, Path, description = "The decision")
    ),
    responses(
        (status = 200, body = IntakeReceiptDto),
        (status = 401), (status = 403), (status = 404)
    )
)]
pub async fn intake_receipt(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, receipt_id)): Path<(String, String)>,
) -> Result<Json<IntakeReceiptDto>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    Ok(Json(
        state
            .applications()
            .intake_receipt(project_id, &receipt_id)?,
    ))
}

/// Every ticket field-mapping revision this build can serve for a connector.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/connectors/{connector}/field-specs",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("connector" = String, Path, description = "The connector implementation")
    ),
    responses(
        (status = 200, body = Vec<ConnectorSpecDto>),
        (status = 401), (status = 403), (status = 404)
    )
)]
pub async fn connector_field_specs(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, connector)): Path<(String, String)>,
) -> Result<Json<Vec<ConnectorSpecDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    Ok(Json(
        state
            .applications()
            .connector_field_specs(project_id, &connector)?,
    ))
}

/// Every external-workflow revision this build can serve for a connector.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/connectors/{connector}/workflow-specs",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("connector" = String, Path, description = "The connector implementation")
    ),
    responses(
        (status = 200, body = Vec<ConnectorSpecDto>),
        (status = 401), (status = 403), (status = 404)
    )
)]
pub async fn connector_workflow_specs(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, connector)): Path<(String, String)>,
) -> Result<Json<Vec<ConnectorSpecDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    Ok(Json(
        state
            .applications()
            .connector_workflow_specs(project_id, &connector)?,
    ))
}

/// Every reconciliation conflict recorded against one task's tickets.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/tasks/{task_id}/ticket:conflicts",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task")
    ),
    responses(
        (status = 200, body = Vec<TicketConflictDto>),
        (status = 401), (status = 403), (status = 404)
    )
)]
pub async fn ticket_conflicts(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
) -> Result<Json<Vec<TicketConflictDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let task_id = parse_id(&state, TaskId::parse(&task_id))?;
    Ok(Json(
        state.applications().ticket_conflicts(project_id, task_id)?,
    ))
}

/// Close one reconciliation conflict.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/ticket:resolve-conflict",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    request_body = ResolveConflictRequest,
    responses(
        (status = 200, body = TicketConflictDto),
        (status = 401), (status = 403), (status = 404),
        (status = 409, description = "The conflict is already resolved, or the key was reused")
    )
)]
pub async fn resolve_ticket_conflict(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<ResolveConflictRequest>,
) -> Result<Json<TicketConflictDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let (project_id, task_id, key) = task_scope(&state, &project_id, &task_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .resolve_ticket_conflict(&key, project_id, task_id, &request)
            .await?,
    ))
}

/// Mirror one task's inbound external comments.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/ticket:pull-comments",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    responses(
        (status = 200, body = TicketCommentPullDto),
        (status = 401), (status = 403), (status = 404),
        (status = 503, description = "The connector this realm would pull through is absent")
    )
)]
pub async fn pull_ticket_comments(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<TicketCommentPullDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let (project_id, task_id, key) = task_scope(&state, &project_id, &task_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .pull_ticket_comments(&key, project_id, task_id)
            .await?,
    ))
}

/// The inbound comment revisions one task holds.
#[utoipa::path(
    get, path = "/v1/projects/{project_id}/tasks/{task_id}/ticket:comments",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task")
    ),
    responses(
        (status = 200, body = Vec<TicketCommentDto>),
        (status = 401), (status = 403), (status = 404)
    )
)]
pub async fn ticket_comments(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
) -> Result<Json<Vec<TicketCommentDto>>, ApiError> {
    caller.require(&state, CallerCapability::Observer)?;
    let project_id = parse_id(&state, ProjectId::parse(&project_id))?;
    let task_id = parse_id(&state, TaskId::parse(&task_id))?;
    Ok(Json(
        state.applications().ticket_comments(project_id, task_id)?,
    ))
}

/// Claim one task's external tickets for the principal Kontor acts as.
#[utoipa::path(
    post, path = "/v1/projects/{project_id}/tasks/{task_id}/ticket:claim",
    tag = "applications",
    params(
        ("project_id" = String, Path, description = "The owning project"),
        ("task_id" = String, Path, description = "The task"),
        ("Idempotency-Key" = String, Header, description = "The caller's stable key")
    ),
    responses(
        (status = 200, body = TicketClaimDto),
        (status = 401), (status = 403), (status = 404),
        (status = 503, description = "The connector this realm would claim through is absent")
    )
)]
pub async fn claim_ticket(
    State(state): State<ApiState>,
    caller: Caller,
    Path((project_id, task_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<TicketClaimDto>, ApiError> {
    caller.require(&state, CallerCapability::Operator)?;
    let (project_id, task_id, key) = task_scope(&state, &project_id, &task_id, &headers)?;
    Ok(Json(
        state
            .applications()
            .claim_ticket(&key, project_id, task_id)
            .await?,
    ))
}

/// Parse the two path ids and the idempotency key every epic-scoped mutation
/// carries, in that order.
fn scope(
    state: &ApiState,
    project_id: &str,
    epic_id: &str,
    headers: &HeaderMap,
) -> Result<(ProjectId, MiniProjectId, IdempotencyKey), ApiError> {
    let project_id = parse_id(state, ProjectId::parse(project_id))?;
    let epic_id = parse_id(state, MiniProjectId::parse(epic_id))?;
    let key = idempotency_key(state, headers)?;
    Ok((project_id, epic_id, key))
}

/// The external identifiers a request carries, parsed once.
///
/// It is here rather than in the service so that a malformed connector key or
/// issue key is a transport-level `invalid_request` rather than something the
/// application layer has to spell a second way.
///
/// # Errors
/// Returns [`crate::error::ApiErrorCode::InvalidRequest`] for any value that is
/// not in canonical form.
pub fn parse_ticket_link(
    state: &ApiState,
    request: &TicketLinkRequest,
) -> Result<(kontor_core::id::ConnectorKey, ExternalId), ApiError> {
    let connector = parse_id(
        state,
        kontor_core::id::ConnectorKey::parse(&request.connector),
    )?;
    let issue = parse_id(state, ExternalId::parse(&request.external_issue_key))?;
    Ok((connector, issue))
}
