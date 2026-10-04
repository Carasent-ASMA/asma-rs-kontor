//! The governed daemon as the only production owner of the shared planning
//! pair invocation coordinator (ASMA-8282 W3a).
//!
//! The sequence and its decisions live in
//! [`kontor_runtime::planning_pair::application`]. Every port here is the
//! daemon's own existing read, check, write or runtime call, unchanged:
//! - the trusted caller boundary and its current generation;
//! - the store, its compare-and-swap and its classified receipts;
//! - the one activated fleet snapshot;
//! - topology and containers, and credentials;
//! - the API mapping.

use super::*;
use kontor_runtime::planning_pair::application::{
    Applied, FrozenPlacement, InvokeOwner, InvokeRefusal, PairState,
};
use kontor_runtime::planning_pair::intent::InvokeMember;

/// The daemon's ports for one invocation.
pub(super) struct InvokePorts<'a>(pub(super) &'a Services);

#[async_trait::async_trait]
impl<'a> InvokeOwner for InvokePorts<'a> {
    type Error = ApiError;
    type Activity = tokio::sync::RwLockReadGuard<'a, ()>;
    type Caller = PlanningPairSeat;
    type Selection = kontor_fleet_activation::JointSelection;
    type Output = PlanningPairRunDto;

    fn refuse(&self, refusal: InvokeRefusal) -> ApiError {
        self.0.invoke_refusal(refusal)
    }

    fn domain_error(&self, error: &kontor_core::DomainError) -> ApiError {
        self.0.refuse_domain(error)
    }

    fn runtime_error(&self, error: &kontor_runtime::RuntimeError) -> ApiError {
        ApiError::from_runtime(self.0.realm_id, error)
    }

    fn now(&self) -> Timestamp {
        kontor_api::now()
    }

    fn realm_id(&self) -> Result<kontor_core::id::RealmId, ApiError> {
        Ok(self.0.state()?.realm_id())
    }

    fn begin_native_activity(&self) -> Result<Self::Activity, ApiError> {
        self.0.native_activity()
    }

    fn pinned_document(
        &self,
        project_id: ProjectId,
        profile_id: &str,
        version: SpecVersion,
    ) -> Result<(StoredConsultationProfileRevision, PlanningPairSpec), ApiError> {
        self.0
            .planning_pair_document(project_id, profile_id, version)
    }

    fn authorize_caller(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        task_id: Option<TaskId>,
        spec: &PlanningPairSpec,
        caller: PlanningPairSeat,
    ) -> Result<kontor_core::state::SeatBinding, ApiError> {
        self.0
            .authorize_planning_pair_caller(project_id, epic_id, task_id, spec, caller)
    }

    fn presented(&self, caller: PlanningPairSeat) -> SeatGeneration {
        presented(caller)
    }

    fn canonical(&self, document: &serde_json::Value) -> Result<CanonicalDocument, ApiError> {
        self.0.intent(document)
    }

    fn replayed(
        &self,
        key: &IdempotencyKey,
        intent: &CanonicalDocument,
        target: &AggregateRef,
    ) -> Result<Option<CommandReceiptId>, ApiError> {
        Ok(self
            .0
            .replayed(key, intent, Some(target))?
            .map(|receipt| receipt.id))
    }

    fn run_by_key(
        &self,
        project_id: ProjectId,
        key: &IdempotencyKey,
    ) -> Result<Option<StoredConsultationRun>, ApiError> {
        self.0
            .state()?
            .with_store(|store| store.get_consultation_run_by_key(project_id, key))
            .map_err(|error| self.0.refuse(&error))
    }

