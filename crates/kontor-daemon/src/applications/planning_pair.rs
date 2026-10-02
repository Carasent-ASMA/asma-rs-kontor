//! Governed `planning_pair@1` (ASMA-8282, disposition D-1 to D-3).
//!
//! One planning pair is one consultation run in the shared registry: the same
//! semantic identity and unique index, the same seat bindings and generation
//! fencing, the same receipts, idempotent replay and compare-and-swap. Its
//! protocol phase is the domain's [`PlanningPairRun`], restored from the
//! latest canonical record through the domain transitions on every read and
//! write; nothing here interprets the record.
//!
//! Authority comes only from authentication. The caller is the exact frozen
//! caller seat at its current hosted generation; a member is the exact frozen
//! member seat at its current occupancy generation, natively observed. An
//! ambient Admin or Operator credential reaches only the document catalog and
//! the observer projection, which carries no contribution.
//!
//! Placement is the shared allocator on one activated snapshot, under the
//! TPM's explicit binding keys and eligibility. Names and slots come only from
//! the epic's pinned Team Definition container the published document selects.
//! A native launch happens only through the runtime port, and only a runtime
//! that supports the planning pair member surface performs one; persistence
//! alone never reports a launch.

use super::*;
use kontor_api::applications::AdviceDispositionDto;
use kontor_api::planning_pair::{
    InvokePlanningPairRequest, PlanningPairClarificationDto, PlanningPairContributionDto,
    PlanningPairDispositionRecordDto, PlanningPairMemberDispositionDto, PlanningPairMemberDto,
    PlanningPairProtocolDto, PlanningPairReader, PlanningPairRunDto, PlanningPairSeat,
    PlanningPairSlotDto, PlanningPairViewerDto, RecordPlanningPairContributionRequest,
    RecordPlanningPairDispositionRequest, RequestPlanningPairClarificationRequest,
};
use kontor_core::id::PlanningPairRunId;
use kontor_core::planning_pair::{
    ClarificationRequest, ConsultationProtocol, MemberDisposition, PlanningPairActor,
    PlanningPairContribution, PlanningPairDisposition, PlanningPairRecord, PlanningPairRound,
    PlanningPairRun, PlanningPairSlot, RecordedClarification, RecordedContribution,
};
use kontor_core::repository::{
    StoredPlanningPairContribution, StoredPlanningPairPlacement, StoredPlanningPairRecord,
};
use kontor_fleet_activation::{PlanningPairMemberRequest, PlanningPairRequest};

/// The logical role every planning pair member seat is held under, as
/// `advisor` is for an Advisor seat.
const MEMBER_LOGICAL_ROLE: &str = "planning_pair_member";

/// The capability profile the pinned Team Definition must declare for both
/// member slots: the closed member surface (run get, finding, answer).
const MEMBER_CAPABILITY_PROFILE: &str = "planning_pair_member";

/// Everything one planning pair operation reads, restored through the domain.
pub(super) struct PairState {
    run: StoredConsultationRun,
    revision: StoredConsultationProfileRevision,
    spec: PlanningPairSpec,
    placement: StoredPlanningPairPlacement,
    record_revision: AggregateRevision,
    record: PlanningPairRecord,
    pair: PlanningPairRun,
    seats: Vec<StoredConsultationSeat>,
}

/// Who a projection is rendered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Viewer {
    Caller,
    Member(PlanningPairSlot),
    Observer,
}

impl Services {
    // -----------------------------------------------------------------------
    // The document catalog: the shared consultation publication path.
    // -----------------------------------------------------------------------

    pub(super) fn planning_pair_catalog(
        &self,
        project_id: ProjectId,
    ) -> Result<ProfileCatalogDto, ApiError> {
        self.consultation_catalog(project_id, ConsultationFamily::PlanningPair)
    }

    pub(super) fn preview_planning_pair_document(
        &self,
        project_id: ProjectId,
        request: &ProfilePreviewRequest,
    ) -> Result<ProfilePreviewDto, ApiError> {
        self.preview_consultation_profile(project_id, ConsultationFamily::PlanningPair, request)
    }

    pub(super) fn apply_planning_pair_document(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &ProfileApplyRequest,
    ) -> Result<AppliedProfileDto, ApiError> {
        self.apply_consultation_profile(key, project_id, ConsultationFamily::PlanningPair, request)
    }

    // -----------------------------------------------------------------------
    // Invocation.
    // -----------------------------------------------------------------------

