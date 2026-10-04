//! Freeze one new planning pair before any native effect (ASMA-8282 W3a).
//!
//! In order:
//! 1. the run in the shared registry, under its shared semantic identity;
//! 2. its container node and two member seats, from the pinned Team Definition;
//! 3. the shared allocator's placement on one activated snapshot;
//! 4. the first record.
//!
//! The owner reads, places and writes; nothing here touches a store or a
//! runtime.

use kontor_core::consultation::{
    ConsultationFamily, ConsultationRunId, ConsultationRunState, ConsultationSubject,
};
use kontor_core::id::{
    AggregateRevision, CanonicalDocument, ContentHash, IdempotencyKey, MiniProjectId,
    PlanningPairRunId, ProjectId, RoleKey, RoleSlotId, SeatBindingId, TopologyNodeId,
};
use kontor_core::planning_pair::{
    ConsultationProtocol, PlanningPairRecord, PlanningPairRun, PlanningPairSlot, PlanningPairSpec,
};
use kontor_core::repository::{
    NewSeatBinding, NewSessionTopologyNode, StoredConsultationProfileRevision,
    StoredConsultationRun, StoredConsultationSeat, StoredPlanningPairPlacement,
    StoredPlanningPairRecord,
};
use kontor_core::spec::TeamDefinitionSnapshot;
use kontor_core::state::SeatBinding;

use super::{InvokeInput, InvokeOwner, InvokeRefusal};

/// The logical role every planning pair member seat is held under, as
/// `advisor` is for an Advisor seat.
const MEMBER_LOGICAL_ROLE: &str = "planning_pair_member";

/// The capability profile the pinned Team Definition must declare for both
/// member slots: the closed member surface (run get, finding, answer).
const MEMBER_CAPABILITY_PROFILE: &str = "planning_pair_member";

