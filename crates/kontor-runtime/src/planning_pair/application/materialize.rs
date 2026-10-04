//! Launch a frozen pair's members through the runtime port (ASMA-8282 W3a).
//!
//! Each member launches with its own scoped credential, its frozen route and
//! the read-only member surface. A runtime that does not compose the planning
//! pair member surface refuses here, before any native effect, and the run
//! stays materializing.
//!
//! A member whose launch was already read back keeps that exact session. A
//! replay or a restart meets it again from its durable claim, with no runtime
//! call and no second create; only the caller's recovery can requalify it.

use kontor_core::id::{BoundedText, SCHEMA_VERSION};
use kontor_core::planning_pair::{PlanningPairSlot, PlanningPairSpec};
use kontor_core::repository::{
    StoredConsultationRun, StoredConsultationSeat, StoredPlanningPairKnownNative,
};
use kontor_core::spec::ContextPolicySnapshot;

use super::super::context as member_context;
use super::super::qualify_member_readback;
use super::{InvokeOwner, InvokeRefusal};
use crate::adapter::{ConsultationLaunchRequest, ConsultationRouteProvenance};
use crate::capability::RuntimeCapability;

/// Launch every member of `run` that is not yet bound.
#[allow(
    clippy::too_many_lines,
    reason = "one ordered launch sequence; splitting it would hide the order"
)]
pub(super) async fn materialize<O: InvokeOwner>(
    owner: &O,
    run: &StoredConsultationRun,
) -> Result<(), O::Error> {
    let pair = owner.pair_state(run)?;
    let project = owner.project(run.project_id)?;
    let node = owner
        .topology_node(run.project_id, run.topology_node_id)?
        .ok_or_else(|| owner.refuse(InvokeRefusal::NodeMissing))?;
    let adapter = owner.runtime()?;
    owner.require_member_routes(adapter.as_ref(), pair.pair.members().members())?;
    let epic_key = owner.epic_tracker_key(run.project_id, run.mini_project_id)?;
    let cwd = owner.consultation_root(project.root_path.as_str(), epic_key.as_ref(), node.id)?;
    let mut seats = pair.seats.clone();
    if seats.iter().all(|seat| seat.native_identity.is_some()) {
        return Ok(());
    }
    // Every frozen member role is proved again against the epic's selected
    // catalog before any native effect, so a stored role that no longer
    // corresponds to the member's explicit code never launches.
    let catalog = owner.epic_role_catalog(run.project_id, run.mini_project_id)?;
    for seat in &seats {
        let slot = PlanningPairSlot::parse(seat.role_slot_id.as_str())
            .map_err(|error| owner.domain_error(&error))?;
        let member = member_of(owner, &pair.spec, slot)?;
        let binding = owner
            .seat_binding(run.project_id, seat.seat_binding_id)?
            .ok_or_else(|| owner.refuse(InvokeRefusal::SeatUnbound))?;
        owner.require_member_role(&catalog, &binding.role, &member.role_code)?;
    }
    let container = owner
        .ensure_container(run.project_id, &node, &cwd, adapter.as_ref())
        .await?;
    let scope = owner.execution_scope(
        run.project_id,
        run.mini_project_id,
        node.task_id,
        adapter.as_ref(),
    )?;
    let capabilities = adapter
        .discover_capabilities()
        .await
        .map_err(|error| owner.runtime_error(&error))?;
    let context_policy = ContextPolicySnapshot::standard(
        &capabilities.limits.context_window,
        capabilities.supports(RuntimeCapability::ContextPolicy),
        SCHEMA_VERSION,
        owner.now(),
    )
    .map_err(|error| owner.domain_error(&error))?;
    let receipt: serde_json::Value = serde_json::from_str(pair.placement.placement.json())
        .map_err(|_| owner.refuse(InvokeRefusal::PlacementUndecodable))?;
    for seat in &mut seats {
        if seat.native_identity.is_some() {
            continue;
        }
        // A member whose launch was already read back keeps that exact
        // session: a replay or a restart meets it again from its durable
        // claim, with no runtime call and no second create. Only the caller's
        // recovery can requalify it.
        if let Some(known) = pair
            .known
            .iter()
            .find(|known| known.seat_binding_id == seat.seat_binding_id)
        {
            return Err(owner.refuse(InvokeRefusal::MemberKeptUnqualified {
                identity: known.identity.clone(),
                refusal: known.readback_refusal,
            }));
        }
        let slot = PlanningPairSlot::parse(seat.role_slot_id.as_str())
            .map_err(|error| owner.domain_error(&error))?;
        let member = member_of(owner, &pair.spec, slot)?;
        let route_provenance = ConsultationRouteProvenance::fleet_configuration(
            pair.placement.placement.hash().clone(),
        );
        let fleet_provenance = member_context::requested_fleet_provenance(&receipt, slot);
        adapter
            .validate_consultation_model_rung(&seat.model_rung, &route_provenance)
            .map_err(|error| owner.runtime_error(&error))?;
        let credential =
            owner.member_credential(seat.seat_binding_id, seat.occupancy_generation)?;
        let prompt = BoundedText::parse(&member_prompt(slot, &pair.spec, member, run, seat))
            .map_err(|error| owner.domain_error(&error))?;
        let topology_seat = owner
            .seat_binding(run.project_id, seat.seat_binding_id)?
            .ok_or_else(|| owner.refuse(InvokeRefusal::SeatUnbound))?;
        let display_name = owner.seat_name(
            run.project_id,
            &node,
            &scope,
            &topology_seat.role.role_code,
            Some(&topology_seat.role_slot_id),
        )?;
        let context = owner.launch_context(
            run,
            &pair,
            seat,
            slot,
            &node,
            &cwd,
            &catalog,
            fleet_provenance.as_ref(),
        )?;
        let context_hash = context
            .frozen_hash()
            .map_err(|error| owner.domain_error(&error))?;
        let outcome = adapter
            .launch_consultation(&ConsultationLaunchRequest {
                scope: scope.clone(),
                run_id: run.id,
                seat_binding_id: seat.seat_binding_id,
                role_slot_id: seat.role_slot_id.clone(),
                display_name,
                container: container.clone(),
                cwd: cwd.clone(),
                prompt,
                credential,
                model_rung: seat.model_rung.clone(),
                route_provenance,
                fleet_provenance: fleet_provenance.clone(),
                context_policy: context_policy.clone(),
                requested_at: owner.now(),
                planning_pair: Some(context),
            })
            .await
            .map_err(|error| owner.runtime_error(&error))?;
        owner.record_launch_provenance(
            seat.seat_binding_id,
            &outcome.identity.native_id,
            fleet_provenance.as_ref(),
            &outcome.fleet_provenance,
        );
        // A member is qualified — bound, so it can contribute — only when its
        // launch reported a member-surface observation, every mandatory field
        // of it matched, and its readback observed exactly its requested
        // provenance. Whatever the readback proved, the exact session it named
        // is kept first as the member's known native claim, in the same
        // transaction as the bind when it qualified. Any missing, wrong or
        // unsupported field is then a typed no-observation refusal: the
        // session is kept, unbound and named, its credential is unqualified,
        // the pair stays materializing with no receipt, and a replay meets that
        // same claim rather than creating, replacing or destroying a session.
        let refusal = qualify_member_readback(&outcome, fleet_provenance.as_ref()).err();
        let claim = StoredPlanningPairKnownNative {
            run_id: run.id,
            project_id: run.project_id,
            seat_binding_id: seat.seat_binding_id,
            occupancy_generation: seat.occupancy_generation,
            identity: outcome.identity.clone(),
            provider_session_id: outcome.provider_session_id.clone(),
            context_hash,
            placement_hash: pair.placement.placement.hash().clone(),
            readback_refusal: refusal,
            observed_at: outcome.observed_at,
        };
        owner.record_member_launch(&claim)?;
        if let Some(refusal) = refusal {
            return Err(owner.refuse(InvokeRefusal::MemberKeptUnqualified {
                identity: outcome.identity,
                refusal: Some(refusal),
            }));
        }
        seat.native_identity = Some(outcome.identity);
        seat.provider_session_id = outcome.provider_session_id;
        seat.observed_at = Some(outcome.observed_at);
        owner.observe_member_attached(run.project_id, seat.seat_binding_id, outcome.observed_at)?;
    }
    Ok(())
}