    pub(super) async fn invoke_planning_pair(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        caller: PlanningPairSeat,
        request: &InvokePlanningPairRequest,
    ) -> Result<PlanningPairRunDto, ApiError> {
        let _native_activity = self.native_activity()?;
        let PlanningPairProtocolDto::PlanningPairV1 = request.protocol;
        // The pinned document first: its caller roles and scopes decide who
        // may invoke it, and its exact hash must be the one the caller read.
        let (revision, spec) =
            self.planning_pair_document(project_id, &request.profile.id, request.profile.version)?;
        if revision.definition_hash != request.profile.definition_hash {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "the pinned planning pair document hash is not the published one",
            ));
        }
        // Authentication before any replay: a retired caller credential never
        // regains authority by repeating a key it once used.
        let caller_seat = self.authorize_planning_pair_caller(
            project_id,
            epic_id,
            request.task_id,
            &spec,
            caller,
        )?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "invoke_planning_pair_run",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "protocol": ConsultationProtocol::PlanningPair.as_str(),
            "profile": [
                request.profile.id.as_str(),
                request.profile.version.get(),
                request.profile.definition_hash.as_str(),
            ],
            "topic": request.topic.as_str(),
            "question": request.question.as_str(),
            "task_id": request.task_id.map(|id| id.to_string()),
            "members": request.members.iter().map(|member| serde_json::json!({
                "slot": member.slot.slot().as_str(),
                "binding_key": member.binding_key,
                "unavailable_accounts": member.unavailable_accounts.iter().collect::<BTreeSet<_>>(),
                "excluded_vendors": member.excluded_vendors.iter().collect::<BTreeSet<_>>(),
            })).collect::<Vec<_>>(),
            "caller_seat_binding_id": caller.seat_binding_id.to_string(),
            "caller_occupancy_generation": caller.occupancy_generation,
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            let run = self
                .state()?
                .with_store(|store| store.get_consultation_run_by_key(project_id, key))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the invocation receipt has no durable planning pair",
                    )
                })?;
            let pair = self.planning_pair_state(&run)?;
            return self.planning_pair_dto(
                &pair,
                Viewer::Caller,
                Some((receipt.id, AppliedDto::Unchanged)),
            );
        }
        // A runtime that has not composed the member surface is the defined
        // capability gap, answered before anything is frozen or prepared.
        self.require_planning_pair_member_surface()?;
        let run = if let Some(existing) = self
            .state()?
            .with_store(|store| store.get_consultation_run_by_key(project_id, key))
            .map_err(|error| self.refuse(&error))?
        {
            if existing.id.family() != ConsultationFamily::PlanningPair
                || existing.mini_project_id != epic_id
                || existing.invoke_intent_hash != *intent.hash()
            {
                return Err(self.deny(
                    ApiErrorCode::IdempotencyConflict,
                    "the idempotency key was already used for a different consultation",
                ));
            }
            existing
        } else {
            let epic = self.epic_row(project_id, epic_id)?;
            if epic.revision != request.expected_revision {
                return Err(self
                    .deny(
                        ApiErrorCode::RevisionConflict,
                        "the epic moved since the planning pair invocation was prepared",
                    )
                    .with_revision(Some(epic.revision)));
            }
            self.freeze_planning_pair(
                key,
                &intent,
                project_id,
                epic_id,
                request,
                &revision,
                &spec,
                &caller_seat,
            )?
        };
        self.materialize_planning_pair_members(&run).await?;
        let run = self.running_planning_pair(&run)?;
        // A request for this key that resumed the run may have finished first;
        // its receipt is then this request's answer.
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            let pair = self.planning_pair_state(&run)?;
            return self.planning_pair_dto(
                &pair,
                Viewer::Caller,
                Some((receipt.id, AppliedDto::Unchanged)),
            );
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::InvokePlanningPairRun,
            target,
            run.revision,
            &intent,
        )?;
        let pair = self.planning_pair_state(&run)?;
        self.planning_pair_dto(
            &pair,
            Viewer::Caller,
            Some((receipt_id, AppliedDto::Created)),
        )
    }

    /// The run once its members are launched, advanced to `running`.
    ///
    /// Another request for the same key may have resumed the run and launched
    /// with this one. The run's compare-and-swap admits exactly one advance
    /// from the frozen revision. A request that loses it answers with the row
    /// the winner wrote; only a run still materializing after that is a
    /// refusal.
    fn running_planning_pair(
        &self,
        frozen: &StoredConsultationRun,
    ) -> Result<StoredConsultationRun, ApiError> {
        if frozen.state != ConsultationRunState::Materializing {
            return Ok(frozen.clone());
        }
        let ConsultationRunId::PlanningPair(run_id) = frozen.id else {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this operation requires a planning pair",
            ));
        };
        match self.state()?.with_store(|store| {
            store.advance_consultation_run(
                frozen.project_id,
                frozen.id,
                frozen.revision,
                ConsultationRunState::Running,
                None,
                kontor_api::now(),
            )
        }) {
            Ok(advanced) => Ok(advanced),
            Err(error) => {
                let after = self.stored_planning_pair(frozen.project_id, run_id)?;
                if after.state == ConsultationRunState::Materializing {
                    Err(self.refuse(&error))
                } else {
                    Ok(after)
                }
            }
        }
    }

    /// Whether the runtime planning pair members are placed on composes their
    /// closed member surface, asked without a native effect.
    fn require_planning_pair_member_surface(&self) -> Result<(), ApiError> {
        let state = self.state()?;
        let runtime_kind = self.node_runtime_kind()?;
        let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime selected for planning pair placement is not configured",
            )
        })?;
        adapter
            .validate_planning_pair_member_surface()
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))
    }

    /// The role catalog this epic selected, exactly as persisted.
    ///
    /// The epic's frozen roster is the selection: every seat names one catalog
    /// revision, and the roster pins that revision's exact bytes by
    /// `catalog_hash`. The persisted revision must hash to that pin. A missing
    /// roster, a roster spanning two catalogs, an unpersisted revision or a
    /// hash mismatch refuses, because no other catalog the realm happens to
    /// know is the one this epic chose.
    fn epic_role_catalog(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<RoleCatalogRevision, ApiError> {
        let roster = self
            .optional_frozen_roster(project_id, epic_id)?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the epic has frozen no Core Team roster, so it has selected no role catalog",
                )
            })?;
        let mut pins = roster
            .revision
            .seats
            .iter()
            .map(|seat| (seat.role.catalog_id, seat.role.catalog_revision));
        let (catalog_id, version) = pins.next().ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the epic's frozen roster names no role catalog revision",
            )
        })?;
        if pins.any(|pin| pin != (catalog_id, version)) {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the epic's frozen roster names more than one role catalog revision",
            ));
        }
        let catalog = self
            .state()?
            .with_store(|store| store.get_role_catalog(catalog_id, version))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the epic's selected role catalog revision is not persisted in this realm",
                )
            })?;
        let persisted = catalog
            .canonicalize()
            .map_err(|error| self.refuse_domain(&error))?;
        if persisted.hash() != &roster.revision.catalog_hash {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the persisted role catalog does not hash to the epic's frozen catalog pin",
            ));
        }
        Ok(catalog)
    }

    /// One member's registered role: its explicit code, as the epic's selected
    /// catalog declares it, current, and provably that catalog's projection.
    fn member_catalog_role(
        &self,
        catalog: &RoleCatalogRevision,
        role_code: &kontor_core::id::RoleCode,
    ) -> Result<CatalogRoleRef, ApiError> {
        let entry = catalog.role(role_code).ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the member's role code is absent from the epic's selected role catalog",
            )
        })?;
        if entry.lifecycle != kontor_core::spec::CodeLifecycle::Current {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the member's role cannot open new seats in the epic's selected role catalog",
            ));
        }
        let role = CatalogRoleRef {
            catalog_id: catalog.catalog_id,
            catalog_revision: catalog.version,
            role_code: entry.role_code.clone(),
            standard_title: entry.standard_title.clone(),
            custom_display_name: None,
        };
        self.require_member_role(catalog, &role, role_code)?;
        Ok(role)
    }

    /// A frozen member role corresponds to the member's explicit code and is
    /// an exact projection of the epic's selected catalog.
    fn require_member_role(
        &self,
        catalog: &RoleCatalogRevision,
        role: &CatalogRoleRef,
        role_code: &kontor_core::id::RoleCode,
    ) -> Result<(), ApiError> {
        role.validate_against(catalog)
            .map_err(|error| self.refuse_domain(&error))?;
        if &role.role_code != role_code {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "a frozen member role does not correspond to the document's member role code",
            ));
        }
        Ok(())
    }

    /// The published document revision and its validated specification.
    fn planning_pair_document(
        &self,
        project_id: ProjectId,
        profile_id: &str,
        version: SpecVersion,
    ) -> Result<(StoredConsultationProfileRevision, PlanningPairSpec), ApiError> {
        let revision = self
            .stored_consultation_profiles(project_id, ConsultationFamily::PlanningPair)?
            .into_iter()
            .find(|revision| revision.profile_id == profile_id && revision.version == version)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such planning pair document revision is published in this project",
                )
            })?;
        let spec: PlanningPairSpec = serde_json::from_str(&revision.definition).map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the stored planning pair document cannot be read by this build",
            )
        })?;
        spec.validate()
            .map_err(|error| self.refuse_domain(&error))?;
        Ok((revision, spec))
    }

    /// The authenticated caller, held to the pinned document's roles and
    /// scopes and to its own current hosted generation.
    fn authorize_planning_pair_caller(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        task_id: Option<TaskId>,
        spec: &PlanningPairSpec,
        caller: PlanningPairSeat,
    ) -> Result<kontor_core::state::SeatBinding, ApiError> {
        if let Some(task_id) = task_id {
            let task = self.task_row(project_id, task_id)?;
            if task.mini_project_id != Some(epic_id) {
                return Err(self.deny(
                    ApiErrorCode::Forbidden,
                    "the requested ticket does not belong to this epic",
                ));
            }
            if !spec.allowed_scopes.contains(&ConsultationScope::Ticket) {
                return Err(self.deny(
                    ApiErrorCode::Forbidden,
                    "the pinned planning pair document does not permit ticket-scoped invocation",
                ));
            }
        } else if !spec.allowed_scopes.contains(&ConsultationScope::Epic) {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "the pinned planning pair document does not permit epic-scoped invocation",
            ));
        }
        let seat = self.active_epic_seat(project_id, epic_id, caller)?;
        let slot_role = seat.role_slot_id.as_role_key().as_str();
        let catalog_role = seat.role.role_code.as_str();
        if !spec.allowed_caller_roles.iter().any(|allowed| {
            allowed.as_str().eq_ignore_ascii_case(slot_role)
                || allowed.as_str().eq_ignore_ascii_case(catalog_role)
        }) {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "the authenticated seat's role may not convene this planning pair",
            ));
        }
        Ok(seat)
    }

    /// One active seat of this epic, authenticated at its current hosted
    /// occupancy generation.
    fn active_epic_seat(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        caller: PlanningPairSeat,
    ) -> Result<kontor_core::state::SeatBinding, ApiError> {
        let state = self.state()?;
        let seat = state
            .with_store(|store| store.get_seat_binding(project_id, caller.seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|seat| seat.is_non_terminal() && !seat.closes_children())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::StaleBinding,
                    "the authenticated caller seat is not active",
                )
            })?;
        let node = state
            .with_store(|store| store.get_topology_node(project_id, seat.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|node| node.mini_project_id == Some(epic_id))
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Forbidden,
                    "the authenticated caller seat does not belong to this epic",
                )
            })?;
        if node.lifecycle != TopologyLifecycle::Active {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the authenticated caller seat's topology node is not active",
            ));
        }
        let current = state
            .with_store(|store| {
                store.hosted_topology_seat_occupancy_generation(project_id, caller.seat_binding_id)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::StaleBinding,
                    "the authenticated caller seat has no active hosted native occupancy",
                )
            })?;
        if caller.occupancy_generation != current {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the caller credential belongs to a fenced native occupancy generation",
            ));
        }
        Ok(seat)
    }

    /// Freeze one new planning pair before any native effect: the run in the
    /// shared registry under its shared semantic identity, its container node
    /// and two member seats from the pinned Team Definition, the shared
    /// allocator's placement on one activated snapshot, and the first record.
    #[allow(
        clippy::too_many_arguments,
        reason = "every frozen authority input is passed explicitly"
    )]
    fn freeze_planning_pair(
        &self,
        key: &IdempotencyKey,
        intent: &CanonicalDocument,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &InvokePlanningPairRequest,
        revision: &StoredConsultationProfileRevision,
        spec: &PlanningPairSpec,
        caller: &kontor_core::state::SeatBinding,
    ) -> Result<StoredConsultationRun, ApiError> {
        let state = self.state()?;
        let definition = self
            .pinned_team_definition(project_id, epic_id)?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the epic has no pinned Team Definition for planning pair placement",
                )
            })?;
        let container = definition.container(&spec.container_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the pinned Team Definition declares no container of the kind this planning pair selects",
            )
        })?;
        if !container.read_only {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "a planning pair container must be read-only",
            ));
        }
        let catalog = self.epic_role_catalog(project_id, epic_id)?;
        let mut slot_roles = Vec::with_capacity(2);
        for slot in PlanningPairSlot::ALL {
            let declared = container
                .slots
                .iter()
                .filter(|declared| declared.slot_id.as_str() == slot.as_str())
                .collect::<Vec<_>>();
            let [declared] = declared.as_slice() else {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the planning pair container must declare seat-a and seat-b exactly once each",
                ));
            };
            if declared.display_name.as_ref().map(ExternalName::as_str) != Some(slot.label()) {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a planning pair slot must be titled SEAT A or SEAT B",
                ));
            }
            if declared.capability_profile.as_str() != MEMBER_CAPABILITY_PROFILE {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a planning pair slot must hold the planning_pair_member capability profile",
                ));
            }
            // The title is the Team Definition's; the registered role is the
            // pinned document's own explicit member role, since a
            // display-named slot carries none, resolved only in the catalog
            // the epic itself selected.
            let member = spec
                .members
                .iter()
                .find(|member| member.slot == *slot)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the pinned planning pair document does not declare this member",
                    )
                })?;
            slot_roles.push(self.member_catalog_role(&catalog, &member.role_code)?);
        }
        if container.slots.len() != 2 {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the planning pair container must declare exactly its two member slots",
            ));
        }
        let semantic_identity_hash = self.consultation_semantic_identity_of_kind(
            project_id,
            epic_id,
            request.task_id,
            ConsultationFamily::PlanningPair,
            &spec.container_kind,
            revision,
            &definition,
            &request.topic,
            None,
        )?;
        if let Some(existing) = state
            .with_store(|store| {
                store.get_consultation_run_by_semantic_identity(project_id, &semantic_identity_hash)
            })
            .map_err(|error| self.refuse(&error))?
        {
            return Err(self
                .deny(
                    ApiErrorCode::IdempotencyConflict,
                    "consultation_semantic_duplicate: this planning pair scope and topic already has one run",
                )
                .about("consultation semantic identity")
                .located_at(format!("consultation-runs/{}", existing.id.as_text()))
                .advising("read or resume the existing consultation run"));
        }
        // One activated snapshot, the shared allocator, distinct actual
        // vendors. A blocked placement freezes nothing.
        let placement = self
            .fleet
            .planning_pair_placement(&PlanningPairRequest {
                members: request
                    .members
                    .iter()
                    .map(|member| PlanningPairMemberRequest {
                        slot: member.slot.slot(),
                        binding_key: member.binding_key.clone(),
                        unavailable_accounts: member.unavailable_accounts.iter().cloned().collect(),
                        excluded_vendors: member.excluded_vendors.iter().cloned().collect(),
                    })
                    .collect(),
            })
            .map_err(|error| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    match error {
                        kontor_fleet::FleetError::Invalid { rule } => rule,
                        _ => "the activated fleet policy cannot place this planning pair",
                    },
                )
                .about("FleetActivation")
            })?;
        let Some(members) = placement.members.clone() else {
            return Err(self
                .deny(
                    ApiErrorCode::PlacementBlocked,
                    "no complete placement gives both planning pair members an eligible route on distinct actual vendors",
                )
                .about("FleetActivation"));
        };
        let placement_document = self.intent(&serde_json::json!({
            "schema_version": 1,
            "protocol": ConsultationProtocol::PlanningPair.as_str(),
            "selection": placement.selection,
        }))?;
        if placement_document.hash() != &placement.placement_hash {
            return Err(self.deny(
                ApiErrorCode::Unavailable,
                "the planning pair placement receipt is not the shared reader's canonical receipt",
            ));
        }
        let pair = PlanningPairRun::admit(spec, members, request.question.clone())
            .map_err(|error| self.refuse_domain(&error))?;
        let record = PlanningPairRecord::admitted(&pair);
        let topology = self.project_topology(project_id)?;
        let epic_node = self.ensure_scope_chain(
            project_id,
            &TopologyScope {
                node_id: None,
                kind: Some(self.domain.delivery.epic_kind.clone()),
                epic_id: Some(epic_id),
                task_id: None,
                key: format!("epic:{epic_id}"),
            },
        )?;
        let now = kontor_api::now();
        let run_id = ConsultationRunId::PlanningPair(PlanningPairRunId::generate());
        let node_id = TopologyNodeId::generate();
        let definition_snapshot = TeamDefinitionSnapshot::from_revision(&definition)
            .map_err(|error| self.refuse_domain(&error))?;
        let question_hash = ContentHash::of(request.question.as_str().as_bytes());
        let context = self.intent(&serde_json::json!({
            "schema_version": 1,
            "realm_id": state.realm_id().to_string(),
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
            context: serde_json::from_str(context.json()).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the frozen planning pair context could not be decoded",
                )
            })?,
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
        let deadline = now
            .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
            .unwrap_or(now);
        let logical_role =
            RoleKey::parse(MEMBER_LOGICAL_ROLE).map_err(|error| self.refuse_domain(&error))?;
        let mut seats = Vec::with_capacity(2);
        let mut bindings = Vec::with_capacity(2);
        for (member, role) in pair.members().members().iter().zip(slot_roles) {
            let seat_binding_id = SeatBindingId::generate();
            let role_slot_id = RoleSlotId::parse(member.slot.as_str())
                .map_err(|error| self.refuse_domain(&error))?;
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
            .map_err(|error| self.refuse_domain(&error))?;
        let pairs: Vec<_> = seats.iter().zip(bindings.iter()).collect();
        state
            .with_store(|store| {
                store.create_planning_pair_run(
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
                )
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(run)
    }

    /// Launch both members through the runtime port, each with its own scoped
    /// credential, its frozen route and the read-only member surface. A runtime
    /// that does not support planning pair members refuses here, before any
    /// native effect, and the run stays materializing.
    async fn materialize_planning_pair_members(
        &self,
        run: &StoredConsultationRun,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let pair = self.planning_pair_state(run)?;
        let project = self.project_row(run.project_id)?;
        let node = state
            .with_store(|store| store.get_topology_node(run.project_id, run.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the planning pair's frozen topology node is missing",
                )
            })?;
        let runtime_kind = self.node_runtime_kind()?;
        let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime selected for planning pair placement is not configured",
            )
        })?;
        adapter
            .validate_planning_pair_member_surface()
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let epic_key = self.epic_tracker_key(run.project_id, run.mini_project_id)?;
        let cwd = self.consultation_root(project.root_path.as_str(), epic_key.as_ref(), node.id)?;
        let mut seats = pair.seats.clone();
        if seats.iter().all(|seat| seat.native_identity.is_some()) {
            return Ok(());
        }
        // Every frozen member role is proved again against the epic's
        // selected catalog before any native effect, so a stored role that
        // no longer corresponds to the member's explicit code never launches.
        let catalog = self.epic_role_catalog(run.project_id, run.mini_project_id)?;
        for seat in &seats {
            let slot = PlanningPairSlot::parse(seat.role_slot_id.as_str())
                .map_err(|error| self.refuse_domain(&error))?;
            let member = pair
                .spec
                .members
                .iter()
                .find(|member| member.slot == slot)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "a frozen planning pair seat is absent from its pinned document",
                    )
                })?;
            let binding = state
                .with_store(|store| store.get_seat_binding(run.project_id, seat.seat_binding_id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the planning pair member has no persistent topology binding",
                    )
                })?;
            self.require_member_role(&catalog, &binding.role, &member.role_code)?;
        }
        let container = self
            .ensure_container(run.project_id, &node, &cwd, adapter.as_ref())
            .await?;
        let scope = self.execution_scope(
            run.project_id,
            run.mini_project_id,
            node.task_id,
            adapter.as_ref(),
        )?;
        let capabilities = adapter
            .discover_capabilities()
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let context_policy = ContextPolicySnapshot::standard(
            &capabilities.limits.context_window,
            capabilities.supports(RuntimeCapability::ContextPolicy),
            SCHEMA_VERSION,
            kontor_api::now(),
        )
        .map_err(|error| self.refuse_domain(&error))?;
        let receipt: serde_json::Value = serde_json::from_str(pair.placement.placement.json())
            .map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the frozen planning pair placement could not be decoded",
                )
            })?;
        for seat in &mut seats {
            if seat.native_identity.is_some() {
                continue;
            }
            let slot = PlanningPairSlot::parse(seat.role_slot_id.as_str())
                .map_err(|error| self.refuse_domain(&error))?;
            let member = pair
                .spec
                .members
                .iter()
                .find(|member| member.slot == slot)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "a frozen planning pair seat is absent from its pinned document",
                    )
                })?;
            let route_provenance = ConsultationRouteProvenance::fleet_configuration(
                pair.placement.placement.hash().clone(),
            );
            let fleet_provenance = planning_pair_fleet_provenance(&receipt, slot);
            adapter
                .validate_consultation_model_rung(&seat.model_rung, &route_provenance)
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            let credential = state
                .credentials()
                .consultation_seat_credential_for_generation(
                    seat.seat_binding_id,
                    seat.occupancy_generation,
                );
            let prompt = BoundedText::parse(&format!(
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
                charter = pair.spec.charter.as_str(),
                specialty = member.specialty.as_str(),
                behavior = member.behavior.as_str(),
                question = run.question.as_str(),
                project = run.project_id,
                run_id = run.id.as_text(),
                revision = run.revision.get(),
                seat_binding = seat.seat_binding_id,
            ))
            .map_err(|error| self.refuse_domain(&error))?;
            let topology_seat = state
                .with_store(|store| store.get_seat_binding(run.project_id, seat.seat_binding_id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the planning pair member has no persistent topology binding",
                    )
                })?;
            let display_name = self.seat_name(
                run.project_id,
                &node,
                &scope,
                &topology_seat.role.role_code,
                Some(&topology_seat.role_slot_id),
            )?;
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
                    credential: ConsultationCredential::new(credential),
                    model_rung: seat.model_rung.clone(),
                    route_provenance,
                    fleet_provenance: fleet_provenance.clone(),
                    context_policy: context_policy.clone(),
                    requested_at: kontor_api::now(),
                })
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            self.record_launch_provenance(
                "consultation",
                seat.seat_binding_id.to_string(),
                &outcome.identity.native_id,
                fleet_provenance.as_ref(),
                &outcome.fleet_provenance,
            );
            seat.native_identity = Some(outcome.identity);
            seat.provider_session_id = outcome.provider_session_id;
            seat.observed_at = Some(outcome.observed_at);
            state
                .with_store(|store| store.bind_consultation_seat(run.project_id, seat))
                .map_err(|error| self.refuse(&error))?;
            state
                .with_store(|store| {
                    store.observe_seat_binding(
                        run.project_id,
                        seat.seat_binding_id,
                        &SeatLivenessObservation {
                            attached_at: Some(outcome.observed_at),
                            runtime_reported: Some(kontor_core::state::ObservedRunState::Running),
                            ..SeatLivenessObservation::default()
                        },
                        outcome.observed_at,
                    )
                })
                .map_err(|error| self.refuse(&error))?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Reading.
    // -----------------------------------------------------------------------

    /// Restore one stored planning pair through the domain transitions.
    pub(super) fn planning_pair_state(
        &self,
        run: &StoredConsultationRun,
    ) -> Result<PairState, ApiError> {
        let state = self.state()?;
        if run.id.family() != ConsultationFamily::PlanningPair {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this operation requires a planning pair",
            ));
        }
        let version = run.profile_version;
        let (revision, spec) =
            self.planning_pair_document(run.project_id, &run.profile_id, version)?;
        if revision.definition_hash != run.definition_hash {
            return Err(self.deny(
                ApiErrorCode::Unavailable,
                "the planning pair's pinned document no longer matches its frozen hash",
            ));
        }
        let placement = state
            .with_store(|store| store.planning_pair_placement(run.project_id, run.id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the planning pair has no frozen placement",
                )
            })?;
        let stored = state
            .with_store(|store| store.latest_planning_pair_record(run.project_id, run.id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the planning pair has no durable record",
                )
            })?;
        let record: PlanningPairRecord = stored
            .record
            .deserialize()
            .map_err(|error| self.refuse_domain(&error))?;
        if record.placement_hash != *placement.placement.hash() {
            return Err(self.deny(
                ApiErrorCode::Unavailable,
                "the planning pair record names another placement",
            ));
        }
        let pair = PlanningPairRun::restore(&spec, record.clone())
            .map_err(|error| self.refuse_domain(&error))?;
        let seats = state
            .with_store(|store| store.list_consultation_seats(run.project_id, run.id))
            .map_err(|error| self.refuse(&error))?;
        Ok(PairState {
            run: run.clone(),
            revision,
            spec,
            placement,
            record_revision: stored.revision,
            record,
            pair,
            seats,
        })
    }

    fn stored_planning_pair(
        &self,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
    ) -> Result<StoredConsultationRun, ApiError> {
        self.state()?
            .with_store(|store| {
                store.get_consultation_run(project_id, ConsultationRunId::PlanningPair(run_id))
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "no such planning pair"))
    }

    pub(super) fn read_planning_pair(
        &self,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
        reader: PlanningPairReader,
    ) -> Result<PlanningPairRunDto, ApiError> {
        let run = self.stored_planning_pair(project_id, run_id)?;
        let pair = self.planning_pair_state(&run)?;
        let viewer = match reader {
            PlanningPairReader::Ambient => Viewer::Observer,
            PlanningPairReader::Seat(seat)
                if seat.seat_binding_id == run.caller_seat_binding_id =>
            {
                self.active_epic_seat(project_id, run.mini_project_id, seat)?;
                Viewer::Caller
            }
            PlanningPairReader::Seat(seat) => {
                let member = self.authenticated_member(&pair, seat, false)?;
                Viewer::Member(member)
            }
        };
        self.planning_pair_dto(&pair, viewer, None)
    }

    /// The member slot an authenticated scoped seat fills: the exact frozen
    /// member at its current occupancy generation, natively observed when it
    /// writes. Any other seat, and every ambient credential, is refused.
    fn authenticated_member(
        &self,
        pair: &PairState,
        seat: PlanningPairSeat,
        writes: bool,
    ) -> Result<PlanningPairSlot, ApiError> {
        let stored = pair
            .seats
            .iter()
            .find(|stored| stored.seat_binding_id == seat.seat_binding_id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Forbidden,
                    "the authenticated seat is not a member of this planning pair",
                )
            })?;
        if stored.occupancy_generation != seat.occupancy_generation {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the member credential belongs to a fenced native occupancy generation",
            ));
        }
        if writes && stored.native_identity.is_none() {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the planning pair member has no observed native identity",
            ));
        }
        PlanningPairSlot::parse(stored.role_slot_id.as_str())
            .map_err(|error| self.refuse_domain(&error))
    }

    // -----------------------------------------------------------------------
    // Member and caller writes.
    // -----------------------------------------------------------------------

    pub(super) fn record_planning_pair_contribution(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
        member: PlanningPairSeat,
        round: PlanningPairRound,
        request: &RecordPlanningPairContributionRequest,
    ) -> Result<PlanningPairRunDto, ApiError> {
        let run = self.stored_planning_pair(project_id, run_id)?;
        let mut pair = self.planning_pair_state(&run)?;
        // Authentication before replay: a fenced member never replays.
        let slot = self.authenticated_member(&pair, member, true)?;
        let (operation, kind) = match round {
            PlanningPairRound::Findings => (
                "record_planning_pair_finding",
                CommandKind::RecordPlanningPairFinding,
            ),
            PlanningPairRound::Clarification => (
                "record_planning_pair_answer",
                CommandKind::RecordPlanningPairAnswer,
            ),
        };
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": operation,
            "project": project_id.to_string(),
            "run": run.id.as_text(),
            "slot": slot.as_str(),
            "advice": request.advice.as_str(),
            "member_seat_binding_id": member.seat_binding_id.to_string(),
            "member_occupancy_generation": member.occupancy_generation,
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: run.mini_project_id,
        };
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            return self.planning_pair_dto(
                &pair,
                Viewer::Member(slot),
                Some((receipt.id, AppliedDto::Unchanged)),
            );
        }
        self.expect_planning_pair_revision(&run, request.expected_revision)?;
        let actor = PlanningPairActor::Member(slot);
        let document_hash = match round {
            PlanningPairRound::Findings => {
                pair.pair
                    .record_finding(actor, slot, request.advice.clone())
            }
            PlanningPairRound::Clarification => {
                pair.pair.record_answer(actor, slot, request.advice.clone())
            }
        }
        .map_err(|error| self.refuse_domain(&error))?;
        let contribution = RecordedContribution {
            slot,
            advice: request.advice.clone(),
            document_hash: document_hash.clone(),
        };
        match round {
            PlanningPairRound::Findings => in_slot_order(&mut pair.record.findings, contribution),
            PlanningPairRound::Clarification => {
                let clarification = pair.record.clarification.as_mut().ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the accepted answer has no stored clarification",
                    )
                })?;
                in_slot_order(&mut clarification.answers, contribution);
            }
        }
        let next = run
            .revision
            .next()
            .map_err(|error| self.refuse_domain(&error))?;
        let now = kontor_api::now();
        let stored = self.append_planning_pair_revision(
            &pair,
            run.state,
            Some(&StoredPlanningPairContribution {
                run_id: run.id,
                round,
                slot,
                document_hash,
                seat_binding_id: member.seat_binding_id,
                occupancy_generation: member.occupancy_generation,
                record_revision: next,
                created_at: now,
            }),
        )?;
        let receipt_id = self.record(key, project_id, kind, target, stored.revision, &intent)?;
        let pair = self.planning_pair_state(&stored)?;
        self.planning_pair_dto(
            &pair,
            Viewer::Member(slot),
            Some((receipt_id, AppliedDto::Created)),
        )
    }

    pub(super) fn request_planning_pair_question(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
        caller: PlanningPairSeat,
        request: &RequestPlanningPairClarificationRequest,
    ) -> Result<PlanningPairRunDto, ApiError> {
        let run = self.stored_planning_pair(project_id, run_id)?;
        let mut pair = self.planning_pair_state(&run)?;
        self.authenticated_caller(&run, caller)?;
        let addressed: Vec<PlanningPairSlot> =
            request.addressed.iter().map(|slot| slot.slot()).collect();
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "request_planning_pair_clarification",
            "project": project_id.to_string(),
            "run": run.id.as_text(),
            "question": request.question.as_str(),
            "addressed": addressed.iter().map(|slot| slot.as_str()).collect::<Vec<_>>(),
            "caller_seat_binding_id": caller.seat_binding_id.to_string(),
            "caller_occupancy_generation": caller.occupancy_generation,
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: run.mini_project_id,
        };
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            return self.planning_pair_dto(
                &pair,
                Viewer::Caller,
                Some((receipt.id, AppliedDto::Unchanged)),
            );
        }
        self.expect_planning_pair_revision(&run, request.expected_revision)?;
        let clarification = ClarificationRequest {
            question: request.question.clone(),
            addressed,
        };
        pair.pair
            .request_clarification(PlanningPairActor::Caller, clarification.clone())
            .map_err(|error| self.refuse_domain(&error))?;
        pair.record.clarification = Some(RecordedClarification {
            request: clarification,
            answers: Vec::new(),
        });
        let stored = self.append_planning_pair_revision(&pair, run.state, None)?;
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::RequestPlanningPairClarification,
            target,
            stored.revision,
            &intent,
        )?;
        let pair = self.planning_pair_state(&stored)?;
        self.planning_pair_dto(
            &pair,
            Viewer::Caller,
            Some((receipt_id, AppliedDto::Created)),
        )
    }

    pub(super) fn record_planning_pair_decision(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
        caller: PlanningPairSeat,
        request: &RecordPlanningPairDispositionRequest,
    ) -> Result<PlanningPairRunDto, ApiError> {
        let run = self.stored_planning_pair(project_id, run_id)?;
        let mut pair = self.planning_pair_state(&run)?;
        self.authenticated_caller(&run, caller)?;
        let disposition = PlanningPairDisposition {
            members: request
                .members
                .iter()
                .map(|member| MemberDisposition {
                    slot: member.slot.slot(),
                    finding: member.finding.clone(),
                    answer: member.answer.clone(),
                    disposition: advice_disposition(member.disposition),
                })
                .collect(),
            rationale: request.rationale.clone(),
        };
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "record_planning_pair_disposition",
            "project": project_id.to_string(),
            "run": run.id.as_text(),
            "disposition": disposition,
            "caller_seat_binding_id": caller.seat_binding_id.to_string(),
            "caller_occupancy_generation": caller.occupancy_generation,
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: run.mini_project_id,
        };
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            return self.planning_pair_dto(
                &pair,
                Viewer::Caller,
                Some((receipt.id, AppliedDto::Unchanged)),
            );
        }
        self.expect_planning_pair_revision(&run, request.expected_revision)?;
        pair.pair
            .record_disposition(PlanningPairActor::Caller, disposition.clone())
            .map_err(|error| self.refuse_domain(&error))?;
        pair.record.disposition = Some(disposition);
        let stored =
            self.append_planning_pair_revision(&pair, ConsultationRunState::Disposed, None)?;
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::RecordPlanningPairDisposition,
            target,
            stored.revision,
            &intent,
        )?;
        let pair = self.planning_pair_state(&stored)?;
        self.planning_pair_dto(
            &pair,
            Viewer::Caller,
            Some((receipt_id, AppliedDto::Created)),
        )
    }

    /// The exact frozen caller, at its current hosted generation.
    fn authenticated_caller(
        &self,
        run: &StoredConsultationRun,
        caller: PlanningPairSeat,
    ) -> Result<(), ApiError> {
        if caller.seat_binding_id != run.caller_seat_binding_id {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "only the planning pair's frozen caller asks for clarification or records the disposition",
            ));
        }
        self.active_epic_seat(run.project_id, run.mini_project_id, caller)?;
        Ok(())
    }

    fn expect_planning_pair_revision(
        &self,
        run: &StoredConsultationRun,
        expected: AggregateRevision,
    ) -> Result<(), ApiError> {
        if run.revision != expected {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the planning pair moved since the write was prepared",
                )
                .with_revision(Some(run.revision)));
        }
        Ok(())
    }

    /// Write the next record revision beside the run's next revision, under
    /// compare-and-swap.
    fn append_planning_pair_revision(
        &self,
        pair: &PairState,
        next_state: ConsultationRunState,
        contribution: Option<&StoredPlanningPairContribution>,
    ) -> Result<StoredConsultationRun, ApiError> {
        let next = pair
            .run
            .revision
            .next()
            .map_err(|error| self.refuse_domain(&error))?;
        let document = pair
            .record
            .canonicalize()
            .map_err(|error| self.refuse_domain(&error))?;
        let record = StoredPlanningPairRecord {
            run_id: pair.run.id,
            project_id: pair.run.project_id,
            revision: next,
            phase: pair.pair.state(),
            record: document,
            created_at: kontor_api::now(),
        };
        self.state()?
            .with_store(|store| {
                store.append_planning_pair_record(
                    pair.run.project_id,
                    pair.run.id,
                    pair.run.revision,
                    next_state,
                    &record,
                    contribution,
                )
            })
            .map_err(|error| self.refuse(&error))
    }

    // -----------------------------------------------------------------------
    // Projection.
    // -----------------------------------------------------------------------

    /// What `viewer` may see: an observer no contribution; a member its own
    /// contributions and nothing of its peer's; the caller only what the
    /// domain has released.
    fn planning_pair_dto(
        &self,
        pair: &PairState,
        viewer: Viewer,
        receipt: Option<(CommandReceiptId, AppliedDto)>,
    ) -> Result<PlanningPairRunDto, ApiError> {
        let state = self.state()?;
        let run = &pair.run;
        let ConsultationRunId::PlanningPair(planning_pair_run_id) = run.id else {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this projection requires a planning pair",
            ));
        };
        let container_name = self.consultation_container_name(run)?;
        let members = pair
            .pair
            .members()
            .members()
            .iter()
            .map(|member| {
                let seat = pair
                    .seats
                    .iter()
                    .find(|seat| seat.role_slot_id.as_str() == member.slot.as_str())
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::Unavailable,
                            "a frozen planning pair member has no consultation seat",
                        )
                    })?;
                Ok(PlanningPairMemberDto {
                    slot: PlanningPairSlotDto::of(member.slot),
                    label: member.slot.label().to_owned(),
                    seat_binding_id: seat.seat_binding_id,
                    occupancy_generation: seat.occupancy_generation,
                    binding_key: member.binding_key.clone(),
                    vendor: member.vendor.clone(),
                    model_route: runtime_model_route_dto(&member.route),
                    observed_binding: seat.native_identity.clone().map(|identity| {
                        ObservedBindingDto {
                            runtime_kind: identity.runtime_kind,
                            native_id: identity.native_id,
                            native_name: None,
                            cwd: None,
                            container_readback: None,
                            observed_at: seat.observed_at.unwrap_or(run.updated_at),
                        }
                    }),
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()?;
        let caller = viewer == Viewer::Caller;
        let findings = pair.pair.findings().filter(|_| caller).map(|findings| {
            findings
                .iter()
                .map(|finding| contribution_dto(finding))
                .collect()
        });
        let clarification =
            pair.pair
                .clarification()
                .filter(|_| caller)
                .map(|(request, answers)| PlanningPairClarificationDto {
                    question: request.question.clone(),
                    addressed: request
                        .addressed
                        .iter()
                        .map(|slot| PlanningPairSlotDto::of(*slot))
                        .collect(),
                    answers: answers.into_iter().map(contribution_dto).collect(),
                });
        let disposition = pair
            .pair
            .disposition()
            .filter(|_| caller)
            .map(|disposition| PlanningPairDispositionRecordDto {
                members: disposition
                    .members
                    .iter()
                    .map(|member| PlanningPairMemberDispositionDto {
                        slot: PlanningPairSlotDto::of(member.slot),
                        finding: member.finding.clone(),
                        answer: member.answer.clone(),
                        disposition: advice_disposition_dto(member.disposition),
                    })
                    .collect(),
                rationale: disposition.rationale.clone(),
            });
        let retained_dissent = if caller {
            pair.pair
                .retained_dissent()
                .into_iter()
                .map(contribution_dto)
                .collect()
        } else {
            Vec::new()
        };
        // A member sees its own words, sealed or not, and never its peer's.
        let own_contributions = match viewer {
            Viewer::Member(slot) => {
                let own = |round: PlanningPairRound, list: &[RecordedContribution]| {
                    list.iter()
                        .filter(|contribution| contribution.slot == slot)
                        .map(|contribution| PlanningPairContributionDto {
                            round: round.as_str().to_owned(),
                            slot: PlanningPairSlotDto::of(contribution.slot),
                            advice: contribution.advice.clone(),
                            document_hash: contribution.document_hash.clone(),
                        })
                        .collect::<Vec<_>>()
                };
                let mut own_list = own(PlanningPairRound::Findings, &pair.record.findings);
                if let Some(clarification) = &pair.record.clarification {
                    own_list.extend(own(
                        PlanningPairRound::Clarification,
                        &clarification.answers,
                    ));
                }
                own_list
            }
            Viewer::Caller | Viewer::Observer => Vec::new(),
        };
        let receipt = receipt
            .map(|(receipt_id, applied)| {
                Ok(MutationReceiptDto {
                    realm_id: state.realm_id(),
                    receipt_id: receipt_id.to_string(),
                    applied,
                    revision: run.revision,
                    snapshot_cursor: self.cursor()?,
                })
            })
            .transpose()?;
        Ok(PlanningPairRunDto {
            realm_id: state.realm_id(),
            planning_pair_run_id,
            epic_id: run.mini_project_id,
            task_id: run.subject.and_then(ConsultationSubject::task_id),
            protocol: PlanningPairProtocolDto::PlanningPairV1,
            profile: consultation_revision_dto(&pair.revision),
            topic: run.topic.clone().ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "a planning pair always carries its topic",
                )
            })?,
            question: run.question.clone(),
            caller_seat_binding_id: run.caller_seat_binding_id,
            topology_node_id: run.topology_node_id,
            container_name,
            state: run.state.as_str().to_owned(),
            phase: pair.pair.state().as_str().to_owned(),
            placement_hash: pair.placement.placement.hash().clone(),
            members,
            viewer: match viewer {
                Viewer::Caller => PlanningPairViewerDto::Caller,
                Viewer::Member(_) => PlanningPairViewerDto::Member,
                Viewer::Observer => PlanningPairViewerDto::Observer,
            },
            findings,
            own_contributions,
            clarification,
            disposition,
            retained_dissent,
            record_revision: pair.record_revision,
            revision: run.revision,
            snapshot_cursor: self.cursor()?,
            receipt,
        })
    }
}

