//! The planning pair invocation coordinator (ASMA-8282 W3a).
//!
//! One API-independent sequence invokes a planning pair:
//! 1. the pinned document and its exact hash;
//! 2. the caller, authorized by its owner **before** any replay;
//! 3. the intent and its replay;
//! 4. the resume of a run this key already froze, or the freeze of a new one;
//! 5. the member launches;
//! 6. the run's advance to `running` under its compare-and-swap;
//! 7. the receipt, classified where it is written.
//!
//! The coordinator owns that order and the decisions between the steps. Its
//! owner ([`InvokeOwner`]) owns every read, the authentication, every store
//! write and receipt, topology and container operations, credentials, the
//! runtime adapter and the answer's rendering. The governed daemon is the only
//! production owner. A fact the owner hands in authorizes nothing by being
//! handed in: the caller is authorized only by
//! [`InvokeOwner::authorize_caller`], and the coordinator never constructs a
//! caller, a credential or a grant.

pub mod commands;

mod freeze;
mod materialize;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use kontor_core::DomainError;
use kontor_core::consultation::{ConsultationFamily, ConsultationRunId, ConsultationRunState};
use kontor_core::id::{
    AggregateRevision, BoundedText, CanonicalDocument, CommandReceiptId, ContentHash, ExternalId,
    ExternalName, IdempotencyKey, MiniProjectId, PlanningPairRunId, ProjectId, RealmId, RoleCode,
    RoleSlotId, SeatBindingId, SpecVersion, TaskId, Timestamp, TopologyKindKey, TopologyNodeId,
};
use kontor_core::planning_pair::{
    PlanningPairMember, PlanningPairMembers, PlanningPairReadbackRefusal, PlanningPairRecord,
    PlanningPairRun, PlanningPairSlot, PlanningPairSpec,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    MiniProject, NewSeatBinding, NewSessionTopologyNode, Project,
    StoredConsultationProfileRevision, StoredConsultationRun, StoredConsultationSeat,
    StoredPlanningPairKnownNative, StoredPlanningPairPlacement, StoredPlanningPairRecord,
};
use kontor_core::spec::{
    CatalogRoleRef, RoleCatalogRevision, TeamDefinitionSpec, TopologySnapshot,
};
use kontor_core::state::{NativeRuntimeIdentity, SeatBinding, SessionTopologyNode};

use super::PlanningPairLaunchContext;
use super::intent::{self as fingerprint, InvokeMember, SeatGeneration};
use crate::adapter::{ConsultationCredential, RuntimeAdapter, RuntimeError};
use crate::container::ContainerBindingSnapshot;
use crate::provenance::{FleetLaunchProvenance, FleetProvenanceObservation};
use crate::scope::ExecutionScope;
use crate::workspace::WorkspaceRoot;

/// Everything one planning pair operation reads, restored through the domain
/// by its owner.
pub struct PairState {
    /// The run.
    pub run: StoredConsultationRun,
    /// The pinned document revision.
    pub revision: StoredConsultationProfileRevision,
    /// The pinned document.
    pub spec: PlanningPairSpec,
    /// The frozen placement.
    pub placement: StoredPlanningPairPlacement,
    /// The latest record's revision.
    pub record_revision: AggregateRevision,
    /// The latest record.
    pub record: PlanningPairRecord,
    /// The pair, restored through the domain transitions.
    pub pair: PlanningPairRun,
    /// The member seats.
    pub seats: Vec<StoredConsultationSeat>,
    /// Each member's known native claim at its current occupancy generation,
    /// when a trusted runtime outcome recorded one. Never a qualification.
    pub known: Vec<StoredPlanningPairKnownNative>,
}

/// What one invocation requests, as its owner parsed it.
#[derive(Debug, Clone, Copy)]
pub struct InvokeInput<'a> {
    /// The pinned document's id.
    pub profile_id: &'a str,
    /// The pinned document's revision.
    pub profile_version: SpecVersion,
    /// The pinned document's hash, as the caller read it.
    pub definition_hash: &'a ContentHash,
    /// The topic.
    pub topic: &'a ExternalName,
    /// The question.
    pub question: &'a BoundedText,
    /// The ticket it is scoped to, if any.
    pub task_id: Option<TaskId>,
    /// The requested member placements, in request order.
    pub members: &'a [InvokeMember<'a>],
    /// The epic revision the caller read.
    pub expected_revision: AggregateRevision,
}