/// Freeze one new planning pair; the frozen, still materializing run.
#[allow(
    clippy::too_many_arguments,
    reason = "every frozen authority input is passed explicitly"
)]
#[allow(
    clippy::too_many_lines,
    reason = "one ordered freeze; splitting it would hide the order"
)]
pub(super) fn freeze<O: InvokeOwner>(
    owner: &O,
    key: &IdempotencyKey,
    intent: &CanonicalDocument,
    project_id: ProjectId,
    epic_id: MiniProjectId,
    request: &InvokeInput<'_>,
    revision: &StoredConsultationProfileRevision,
    spec: &PlanningPairSpec,
    caller: &SeatBinding,
) -> Result<StoredConsultationRun, O::Error> {
    let definition = owner
        .pinned_team_definition(project_id, epic_id)?
        .ok_or_else(|| owner.refuse(InvokeRefusal::NoPinnedTeamDefinition))?;
    let container = definition
        .container(&spec.container_kind)
        .ok_or_else(|| owner.refuse(InvokeRefusal::NoContainerOfKind))?;
    if !container.read_only {
        return Err(owner.refuse(InvokeRefusal::ContainerNotReadOnly));
    }
    let catalog = owner.epic_role_catalog(project_id, epic_id)?;
    let mut slot_roles = Vec::with_capacity(2);
    for slot in PlanningPairSlot::ALL {
        let declared = container
            .slots
            .iter()
            .filter(|declared| declared.slot_id.as_str() == slot.as_str())
            .collect::<Vec<_>>();
        let [declared] = declared.as_slice() else {
            return Err(owner.refuse(InvokeRefusal::SlotNotDeclaredOnce));
        };
        if declared
            .display_name
            .as_ref()
            .map(kontor_core::id::ExternalName::as_str)
            != Some(slot.label())
        {
            return Err(owner.refuse(InvokeRefusal::SlotTitle));
        }
        if declared.capability_profile.as_str() != MEMBER_CAPABILITY_PROFILE {
            return Err(owner.refuse(InvokeRefusal::SlotCapabilityProfile));
        }
        // The title is the Team Definition's; the registered role is the
        // pinned document's own explicit member role, since a display-named
        // slot carries none, resolved only in the catalog the epic itself
        // selected.
        let member = spec
            .members
            .iter()
            .find(|member| member.slot == *slot)
            .ok_or_else(|| owner.refuse(InvokeRefusal::MemberUndeclared))?;
        slot_roles.push(owner.member_catalog_role(&catalog, &member.role_code)?);
    }
    if container.slots.len() != 2 {
        return Err(owner.refuse(InvokeRefusal::ContainerSlotCount));
    }
    let semantic_identity_hash = owner.semantic_identity(
        project_id,
        epic_id,
        request.task_id,
        ConsultationFamily::PlanningPair,
        &spec.container_kind,
        revision,
        &definition,
        request.topic,
    )?;
    if let Some(existing) = owner.run_by_semantic_identity(project_id, &semantic_identity_hash)? {
        return Err(owner.refuse(InvokeRefusal::SemanticDuplicate {
            existing: existing.id,
        }));
    }
    // One activated snapshot, the shared allocator, distinct actual vendors.
    // A blocked placement freezes nothing.
    let placement = owner.place(request.members)?;
    let Some(members) = placement.members.clone() else {
        return Err(owner.refuse(InvokeRefusal::NoCompletePlacement));
    };
    // The two actual placed routes must each have the closed member surface
    // on this runtime before anything is frozen.
    owner.require_member_routes(owner.runtime()?.as_ref(), members.members())?;
    let placement_document = owner.canonical(&serde_json::json!({
        "schema_version": 1,
        "protocol": ConsultationProtocol::PlanningPair.as_str(),
        "selection": placement.selection,
    }))?;
    if placement_document.hash() != &placement.placement_hash {
        return Err(owner.refuse(InvokeRefusal::PlacementReceiptNotCanonical));
    }
    let pair = PlanningPairRun::admit(spec, members, request.question.clone())
        .map_err(|error| owner.domain_error(&error))?;
    let record = PlanningPairRecord::admitted(&pair);
    let topology = owner.project_topology(project_id)?;
    let epic_node = owner.ensure_epic_node(project_id, epic_id)?;
    let now = owner.now();
    let run_id = ConsultationRunId::PlanningPair(PlanningPairRunId::generate());
    let node_id = TopologyNodeId::generate();
    let definition_snapshot = TeamDefinitionSnapshot::from_revision(&definition)
        .map_err(|error| owner.domain_error(&error))?;
    let question_hash = ContentHash::of(request.question.as_str().as_bytes());
    let context = owner.canonical(&serde_json::json!({
        "schema_version": 1,
        "realm_id": owner.realm_id()?.to_string(),
        "project_id": project_id.to_string(),
        "epic_id": epic_id.to_string(),
        "task_id": request.task_id.map(|id| id.to_string()),
        "protocol": ConsultationProtocol::PlanningPair.as_str(),
        "caller_seat_binding_id": caller.id.to_string(),
        "caller_role_slot": caller.role_slot_id.as_str(),
        "profile_id": revision.profile_id,
        "profile_version": revision.version.get(),
        "profile_hash": revision.definition_hash.as_str(),
        "container_kind": spec.container_kind.as_str(),
        "team_definition_id": definition_snapshot.definition_id.to_string(),
        "team_definition_version": definition_snapshot.version.get(),
        "team_definition_hash": definition_snapshot.canonical_hash.as_str(),
        "topic": request.topic.as_str(),
        "question_hash": question_hash.as_str(),
        "placement_hash": placement.placement_hash.as_str(),
    }))?;
    let run = StoredConsultationRun {
        id: run_id,
        project_id,
        mini_project_id: epic_id,
        profile_id: revision.profile_id.clone(),
        profile_version: revision.version,
        definition_hash: revision.definition_hash.clone(),
        semantic_identity_hash: Some(semantic_identity_hash),
        subject: Some(ConsultationSubject::from_requested_task(request.task_id)),
        topic: Some(request.topic.clone()),
        question: request.question.clone(),
        question_hash,
        context: serde_json::from_str(context.json())
            .map_err(|_| owner.refuse(InvokeRefusal::ContextUndecodable))?,
        context_hash: context.hash().clone(),
        caller_seat_binding_id: caller.id,
        topology_node_id: node_id,
        invoke_key: key.clone(),
        invoke_intent_hash: intent.hash().clone(),
        state: ConsultationRunState::Materializing,
        round: 1,
        result: None,
        result_hash: None,
        revision: AggregateRevision::INITIAL,
        created_at: now,
        updated_at: now,
        settled_at: None,
    };
    let node = NewSessionTopologyNode {
        id: node_id,
        project_id,
        mini_project_id: Some(epic_id),
        topology,
        kind: spec.container_kind.clone(),
        parent_id: Some(epic_node.id),
        task_id: request.task_id,
        created_at: now,
    };
    let deadline = owner.attach_deadline(now);
    let logical_role =
        RoleKey::parse(MEMBER_LOGICAL_ROLE).map_err(|error| owner.domain_error(&error))?;
    let mut seats = Vec::with_capacity(2);
    let mut bindings = Vec::with_capacity(2);
    for (member, role) in pair.members().members().iter().zip(slot_roles) {
        let seat_binding_id = SeatBindingId::generate();
        let role_slot_id =
            RoleSlotId::parse(member.slot.as_str()).map_err(|error| owner.domain_error(&error))?;
        seats.push(StoredConsultationSeat {
            run_id,
            role_slot_id: role_slot_id.clone(),
            committee_role: None,
            logical_role: logical_role.clone(),
            seat_binding_id,
            model_rung: member.route.clone(),
            occupancy_generation: 1,
            native_identity: None,
            provider_session_id: None,
            observed_at: None,
        });
        bindings.push(NewSeatBinding {
            id: seat_binding_id,
            project_id,
            topology_node_id: node_id,
            role_slot_id,
            role,
            task_id: request.task_id,
            team_run_id: None,
            attach_deadline: deadline,
            parent_seat_binding_id: Some(caller.id),
            created_at: now,
        });
    }
    let record_document = record
        .canonicalize()
        .map_err(|error| owner.domain_error(&error))?;
    let pairs: Vec<_> = seats.iter().zip(bindings.iter()).collect();
    owner.create_run(
        &run,
        &node,
        &pairs,
        &StoredPlanningPairPlacement {
            run_id,
            project_id,
            placement: placement_document.clone(),
            created_at: now,
        },
        &StoredPlanningPairRecord {
            run_id,
            project_id,
            revision: AggregateRevision::INITIAL,
            phase: pair.state(),
            record: record_document.clone(),
            created_at: now,
        },
    )?;
    Ok(run)
}