fn contribution_dto(contribution: &PlanningPairContribution) -> PlanningPairContributionDto {
    PlanningPairContributionDto {
        round: contribution.round.as_str().to_owned(),
        slot: PlanningPairSlotDto::of(contribution.slot),
        advice: contribution.advice.clone(),
        document_hash: contribution.document_hash.clone(),
    }
}

/// Findings and answers are kept in slot order, whatever order they arrive in.
fn in_slot_order(list: &mut Vec<RecordedContribution>, contribution: RecordedContribution) {
    let at = list
        .iter()
        .position(|kept| kept.slot > contribution.slot)
        .unwrap_or(list.len());
    list.insert(at, contribution);
}

const fn advice_disposition(
    disposition: AdviceDispositionDto,
) -> kontor_core::consultation::AdviceDisposition {
    use kontor_core::consultation::AdviceDisposition;
    match disposition {
        AdviceDispositionDto::Accepted => AdviceDisposition::Accepted,
        AdviceDispositionDto::PartiallyAccepted => AdviceDisposition::PartiallyAccepted,
        AdviceDispositionDto::Rejected => AdviceDisposition::Rejected,
        AdviceDispositionDto::Superseded => AdviceDisposition::Superseded,
    }
}

const fn advice_disposition_dto(
    disposition: kontor_core::consultation::AdviceDisposition,
) -> AdviceDispositionDto {
    use kontor_core::consultation::AdviceDisposition;
    match disposition {
        AdviceDisposition::Accepted => AdviceDispositionDto::Accepted,
        AdviceDisposition::PartiallyAccepted => AdviceDispositionDto::PartiallyAccepted,
        AdviceDisposition::Rejected => AdviceDispositionDto::Rejected,
        AdviceDisposition::Superseded => AdviceDispositionDto::Superseded,
    }
}

