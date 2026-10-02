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
    ClarificationRequest, ConsultationProtocol, MemberDisposition, PlanningPairActor,
    PlanningPairContribution, PlanningPairDisposition, PlanningPairReadbackRefusal,
    PlanningPairRecord, PlanningPairRound, PlanningPairRun, PlanningPairSlot,
    RecordedClarification, RecordedContribution,
};
use kontor_core::repository::{
    PlanningPairMemberReadback, StoredPlanningPairContribution, StoredPlanningPairKnownNative,
    StoredPlanningPairPlacement, StoredPlanningPairRecord,
};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_fleet_activation::{PlanningPairMemberRequest, PlanningPairRequest};
use kontor_runtime::planning_pair::caller::{self as eligibility, CallerRefusal, FrozenCallerAct};
use kontor_runtime::planning_pair::context::{self as member_context, ContextRefusal};
use kontor_runtime::planning_pair::intent::{self as fingerprint, SeatGeneration};
use kontor_runtime::planning_pair::recovery::{self as member_recovery, RecoveryOutcome};
use refusal::{caller_refusal_rule, context_refusal_rule, recovery_refusal_rule, withdraw_rule};

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
    /// Each member's known native claim at its current occupancy generation,
    /// when a trusted runtime outcome recorded one. Never a qualification.
    known: Vec<StoredPlanningPairKnownNative>,
}

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
        let intent = self.intent(
            &fingerprint::Invoke {
                project_id,
                epic_id,
                profile_id: &request.profile.id,
                profile_version: request.profile.version,
                definition_hash: &request.profile.definition_hash,
                topic: &request.topic,
                question: &request.question,
                task_id: request.task_id,
                members: &members,
                caller: presented(caller),
            }
            .document(),
        )?;
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
        self.planning_pair_receipt_hold_point().await;
        // A request for this key that resumed the run may record alongside
        // this one. Whether this request wrote the receipt is decided where
        // the receipt is written, so exactly one of them answers `created`.
        let (receipt_id, inserted) = self.record_classified(
            key,
            project_id,
            CommandKind::InvokePlanningPairRun,
            target,
            run.revision,
            &intent,
        )?;
        let applied = if inserted {
            AppliedDto::Created
        } else {
            AppliedDto::Unchanged
        };
        let pair = self.planning_pair_state(&run)?;
        self.planning_pair_dto(&pair, Viewer::Caller, Some((receipt_id, applied)))
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
        // The two actual placed routes must each have the closed member
        // surface on this runtime before anything is frozen.
        self.require_planning_pair_member_routes(
            self.planning_pair_runtime()?.as_ref(),
            members.members(),
        )?;
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
        let adapter = self.planning_pair_runtime()?;
        self.require_planning_pair_member_routes(adapter.as_ref(), pair.pair.members().members())?;
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
            // A member whose launch was already read back keeps that exact
            // session: a replay or a restart meets it again from its durable
            // claim, with no runtime call and no second create. Only the
            // caller's recovery can requalify it.
            if let Some(known) = pair
                .known
                .iter()
                .find(|known| known.seat_binding_id == seat.seat_binding_id)
            {
                return Err(self.unqualified_member(&known.identity, known.readback_refusal));
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
            let fleet_provenance = member_context::requested_fleet_provenance(&receipt, slot);
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
            let context = self.planning_pair_launch_context(
                run,
                &pair,
                seat,
                slot,
                &node,
                &cwd,
                &catalog,
                fleet_provenance.as_ref(),
            )?;
            let context_hash = self.member_context_hash(&context)?;
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
                    planning_pair: Some(context),
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
            // A member is qualified — bound, so it can contribute — only when
            // its launch reported a member-surface observation, every mandatory
            // field of it matched, and its readback observed exactly its
            // requested provenance. Whatever the readback proved, the exact
            // session it named is kept first as the member's known native
            // claim, in the same transaction as the bind when it qualified.
            // Any missing, wrong or unsupported field is then a typed
            // no-observation refusal: the session is kept, unbound and named,
            // its credential is unqualified, the pair stays materializing with
            // no receipt, and a replay meets that same claim rather than
            // creating, replacing or destroying a session.
            let refusal = kontor_runtime::planning_pair::qualify_member_readback(
                &outcome,
                fleet_provenance.as_ref(),
            )
            .err();
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
            state
                .with_store(|store| store.record_planning_pair_member_launch(&claim))
                .map_err(|error| self.refuse(&error))?;
            if let Some(refusal) = refusal {
                return Err(self.unqualified_member(&outcome.identity, Some(refusal)));
            }
            seat.native_identity = Some(outcome.identity);
            seat.provider_session_id = outcome.provider_session_id;
            seat.observed_at = Some(outcome.observed_at);
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
        let run = self.stored_planning_pair(project_id, run_id)?;
        let mut pair = self.planning_pair_state(&run)?;
        // Authentication before replay: a fenced member never replays.
        let slot = self.authenticated_member(&pair, member, true)?;
        let kind = match round {
            PlanningPairRound::Findings => CommandKind::RecordPlanningPairFinding,
            PlanningPairRound::Clarification => CommandKind::RecordPlanningPairAnswer,
        };
        let intent = self.intent(
            &fingerprint::Contribution {
                project_id,
                run_id: run.id,
                round,
                slot,
                advice: &request.advice,
                member: presented(member),
            }
            .document(),
        )?;
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
        let intent = self.intent(
            &fingerprint::Clarification {
                project_id,
                run_id: run.id,
                question: &request.question,
                addressed: &addressed,
                caller: presented(caller),
            }
            .document(),
        )?;
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
        let intent = self.intent(
            &fingerprint::Disposition {
                project_id,
                run_id: run.id,
                disposition: &disposition,
                caller: presented(caller),
            }
            .document(),
        )?;
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
        let _native_activity = self.native_activity()?;
        let run = self.stored_planning_pair(project_id, run_id)?;
        let pair = self.planning_pair_state(&run)?;
        self.authenticated_recovering_caller(&run, &pair.spec, caller)?;
        let seat = pair
            .seats
            .iter()
            .find(|seat| seat.seat_binding_id == seat_binding_id)
            .cloned()
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the planning pair has no such member seat",
                )
            })?;
        let slot = PlanningPairSlot::parse(seat.role_slot_id.as_str())
            .map_err(|error| self.refuse_domain(&error))?;
        let context = self.planning_pair_member_context(&run, &pair, &seat, slot)?;
        let context_hash = self.member_context_hash(&context)?;
        let expected = &request.expected_native_identity;
        let expected_identity = NativeRuntimeIdentity {
            runtime_kind: expected.runtime_kind.clone(),
            host: expected.host.clone(),
            generation: expected.generation,
            native_id: expected.native_id.clone(),
        };
        let placement_hash = pair.placement.placement.hash().clone();
        let intent = self.intent(
            &fingerprint::Recovery {
                project_id,
                run_id: run.id,
                member_seat_binding_id: seat_binding_id,
                slot,
                caller: presented(caller),
                expected_run_revision: request.expected_run_revision,
                expected_member_occupancy_generation: request.expected_member_occupancy_generation,
                expected_native_identity: &expected_identity,
                expected_provider_session_id: request.expected_provider_session_id.as_ref(),
                member_context_hash: &context_hash,
                placement_hash: &placement_hash,
            }
            .document(),
        )?;
        let target = AggregateRef::MiniProject {
            mini_project_id: run.mini_project_id,
        };
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            return self.planning_pair_recovery_dto(
                &pair,
                &seat,
                slot,
                &expected_identity,
                request.expected_provider_session_id.as_ref(),
                receipt.target_revision,
                (receipt.id, AppliedDto::Unchanged),
            );
        }
        member_recovery::require_recoverable(run.state)
            .map_err(|error| self.refuse_domain(&error))?;
        self.expect_planning_pair_revision(&run, request.expected_run_revision)?;
        // The known session is the bound seat's or else its kept claim, never
        // one the request names: the body only asserts it.
        let plan = member_recovery::plan(
            member_recovery::RecoveryFacts {
                run: &run,
                seat: &seat,
                known: &pair.known,
                context_hash: &context_hash,
                placement_hash: &placement_hash,
            },
            member_recovery::RecoveryAssertion {
                member_occupancy_generation: request.expected_member_occupancy_generation,
                native_identity: &expected_identity,
                provider_session_id: request.expected_provider_session_id.as_ref(),
            },
        )
        .map_err(|refusal| {
            let (code, rule) = recovery_refusal_rule(refusal);
            self.deny(code, rule)
        })?;
        // Every real route is refused here, before any runtime effect.
        let state = self.state()?;
        let adapter = self.planning_pair_runtime()?;
        self.require_planning_pair_member_routes(adapter.as_ref(), pair.pair.members().members())?;
        let readback_request = kontor_runtime::planning_pair::PlanningPairMemberReconcileRequest {
            context,
            identity: plan.identity().clone(),
            requested_at: kontor_api::now(),
        };
        let answer = adapter
            .reconcile_planning_pair_member(&readback_request)
            .await;
        let outcome = match plan.outcome(&readback_request, answer) {
            RecoveryOutcome::Requalify(outcome) => *outcome,
            RecoveryOutcome::NoObservation(error) => {
                return Err(ApiError::from_runtime(state.realm_id(), &error));
            }
            RecoveryOutcome::Withdraw(reason) => {
                // Only this member's current qualification is withdrawn, and
                // only if it held one: its claim, its peer and every finding
                // stay, and a running pair needs a human.
                if plan.holds_qualification() {
                    state
                        .with_store(|store| {
                            store.disqualify_planning_pair_member(&PlanningPairMemberReadback {
                                project_id,
                                run_id: run.id,
                                seat_binding_id,
                                expected_revision: run.revision,
                                occupancy_generation: seat.occupancy_generation,
                                verified: plan.verified().clone(),
                                applied_at: kontor_api::now(),
                            })
                        })
                        .map_err(|error| self.refuse_recovery_write(&error))?;
                }
                return Err(self
                    .deny(ApiErrorCode::Unavailable, withdraw_rule(reason))
                    .about("planning pair member readback")
                    .located_at(format!("native/{}", plan.identity().native_id.as_str()))
                    .advising("confirmation unknown: the member's known native session is kept and is not qualified now; nothing was created, replaced or archived"));
            }
        };
        self.planning_pair_recovery_hold_point().await;
        let now = kontor_api::now();
        let next = run
            .revision
            .next()
            .map_err(|error| self.refuse_domain(&error))?;
        let readback = PlanningPairMemberReadback {
            project_id,
            run_id: run.id,
            seat_binding_id,
            expected_revision: run.revision,
            occupancy_generation: seat.occupancy_generation,
            verified: plan.requalified(&outcome),
            applied_at: now,
        };
        let envelope = ReceiptEnvelope::new(
            state.realm_id(),
            NewLocalCommand {
                project_id,
                receipt_id: CommandReceiptId::generate(),
                idempotency_key: key.clone(),
                kind: CommandKind::RecoverPlanningPairSeat,
                target,
                target_revision: next,
                intent: intent.clone(),
                created_at: now,
            },
        );
        // The qualification and its receipt are one compare-and-swap: a
        // concurrent request for this key answers the receipt the winner
        // wrote, and any other write that moved the run refuses this one.
        let (after, receipt_id, created) = state
            .with_store(|store| store.requalify_planning_pair_member(&readback, &envelope))
            .map_err(|error| self.refuse_recovery_write(&error))?;
        state.signals().appended();
        let pair = self.planning_pair_state(&after)?;
        self.planning_pair_recovery_dto(
            &pair,
            &seat,
            slot,
            plan.identity(),
            outcome.provider_session_id.as_ref(),
            next,
            (
                receipt_id,
                if created {
                    AppliedDto::Created
                } else {
                    AppliedDto::Unchanged
                },
            ),
        )
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

    /// The canonical hash of one member's frozen launch context. Nothing in it
    /// is secret.
    fn member_context_hash(
        &self,
        context: &kontor_runtime::planning_pair::PlanningPairLaunchContext,
    ) -> Result<ContentHash, ApiError> {
        context
            .frozen_hash()
            .map_err(|error| self.refuse_domain(&error))
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