    fn epic(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<kontor_core::repository::MiniProject, ApiError> {
        self.0.epic_row(project_id, epic_id)
    }

    fn pair_state(&self, run: &StoredConsultationRun) -> Result<PairState, ApiError> {
        self.0.planning_pair_state(run)
    }

    fn advance_to_running(
        &self,
        frozen: &StoredConsultationRun,
    ) -> Result<StoredConsultationRun, ApiError> {
        self.0
            .state()?
            .with_store(|store| {
                store.advance_consultation_run(
                    frozen.project_id,
                    frozen.id,
                    frozen.revision,
                    ConsultationRunState::Running,
                    None,
                    kontor_api::now(),
                )
            })
            .map_err(|error| self.0.refuse(&error))
    }

    fn stored_run(
        &self,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
    ) -> Result<StoredConsultationRun, ApiError> {
        self.0.stored_planning_pair(project_id, run_id)
    }

    async fn receipt_hold_point(&self) {
        self.0.planning_pair_receipt_hold_point().await;
    }

    fn record(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        kind: CommandKind,
        target: AggregateRef,
        revision: AggregateRevision,
        intent: &CanonicalDocument,
    ) -> Result<(CommandReceiptId, bool), ApiError> {
        self.0
            .record_classified(key, project_id, kind, target, revision, intent)
    }

    fn render(
        &self,
        pair: &PairState,
        receipt: CommandReceiptId,
        applied: Applied,
    ) -> Result<PlanningPairRunDto, ApiError> {
        let applied = match applied {
            Applied::Created => AppliedDto::Created,
            Applied::Unchanged => AppliedDto::Unchanged,
        };
        self.0
            .planning_pair_dto(pair, Viewer::Caller, Some((receipt, applied)))
    }

    fn pinned_team_definition(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<Option<TeamDefinitionSpec>, ApiError> {
        self.0.pinned_team_definition(project_id, epic_id)
    }

    fn epic_role_catalog(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<RoleCatalogRevision, ApiError> {
        self.0.epic_role_catalog(project_id, epic_id)
    }

    fn member_catalog_role(
        &self,
        catalog: &RoleCatalogRevision,
        role_code: &RoleCode,
    ) -> Result<CatalogRoleRef, ApiError> {
        self.0.member_catalog_role(catalog, role_code)
    }

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
    ) -> Result<ContentHash, ApiError> {
        self.0.consultation_semantic_identity_of_kind(
            project_id, epic_id, task_id, family, kind, revision, definition, topic, None,
        )
    }

    fn run_by_semantic_identity(
        &self,
        project_id: ProjectId,
        identity: &ContentHash,
    ) -> Result<Option<StoredConsultationRun>, ApiError> {
        self.0
            .state()?
            .with_store(|store| {
                store.get_consultation_run_by_semantic_identity(project_id, identity)
            })
            .map_err(|error| self.0.refuse(&error))
    }

    fn place(
        &self,
        members: &[InvokeMember<'_>],
    ) -> Result<FrozenPlacement<kontor_fleet_activation::JointSelection>, ApiError> {
        let placement = self
            .0
            .fleet
            .planning_pair_placement(&PlanningPairRequest {
                members: members
                    .iter()
                    .map(|member| PlanningPairMemberRequest {
                        slot: member.slot,
                        binding_key: member.binding_key.to_owned(),
                        unavailable_accounts: member.unavailable_accounts.iter().cloned().collect(),
                        excluded_vendors: member.excluded_vendors.iter().cloned().collect(),
                    })
                    .collect(),
            })
            .map_err(|error| {
                self.0
                    .deny(
                        ApiErrorCode::PlacementBlocked,
                        match error {
                            kontor_fleet::FleetError::Invalid { rule } => rule,
                            _ => "the activated fleet policy cannot place this planning pair",
                        },
                    )
                    .about("FleetActivation")
            })?;
        Ok(FrozenPlacement {
            selection: placement.selection,
            placement_hash: placement.placement_hash,
            members: placement.members,
        })
    }

    fn runtime(&self) -> Result<std::sync::Arc<dyn RuntimeAdapter>, ApiError> {
        self.0.planning_pair_runtime()
    }

    fn require_member_routes(
        &self,
        adapter: &dyn RuntimeAdapter,
        members: &[kontor_core::planning_pair::PlanningPairMember],
    ) -> Result<(), ApiError> {
        self.0.require_planning_pair_member_routes(adapter, members)
    }

    fn project_topology(&self, project_id: ProjectId) -> Result<TopologySnapshot, ApiError> {
        self.0.project_topology(project_id)
    }

    fn ensure_epic_node(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<SessionTopologyNode, ApiError> {
        self.0.ensure_scope_chain(
            project_id,
            &TopologyScope {
                node_id: None,
                kind: Some(self.0.domain.delivery.epic_kind.clone()),
                epic_id: Some(epic_id),
                task_id: None,
                key: format!("epic:{epic_id}"),
            },
        )
    }

    fn attach_deadline(&self, now: Timestamp) -> Timestamp {
        now.checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
            .unwrap_or(now)
    }

    fn create_run(
        &self,
        run: &StoredConsultationRun,
        node: &NewSessionTopologyNode,
        seats: &[(&StoredConsultationSeat, &NewSeatBinding)],
        placement: &StoredPlanningPairPlacement,
        record: &StoredPlanningPairRecord,
    ) -> Result<(), ApiError> {
        self.0
            .state()?
            .with_store(|store| store.create_planning_pair_run(run, node, seats, placement, record))
            .map_err(|error| self.0.refuse(&error))
    }

    fn project(&self, project_id: ProjectId) -> Result<kontor_core::repository::Project, ApiError> {
        self.0.project_row(project_id)
    }

    fn topology_node(
        &self,
        project_id: ProjectId,
        node_id: TopologyNodeId,
    ) -> Result<Option<SessionTopologyNode>, ApiError> {
        self.0
            .state()?
            .with_store(|store| store.get_topology_node(project_id, node_id))
            .map_err(|error| self.0.refuse(&error))
    }

    fn epic_tracker_key(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<Option<TrackerKey>, ApiError> {
        self.0.epic_tracker_key(project_id, epic_id)
    }

    fn consultation_root(
        &self,
        project_root: &str,
        epic_key: Option<&TrackerKey>,
        node_id: TopologyNodeId,
    ) -> Result<kontor_runtime::workspace::WorkspaceRoot, ApiError> {
        self.0.consultation_root(project_root, epic_key, node_id)
    }

    fn seat_binding(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
    ) -> Result<Option<kontor_core::state::SeatBinding>, ApiError> {
        self.0
            .state()?
            .with_store(|store| store.get_seat_binding(project_id, seat_binding_id))
            .map_err(|error| self.0.refuse(&error))
    }

    fn require_member_role(
        &self,
        catalog: &RoleCatalogRevision,
        role: &CatalogRoleRef,
        role_code: &RoleCode,
    ) -> Result<(), ApiError> {
        self.0.require_member_role(catalog, role, role_code)
    }

    async fn ensure_container(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        cwd: &kontor_runtime::workspace::WorkspaceRoot,
        adapter: &dyn RuntimeAdapter,
    ) -> Result<ContainerBindingSnapshot, ApiError> {
        self.0
            .ensure_container(project_id, node, cwd, adapter)
            .await
    }

    fn execution_scope(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        task_id: Option<TaskId>,
        adapter: &dyn RuntimeAdapter,
    ) -> Result<ExecutionScope, ApiError> {
        self.0
            .execution_scope(project_id, epic_id, task_id, adapter)
    }

    fn member_credential(
        &self,
        seat_binding_id: SeatBindingId,
        occupancy_generation: u64,
    ) -> Result<ConsultationCredential, ApiError> {
        Ok(ConsultationCredential::new(
            self.0
                .state()?
                .credentials()
                .consultation_seat_credential_for_generation(seat_binding_id, occupancy_generation),
        ))
    }

    fn seat_name(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        scope: &ExecutionScope,
        role_code: &RoleCode,
        role_slot_id: Option<&RoleSlotId>,
    ) -> Result<ExternalName, ApiError> {
        self.0
            .seat_name(project_id, node, scope, role_code, role_slot_id)
    }

    fn launch_context(
        &self,
        run: &StoredConsultationRun,
        pair: &PairState,
        seat: &StoredConsultationSeat,
        slot: PlanningPairSlot,
        node: &SessionTopologyNode,
        cwd: &kontor_runtime::workspace::WorkspaceRoot,
        catalog: &RoleCatalogRevision,
        requested: Option<&kontor_runtime::FleetLaunchProvenance>,
    ) -> Result<kontor_runtime::planning_pair::PlanningPairLaunchContext, ApiError> {
        self.0
            .planning_pair_launch_context(run, pair, seat, slot, node, cwd, catalog, requested)
    }

    fn record_launch_provenance(
        &self,
        seat_binding_id: SeatBindingId,
        native_id: &ExternalId,
        requested: Option<&kontor_runtime::FleetLaunchProvenance>,
        observed: &kontor_runtime::FleetProvenanceObservation,
    ) {
        self.0.record_launch_provenance(
            "consultation",
            seat_binding_id.to_string(),
            native_id,
            requested,
            observed,
        );
    }

    fn record_member_launch(&self, claim: &StoredPlanningPairKnownNative) -> Result<(), ApiError> {
        self.0
            .state()?
            .with_store(|store| store.record_planning_pair_member_launch(claim))
            .map(drop)
            .map_err(|error| self.0.refuse(&error))
    }

    fn observe_member_attached(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        observed_at: Timestamp,
    ) -> Result<(), ApiError> {
        self.0
            .state()?
            .with_store(|store| {
                store.observe_seat_binding(
                    project_id,
                    seat_binding_id,
                    &SeatLivenessObservation {
                        attached_at: Some(observed_at),
                        runtime_reported: Some(kontor_core::state::ObservedRunState::Running),
                        ..SeatLivenessObservation::default()
                    },
                    observed_at,
                )
            })
            .map(drop)
            .map_err(|error| self.0.refuse(&error))
    }
}