/// The pinned document's member for one frozen slot.
fn member_of<'a, O: InvokeOwner>(
    owner: &O,
    spec: &'a PlanningPairSpec,
    slot: PlanningPairSlot,
) -> Result<&'a kontor_core::planning_pair::PlanningPairMemberSpec, O::Error> {
    spec.members
        .iter()
        .find(|member| member.slot == slot)
        .ok_or_else(|| owner.refuse(InvokeRefusal::SeatAbsentFromDocument))
}

/// The read-only member's first turn.
fn member_prompt(
    slot: PlanningPairSlot,
    spec: &PlanningPairSpec,
    member: &kontor_core::planning_pair::PlanningPairMemberSpec,
    run: &StoredConsultationRun,
    seat: &StoredConsultationSeat,
) -> String {
    format!(
        "Read-only planning pair member {label}. You may inspect evidence but must not \
         mutate code, Jira, topology, scheduling, or runtime state, and you cannot see \
         the other member's finding until both are recorded. Charter: {charter} \
         Specialty: {specialty} Role instructions: {behavior} Question: {question} \
         Record exactly one finding with kontor_planning_pair_findings_record and, only \
         if the caller asks one, one answer with kontor_planning_pair_answer_record. Your \
         scoped Kontor tools inherit authentication automatically. Context: project_id \
         {project}, planning_pair_run_id {run_id}, expected_revision {revision}, \
         seat_binding_id {seat_binding}. Never disclose credentials.",
        label = slot.label(),
        charter = spec.charter.as_str(),
        specialty = member.specialty.as_str(),
        behavior = member.behavior.as_str(),
        question = run.question.as_str(),
        project = run.project_id,
        run_id = run.id.as_text(),
        revision = run.revision.get(),
        seat_binding = seat.seat_binding_id,
    )
}