/// Whether this request wrote the receipt it answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// This request wrote it.
    Created,
    /// Another request, or an earlier one under this key, wrote it.
    Unchanged,
}

/// The shared allocator's placement on the owner's one activated snapshot.
#[derive(Debug, Clone)]
pub struct FrozenPlacement<S> {
    /// The allocator's receipt, exactly as it answered.
    pub selection: S,
    /// The canonical hash of the protocol and that receipt.
    pub placement_hash: ContentHash,
    /// The two members, when the allocation placed both.
    pub members: Option<PlanningPairMembers>,
}

/// Why the coordinator refuses. Its owner maps each one to its own refusal;
/// no variant carries wire text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeRefusal {
    /// The pinned document's hash is not the one the caller read.
    DocumentHashDiffers,
    /// A replayed receipt names no durable run.
    ReceiptWithoutRun,
    /// The key was already used for another consultation.
    KeyReused,
    /// The epic moved since the caller read it.
    EpicMoved {
        /// The epic's current revision.
        current: AggregateRevision,
    },
    /// The run is not a planning pair.
    NotPlanningPair,
    /// The epic has no pinned Team Definition.
    NoPinnedTeamDefinition,
    /// The pinned Team Definition has no container of the selected kind.
    NoContainerOfKind,
    /// The selected container is writable.
    ContainerNotReadOnly,
    /// A member slot is not declared exactly once.
    SlotNotDeclaredOnce,
    /// A member slot has the wrong title.
    SlotTitle,
    /// A member slot has the wrong capability profile.
    SlotCapabilityProfile,
    /// The pinned document does not declare a slot's member.
    MemberUndeclared,
    /// The container declares slots beyond its two members.
    ContainerSlotCount,
    /// This scope and topic already has a run.
    SemanticDuplicate {
        /// The existing run.
        existing: ConsultationRunId,
    },
    /// The allocator placed fewer than both members.
    NoCompletePlacement,
    /// The placement receipt is not the shared reader's canonical receipt.
    PlacementReceiptNotCanonical,
    /// The frozen context could not be decoded.
    ContextUndecodable,
    /// The run's frozen topology node is missing.
    NodeMissing,
    /// A frozen seat is absent from the pinned document.
    SeatAbsentFromDocument,
    /// A member seat has no persistent topology binding.
    SeatUnbound,
    /// The frozen placement could not be decoded.
    PlacementUndecodable,
    /// A member's known native session is kept and is not qualified.
    MemberKeptUnqualified {
        /// The kept session.
        identity: NativeRuntimeIdentity,
        /// Why its readback did not qualify it, when one says why.
        refusal: Option<PlanningPairReadbackRefusal>,
    },
}

/// The owner of one planning pair invocation: every read, authorization,
/// write, receipt, topology and container operation and runtime the
/// coordinator's sequence needs, and nothing else.
#[async_trait::async_trait]
pub trait InvokeOwner: Sync {
    /// The owner's refusal.
    type Error: Send;
    /// The owner's native-activity guard, held for the whole invocation.
    type Activity: Send;
    /// The caller exactly as the owner's trusted boundary authenticated it.
    type Caller: Copy + Send + Sync;
    /// The allocator's receipt.
    type Selection: serde::Serialize + Send + Sync;
    /// The owner's answer.
    type Output: Send;

    /// Answer one of the coordinator's typed refusals.
    fn refuse(&self, refusal: InvokeRefusal) -> Self::Error;
    /// Answer a domain error.
    fn domain_error(&self, error: &DomainError) -> Self::Error;
    /// Answer a runtime error.
    fn runtime_error(&self, error: &RuntimeError) -> Self::Error;
    /// The owner's clock.
    fn now(&self) -> Timestamp;
    /// The Realm the owner's state belongs to.
    fn realm_id(&self) -> Result<RealmId, Self::Error>;