/// The fleet provenance one member's launch requests, read from the frozen
/// shared-allocator receipt and never re-derived.
fn planning_pair_fleet_provenance(
    receipt: &serde_json::Value,
    slot: PlanningPairSlot,
) -> Option<kontor_runtime::FleetLaunchProvenance> {
    let selection = receipt.get("selection")?;
    let provenance = selection.get("provenance")?;
    let member = selection
        .get("slots")?
        .as_array()?
        .iter()
        .find(|candidate| {
            candidate.get("slot_id").and_then(serde_json::Value::as_str) == Some(slot.as_str())
        })?;
    let selected = member.get("selected")?;
    let strings = |value: Option<&serde_json::Value>| -> BTreeSet<String> {
        value
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    Some(kontor_runtime::FleetLaunchProvenance {
        policy_hash: ContentHash::parse(provenance.get("policy_hash")?.as_str()?).ok()?,
        source_bundle_hash: provenance
            .get("source_bundle_hash")
            .and_then(serde_json::Value::as_str)
            .and_then(|hash| ContentHash::parse(hash).ok()),
        binding_key: member.get("binding_key")?.as_str()?.to_owned(),
        chain: member.get("chain")?.as_str()?.to_owned(),
        step: u16::try_from(selected.get("step")?.as_u64()?).ok()?,
        sub_step: u16::try_from(selected.get("sub_step")?.as_u64()?).ok()?,
        vendor: selected.get("vendor")?.as_str()?.to_owned(),
        eligibility: Some(kontor_runtime::LaunchEligibility {
            unavailable_accounts: strings(
                member
                    .get("eligibility")
                    .and_then(|value| value.get("unavailable_accounts")),
            ),
            excluded_vendors: strings(
                member
                    .get("eligibility")
                    .and_then(|value| value.get("excluded_vendors")),
            ),
        }),
    })
}
