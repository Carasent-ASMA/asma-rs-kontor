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

mod commands;
mod invocation;
mod refusal;

use super::*;
use kontor_api::applications::AdviceDispositionDto;
use kontor_api::planning_pair::{
    InvokePlanningPairRequest, PlanningPairClarificationDto, PlanningPairContributionDto,
    PlanningPairDispositionRecordDto, PlanningPairKnownNativeDto, PlanningPairMemberDispositionDto,
    PlanningPairMemberDto, PlanningPairNativeIdentityDto, PlanningPairProtocolDto,
    PlanningPairReader, PlanningPairRunDto, PlanningPairSeat, PlanningPairSeatRecoveryDto,
    PlanningPairSlotDto, PlanningPairViewerDto, RecordPlanningPairContributionRequest,
    RecordPlanningPairDispositionRequest, RecoverPlanningPairSeatRequest,
    RequestPlanningPairClarificationRequest,
};
use kontor_core::id::PlanningPairRunId;
use kontor_core::planning_pair::{
    MemberDisposition, PlanningPairContribution, PlanningPairDisposition,
    PlanningPairReadbackRefusal, PlanningPairRecord, PlanningPairRound, PlanningPairRun,
    PlanningPairSlot, RecordedContribution,
};
use kontor_core::repository::{
    PlanningPairMemberReadback, StoredPlanningPairContribution, StoredPlanningPairKnownNative,
    StoredPlanningPairPlacement, StoredPlanningPairRecord,
};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_fleet_activation::{PlanningPairMemberRequest, PlanningPairRequest};
use kontor_runtime::planning_pair::application::commands::{
    contribution as contribution_sequence, recovery as recovery_sequence,
};
use kontor_runtime::planning_pair::application::{
    self as invocation_sequence, InvokeInput, InvokeRefusal, PairState,
};
use kontor_runtime::planning_pair::caller::{self as eligibility, CallerRefusal, FrozenCallerAct};
use kontor_runtime::planning_pair::context::{self as member_context, ContextRefusal};
use kontor_runtime::planning_pair::intent::{self as fingerprint, SeatGeneration};
use refusal::{
    caller_refusal_rule, context_refusal_rule, invoke_refusal_rule, recovery_refusal_rule,
    withdraw_rule,
};

/// A black-box test's hold before a planning pair invocation receipt is
/// written: every invocation that reaches the write waits until released.
///
/// It needs no executor, and it changes only when the receipt is written,
/// never what is written or whether. No composed daemon installs one.
#[doc(hidden)]
#[derive(Debug, Clone, Default)]
pub struct PlanningPairReceiptHold {
    state: std::sync::Arc<std::sync::Mutex<ReceiptHoldState>>,
}

#[derive(Debug, Default)]
struct ReceiptHoldState {
    waiting: usize,
    released: bool,
    held: Vec<std::task::Waker>,
}