    /// Take the native-activity guard.
    fn begin_native_activity(&self) -> Result<Self::Activity, Self::Error>;
    /// The published document revision.
    fn pinned_document(
        &self,
        project_id: ProjectId,
        profile_id: &str,
        version: SpecVersion,
    ) -> Result<(StoredConsultationProfileRevision, PlanningPairSpec), Self::Error>;
    /// Authorize the authenticated caller at its current generation, against
    /// the document's roles and scopes; the caller's seat.
    fn authorize_caller(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        task_id: Option<TaskId>,
        spec: &PlanningPairSpec,
        caller: Self::Caller,
    ) -> Result<SeatBinding, Self::Error>;
    /// The authenticated caller's seat and generation, as its fingerprint
    /// names them.
    fn presented(&self, caller: Self::Caller) -> SeatGeneration;
    /// Canonicalize one document.
    fn canonical(&self, document: &serde_json::Value) -> Result<CanonicalDocument, Self::Error>;
    /// The receipt an exact replay answers with, if this key has one.
    fn replayed(
        &self,
        key: &IdempotencyKey,
        intent: &CanonicalDocument,
        target: &AggregateRef,
    ) -> Result<Option<CommandReceiptId>, Self::Error>;
    /// The run this key froze, if any.
    fn run_by_key(
        &self,
        project_id: ProjectId,
        key: &IdempotencyKey,
    ) -> Result<Option<StoredConsultationRun>, Self::Error>;
    /// The epic, as read now.
    fn epic(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<MiniProject, Self::Error>;
    /// Restore one run.
    fn pair_state(&self, run: &StoredConsultationRun) -> Result<PairState, Self::Error>;
    /// Advance a materializing run to `running` under its compare-and-swap.
    fn advance_to_running(
        &self,
        frozen: &StoredConsultationRun,
    ) -> Result<StoredConsultationRun, Self::Error>;
    /// The run, as stored now.
    fn stored_run(
        &self,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
    ) -> Result<StoredConsultationRun, Self::Error>;
    /// Wait at an installed receipt hold; with none, return at once.
    async fn receipt_hold_point(&self);
    /// Record the receipt, classified where it is written: its id, and whether
    /// this write inserted it.
    fn record(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        kind: CommandKind,
        target: AggregateRef,
        revision: AggregateRevision,
        intent: &CanonicalDocument,
    ) -> Result<(CommandReceiptId, bool), Self::Error>;
    /// Render the caller's answer for one run.
    fn render(
        &self,
        pair: &PairState,
        receipt: CommandReceiptId,
        applied: Applied,
    ) -> Result<Self::Output, Self::Error>;

    /// The epic's pinned Team Definition, if any.
    fn pinned_team_definition(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<Option<TeamDefinitionSpec>, Self::Error>;
    /// The role catalog the epic selected, exactly as persisted.
    fn epic_role_catalog(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<RoleCatalogRevision, Self::Error>;
    /// One member's role, resolved in that catalog.
    fn member_catalog_role(
        &self,
        catalog: &RoleCatalogRevision,
        role_code: &RoleCode,
    ) -> Result<CatalogRoleRef, Self::Error>;
    /// The consultation's shared semantic identity.
    #[allow(
        clippy::too_many_arguments,
        reason = "every identity input is passed explicitly"
    )]
    fn semantic_identity(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        task_id: Option<TaskId>,
        family: ConsultationFamily,
        kind: &TopologyKindKey,
        revision: &StoredConsultationProfileRevision,
        definition: &TeamDefinitionSpec,
        topic: &ExternalName,
    ) -> Result<ContentHash, Self::Error>;
    /// The run already holding a semantic identity, if any.
    fn run_by_semantic_identity(
        &self,
        project_id: ProjectId,
        identity: &ContentHash,
    ) -> Result<Option<StoredConsultationRun>, Self::Error>;
    /// Place both members with the shared allocator on the one activated
    /// snapshot.
    fn place(
        &self,
        members: &[InvokeMember<'_>],
    ) -> Result<FrozenPlacement<Self::Selection>, Self::Error>;
    /// The runtime members are placed on.
    fn runtime(&self) -> Result<Arc<dyn RuntimeAdapter>, Self::Error>;
    /// Refuse, before any native effect, a member route the runtime cannot
    /// compose the closed member surface for.
    fn require_member_routes(
        &self,
        adapter: &dyn RuntimeAdapter,
        members: &[PlanningPairMember],
    ) -> Result<(), Self::Error>;
    /// The project's topology.
    fn project_topology(&self, project_id: ProjectId) -> Result<TopologySnapshot, Self::Error>;
    /// The epic's topology node, ensured.
    fn ensure_epic_node(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<SessionTopologyNode, Self::Error>;
    /// The attach deadline for a seat bound now.
    fn attach_deadline(&self, now: Timestamp) -> Timestamp;
    /// Persist the frozen run, its node, seats, placement and first record in
    /// one write.
    fn create_run(
        &self,
        run: &StoredConsultationRun,
        node: &NewSessionTopologyNode,
        seats: &[(&StoredConsultationSeat, &NewSeatBinding)],
        placement: &StoredPlanningPairPlacement,
        record: &StoredPlanningPairRecord,
    ) -> Result<(), Self::Error>;

    /// The project.
    fn project(&self, project_id: ProjectId) -> Result<Project, Self::Error>;
    /// One topology node, if it exists.
    fn topology_node(
        &self,
        project_id: ProjectId,
        node_id: TopologyNodeId,
    ) -> Result<Option<SessionTopologyNode>, Self::Error>;
    /// The epic's confirmed tracker key, if any.
    fn epic_tracker_key(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<Option<kontor_core::branch::TrackerKey>, Self::Error>;
    /// The members' working directory.
    fn consultation_root(
        &self,
        project_root: &str,
        epic_key: Option<&kontor_core::branch::TrackerKey>,
        node_id: TopologyNodeId,
    ) -> Result<WorkspaceRoot, Self::Error>;
    /// One persistent seat binding, if it exists.
    fn seat_binding(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
    ) -> Result<Option<SeatBinding>, Self::Error>;
    /// Refuse a frozen member role that no longer corresponds to the
    /// member's explicit code.
    fn require_member_role(
        &self,
        catalog: &RoleCatalogRevision,
        role: &CatalogRoleRef,
        role_code: &RoleCode,
    ) -> Result<(), Self::Error>;
    /// The node's native container, ensured.
    async fn ensure_container(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        cwd: &WorkspaceRoot,
        adapter: &dyn RuntimeAdapter,
    ) -> Result<ContainerBindingSnapshot, Self::Error>;
    /// The execution scope the members run in.
    fn execution_scope(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        task_id: Option<TaskId>,
        adapter: &dyn RuntimeAdapter,
    ) -> Result<ExecutionScope, Self::Error>;
    /// A member's scoped credential at its frozen generation.
    fn member_credential(
        &self,
        seat_binding_id: SeatBindingId,
        occupancy_generation: u64,
    ) -> Result<ConsultationCredential, Self::Error>;
    /// A member seat's native display name.
    fn seat_name(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        scope: &ExecutionScope,
        role_code: &RoleCode,
        role_slot_id: Option<&RoleSlotId>,
    ) -> Result<ExternalName, Self::Error>;
    /// A member's frozen launch context, derived from durable state.
    #[allow(
        clippy::too_many_arguments,
        reason = "every frozen input the context is derived from is passed explicitly"
    )]
    fn launch_context(
        &self,
        run: &StoredConsultationRun,
        pair: &PairState,
        seat: &StoredConsultationSeat,
        slot: PlanningPairSlot,
        node: &SessionTopologyNode,
        cwd: &WorkspaceRoot,
        catalog: &RoleCatalogRevision,
        requested: Option<&FleetLaunchProvenance>,
    ) -> Result<PlanningPairLaunchContext, Self::Error>;
    /// Record what one member launch requested and observed.
    fn record_launch_provenance(
        &self,
        seat_binding_id: SeatBindingId,
        native_id: &ExternalId,
        requested: Option<&FleetLaunchProvenance>,
        observed: &FleetProvenanceObservation,
    );
    /// Keep one member's launched session as its known native claim, bound
    /// in the same write when it qualified.
    fn record_member_launch(
        &self,
        claim: &StoredPlanningPairKnownNative,
    ) -> Result<(), Self::Error>;
    /// Observe one member seat attached and running.
    fn observe_member_attached(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        observed_at: Timestamp,
    ) -> Result<(), Self::Error>;
}

/// Invoke one planning pair, as its owner's authenticated caller.
///
/// # Errors
/// Every refusal the sequence reaches, in its order.
pub async fn invoke<O: InvokeOwner>(
    owner: &O,
    key: &IdempotencyKey,
    project_id: ProjectId,
    epic_id: MiniProjectId,
    caller: O::Caller,
    input: &InvokeInput<'_>,
) -> Result<O::Output, O::Error> {
    let _native_activity = owner.begin_native_activity()?;
    // The pinned document first: its caller roles and scopes decide who may
    // invoke it, and its exact hash must be the one the caller read.
    let (revision, spec) =
        owner.pinned_document(project_id, input.profile_id, input.profile_version)?;
    if revision.definition_hash != *input.definition_hash {
        return Err(owner.refuse(InvokeRefusal::DocumentHashDiffers));
    }
    // Authentication before any replay: a retired caller credential never
    // regains authority by repeating a key it once used.
    let caller_seat = owner.authorize_caller(project_id, epic_id, input.task_id, &spec, caller)?;
    let intent = owner.canonical(
        &fingerprint::Invoke {
            project_id,
            epic_id,
            profile_id: input.profile_id,
            profile_version: input.profile_version,
            definition_hash: input.definition_hash,
            topic: input.topic,
            question: input.question,
            task_id: input.task_id,
            members: input.members,
            caller: owner.presented(caller),
        }
        .document(),
    )?;
    let target = AggregateRef::MiniProject {
        mini_project_id: epic_id,
    };
    if let Some(receipt) = owner.replayed(key, &intent, &target)? {
        let run = owner
            .run_by_key(project_id, key)?
            .ok_or_else(|| owner.refuse(InvokeRefusal::ReceiptWithoutRun))?;
        let pair = owner.pair_state(&run)?;
        return owner.render(&pair, receipt, Applied::Unchanged);
    }
    let run = if let Some(existing) = owner.run_by_key(project_id, key)? {
        if existing.id.family() != ConsultationFamily::PlanningPair
            || existing.mini_project_id != epic_id
            || existing.invoke_intent_hash != *intent.hash()
        {
            return Err(owner.refuse(InvokeRefusal::KeyReused));
        }
        existing
    } else {
        let epic = owner.epic(project_id, epic_id)?;
        if epic.revision != input.expected_revision {
            return Err(owner.refuse(InvokeRefusal::EpicMoved {
                current: epic.revision,
            }));
        }
        freeze::freeze(
            owner,
            key,
            &intent,
            project_id,
            epic_id,
            input,
            &revision,
            &spec,
            &caller_seat,
        )?
    };
    materialize::materialize(owner, &run).await?;
    let run = running(owner, &run)?;
    owner.receipt_hold_point().await;
    // A request for this key that resumed the run may record alongside this
    // one. Whether this request wrote the receipt is decided where the receipt
    // is written, so exactly one of them answers `created`.
    let (receipt_id, inserted) = owner.record(
        key,
        project_id,
        CommandKind::InvokePlanningPairRun,
        target,
        run.revision,
        &intent,
    )?;
    let applied = if inserted {
        Applied::Created
    } else {
        Applied::Unchanged
    };
    let pair = owner.pair_state(&run)?;
    owner.render(&pair, receipt_id, applied)
}

/// The run once its members are launched, advanced to `running`.
///
/// Another request for the same key may have resumed the run and launched
/// with this one. The run's compare-and-swap admits exactly one advance from
/// the frozen revision. A request that loses it answers with the row the
/// winner wrote; only a run still materializing after that is a refusal.
fn running<O: InvokeOwner>(
    owner: &O,
    frozen: &StoredConsultationRun,
) -> Result<StoredConsultationRun, O::Error> {
    if frozen.state != ConsultationRunState::Materializing {
        return Ok(frozen.clone());
    }
    let ConsultationRunId::PlanningPair(run_id) = frozen.id else {
        return Err(owner.refuse(InvokeRefusal::NotPlanningPair));
    };
    match owner.advance_to_running(frozen) {
        Ok(advanced) => Ok(advanced),
        Err(error) => {
            let after = owner.stored_run(frozen.project_id, run_id)?;
            if after.state == ConsultationRunState::Materializing {
                Err(error)
            } else {
                Ok(after)
            }
        }
    }
}