impl PlanningPairReceiptHold {
    /// How many invocations have reached the receipt write so far.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.lock().waiting
    }

    /// Let every held and every later invocation write its receipt.
    pub fn release(&self) {
        let mut state = self.lock();
        state.released = true;
        for waker in state.held.drain(..) {
            waker.wake();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ReceiptHoldState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    async fn pass(&self) {
        self.lock().waiting += 1;
        std::future::poll_fn(|context| {
            let mut state = self.lock();
            if state.released {
                std::task::Poll::Ready(())
            } else {
                state.held.push(context.waker().clone());
                std::task::Poll::Pending
            }
        })
        .await;
    }
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
        let PlanningPairProtocolDto::PlanningPairV1 = request.protocol;
        let members: Vec<fingerprint::InvokeMember<'_>> = request
            .members
            .iter()
            .map(|member| fingerprint::InvokeMember {
                slot: member.slot.slot(),
                binding_key: &member.binding_key,
                unavailable_accounts: &member.unavailable_accounts,
                excluded_vendors: &member.excluded_vendors,
            })
            .collect();
        invocation_sequence::invoke(
            &invocation::InvokePorts(self),
            key,
            project_id,
            epic_id,
            caller,
            &InvokeInput {
                profile_id: &request.profile.id,
                profile_version: request.profile.version,
                definition_hash: &request.profile.definition_hash,
                topic: &request.topic,
                question: &request.question,
                task_id: request.task_id,
                members: &members,
                expected_revision: request.expected_revision,
            },
        )
        .await
    }

    /// Hold every invocation that reaches its receipt write, for a black-box
    /// test that must line two requests up after the run's compare-and-swap
    /// and before either receipt exists.
    #[doc(hidden)]
    pub fn hold_planning_pair_invocation_receipts(&self) -> PlanningPairReceiptHold {
        let hold = PlanningPairReceiptHold::default();
        *self
            .planning_pair_receipt_hold
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hold.clone());
        hold
    }

    /// Wait at an installed test hold; with none installed, return at once.
    async fn planning_pair_receipt_hold_point(&self) {
        let hold = self
            .planning_pair_receipt_hold
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(hold) = hold {
            hold.pass().await;
        }
    }

    /// Whether the runtime planning pair members are placed on composes the
    /// closed member surface for both actual frozen routes, asked without a
    /// native effect (D-3). A route whose provider it cannot compose is
    /// refused with that provider named; nothing is substituted.
    fn require_planning_pair_member_routes(
        &self,
        adapter: &dyn kontor_runtime::adapter::RuntimeAdapter,
        members: &[kontor_core::planning_pair::PlanningPairMember],
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let routes: Vec<kontor_runtime::planning_pair::PlanningPairMemberRoute> = members
            .iter()
            .map(
                |member| kontor_runtime::planning_pair::PlanningPairMemberRoute {
                    slot: member.slot,
                    model_rung: member.route.clone(),
                },
            )
            .collect();
        adapter
            .validate_planning_pair_member_surface(&routes)
            .map_err(|error| match &error {
                kontor_runtime::RuntimeError::PlanningPairMemberSurfaceUnsupported {
                    provider,
                    gap,
                } => self
                    .deny(
                        ApiErrorCode::UnsupportedCapability,
                        "this runtime cannot establish the closed planning pair member surface for a member route",
                    )
                    .about("planning pair member route")
                    .located_at(format!("providers/{provider}/{}", gap.as_str())),
                _ => ApiError::from_runtime(state.realm_id(), &error),
            })
    }

    /// The runtime planning pair members are placed on.
    fn planning_pair_runtime(
        &self,
    ) -> Result<std::sync::Arc<dyn kontor_runtime::adapter::RuntimeAdapter>, ApiError> {
        let state = self.state()?;
        let runtime_kind = self.node_runtime_kind()?;
        state.runtimes().get(&runtime_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime selected for planning pair placement is not configured",
            )
        })
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
        let scope = match task_id {
            Some(task_id) => eligibility::InvocationScope::Ticket {
                ticket_epic: self.task_row(project_id, task_id)?.mini_project_id,
            },
            None => eligibility::InvocationScope::Epic,
        };
        eligibility::require_invocation_scope(spec, epic_id, scope)
            .map_err(|refusal| self.caller_refusal(refusal))?;
        let seat = self.active_epic_seat(project_id, epic_id, caller)?;
        eligibility::require_caller_role(spec, &seat)
            .map_err(|refusal| self.caller_refusal(refusal))?;
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
            .map_err(|error| self.refuse(&error))?;
        let seat = eligibility::require_active_seat(seat)
            .map_err(|refusal| self.caller_refusal(refusal))?;
        let node = state
            .with_store(|store| store.get_topology_node(project_id, seat.topology_node_id))
            .map_err(|error| self.refuse(&error))?;
        eligibility::require_active_epic_node(node.as_ref(), epic_id)
            .map_err(|refusal| self.caller_refusal(refusal))?;
        let current = state
            .with_store(|store| {
                store.hosted_topology_seat_occupancy_generation(project_id, caller.seat_binding_id)
            })
            .map_err(|error| self.refuse(&error))?;
        eligibility::require_current_generation(caller.occupancy_generation, current)
            .map_err(|refusal| self.caller_refusal(refusal))?;
        Ok(seat)
    }

    /// The frozen context one member launch is held to, derived from durable
    /// state and checked against it before any native effect (D-3).
    ///
    /// The occupancy generation is the frozen seat's own, which is also the
    /// generation the member's credential is minted for; nothing here reads
    /// the credential, defaults a generation or takes a caller's label.
    #[allow(
        clippy::too_many_arguments,
        reason = "every frozen input the context is derived from is passed explicitly"
    )]
    fn planning_pair_launch_context(
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
        let refuse = |refusal| self.context_refusal(refusal);
        let frozen = member_context::frozen_member(
            run,
            pair.pair.pin(),
            pair.pair.members(),
            pair.placement.placement.hash(),
            seat,
            slot,
        )
        .map_err(refuse)?;
        let pinned = self
            .state()?
            .with_store(|store| {
                store.get_mini_project_team_definition(run.project_id, run.mini_project_id)
            })
            .map_err(|error| self.refuse(&error))?
            .map(|pin| pin.definition);
        let team_definition = frozen.require_team_definition(pinned).map_err(refuse)?;
        member_context::require_container_topology(
            &self.epic_pin(run.project_id, run.mini_project_id)?,
            node,
        )
        .map_err(refuse)?;
        frozen
            .into_context(team_definition, node, cwd, catalog, requested)
            .map_err(|error| match error {
                member_context::ContextBuildError::Refused(refusal) => {
                    self.context_refusal(refusal)
                }
                member_context::ContextBuildError::Domain(error) => self.refuse_domain(&error),
            })
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
        let mut known = Vec::new();
        for seat in &seats {
            if let Some(claim) = state
                .with_store(|store| {
                    store.planning_pair_known_native(
                        run.project_id,
                        run.id,
                        seat.seat_binding_id,
                        seat.occupancy_generation,
                    )
                })
                .map_err(|error| self.refuse(&error))?
            {
                known.push(claim);
            }
        }
        Ok(PairState {
            run: run.clone(),
            revision,
            spec,
            placement,
            record_revision: stored.revision,
            record,
            pair,
            seats,
            known,
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
        contribution_sequence::record(
            &commands::CommandPorts(self),
            key,
            project_id,
            run_id,
            member,
            round,
            &request.advice,
            request.expected_revision,
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
        let addressed: Vec<PlanningPairSlot> =
            request.addressed.iter().map(|slot| slot.slot()).collect();
        contribution_sequence::clarify(
            &commands::CommandPorts(self),
            key,
            project_id,
            run_id,
            caller,
            &request.question,
            &addressed,
            request.expected_revision,
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
        contribution_sequence::decide(
            &commands::CommandPorts(self),
            key,
            project_id,
            run_id,
            caller,
            &disposition,
            request.expected_revision,
        )
    }

    // -----------------------------------------------------------------------
    // Same-native member recovery (frontier A).
    // -----------------------------------------------------------------------

    /// Requalify one member on its exact known native session, as the pair's
    /// frozen caller (ASMA-8282 frontier A).
    ///
    /// The caller is authenticated first, at its current hosted generation and
    /// under the pinned document's roles and scopes, so a retired credential,
    /// a member, another seat or an ambient tier never reaches a replay or the
    /// runtime. An exact replay answers its original receipt with no runtime
    /// call. A new request is held to the run revision, the member's current
    /// generation and its known session (bound, or kept as its claim), every
    /// real route is refused before any effect, and the same session is read
    /// back again through the opt-in reconcile seam: no credential, no new
    /// generation, launch, replacement, archive, reallocation or reroute.
    ///
    /// A readback that qualifies the member binds that session and writes the
    /// receipt in one compare-and-swap. An adverse one withdraws only this
    /// member's current qualification, keeps its claim and every finding, and
    /// is refused.
    pub(super) async fn recover_planning_pair_seat(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        run_id: PlanningPairRunId,
        seat_binding_id: SeatBindingId,
        caller: PlanningPairSeat,
        request: &RecoverPlanningPairSeatRequest,
    ) -> Result<PlanningPairSeatRecoveryDto, ApiError> {
        let expected = &request.expected_native_identity;
        let expected_identity = NativeRuntimeIdentity {
            runtime_kind: expected.runtime_kind.clone(),
            host: expected.host.clone(),
            generation: expected.generation,
            native_id: expected.native_id.clone(),
        };
        recovery_sequence::recover(
            &commands::CommandPorts(self),
            key,
            project_id,
            run_id,
            seat_binding_id,
            caller,
            recovery_sequence::Input {
                expected_revision: request.expected_run_revision,
                expected_member_generation: request.expected_member_occupancy_generation,
                expected_identity: &expected_identity,
                expected_provider_session_id: request.expected_provider_session_id.as_ref(),
            },
        )
        .await
    }

    /// Hold every member recovery that reaches its compare-and-swap, for a
    /// black-box test that must line requests up after their readback and
    /// before any of them writes.
    #[doc(hidden)]
    pub fn hold_planning_pair_recovery_writes(&self) -> PlanningPairReceiptHold {
        let hold = PlanningPairReceiptHold::default();
        *self
            .planning_pair_recovery_hold
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hold.clone());
        hold
    }

    /// Wait at an installed recovery hold; with none installed, return at once.
    async fn planning_pair_recovery_hold_point(&self) {
        let hold = self
            .planning_pair_recovery_hold
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(hold) = hold {
            hold.pass().await;
        }
    }

    /// A recovery write storage refused: a run that moved is a revision
    /// conflict, a member whose generation moved a stale binding.
    fn refuse_recovery_write(&self, error: &RepositoryError) -> ApiError {
        match error {
            RepositoryError::Conflict {
                subject: "consultation run",
                ..
            } => self.deny(
                ApiErrorCode::RevisionConflict,
                "the planning pair moved since the recovery was prepared",
            ),
            RepositoryError::Conflict {
                subject: "planning pair member",
                ..
            } => self.deny(
                ApiErrorCode::StaleBinding,
                "the member's occupancy generation moved since the recovery was prepared",
            ),
            other => self.refuse(other),
        }
    }

    /// The typed refusal for a member whose exact known native session is
    /// kept but is not qualified.
    fn unqualified_member(
        &self,
        identity: &NativeRuntimeIdentity,
        refusal: Option<PlanningPairReadbackRefusal>,
    ) -> ApiError {
        self.deny(ApiErrorCode::Unavailable, readback_refusal_rule(refusal))
            .about("planning pair member readback")
            .located_at(format!("native/{}", identity.native_id.as_str()))
            .advising("confirmation unknown: the member's native session is kept unqualified; its caller may requalify that same session with kontor_planning_pair_seat_recover")
    }

    /// The authenticated recovering caller: the pair's exact frozen caller
    /// seat at its current hosted generation, held to the pinned document's
    /// caller roles and scopes.
    fn authenticated_recovering_caller(
        &self,
        run: &StoredConsultationRun,
        spec: &PlanningPairSpec,
        caller: PlanningPairSeat,
    ) -> Result<(), ApiError> {
        eligibility::require_frozen_caller(
            run.caller_seat_binding_id,
            caller.seat_binding_id,
            FrozenCallerAct::Recover,
        )
        .map_err(|refusal| self.caller_refusal(refusal))?;
        self.authorize_planning_pair_caller(
            run.project_id,
            run.mini_project_id,
            run.subject.and_then(ConsultationSubject::task_id),
            spec,
            caller,
        )?;
        Ok(())
    }

    /// One member's frozen context, derived from durable state and validated
    /// against it exactly as its launch was: its catalog role, pins, route,
    /// generation and placement.
    fn planning_pair_member_context(
        &self,
        run: &StoredConsultationRun,
        pair: &PairState,
        seat: &StoredConsultationSeat,
        slot: PlanningPairSlot,
    ) -> Result<kontor_runtime::planning_pair::PlanningPairLaunchContext, ApiError> {
        let state = self.state()?;
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
        let epic_key = self.epic_tracker_key(run.project_id, run.mini_project_id)?;
        let cwd = self.consultation_root(project.root_path.as_str(), epic_key.as_ref(), node.id)?;
        let catalog = self.epic_role_catalog(run.project_id, run.mini_project_id)?;
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
        let receipt: serde_json::Value = serde_json::from_str(pair.placement.placement.json())
            .map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the frozen planning pair placement could not be decoded",
                )
            })?;
        let fleet_provenance = member_context::requested_fleet_provenance(&receipt, slot);
        self.planning_pair_launch_context(
            run,
            pair,
            seat,
            slot,
            &node,
            &cwd,
            &catalog,
            fleet_provenance.as_ref(),
        )
    }

    /// One recovery's answer: the pair as its caller now sees it, the member's
    /// unchanged seat, generation and session, and the receipt.
    #[allow(
        clippy::too_many_arguments,
        reason = "every durable fact the answer reports is passed explicitly"
    )]
    fn planning_pair_recovery_dto(
        &self,
        pair: &PairState,
        seat: &StoredConsultationSeat,
        slot: PlanningPairSlot,
        identity: &NativeRuntimeIdentity,
        provider_session_id: Option<&ExternalId>,
        recovered_revision: AggregateRevision,
        receipt: (CommandReceiptId, AppliedDto),
    ) -> Result<PlanningPairSeatRecoveryDto, ApiError> {
        let state = self.state()?;
        Ok(PlanningPairSeatRecoveryDto {
            planning_pair: self.planning_pair_dto(pair, Viewer::Caller, None)?,
            seat_binding_id: seat.seat_binding_id,
            slot: PlanningPairSlotDto::of(slot),
            member_occupancy_generation: seat.occupancy_generation,
            native_identity: native_identity_dto(identity),
            provider_session_id: provider_session_id.cloned(),
            placement_hash: pair.placement.placement.hash().clone(),
            recovered_revision,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt.0.to_string(),
                applied: receipt.1,
                revision: recovered_revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    /// One frozen-caller eligibility refusal, answered exactly as before.
    const fn caller_refusal(&self, refusal: CallerRefusal) -> ApiError {
        let (code, rule) = caller_refusal_rule(refusal);
        self.deny(code, rule)
    }

    /// One invocation-sequence refusal, answered exactly as before.
    fn invoke_refusal(&self, refusal: InvokeRefusal) -> ApiError {
        let (code, rule) = invoke_refusal_rule(&refusal);
        let denied = self.deny(code, rule);
        match refusal {
            InvokeRefusal::EpicMoved { current } => denied.with_revision(Some(current)),
            InvokeRefusal::SemanticDuplicate { existing } => denied
                .about("consultation semantic identity")
                .located_at(format!("consultation-runs/{}", existing.as_text()))
                .advising("read or resume the existing consultation run"),
            InvokeRefusal::NoCompletePlacement => denied.about("FleetActivation"),
            InvokeRefusal::MemberKeptUnqualified { identity, refusal } => {
                self.unqualified_member(&identity, refusal)
            }
            _ => denied,
        }
    }

    /// One frozen-context refusal, answered exactly as before.
    const fn context_refusal(&self, refusal: ContextRefusal) -> ApiError {
        self.deny(
            ApiErrorCode::PlacementBlocked,
            context_refusal_rule(refusal),
        )
    }

    /// The exact frozen caller, at its current hosted generation.
    fn authenticated_caller(
        &self,
        run: &StoredConsultationRun,
        caller: PlanningPairSeat,
    ) -> Result<(), ApiError> {
        eligibility::require_frozen_caller(
            run.caller_seat_binding_id,
            caller.seat_binding_id,
            FrozenCallerAct::Decide,
        )
        .map_err(|refusal| self.caller_refusal(refusal))?;
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
                    known_native: pair
                        .known
                        .iter()
                        .find(|known| known.seat_binding_id == seat.seat_binding_id)
                        .map(|known| PlanningPairKnownNativeDto {
                            native_identity: native_identity_dto(&known.identity),
                            provider_session_id: known.provider_session_id.clone(),
                            readback_refusal: known
                                .readback_refusal
                                .map(|refusal| refusal.as_str().to_owned()),
                            observed_at: known.observed_at,
                        })
                        .or_else(|| {
                            seat.native_identity.as_ref().map(|identity| {
                                PlanningPairKnownNativeDto {
                                    native_identity: native_identity_dto(identity),
                                    provider_session_id: seat.provider_session_id.clone(),
                                    readback_refusal: None,
                                    observed_at: seat.observed_at.unwrap_or(run.updated_at),
                                }
                            })
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

/// The seat and generation a scoped credential authenticated, as a
/// fingerprint names it.
const fn presented(seat: PlanningPairSeat) -> SeatGeneration {
    SeatGeneration {
        seat_binding_id: seat.seat_binding_id,
        occupancy_generation: seat.occupancy_generation,
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

/// The stable rule a member readback refusal is answered with. A member
/// whose known claim records no refusal had qualified and since lost it.
const fn readback_refusal_rule(refusal: Option<PlanningPairReadbackRefusal>) -> &'static str {
    match refusal {
        Some(PlanningPairReadbackRefusal::NoMemberSurface) => {
            "the planning pair member's launch reported no member-surface observation"
        }
        Some(PlanningPairReadbackRefusal::CorrelationUnobserved) => {
            "the planning pair member's readback did not observe its correlation"
        }
        Some(PlanningPairReadbackRefusal::RouteUnobserved) => {
            "the planning pair member's readback did not observe its route"
        }
        Some(PlanningPairReadbackRefusal::ToolRestrictionUnobserved) => {
            "the planning pair member's readback did not observe its closed tool restriction"
        }
        Some(PlanningPairReadbackRefusal::ProvenanceUnconfirmed) => {
            "the planning pair member's native readback did not confirm its frozen provenance, so it is not qualified to contribute"
        }
        None => "the planning pair member's known native session is not currently qualified",
    }
}

fn native_identity_dto(identity: &NativeRuntimeIdentity) -> PlanningPairNativeIdentityDto {
    PlanningPairNativeIdentityDto {
        runtime_kind: identity.runtime_kind.clone(),
        host: identity.host.clone(),
        generation: identity.generation,
        native_id: identity.native_id.clone(),
    }
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
