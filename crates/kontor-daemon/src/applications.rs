//! The composed application services behind the public operations.
//!
//! This is the other half of the seam `kontor-api` declares. The HTTP layer knows
//! the shape of every operation and none of the decisions inside one; this module
//! knows the decisions and nothing about HTTP. It is the only place where the
//! store, the bundled profile pack, the team layer, the scheduler and the runtime
//! adapters are held in the same hand — which is exactly why it is in the
//! composition root and not in the transport.
//!
//! # The rule every method here keeps
//!
//! An answer means the work happened. Not that an intent was recorded, not that a
//! dispatcher will get to it: the row is written, the authorization exists, the
//! runtime agreed. Where an operation cannot honestly say that — because a
//! runtime is unreachable, or reconciliation has not finished — it refuses with a
//! typed code rather than reporting a success it cannot evidence.
//!
//! # And the one it keeps about seats
//!
//! A native session is created in exactly one place in this file: inside
//! [`Services::start`], after `admit_candidate` has committed. Admission commits
//! first because a crash between the two must leave a run with no session (which
//! reconciliation can see and finish) rather than a session with no run (which
//! nothing can). There is no method here, and no route above it, that creates a
//! session any other way.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use kontor_accounts::{AccountAvailability, AdaptivePosition, CapacityReading, ProbeOutcome};
use kontor_api::applications::{
    AbandonRunRequest, AbandonedRunDto, AccountProfileDto, ApplicationOperations, AppliedDto,
    AppliedEpicDto, AppliedLinkDto, AppliedTaskDto, ApplyEpicRequest, ArmRequest,
    AuthorizationProjectionDto, BlockedTaskDto, DisarmRequest, EnsureAccountProfileRequest,
    EnsureProjectRequest, EpicProjectionDto, EpicTaskProjectionDto, LifecycleAction,
    LifecycleOutcomeDto, LifecycleRequest, ModelCatalogDto, ProjectDto, PublishedTeamRevisionDto,
    ReadyTaskDto, RevisionRefDto, RuntimeCapabilityDto, SchedulerPlanDto, SchedulerStartDto,
    SeatProjectionDto, StartRequest, StartedSeatDto, TeamDraftDto, TeamDraftRequest,
    TeamDraftSlotDto, TeamRunProjectionDto, TeamTemplateCatalogDto, TeamsProjectionDto,
    WorkProfileCatalogDto,
};
use kontor_api::applications::{
    AccountAvailabilityDto, AdaptiveWindowDto, AvailabilityOverrideDto,
    AvailabilityOverrideRequest, CapacityCeilingsDto, CapacityConfigurationDto,
    CapacityConfigurationPreviewDto, CapacityConfigurationRequest, CapacityObservationDto,
    CapacityRefreshRequest, MutationReceiptDto, ObservedBindingDto, ProjectCapacityDto,
    ResolvedRoleRefDto, SeatBindingOutcomeDto, SeatBindingRequest, TopologySeatDto,
};
use kontor_api::applications::{
    AdvanceCompletionRequest, AdvisorRunDto, AppliedProfileDto, CloseoutEvidenceDto,
    CloseoutRequirementDto, CommitteeRunDto, CommitteeVerdictDto, CompletionBlockerDto,
    CompletionOutcomeDto, CompletionPhaseDto, CompletionRoundDto, CompletionStateDto,
    CompletionWakeDto, CoreTeamApplyRequest, CoreTeamDto, CoreTeamMaterializeRequest,
    CoreTeamOutcomeDto, CoreTeamPreviewDto, CoreTeamPreviewRequest, CoreTeamSeatDto,
    CoreTeamSeatSelectionDto, DeliberationStepDto, EnsureQuickSessionRequest, IntegrationRecordDto,
    InvokeConsultationRequest, NeedsHumanDto, ProfileApplyRequest, ProfileCatalogDto,
    ProfilePreviewDto, ProfilePreviewRequest, ProfileRevisionDto, PromotedSessionDto,
    PromotionApplyRequest, PromotionPreviewDto, QuickRolesDto, QuickSessionDto,
    RecordFindingsRequest, RemediateCompletionRequest, RemediationActionDto, RepositoryOutcomeDto,
    RosterUpgradePreviewDto, RosterUpgradePreviewRequest, SettleConsultationRequest,
};
use kontor_api::applications::{
    AppliedTopologyUpgradeDto, CodeHelpEntryDto, DesiredBindingDto, PinnedSpecDto,
    SemanticTopologyRequest, SemanticTopologyTargetDto, ShareabilityDto, TopologyMutationDto,
    TopologyNodeDto, TopologyNodeRequest, TopologyProjectionDto, TopologyUpgradeApplyRequest,
    TopologyUpgradeEffectDto, TopologyUpgradePreviewDto, TopologyUpgradePreviewRequest,
};
use kontor_api::applications::{
    AttestLateHandoffRequest, ConnectorSpecDto, IntakeReceiptDto, LateHandoffAttestationDto,
    ProfileArtifactDto, ProfileHandoffDto, ProfilePackDto, ProfilePhaseDto, ProfileValidationDto,
    RegisterPackRequest, ReplaceSeatRequest, ReplacedSeatDto, ResolveConflictRequest,
    RoleSlotWaiverDto, SettleTurnRequest, SettledTurnDto, SubmitIntakeRequest, TicketClaimDto,
    TicketCommentDto, TicketCommentPullDto, TicketConflictDto, TriggerSpecDto, TurnFollowUpDto,
    WaiveRoleSlotRequest, WorkProfileDetailDto,
};
use kontor_api::applications::{
    CodeHelpProjectionDto, DraftTopologySpecRequest, PublishTopologySpecRequest,
    PublishedTopologySpecDto, RoleCatalogDto, RoleCatalogEntryDto, TopologySpecCandidateDto,
    TopologySpecDocumentDto, TopologySpecValidationDto, ValidateTopologySpecRequest,
};
use kontor_api::applications::{
    GateProjectionDto, GateVerdictDto, ProvenanceDto, RecordGateRequest, RedactionDto,
    ResolveContextRequest, ResolvedContextDto, RuntimeSettlementDto, SelectionDto,
    SelectionRequest, TicketFieldDiffDto, TicketReconcileAppliedDto, TicketReconcileApplyRequest,
    TicketReconcilePlanDto,
};
use kontor_api::error::{ApiError, ApiErrorCode};
use kontor_api::state::ApiState;
use kontor_core::calendar::{ExecutionAuthorization, TimeRange, WorkScope};
use kontor_core::compaction::{CompactionReceipt, CompactionStatus};
use kontor_core::id::{
    AccountProfileId, AdvisorRunId, AgentRunId, AggregateRevision, ArtifactKey, BoundedText,
    CanonicalDocument, CommandReceiptId, CommitteeRunId, ConnectorKey, ContentHash, CurrencyCode,
    ExecutionAuthorizationId, ExternalId, ExternalName, GateKey, IdempotencyKey, IntakeReceiptId,
    MiniProjectId, ModuleKey, Money, ProjectId, QuickSessionId, RoleCatalogId, RoleCode,
    RoleSlotId, RoleTurnId, RuntimeKindKey, SCHEMA_VERSION, SeatBindingId, SourceEventId,
    SpecVersion, StatusConflictId, TaskId, TeamRunId, Timestamp, TopologyKindKey, TopologyNodeId,
    TopologySpecId, TriggerKey,
};
use kontor_core::realm::ReceiptEnvelope;
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    AdaptiveAdmissionAdvance, CalendarRepository, CapacityRepository, CommandRepository,
    CredentialReference, CredentialReferenceKind, IntakeOutcome, IntakeRepository,
    MiniProjectTopologySnapshot, NewAccountProfile, NewAdaptiveAdmissionState, NewAgentRun,
    NewAvailabilityOverride, NewCapacityObservation, NewCommandIntent, NewGateEvaluation,
    NewMiniProject, NewNativeContainerBinding, NewSeatBinding, NewSessionTopologyNode,
    NewSourceEvent, NewTeamRun, ProjectRepository, ProjectTopologyDefault, RealmRepository,
    RepositoryError, RunRepository, RuntimeBinding, SeatLivenessObservation, SourceDisposition,
    SpecRepository, StoredCompletionProfile, StoredCompletionWake, StoredCoreTeamRevision,
    StoredEpicCompletion, StoredEpicRoster, StoredPromotion, StoredQuickSession,
    StoredRemediationProposal, TaskTransitionRequest, TicketRepository, TopologyRepository,
    WorkflowRepository,
};
use kontor_core::spec::{
    AutoArmPolicy, CanonicalSourceEvent, CatalogRoleRef, CodeCategory, ContextEnforcement,
    ContextPolicySnapshot, EffectiveContextPolicy, EpicPresence, IntakeReceipt, IntakeResult,
    ModelRung, NodeProjectionCapability, ProjectSessionTopologySpec, RequestedContextPolicy,
    RoleCatalogRevision, Shareability, ShareabilityTier, SourceIdentity, SourceProcessingState,
    TeamRunSnapshot, TopologySnapshot, TriggerSpec,
};
use kontor_core::state::{
    GateVerdict, ObservedContainerKind, SessionTopologyNode, TaskState, TaskTeamClosure,
    TerminalEvidenceSource, TerminalOutcome, TopologyLifecycle,
};
use kontor_core::ticket::OwnershipAction;
use kontor_integrations_asma::jira::SpecCatalog;
use kontor_policy::{
    CloseoutEvidence, CloseoutRequirement, DeliberationStep, NeedsHumanPayload, TicketEvidence,
    TicketGateBlocker, TicketRequirement,
};
use kontor_profiles::pack::{
    OperationalDomainPack, PackAvailability, PackCategoryKey, ProfilePackSpec,
    ResolvedProfileBundle, parse_pack, resolve_profile, validate_pack,
};
use kontor_runtime::adapter::{RuntimeAdapter, RuntimeError};
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{RuntimeBindingSnapshot, RuntimeCapability};
use kontor_runtime::container::{
    ContainerBinding, ContainerBindingId, ContainerBindingSnapshot, ContainerProjection,
    ContainerRequest,
};
use kontor_runtime::request::{LaunchParts, LaunchPlacement};
use kontor_runtime::workspace::WorkspaceRoot;
use kontor_scheduler::model::{
    AccountAdmissionEvidence, AdaptiveWindow, AdmissionEventId, AdmittedCandidate,
    AuthorizationEvidence, CalendarAdmission, Candidate, CandidateDecision, CapacityConfig,
    CapacityUsage, ExternalWorkEvidence, ReconciliationEvidence, ReconciliationScope,
    RuntimeAdmissionEvidence, RuntimeHealth, SchedulingSnapshot, TaskOrigin,
};
use kontor_scheduler::{
    CommitteeVerdict, CompiledCompletion, CompletionBlocker, CompletionCommand,
    CompletionObservation, CompletionPhase, CompletionProfile, CompletionSignal, CompletionState,
    CompletionTransition, RemediationApproval, RemediationAuthorization, SignalDelivery,
};
use kontor_store::{
    AdmissionCommit, Applied, AuthorizationRevocation, EpicApplication, EpicTask, EpicTicketLink,
    IdempotencyBinding, NewRoleTurn, ProjectEnsure, RegisteredPack, SettledTurn, SqliteStore,
    StoredConflict, StoredTeamDraft, StoredTeamsProjection, TurnDispatch,
};
use kontor_teams::run::{SlotLaunch, TeamClosureCertificate, TeamRunLease, TeamRunSlots};
use kontor_teams::{
    CoreTeamRevision, CoreTeamSeat, CoreTeamSeatSelection, MANDATORY_LEAD_ROLE,
    MANDATORY_PROGRAM_ROLE,
};

/// One epic's roster, with the epic-side facts that travel with it.
struct FrozenRoster {
    /// The Core Team revision the epic is staffed from.
    revision: CoreTeamRevision,
    /// The epic roster row's own revision, which a write must present.
    revision_of_epic: AggregateRevision,
    /// The Quick session this epic came from, when it came from one.
    quick_session_id: Option<QuickSessionId>,
}

/// The adopted session base a Quick session is placed under.
struct SessionBase {
    /// The logical root node.
    node: SessionTopologyNode,
    /// The native project its runtime reported for that node, when it has
    /// reported one.
    native_id: Option<ExternalId>,
}

/// The realm-scoped operation a pack registration binds its key to.
///
/// A `&'static str` and not a free string: it is half of what a key is bound to,
/// and it is also a closed `CHECK` value in the schema, so the two spellings have
/// to be the same one.
const REGISTER_PACK: &str = "register_profile_pack";

/// How long a scheduler-held module or worktree lease lives, in seconds.
const LEASE_SECONDS: i64 = 3_600;

/// How long after creation a seat must have been observed attached (OP-REQ-039a).
///
/// Fixed at creation and stored, never recomputed from the row's age: a deadline
/// derived at read time moves every time the row is read, which is the defect
/// the persisted column exists to remove.
const SEAT_ATTACH_SECONDS: i64 = 600;

/// The composed services, and the one process state they run against.
///
/// The state arrives *after* construction because the two are mutually
/// dependent: `ApiState` needs the service to serve the routes, and the service
/// needs the store the state is holding. A [`OnceLock`] makes the ordering
/// explicit and makes a second attach impossible, which is better than a
/// nullable field that every method would have to re-justify.
pub struct Services {
    realm_id: kontor_core::id::RealmId,
    state: OnceLock<ApiState>,
    pack: ProfilePackSpec,
    /// The Operational topology vocabulary and delivery binding this build
    /// ships.
    ///
    /// Held beside the profile pack for the same reason: it is seeded data the
    /// build carries, so admission reads *which* kinds and role codes delivery
    /// uses rather than knowing them.
    domain: OperationalDomainPack,
    /// The connector specifications this build ships, parsed on first use.
    connectors: OnceLock<SpecCatalog>,
    /// How many simultaneous runs this Realm admits, from its configuration.
    ///
    /// Held here and read at both the planning and the admission call site, so a
    /// plan and the commit that follows it are judged against the same ceilings.
    /// It arrives at construction and never changes: a Realm that re-read its
    /// ceilings mid-flight could refuse a candidate the plan it is executing had
    /// already admitted.
    capacity: CapacityConfig,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Services")
            .field("attached", &self.state.get().is_some())
            .field("pack", &self.pack.pack_id.as_str())
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

impl Services {
    /// Compose the services around the profile pack this build ships, admitting
    /// work under `capacity`.
    ///
    /// The ceilings are a parameter rather than a constant in this file because
    /// they are a deployment's number and not a decision this module gets to make
    /// — the same reason [`kontor_scheduler::model::CapacityConfig`] ships no
    /// default of its own. [`crate::DEFAULT_CAPACITY`] is what a daemon composed
    /// the ordinary way passes.
    ///
    /// # Errors
    /// Returns the domain's own refusal when the bundled pack does not validate,
    /// which is a defect in the shipped data and not a runtime condition. The
    /// ceilings are *not* judged here: [`crate::Daemon::start`] validates them
    /// before it claims a state root, so a refused set stops a start rather than
    /// failing a composition halfway through one.
    pub fn new(
        realm_id: kontor_core::id::RealmId,
        capacity: CapacityConfig,
    ) -> Result<Arc<Self>, kontor_core::DomainError> {
        Ok(Arc::new(Self {
            realm_id,
            state: OnceLock::new(),
            pack: kontor_profiles::seeds::bundled_pack()?,
            domain: kontor_profiles::bundled_operational_domain()?,
            connectors: OnceLock::new(),
            capacity,
        }))
    }

    /// Hand the services the process state they run against. Once only.
    pub fn attach(&self, state: ApiState) {
        let _ = self.state.set(state);
    }

    /// The attached state, or the refusal a request is owed before one exists.
    fn state(&self) -> Result<&ApiState, ApiError> {
        self.state.get().ok_or_else(|| {
            ApiError::new(
                self.realm_id,
                ApiErrorCode::Unavailable,
                "the realm's application services are not composed yet",
            )
        })
    }

    /// Turn a repository refusal into the one the caller is owed.
    fn refuse(&self, error: &RepositoryError) -> ApiError {
        ApiError::from_repository(self.realm_id, error)
    }

    /// Turn a domain refusal into the one the caller is owed.
    fn refuse_domain(&self, error: &kontor_core::DomainError) -> ApiError {
        ApiError::from_domain(self.realm_id, error)
    }

    /// A refusal about this Realm with a static rule.
    const fn deny(&self, code: ApiErrorCode, rule: &'static str) -> ApiError {
        ApiError::new(self.realm_id, code, rule)
    }

    /// Refuse a Teams write, naming a reused key as one.
    ///
    /// The Teams store guards its own replay table, and the only conflict it can
    /// raise is a key already bound to different content. The generic mapping
    /// turns any repository conflict into `revision_conflict`, which tells a
    /// client to re-read and retry — and a retry never clears a reused key. So
    /// the one conflict this path can produce is named for what it is, the same
    /// way `projects:ensure` already names it.
    fn reused_team_key(&self, error: &RepositoryError) -> ApiError {
        match error {
            RepositoryError::Conflict { .. } => self.deny(
                ApiErrorCode::IdempotencyConflict,
                "the idempotency key was already used for a different Teams command",
            ),
            other => self.refuse(other),
        }
    }

    fn artifact_keys(&self, values: &[String]) -> Result<BTreeSet<ArtifactKey>, ApiError> {
        values
            .iter()
            .map(|value| {
                ArtifactKey::parse(value).map_err(|_| {
                    self.deny(
                        ApiErrorCode::InvalidRequest,
                        "artifact keys may contain only lowercase ASCII letters, digits, '.', '_' and '-'",
                    )
                })
            })
            .collect()
    }

    fn latest_handoff_receipt(
        &self,
        project_id: ProjectId,
        run: &kontor_core::repository::AgentRun,
    ) -> Result<Option<CompactionReceipt>, ApiError> {
        let Some(binding) = run.binding.as_ref() else {
            return Ok(None);
        };
        let receipt = self
            .state()?
            .with_store(|store| store.latest_compaction_receipt(project_id, run.id))
            .map_err(|error| self.refuse(&error))?;
        Ok(receipt.filter(|receipt| {
            receipt.binding_id == binding.id
                && receipt.native_before == binding.identity
                && receipt.handoff_hash.is_some()
        }))
    }

    fn best_effort_handoff_receipt(
        &self,
        project_id: ProjectId,
        run: &kontor_core::repository::AgentRun,
    ) -> Result<Option<CompactionReceipt>, ApiError> {
        Ok(self
            .latest_handoff_receipt(project_id, run)?
            .filter(|receipt| {
                receipt.requested.policy.enforcement == ContextEnforcement::BestEffort
                    && receipt.status == CompactionStatus::NotEnforced
            }))
    }

    fn role_slot_has_disposition(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        run: &kontor_core::repository::AgentRun,
        role_slot: &RoleSlotId,
    ) -> Result<bool, ApiError> {
        let state = self.state()?;
        let has_turn = state
            .with_store(|store| store.list_settled_turns(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
            .iter()
            .any(|turn| turn.agent_run_id == run.id && turn.role_slot_id == *role_slot);
        let has_waiver = state
            .with_store(|store| store.list_role_slot_waivers(project_id, run.team_run_id))
            .map_err(|error| self.refuse(&error))?
            .iter()
            .any(|waiver| waiver.role_slot_id == *role_slot);
        Ok(has_turn || has_waiver)
    }

    /// Resolve one advertised profile category into its frozen bundle.
    /// Every pack this Realm may resolve a category from: the compiled seeds
    /// first, then the operator-registered ones in registration order.
    ///
    /// The seeds come first deliberately. Registration is *additive* — a
    /// deployment introducing an incident profile must not be able to silently
    /// redefine `code` underneath every epic already frozen against it — so a
    /// category the build ships always wins, and a registered pack that
    /// re-advertises one is reported as shadowed rather than applied.
    fn packs(&self) -> Result<Vec<ProfilePackSpec>, ApiError> {
        let state = self.state()?;
        let registered = state
            .with_store(SqliteStore::list_profile_packs)
            .map_err(|error| self.refuse(&error))?;
        let mut packs = vec![self.pack.clone()];
        for pack in &registered {
            // A stored pack is re-parsed and re-validated rather than trusted.
            // It was validated when it was registered, but the rules that
            // validate it live in this binary and this binary may be newer than
            // the row.
            packs.push(parse_pack(&pack.document).map_err(|error| self.refuse_domain(&error))?);
        }
        Ok(packs)
    }

    /// The pack that owns `category`, and the category key.
    fn owning_pack(&self, category: &str) -> Result<(ProfilePackSpec, PackCategoryKey), ApiError> {
        let parsed =
            PackCategoryKey::parse(category).map_err(|error| self.refuse_domain(&error))?;
        self.packs()?
            .into_iter()
            .find(|pack| pack.category(&parsed).is_some())
            .map(|pack| (pack, parsed))
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no pack this realm holds advertises that category",
                )
            })
    }

    fn bundle(&self, category: &str, at: Timestamp) -> Result<ResolvedProfileBundle, ApiError> {
        let (pack, category) = self.owning_pack(category)?;
        resolve_profile(&pack, &category, at).map_err(|error| self.refuse_domain(&error))
    }

    /// The profile carrying `label` in this project, if there is one.
    ///
    /// The label is the natural identity an ensure matches on, exactly as a root
    /// path is for a project.
    fn profile_by_label(
        &self,
        project_id: ProjectId,
        label: &ExternalName,
    ) -> Result<Option<kontor_core::repository::AccountProfile>, ApiError> {
        let state = self.state()?;
        Ok(state
            .with_store(|store| store.list_account_profiles(project_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|profile| &profile.label == label))
    }

    /// Rebuild the answer a previous `epics:apply` gave, from what it wrote.
    ///
    /// A replay never re-runs the application: the graph is already there, and
    /// re-running it would take the same locks to prove the same thing. Every
    /// item reports `unchanged`, which is exactly what a second identical apply
    /// would have reported anyway.
    fn applied_epic_replay(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        bundle: &ResolvedProfileBundle,
    ) -> Result<AppliedEpicDto, ApiError> {
        let state = self.state()?;
        let epic = self.epic_row(project_id, epic_id)?;
        let tasks = state
            .with_store(|store| store.list_epic_tasks(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?;
        let edges = state
            .with_store(|store| store.task_dependency_graph(project_id))
            .map_err(|error| self.refuse(&error))?;
        let mut applied = Vec::with_capacity(tasks.len());
        for task in &tasks {
            let workflow = state
                .with_store(|store| store.get_active_task_workflow(project_id, task.id))
                .map_err(|error| self.refuse(&error))?;
            let links = state
                .with_store(|store| store.list_task_ticket_links(project_id, task.id))
                .map_err(|error| self.refuse(&error))?;
            let worktree = state
                .with_store(|store| store.task_worktree(project_id, task.id))
                .map_err(|error| self.refuse(&error))?;
            applied.push(AppliedTaskDto {
                title: task.title.clone(),
                task_id: task.id,
                applied: AppliedDto::Unchanged,
                state: task.state.as_str().to_owned(),
                revision: task.revision,
                workflow_id: workflow
                    .map(|workflow| workflow.id.to_string())
                    .unwrap_or_default(),
                depends_on: edges
                    .get(&task.id)
                    .map(|set| set.iter().copied().collect())
                    .unwrap_or_default(),
                links: links
                    .into_iter()
                    .map(|link| AppliedLinkDto {
                        link_id: link.id.to_string(),
                        connector: link.connector.as_str().to_owned(),
                        external_issue_key: link.external_issue_key.as_str().to_owned(),
                        applied: AppliedDto::Unchanged,
                    })
                    .collect(),
                worktree,
            });
        }
        self.sealed(AppliedEpicDto {
            realm_id: state.realm_id(),
            project_id,
            epic_id,
            applied: AppliedDto::Unchanged,
            revision: epic.revision,
            work_profile: RevisionRefDto {
                id: bundle.profile.definition.id.as_str().to_owned(),
                version: bundle.profile.definition.version,
            },
            team_template: bundle.team.as_ref().map(|team| RevisionRefDto {
                id: team.template_id.to_string(),
                version: team.version,
            }),
            // Sealed below, from the finished shape, exactly as the fresh apply
            // is. Reporting the resolved bundle's digest here is what made a
            // receipt-served replay of an unchanged graph disagree with the
            // apply that created it.
            bundle_hash: String::new(),
            tasks: applied,
        })
    }

    /// Every agent run in one team run, loaded whole.
    fn team_members(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
    ) -> Result<Vec<kontor_core::repository::AgentRun>, ApiError> {
        let state = self.state()?;
        let seats = state
            .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?;
        let mut runs = Vec::with_capacity(seats.len());
        for seat in seats {
            if let Some(run) = state
                .with_store(|store| store.get_agent_run(project_id, seat.agent_run_id))
                .map_err(|error| self.refuse(&error))?
            {
                runs.push(run);
            }
        }
        Ok(runs)
    }

    /// Certify one team run's closure from its declared slots, or say why not.
    ///
    /// The certificate is *derived*, never accepted: it is produced by
    /// [`TeamRunSlots::certify_team_closure`] walking the frozen template's own
    /// slots, so a seat that never ran is a refusal rather than an omission
    /// nobody notices. The lease is held only for the length of this call.
    fn certify_team(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
    ) -> Result<Result<TeamClosureCertificate, &'static str>, ApiError> {
        let state = self.state()?;
        let Some(team) = state
            .with_store(|store| store.get_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(Err("no such team run exists in this project"));
        };
        let runs = self.team_members(project_id, team_run_id)?;
        let bindings: Vec<_> = runs
            .iter()
            .filter_map(|run| run.binding.as_ref())
            .filter_map(|binding| state.sessions().get(binding.id))
            .collect();
        let Ok(lease) = TeamRunLease::acquire(team_run_id) else {
            return Ok(Err("another settlement is already closing this team run"));
        };
        let slots = match TeamRunSlots::hydrate(lease, &team.snapshot, &runs, &bindings) {
            Ok(slots) => slots,
            // Hydration refuses a shape the template does not describe. That is a
            // fact about the team, not a failure of this call, so it is reported
            // rather than raised.
            Err(_) => {
                return Ok(Err(
                    "this team run's seats do not match its frozen template",
                ));
            }
        };
        // Two ways a team can be finished, tried in the order that does not lie.
        //
        // Terminal runs first: if every declared slot's run actually ended, that
        // is the stronger statement and the one to make. Only when it does not
        // hold is the settled-turn basis considered — and that is not a fallback
        // in the sense of "try something weaker", it is a different question:
        // *did Kontor's work in every declared slot finish?* A persistent seat
        // makes that question the only answerable one, because the session is
        // meant to still be sitting there.
        //
        // Every basis is offered the *persisted* waivers. Passing an empty slice
        // would make a waiver a thing the API records and the closure ignores,
        // which is the one way this design can silently do nothing.
        let waivers = self.recorded_waivers(project_id, team_run_id)?;
        if let Ok(certificate) = slots.certify_team_closure(&waivers) {
            return Ok(Ok(certificate));
        }
        let accounted = self.settled_slots(project_id, team_run_id)?;
        if waivers.is_empty() {
            return match slots.certify_from_settled_turns(&accounted, &waivers) {
                Ok(certificate) => Ok(Ok(certificate)),
                Err(_) => Ok(Err(
                    "a declared role slot has neither ended nor settled its final turn",
                )),
            };
        }
        // A waived slot settles no turn, so the settled-turn basis cannot speak
        // for this team at all. The disposition basis is the only honest one,
        // and it requires *exactly one* source per declared slot.
        let dispositions = self.slot_dispositions(project_id, team_run_id, &accounted)?;
        match slots.certify_from_dispositions(&dispositions, &waivers) {
            Ok(certificate) => Ok(Ok(certificate)),
            Err(_) => Ok(Err("a declared role slot is neither settled nor waived")),
        }
    }

    /// The wire view of one recorded waiver.
    fn waiver_dto(
        &self,
        realm_id: kontor_core::id::RealmId,
        stored: kontor_store::StoredWaiver,
        applied: Applied,
        closed: Option<TeamRunId>,
    ) -> RoleSlotWaiverDto {
        RoleSlotWaiverDto {
            realm_id,
            project_id: stored.project_id,
            task_id: stored.task_id,
            team_run_id: stored.team_run_id.to_string(),
            role_slot: stored.role_slot_id.as_str().to_owned(),
            waiver_id: stored.id.to_string(),
            disposition: "waived",
            authorized_by_role: stored.authorized_role,
            authority_tier: stored.authority_tier,
            evidence: stored.evidence,
            evidence_hash: stored.evidence_hash.as_str().to_owned(),
            recorded_at: stored.recorded_at,
            applied: applied_dto(applied),
            team_run_closed: closed.map(|id| id.to_string()),
        }
    }

    /// Whether one slot of one team run carries a waiver.
    fn slot_is_waived(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
        slot: &RoleSlotId,
    ) -> Result<bool, ApiError> {
        let state = self.state()?;
        Ok(state
            .with_store(|store| store.list_role_slot_waivers(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
            .iter()
            .any(|waiver| &waiver.role_slot_id == slot))
    }

    /// The waivers persisted against one team run, in the shape the certifiers
    /// validate.
    fn recorded_waivers(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
    ) -> Result<Vec<kontor_teams::run::RoleSlotWaiver>, ApiError> {
        let state = self.state()?;
        state
            .with_store(|store| store.list_role_slot_waivers(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .map(|waiver| {
                Ok(kontor_teams::run::RoleSlotWaiver {
                    slot: waiver.role_slot_id,
                    authorized_by: kontor_core::id::RoleKey::parse(&waiver.authorized_role)
                        .map_err(|error| self.refuse_domain(&error))?,
                    evidence: waiver
                        .evidence
                        .iter()
                        .map(|key| kontor_core::id::ArtifactKey::parse(key))
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|error| self.refuse_domain(&error))?,
                    recorded_at: waiver.recorded_at,
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()
    }

    /// One disposition per declared slot, built from the same rows the store
    /// re-proves the closure from.
    ///
    /// A slot carrying both sources is left carrying both, deliberately: the
    /// certifier refuses that rather than this function picking a winner.
    fn slot_dispositions(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
        accounted: &BTreeMap<RoleSlotId, ContentHash>,
    ) -> Result<BTreeMap<RoleSlotId, kontor_core::state::SlotDisposition>, ApiError> {
        let state = self.state()?;
        let mut dispositions: BTreeMap<RoleSlotId, kontor_core::state::SlotDisposition> =
            BTreeMap::new();
        for (slot, evidence_hash) in accounted {
            dispositions.insert(
                slot.clone(),
                kontor_core::state::SlotDisposition::SettledTurn {
                    evidence_hash: evidence_hash.clone(),
                },
            );
        }
        for waiver in state
            .with_store(|store| store.list_role_slot_waivers(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
        {
            if dispositions.contains_key(&waiver.role_slot_id) {
                continue;
            }
            dispositions.insert(
                waiver.role_slot_id,
                kontor_core::state::SlotDisposition::WaivedUnbound {
                    evidence_hash: waiver.evidence_hash,
                },
            );
        }
        Ok(dispositions)
    }

    /// The final settled turn of each role slot in one team run.
    ///
    /// The digest carried per slot is the turn's own `evidence_hash`, so the
    /// closure policy digest transitively covers *which* turn accounted for each
    /// seat rather than merely that one did.
    fn settled_slots(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
    ) -> Result<BTreeMap<RoleSlotId, ContentHash>, ApiError> {
        let state = self.state()?;
        let task_id = self.task_for_team_run(project_id, team_run_id)?;
        let mut accounted: BTreeMap<RoleSlotId, ContentHash> = BTreeMap::new();
        for turn in state
            .with_store(|store| store.list_settled_turns(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
        {
            if turn.team_run_id != team_run_id {
                continue;
            }
            // Ordered oldest-first by the store, so the last write per slot is
            // that slot's newest turn.
            accounted.insert(turn.role_slot_id, turn.evidence_hash);
        }
        Ok(accounted)
    }

    /// Hand back every lease an abandoned run still holds.
    ///
    /// A lease is given up deliberately — closing the run it belonged to does
    /// not touch it — so an abandoned run goes on holding its module until the
    /// expiry lapses, and the next admission of the very task the abandonment
    /// was meant to free is refused with "an active lease already claims this
    /// place" for the rest of the window. The receipt that decided the
    /// abandonment is the receipt that decides the release.
    fn release_run_leases(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        receipt_id: CommandReceiptId,
        now: Timestamp,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        for lease in state
            .with_store(|store| store.live_leases_of_run(project_id, agent_run_id, now))
            .map_err(|error| self.refuse(&error))?
        {
            state
                .with_store(|store| {
                    store.release_lease(&kontor_store::LeaseRelease {
                        project_id,
                        lease_id: lease.id,
                        presented_token: lease.fencing_token,
                        receipt_id,
                        released_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }
        Ok(())
    }

    /// Abandon the team run of a run just abandoned, when nothing of it is left
    /// running and no certificate can close it.
    ///
    /// Returns `None` when the team still has a live run, or when it is already
    /// closed — in both cases there is nothing an operator decision should do to
    /// it.
    fn abandon_team_run(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        run: &kontor_core::repository::AgentRun,
        now: Timestamp,
    ) -> Result<Option<String>, ApiError> {
        let state = self.state()?;
        let Some(team) = state
            .with_store(|store| store.get_team_run(project_id, run.team_run_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(None);
        };
        if team.lifecycle.is_terminal() {
            return Ok(None);
        }
        let members = state
            .with_store(|store| store.list_agent_runs_for_team_run(project_id, run.team_run_id))
            .map_err(|error| self.refuse(&error))?;
        for member in &members {
            let member = state
                .with_store(|store| store.get_agent_run(project_id, member.agent_run_id))
                .map_err(|error| self.refuse(&error))?;
            if member.is_some_and(|member| member.terminal.is_none()) {
                return Ok(None);
            }
        }

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "team_run_abandon",
            "team_run_id": run.team_run_id.to_string(),
            "expected_revision": team.revision.get(),
            "reason": "every run of this team ended without a certifiable closure",
        }))?;
        let team_key = IdempotencyKey::parse(&format!("{}-team", key.as_str()))
            .map_err(|error| self.refuse_domain(&error))?;
        let receipt_id = state
            .with_store(|store| {
                store.record_abandon_receipt(&kontor_core::repository::NewAbandonReceipt {
                    project_id,
                    receipt_id: CommandReceiptId::generate(),
                    idempotency_key: team_key.clone(),
                    target: AggregateRef::TeamRun {
                        team_run_id: run.team_run_id,
                    },
                    target_revision: team.revision,
                    intent: intent.clone(),
                    recorded_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        state
            .with_store(|store| {
                store.close_team_run(&kontor_core::repository::TeamRunClosure {
                    project_id,
                    team_run_id: run.team_run_id,
                    expected_revision: team.revision,
                    evidence: kontor_core::state::TeamTerminalEvidence {
                        outcome: TerminalOutcome::Abandoned,
                        source: kontor_core::state::TeamEvidenceSource::OperatorAbandon {
                            receipt_id,
                        },
                        evidence_hash: intent.hash().clone(),
                        closed_at: now,
                    },
                })
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(Some(run.team_run_id.to_string()))
    }

    /// Close the team run behind a settled agent run, when every slot is done.
    fn settle_team(
        &self,
        project_id: ProjectId,
        run: &kontor_core::repository::AgentRun,
        now: Timestamp,
    ) -> Result<(Option<String>, Option<String>), ApiError> {
        let state = self.state()?;
        let Some(team) = state
            .with_store(|store| store.get_team_run(project_id, run.team_run_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok((
                None,
                Some("no such team run exists in this project".to_owned()),
            ));
        };
        if team.lifecycle.is_terminal() {
            return Ok((Some(run.team_run_id.to_string()), None));
        }
        let certificate = match self.certify_team(project_id, run.team_run_id)? {
            Ok(certificate) => certificate,
            Err(pending) => return Ok((None, Some(pending.to_owned()))),
        };
        // The envelope has to match what the certificate actually proved. A
        // settled-turn closure cites no child evidence, because its children are
        // expected to be live.
        let evidence = match certificate.basis() {
            kontor_teams::run::TeamClosureBasis::TerminalRuns => certificate
                .into_terminal_evidence(now)
                .map_err(|error| self.refuse_domain(&error))?,
            kontor_teams::run::TeamClosureBasis::SettledTurns => certificate
                .into_settled_turn_evidence(now)
                .map_err(|error| self.refuse_domain(&error))?,
            kontor_teams::run::TeamClosureBasis::RoleSlotDispositions => certificate
                .into_disposition_evidence(now)
                .map_err(|error| self.refuse_domain(&error))?,
        };
        state
            .with_store(|store| {
                store.close_team_run(&kontor_core::repository::TeamRunClosure {
                    project_id,
                    team_run_id: run.team_run_id,
                    expected_revision: team.revision,
                    evidence,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        state.signals().appended();
        Ok((Some(run.team_run_id.to_string()), None))
    }

    /// How a settled run's team currently stands, for an idempotent replay.
    fn team_closure_state(
        &self,
        project_id: ProjectId,
        run: &kontor_core::repository::AgentRun,
    ) -> Result<(Option<String>, Option<String>), ApiError> {
        let state = self.state()?;
        let Some(team) = state
            .with_store(|store| store.get_team_run(project_id, run.team_run_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok((
                None,
                Some("no such team run exists in this project".to_owned()),
            ));
        };
        if team.lifecycle.is_terminal() {
            Ok((Some(run.team_run_id.to_string()), None))
        } else {
            Ok((
                None,
                Some("a declared role slot is still live or produced no terminal run".to_owned()),
            ))
        }
    }

    /// The closure a task may cite when completing, if its team has certified one.
    ///
    /// The citation carries *identity* only — which team run, and the digest of
    /// the declared-slot policy that was proved about it. The store re-proves the
    /// substance against its own rows, so a fabricated citation buys nothing; this
    /// derives a real one rather than asserting it.
    fn task_team_closure(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<Result<TaskTeamClosure, &'static str>, ApiError> {
        let state = self.state()?;
        let runs = state
            .with_store(|store| store.list_team_runs_for_task(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        let Some((team_run_id, lifecycle)) = runs.into_iter().next_back() else {
            // No team ran, so there are no role slots to account for.
            return Ok(Ok(TaskTeamClosure::NoTeam));
        };
        // A team run that has not closed yet is not a refusal on its own: it may
        // be closable *now* on settled turns, and the certifier is what decides.
        // Refusing here on lifecycle alone is what made the public close route
        // unreachable for a team whose seats are deliberately still live.
        let _ = lifecycle;
        match self.certify_team(project_id, team_run_id)? {
            Ok(certificate) => Ok(Ok(certificate.task_team_closure())),
            Err(pending) => Ok(Err(pending)),
        }
    }

    /// The project row, refusing an id that is not in this Realm.
    fn project_row(
        &self,
        project_id: ProjectId,
    ) -> Result<kontor_core::repository::Project, ApiError> {
        let state = self.state()?;
        state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })
    }

    /// The category the pack advertises under this name, and its availability.
    fn advertised(&self, category: &str) -> Result<(PackCategoryKey, PackAvailability), ApiError> {
        let (pack, parsed) = self.owning_pack(category)?;
        pack.manifest
            .iter()
            .find(|entry| entry.category == parsed)
            .map(|entry| (parsed.clone(), entry.availability))
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no pack this realm holds advertises that category",
                )
            })
    }

    /// One pinned trigger revision, refusing an unknown key or revision.
    fn trigger_spec(
        &self,
        project_id: ProjectId,
        trigger: &str,
        version: SpecVersion,
    ) -> Result<TriggerSpec, ApiError> {
        let state = self.state()?;
        self.project_row(project_id)?;
        let id = TriggerKey::parse(trigger).map_err(|error| self.refuse_domain(&error))?;
        state
            .with_store(|store| store.get_trigger_spec(project_id, &id, version))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such trigger revision is installed in this project",
                )
            })
    }

    /// The connector specifications this build ships.
    ///
    /// ponytail: parsed once per process and held, because the bundled data is
    /// compiled into the binary and cannot change while it runs. A deployment
    /// that later loads specifications of its own would replace this with the
    /// same loader reading its directory — not with a second catalogue.
    fn connector_catalog(&self) -> Result<&SpecCatalog, ApiError> {
        if let Some(catalog) = self.connectors.get() {
            return Ok(catalog);
        }
        let catalog = SpecCatalog::bundled().map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "this build's bundled connector specifications did not load",
            )
        })?;
        Ok(self.connectors.get_or_init(|| catalog))
    }

    /// Hold a runtime's frozen snapshot in this process *and* durably.
    ///
    /// Both, because they answer different questions. The in-process registry is
    /// what session operations read on the hot path; the durable row is what the
    /// next process has to present to the runtime to get the binding attested
    /// again. Recording only the first is what left a live session unusable
    /// after a restart.
    fn hold(&self, snapshot: &RuntimeBindingSnapshot) -> Result<(), ApiError> {
        let state = self.state()?;
        let document = serde_json::to_string(snapshot).map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime's binding snapshot could not be recorded durably",
            )
        })?;
        state
            .with_store(|store| {
                store.persist_binding_snapshot(
                    snapshot.binding_id(),
                    snapshot.agent_run_id(),
                    &document,
                )
            })
            .map_err(|error| self.refuse(&error))?;
        state.sessions().record(snapshot.clone());
        Ok(())
    }

    /// Release a binding from this process and from the durable claim.
    ///
    /// A snapshot for a closed run is not evidence of anything, and leaving one
    /// behind would give the next startup a binding to re-attest that nothing
    /// should be operating.
    fn release(&self, binding_id: kontor_core::id::RuntimeBindingId) -> Result<(), ApiError> {
        let state = self.state()?;
        state
            .with_store(|store| store.forget_binding_snapshot(binding_id))
            .map_err(|error| self.refuse(&error))?;
        state.sessions().forget(binding_id);
        Ok(())
    }

    /// The task one team run serves.
    fn task_for_team_run(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
    ) -> Result<TaskId, ApiError> {
        let state = self.state()?;
        state
            .with_store(|store| store.get_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
            .map(|run| run.task_id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "this seat's team run does not exist in this project",
                )
            })
    }

    /// Derive, at most once, the follow-ups one settled turn unlocks.
    ///
    /// Everything it decides from is *persisted*: the settled turns of this task
    /// (which carry the artifacts each produced), the task's pinned workflow
    /// phase, and the frozen team template's handoff DAG. Nothing is read from
    /// memory that a restart would lose, which is what lets the same derivation
    /// run again from the reconciliation seam and reach the same answer.
    ///
    /// At-most-once is the store's, not a flag's: `turn_dispatches` is keyed by
    /// `(settling turn, receiving slot)`, so a replayed settlement and a restart
    /// that re-derives the same follow-up both insert nothing.
    async fn derive_follow_ups(
        &self,
        project_id: ProjectId,
        settled: &SettledTurn,
        now: Timestamp,
    ) -> Result<Vec<TurnFollowUpDto>, ApiError> {
        let state = self.state()?;
        let Some(team_run) = state
            .with_store(|store| store.get_team_run(project_id, settled.team_run_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(Vec::new());
        };
        let template = kontor_teams::spec::TeamTemplateSpec::from_snapshot(&team_run.snapshot)
            .map_err(|error| self.refuse_domain(&error))?;

        // Every artifact this task's turns have produced, not only this turn's.
        // A handoff waits on artifacts, and it does not care which turn produced
        // which: the condition is about the task's state, not about authorship.
        let produced: BTreeSet<ArtifactKey> = state
            .with_store(|store| store.list_settled_turns(project_id, settled.task_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .flat_map(|turn| turn.artifacts)
            .collect();
        let phase = state
            .with_store(|store| store.get_active_task_workflow(project_id, settled.task_id))
            .map_err(|error| self.refuse(&error))?;

        let mut follow_ups = Vec::new();
        for handoff in &template.handoffs {
            if handoff.from_slot != settled.role_slot_id {
                continue;
            }
            // Both halves of the condition, read from persisted facts.
            if !handoff
                .required_artifacts
                .iter()
                .all(|artifact| produced.contains(artifact))
            {
                continue;
            }
            if let (Some(after), Some(workflow)) = (handoff.after_phase.as_ref(), phase.as_ref())
                && !phase_reached(
                    &workflow.snapshot.definition,
                    &workflow.current_phase,
                    after,
                )
            {
                continue;
            }
            // A waived slot is not a recipient. The waiver is the durable
            // statement that this seat will not exist, so deriving a dispatch to
            // it would create a row nothing can ever deliver.
            if self.slot_is_waived(project_id, settled.team_run_id, &handoff.to_slot)? {
                continue;
            }
            // The seat for the receiving slot, already materialized by the start
            // that seated this team. A follow-up never creates one: activation is
            // about giving work to a seat that exists.
            let target = self.seat_for_slot(project_id, settled.team_run_id, &handoff.to_slot)?;
            // The message id is minted *once*, with the row, and never per
            // attempt. A retry of an undelivered follow-up therefore presents
            // the same id, which is what lets the runtime recognise an effect it
            // already committed but could not acknowledge.
            let message_id = kontor_runtime::request::MessageId::generate();
            let derived = state
                .with_store(|store| {
                    store.derive_turn_dispatch(&TurnDispatch {
                        settled_turn_id: settled.id,
                        to_role_slot_id: handoff.to_slot.clone(),
                        project_id,
                        team_run_id: settled.team_run_id,
                        message_id: message_id.to_string(),
                        target_agent_run: target,
                        dispatched: false,
                        derived_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
            let dispatched = if derived == Applied::Created {
                // Only a *newly* derived follow-up produces an effect. A replay
                // reaches the same row and does nothing, which is the whole of
                // "at most one follow-up effect".
                self.deliver_follow_up(project_id, settled, handoff, target, message_id, now)
                    .await?
            } else {
                self.already_dispatched(project_id, settled.id, &handoff.to_slot)?
            };
            follow_ups.push(TurnFollowUpDto {
                to_role_slot: handoff.to_slot.as_role_key().as_str().to_owned(),
                target_agent_run_id: target.map(|id| id.to_string()),
                dispatched,
                after_phase: handoff
                    .after_phase
                    .as_ref()
                    .map(|phase| phase.as_str().to_owned()),
            });
        }
        Ok(follow_ups)
    }

    /// One settled turn of this project, by id.
    fn settled_turn(
        &self,
        project_id: ProjectId,
        turn: RoleTurnId,
    ) -> Result<Option<SettledTurn>, ApiError> {
        let state = self.state()?;
        // Turns are listed per task, so the lookup goes through the dispatch row
        // that named this turn: it carries the team run, and the team run names
        // the task. A by-id read would be a second index over a table nothing
        // else queries that way.
        let dispatches = state
            .with_store(|store| store.list_turn_dispatches(project_id))
            .map_err(|error| self.refuse(&error))?;
        for row in dispatches {
            if row.settled_turn_id != turn {
                continue;
            }
            let task_id = self.task_for_team_run(project_id, row.team_run_id)?;
            let turns = state
                .with_store(|store| store.list_settled_turns(project_id, task_id))
                .map_err(|error| self.refuse(&error))?;
            return Ok(turns.into_iter().find(|candidate| candidate.id == turn));
        }
        Ok(None)
    }

    /// The handoff a derived follow-up came from.
    fn handoff_for(
        &self,
        project_id: ProjectId,
        settled: &SettledTurn,
        to_slot: &RoleSlotId,
    ) -> Result<Option<kontor_teams::spec::RoleHandoff>, ApiError> {
        let state = self.state()?;
        let Some(team_run) = state
            .with_store(|store| store.get_team_run(project_id, settled.team_run_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(None);
        };
        let template = kontor_teams::spec::TeamTemplateSpec::from_snapshot(&team_run.snapshot)
            .map_err(|error| self.refuse_domain(&error))?;
        Ok(template
            .handoffs
            .iter()
            .find(|handoff| {
                handoff.from_slot == settled.role_slot_id && &handoff.to_slot == to_slot
            })
            .cloned())
    }

    /// Whether one derived follow-up has already reached its seat.
    fn already_dispatched(
        &self,
        project_id: ProjectId,
        turn: RoleTurnId,
        slot: &RoleSlotId,
    ) -> Result<bool, ApiError> {
        let state = self.state()?;
        Ok(state
            .with_store(|store| store.list_turn_dispatches(project_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .any(|row| {
                row.settled_turn_id == turn && &row.to_role_slot_id == slot && row.dispatched
            }))
    }

    /// The seat holding one role slot in a team run, if it was materialized.
    fn seat_for_slot(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
        slot: &RoleSlotId,
    ) -> Result<Option<AgentRunId>, ApiError> {
        let role = slot.clone().into_role_key();
        let mut live = self
            .team_members(project_id, team_run_id)?
            .into_iter()
            .filter(|run| run.role == role && run.terminal.is_none());
        let seat = live.next().map(|run| run.id);
        if live.next().is_some() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the role slot has more than one non-terminal successor",
            ));
        }
        Ok(seat)
    }

    /// Give one already-materialized seat its follow-up work.
    ///
    /// The effect is a message into the seat's live session, which is why the
    /// seat has to still be one: a follow-up to a seat this process cannot drive
    /// is recorded as derived and undelivered rather than reported as done, and
    /// the next reconciliation retries exactly that row.
    async fn deliver_follow_up(
        &self,
        project_id: ProjectId,
        settled: &SettledTurn,
        handoff: &kontor_teams::spec::RoleHandoff,
        target: Option<AgentRunId>,
        message_id: kontor_runtime::request::MessageId,
        now: Timestamp,
    ) -> Result<bool, ApiError> {
        let state = self.state()?;
        let Some(target) = target else {
            return Ok(false);
        };
        let Some(run) = state
            .with_store(|store| store.get_agent_run(project_id, target))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(false);
        };
        let Some(binding) = run.binding.as_ref() else {
            return Ok(false);
        };
        let Some(snapshot) = state.sessions().get(binding.id) else {
            return Ok(false);
        };
        let Some(adapter) = state.runtimes().get(&binding.identity.runtime_kind) else {
            return Ok(false);
        };
        let body = kontor_core::id::BoundedText::parse(&format!(
            "handoff: {} finished its turn. The artifacts it owed are recorded. Begin your turn.",
            settled.role_slot_id.as_role_key().as_str()
        ))
        .map_err(|error| self.refuse_domain(&error))?;
        // The id belongs to the dispatch row, not to this attempt: it was minted
        // once when the follow-up was derived and is read back on every retry. An
        // effect the runtime committed but could not acknowledge is therefore
        // recognised as the *same* message rather than delivered a second time.
        let request = kontor_runtime::request::SendMessageRequest {
            binding: snapshot,
            message_id,
            body,
            sent_at: now,
        };
        match adapter.send(&request).await {
            Ok(_) => {
                state
                    .with_store(|store| {
                        store.mark_turn_dispatched(settled.id, &handoff.to_slot, target)
                    })
                    .map_err(|error| self.refuse(&error))?;
                Ok(true)
            }
            // Derived and undelivered. The row stands, so the next reconciliation
            // retries this one instead of deriving another.
            Err(_) => Ok(false),
        }
    }

    /// The root a task's workspace is prepared at.
    ///
    /// It is read from the task's declared worktree and from nowhere else.
    /// Before this existed, admission synthesized `/w/<task_id>` — a path that
    /// names a real task and no real directory — because the model carried no
    /// worktree at all. A runtime that verifies placement refused it; one that
    /// did not would have run the work in a directory nobody chose.
    ///
    /// A task with no declared worktree is refused rather than placed at a
    /// guess. There is no safe default: the whole point of the field is that
    /// only the operator knows where this task's tree is.
    fn task_root(&self, project_id: ProjectId, task_id: TaskId) -> Result<WorkspaceRoot, ApiError> {
        let state = self.state()?;
        let declared = state
            .with_store(|store| store.task_worktree(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "this task declares no worktree, so there is nowhere to prepare its workspace",
                )
            })?;
        WorkspaceRoot::parse(declared.as_str()).map_err(|error| self.refuse_domain(&error))
    }

    /// Read one task row, refusing an id that is not in this project.
    fn task_row(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<kontor_core::repository::Task, ApiError> {
        let state = self.state()?;
        state
            .with_store(|store| store.get_task(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such task exists in this project",
                )
            })
    }

    /// The agent run currently filling this task's seat, if there is one.
    fn live_seat(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<Option<AgentRunId>, ApiError> {
        let state = self.state()?;
        let runs = state
            .with_store(|store| store.list_team_runs_for_task(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        for (team_run_id, _) in runs {
            let seats = state
                .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
                .map_err(|error| self.refuse(&error))?;
            if let Some(seat) = seats.into_iter().next() {
                return Ok(Some(seat.agent_run_id));
            }
        }
        Ok(None)
    }

    /// Refuse a correction to a selection a run has already frozen.
    ///
    /// This is the whole reason the selection routes are separate from
    /// `epics:apply`: they exist for a deliberate pre-run change, and a change
    /// after a run snapshotted the pin would re-grade work already done against a
    /// specification it never ran under.
    fn ensure_pre_run(&self, project_id: ProjectId, task_id: TaskId) -> Result<(), ApiError> {
        if self.live_seat(project_id, task_id)?.is_some() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "a run has already snapshotted this task's selections",
            ));
        }
        Ok(())
    }

    /// The verdict a replayed gate recording already appended.
    fn gate_verdict_replay(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        workflow_id: &kontor_core::id::TaskWorkflowId,
        gate: &GateKey,
        _key: &IdempotencyKey,
    ) -> Result<GateVerdictDto, ApiError> {
        let state = self.state()?;
        let evaluations = state
            .with_store(|store| store.list_gate_evaluations(project_id, *workflow_id))
            .map_err(|error| self.refuse(&error))?;
        let last = evaluations
            .iter()
            .rfind(|evaluation| &evaluation.gate == gate)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the replayed receipt names a verdict this workflow no longer has",
                )
            })?;
        let gates = state
            .with_store(|store| store.gate_states(project_id, *workflow_id))
            .map_err(|error| self.refuse(&error))?;
        Ok(GateVerdictDto {
            realm_id: state.realm_id(),
            task_id,
            gate: gate.as_str().to_owned(),
            sequence: last.sequence,
            verdict: last.verdict.as_str().to_owned(),
            state: gates
                .get(gate)
                .map_or("not_ready", |state| state.as_str())
                .to_owned(),
            receipt_id: String::new(),
        })
    }

    /// The typed reconciliation projection for one task's tickets.
    ///
    /// The field set is closed by construction: the only thing Kontor asserts
    /// about an external ticket here is the semantic milestone its own workflow
    /// phase maps to. There is no branch that could add a status string, an
    /// assignee or a comment, because there is no input carrying one.
    fn reconcile_projection(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<
        (
            Vec<kontor_core::id::TicketLinkId>,
            Vec<TicketFieldDiffDto>,
            String,
        ),
        ApiError,
    > {
        let state = self.state()?;
        self.task_row(project_id, task_id)?;
        let links = state
            .with_store(|store| store.list_task_ticket_links(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        let workflow = state
            .with_store(|store| store.get_active_task_workflow(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        let milestone = workflow.as_ref().map_or_else(
            || "unknown".to_owned(),
            |workflow| workflow.current_phase.as_str().to_owned(),
        );

        let mut diff = Vec::new();
        for link in &links {
            // Without a pinned field specification there is no mapping from a
            // Kontor phase to an external field, and inventing one is exactly the
            // arbitrary mutation this operation must not perform. Such a link is
            // reported as needing a specification rather than as converged.
            let spec = state
                .with_store(|store| {
                    store.get_ticket_field_spec(&kontor_core::repository::ConnectorSpecSelector {
                        project_id,
                        connector: link.connector.clone(),
                        project: kontor_core::id::ExternalProjectKey::parse("unknown")
                            .unwrap_or_else(|_| unreachable!("a literal open key parses")),
                        issue_type: kontor_core::id::ExternalIssueTypeKey::parse("unknown")
                            .unwrap_or_else(|_| unreachable!("a literal open key parses")),
                        version: SpecVersion::FIRST,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
            if spec.is_none() {
                diff.push(TicketFieldDiffDto {
                    milestone: milestone.clone(),
                    kontor: milestone.clone(),
                    external: None,
                });
            }
        }
        let document = self.intent(&serde_json::json!({
            "schema_version": 1,
            "task_id": task_id.to_string(),
            "milestone": milestone,
            "links": links.iter().map(|link| link.id.to_string()).collect::<Vec<_>>(),
            "diff": diff.len(),
        }))?;
        Ok((
            links.iter().map(|link| link.id).collect(),
            diff,
            document.hash().as_str().to_owned(),
        ))
    }

    /// Read one epic's goal row, refusing an id that is not in this project.
    fn epic_row(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<kontor_core::repository::MiniProject, ApiError> {
        let state = self.state()?;
        state
            .with_store(|store| store.get_mini_project(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such epic exists in this project",
                )
            })
    }

    /// Whether this key has already recorded *this exact* request.
    ///
    /// The lookup is by key alone and is not project-scoped, which is what lets
    /// `projects:ensure` — the one mutation whose project may not exist yet — be
    /// guarded before it writes anything. A key that recorded a different intent
    /// is a conflict here, before the operation runs, rather than after it has
    /// already created a row it then has to refuse.
    ///
    /// The target is compared only when the caller can name one; a bootstrap
    /// replay learns its target *from* the stored receipt.
    fn replayed(
        &self,
        key: &IdempotencyKey,
        intent: &CanonicalDocument,
        target: Option<&AggregateRef>,
    ) -> Result<Option<kontor_core::receipt::CommandReceipt>, ApiError> {
        let state = self.state()?;
        let Some(existing) = state
            .with_store(|store| store.get_receipt_by_key(key))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(None);
        };
        let same_target = target.is_none_or(|target| &existing.target == target);
        if !same_target || existing.intent.hash() != intent.hash() {
            return Err(self.deny(
                ApiErrorCode::IdempotencyConflict,
                "the idempotency key was already used for a different operation",
            ));
        }
        Ok(Some(existing))
    }

    /// Canonicalize one intent document, refusing anything the domain will not
    /// store.
    fn intent(&self, value: &serde_json::Value) -> Result<CanonicalDocument, ApiError> {
        CanonicalDocument::from_value(value).map_err(|error| self.refuse_domain(&error))
    }

    /// A stable digest of one applied epic graph.
    ///
    /// Two properties matter, and both are about what is *left out*. Nothing
    /// here describes the call: no timestamp, no receipt, no resolution instant,
    /// and not the per-row `applied` flags — those say "this call created it"
    /// versus "it was already there", which is precisely the difference between
    /// a first apply and a replay of it. And nothing here is minted per write:
    /// the workflow id a task's profile was frozen under is deliberately absent,
    /// because a caller diffing this wants to know whether the *graph* moved.
    ///
    /// What is left is the graph: the epic and its revision, the pinned profile
    /// and team revisions, and every task's identity, title, state, revision,
    /// dependency set and ticket links. Dependencies arrive as a `BTreeSet` and
    /// links are sorted here, so a re-derivation cannot differ by ordering.
    /// It takes the finished [`AppliedEpicDto`] rather than the store's
    /// `AppliedEpic`, and that is the fix for the second half of this defect. A
    /// fresh apply and a receipt-served replay build the same DTO by different
    /// routes — one from what the transaction wrote, one from what the store
    /// holds — and while each computed its own digest from its own shape, the
    /// two could disagree without either being wrong on its own terms. One
    /// function over one shape makes that structurally impossible.
    fn graph_digest(&self, epic: &AppliedEpicDto) -> Result<String, ApiError> {
        let tasks: Vec<serde_json::Value> = epic
            .tasks
            .iter()
            .map(|task| {
                let mut links: Vec<String> = task
                    .links
                    .iter()
                    .map(|link| format!("{}\u{1f}{}", link.connector, link.external_issue_key))
                    .collect();
                links.sort();
                let mut depends_on: Vec<String> =
                    task.depends_on.iter().map(ToString::to_string).collect();
                depends_on.sort();
                serde_json::json!({
                    "task_id": task.task_id.to_string(),
                    "title": task.title.as_str(),
                    "state": task.state,
                    "revision": task.revision.get(),
                    "depends_on": depends_on,
                    "links": links,
                })
            })
            .collect();
        let document = self.intent(&serde_json::json!({
            "schema_version": 1,
            "epic_id": epic.epic_id.to_string(),
            "revision": epic.revision.get(),
            "work_profile": [epic.work_profile.id.as_str(), epic.work_profile.version.get()],
            "team_template": epic
                .team_template
                .as_ref()
                .map(|team| serde_json::json!([team.id.as_str(), team.version.get()])),
            "tasks": tasks,
        }))?;
        Ok(document.hash().as_str().to_owned())
    }

    /// Fill in an epic answer's digest, whichever path built it.
    ///
    /// Both callers construct the whole DTO with an empty `bundle_hash` and hand
    /// it here. There is no route by which one of them computes a digest the
    /// other would not.
    fn sealed(&self, mut epic: AppliedEpicDto) -> Result<AppliedEpicDto, ApiError> {
        epic.bundle_hash = self.graph_digest(&epic)?;
        Ok(epic)
    }

    /// Record one command intent and return its receipt.
    ///
    /// Every authority-bearing operation in this file goes through here, so the
    /// durable trail of *who asked for what* is the same one the generic command
    /// route writes, and the `Idempotency-Key` check is the store's rather than a
    /// second one that could disagree with it.
    fn record(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        kind: CommandKind,
        target: AggregateRef,
        target_revision: AggregateRevision,
        document: &CanonicalDocument,
    ) -> Result<CommandReceiptId, ApiError> {
        let state = self.state()?;
        let realm_id = state.realm_id();
        let document = document.clone();
        let now = kontor_api::now();
        let receipt = state.with_store(|store| {
            if let Some(existing) = store
                .get_receipt_by_key(key)
                .map_err(|error| self.refuse(&error))?
            {
                existing.ensure_replay(&target, &document).map_err(|_| {
                    self.deny(
                        ApiErrorCode::IdempotencyConflict,
                        "the idempotency key was already used for a different operation",
                    )
                })?;
                return Ok(existing.id);
            }
            let envelope = ReceiptEnvelope::new(
                realm_id,
                NewCommandIntent {
                    project_id,
                    receipt_id: CommandReceiptId::generate(),
                    idempotency_key: key.clone(),
                    kind,
                    target,
                    target_revision,
                    intent: document.clone(),
                    payload: document.clone(),
                    desired: None,
                    not_before: now,
                    created_at: now,
                },
            );
            store
                .record_intent_in_realm(&envelope)
                .map(|receipt| receipt.id)
                .map_err(|error| self.refuse(&error))
        })?;
        state.signals().appended();
        Ok(receipt)
    }

    /// Build the scheduling snapshot one epic is judged against.
    ///
    /// Every field is read from what is durably true right now, and the runtime
    /// half is read from the adapters rather than from a stored copy: a
    /// capability set that was true an hour ago is not evidence about this
    /// admission. Nothing here writes.
    async fn snapshot(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<SchedulingSnapshot, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let tasks = state
            .with_store(|store| store.list_epic_tasks(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?;
        let edges = state
            .with_store(|store| store.task_dependency_graph(project_id))
            .map_err(|error| self.refuse(&error))?;
        let authorizations = state
            .with_store(|store| store.list_authorizations(project_id))
            .map_err(|error| self.refuse(&error))?;
        let in_flight = state
            .with_store(SqliteStore::tasks_with_open_runs)
            .map_err(|error| self.refuse(&error))?;
        let module_leases = state
            .with_store(|store| store.active_module_claims(now))
            .map_err(|error| self.refuse(&error))?;
        let worktree_leases = state
            .with_store(|store| store.active_worktree_leases(now))
            .map_err(|error| self.refuse(&error))?;
        let all_tasks = state
            .with_store(|store| store.list_tasks(project_id))
            .map_err(|error| self.refuse(&error))?;
        let completed: BTreeSet<TaskId> = all_tasks
            .iter()
            .filter(|task| task.state == TaskState::Done)
            .map(|task| task.id)
            .collect();

        let runtime = self.runtime_evidence(project_id, now).await?;
        let mut candidates = Vec::new();
        for task in &tasks {
            let Some(workflow) = state
                .with_store(|store| store.get_active_task_workflow(project_id, task.id))
                .map_err(|error| self.refuse(&error))?
            else {
                // A task with no active workflow was never given a profile, so
                // there is nothing to judge it against. It is simply not a
                // candidate; it is not a blocked one.
                continue;
            };
            let armed = authorizations
                .iter()
                .find(|stored| stored.arms(now, Some(epic_id), Some(task.id)))
                .map(|stored| evidence_of(&stored.authorization));
            candidates.push(Candidate {
                project_id,
                task_id: task.id,
                mini_project_id: Some(epic_id),
                workflow_id: workflow.id,
                state: task.state,
                revision: task.revision,
                created_at: task.created_at,
                priority: 0,
                module: task.module.clone(),
                worktree: None,
                depends_on: edges.get(&task.id).cloned().unwrap_or_default(),
                serializes_with: BTreeSet::new(),
                origin: TaskOrigin::Manual,
                authorization: armed,
                // A Realm with no calendar assignment is unrestricted, which is
                // what an unconfigured deployment is — not "closed".
                calendar: CalendarAdmission::unrestricted(),
                runtime: runtime.clone(),
                // The pre-run selection is what the planner reads. Without this
                // the account-selection operation would store a pin nothing ever
                // looks at, which is worse than not having the operation.
                account: self.account_evidence(project_id, task.id, &runtime)?,
                external: ExternalWorkEvidence::default(),
            });
        }

        Ok(SchedulingSnapshot {
            schema_version: SCHEMA_VERSION,
            taken_at: now,
            candidates,
            in_flight_tasks: in_flight,
            completed_tasks: completed,
            module_leases,
            worktree_leases,
            usage: self.mission_usage(project_id, epic_id, &tasks)?,
            capacity: self.capacity,
            adaptive_window: self.admission_window(project_id, epic_id)?,
            freshness: jiff::SignedDuration::from_secs(state.evidence_window_seconds()),
        })
    }

    /// The adaptive window this epic is actually standing at.
    ///
    /// Read, never started. A snapshot that began a fresh window would reset the
    /// width to four on every plan, which quietly discards whatever pressure the
    /// last pass observed — the epic would keep re-learning the same throttling
    /// and keep admitting into it. An epic with no persisted state yet is the
    /// one case that legitimately starts fresh.
    fn admission_window(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<AdaptiveWindow, ApiError> {
        let state = self.state()?;
        let persisted = state
            .with_store(|store| store.get_adaptive_admission_state(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?;
        Ok(match persisted {
            Some(stored) => AdaptivePosition {
                current_window: stored.current_window,
                clean_observation_streak: stored.clean_observation_streak,
                last_observation_id: stored.last_observation_id,
            }
            .window(self.capacity.adaptive),
            None => AdaptiveWindow::start(self.capacity.adaptive),
        })
    }

    /// Judge one candidate specification document, and hash exactly what was
    /// judged.
    ///
    /// The violations come from `ProjectSessionTopologySpec::validate`, which is
    /// the domain's own rule set. It refuses at the first rule it finds broken,
    /// so this list holds at most one entry — and that is deliberate: a second,
    /// more thorough validator here would be a competing source of truth about
    /// what a valid vocabulary is, and the two would eventually disagree at
    /// exactly the moment a publication depended on it.
    fn judge_candidate(
        &self,
        candidate: &serde_json::Value,
    ) -> Result<(Vec<String>, ContentHash), ApiError> {
        let Ok(spec) = serde_json::from_value::<ProjectSessionTopologySpec>(candidate.clone())
        else {
            return Ok((
                vec!["is not a topology specification document".to_owned()],
                self.intent(candidate)?.hash().clone(),
            ));
        };
        let hash = self.candidate_hash(&spec)?;
        let mut violations = Vec::new();
        if let Err(error) = spec.validate() {
            violations.push(error.to_string());
        }
        Ok((violations, hash))
    }

    /// The canonical identity of one candidate specification.
    ///
    /// Hashed from the *parsed document*, never from the bytes a caller sent.
    /// A specification has optional fields — an empty `historical_codes` is
    /// omitted, not written as an empty list — so the same revision has more
    /// than one JSON spelling, and hashing the spelling would give a draft, a
    /// verdict and the stored revision three different identities for one
    /// document. The store already hashes the parsed form, so this is the same
    /// rule stated once rather than a second one that agrees by luck.
    ///
    /// Deliberately not `canonicalize`, which validates first: a draft of an
    /// incomplete vocabulary still has an identity, and refusing to name it
    /// would make the verdict impossible to ask for.
    fn candidate_hash(&self, spec: &ProjectSessionTopologySpec) -> Result<ContentHash, ApiError> {
        Ok(CanonicalDocument::from_serializable(spec)
            .map_err(|error| self.refuse_domain(&error))?
            .hash()
            .clone())
    }

    /// One catalog revision, as this Realm can answer for it.
    ///
    /// The store first, because a published revision is what the Realm's own
    /// seats are recorded against. The bundled document is the fallback for the
    /// exact same identity and version, and only that: a catalog revision is a
    /// property of the build rather than of a project, so a fresh Realm that has
    /// not yet published one can still answer what its own codes mean. Nothing
    /// is invented — a revision this build does not ship is simply not found.
    fn catalog_revision(
        &self,
        catalog_id: RoleCatalogId,
        version: SpecVersion,
    ) -> Result<RoleCatalogRevision, ApiError> {
        let state = self.state()?;
        if let Some(published) = state
            .with_store(|store| store.get_role_catalog(catalog_id, version))
            .map_err(|error| self.refuse(&error))?
        {
            return Ok(published);
        }
        self.domain
            .role_catalogs
            .iter()
            .find(|catalog| catalog.catalog_id == catalog_id && catalog.version == version)
            .cloned()
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such role catalog revision exists in this realm",
                )
            })
    }

    /// The catalog revision this build publishes.
    fn published_catalog(&self) -> Result<RoleCatalogRevision, ApiError> {
        let catalog = self.domain.role_catalogs.first().ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "this build ships no role catalog",
            )
        })?;
        self.catalog_revision(catalog.catalog_id, catalog.version)
    }

    /// One epic's current pin.
    fn epic_pin(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<TopologySnapshot, ApiError> {
        let state = self.state()?;
        Ok(state
            .with_store(|store| store.get_mini_project_topology(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "this epic is not pinned to a topology revision yet",
                )
            })?
            .topology)
    }

    /// What moving one epic's pin to `target` would do.
    ///
    /// Every effect is derived from what is stored: the kinds the target no
    /// longer declares and the nodes standing on them, the kinds it adds, and
    /// the seats and native containers that would be left citing a vocabulary
    /// their node's kind has left. Nothing here writes, and nothing is inferred
    /// from the *desired* shape — a node's own recorded kind is what is judged.
    fn upgrade_effects(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        target: &RevisionRefDto,
    ) -> Result<
        (
            TopologySnapshot,
            TopologySnapshot,
            Vec<TopologyUpgradeEffectDto>,
        ),
        ApiError,
    > {
        let state = self.state()?;
        let current = self.epic_pin(project_id, epic_id)?;
        let target_id =
            TopologySpecId::parse(&target.id).map_err(|error| self.refuse_domain(&error))?;
        let target_spec = state
            .with_store(|store| store.get_topology_spec(project_id, target_id, target.version))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the target revision is not published in this project",
                )
            })?;
        let current_spec = state
            .with_store(|store| {
                store.get_topology_spec(project_id, current.spec_id, current.version)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the revision this epic is pinned to is not published in this project",
                )
            })?;
        let target_snapshot = TopologySnapshot {
            spec_id: target_spec.spec_id,
            version: target_spec.version,
            canonical_hash: target_spec
                .canonicalize()
                .map_err(|error| self.refuse_domain(&error))?
                .hash()
                .clone(),
        };
        if target_snapshot == current {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "the epic is already pinned to that revision",
            ));
        }

        let declared: BTreeSet<&TopologyKindKey> = target_spec
            .node_kinds
            .iter()
            .map(|kind| &kind.kind)
            .collect();
        let held: BTreeSet<&TopologyKindKey> = current_spec
            .node_kinds
            .iter()
            .map(|kind| &kind.kind)
            .collect();

        let mut effects = Vec::new();
        for kind in held.difference(&declared) {
            effects.push(TopologyUpgradeEffectDto {
                subject: "kind".to_owned(),
                topology_node_id: None,
                effect: "withdrawn".to_owned(),
                detail: self.detail(&format!("`{kind}` is no longer a declared node kind"))?,
            });
        }
        for kind in declared.difference(&held) {
            effects.push(TopologyUpgradeEffectDto {
                subject: "kind".to_owned(),
                topology_node_id: None,
                effect: "introduced".to_owned(),
                detail: self.detail(&format!("`{kind}` becomes available to place"))?,
            });
        }

        for node in state
            .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
            .map_err(|error| self.refuse(&error))?
        {
            if declared.contains(&node.kind) {
                continue;
            }
            effects.push(TopologyUpgradeEffectDto {
                subject: "node".to_owned(),
                topology_node_id: Some(node.id),
                effect: "orphaned".to_owned(),
                detail: self.detail(&format!(
                    "this node stands on `{}`, which the target does not declare",
                    node.kind
                ))?,
            });
            let seats = state
                .with_store(|store| store.list_seat_bindings(project_id, node.id))
                .map_err(|error| self.refuse(&error))?;
            if seats
                .iter()
                .any(kontor_core::state::SeatBinding::is_non_terminal)
            {
                effects.push(TopologyUpgradeEffectDto {
                    subject: "seat".to_owned(),
                    topology_node_id: Some(node.id),
                    effect: "stranded".to_owned(),
                    detail: self.detail("the node still hosts a live seat")?,
                });
            }
            if state
                .with_store(|store| store.get_topology_node_container(project_id, node.id))
                .map_err(|error| self.refuse(&error))?
                .is_some()
            {
                effects.push(TopologyUpgradeEffectDto {
                    subject: "container".to_owned(),
                    topology_node_id: Some(node.id),
                    effect: "stranded".to_owned(),
                    detail: self.detail("the node still holds a native container binding")?,
                });
            }
        }

        // Sorted so two runs over the same Realm produce the same list, which is
        // what makes the preview hash mean anything at all.
        effects.sort_by(|left, right| {
            (
                &left.subject,
                left.topology_node_id.map(|id| id.to_string()),
                &left.effect,
                &left.detail,
            )
                .cmp(&(
                    &right.subject,
                    right.topology_node_id.map(|id| id.to_string()),
                    &right.effect,
                    &right.detail,
                ))
        });
        Ok((current, target_snapshot, effects))
    }

    // -- Project Core Team ---------------------------------------------------
    //
    // The Core Team is configuration: which standard roles this project staffs
    // an epic with. Nothing here creates a seat, a run or a topology node. The
    // seats appear when an epic materializes its frozen roster, which is a
    // different operation against a different aggregate.

    /// The project's current published Core Team, as the domain models it.
    fn stored_core_team(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<CoreTeamRevision>, ApiError> {
        let stored = self
            .state()?
            .with_store(|store| store.get_current_core_team(project_id))
            .map_err(|error| self.refuse(&error))?;
        stored
            .map(|stored| {
                Ok(CoreTeamRevision {
                    version: stored.version,
                    catalog_hash: stored.catalog_hash,
                    seats: serde_json::from_value(stored.seats).map_err(|_| {
                        self.deny(
                            ApiErrorCode::Unavailable,
                            "a stored Core Team revision cannot be read by this build",
                        )
                    })?,
                })
            })
            .transpose()
    }

    /// Resolve one caller's selections into the next immutable revision.
    ///
    /// Every selection is resolved against the exact catalog revision it names,
    /// and all of them must name the same one: a revision records a single
    /// `catalog_hash`, and a roster assembled from two catalogs could not say
    /// which one it was resolved against.
    fn resolve_core_team(
        &self,
        project_id: ProjectId,
        seats: &[CoreTeamSeatSelectionDto],
        current: Option<&CoreTeamRevision>,
    ) -> Result<CoreTeamRevision, ApiError> {
        let first = seats
            .first()
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "a Core Team names at least one role",
                )
            })?
            .role
            .catalog_revision
            .clone();
        if seats.iter().any(|seat| seat.role.catalog_revision != first) {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "every Core Team selection resolves against one catalog revision",
            ));
        }
        let catalog_id =
            RoleCatalogId::parse(&first.id).map_err(|error| self.refuse_domain(&error))?;
        let catalog = self.catalog_revision(catalog_id, first.version)?;
        let selections: Vec<CoreTeamSeatSelection> = seats
            .iter()
            .map(|seat| CoreTeamSeatSelection {
                role_code: seat.role.role_code.clone(),
                custom_display_name: seat.role.custom_display_name.clone(),
                presence: seat.presence,
                ad_hoc_allowed: seat.ad_hoc_allowed,
            })
            .collect();
        let version = current
            .map_or(Ok(SpecVersion::FIRST), |current| current.version.next())
            .map_err(|error| self.refuse_domain(&error))?;
        let _ = project_id;
        CoreTeamRevision::resolve(version, &catalog, &selections)
            .map_err(|error| self.refuse_domain(&error))
    }

    // -- Quick sessions ------------------------------------------------------

    /// The project's adopted session base, with its exact native readback.
    ///
    /// Both halves are required. A base whose configured node has never been
    /// read back, or whose readback disagrees with the container it is bound
    /// to, is `placement_blocked` — never a reason to create a replacement
    /// native project, which is how an ad-hoc session ends up in a workspace
    /// nobody is watching.
    fn session_base(&self, project_id: ProjectId) -> Result<SessionBase, ApiError> {
        let state = self.state()?;
        let spec = self.pinned_spec(project_id)?;
        let node = state
            .with_store(|store| store.list_topology_nodes(project_id, None))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|node| node.kind == spec.root_kind && node.parent_id.is_none())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "this project has no adopted session base to place a Quick session under",
                )
            })?;
        // The observation is evidence, and evidence can legitimately be absent:
        // a base nothing has been placed under yet has never been read back.
        // What is refused above is a base that does not exist at all, because
        // there is then nothing to place under and the only way forward would
        // be to invent a native project — which is exactly the fallback this
        // path must never take.
        let native_id = state
            .with_store(|store| store.get_topology_node_container(project_id, node.id))
            .map_err(|error| self.refuse(&error))?
            .map(|container| container.identity.native_id.clone());
        Ok(SessionBase { native_id, node })
    }

    /// The Quick session one command already opened, if any.
    fn quick_session_for_intent(
        &self,
        project_id: ProjectId,
        intent: &CanonicalDocument,
    ) -> Result<Option<StoredQuickSession>, ApiError> {
        self.state()?
            .with_store(|store| store.get_quick_session_by_intent(project_id, intent.hash()))
            .map_err(|error| self.refuse(&error))
    }

    /// Project one stored Quick session onto the wire.
    fn quick_session_dto(
        &self,
        session: &StoredQuickSession,
        replayed: bool,
    ) -> Result<QuickSessionDto, ApiError> {
        let state = self.state()?;
        let catalog =
            self.catalog_revision(session.role.catalog_id, session.role.catalog_revision)?;
        let segment = catalog
            .role(&session.role.role_code)
            .map(|entry| entry.segment)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the catalog revision this seat is pinned to no longer declares it",
                )
            })?;
        Ok(QuickSessionDto {
            realm_id: state.realm_id(),
            quick_session_id: session.id,
            role: ResolvedRoleRefDto {
                catalog_revision: RevisionRefDto {
                    id: session.role.catalog_id.to_string(),
                    version: session.role.catalog_revision,
                },
                role_code: session.role.role_code.clone(),
                standard_title: session.role.standard_title.clone(),
                segment,
                custom_display_name: session.role.custom_display_name.clone(),
            },
            topology_node_id: session.topology_node_id,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: session.id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: session.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    // -- Promotion and epic rosters ------------------------------------------

    /// One Quick session, or a refusal that it does not exist here.
    fn quick_session_row(
        &self,
        project_id: ProjectId,
        quick_session_id: QuickSessionId,
    ) -> Result<StoredQuickSession, ApiError> {
        self.state()?
            .with_store(|store| store.get_quick_session(project_id, quick_session_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such Quick session exists in this project",
                )
            })
    }

    /// An epic, when it exists.
    fn epic_row_opt(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<Option<kontor_core::repository::MiniProject>, ApiError> {
        self.state()?
            .with_store(|store| store.get_mini_project(project_id, epic_id))
            .map_err(|error| self.refuse(&error))
    }

    /// A Quick session that may still be promoted, and the roster it would
    /// freeze.
    fn promotable(
        &self,
        project_id: ProjectId,
        quick_session_id: QuickSessionId,
    ) -> Result<(StoredQuickSession, FrozenRoster), ApiError> {
        let session = self.quick_session_row(project_id, quick_session_id)?;
        if self
            .state()?
            .with_store(|store| store.get_promotion(quick_session_id))
            .map_err(|error| self.refuse(&error))?
            .is_some()
        {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this Quick session has already been promoted",
            ));
        }
        // The base is re-read here rather than trusted from the row: a session
        // whose base has drifted since it was opened must not carry that drift
        // into a new epic. Compared only where both sides have something to
        // say: a base since read back as a *different* native project is drift
        // and refuses, while one that has still never been read back is not a
        // disagreement, and treating it as one would block every promotion in a
        // realm whose runtime has not answered yet.
        let base = self.session_base(project_id)?;
        if matches!(
            (&base.native_id, &session.psw_native_id),
            (Some(observed), Some(placed)) if observed != placed
        ) {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the session base no longer reads back as the one this session was placed under",
            ));
        }
        let current = self.stored_core_team(project_id)?.ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this project has published no Core Team revision",
            )
        })?;
        Ok((
            session,
            FrozenRoster {
                revision: current,
                revision_of_epic: AggregateRevision::INITIAL,
                quick_session_id: Some(quick_session_id),
            },
        ))
    }

    /// The roster one epic froze, or a refusal that it has none.
    fn frozen_roster(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<FrozenRoster, ApiError> {
        let stored = self
            .state()?
            .with_store(|store| store.get_epic_roster(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "this epic has frozen no Core Team roster",
                )
            })?;
        Ok(FrozenRoster {
            revision: CoreTeamRevision {
                version: stored.core_team_version,
                catalog_hash: stored.catalog_hash,
                seats: serde_json::from_value(stored.seats).map_err(|_| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "a stored epic roster cannot be read by this build",
                    )
                })?,
            },
            revision_of_epic: stored.revision,
            quick_session_id: stored.quick_session_id,
        })
    }

    /// One project Core Team revision by version.
    fn published_core_team(
        &self,
        project_id: ProjectId,
        version: SpecVersion,
    ) -> Result<CoreTeamRevision, ApiError> {
        let current = self.stored_core_team(project_id)?.ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this project has published no Core Team revision",
            )
        })?;
        if current.version != version {
            return Err(self.deny(
                ApiErrorCode::NotFound,
                "this project has not published that Core Team revision as its current one",
            ));
        }
        Ok(current)
    }

    /// The stored shape of one epic's frozen roster.
    fn epic_roster_row(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        roster: &FrozenRoster,
        quick_session_id: Option<QuickSessionId>,
        now: Timestamp,
    ) -> Result<StoredEpicRoster, ApiError> {
        Ok(StoredEpicRoster {
            project_id,
            mini_project_id: epic_id,
            core_team_version: roster.revision.version,
            catalog_hash: roster.revision.catalog_hash.clone(),
            seats: serde_json::to_value(&roster.revision.seats).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the frozen roster could not be canonicalized",
                )
            })?,
            quick_session_id,
            revision: AggregateRevision::INITIAL,
            pinned_at: now,
        })
    }

    /// Move the roster an epic is staffed from.
    fn freeze_roster(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        roster: &FrozenRoster,
        quick_session_id: Option<QuickSessionId>,
        now: Timestamp,
    ) -> Result<(), ApiError> {
        let row = self.epic_roster_row(project_id, epic_id, roster, quick_session_id, now)?;
        self.state()?
            .with_store(|store| store.put_epic_roster(&row))
            .map_err(|error| self.refuse(&error))
    }

    /// Create every required/default seat the roster declares and the control
    /// plane does not already hold, and report all of them.
    ///
    /// On-demand roles are left absent: their declaration says an epic *may*
    /// need them, which is not the same as permission to open them at bootstrap.
    fn materialize_roster_seats(
        &self,
        project_id: ProjectId,
        control: &SessionTopologyNode,
        roster: &FrozenRoster,
        now: Timestamp,
    ) -> Result<Vec<(CoreTeamSeat, SeatBindingId)>, ApiError> {
        let state = self.state()?;
        let held = state
            .with_store(|store| store.list_seat_bindings(project_id, control.id))
            .map_err(|error| self.refuse(&error))?;
        let deadline = now
            .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
            .unwrap_or(now);
        let mut seats = Vec::new();
        for seat in roster
            .revision
            .seats
            .iter()
            .filter(|seat| seat.presence != EpicPresence::OnDemand)
        {
            if let Some(existing) = held.iter().find(|binding| {
                binding.role_slot_id == seat.role_slot_id && binding.is_non_terminal()
            }) {
                seats.push((seat.clone(), existing.id));
                continue;
            }
            let id = SeatBindingId::generate();
            state
                .with_store(|store| {
                    store.create_seat_binding(&NewSeatBinding {
                        id,
                        project_id,
                        topology_node_id: control.id,
                        role_slot_id: seat.role_slot_id.clone(),
                        role: seat.role.clone(),
                        // A Core Team seat is persistent control-plane presence,
                        // not delivery work: no task, no TeamRun, and so no
                        // mission slot consumed.
                        task_id: None,
                        team_run_id: None,
                        attach_deadline: deadline,
                        parent_seat_binding_id: None,
                        created_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
            seats.push((seat.clone(), id));
        }
        Ok(seats)
    }

    /// The immutable capsule promotion hands to the epic's lead architect.
    ///
    /// Server-owned throughout. The promotion contract carries no body, so
    /// every field here is read from what Kontor durably knows about the
    /// source: its identity, the base it ran in, its purpose, and the roster
    /// and epic it is being handed to.
    fn promotion_handoff(
        &self,
        session: &StoredQuickSession,
        roster: &FrozenRoster,
        epic_id: MiniProjectId,
        lsa: SeatBindingId,
    ) -> Result<serde_json::Value, ApiError> {
        Ok(serde_json::json!({
            "schema_version": 1,
            "realm_id": self.state()?.realm_id().to_string(),
            "continuation": "cross_engine_handoff",
            "source": {
                "quick_session_id": session.id.to_string(),
                "topology_node_id": session.topology_node_id.to_string(),
                "seat_binding_id": session.seat_binding_id.to_string(),
                "role_code": session.role.role_code.as_str(),
                "session_base": session.psw_native_id.as_ref().map(ExternalId::as_str),
                "opened_at": session.created_at.to_string(),
            },
            "purpose": session.purpose.as_str(),
            "target": {
                "epic_id": epic_id.to_string(),
                "lsa_seat_binding_id": lsa.to_string(),
                "core_team_version": roster.revision.version.get(),
                "catalog": roster.revision.catalog_hash.as_str(),
            },
            "recommended_next_action":
                "Continue the work opened in the Quick session, now under this epic.",
        }))
    }

    /// The digest an apply must name to prove it saw this promotion preview.
    fn promotion_hash(
        &self,
        session: &StoredQuickSession,
        roster: &FrozenRoster,
        effects: &[TopologyUpgradeEffectDto],
    ) -> Result<ContentHash, ApiError> {
        self.preview_hash(&serde_json::json!({
            "schema_version": 1,
            "operation": "promotion_preview",
            "source": session.id.to_string(),
            "source_revision": session.revision.get(),
            "core_team_version": roster.revision.version.get(),
            "catalog": roster.revision.catalog_hash.as_str(),
            "effects": effect_digest(effects),
        }))
    }

    /// The digest a roster-upgrade apply must name.
    fn roster_upgrade_hash(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        target: &CoreTeamRevision,
        effects: &[TopologyUpgradeEffectDto],
    ) -> Result<ContentHash, ApiError> {
        self.preview_hash(&serde_json::json!({
            "schema_version": 1,
            "operation": "epic_roster_upgrade_preview",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "target_version": target.version.get(),
            "catalog": target.catalog_hash.as_str(),
            "effects": effect_digest(effects),
        }))
    }

    /// Find the published revision one roster preview was computed against.
    ///
    /// Recomputed rather than remembered, for the reason a topology upgrade
    /// recomputes its own: a stored preview would let an apply commit a diff
    /// the realm no longer has.
    fn target_of_roster_preview(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        preview_hash: &ContentHash,
    ) -> Result<FrozenRoster, ApiError> {
        let current = self.frozen_roster(project_id, epic_id)?;
        let target = self.stored_core_team(project_id)?.ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this project has published no Core Team revision",
            )
        })?;
        let effects = roster_upgrade_effects(&current, &target)
            .map_err(|error| self.refuse_domain(&error))?;
        if &self.roster_upgrade_hash(project_id, epic_id, &target, &effects)? != preview_hash {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "no currently published roster still produces the previewed effects",
            ));
        }
        Ok(FrozenRoster {
            revision: target,
            revision_of_epic: current.revision_of_epic,
            quick_session_id: current.quick_session_id,
        })
    }

    /// One epic's roster, with the seats that are actually filling it.
    fn epic_core_team_dto(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        roster: &FrozenRoster,
    ) -> Result<CoreTeamDto, ApiError> {
        let state = self.state()?;
        let control = self
            .state()?
            .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|node| node.kind == self.domain.delivery.control_kind);
        let held = match &control {
            Some(node) => state
                .with_store(|store| store.list_seat_bindings(project_id, node.id))
                .map_err(|error| self.refuse(&error))?,
            None => Vec::new(),
        };
        let mut seats = self.core_team_seat_dtos(&roster.revision)?;
        for seat in &mut seats {
            // Scoped by the epic in the route, which is what makes reporting a
            // binding here honest: this projection is one epic's control plane.
            seat.seat_binding_id = held
                .iter()
                .find(|binding| {
                    binding.role.role_code == seat.role.role_code && binding.is_non_terminal()
                })
                .map(|binding| binding.id);
        }
        Ok(CoreTeamDto {
            realm_id: state.realm_id(),
            project_id,
            seats,
            revision: roster.revision_of_epic,
            snapshot_cursor: self.cursor()?,
        })
    }

    /// The digest an apply must name to prove it saw this preview.
    fn core_team_hash(
        &self,
        project_id: ProjectId,
        proposed: &CoreTeamRevision,
        effects: &[TopologyUpgradeEffectDto],
    ) -> Result<ContentHash, ApiError> {
        self.preview_hash(&serde_json::json!({
            "schema_version": 1,
            "operation": "core_team_preview",
            "project": project_id.to_string(),
            // The revision this would become and the exact catalog it resolved
            // against are both in the digest: the same seats resolved against a
            // corrected catalog are not the same authorization.
            "version": proposed.version.get(),
            "catalog": proposed.catalog_hash.as_str(),
            "effects": effects
                .iter()
                .map(|effect| {
                    serde_json::json!({
                        "subject": effect.subject,
                        "effect": effect.effect,
                        "detail": effect.detail,
                    })
                })
                .collect::<Vec<_>>(),
        }))
    }

    /// Project one stored revision's seats onto the wire.
    fn core_team_seat_dtos(
        &self,
        revision: &CoreTeamRevision,
    ) -> Result<Vec<CoreTeamSeatDto>, ApiError> {
        revision
            .seats
            .iter()
            .map(|seat| {
                let catalog =
                    self.catalog_revision(seat.role.catalog_id, seat.role.catalog_revision)?;
                let segment = catalog
                    .role(&seat.role.role_code)
                    .map(|entry| entry.segment)
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::Unavailable,
                            "the catalog revision this seat is pinned to no longer declares it",
                        )
                    })?;
                Ok(CoreTeamSeatDto {
                    role: ResolvedRoleRefDto {
                        catalog_revision: RevisionRefDto {
                            id: seat.role.catalog_id.to_string(),
                            version: seat.role.catalog_revision,
                        },
                        role_code: seat.role.role_code.clone(),
                        standard_title: seat.role.standard_title.clone(),
                        segment,
                        custom_display_name: seat.role.custom_display_name.clone(),
                    },
                    presence: seat.presence,
                    ad_hoc_allowed: seat.ad_hoc_allowed,
                    seat_binding_id: None,
                })
            })
            .collect()
    }

    /// The digest an apply must name to prove it saw this preview.
    fn upgrade_hash(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        current: &TopologySnapshot,
        target: &TopologySnapshot,
        effects: &[TopologyUpgradeEffectDto],
    ) -> Result<ContentHash, ApiError> {
        self.preview_hash(&serde_json::json!({
            "schema_version": 1,
            "operation": "topology_upgrade_preview",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "current": current.canonical_hash.as_str(),
            "target": target.canonical_hash.as_str(),
            "effects": effects
                .iter()
                .map(|effect| {
                    serde_json::json!({
                        "subject": effect.subject,
                        "node": effect.topology_node_id.map(|id| id.to_string()),
                        "effect": effect.effect,
                        "detail": effect.detail,
                    })
                })
                .collect::<Vec<_>>(),
        }))
    }

    /// Which published revision produces `preview_hash` for this epic.
    ///
    /// The apply names its preview by digest and never by target, so the server
    /// searches its own published revisions for the one that still produces
    /// exactly those effects. A hash that matches none of them is a preview the
    /// Realm has moved past — refused rather than re-derived, because the caller
    /// authorized the effects it saw and not whatever replaced them.
    fn target_of_preview(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        preview_hash: &ContentHash,
    ) -> Result<TopologySnapshot, ApiError> {
        let state = self.state()?;
        let published = state
            .with_store(|store| store.list_topology_specs(project_id))
            .map_err(|error| self.refuse(&error))?;
        for candidate in published {
            let reference = RevisionRefDto {
                id: candidate.spec_id.to_string(),
                version: candidate.version,
            };
            let Ok((current, target, effects)) =
                self.upgrade_effects(project_id, epic_id, &reference)
            else {
                continue;
            };
            if &self.upgrade_hash(project_id, epic_id, &current, &target, &effects)? == preview_hash
            {
                return Ok(target);
            }
        }
        Err(self.deny(
            ApiErrorCode::RevisionConflict,
            "no published revision still produces the preview this apply names",
        ))
    }

    /// One semantic scope, resolved to the chain of nodes that realizes it.
    ///
    /// This is where the semantic boundary is actually enforced. A caller names
    /// a *meaning* — this epic, that consultation — and everything native about
    /// it is derived here from the pinned specification and the seeded delivery
    /// binding: the kind, the parent, the epic scope, the delivery task. None of
    /// those may arrive in a request, and none of them is spelled as a literal
    /// in this file.
    fn resolve_scope(
        &self,
        project_id: ProjectId,
        target: &SemanticTopologyTargetDto,
    ) -> Result<TopologyScope, ApiError> {
        let delivery = &self.domain.delivery;
        Ok(match target {
            SemanticTopologyTargetDto::ProjectRoot => TopologyScope {
                kind: None,
                epic_id: None,
                task_id: None,
                key: "project_root".to_owned(),
            },
            SemanticTopologyTargetDto::QuickSession { quick_session_id } => TopologyScope {
                kind: Some(delivery.quick_kind.clone()),
                epic_id: None,
                task_id: None,
                key: format!("quick_session:{quick_session_id}"),
            },
            SemanticTopologyTargetDto::Epic { epic_id } => TopologyScope {
                kind: Some(delivery.epic_kind.clone()),
                epic_id: Some(self.epic_row(project_id, *epic_id)?.id),
                task_id: None,
                key: format!("epic:{epic_id}"),
            },
            SemanticTopologyTargetDto::EpicControl { epic_id } => TopologyScope {
                kind: Some(delivery.control_kind.clone()),
                epic_id: Some(self.epic_row(project_id, *epic_id)?.id),
                task_id: None,
                key: format!("epic_control:{epic_id}"),
            },
            SemanticTopologyTargetDto::Ticket { task_id } => {
                let task = self.task_row(project_id, *task_id)?;
                TopologyScope {
                    kind: Some(delivery.task_kind.clone()),
                    epic_id: Some(task.mini_project_id.ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::PlacementBlocked,
                            "this task belongs to no epic, so it has no place in the topology",
                        )
                    })?),
                    task_id: Some(task.id),
                    key: format!("ticket:{task_id}"),
                }
            }
            // A consultation is scoped to the epic its run belongs to. Both
            // successor families are OP-05's to open, so the run itself is not
            // resolvable here yet — what *is* resolvable, and what the topology
            // needs, is a consultation node below an epic the caller can name.
            SemanticTopologyTargetDto::AdvisorConsultation { .. } => {
                return Err(self.deny(
                    ApiErrorCode::Unavailable,
                    "advisor consultations are opened by the service that owns them, \
                     which is not composed in this build",
                ));
            }
            SemanticTopologyTargetDto::CommitteeConsultation { .. } => {
                return Err(self.deny(
                    ApiErrorCode::Unavailable,
                    "committee consultations are opened by the service that owns them, \
                     which is not composed in this build",
                ));
            }
        })
    }

    /// Create the chain of nodes one scope needs, and return its leaf.
    ///
    /// Every level is looked up before it is created, so ensuring twice creates
    /// nothing the second time. The chain itself is the specification's: the
    /// root kind it declares, then the epic, then the scope's own kind below
    /// whichever of those the vocabulary allows as its parent.
    fn ensure_scope_chain(
        &self,
        project_id: ProjectId,
        scope: &TopologyScope,
    ) -> Result<SessionTopologyNode, ApiError> {
        let state = self.state()?;
        let topology = self.project_topology(project_id)?;
        let spec = self.pinned_spec(project_id)?;
        let now = kontor_api::now();

        let unscoped = state
            .with_store(|store| store.list_topology_nodes(project_id, None))
            .map_err(|error| self.refuse(&error))?;
        let root = self.ensure_node(
            unscoped
                .iter()
                .find(|node| node.kind == spec.root_kind && node.parent_id.is_none()),
            NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id,
                mini_project_id: None,
                topology: topology.clone(),
                kind: spec.root_kind.clone(),
                parent_id: None,
                task_id: None,
                created_at: now,
            },
        )?;
        let Some(kind) = scope.kind.clone() else {
            return Ok(root);
        };

        let Some(epic_id) = scope.epic_id else {
            // A scope with a kind but no epic hangs directly off the root — a
            // Quick session is the one the bundled vocabulary declares.
            let existing = unscoped.iter().find(|node| node.kind == kind);
            return self.ensure_node(
                existing,
                NewSessionTopologyNode {
                    id: TopologyNodeId::generate(),
                    project_id,
                    mini_project_id: None,
                    topology,
                    kind,
                    parent_id: Some(root.id),
                    task_id: None,
                    created_at: now,
                },
            );
        };

        self.pin_epic_topology(project_id, epic_id, &topology)?;
        let scoped = state
            .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
            .map_err(|error| self.refuse(&error))?;
        let epic = self.ensure_node(
            scoped
                .iter()
                .find(|node| node.kind == self.domain.delivery.epic_kind),
            NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id,
                mini_project_id: Some(epic_id),
                topology: topology.clone(),
                kind: self.domain.delivery.epic_kind.clone(),
                parent_id: Some(root.id),
                task_id: None,
                created_at: now,
            },
        )?;
        if kind == self.domain.delivery.epic_kind {
            return Ok(epic);
        }

        // A task-scoped node is unique per task; every other epic-scoped kind
        // is unique per epic. Matching on the task when there is one is what
        // keeps two tickets in one epic from resolving to each other's node.
        let existing = scoped.iter().find(|node| {
            node.kind == kind && (scope.task_id.is_none() || node.task_id == scope.task_id)
        });
        self.ensure_node(
            existing,
            NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id,
                mini_project_id: Some(epic_id),
                topology,
                kind,
                parent_id: Some(epic.id),
                task_id: scope.task_id,
                created_at: now,
            },
        )
    }

    /// The nodes one scope covers, for a readback.
    fn scope_nodes(
        &self,
        project_id: ProjectId,
        scope: &TopologyScope,
    ) -> Result<Vec<SessionTopologyNode>, ApiError> {
        let state = self.state()?;
        let nodes = state
            .with_store(|store| store.list_project_topology_nodes(project_id))
            .map_err(|error| self.refuse(&error))?;
        Ok(match (scope.epic_id, &scope.kind) {
            (Some(epic_id), _) => nodes
                .into_iter()
                .filter(|node| node.mini_project_id == Some(epic_id))
                .collect(),
            (None, _) => nodes,
        })
    }

    /// Pin one epic to the project's selected topology revision, once.
    ///
    /// A pin already there is never rewritten: repinning an epic would silently
    /// move every node already placed under it.
    fn pin_epic_topology(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        topology: &TopologySnapshot,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        match state
            .with_store(|store| store.get_mini_project_topology(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
        {
            Some(pinned) if &pinned.topology != topology => Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "this epic is pinned to another topology revision than the project selects",
            )),
            Some(_) => Ok(()),
            None => state
                .with_store(|store| {
                    store.pin_mini_project_topology(&MiniProjectTopologySnapshot {
                        project_id,
                        mini_project_id: epic_id,
                        topology: topology.clone(),
                        pinned_at: kontor_api::now(),
                    })
                })
                .map_err(|error| self.refuse(&error)),
        }
    }

    /// The specification revision this project's topology is pinned to.
    fn pinned_spec(
        &self,
        project_id: ProjectId,
    ) -> Result<kontor_core::spec::ProjectSessionTopologySpec, ApiError> {
        let state = self.state()?;
        let topology = self.project_topology(project_id)?;
        state
            .with_store(|store| {
                store.get_topology_spec(project_id, topology.spec_id, topology.version)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the selected topology revision is not published in this project",
                )
            })
    }

    /// The project, proved to be at the revision the caller read.
    fn project_at(
        &self,
        project_id: ProjectId,
        expected_revision: AggregateRevision,
    ) -> Result<kontor_core::repository::Project, ApiError> {
        let state = self.state()?;
        let project = state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })?;
        if project.revision != expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the project moved since the caller read it",
                )
                .with_revision(Some(project.revision)));
        }
        Ok(project)
    }

    /// Move one already-returned node along its one-way lifecycle.
    fn move_node_lifecycle(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        request: &TopologyNodeRequest,
        lifecycle: TopologyLifecycle,
    ) -> Result<TopologyMutationDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let project = state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })?;
        let node = state
            .with_store(|store| store.get_topology_node(project_id, topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such topology node exists in this project",
                )
            })?;
        if node.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the topology node moved since the caller read it",
                )
                .with_revision(Some(node.revision)));
        }

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": match lifecycle {
                TopologyLifecycle::Retired => "topology_node_retire",
                TopologyLifecycle::Archived => "topology_node_archive",
                TopologyLifecycle::Active => "topology_node_reopen",
            },
            "project": project_id.to_string(),
            "node": topology_node_id.to_string(),
            "reason": request.reason.as_str(),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();
        let moved = if replayed {
            node
        } else {
            state
                .with_store(|store| {
                    store.transition_topology_node(
                        project_id,
                        topology_node_id,
                        lifecycle,
                        request.expected_revision,
                        now,
                    )
                })
                .map_err(|error| self.refuse(&error))?
        };
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::TransitionEpic,
            AggregateRef::MiniProject {
                mini_project_id: moved.mini_project_id.ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "an unscoped node has no epic for its lifecycle receipt to name",
                    )
                })?,
            },
            project.revision,
            &intent,
        )?;
        self.topology_mutation(
            project_id,
            moved.mini_project_id,
            receipt_id,
            replayed,
            moved.revision,
        )
    }

    /// One semantic write's answer: the topology as it now stands, and the
    /// receipt it was committed under.
    fn topology_mutation(
        &self,
        project_id: ProjectId,
        epic_id: Option<MiniProjectId>,
        receipt_id: CommandReceiptId,
        replayed: bool,
        revision: AggregateRevision,
    ) -> Result<TopologyMutationDto, ApiError> {
        let state = self.state()?;
        Ok(TopologyMutationDto {
            projection: self.topology_projection(project_id, epic_id)?,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    /// One project's topology, as stored.
    ///
    /// The native half of every node is *evidence*: the derived desired shape
    /// and, where anything has been read back, the exact identity observed.
    /// Their presence in an answer does not make them legal in a request, which
    /// is what keeps the model-facing boundary semantic.
    fn topology_projection(
        &self,
        project_id: ProjectId,
        epic_id: Option<MiniProjectId>,
    ) -> Result<TopologyProjectionDto, ApiError> {
        let state = self.state()?;
        let topology = self.project_topology(project_id)?;
        let spec = self.pinned_spec(project_id)?;
        let nodes = state
            .with_store(|store| store.list_project_topology_nodes(project_id))
            .map_err(|error| self.refuse(&error))?;

        let mut projected = Vec::new();
        for node in nodes {
            if epic_id.is_some() && node.mini_project_id != epic_id {
                continue;
            }
            let declared = spec
                .node_kinds
                .iter()
                .find(|declared| declared.kind == node.kind)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "a stored node names a kind the pinned specification no longer declares",
                    )
                })?;
            let container = state
                .with_store(|store| store.get_topology_node_container(project_id, node.id))
                .map_err(|error| self.refuse(&error))?;
            let seats = state
                .with_store(|store| store.list_seat_bindings(project_id, node.id))
                .map_err(|error| self.refuse(&error))?;
            projected.push(TopologyNodeDto {
                topology_node_id: node.id,
                parent_topology_node_id: node.parent_id,
                kind_key: node.kind.clone(),
                lifecycle: node.lifecycle,
                placement: node.placement,
                desired_binding: DesiredBindingDto {
                    runtime_kind: self.node_runtime_kind()?,
                    projection_capabilities: declared
                        .projection_capabilities
                        .iter()
                        .map(|capability| capability.as_str().to_owned())
                        .collect(),
                },
                observed_binding: container.as_ref().map(|binding| ObservedBindingDto {
                    runtime_kind: binding.identity.runtime_kind.clone(),
                    native_id: binding.identity.native_id.clone(),
                    native_name: None,
                    cwd: binding
                        .canonical_cwd
                        .as_ref()
                        .and_then(|cwd| ExternalId::parse(cwd.as_str()).ok()),
                    observed_at: binding.last_readback_at,
                }),
                seats: seats
                    .iter()
                    .map(|seat| self.seat_dto(seat))
                    .collect::<Result<Vec<_>, _>>()?,
            });
        }

        Ok(TopologyProjectionDto {
            realm_id: state.realm_id(),
            project_id,
            pinned_spec: PinnedSpecDto {
                id: topology.spec_id,
                version: topology.version,
                canonical_hash: topology.canonical_hash,
            },
            nodes: projected,
            snapshot_cursor: self.cursor()?,
        })
    }

    /// The runtime family a node's container must come from.
    ///
    /// One configured family, because the pinned specification declares
    /// capabilities and not families: which adapter supplies them is the
    /// deployment's answer, and a Realm configured with none cannot place
    /// anything.
    fn node_runtime_kind(&self) -> Result<RuntimeKindKey, ApiError> {
        let state = self.state()?;
        state.runtimes().families().next().cloned().ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "this realm is configured with no runtime family to place a node in",
            )
        })
    }

    /// The control-plane position a read is consistent with.
    ///
    /// The Realm's own newest event cursor, which is what every other
    /// projection in this file takes its position from. Deliberately not the
    /// Teams projection counter: that one starts at zero, and a position of
    /// zero is not a position — a subscriber resuming strictly after it would
    /// be told to start before the beginning.
    fn cursor(&self) -> Result<kontor_core::id::EventCursor, ApiError> {
        let state = self.state()?;
        Ok(state
            .with_store(|store| store.realm_event_page(None, 1))
            .map_err(|error| self.refuse(&error))?
            .newest
            .cursor)
    }

    /// One bounded line a human can read.
    ///
    /// Every effect line is server-authored and short by construction, so the
    /// bound is a type-level fact rather than a length check anyone has to
    /// remember; a line this build made too long is a refusal here rather than a
    /// truncation nobody sees.
    fn detail(&self, text: &str) -> Result<BoundedText, ApiError> {
        BoundedText::parse(text).map_err(|error| self.refuse_domain(&error))
    }

    /// The hash an apply must name to prove it saw this preview.
    fn preview_hash(&self, value: &serde_json::Value) -> Result<ContentHash, ApiError> {
        Ok(self.intent(value)?.hash().clone())
    }

    /// One project's admission picture, from what is durably true.
    ///
    /// Availability is the derived conclusion of the newest raw observation,
    /// with any standing operator judgement applied *on top* rather than folded
    /// in: the account's own `override_reason` is what tells a reader the two
    /// disagreed, and the observation it cites is still the provider's word.
    fn capacity_projection(&self, project_id: ProjectId) -> Result<ProjectCapacityDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })?;

        let profiles = state
            .with_store(|store| store.list_account_profiles(project_id))
            .map_err(|error| self.refuse(&error))?;
        let observations = state
            .with_store(|store| store.latest_capacity_observations(project_id))
            .map_err(|error| self.refuse(&error))?;
        let overrides = state
            .with_store(|store| store.list_availability_overrides(project_id))
            .map_err(|error| self.refuse(&error))?;

        let accounts = profiles
            .iter()
            .map(|profile| {
                let observed = observations
                    .iter()
                    .find(|observation| observation.account_profile_id == profile.id);
                let standing = overrides
                    .iter()
                    .find(|stored| stored.account_profile_id == profile.id)
                    .filter(|stored| stored.is_standing(now));
                AccountAvailabilityDto {
                    account_profile_id: profile.id,
                    observation_id: observed.map(|observation| observation.id),
                    // No observation is not "available": nothing has been read,
                    // and admitting against a provider nobody asked is the one
                    // answer that could start work the provider never agreed to.
                    available: standing.map_or_else(
                        || observed.is_some_and(|observation| observation.available),
                        |stored| stored.available,
                    ),
                    override_reason: standing.map(|stored| stored.reason.clone()),
                    override_expires_at: standing.and_then(|stored| stored.expires_at),
                }
            })
            .collect();

        // Active admitted non-terminal TeamRun envelopes, counted once each.
        let runs = state
            .with_store(|store| store.list_team_runs(project_id))
            .map_err(|error| self.refuse(&error))?;
        let active = runs
            .iter()
            .filter(|run| !run.lifecycle.is_terminal())
            .count();

        // The widest position any epic in the project currently stands at, and
        // the trend behind it. A project with no epic applied through this
        // build reports the configured start rather than inventing a width.
        let positions = state
            .with_store(|store| store.list_adaptive_admission_states(project_id))
            .map_err(|error| self.refuse(&error))?;
        let widest = positions
            .iter()
            .max_by_key(|persisted| persisted.current_window);

        Ok(ProjectCapacityDto {
            realm_id: state.realm_id(),
            project_id,
            accounts,
            active_team_runs: u32::try_from(active).unwrap_or(u32::MAX),
            mission_ceiling: self.capacity.mission_max_in_flight,
            adaptive_width: widest.map_or(self.capacity.adaptive.initial, |persisted| {
                persisted.current_window
            }),
            adaptive_streak: widest.map_or(0, |persisted| persisted.clean_observation_streak),
            last_observation_id: widest
                .and_then(|persisted| persisted.last_observation_id.as_ref())
                .and_then(|observed| {
                    kontor_core::id::CapacityObservationId::parse(observed.as_str()).ok()
                }),
            last_refusal: None,
            snapshot_cursor: self.cursor()?,
        })
    }

    /// Fold one capacity verdict into every epic's persisted position.
    ///
    /// The arithmetic is the scheduler's and the transition is the account
    /// layer's; this only supplies the evidence and persists the answer. A
    /// position the fold leaves unchanged is not written at all, which is what
    /// makes a replayed observation cost nothing.
    fn fold_admission(
        &self,
        project_id: ProjectId,
        observation_id: &str,
        observation: kontor_scheduler::model::CapacityObservation,
        now: Timestamp,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let evidence =
            ExternalId::parse(observation_id).map_err(|error| self.refuse_domain(&error))?;
        let positions = state
            .with_store(|store| store.list_adaptive_admission_states(project_id))
            .map_err(|error| self.refuse(&error))?;
        for persisted in positions {
            let current = AdaptivePosition {
                current_window: persisted.current_window,
                clean_observation_streak: persisted.clean_observation_streak,
                last_observation_id: persisted.last_observation_id.clone(),
            };
            let folded =
                kontor_accounts::fold(self.capacity.adaptive, &current, &evidence, observation);
            if folded == current {
                continue;
            }
            state
                .with_store(|store| {
                    store.advance_adaptive_admission_state(&AdaptiveAdmissionAdvance {
                        project_id,
                        mini_project_id: persisted.mini_project_id,
                        current_window: folded.current_window,
                        clean_observation_streak: folded.clean_observation_streak,
                        last_observation_id: folded.last_observation_id.clone(),
                        expected_revision: persisted.revision,
                        updated_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }
        Ok(())
    }

    /// Observe or retire one exact seat.
    ///
    /// Both operations address the binding id and nothing else — never a name,
    /// never a `cwd`, never a scan — and both read the runtime back before they
    /// conclude anything. The only difference is what is recorded afterwards,
    /// which is why they are one function: two copies would eventually diverge
    /// on how a seat is *found*, and that is the part that must not.
    async fn address_exact_seat(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        request: &SeatBindingRequest,
        act: SeatAct,
    ) -> Result<SeatBindingOutcomeDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let project = state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })?;
        let seat = state
            .with_store(|store| store.get_seat_binding(project_id, seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such seat binding exists in this project",
                )
            })?;
        if seat.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the seat binding moved since the caller read it",
                )
                .with_revision(Some(seat.revision)));
        }

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": act.operation(),
            "project": project_id.to_string(),
            "seat_binding": seat_binding_id.to_string(),
            "reason": request.reason.as_str(),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();

        // The runtime is read back through the node's stored container binding,
        // which is the only native identity Kontor holds for this seat. A seat
        // whose node was never placed has nothing to observe, and saying so is
        // the honest answer — not an empty reading a caller would read as "the
        // runtime replied and found nothing".
        let container = state
            .with_store(|store| {
                store.get_topology_node_container(project_id, seat.topology_node_id)
            })
            .map_err(|error| self.refuse(&error))?;
        let observed = container.as_ref().map(|binding| ObservedBindingDto {
            runtime_kind: binding.identity.runtime_kind.clone(),
            native_id: binding.identity.native_id.clone(),
            // The four-part native identity carries no display name; the
            // container's own name is not something Kontor stores, and
            // inventing one from the id would be a second answer.
            native_name: None,
            cwd: binding
                .canonical_cwd
                .as_ref()
                .and_then(|cwd| ExternalId::parse(cwd.as_str()).ok()),
            observed_at: binding.last_readback_at,
        });

        let seat = if replayed {
            seat
        } else {
            state
                .with_store(|store| {
                    store.observe_seat_binding(
                        project_id,
                        seat_binding_id,
                        &act.observation(container.is_some(), now),
                        now,
                    )
                })
                .map_err(|error| self.refuse(&error))?
        };
        let receipt_id = self.record(
            key,
            project_id,
            act.command_kind(),
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;

        Ok(SeatBindingOutcomeDto {
            seat: self.seat_dto(&seat)?,
            observed_binding: observed,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: seat.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    /// One seat as a projection reports it.
    ///
    /// The segment comes from the catalog revision the seat is pinned to, not
    /// from the newest one: a seat's role is the role it was created with, and
    /// re-resolving it against a later catalog would silently reclassify work
    /// that has already happened.
    fn seat_dto(
        &self,
        seat: &kontor_core::state::SeatBinding,
    ) -> Result<TopologySeatDto, ApiError> {
        let segment = self
            .domain
            .role_catalogs
            .iter()
            .find(|catalog| {
                catalog.catalog_id == seat.role.catalog_id
                    && catalog.version == seat.role.catalog_revision
            })
            .and_then(|catalog| {
                catalog
                    .roles
                    .iter()
                    .find(|entry| entry.role_code == seat.role.role_code)
            })
            .map(|entry| entry.segment)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the catalog revision this seat is pinned to is not in this build",
                )
            })?;
        Ok(TopologySeatDto {
            seat_binding_id: seat.id,
            role_slot_id: seat.role_slot_id.to_string(),
            role: ResolvedRoleRefDto {
                catalog_revision: RevisionRefDto {
                    id: seat.role.catalog_id.to_string(),
                    version: seat.role.catalog_revision,
                },
                role_code: seat.role.role_code.clone(),
                standard_title: seat.role.standard_title.clone(),
                segment,
                custom_display_name: seat.role.custom_display_name.clone(),
            },
            lifecycle: seat.lifecycle,
        })
    }

    /// What the mission ceiling is currently counting.
    ///
    /// One active TeamRun envelope counts once. Not its seats: a team of five
    /// filling one envelope is one piece of work in flight, and counting the
    /// seats would refuse the second epic at a ceiling meant to allow seven.
    /// A persistent idle SeatBinding counts for nothing at all — it is a seat
    /// waiting to be used, not work being done.
    fn mission_usage(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        tasks: &[kontor_core::repository::Task],
    ) -> Result<CapacityUsage, ApiError> {
        let state = self.state()?;
        let in_epic: BTreeSet<TaskId> = tasks.iter().map(|task| task.id).collect();
        let runs = state
            .with_store(|store| store.list_team_runs(project_id))
            .map_err(|error| self.refuse(&error))?;
        let active = runs
            .iter()
            .filter(|run| !run.lifecycle.is_terminal() && in_epic.contains(&run.task_id))
            .count();
        let mut usage = CapacityUsage::default();
        if active > 0 {
            usage
                .mission_in_flight
                .insert(epic_id, u32::try_from(active).unwrap_or(u32::MAX));
        }
        Ok(usage)
    }

    /// The account pin one task carries, as admission evidence.
    ///
    /// A task with no selection has no pin, which is not "any account will do":
    /// there is no account, so there is nothing to prove about one. A task *with*
    /// a selection carries the profile's current revision alongside the one that
    /// was pinned, so the planner can refuse a pin that moved underneath it.
    fn account_evidence(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        runtime: &RuntimeAdmissionEvidence,
    ) -> Result<AccountAdmissionEvidence, ApiError> {
        let state = self.state()?;
        let Some((account_profile_id, pinned_revision)) = state
            .with_store(|store| store.task_account_selection(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(AccountAdmissionEvidence {
                pin: None,
                required_capabilities: BTreeSet::new(),
            });
        };
        let Some(profile) = state
            .with_store(|store| store.get_account_profile(project_id, account_profile_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(AccountAdmissionEvidence {
                pin: None,
                required_capabilities: BTreeSet::new(),
            });
        };
        Ok(AccountAdmissionEvidence {
            pin: Some(kontor_scheduler::model::AccountPin {
                account_profile_id,
                pinned_revision,
                current_revision: profile.revision,
                enabled: profile.enabled,
                cooldown_until: None,
                harness: profile.harness.clone(),
                declared_capabilities: BTreeSet::new(),
                provider_identity: profile.provider_identity.clone(),
                preflight: kontor_scheduler::model::FleetPreflight {
                    outcome: kontor_scheduler::model::PreflightOutcome::Passed,
                    evidence_hash: ContentHash::of(profile.capability.json().as_bytes()),
                    // The same instant the snapshot is taken at, for the reason
                    // the runtime's confirmation is: a preflight dated after the
                    // snapshot is evidence from the future.
                    observed_at: runtime.last_confirmed_at.unwrap_or_else(kontor_api::now),
                },
            }),
            required_capabilities: BTreeSet::new(),
        })
    }

    /// What the one configured runtime family can prove, as admission evidence.
    ///
    /// A Realm with no fleet, or one whose runtime cannot be reached, reports
    /// [`RuntimeHealth::Unavailable`] and an incomplete reconciliation: the
    /// planner then blocks every candidate with a named reason instead of the
    /// call failing, which is what a Lead asking "why is nothing running" needs.
    async fn runtime_evidence(
        &self,
        project_id: ProjectId,
        taken_at: Timestamp,
    ) -> Result<RuntimeAdmissionEvidence, ApiError> {
        let state = self.state()?;
        let family = state.runtimes().families().next().cloned();
        let required = kontor_scheduler::ready::minimum_launch_capabilities();
        let host = ExternalName::parse("loopback").map_err(|error| self.refuse_domain(&error))?;
        let Some(family) = family else {
            return Ok(unreachable_runtime(
                project_id,
                RuntimeKindKey::parse("none").map_err(|error| self.refuse_domain(&error))?,
                host,
                required,
            ));
        };
        let Some(adapter) = state.runtimes().get(&family) else {
            return Ok(unreachable_runtime(project_id, family, host, required));
        };
        let Ok(capabilities) = adapter.discover_capabilities().await else {
            return Ok(unreachable_runtime(project_id, family, host, required));
        };
        let open = state.barrier().state().is_open();
        Ok(RuntimeAdmissionEvidence {
            runtime_kind: family.clone(),
            host: host.clone(),
            generation: 1,
            capabilities,
            required,
            health: RuntimeHealth::Healthy,
            reconciliation: ReconciliationEvidence {
                // The barrier *is* the reconciliation fact: it opens only when
                // every configured runtime answered, so reporting anything else
                // here would be a second opinion about the same sweep.
                epoch_completed: open,
                scope: ReconciliationScope {
                    project_id,
                    runtime_kind: family,
                    host,
                    generation: 1,
                },
                open_replay_gap: false,
                divergence: false,
                orphan_ambiguity: false,
                stale_lost_contact: false,
            },
            // The same instant the snapshot is taken at. Reading a second clock
            // here would produce a confirmation dated after the snapshot, which
            // the planner correctly refuses as evidence from the future.
            last_confirmed_at: Some(taken_at),
        })
    }
}

/// Evidence for a family this process cannot currently ask anything.
fn unreachable_runtime(
    project_id: ProjectId,
    family: RuntimeKindKey,
    host: ExternalName,
    required: BTreeSet<RuntimeCapability>,
) -> RuntimeAdmissionEvidence {
    RuntimeAdmissionEvidence {
        runtime_kind: family.clone(),
        host: host.clone(),
        generation: 1,
        capabilities: kontor_runtime::capability::RuntimeCapabilities {
            trust_grade: kontor_runtime::capability::TrustGrade::C,
            supported: BTreeSet::new(),
            account_env: false,
            limits: kontor_runtime::capability::RuntimeLimits {
                max_message_bytes: 0,
                max_history_page: 0,
                max_concurrent_sessions: 0,
                context_window: kontor_core::spec::ContextWindowBounds::unknown(),
            },
        },
        required,
        health: RuntimeHealth::Unavailable,
        reconciliation: ReconciliationEvidence {
            epoch_completed: false,
            scope: ReconciliationScope {
                project_id,
                runtime_kind: family,
                host,
                generation: 1,
            },
            open_replay_gap: false,
            divergence: false,
            orphan_ambiguity: false,
            stale_lost_contact: false,
        },
        last_confirmed_at: None,
    }
}

/// The digest that names one plan.
///
/// It covers the *decisions* and not the instant the plan was taken at. A plan is
/// a batch — this task admitted under that authorization, that one blocked for
/// this reason — and two reads a second apart describe the same batch. Hashing
/// `taken_at` as well would make every plan un-startable by construction, which
/// is a different property from "the Realm has not moved": that one is checked by
/// re-deriving the plan at start time and comparing what it decided.
fn plan_digest(
    plan: &kontor_scheduler::model::Plan,
) -> kontor_core::DomainResult<CanonicalDocument> {
    CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "decisions": plan.decisions,
    }))
}

/// Flatten a stored authorization into the shape the planner reads.
fn evidence_of(authorization: &ExecutionAuthorization) -> AuthorizationEvidence {
    AuthorizationEvidence {
        id: authorization.id,
        project_id: authorization.project_id,
        scope: authorization.scope,
        selected_tasks: authorization.selected_tasks.iter().copied().collect(),
        allowed_start: authorization.allowed_start.start,
        allowed_end: authorization.allowed_start.end,
        max_concurrency: authorization.max_concurrency,
    }
}

/// The wire view of one stored authorization.
fn authorization_dto(stored: &kontor_store::StoredAuthorization) -> AuthorizationProjectionDto {
    AuthorizationProjectionDto {
        authorization_id: stored.authorization.id.to_string(),
        scope: match stored.authorization.scope {
            WorkScope::Project => "project".to_owned(),
            WorkScope::MiniProject { .. } => "epic".to_owned(),
            WorkScope::Task { .. } => "task".to_owned(),
        },
        selected_tasks: stored.authorization.selected_tasks.clone(),
        allowed_start: stored.authorization.allowed_start.start,
        allowed_end: stored.authorization.allowed_start.end,
        max_concurrency: stored.authorization.max_concurrency,
        revoked_at: stored
            .revocation
            .as_ref()
            .map(|revocation| revocation.revoked_at),
    }
}

/// The stable spelling of one lifecycle action, for the durable intent.
///
/// Written out rather than derived from `Serialize`, so the recorded vocabulary
/// is this crate's decision and cannot change because a wire enum was renamed.
const fn action_name(action: LifecycleAction) -> &'static str {
    match action {
        LifecycleAction::Block => "block",
        LifecycleAction::Resume => "resume",
        LifecycleAction::CompleteTask => "complete_task",
        LifecycleAction::ReopenTask => "reopen_task",
        LifecycleAction::CloseEpic => "close_epic",
        LifecycleAction::ReopenEpic => "reopen_epic",
    }
}

/// Everything filling one more seat of an already-admitted team needs.
///
/// It exists so the per-slot call takes one context rather than seven loose
/// arguments that could be passed in the wrong order — every one of them is about
/// the same team run, and they only make sense together.
#[derive(Clone, Copy)]
struct Seating<'a> {
    /// The project the team run belongs to.
    project_id: ProjectId,
    /// The admitted task the team is working.
    admitted: &'a AdmittedCandidate,
    /// The team run every seat joins.
    team_run_id: TeamRunId,
    /// The slots the frozen handoff DAG starts at.
    roots: &'a BTreeSet<RoleSlotId>,
    /// The runtime that issues the sessions.
    adapter: &'a std::sync::Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
    /// The node-keyed container every seat of this team run is placed in.
    ///
    /// Prepared once, by `seat`, and carried rather than re-prepared. One
    /// container per node is the invariant; a seat that prepared its own would
    /// be asserting a second answer to "where is this task's work".
    container: &'a ContainerBindingSnapshot,
    /// Where every seat of this team run works.
    cwd: &'a kontor_runtime::workspace::WorkspaceRoot,
    /// The instant every seat of this start is dated at.
    now: Timestamp,
}

/// The slots the frozen handoff DAG starts at.
///
/// A root is a declared slot that is no handoff's `to_slot`: nothing hands work
/// to it, so nothing is being waited for. Everything else is downstream of some
/// root and receives its work when that handoff is satisfied.
///
/// A template whose handoffs form a cycle — or that declares no handoffs at all —
/// yields every slot as a root, which is the honest reading: with nothing to wait
/// for, there is nothing to be downstream of. `TeamTemplateSpec::validate` already
/// refuses a cyclic handoff graph, so the first case does not reach here.
fn eligible_roots(team: &kontor_teams::spec::TeamTemplateSpec) -> BTreeSet<RoleSlotId> {
    let downstream: BTreeSet<&RoleSlotId> = team
        .handoffs
        .iter()
        .map(|handoff| &handoff.to_slot)
        .collect();
    let roots: BTreeSet<RoleSlotId> = team
        .slots
        .iter()
        .map(|slot| slot.id.clone())
        .filter(|slot| !downstream.contains(slot))
        .collect();
    if roots.is_empty() {
        return team.slots.iter().map(|slot| slot.id.clone()).collect();
    }
    roots
}

/// What one seat is told to do when its session starts.
///
/// A root is given the work. A downstream seat is given an explicit instruction
/// to wait, naming the fact it is waiting on — not silence, and not the same
/// instruction as the root's. An idle seat that was told nothing looks exactly
/// like a seat that was told to start, and the agent in it will behave that way.
///
/// ponytail: the instruction is static text. Activating a downstream seat when
/// its handoff's phase and artifacts are actually satisfied is scheduler/team
/// orchestration — it needs the phase advance, the artifact ledger and a
/// follow-up message into a live session — and it is deliberately not built here.
fn slot_prompt(
    slot: &RoleSlotId,
    roots: &BTreeSet<RoleSlotId>,
) -> kontor_core::DomainResult<kontor_core::id::BoundedText> {
    if roots.contains(slot) {
        kontor_core::id::BoundedText::parse("begin the admitted task")
    } else {
        kontor_core::id::BoundedText::parse(
            "wait: this seat is downstream of a handoff. Do no work until you are \
             handed the artifacts your role requires.",
        )
    }
}

/// Freeze the context-window policy one seat launches under, before its session
/// exists.
///
/// Resolution reads the team run's *own frozen* inputs, so a later edit to a
/// profile pack, a seed table or a template cannot change what this run asked
/// for. The effective half comes from what the runtime attests right now: a
/// runtime that cannot configure a context window records `not_enforced` rather
/// than a claim of success, and a `required` policy on such a runtime is refused
/// here — before admission is spent and before any native effect.
async fn freeze_seat_context_policy(
    adapter: &std::sync::Arc<dyn kontor_runtime::adapter::RuntimeAdapter>,
    snapshot: &TeamRunSnapshot,
    slot: &RoleSlotId,
    now: Timestamp,
) -> kontor_runtime::adapter::RuntimeResult<ContextPolicySnapshot> {
    let template = kontor_teams::spec::TeamTemplateSpec::from_snapshot(snapshot)?;
    let declared = template
        .slot(slot)
        .and_then(|seat| seat.context_window.as_ref());
    // No authorized run override at this door: the scheduler starts a seat from
    // the frozen team definition, and an override is an operator act that
    // arrives through the explicit control-plane command instead.
    let resolved = snapshot.resolve_context_window(slot.as_role_key(), declared, None)?;
    let capabilities = adapter.discover_capabilities().await?;
    let requested = RequestedContextPolicy::of(&resolved, SCHEMA_VERSION);
    let effective = EffectiveContextPolicy::derive(
        &requested,
        &capabilities.limits.context_window,
        capabilities.supports(RuntimeCapability::ContextPolicy),
    )?;
    Ok(ContextPolicySnapshot::freeze(requested, effective, now)?)
}

/// Select the primary model rung from the team run's immutable template.
fn freeze_seat_model_rung(
    snapshot: &TeamRunSnapshot,
    slot: &RoleSlotId,
) -> kontor_core::DomainResult<ModelRung> {
    kontor_teams::spec::TeamTemplateSpec::from_snapshot(snapshot)?
        .slot(slot)
        .and_then(|seat| seat.model_chain.as_ref())
        .and_then(|chain| chain.rungs.first())
        .cloned()
        .ok_or_else(|| {
            kontor_core::DomainError::invalid("TeamRunSnapshot", "the role slot has no model route")
        })
}

/// The stable spelling of one context layer.
const fn layer_name(layer: kontor_context::model::ContextLayer) -> &'static str {
    use kontor_context::model::ContextLayer as L;
    match layer {
        L::GlobalProfile => "global_profile",
        L::ProjectProfile => "project_profile",
        L::Scope => "scope",
        L::TeamRoleProfile => "team_role_profile",
        L::TaskAdditions => "task_additions",
        L::RunOverride => "run_override",
    }
}

/// The context layers one task resolves through.
///
/// They are derived from the task and its pinned profile and from nothing a
/// caller supplied, which is what makes the resolution deterministic: the same
/// task resolves to the same hash until the task or its pin changes.
fn context_sources(
    realm_id: kontor_core::id::RealmId,
    task: &kontor_core::repository::Task,
    workflow: &kontor_core::repository::TaskWorkflow,
) -> Result<Vec<kontor_context::model::ContextSource>, ApiError> {
    use kontor_context::model::{ContextLayer, ContextSource};
    let refuse = |error: &kontor_core::DomainError| ApiError::from_domain(realm_id, error);
    vec![
        ContextSource {
            schema_version: SCHEMA_VERSION,
            realm_id,
            layer: ContextLayer::ProjectProfile,
            source_id: task.project_id.to_string(),
            revision: SpecVersion::FIRST,
            restricted_references: Vec::new(),
            redactions: Vec::new(),
            content: serde_json::json!({ "project_id": task.project_id.to_string() }),
        },
        ContextSource {
            schema_version: SCHEMA_VERSION,
            realm_id,
            layer: ContextLayer::TeamRoleProfile,
            source_id: workflow.snapshot.definition.id.as_str().to_owned(),
            revision: workflow.snapshot.definition.version,
            restricted_references: Vec::new(),
            redactions: Vec::new(),
            content: serde_json::json!({
                "work_profile": workflow.snapshot.definition.id.as_str(),
                "work_profile_version": workflow.snapshot.definition.version.get(),
                "current_phase": workflow.current_phase.as_str(),
            }),
        },
        ContextSource {
            schema_version: SCHEMA_VERSION,
            realm_id,
            layer: ContextLayer::TaskAdditions,
            source_id: task.id.to_string(),
            revision: SpecVersion::FIRST,
            restricted_references: Vec::new(),
            redactions: Vec::new(),
            content: serde_json::json!({
                "task_id": task.id.to_string(),
                "title": task.title.as_str(),
                "module": task.module.as_ref().map(kontor_core::id::ModuleKey::as_str),
            }),
        },
    ]
    .into_iter()
    .map(|source| {
        source
            .validate(realm_id)
            .map(|()| source)
            .map_err(|error| refuse(&error))
    })
    .collect::<Result<Vec<_>, _>>()
}

/// The wire view of one account profile.
///
/// There is no branch here that could add the credential reference: the DTO has
/// no field for it, so the alias cannot reach a response by being forgotten about
/// in one code path.
fn account_profile_dto(
    profile: &kontor_core::repository::AccountProfile,
    applied: AppliedDto,
) -> AccountProfileDto {
    AccountProfileDto {
        account_profile_id: profile.id,
        label: profile.label.clone(),
        harness: profile.harness.clone(),
        enabled: profile.enabled,
        revision: profile.revision,
        applied,
    }
}

/// The wire view of one applied-or-unchanged outcome.
/// One semantic scope, resolved to what the topology needs to realize it.
///
/// Everything here is *derived*. A caller supplies a meaning; the kind, the
/// epic and the delivery task are read out of the pinned specification and the
/// seeded delivery binding, which is what makes the model-facing boundary
/// semantic rather than a native shape wearing a nicer name.
#[derive(Debug, Clone)]
struct TopologyScope {
    /// The node kind this scope materializes as; `None` is the project root.
    kind: Option<TopologyKindKey>,
    /// The epic this scope belongs to, when it belongs to one.
    epic_id: Option<MiniProjectId>,
    /// The delivery task this scope serves, for the task-scoped kinds.
    task_id: Option<TaskId>,
    /// The scope's stable spelling in a canonical intent.
    key: String,
}

impl TopologyScope {
    /// The scope as one canonical-intent field.
    fn intent_key(&self) -> &str {
        &self.key
    }

    /// The epic this scope belongs to.
    const fn epic_id(&self) -> Option<MiniProjectId> {
        self.epic_id
    }
}

/// Which exact-seat act is being performed.
///
/// The two differ only in what they record, so the difference is a value rather
/// than a second copy of the addressing logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeatAct {
    /// Look at the seat and record what came back.
    Observe,
    /// Release the seat.
    Retire,
}

impl SeatAct {
    /// The operation name that goes into the canonical intent.
    const fn operation(self) -> &'static str {
        match self {
            Self::Observe => "seat_attention",
            Self::Retire => "seat_retire",
        }
    }

    /// The command kind the receipt is recorded under.
    const fn command_kind(self) -> CommandKind {
        match self {
            Self::Observe => CommandKind::ObserveSeat,
            Self::Retire => CommandKind::RetireSeat,
        }
    }

    /// What this act records about the seat.
    ///
    /// A successful readback fills `attached_at` and never `activity_at`. The
    /// distinction is the whole phantom-seat guard: Kontor asking and getting an
    /// answer proves the seat is there, not that it is working, and recording it
    /// as activity would make a wedged seat look busy for as long as anything
    /// kept asking.
    fn observation(self, readback: bool, now: Timestamp) -> SeatLivenessObservation {
        match self {
            Self::Observe => SeatLivenessObservation {
                attached_at: readback.then_some(now),
                ..SeatLivenessObservation::default()
            },
            Self::Retire => SeatLivenessObservation {
                released_at: Some(now),
                ..SeatLivenessObservation::default()
            },
        }
    }
}

/// The stored capacity document, which is the wire shape plus its generation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredCeilings {
    /// Wire generation, so a stored document stays parseable.
    schema_version: kontor_core::id::SchemaVersion,
    /// The ceilings themselves.
    ceilings: CapacityCeilingsDto,
}

/// The wire shape of one capacity configuration.
fn ceilings_dto(config: CapacityConfig) -> CapacityCeilingsDto {
    CapacityCeilingsDto {
        global_max_in_flight: config.global_max_in_flight,
        project_max_in_flight: config.project_max_in_flight,
        mission_max_in_flight: config.mission_max_in_flight,
        account_max_in_flight: config.account_max_in_flight,
        provider_max_in_flight: config.provider_max_in_flight,
        runtime_max_in_flight: config.runtime_max_in_flight,
        adaptive: AdaptiveWindowDto {
            initial: config.adaptive.initial,
            floor: config.adaptive.floor,
            ceiling: config.adaptive.ceiling,
            growth_step: config.adaptive.growth_step,
        },
    }
}

/// The scheduler's shape of one capacity configuration.
///
/// The pair is deliberately not a `From` in either crate: `kontor-api` may not
/// know the scheduler's types and the scheduler may not know the wire's, so the
/// translation lives here, in the one place allowed to hold both.
fn capacity_config(ceilings: &CapacityCeilingsDto) -> CapacityConfig {
    CapacityConfig {
        global_max_in_flight: ceilings.global_max_in_flight,
        project_max_in_flight: ceilings.project_max_in_flight,
        mission_max_in_flight: ceilings.mission_max_in_flight,
        account_max_in_flight: ceilings.account_max_in_flight,
        provider_max_in_flight: ceilings.provider_max_in_flight,
        runtime_max_in_flight: ceilings.runtime_max_in_flight,
        adaptive: kontor_scheduler::model::AdaptiveWindowConfig {
            initial: ceilings.adaptive.initial,
            floor: ceilings.adaptive.floor,
            ceiling: ceilings.adaptive.ceiling,
            growth_step: ceilings.adaptive.growth_step,
        },
    }
}

/// One catalog entry, as a reference projection reports it.
fn role_entry_dto(entry: &kontor_core::spec::RoleCatalogEntry) -> RoleCatalogEntryDto {
    RoleCatalogEntryDto {
        role_code: entry.role_code.clone(),
        standard_title: entry.standard_title.clone(),
        segment: entry.segment,
        responsibility_summary: entry.responsibility_summary.clone(),
        lifecycle: entry.lifecycle,
        capability_defaults: entry
            .capability_defaults
            .iter()
            .map(|skill| skill.as_str().to_owned())
            .collect(),
    }
}

/// How one durable record was classified for leaving Kontor.
fn shareability_dto(stamp: &Shareability) -> ShareabilityDto {
    ShareabilityDto {
        class: stamp.class,
        classifier: stamp.classifier.clone(),
        provenance: stamp.provenance,
    }
}

/// One pinned specification reference.
/// The revision a Core Team write must present.
///
/// One ahead of the published version, so that "nothing published yet" and
/// "version one is published" are different values. Collapsing them would let
/// an apply written against an unconfigured project land on a project that had
/// meanwhile published its first roster.
fn core_team_revision_of(current: Option<&CoreTeamRevision>) -> AggregateRevision {
    current.map_or(AggregateRevision::INITIAL, |current| {
        AggregateRevision::parse(u64::from(current.version.get()).saturating_add(1))
            .unwrap_or(AggregateRevision::INITIAL)
    })
}

/// What publishing `proposed` would change about the roster.
///
/// Reported per role slot, in slot order, so the same edit always produces the
/// same digest. Presence and ad-hoc eligibility are part of a seat's identity
/// here: changing either changes which epics staff the role and whether it can
/// open a Quick session, and neither is visible as an add or a remove.
fn core_team_effects(
    current: Option<&CoreTeamRevision>,
    proposed: &CoreTeamRevision,
) -> kontor_core::DomainResult<Vec<TopologyUpgradeEffectDto>> {
    let before: BTreeMap<&RoleSlotId, &CoreTeamSeat> = current
        .map(|current| {
            current
                .seats
                .iter()
                .map(|seat| (&seat.role_slot_id, seat))
                .collect()
        })
        .unwrap_or_default();
    let after: BTreeMap<&RoleSlotId, &CoreTeamSeat> = proposed
        .seats
        .iter()
        .map(|seat| (&seat.role_slot_id, seat))
        .collect();

    let mut effects = Vec::new();
    let mut push =
        |slot: &RoleSlotId, effect: &str, detail: String| -> kontor_core::DomainResult<()> {
            effects.push(TopologyUpgradeEffectDto {
                subject: format!("core_team_seat:{slot}"),
                topology_node_id: None,
                effect: effect.to_owned(),
                detail: BoundedText::parse(&detail)?,
            });
            Ok(())
        };
    for (slot, seat) in &after {
        match before.get(slot) {
            None => push(
                slot,
                "seat_added",
                format!(
                    "{} joins the Core Team as {} ({})",
                    seat.role.role_code,
                    seat.presence,
                    if seat.ad_hoc_allowed {
                        "quick-eligible"
                    } else {
                        "not quick-eligible"
                    }
                ),
            )?,
            Some(existing)
                if existing.presence != seat.presence
                    || existing.ad_hoc_allowed != seat.ad_hoc_allowed =>
            {
                push(
                    slot,
                    "seat_policy_changed",
                    format!(
                        "{} moves from {} to {}",
                        seat.role.role_code, existing.presence, seat.presence
                    ),
                )?;
            }
            Some(_) => {}
        }
    }
    for (slot, seat) in &before {
        if !after.contains_key(slot) {
            push(
                slot,
                "seat_removed",
                format!("{} leaves the Core Team", seat.role.role_code),
            )?;
        }
    }
    Ok(effects)
}

/// The stable part of an effect list, for a preview digest.
fn effect_digest(effects: &[TopologyUpgradeEffectDto]) -> Vec<serde_json::Value> {
    effects
        .iter()
        .map(|effect| {
            serde_json::json!({
                "subject": effect.subject,
                "effect": effect.effect,
                "detail": effect.detail,
            })
        })
        .collect()
}

/// What promoting one Quick session would do.
///
/// Node and seat identities are deliberately absent. They are minted by the
/// apply, inside the transaction that records them, so that a preview commits
/// nothing and two previews of the same unchanged source agree.
fn promotion_effects(
    session: &StoredQuickSession,
    roster: &FrozenRoster,
) -> kontor_core::DomainResult<Vec<TopologyUpgradeEffectDto>> {
    let mut effects = vec![
        TopologyUpgradeEffectDto {
            subject: "mini_project".to_owned(),
            topology_node_id: None,
            effect: "epic_created".to_owned(),
            detail: BoundedText::parse(&format!(
                "a tracker-neutral epic is created for {}",
                session.purpose.as_str()
            ))?,
        },
        TopologyUpgradeEffectDto {
            subject: "topology_node:esw".to_owned(),
            topology_node_id: None,
            effect: "node_created".to_owned(),
            detail: BoundedText::parse("the epic gets its own session workspace")?,
        },
        TopologyUpgradeEffectDto {
            subject: "topology_node:ecp".to_owned(),
            topology_node_id: None,
            effect: "node_created".to_owned(),
            detail: BoundedText::parse("exactly one control plane is created inside it")?,
        },
    ];
    for seat in roster
        .revision
        .seats
        .iter()
        .filter(|seat| seat.presence != EpicPresence::OnDemand)
    {
        effects.push(TopologyUpgradeEffectDto {
            subject: format!("seat:{}", seat.role_slot_id),
            topology_node_id: None,
            effect: "seat_created".to_owned(),
            detail: BoundedText::parse(&format!(
                "{} is seated in the epic control plane",
                seat.role.role_code
            ))?,
        });
    }
    effects.push(TopologyUpgradeEffectDto {
        subject: "handoff".to_owned(),
        topology_node_id: None,
        effect: "handoff_delivered".to_owned(),
        detail: BoundedText::parse("the source's work is handed to the epic's LSA seat")?,
    });
    // Stated as an effect because it is one a caller must be able to see: the
    // source survives, and promotion is not a move.
    effects.push(TopologyUpgradeEffectDto {
        subject: "source".to_owned(),
        topology_node_id: Some(session.topology_node_id),
        effect: "source_left_idle".to_owned(),
        detail: BoundedText::parse("the Quick session remains as durable provenance")?,
    });
    Ok(effects)
}

/// What moving one epic's roster pin would do.
fn roster_upgrade_effects(
    current: &FrozenRoster,
    target: &CoreTeamRevision,
) -> kontor_core::DomainResult<Vec<TopologyUpgradeEffectDto>> {
    let held: BTreeSet<&RoleSlotId> = current
        .revision
        .seats
        .iter()
        .map(|seat| &seat.role_slot_id)
        .collect();
    let mut effects = Vec::new();
    for seat in target
        .seats
        .iter()
        .filter(|seat| seat.presence != EpicPresence::OnDemand)
    {
        if !held.contains(&seat.role_slot_id) {
            effects.push(TopologyUpgradeEffectDto {
                subject: format!("seat:{}", seat.role_slot_id),
                topology_node_id: None,
                effect: "seat_created".to_owned(),
                detail: BoundedText::parse(&format!(
                    "{} joins this epic's control plane",
                    seat.role.role_code
                ))?,
            });
        }
    }
    // Reported, never performed. A role the project dropped keeps its seat in
    // an epic already running under it, because closing that seat would end a
    // session someone is working in.
    for seat in &current.revision.seats {
        if !target
            .seats
            .iter()
            .any(|candidate| candidate.role_slot_id == seat.role_slot_id)
        {
            effects.push(TopologyUpgradeEffectDto {
                subject: format!("seat:{}", seat.role_slot_id),
                topology_node_id: None,
                effect: "seat_left_in_place".to_owned(),
                detail: BoundedText::parse(&format!(
                    "{} is no longer on the project roster and keeps its existing seat",
                    seat.role.role_code
                ))?,
            });
        }
    }
    Ok(effects)
}

fn pinned_spec_dto(snapshot: &TopologySnapshot) -> PinnedSpecDto {
    PinnedSpecDto {
        id: snapshot.spec_id,
        version: snapshot.version,
        canonical_hash: snapshot.canonical_hash.clone(),
    }
}

const fn applied_dto(applied: Applied) -> AppliedDto {
    match applied {
        Applied::Created => AppliedDto::Created,
        Applied::Unchanged => AppliedDto::Unchanged,
    }
}

fn teams_projection_dto(
    realm_id: kontor_core::id::RealmId,
    stored: StoredTeamsProjection,
) -> Result<TeamsProjectionDto, serde_json::Error> {
    let drafts = stored
        .drafts
        .into_iter()
        .map(|draft| {
            let slots: Vec<TeamDraftSlotDto> = serde_json::from_str(&draft.slots_json)?;
            let resolved_policy = resolved_policy(&slots);
            Ok(TeamDraftDto {
                id: draft.id,
                name: draft.name,
                slots,
                resolved_policy,
            })
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;
    let revisions = stored
        .revisions
        .into_iter()
        .map(|revision| {
            let slots: Vec<TeamDraftSlotDto> = serde_json::from_str(&revision.slots_json)?;
            let resolved_policy = resolved_policy(&slots);
            Ok(PublishedTeamRevisionDto {
                id: revision.id,
                version: revision.version,
                name: revision.name,
                slots,
                resolved_policy,
            })
        })
        .collect::<Result<Vec<_>, serde_json::Error>>()?;
    Ok(TeamsProjectionDto {
        realm_id,
        snapshot_cursor: stored.cursor,
        drafts,
        revisions,
    })
}

fn resolved_policy(slots: &[TeamDraftSlotDto]) -> Vec<serde_json::Value> {
    slots
        .iter()
        .map(|slot| {
            let capabilities = &slot.capabilities;
            let class = capabilities["context"]["class"]
                .as_str()
                .unwrap_or("native");
            let enforcement = capabilities["context"]["enforcement"]
                .as_str()
                .unwrap_or("best_effort");
            let provider = capabilities["chain"][0]["provider"].as_str().unwrap_or("");
            let target = match class {
                "lean" => Some(128_000),
                "standard" => Some(256_000),
                "deep" => Some(512_000),
                "extended" => Some(720_000),
                _ => None,
            };
            let window = (provider == "claude").then_some(1_000_000);
            let effective = match (target, window) {
                (Some(target), Some(window)) => Some(target.min(window)),
                (None, window) => window,
                _ => None,
            };
            let capability = match (target, window) {
                (Some(_), None) => "unsupported",
                (Some(target), Some(window)) if target > window => "clamped",
                _ => "supported",
            };
            let need = capabilities["need"]["minTokens"].as_i64().unwrap_or(0);
            let task_minimum = capabilities["taskMinimum"]["minTokens"].as_i64();
            let source = if task_minimum.is_some_and(|minimum| minimum > need) {
                "run_override"
            } else {
                "role_slot"
            };
            serde_json::json!({
                "slot": slot.id,
                "class": class,
                "source": source,
                "effective_threshold": effective,
                "enforcement": enforcement,
                "capability": capability,
                "latest_receipt": capabilities["latestReceipt"]
            })
        })
        .collect()
}

/// The reserved id of the Completion Profile every build ships.
///
/// Reserved rather than seeded: a per-project row would be one copy per project
/// that a later build could not correct, and a project created before the seed
/// ran would carry a different catalog from one created after. Publishing under
/// this id is refused for the same reason — two definitions answering to one
/// pinned name is exactly what an epic's pin exists to prevent.
const BUILTIN_COMPLETION_PROFILE: &str = "operational_default";

/// Epic Completion Profiles, completion runs and their bounded effects.
impl Services {
    /// The built-in profile, compiled.
    fn builtin_completion(&self) -> Result<CompiledCompletion, ApiError> {
        let profile =
            kontor_scheduler::operational_default().map_err(|error| self.refuse_domain(&error))?;
        kontor_scheduler::compile(profile).map_err(|error| self.refuse_domain(&error))
    }

    /// Decode one caller definition strictly, then compile it.
    ///
    /// Unknown fields are refused here, before the hash is taken, so a caller
    /// cannot get an unmodelled key counted into the preview hash that its apply
    /// will later be compared against.
    fn compile_completion_definition(
        &self,
        definition: &serde_json::Value,
    ) -> Result<CompiledCompletion, ApiError> {
        let profile: CompletionProfile =
            serde_json::from_value(definition.clone()).map_err(|_| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "the definition is not a Completion Profile this realm can read",
                )
            })?;
        if profile.id.as_str() == BUILTIN_COMPLETION_PROFILE {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "`operational_default` is the built-in profile and cannot be republished",
            ));
        }
        kontor_scheduler::compile(profile).map_err(|error| self.refuse_domain(&error))
    }

    /// One published revision as the shared profile projection.
    fn completion_profile_dto(compiled: &CompiledCompletion) -> ProfileRevisionDto {
        ProfileRevisionDto {
            id: compiled.profile.id.as_str().to_owned(),
            version: compiled.profile.version,
            name: compiled.profile.name.clone(),
            definition_hash: compiled.definition_hash.clone(),
        }
    }

    /// Every profile this project may pin: the built-in, then its published ones.
    fn completion_catalog(
        &self,
        project_id: ProjectId,
    ) -> Result<(Vec<ProfileRevisionDto>, AggregateRevision), ApiError> {
        let stored = self
            .state()?
            .with_store(|store| store.list_completion_profiles(project_id))
            .map_err(|error| self.refuse(&error))?;
        let mut revisions = vec![Self::completion_profile_dto(&self.builtin_completion()?)];
        for row in &stored {
            revisions.push(ProfileRevisionDto {
                id: row.id.as_str().to_owned(),
                version: row.version,
                name: row.name.clone(),
                definition_hash: row.definition_hash.clone(),
            });
        }
        // The catalog is append-only, so its revision is how many publications
        // have happened. An empty catalog stands at `INITIAL`, which is what an
        // apply against a project that has published nothing must present.
        let revision =
            AggregateRevision::parse(1 + u64::try_from(stored.len()).unwrap_or(u64::MAX))
                .map_err(|error| self.refuse_domain(&error))?;
        Ok((revisions, revision))
    }

    /// The compiled profile one epic's run is pinned to.
    ///
    /// A stored state whose pin does not recompile to the digest it froze is
    /// refused rather than advanced: the compiled graph is what every transition
    /// is judged against, and a graph that is not the pinned one would judge the
    /// run against a contract it never agreed to.
    fn pinned_completion(
        &self,
        stored: &StoredEpicCompletion,
    ) -> Result<CompiledCompletion, ApiError> {
        let compiled = if stored.profile_id.as_str() == BUILTIN_COMPLETION_PROFILE {
            self.builtin_completion()?
        } else {
            let row = self
                .state()?
                .with_store(|store| store.list_completion_profiles(stored.project_id))
                .map_err(|error| self.refuse(&error))?
                .into_iter()
                .find(|row| row.id == stored.profile_id && row.version == stored.profile_version)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the revision this epic's completion pinned is no longer readable",
                    )
                })?;
            self.compile_completion_definition(&row.definition)?
        };
        if compiled.definition_hash != stored.definition_hash {
            return Err(self.deny(
                ApiErrorCode::Unavailable,
                "the pinned Completion Profile no longer compiles to the digest it froze",
            ));
        }
        Ok(compiled)
    }

    /// Decode one stored completion state.
    fn completion_state(&self, stored: &StoredEpicCompletion) -> Result<CompletionState, ApiError> {
        serde_json::from_value(stored.state.clone()).map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "a stored completion state cannot be read by this build",
            )
        })
    }

    /// The exact seat in this epic's control plane holding one standard role.
    ///
    /// Read-only: `scope_nodes` finds the ECP that promotion already placed and
    /// creates nothing. Completion wakes and reuses these seats; a completion
    /// that could create one would be able to invent the authority it is
    /// supposed to be checking.
    fn epic_control_seat(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        role_code: &str,
    ) -> Result<SeatBindingId, ApiError> {
        let scope = self.resolve_scope(
            project_id,
            &SemanticTopologyTargetDto::EpicControl { epic_id },
        )?;
        let nodes = self.scope_nodes(project_id, &scope)?;
        // Matched on the scope's kind, never "the first node this epic has".
        // `scope_nodes` filters by epic only, and an epic owns at least its own
        // ESW as well as its ECP — so taking the first would address the delivery
        // workspace and then truthfully report that it holds none of the
        // control-plane seats, which live on the ECP.
        let control = scope
            .kind
            .as_ref()
            .and_then(|kind| nodes.iter().find(|node| &node.kind == kind))
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "this epic has no control plane; promote or materialize it first",
                )
            })?;
        let seats = self
            .state()?
            .with_store(|store| store.list_seat_bindings(project_id, control.id))
            .map_err(|error| self.refuse(&error))?;
        seats
            .into_iter()
            .find(|seat| {
                seat.role.role_code.as_str() == role_code
                    && seat.lifecycle != TopologyLifecycle::Retired
            })
            .map(|seat| seat.id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::RoleSlotUnbound,
                    "this epic's control plane holds no live seat for the required role",
                )
            })
    }

    /// The ticket contract one epic's tasks declare.
    ///
    /// The contract is read from each task's pinned work profile: its declared
    /// gates are the goals and its declared artifact contracts are the evidence
    /// keys. A task lifecycle value is deliberately not consulted — `done` is a
    /// state a task can reach, not evidence that the things it promised exist.
    fn epic_ticket_requirements(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<Vec<TicketRequirement>, ApiError> {
        let state = self.state()?;
        let tasks = state
            .with_store(|store| store.list_tasks(project_id))
            .map_err(|error| self.refuse(&error))?;
        let mut requirements = Vec::new();
        for task in tasks
            .into_iter()
            .filter(|task| task.mini_project_id == Some(epic_id))
        {
            let inspection = state
                .with_store(|store| store.snapshot_task_inspection(project_id, task.id))
                .map_err(|error| self.refuse(&error))?
                .value;
            let Some(inspection) = inspection else {
                continue;
            };
            let mut goals = BTreeSet::new();
            let mut evidence = BTreeSet::new();
            if let Some(workflow) = &inspection.workflow {
                for gate in &workflow.snapshot.definition.gates {
                    goals.insert(
                        ExternalName::parse(gate.id.as_str())
                            .map_err(|error| self.refuse_domain(&error))?,
                    );
                }
                for artifact in &workflow.snapshot.definition.artifacts {
                    evidence.insert(
                        ExternalName::parse(artifact.key.as_str())
                            .map_err(|error| self.refuse_domain(&error))?,
                    );
                }
            }
            requirements.push(TicketRequirement {
                task_id: task.id,
                goals,
                evidence,
            });
        }
        Ok(requirements)
    }

    /// What each declared ticket currently has durable evidence for.
    fn epic_ticket_evidence(
        &self,
        project_id: ProjectId,
        requirements: &[TicketRequirement],
    ) -> Result<Vec<TicketEvidence>, ApiError> {
        let state = self.state()?;
        let mut recorded = Vec::new();
        for requirement in requirements {
            let inspection = state
                .with_store(|store| store.snapshot_task_inspection(project_id, requirement.task_id))
                .map_err(|error| self.refuse(&error))?
                .value;
            let Some(inspection) = inspection else {
                continue;
            };
            let mut goals = BTreeSet::new();
            for (gate, gate_state) in &inspection.gates {
                if gate_state.satisfies_requirement() {
                    goals.insert(
                        ExternalName::parse(gate.as_str())
                            .map_err(|error| self.refuse_domain(&error))?,
                    );
                }
            }
            let evidence = state
                .with_store(|store| store.list_task_artifact_keys(project_id, requirement.task_id))
                .map_err(|error| self.refuse(&error))?;
            recorded.push(TicketEvidence {
                task_id: requirement.task_id,
                goals,
                evidence,
            });
        }
        Ok(recorded)
    }

    /// Project one epic's durable completion into its read model.
    fn completion_dto(
        &self,
        stored: &StoredEpicCompletion,
        compiled: &CompiledCompletion,
    ) -> Result<CompletionStateDto, ApiError> {
        let state = self.completion_state(stored)?;
        let wakes = self
            .state()?
            .with_store(|store| {
                store.list_completion_wakes(stored.project_id, stored.mini_project_id)
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(CompletionStateDto {
            realm_id: self.realm_id,
            epic_id: stored.mini_project_id,
            profile: Self::completion_profile_dto(compiled),
            phase: completion_phase_dto(state.phase),
            blockers: kontor_scheduler::blockers(&state)
                .map_err(|error| self.refuse_domain(&error))?
                .iter()
                .map(completion_blocker_dto)
                .collect(),
            integrations: state.integrations.iter().map(integration_dto).collect(),
            rounds: state.rounds.iter().map(completion_round_dto).collect(),
            closeout: closeout_dto(&state.closeout),
            wakes: wakes
                .iter()
                .map(|wake| CompletionWakeDto {
                    completion_revision: wake.completion_revision,
                    reason: wake.reason.clone(),
                    seat_binding_id: wake.seat_binding_id,
                    receipt: wake.receipt.clone(),
                    acknowledged: wake.acknowledged_at.is_some(),
                })
                .collect(),
            needs_human: state.needs_human.as_ref().map(needs_human_dto),
            revision: state.revision,
            snapshot_cursor: self.cursor()?,
        })
    }

    /// Read one epic's completion, refusing when it has not started.
    fn require_completion(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<StoredEpicCompletion, ApiError> {
        self.state()?
            .with_store(|store| store.get_epic_completion(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "this epic has no completion run yet",
                )
            })
    }

    /// Start one epic's completion run against the built-in profile.
    ///
    /// The built-in is pinned because nothing else has been selected: an epic
    /// pins its profile at the moment completion starts, and there is no route
    /// that lets a caller choose one for an epic. When such a route exists the
    /// pin moves here and nowhere else — every later transition already reads
    /// the pin off the row rather than re-deciding it.
    fn start_completion(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        now: Timestamp,
    ) -> Result<(StoredEpicCompletion, CompiledCompletion), ApiError> {
        let compiled = self.builtin_completion()?;
        let tpm = self.epic_control_seat(project_id, epic_id, MANDATORY_PROGRAM_ROLE)?;
        let requirements = self.epic_ticket_requirements(project_id, epic_id)?;
        if requirements.is_empty() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this epic declares no tickets, so it has no completion contract to judge",
            ));
        }
        let state = kontor_scheduler::start(&compiled, tpm, requirements)
            .map_err(|error| self.refuse_domain(&error))?;
        let stored = StoredEpicCompletion {
            project_id,
            mini_project_id: epic_id,
            profile_id: compiled.profile.id.clone(),
            profile_version: compiled.profile.version,
            definition_hash: compiled.definition_hash.clone(),
            state: serde_json::to_value(&state).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the completion state does not serialize",
                )
            })?,
            revision: state.revision,
            updated_at: now,
        };
        self.state()?
            .with_store(|store| store.create_epic_completion(&stored))
            .map_err(|error| self.refuse(&error))?;
        Ok((stored, compiled))
    }

    /// Derive the one observation this run's phase is waiting for.
    ///
    /// Only observations whose authoritative source is composed in this build are
    /// derived. The final Committee verdict is not: OP-05 owns it, its service is
    /// still uncomposed, so no CommitteeRun exists to settle a round. Returning
    /// `Unavailable` there is the honest answer — a synthesized pass would close
    /// an epic on a verdict nobody reached.
    fn observe_completion(
        &self,
        stored: &StoredEpicCompletion,
        state: &CompletionState,
    ) -> Result<CompletionObservation, ApiError> {
        match state.phase {
            CompletionPhase::Tickets => {
                let evidence =
                    self.epic_ticket_evidence(stored.project_id, &state.ticket_requirements)?;
                Ok(CompletionObservation::TicketsClosed(evidence))
            }
            CompletionPhase::Integration | CompletionPhase::Remediating(_) => Err(self.deny(
                ApiErrorCode::Unavailable,
                "integration TeamRun outcomes are not yet observable in this build",
            )),
            CompletionPhase::Verdict(_) => Err(self.deny(
                ApiErrorCode::Unavailable,
                "the Committee service is not composed in this build, so no round can settle",
            )),
            CompletionPhase::AwaitRemediation(_) => Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this round is waiting for its LSA proposal and TPM route, not for an advance",
            )),
            CompletionPhase::Closeout => Err(self.deny(
                ApiErrorCode::Unavailable,
                "closeout receipts are recorded by their connectors, which are not composed \
                 in this build",
            )),
            CompletionPhase::Done | CompletionPhase::NeedsHuman => Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this completion has reached a terminal state",
            )),
        }
    }

    /// Commit one transition and append the wake intents its commands ask for.
    ///
    /// The state is stored first and the wake intents with it, in that order, so
    /// a crash between them cannot leave an effect that no durable record asked
    /// for. Every intent is keyed by the revision it reports, so replaying the
    /// same observation re-appends nothing.
    fn commit_completion(
        &self,
        stored: &StoredEpicCompletion,
        transition: &CompletionTransition,
        reason: &str,
        now: Timestamp,
    ) -> Result<StoredEpicCompletion, ApiError> {
        let next = StoredEpicCompletion {
            state: serde_json::to_value(&transition.state).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the completion state does not serialize",
                )
            })?,
            revision: transition.state.revision,
            updated_at: now,
            ..stored.clone()
        };
        if !transition.replayed {
            self.state()?
                .with_store(|store| store.update_epic_completion(&next, stored.revision))
                .map_err(|error| self.refuse(&error))?;
        }
        for command in &transition.commands {
            if let CompletionCommand::WakeTpm { seat_binding_id } = command {
                // The seat is re-resolved rather than trusted from the state: a
                // seat that was replaced or retired since the run started must
                // refuse here, which is the reconciliation the wake owes.
                let live = self.epic_control_seat(
                    next.project_id,
                    next.mini_project_id,
                    MANDATORY_PROGRAM_ROLE,
                )?;
                if live != *seat_binding_id {
                    return Err(self.deny(
                        ApiErrorCode::StaleBinding,
                        "this epic's TPM seat was replaced since completion started",
                    ));
                }
                let wake = StoredCompletionWake {
                    project_id: next.project_id,
                    mini_project_id: next.mini_project_id,
                    completion_revision: next.revision,
                    reason: ExternalName::parse(reason)
                        .map_err(|error| self.refuse_domain(&error))?,
                    seat_binding_id: *seat_binding_id,
                    receipt: next.definition_hash.clone(),
                    appended_at: now,
                    acknowledged_at: None,
                };
                self.state()?
                    .with_store(|store| store.append_completion_wake(&wake))
                    .map_err(|error| self.refuse(&error))?;
            }
        }
        Ok(next)
    }
}

/// One phase as its wire projection.
fn completion_phase_dto(phase: CompletionPhase) -> CompletionPhaseDto {
    match phase {
        CompletionPhase::Tickets => CompletionPhaseDto::TicketGate,
        CompletionPhase::Integration => CompletionPhaseDto::Integration,
        CompletionPhase::Verdict(round) => CompletionPhaseDto::Verdict { round },
        CompletionPhase::AwaitRemediation(round) => CompletionPhaseDto::AwaitingLsa { round },
        CompletionPhase::Remediating(round) => CompletionPhaseDto::Remediation { round },
        CompletionPhase::Closeout => CompletionPhaseDto::Closeout,
        CompletionPhase::Done => CompletionPhaseDto::Done,
        CompletionPhase::NeedsHuman => CompletionPhaseDto::NeedsHuman,
    }
}

/// One closeout prerequisite as its wire projection.
const fn closeout_requirement_dto(requirement: CloseoutRequirement) -> CloseoutRequirementDto {
    match requirement {
        CloseoutRequirement::Merge => CloseoutRequirementDto::Merge,
        CloseoutRequirement::Release => CloseoutRequirementDto::Release,
        CloseoutRequirement::VersionInventory => CloseoutRequirementDto::VersionInventory,
        CloseoutRequirement::Summary => CloseoutRequirementDto::Summary,
        CloseoutRequirement::Notification => CloseoutRequirementDto::Notification,
        CloseoutRequirement::Archive => CloseoutRequirementDto::Archive,
    }
}

/// One typed blocker as its wire projection.
fn completion_blocker_dto(blocker: &CompletionBlocker) -> CompletionBlockerDto {
    match blocker {
        CompletionBlocker::Ticket(TicketGateBlocker::MissingTicket(task_id)) => {
            CompletionBlockerDto::MissingTicket { task_id: *task_id }
        }
        CompletionBlocker::Ticket(TicketGateBlocker::MissingGoal { task_id, goal }) => {
            CompletionBlockerDto::MissingTicketGoal {
                task_id: *task_id,
                goal: goal.clone(),
            }
        }
        CompletionBlocker::Ticket(TicketGateBlocker::MissingEvidence { task_id, evidence }) => {
            CompletionBlockerDto::MissingTicketEvidence {
                task_id: *task_id,
                evidence: evidence.clone(),
            }
        }
        CompletionBlocker::IntegrationTeamRun => CompletionBlockerDto::IntegrationTeamRun,
        CompletionBlocker::CommitteeVerdict { round } => {
            CompletionBlockerDto::CommitteeVerdict { round: *round }
        }
        CompletionBlocker::RemediationAuthorization { round } => {
            CompletionBlockerDto::RemediationAuthorization { round: *round }
        }
        CompletionBlocker::RemediationResult { round } => {
            CompletionBlockerDto::RemediationResult { round: *round }
        }
        CompletionBlocker::Closeout(requirement) => CompletionBlockerDto::Closeout {
            requirement: closeout_requirement_dto(*requirement),
        },
    }
}

/// One deliberation step as its wire projection.
fn deliberation_dto(step: &DeliberationStep) -> DeliberationStepDto {
    DeliberationStepDto {
        role: step.role.clone(),
        consultation: step.consultation.clone(),
        round: step.round,
        outcome: step.outcome.clone(),
    }
}

/// One Committee round as its wire projection.
fn completion_round_dto(round: &kontor_scheduler::CompletionRound) -> CompletionRoundDto {
    CompletionRoundDto {
        round: round.round,
        verdict: match round.verdict {
            CommitteeVerdict::Pass => CommitteeVerdictDto::Pass,
            CommitteeVerdict::Fail => CommitteeVerdictDto::Fail,
        },
        evidence: round.evidence.clone(),
        deliberation: round.deliberation.iter().map(deliberation_dto).collect(),
    }
}

/// One integration result as its wire projection.
fn integration_dto(record: &kontor_scheduler::IntegrationRecord) -> IntegrationRecordDto {
    IntegrationRecordDto {
        receipt: record.receipt.clone(),
        repositories: record
            .repositories
            .iter()
            .map(|outcome| RepositoryOutcomeDto {
                repository: outcome.repository.clone(),
                pull_request: outcome.pull_request.clone(),
                module_revision: outcome.module_revision.clone(),
                root_pointer_revision: outcome.root_pointer_revision.clone(),
            })
            .collect(),
    }
}

/// Accumulated closeout receipts as their wire projection.
fn closeout_dto(evidence: &CloseoutEvidence) -> CloseoutEvidenceDto {
    CloseoutEvidenceDto {
        merge_receipt: evidence.merge_receipt.clone(),
        release_receipt: evidence.release_receipt.clone(),
        delivered_versions: evidence
            .delivered_versions
            .iter()
            .map(|(module, revision)| (module.as_str().to_owned(), revision.as_str().to_owned()))
            .collect(),
        summary_receipt: evidence.summary_receipt.clone(),
        notification_receipt: evidence.notification_receipt.clone(),
        archive_receipt: evidence.archive_receipt.clone(),
    }
}

/// The mandatory human-attention payload as its wire projection.
fn needs_human_dto(payload: &NeedsHumanPayload) -> NeedsHumanDto {
    NeedsHumanDto {
        recommended_resolution: payload.recommended_resolution().clone(),
        tried_deliberation_path: payload
            .tried_deliberation_path()
            .iter()
            .map(deliberation_dto)
            .collect(),
    }
}

#[async_trait]
impl ApplicationOperations for Services {
    async fn ensure_project(
        &self,
        key: &IdempotencyKey,
        request: &EnsureProjectRequest,
    ) -> Result<ProjectDto, ApiError> {
        let state = self.state()?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "projects_ensure",
            "name": request.name.as_str(),
            "root_path": request.root_path.as_str(),
        }))?;
        // The key is judged before the project is touched. It is the one mutation
        // whose target may not exist yet, so the lookup is by key alone and the
        // target is learned from the receipt rather than presented to it.
        if let Some(receipt) = self.replayed(key, &intent, None)? {
            let AggregateRef::Project { project_id } = receipt.target else {
                return Err(self.deny(
                    ApiErrorCode::IdempotencyConflict,
                    "the idempotency key was already used for a different operation",
                ));
            };
            let project = state
                .with_store(|store| store.get_project(project_id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "the replayed receipt names a project this realm no longer has",
                    )
                })?;
            return Ok(ProjectDto {
                realm_id: state.realm_id(),
                project_id: project.id,
                name: project.name,
                root_path: project.root_path,
                revision: project.revision,
                applied: AppliedDto::Unchanged,
                created_at: project.created_at,
            });
        }

        let (project, applied) = state
            .with_store(|store| {
                store.ensure_project(&ProjectEnsure {
                    id: ProjectId::generate(),
                    name: request.name.clone(),
                    root_path: request.root_path.clone(),
                    created_at: kontor_api::now(),
                })
            })
            .map_err(|error| self.refuse(&error))?;
        // Recorded after the ensure because the receipt's own foreign key needs
        // the project to exist. The key was already proved unused above, so this
        // cannot be the write that discovers a conflict.
        self.record(
            key,
            project.id,
            CommandKind::EnsureProject,
            AggregateRef::Project {
                project_id: project.id,
            },
            project.revision,
            &intent,
        )?;
        Ok(ProjectDto {
            realm_id: state.realm_id(),
            project_id: project.id,
            name: project.name,
            root_path: project.root_path,
            revision: project.revision,
            applied: applied_dto(applied),
            created_at: project.created_at,
        })
    }

    fn work_profiles(&self) -> Result<Vec<WorkProfileCatalogDto>, ApiError> {
        let now = kontor_api::now();
        let mut catalog = Vec::new();
        for pack in &self.packs()? {
            for entry in &pack.manifest {
                if entry.availability != PackAvailability::Seeded {
                    continue;
                }
                let bundle = resolve_profile(pack, &entry.category, now)
                    .map_err(|error| self.refuse_domain(&error))?;
                catalog.push(WorkProfileCatalogDto {
                    category: entry.category.as_str().to_owned(),
                    label: entry.label.clone(),
                    profile: RevisionRefDto {
                        id: bundle.profile.definition.id.as_str().to_owned(),
                        version: bundle.profile.definition.version,
                    },
                    team: bundle.team.as_ref().map(|team| RevisionRefDto {
                        id: team.template_id.to_string(),
                        version: team.version,
                    }),
                    bundle_hash: bundle.bundle_hash.as_str().to_owned(),
                });
            }
        }
        Ok(catalog)
    }

    fn team_templates(&self) -> Result<Vec<TeamTemplateCatalogDto>, ApiError> {
        let mut catalog = Vec::new();
        for pack in &self.packs()? {
            for team in &pack.teams {
                let revision = team
                    .to_revision()
                    .map_err(|error| self.refuse_domain(&error))?;
                catalog.push(TeamTemplateCatalogDto {
                    template: RevisionRefDto {
                        id: team.template_id.to_string(),
                        version: team.version,
                    },
                    name: team.name.clone(),
                    slots: team
                        .slots
                        .iter()
                        .map(|slot| slot.id.as_role_key().as_str().to_owned())
                        .collect(),
                    definition_hash: revision.definition.hash().as_str().to_owned(),
                });
            }
        }
        Ok(catalog)
    }

    fn model_catalog(&self) -> Result<ModelCatalogDto, ApiError> {
        let state = self.state()?;
        let provenance = serde_json::json!({
            "state": "live",
            "reviewRef": "KON-MVP-25-GATE-2026-08-14-02",
            "citation": "kontord /v1/catalog realm discovery projection",
            "observedAt": "2026-08-14"
        });
        let unverified = serde_json::json!({
            "state": "fixture/needs-verification",
            "reviewRef": null,
            "citation": null,
            "observedAt": null
        });
        let providers = vec![
            serde_json::json!({
                "id": "codex", "label": "Codex",
                "basis": { "value": "plan_allowance", "provenance": unverified },
                "reachedVia": null, "pooledUsage": false
            }),
            serde_json::json!({
                "id": "claude", "label": "Claude",
                "basis": { "value": "plan_allowance", "provenance": unverified },
                "reachedVia": null, "pooledUsage": true
            }),
        ];
        let models = vec![
            serde_json::json!({
                "id": "gpt-5.6-sol", "label": "GPT-5.6 Sol", "provider": "codex",
                "isDefault": true,
                "contextWindow": { "value": null, "provenance": provenance },
                "efforts": { "value": ["low", "medium", "high", "xhigh", "max", "ultra"], "provenance": provenance },
                "pricing": [], "degradedLane": false
            }),
            serde_json::json!({
                "id": "gpt-5.6-terra", "label": "GPT-5.6 Terra", "provider": "codex",
                "isDefault": false,
                "contextWindow": { "value": null, "provenance": provenance },
                "efforts": { "value": ["low", "medium", "high", "xhigh", "max", "ultra"], "provenance": provenance },
                "pricing": [], "degradedLane": false
            }),
            serde_json::json!({
                "id": "claude-opus-5", "label": "Claude Opus 5", "provider": "claude",
                "isDefault": true,
                "contextWindow": { "value": 1000000, "provenance": provenance },
                "efforts": { "value": ["off", "low", "medium", "high", "xhigh", "max", "ultracode"], "provenance": provenance },
                "pricing": [], "degradedLane": false
            }),
            serde_json::json!({
                "id": "claude-fable-5", "label": "Claude Fable 5", "provider": "claude",
                "isDefault": false,
                "contextWindow": { "value": 1000000, "provenance": provenance },
                "efforts": { "value": ["low", "medium", "high", "xhigh", "max", "ultracode"], "provenance": provenance },
                "pricing": [], "degradedLane": false
            }),
        ];
        let cursor = state
            .with_store(SqliteStore::teams_projection)
            .map_err(|error| self.refuse(&error))?
            .cursor;
        Ok(ModelCatalogDto {
            realm_id: state.realm_id(),
            snapshot_cursor: cursor,
            providers,
            models,
        })
    }

    fn teams(&self) -> Result<TeamsProjectionDto, ApiError> {
        let state = self.state()?;
        let stored = state
            .with_store(SqliteStore::teams_projection)
            .map_err(|error| self.refuse(&error))?;
        teams_projection_dto(state.realm_id(), stored).map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the stored Teams projection is invalid",
            )
        })
    }

    async fn save_team_draft(
        &self,
        key: &IdempotencyKey,
        request: &TeamDraftRequest,
    ) -> Result<TeamsProjectionDto, ApiError> {
        let state = self.state()?;
        if request.id.trim().is_empty() || request.name.trim().is_empty() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "a team draft needs a non-empty id and name",
            ));
        }
        let fingerprint = serde_json::to_string(&("save", request)).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "the team draft could not be encoded",
            )
        })?;
        let slots_json = serde_json::to_string(&request.slots).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "the team slots could not be encoded",
            )
        })?;
        let stored = state
            .with_store(|store| {
                store.save_team_draft(
                    key.as_str(),
                    &fingerprint,
                    &StoredTeamDraft {
                        id: request.id.clone(),
                        name: request.name.clone(),
                        slots_json,
                    },
                )
            })
            .map_err(|error| self.reused_team_key(&error))?;
        teams_projection_dto(state.realm_id(), stored).map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the stored Teams projection is invalid",
            )
        })
    }

    async fn publish_team(
        &self,
        key: &IdempotencyKey,
        team_id: &str,
    ) -> Result<TeamsProjectionDto, ApiError> {
        let state = self.state()?;
        let fingerprint = format!("publish:{team_id}");
        let stored = state
            .with_store(|store| store.publish_team(key.as_str(), &fingerprint, team_id))
            .map_err(|error| self.reused_team_key(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "no Teams draft has that id"))?;
        teams_projection_dto(state.realm_id(), stored).map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the stored Teams projection is invalid",
            )
        })
    }

    fn account_profiles(&self, project_id: ProjectId) -> Result<Vec<AccountProfileDto>, ApiError> {
        let state = self.state()?;
        let profiles = state
            .with_store(|store| store.list_account_profiles(project_id))
            .map_err(|error| self.refuse(&error))?;
        Ok(profiles
            .iter()
            .map(|profile| account_profile_dto(profile, AppliedDto::Unchanged))
            .collect())
    }

    async fn ensure_account_profile(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &EnsureAccountProfileRequest,
    ) -> Result<AccountProfileDto, ApiError> {
        let state = self.state()?;
        let alias = kontor_core::id::CredentialAlias::parse(&request.credential_alias)
            .map_err(|error| self.refuse_domain(&error))?;
        // The alias is a *request* field and never an answer: it is not credential
        // material, but it is the name a resolver policy is keyed on, and a
        // control plane that echoes it into receipts, logs and error bodies has
        // published its own lookup table. The intent therefore carries a digest of
        // it, which distinguishes two different aliases under one key — which is
        // the whole job of the intent — without storing the value anywhere.
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "provider_account_profiles_ensure",
            "label": request.label.as_str(),
            "harness": request.harness.as_str(),
            "credential_alias_digest": ContentHash::of(alias.as_str().as_bytes()).as_str(),
            "enabled": request.enabled,
        }))?;
        let target = AggregateRef::Project { project_id };
        if let Some(_receipt) = self.replayed(key, &intent, Some(&target))? {
            let profile = self
                .profile_by_label(project_id, &request.label)?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "the replayed receipt names a profile this project no longer has",
                    )
                })?;
            return Ok(account_profile_dto(&profile, AppliedDto::Unchanged));
        }

        if let Some(profile) = self.profile_by_label(project_id, &request.label)? {
            // Every field the caller supplied is compared, not only the ones that
            // would fail a foreign key. An ensure that presents a different
            // harness, a different approved alias or a different launch policy is
            // describing a *different* profile under a name that is already taken,
            // and quietly returning the old one would hand the caller a profile it
            // did not ask for — which is how a run ends up authenticating as
            // somebody else.
            let same = profile.harness == request.harness
                && profile.credential_ref.alias == alias
                && profile.credential_ref.kind == CredentialReferenceKind::ConfigHome
                && profile.enabled == request.enabled;
            if !same {
                // The refusal names the rule and not the field, because naming
                // which field disagreed would confirm a guessed alias.
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "a profile with that label already exists and differs from the one described",
                ));
            }
            self.record(
                key,
                project_id,
                CommandKind::EnsureAccountProfile,
                target,
                AggregateRevision::INITIAL,
                &intent,
            )?;
            return Ok(account_profile_dto(&profile, AppliedDto::Unchanged));
        }

        let empty = self.intent(&serde_json::json!({"schema_version": 1}))?;
        let profile = state
            .with_store(|store| {
                store.create_account_profile(&NewAccountProfile {
                    id: AccountProfileId::generate(),
                    project_id,
                    label: request.label.clone(),
                    external_account_id: None,
                    harness: request.harness.clone(),
                    // The alias is the whole stored reference. Where it resolves
                    // to is the resolver policy's business and never a column.
                    credential_ref: CredentialReference {
                        kind: CredentialReferenceKind::ConfigHome,
                        alias: alias.clone(),
                    },
                    environment: empty.clone(),
                    routing: empty.clone(),
                    capability: empty.clone(),
                    provider_identity: None,
                    enabled: request.enabled,
                    created_at: kontor_api::now(),
                })
            })
            .map_err(|error| self.refuse(&error))?;
        self.record(
            key,
            project_id,
            CommandKind::EnsureAccountProfile,
            target,
            AggregateRevision::INITIAL,
            &intent,
        )?;
        Ok(account_profile_dto(&profile, AppliedDto::Created))
    }

    async fn runtime_capabilities(&self) -> Result<Vec<RuntimeCapabilityDto>, ApiError> {
        let state = self.state()?;
        let mut reported = Vec::new();
        for family in state.runtimes().families().cloned().collect::<Vec<_>>() {
            let Some(adapter) = state.runtimes().get(&family) else {
                continue;
            };
            match adapter.discover_capabilities().await {
                Ok(capabilities) => reported.push(RuntimeCapabilityDto {
                    runtime_kind: family,
                    trust_grade: capabilities.trust_grade.as_str().to_owned(),
                    supported: capabilities
                        .supported
                        .iter()
                        .map(|capability| capability.as_str().to_owned())
                        .collect(),
                    account_env: capabilities.account_env,
                    max_message_bytes: capabilities.limits.max_message_bytes,
                    max_history_page: capabilities.limits.max_history_page,
                    max_concurrent_sessions: capabilities.limits.max_concurrent_sessions,
                    reachable: true,
                }),
                // An unreachable runtime declares nothing. Reporting an empty set
                // as if it were discovered would be a claim about a runtime that
                // never answered.
                Err(_) => reported.push(RuntimeCapabilityDto {
                    runtime_kind: family,
                    trust_grade: kontor_runtime::capability::TrustGrade::C
                        .as_str()
                        .to_owned(),
                    supported: Vec::new(),
                    account_env: false,
                    max_message_bytes: 0,
                    max_history_page: 0,
                    max_concurrent_sessions: 0,
                    reachable: false,
                }),
            }
        }
        Ok(reported)
    }

    // -- Topology specification, catalog and reference ----------------------
    //
    // The Admin tier's defining Operational power: deciding which node kinds may
    // ever exist in a project, and what every controlled code means. These read
    // and write the OP-01 documents — `ProjectSessionTopologySpec` and
    // `RoleCatalogRevision` — and nothing here keeps a second dictionary. A code
    // this server cannot explain stays visibly unknown rather than being guessed,
    // because a client that had to keep its own glossary would eventually
    // disagree with the server about what its own state means.

    fn draft_topology_spec(
        &self,
        project_id: ProjectId,
        request: &DraftTopologySpecRequest,
    ) -> Result<TopologySpecCandidateDto, ApiError> {
        let state = self.state()?;
        // A draft edits a lineage or starts one. Editing means the *next*
        // version of the named revision, which the server derives — a caller
        // that could choose the version could publish over a revision something
        // is already pinned to.
        let (spec_id, version) = match &request.base {
            Some(base) => {
                let spec_id =
                    TopologySpecId::parse(&base.id).map_err(|error| self.refuse_domain(&error))?;
                let published = state
                    .with_store(|store| store.get_topology_spec(project_id, spec_id, base.version))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::NotFound,
                            "the base revision is not published in this project",
                        )
                    })?;
                (
                    spec_id,
                    published
                        .version
                        .next()
                        .map_err(|error| self.refuse_domain(&error))?,
                )
            }
            None => (TopologySpecId::generate(), SpecVersion::FIRST),
        };

        // Assembled by the server, from the parts a caller is allowed to state.
        // The identity, the version and the schema generation are not among
        // them, which is what makes a draft impossible to aim at an existing
        // revision.
        let candidate = serde_json::json!({
            "schema_version": SCHEMA_VERSION.get(),
            "spec_id": spec_id.to_string(),
            "version": version.get(),
            "name": request.name.as_str(),
            "root_kind": request.root_kind.as_str(),
            "node_kinds": request.node_kinds,
            "historical_codes": request.historical_codes,
        });
        // Returned in the exact shape a specification has, so the bytes a
        // caller round-trips through validate and publish are the bytes that
        // get stored. Not validated: judging it is its own operation and its
        // own answer, and a draft that refused an incomplete vocabulary would
        // be useless for the one thing a draft is for.
        let (candidate, hash) =
            match serde_json::from_value::<ProjectSessionTopologySpec>(candidate.clone()) {
                Ok(spec) => (
                    serde_json::to_value(&spec).map_err(|_| {
                        self.deny(
                            ApiErrorCode::Unavailable,
                            "the candidate could not be served",
                        )
                    })?,
                    self.candidate_hash(&spec)?,
                ),
                // A vocabulary this build cannot even parse is still a draft. It
                // carries the identity of the bytes it is, and validation is
                // where a caller learns it is not a specification.
                Err(_) => {
                    let document = self.intent(&candidate)?;
                    (candidate, document.hash().clone())
                }
            };
        Ok(TopologySpecCandidateDto {
            realm_id: state.realm_id(),
            candidate,
            candidate_hash: hash,
        })
    }

    fn validate_topology_spec(
        &self,
        _project_id: ProjectId,
        request: &ValidateTopologySpecRequest,
    ) -> Result<TopologySpecValidationDto, ApiError> {
        let state = self.state()?;
        // Judged against the rules alone. Whether the revision it names is
        // already published is a fact about this project rather than about the
        // candidate, and publication answers it with the conflict it is.
        let (violations, hash) = self.judge_candidate(&request.candidate)?;
        Ok(TopologySpecValidationDto {
            realm_id: state.realm_id(),
            violations,
            validation_hash: hash,
        })
    }

    async fn publish_topology_spec(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &PublishTopologySpecRequest,
    ) -> Result<PublishedTopologySpecDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let project = self.project_at(project_id, request.expected_revision)?;

        // Revalidated, not trusted. The hash proves the caller is publishing the
        // document it had judged; it does not prove the verdict still stands,
        // because the rules live in this build and not in the hash.
        let (violations, hash) = self.judge_candidate(&request.candidate)?;
        if hash != request.validation_hash {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the candidate is not the document the validation answered about",
            ));
        }
        let spec: ProjectSessionTopologySpec = serde_json::from_value(request.candidate.clone())
            .map_err(|_| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "the candidate is not a topology specification document",
                )
            })?;

        // A published revision is immutable. Re-publishing the same bytes is a
        // replay and answers the original; re-publishing *different* bytes under
        // the same identity and version is the shortcut this whole family exists
        // to refuse, and it is refused before anything is written.
        //
        // Judged before the rules on purpose. A caller who edited a published
        // revision has made one mistake, and being told "your vocabulary is
        // invalid" would send them to fix the wrong thing.
        let existing = state
            .with_store(|store| store.get_topology_spec(project_id, spec.spec_id, spec.version))
            .map_err(|error| self.refuse(&error))?;
        let already = match &existing {
            Some(published) => {
                let published_hash = published
                    .canonicalize()
                    .map_err(|error| self.refuse_domain(&error))?
                    .hash()
                    .clone();
                if published_hash != hash {
                    return Err(self.deny(
                        ApiErrorCode::RevisionConflict,
                        "this specification revision is already published with different content",
                    ));
                }
                true
            }
            None => false,
        };
        if !violations.is_empty() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "the candidate does not satisfy the specification rules",
            ));
        }

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "topology_spec_publish",
            "project": project_id.to_string(),
            "spec": spec.spec_id.to_string(),
            "version": spec.version.get(),
            "canonical_hash": hash.as_str(),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();

        let stamp = Shareability::default_for(ShareabilityTier::ProjectKnowledge)
            .map_err(|error| self.refuse_domain(&error))?;
        if !already && !replayed {
            state
                .with_store(|store| store.publish_topology_spec(project_id, &spec, &stamp, now))
                .map_err(|error| self.refuse(&error))?;
        }
        let stored = state
            .with_store(|store| {
                store.get_topology_spec_shareability(project_id, spec.spec_id, spec.version)
            })
            .map_err(|error| self.refuse(&error))?
            .unwrap_or(stamp);
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::PublishTopologySpec,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;

        Ok(PublishedTopologySpecDto {
            spec: PinnedSpecDto {
                id: spec.spec_id,
                version: spec.version,
                canonical_hash: hash,
            },
            shareability: shareability_dto(&stored),
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if already || replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: project.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    fn topology_spec(
        &self,
        project_id: ProjectId,
        spec_id: TopologySpecId,
        version: SpecVersion,
    ) -> Result<TopologySpecDocumentDto, ApiError> {
        let state = self.state()?;
        let spec = state
            .with_store(|store| store.get_topology_spec(project_id, spec_id, version))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such topology specification revision is published in this project",
                )
            })?;
        let shareability = state
            .with_store(|store| store.get_topology_spec_shareability(project_id, spec_id, version))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the published revision carries no classification",
                )
            })?;
        let document = spec
            .canonicalize()
            .map_err(|error| self.refuse_domain(&error))?;
        Ok(TopologySpecDocumentDto {
            realm_id: state.realm_id(),
            spec: PinnedSpecDto {
                id: spec.spec_id,
                version: spec.version,
                canonical_hash: document.hash().clone(),
            },
            document: serde_json::from_str(document.json()).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the published document could not be served",
                )
            })?,
            shareability: shareability_dto(&shareability),
            snapshot_cursor: self.cursor()?,
        })
    }

    fn role_catalog(
        &self,
        catalog_id: RoleCatalogId,
        version: SpecVersion,
    ) -> Result<RoleCatalogDto, ApiError> {
        let state = self.state()?;
        let catalog = self.catalog_revision(catalog_id, version)?;
        Ok(RoleCatalogDto {
            realm_id: state.realm_id(),
            catalog_id: catalog.catalog_id,
            version: catalog.version,
            name: catalog.name.clone(),
            // The catalog's own declaration order, not one this projection
            // chose: the order is part of what was published.
            roles: catalog.roles.iter().map(role_entry_dto).collect(),
            snapshot_cursor: self.cursor()?,
        })
    }

    fn role(
        &self,
        catalog_id: RoleCatalogId,
        version: SpecVersion,
        role_code: &str,
    ) -> Result<RoleCatalogEntryDto, ApiError> {
        let catalog = self.catalog_revision(catalog_id, version)?;
        // Parsed before it is looked up, so a code that could never be a code is
        // refused as malformed rather than reported as absent.
        let code = RoleCode::parse(role_code).map_err(|error| self.refuse_domain(&error))?;
        catalog.role(&code).map(role_entry_dto).ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this catalog revision declares no such role code",
            )
        })
    }

    fn code_help(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<CodeHelpProjectionDto, ApiError> {
        let state = self.state()?;
        let pinned = state
            .with_store(|store| store.get_mini_project_topology(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "this epic is not pinned to a topology revision yet",
                )
            })?;
        let spec = state
            .with_store(|store| {
                store.get_topology_spec(
                    project_id,
                    pinned.topology.spec_id,
                    pinned.topology.version,
                )
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the revision this epic is pinned to is not published in this project",
                )
            })?;
        let source = RevisionRefDto {
            id: spec.spec_id.to_string(),
            version: spec.version,
        };

        // One combined projection, because a client rendering a transcript has a
        // code in hand and does not know which family it came from. Historical
        // codes are included on purpose: a client reading old state still has to
        // render them honestly, and a projection that only described the current
        // vocabulary would force exactly the private glossary this prevents.
        let mut entries: Vec<CodeHelpEntryDto> = spec
            .node_kinds
            .iter()
            .map(|declared| (declared.kind.as_str().to_owned(), &declared.code_help))
            .chain(
                spec.historical_codes
                    .iter()
                    .map(|entry| (entry.kind.as_str().to_owned(), &entry.help)),
            )
            .map(|(code, help)| CodeHelpEntryDto {
                code,
                full_name: help.full_name.clone(),
                meaning: help.meaning.clone(),
                category: help.category,
                lifecycle: help.lifecycle,
                source: source.clone(),
            })
            .collect();

        // The role codes come from the catalog revision the project publishes,
        // which is the same one every seat in it is recorded under.
        let catalog = self.published_catalog()?;
        let catalog_source = RevisionRefDto {
            id: catalog.catalog_id.to_string(),
            version: catalog.version,
        };
        entries.extend(catalog.roles.iter().map(|entry| CodeHelpEntryDto {
            code: entry.role_code.as_str().to_owned(),
            full_name: entry.standard_title.clone(),
            meaning: entry.responsibility_summary.clone(),
            category: CodeCategory::Role,
            lifecycle: entry.lifecycle,
            source: catalog_source.clone(),
        }));
        entries.sort_by(|left, right| {
            left.category
                .as_str()
                .cmp(right.category.as_str())
                .then_with(|| left.code.cmp(&right.code))
        });

        Ok(CodeHelpProjectionDto {
            realm_id: state.realm_id(),
            epic_id,
            entries,
            snapshot_cursor: self.cursor()?,
        })
    }

    fn inspect_topology(
        &self,
        project_id: ProjectId,
        epic_id: Option<MiniProjectId>,
    ) -> Result<TopologyProjectionDto, ApiError> {
        self.topology_projection(project_id, epic_id)
    }

    async fn drift_topology(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &SemanticTopologyRequest,
    ) -> Result<TopologyMutationDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let project = self.project_at(project_id, request.expected_revision)?;
        let scope = self.resolve_scope(project_id, &request.target)?;

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "topology_drift",
            "project": project_id.to_string(),
            "scope": scope.intent_key(),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();

        if !replayed {
            // Drift is a *readback*, so it only ever revisits nodes that were
            // already placed. A node with no stored container has nothing to
            // read back, and inventing an observation for it would make an
            // unplaced node indistinguishable from a placed one that answered.
            for node in self.scope_nodes(project_id, &scope)? {
                let Some(binding) = state
                    .with_store(|store| store.get_topology_node_container(project_id, node.id))
                    .map_err(|error| self.refuse(&error))?
                else {
                    continue;
                };
                // The exact identity, re-confirmed against the family that
                // issued it. A family this Realm is no longer configured with
                // cannot confirm anything, so the stored readback instant stays
                // where it was — an old confirmation reads as old.
                if state
                    .runtimes()
                    .get(&binding.identity.runtime_kind)
                    .is_none()
                {
                    continue;
                }
                state
                    .with_store(|store| {
                        store.bind_topology_node_container(&NewNativeContainerBinding {
                            topology_node_id: node.id,
                            project_id,
                            container_binding_id: binding.container_binding_id.clone(),
                            identity: binding.identity.clone(),
                            observed_kind: binding.observed_kind,
                            canonical_cwd: binding.canonical_cwd.clone(),
                            observed_at: now,
                        })
                    })
                    .map_err(|error| self.refuse(&error))?;
            }
        }

        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::ObserveSeat,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        self.topology_mutation(
            project_id,
            scope.epic_id(),
            receipt_id,
            replayed,
            project.revision,
        )
    }

    async fn ensure_topology(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &SemanticTopologyRequest,
    ) -> Result<TopologyMutationDto, ApiError> {
        let project = self.project_at(project_id, request.expected_revision)?;
        let scope = self.resolve_scope(project_id, &request.target)?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "topology_ensure",
            "project": project_id.to_string(),
            "scope": scope.intent_key(),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();
        if !replayed {
            self.ensure_scope_chain(project_id, &scope)?;
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::EnsureProject,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        self.topology_mutation(
            project_id,
            scope.epic_id(),
            receipt_id,
            replayed,
            project.revision,
        )
    }

    async fn materialize_topology(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &SemanticTopologyRequest,
    ) -> Result<TopologyMutationDto, ApiError> {
        let state = self.state()?;
        let project = self.project_at(project_id, request.expected_revision)?;
        let scope = self.resolve_scope(project_id, &request.target)?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "topology_materialize",
            "project": project_id.to_string(),
            "scope": scope.intent_key(),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();

        if !replayed {
            // Materializing is ensuring plus binding the seats the scope hosts.
            // The chain comes first because a seat can only ever belong to a
            // node that exists.
            let leaf = self.ensure_scope_chain(project_id, &scope)?;
            let spec = self.pinned_spec(project_id)?;
            let declared = spec
                .node_kinds
                .iter()
                .find(|declared| declared.kind == leaf.kind)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the pinned specification no longer declares this node's kind",
                    )
                })?;
            // Capability-dispatched, exactly as OP-02 does it: only a kind the
            // specification declares as a session host may hold a seat. A kind
            // that is a native root hosts nothing, and opening a seat on one
            // would be Kontor placing work where the vocabulary says it cannot
            // go.
            if declared
                .projection_capabilities
                .contains(&NodeProjectionCapability::SessionHost)
            {
                let slot = self.control_slot()?;
                let held = state
                    .with_store(|store| store.list_seat_bindings(project_id, leaf.id))
                    .map_err(|error| self.refuse(&error))?
                    .into_iter()
                    .any(|binding| binding.role_slot_id == slot && binding.is_non_terminal());
                if !held {
                    let role =
                        self.catalog_role_for_code(&self.domain.delivery.control_role_code)?;
                    let now = kontor_api::now();
                    let deadline = now
                        .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
                        .unwrap_or(now);
                    state
                        .with_store(|store| {
                            store.create_seat_binding(&NewSeatBinding {
                                id: SeatBindingId::generate(),
                                project_id,
                                topology_node_id: leaf.id,
                                role_slot_id: slot.clone(),
                                role: role.clone(),
                                task_id: leaf.task_id,
                                team_run_id: None,
                                attach_deadline: deadline,
                                parent_seat_binding_id: None,
                                created_at: now,
                            })
                        })
                        .map_err(|error| self.refuse(&error))?;
                }
            }
        }

        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::StartScheduledWork,
            AggregateRef::MiniProject {
                mini_project_id: scope.epic_id().ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "this scope has no epic for a materialization receipt to name",
                    )
                })?,
            },
            project.revision,
            &intent,
        )?;
        self.topology_mutation(
            project_id,
            scope.epic_id(),
            receipt_id,
            replayed,
            project.revision,
        )
    }

    async fn retire_topology_node(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        request: &TopologyNodeRequest,
    ) -> Result<TopologyMutationDto, ApiError> {
        self.move_node_lifecycle(
            key,
            project_id,
            topology_node_id,
            request,
            TopologyLifecycle::Retired,
        )
    }

    async fn archive_topology_node(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        request: &TopologyNodeRequest,
    ) -> Result<TopologyMutationDto, ApiError> {
        self.move_node_lifecycle(
            key,
            project_id,
            topology_node_id,
            request,
            TopologyLifecycle::Archived,
        )
    }

    fn preview_topology_upgrade(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &TopologyUpgradePreviewRequest,
    ) -> Result<TopologyUpgradePreviewDto, ApiError> {
        let state = self.state()?;
        let (current, target, effects) =
            self.upgrade_effects(project_id, epic_id, &request.target_spec)?;
        Ok(TopologyUpgradePreviewDto {
            realm_id: state.realm_id(),
            epic_id,
            current_spec: pinned_spec_dto(&current),
            target_spec: pinned_spec_dto(&target),
            preview_hash: self.upgrade_hash(project_id, epic_id, &current, &target, &effects)?,
            effects,
            snapshot_cursor: self.cursor()?,
        })
    }

    async fn apply_topology_upgrade(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &TopologyUpgradeApplyRequest,
    ) -> Result<AppliedTopologyUpgradeDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let epic = self.epic_row(project_id, epic_id)?;
        if epic.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the epic moved since the caller read it",
                )
                .with_revision(Some(epic.revision)));
        }

        // The intent names the *preview*, not the target it resolves to, and the
        // key is judged before anything is searched for. Both follow from the
        // same rule: what the caller authorized is a set of effects, named by
        // their digest. Recording the target instead would mean re-deriving the
        // preview to find out what to record — and once the first call has
        // moved the pin, that preview no longer describes the Realm, so a
        // perfectly ordinary retry would be refused for succeeding.
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "topology_upgrade_apply",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "preview": request.preview_hash.as_str(),
        }))?;
        let replayed = self
            .replayed(
                key,
                &intent,
                Some(&AggregateRef::MiniProject {
                    mini_project_id: epic_id,
                }),
            )?
            .is_some();
        if !replayed {
            // Recomputed rather than remembered. A stored preview would let an
            // apply commit effects the Realm no longer has; searching the
            // published revisions for the one that still produces exactly these
            // is what makes the authorization mean what it said.
            let target = self.target_of_preview(project_id, epic_id, &request.preview_hash)?;
            state
                .with_store(|store| {
                    store.repin_mini_project_topology(&MiniProjectTopologySnapshot {
                        project_id,
                        mini_project_id: epic_id,
                        topology: target,
                        pinned_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::UpgradeTopology,
            AggregateRef::MiniProject {
                mini_project_id: epic_id,
            },
            epic.revision,
            &intent,
        )?;

        Ok(AppliedTopologyUpgradeDto {
            pinned_spec: pinned_spec_dto(&self.epic_pin(project_id, epic_id)?),
            projection: self.topology_projection(project_id, Some(epic_id))?,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: epic.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    // -- Native capacity and exact-seat operations ---------------------------
    //
    // `kontor-accounts` owns the raw observations, the derived availability and
    // the adaptive transition; `kontor-scheduler` owns the arithmetic; the
    // collectors are composed here, against the runtime families this Realm was
    // actually configured with. Nothing in this section shells out, reads
    // another program's store, or reports an availability it has not observed —
    // a fabricated "available" is the one answer that would let the scheduler
    // admit work against a provider that never agreed.

    fn capacity_configuration(&self) -> Result<CapacityConfigurationDto, ApiError> {
        let state = self.state()?;
        let stored = state
            .with_store(SqliteStore::get_capacity_configuration)
            .map_err(|error| self.refuse(&error))?;
        Ok(CapacityConfigurationDto {
            realm_id: state.realm_id(),
            // The ceilings this Realm is *admitting under*, which are the ones
            // it was composed with. An operator's stored replacement is a
            // separate fact, and it is reported through its revision rather
            // than by answering with numbers nothing is enforcing yet.
            ceilings: ceilings_dto(self.capacity),
            revision: stored
                .as_ref()
                .map_or(AggregateRevision::INITIAL, |stored| stored.revision),
            snapshot_cursor: self.cursor()?,
        })
    }

    fn preview_capacity_configuration(
        &self,
        request: &CapacityConfigurationRequest,
    ) -> Result<CapacityConfigurationPreviewDto, ApiError> {
        let state = self.state()?;
        let proposed = capacity_config(&request.ceilings);
        proposed
            .validate()
            .map_err(|error| self.refuse_domain(&error))?;

        // What a caller actually needs to see before applying: which of the
        // windows currently open would be narrower than they are now. Computed
        // against the composed ceilings rather than the stored ones, because
        // those are what is in force.
        let current = self.capacity;
        let mut clamped = Vec::new();
        for (name, before, after) in [
            (
                "global_max_in_flight",
                current.global_max_in_flight,
                proposed.global_max_in_flight,
            ),
            (
                "project_max_in_flight",
                current.project_max_in_flight,
                proposed.project_max_in_flight,
            ),
            (
                "mission_max_in_flight",
                current.mission_max_in_flight,
                proposed.mission_max_in_flight,
            ),
            (
                "account_max_in_flight",
                current.account_max_in_flight,
                proposed.account_max_in_flight,
            ),
            (
                "provider_max_in_flight",
                current.provider_max_in_flight,
                proposed.provider_max_in_flight,
            ),
            (
                "runtime_max_in_flight",
                current.runtime_max_in_flight,
                proposed.runtime_max_in_flight,
            ),
            (
                "adaptive.ceiling",
                current.adaptive.ceiling,
                proposed.adaptive.ceiling,
            ),
        ] {
            if after < before {
                clamped.push(name.to_owned());
            }
        }

        Ok(CapacityConfigurationPreviewDto {
            realm_id: state.realm_id(),
            ceilings: request.ceilings.clone(),
            clamped,
            preview_hash: self.preview_hash(&serde_json::json!({
                "schema_version": 1,
                "operation": "capacity_configuration_preview",
                "ceilings": request.ceilings,
                "expected_revision": request.expected_revision.get(),
            }))?,
        })
    }

    async fn apply_capacity_configuration(
        &self,
        key: &IdempotencyKey,
        request: &CapacityConfigurationRequest,
    ) -> Result<CapacityConfigurationDto, ApiError> {
        let state = self.state()?;
        let proposed = capacity_config(&request.ceilings);
        proposed
            .validate()
            .map_err(|error| self.refuse_domain(&error))?;
        let document = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "capacity_configuration_apply",
            "ceilings": request.ceilings,
        }))?;
        let binding = IdempotencyBinding {
            key: key.as_str().to_owned(),
            operation: "apply_capacity_configuration",
            fingerprint: document.hash().clone(),
            bound_at: kontor_api::now(),
        };
        let stored = state
            .with_store(|store| {
                store.set_capacity_configuration(&document, &binding, request.expected_revision)
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(CapacityConfigurationDto {
            realm_id: state.realm_id(),
            // The stored document, not the composed one: this answer is about
            // the record that was just written. The Realm keeps admitting under
            // the ceilings it started with until it next composes — re-reading
            // them between planning a batch and committing it could refuse a
            // candidate the plan had already admitted.
            ceilings: stored
                .ceilings
                .deserialize::<StoredCeilings>()
                .map(|stored| stored.ceilings)
                .map_err(|error| self.refuse_domain(&error))?,
            revision: stored.revision,
            snapshot_cursor: self.cursor()?,
        })
    }

    fn project_capacity(&self, project_id: ProjectId) -> Result<ProjectCapacityDto, ApiError> {
        self.capacity_projection(project_id)
    }

    async fn refresh_capacity(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &CapacityRefreshRequest,
    ) -> Result<ProjectCapacityDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let project = state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })?;

        // Only accounts this project already has. A refresh that could name
        // anything else would be choosing what to talk to, which is
        // configuration and not a request.
        let profiles = state
            .with_store(|store| store.list_account_profiles(project_id))
            .map_err(|error| self.refuse(&error))?;
        let selected: Vec<_> = if request.account_profile_ids.is_empty() {
            profiles
        } else {
            let mut selected = Vec::with_capacity(request.account_profile_ids.len());
            for wanted in &request.account_profile_ids {
                let profile = profiles
                    .iter()
                    .find(|profile| &profile.id == wanted)
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::NotFound,
                            "the request named an account profile this project does not have",
                        )
                    })?;
                selected.push(profile.clone());
            }
            selected
        };

        let mut collected: Vec<_> = selected
            .iter()
            .map(|profile| profile.id.to_string())
            .collect();
        collected.sort();
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "capacity_refresh",
            "project": project_id.to_string(),
            "accounts": collected,
        }))?;
        // A replayed refresh answers from what is durable rather than probing
        // again: two probes are two different facts, and the caller asked for
        // the one it already got.
        if self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some()
        {
            return self.capacity_projection(project_id);
        }

        let mut readings = Vec::with_capacity(selected.len());
        for profile in &selected {
            let discovery = match state.runtimes().get(&profile.harness) {
                Some(adapter) => adapter.discover_capabilities().await,
                // A family this Realm was never configured with is unreachable,
                // which is a fact about the deployment and not the provider —
                // so it must not read as pressure. `ProbeRefusal` keeps them
                // apart.
                None => Err(RuntimeError::AccountEnvironmentUnavailable),
            };
            let reading = CapacityReading {
                schema_version: SCHEMA_VERSION,
                profile_enabled: profile.enabled,
                runtime_kind: profile.harness.clone(),
                probe: ProbeOutcome::of(discovery.as_ref()),
            };
            let derived = kontor_accounts::derive(&reading, now);
            readings.push((profile.id, reading, derived));
        }

        // Raw first, and derived in the same call: a row whose conclusion was
        // written by a later pass could disagree with its own evidence.
        let mut last_observation = None;
        for (account_profile_id, reading, derived) in &readings {
            let document = CanonicalDocument::from_serializable(reading)
                .map_err(|error| self.refuse_domain(&error))?;
            let observation_id = kontor_core::id::CapacityObservationId::generate();
            state
                .with_store(|store| {
                    store.record_capacity_observation(&NewCapacityObservation {
                        id: observation_id,
                        project_id,
                        account_profile_id: *account_profile_id,
                        observed_at: now,
                        reading: document.clone(),
                        available: derived.available,
                        pressure: derived.pressure,
                        cooling_until: match derived.availability {
                            AccountAvailability::Cooling { blocked_until } => Some(blocked_until),
                            AccountAvailability::Available | AccountAvailability::Unknown => None,
                        },
                    })
                })
                .map_err(|error| self.refuse(&error))?;
            last_observation = Some(observation_id);
        }

        // One refresh moves each epic's position once. The verdict is the
        // whole batch's: any account under pressure narrows the window, because
        // a Realm that kept widening while one provider throttled would keep
        // walking back into it.
        if let Some(observation_id) = last_observation {
            let verdict = if readings.iter().any(|(_, _, derived)| derived.pressure) {
                kontor_scheduler::model::CapacityObservation::Pressure
            } else {
                kontor_scheduler::model::CapacityObservation::Clean
            };
            self.fold_admission(project_id, &observation_id.to_string(), verdict, now)?;
        }

        self.record(
            key,
            project_id,
            CommandKind::RefreshCapacity,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        self.capacity_projection(project_id)
    }

    fn capacity_observation(
        &self,
        project_id: ProjectId,
        observation_id: kontor_core::id::CapacityObservationId,
    ) -> Result<CapacityObservationDto, ApiError> {
        let state = self.state()?;
        let stored = state
            .with_store(|store| store.get_capacity_observation(project_id, observation_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such capacity observation exists in this project",
                )
            })?;
        Ok(CapacityObservationDto {
            realm_id: state.realm_id(),
            observation_id: stored.id,
            account_profile_id: stored.account_profile_id,
            observed_at: stored.observed_at,
            // Served exactly as it was stored. It is closed by construction —
            // every field is a token, a boolean or a runtime kind — so there is
            // nothing here to redact and nothing that could have arrived
            // carrying a credential or an endpoint.
            reading: serde_json::to_value(&stored.reading).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the stored observation could not be served",
                )
            })?,
            available: stored.available,
            pressure: stored.pressure,
        })
    }

    async fn override_availability(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        account_profile_id: AccountProfileId,
        request: &AvailabilityOverrideRequest,
    ) -> Result<AvailabilityOverrideDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let project = state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })?;
        state
            .with_store(|store| store.get_account_profile(project_id, account_profile_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such account profile exists in this project",
                )
            })?;

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "capacity_override",
            "project": project_id.to_string(),
            "account": account_profile_id.to_string(),
            "available": request.available,
            "reason": request.reason.as_str(),
            "expires_at": request.expires_at.map(|expiry| expiry.to_string()),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();
        if !replayed {
            state
                .with_store(|store| {
                    store.set_availability_override(&NewAvailabilityOverride {
                        project_id,
                        account_profile_id,
                        available: request.available,
                        reason: request.reason.clone(),
                        expires_at: request.expires_at,
                        expected_revision: request.expected_revision,
                        updated_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::OverrideAvailability,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;

        let projection = self.capacity_projection(project_id)?;
        let account = projection
            .accounts
            .into_iter()
            .find(|account| account.account_profile_id == account_profile_id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the account could not be read back after the override",
                )
            })?;
        let revision = state
            .with_store(|store| store.list_availability_overrides(project_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|stored| stored.account_profile_id == account_profile_id)
            .map_or(AggregateRevision::INITIAL, |stored| stored.revision);
        Ok(AvailabilityOverrideDto {
            account,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    async fn seat_attention(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        request: &SeatBindingRequest,
    ) -> Result<SeatBindingOutcomeDto, ApiError> {
        self.address_exact_seat(key, project_id, seat_binding_id, request, SeatAct::Observe)
            .await
    }

    async fn retire_seat(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        request: &SeatBindingRequest,
    ) -> Result<SeatBindingOutcomeDto, ApiError> {
        self.address_exact_seat(key, project_id, seat_binding_id, request, SeatAct::Retire)
            .await
    }

    fn core_team(&self, project_id: ProjectId) -> Result<CoreTeamDto, ApiError> {
        let state = self.state()?;
        // A project that does not exist has no configuration to report, and
        // answering with an empty roster would make it look configurable.
        self.project_row(project_id)?;
        // A project that has published nothing has no Core Team — and it must
        // not be told it has an empty one. Every valid roster contains a
        // required LSA and TPM, so an empty seat list is a state this domain
        // cannot reach; answering with one would be indistinguishable from a
        // real roster that happens to seat nobody, and a caller would act on it.
        let stored = self.stored_core_team(project_id)?.ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this project has published no Core Team revision",
            )
        })?;
        Ok(CoreTeamDto {
            realm_id: state.realm_id(),
            project_id,
            // `seat_binding_id` is absent throughout: this is the roster a
            // project staffs epics *from*. The seats that fill it belong to one
            // epic's control plane, and the route that returns them is scoped by
            // an `epic_id` this one does not have.
            seats: self.core_team_seat_dtos(&stored)?,
            revision: core_team_revision_of(Some(&stored)),
            snapshot_cursor: self.cursor()?,
        })
    }

    fn preview_core_team(
        &self,
        project_id: ProjectId,
        request: &CoreTeamPreviewRequest,
    ) -> Result<CoreTeamPreviewDto, ApiError> {
        let state = self.state()?;
        self.project_row(project_id)?;
        let stored = self.stored_core_team(project_id)?;
        // Pure: the candidate revision is resolved and hashed, and nothing is
        // written. No draft, no id, no receipt — an apply recomputes this from
        // current state and compares the hash, so a stored plan here would only
        // be a second answer able to disagree with the Realm.
        let proposed = self.resolve_core_team(project_id, &request.seats, stored.as_ref())?;
        let effects = core_team_effects(stored.as_ref(), &proposed)
            .map_err(|error| self.refuse_domain(&error))?;
        Ok(CoreTeamPreviewDto {
            realm_id: state.realm_id(),
            preview_hash: self.core_team_hash(project_id, &proposed, &effects)?,
            effects,
        })
    }

    async fn apply_core_team(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &CoreTeamApplyRequest,
    ) -> Result<CoreTeamOutcomeDto, ApiError> {
        let state = self.state()?;
        let project = self.project_row(project_id)?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "core_team_apply",
            "project": project_id.to_string(),
            "preview": request.preview_hash.as_str(),
        }))?;
        // Replay is judged before the expected revision, unlike a topology
        // upgrade. Publishing moves this aggregate's revision, so a retry after
        // a lost acknowledgement necessarily presents the revision it read
        // before the first attempt. Checking that first would refuse the retry
        // for the sole reason that the original call succeeded.
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();

        if !replayed {
            let stored = self.stored_core_team(project_id)?;
            let current = core_team_revision_of(stored.as_ref());
            if current != request.expected_revision {
                return Err(self
                    .deny(
                        ApiErrorCode::RevisionConflict,
                        "the Core Team moved since the caller read it",
                    )
                    .with_revision(Some(current)));
            }
            // Recomputed rather than remembered, then held to the hash the
            // caller was shown. What was authorized is this exact roster
            // resolved against these exact catalog revisions.
            let proposed = self.resolve_core_team(project_id, &request.seats, stored.as_ref())?;
            let effects = core_team_effects(stored.as_ref(), &proposed)
                .map_err(|error| self.refuse_domain(&error))?;
            if self.core_team_hash(project_id, &proposed, &effects)? != request.preview_hash {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "the apply does not match the named preview",
                ));
            }
            let seats = serde_json::to_value(&proposed.seats).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the resolved Core Team could not be canonicalized",
                )
            })?;
            state
                .with_store(|store| {
                    store.publish_core_team_revision(&StoredCoreTeamRevision {
                        project_id,
                        version: proposed.version,
                        catalog_hash: proposed.catalog_hash.clone(),
                        seats,
                        published_at: kontor_api::now(),
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }

        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::ApplyCoreTeam,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        let core_team = self.core_team(project_id)?;
        Ok(CoreTeamOutcomeDto {
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: core_team.revision,
                snapshot_cursor: self.cursor()?,
            },
            core_team,
        })
    }
    async fn materialize_core_team(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &CoreTeamMaterializeRequest,
    ) -> Result<CoreTeamOutcomeDto, ApiError> {
        let state = self.state()?;
        let epic = self.epic_row(project_id, epic_id)?;
        if epic.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the epic moved since the caller read it",
                )
                .with_revision(Some(epic.revision)));
        }
        // The roster the epic froze, never the project's current one. An epic
        // staffed from whatever the project happens to say today would quietly
        // acquire roles decided after it started.
        let roster = self.frozen_roster(project_id, epic_id)?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "core_team_materialize",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
        }))?;
        let replayed = self
            .replayed(
                key,
                &intent,
                Some(&AggregateRef::MiniProject {
                    mini_project_id: epic_id,
                }),
            )?
            .is_some();
        if !replayed {
            let control = self.ensure_scope_chain(
                project_id,
                &self.resolve_scope(
                    project_id,
                    &SemanticTopologyTargetDto::EpicControl { epic_id },
                )?,
            )?;
            // Missing seats only. Every seat already there keeps its identity,
            // because a seat binding is what a running agent is attached to.
            self.materialize_roster_seats(project_id, &control, &roster, kontor_api::now())?;
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::MaterializeCoreTeam,
            AggregateRef::MiniProject {
                mini_project_id: epic_id,
            },
            epic.revision,
            &intent,
        )?;
        Ok(CoreTeamOutcomeDto {
            core_team: self.epic_core_team_dto(project_id, epic_id, &roster)?,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: epic.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }
    fn quick_roles(&self, project_id: ProjectId) -> Result<QuickRolesDto, ApiError> {
        let state = self.state()?;
        self.project_row(project_id)?;
        // Derived, never stored. There is no Quick Team aggregate: the roles a
        // Quick session may be opened against are exactly the current Core Team
        // entries marked `ad_hoc_allowed`, so a second list here could disagree
        // with the roster the moment either was edited.
        let roster = self.stored_core_team(project_id)?.ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this project has published no Core Team revision",
            )
        })?;
        let mut roles = Vec::new();
        for seat in roster.seats.iter().filter(|seat| seat.ad_hoc_allowed) {
            let catalog =
                self.catalog_revision(seat.role.catalog_id, seat.role.catalog_revision)?;
            let entry = catalog.role(&seat.role.role_code).ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the catalog revision this seat is pinned to no longer declares it",
                )
            })?;
            roles.push(role_entry_dto(entry));
        }
        Ok(QuickRolesDto {
            realm_id: state.realm_id(),
            project_id,
            roles,
            snapshot_cursor: self.cursor()?,
        })
    }

    async fn ensure_quick_session(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &EnsureQuickSessionRequest,
    ) -> Result<QuickSessionDto, ApiError> {
        let state = self.state()?;
        let project = self.project_row(project_id)?;
        let purpose = BoundedText::parse(request.purpose.as_str())
            .map_err(|error| self.refuse_domain(&error))?;

        // Everything below is checked before the first native effect, in the
        // order the architecture fixes: the roster, then the role's ad-hoc
        // eligibility, then the exact base readback.
        let roster = self.stored_core_team(project_id)?.ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this project has published no Core Team revision",
            )
        })?;
        let catalog_id = RoleCatalogId::parse(&request.role.catalog_revision.id)
            .map_err(|error| self.refuse_domain(&error))?;
        let seat = roster
            .seats
            .iter()
            .find(|seat| {
                seat.role.role_code == request.role.role_code
                    && seat.role.catalog_id == catalog_id
                    && seat.role.catalog_revision == request.role.catalog_revision.version
            })
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "that role is not on this project's Core Team at that catalog revision",
                )
            })?;
        if !seat.ad_hoc_allowed {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "that Core Team role may not open a Quick session",
            ));
        }

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "quick_session_ensure",
            "project": project_id.to_string(),
            "role": seat.role.role_code.as_str(),
            "catalog": seat.role.catalog_revision.get(),
            "purpose": purpose.as_str(),
        }))?;
        // The command record is what makes a lost acknowledgement safe: a retry
        // finds the same receipt and reads back the session it already opened,
        // rather than placing a second workspace for the same request.
        self.replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?;

        // The pinned specification is consulted before anything is written: a
        // vocabulary that does not declare a Quick session kind, or declares it
        // as something that cannot host a seat, is `placement_blocked` and
        // leaves no trace.
        let topology = self.project_topology(project_id)?;
        let spec = self.pinned_spec(project_id)?;
        let kind = self.domain.delivery.quick_kind.clone();
        let declared = spec
            .node_kinds
            .iter()
            .find(|declared| declared.kind == kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the pinned specification declares no Quick session kind",
                )
            })?;
        if !declared
            .projection_capabilities
            .contains(&NodeProjectionCapability::SessionHost)
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the pinned specification does not declare the Quick session kind a session host",
            ));
        }

        let base = self.session_base(project_id)?;
        let now = kontor_api::now();

        // The row comes first, and it carries the ids the effects will use.
        //
        // It is the only thing that can reconcile a retry: the node cannot be
        // found by searching, because two Quick sessions in one project are
        // both QSW nodes below the same base and a search cannot tell them
        // apart. Created after its effects, it would leave any failure in
        // between with an orphaned node and an unattached seat binding that
        // nothing can attribute — and an unattached seat binding is exactly the
        // artefact the OP-REQ-039 phantom was made of. The columns are plain
        // `TEXT` with no foreign keys precisely so the row can be written while
        // the things it names do not exist yet.
        let existing = self.quick_session_for_intent(project_id, &intent)?;
        let session = match existing.clone() {
            Some(session) => session,
            None => {
                let mut role = seat.role.clone();
                role.custom_display_name
                    .clone_from(&request.role.custom_display_name);
                let planned = StoredQuickSession {
                    id: QuickSessionId::generate(),
                    project_id,
                    role,
                    role_slot_id: seat.role_slot_id.clone(),
                    topology_node_id: TopologyNodeId::generate(),
                    seat_binding_id: SeatBindingId::generate(),
                    psw_topology_node_id: base.node.id,
                    psw_native_id: base.native_id.clone(),
                    purpose,
                    intent_hash: intent.hash().clone(),
                    // Idle, and only ever moved from idle by an explicit archive
                    // after a promotion has delivered its handoff.
                    disposition: SourceDisposition::Idle,
                    revision: AggregateRevision::INITIAL,
                    created_at: now,
                };
                match state.with_store(|store| store.create_quick_session(&planned)) {
                    Ok(()) => planned,
                    // Another ensure of the same request won the race. It has a
                    // session and this one has written nothing, so the answer is
                    // theirs — reconciling below against their ids rather than
                    // placing a second workspace under the same base.
                    Err(RepositoryError::Conflict { .. }) => self
                        .quick_session_for_intent(project_id, &intent)?
                        .ok_or_else(|| {
                            self.deny(
                                ApiErrorCode::Unavailable,
                                "a Quick session was claimed by another command and then vanished",
                            )
                        })?,
                    Err(error) => return Err(self.refuse(&error)),
                }
            }
        };

        // Everything below reconciles by the ids that row already froze, so a
        // resumed ensure completes whichever suffix is missing rather than
        // starting again.
        if state
            .with_store(|store| store.get_topology_node(project_id, session.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .is_none()
        {
            state
                .with_store(|store| {
                    store.create_topology_node(&NewSessionTopologyNode {
                        id: session.topology_node_id,
                        project_id,
                        mini_project_id: None,
                        topology: topology.clone(),
                        kind: kind.clone(),
                        parent_id: Some(session.psw_topology_node_id),
                        task_id: None,
                        created_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }
        let seated = state
            .with_store(|store| store.list_seat_bindings(project_id, session.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .any(|binding| binding.id == session.seat_binding_id);
        if !seated {
            let deadline = now
                .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
                .unwrap_or(now);
            state
                .with_store(|store| {
                    store.create_seat_binding(&NewSeatBinding {
                        id: session.seat_binding_id,
                        project_id,
                        topology_node_id: session.topology_node_id,
                        role_slot_id: session.role_slot_id.clone(),
                        role: session.role.clone(),
                        // A Quick session is not delivery work: it has no task
                        // and no TeamRun, and so consumes no mission slot.
                        task_id: None,
                        team_run_id: None,
                        attach_deadline: deadline,
                        parent_seat_binding_id: None,
                        created_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }

        self.record(
            key,
            project_id,
            CommandKind::EnsureQuickSession,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        // `unchanged` means this command had already opened this session — read
        // from the durable row rather than from the receipt ledger, because a
        // second key naming the same request reconciles the same session and
        // reporting that as `created` would claim a workspace it did not place.
        self.quick_session_dto(&session, existing.is_some())
    }

    fn preview_promotion(
        &self,
        project_id: ProjectId,
        quick_session_id: QuickSessionId,
    ) -> Result<PromotionPreviewDto, ApiError> {
        let state = self.state()?;
        let (session, roster) = self.promotable(project_id, quick_session_id)?;
        let effects =
            promotion_effects(&session, &roster).map_err(|error| self.refuse_domain(&error))?;
        Ok(PromotionPreviewDto {
            realm_id: state.realm_id(),
            quick_session_id,
            preview_hash: self.promotion_hash(&session, &roster, &effects)?,
            effects,
        })
    }

    async fn apply_promotion(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        quick_session_id: QuickSessionId,
        request: &PromotionApplyRequest,
    ) -> Result<PromotedSessionDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();

        // A promotion already recorded is resumed, never restarted. The row was
        // written before the first effect and carries the ids those effects
        // use, so everything below reconciles what is already there and fills
        // in whichever suffix is missing.
        let existing = state
            .with_store(|store| store.get_promotion(quick_session_id))
            .map_err(|error| self.refuse(&error))?;
        let (session, roster) = match &existing {
            Some(promotion) => {
                if promotion.preview_hash != request.preview_hash {
                    return Err(self.deny(
                        ApiErrorCode::InvalidRequest,
                        "this source is already being promoted under a different preview",
                    ));
                }
                let session = self.quick_session_row(project_id, quick_session_id)?;
                let roster = self.frozen_roster(project_id, promotion.mini_project_id)?;
                (session, roster)
            }
            None => {
                let (session, roster) = self.promotable(project_id, quick_session_id)?;
                if session.revision != request.expected_revision {
                    return Err(self
                        .deny(
                            ApiErrorCode::RevisionConflict,
                            "the source moved since the caller previewed it",
                        )
                        .with_revision(Some(session.revision)));
                }
                let effects = promotion_effects(&session, &roster)
                    .map_err(|error| self.refuse_domain(&error))?;
                if self.promotion_hash(&session, &roster, &effects)? != request.preview_hash {
                    return Err(self.deny(
                        ApiErrorCode::InvalidRequest,
                        "the apply does not match the named preview",
                    ));
                }
                (session, roster)
            }
        };

        let epic_id = match &existing {
            Some(promotion) => promotion.mini_project_id,
            None => {
                let epic_id = MiniProjectId::generate();
                // Both reconciliation keys, in one transaction, before the
                // first effect. The architecture orders the transaction this
                // way — freeze the Core Team revision at step 2, create the
                // MiniProject at step 4 — and the resume path depends on it:
                // it reads the frozen roster before anything else, so a roster
                // written after the effects would leave any failure in between
                // recorded as promoted and impossible to resume. The promotion
                // row is keyed by its source and nothing deletes it, so that
                // source would be unpromotable for good.
                let roster_row = self.epic_roster_row(
                    project_id,
                    epic_id,
                    &roster,
                    Some(quick_session_id),
                    now,
                )?;
                state
                    .with_store(|store| {
                        store.begin_promotion(
                            &StoredPromotion {
                                quick_session_id,
                                project_id,
                                mini_project_id: epic_id,
                                preview_hash: request.preview_hash.clone(),
                                // The fixed contract authorizes no archive, so
                                // the source stays idle. Reporting an archive
                                // the request never asked for would be claiming
                                // an effect nobody authorized.
                                source_disposition: SourceDisposition::Idle,
                                handoff: None,
                                handoff_hash: None,
                                lsa_seat_binding_id: None,
                                completed_at: None,
                                created_at: now,
                            },
                            &roster_row,
                        )
                    })
                    .map_err(|error| self.refuse(&error))?;
                epic_id
            }
        };

        // A tracker-neutral MiniProject in the ordinary pre-execution planning
        // lifecycle. No TeamRun starts, no phase is skipped, and no ASMA Epic
        // policy activates — that needs a confirmed Jira Epic binding, which
        // this contract carries no way to supply.
        if self.epic_row_opt(project_id, epic_id)?.is_none() {
            let name = ExternalName::parse(session.purpose.as_str())
                .map_err(|error| self.refuse_domain(&error))?;
            state
                .with_store(|store| {
                    store.create_mini_project(&NewMiniProject {
                        id: epic_id,
                        project_id,
                        name: name.clone(),
                        created_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }

        // ESW as its own native project, then exactly one ECP inside it, both
        // through the OP-02 chain that owns placement.
        self.ensure_scope_chain(
            project_id,
            &self.resolve_scope(project_id, &SemanticTopologyTargetDto::Epic { epic_id })?,
        )?;
        let control = self.ensure_scope_chain(
            project_id,
            &self.resolve_scope(
                project_id,
                &SemanticTopologyTargetDto::EpicControl { epic_id },
            )?,
        )?;

        // The roster was frozen with the promotion row above, so the seats and
        // the record of what they were created from cannot disagree, and a
        // resumed apply finds both.
        let seats = self.materialize_roster_seats(project_id, &control, &roster, now)?;
        let lsa = seats
            .iter()
            .find(|(seat, _)| seat.role.role_code.as_str() == MANDATORY_LEAD_ROLE)
            .map(|(_, id)| *id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the frozen roster has no LSA seat to hand the work to",
                )
            })?;

        // Delivery last, and success only after it. A promotion that reported
        // done with the handoff still undelivered would leave the epic's lead
        // architect holding an epic and none of the work that justified it.
        if existing.as_ref().and_then(|it| it.completed_at).is_none() {
            let handoff = self.promotion_handoff(&session, &roster, epic_id, lsa)?;
            let document = self.intent(&handoff)?;
            state
                .with_store(|store| {
                    store.complete_promotion(quick_session_id, &handoff, document.hash(), lsa, now)
                })
                .map_err(|error| self.refuse(&error))?;
        }

        let epic = self.epic_row(project_id, epic_id)?;
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::PromoteQuickSession,
            AggregateRef::MiniProject {
                mini_project_id: epic_id,
            },
            epic.revision,
            &self.intent(&serde_json::json!({
                "schema_version": 1,
                "operation": "promotion_apply",
                "project": project_id.to_string(),
                "source": quick_session_id.to_string(),
                "preview": request.preview_hash.as_str(),
            }))?,
        )?;
        Ok(PromotedSessionDto {
            epic_id,
            quick_session_id,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if existing.is_some() {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: epic.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }
    fn preview_roster_upgrade(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &RosterUpgradePreviewRequest,
    ) -> Result<RosterUpgradePreviewDto, ApiError> {
        let state = self.state()?;
        let current = self.frozen_roster(project_id, epic_id)?;
        // The target the caller named, not whichever revision happens to be
        // current. An upgrade that silently retargeted itself would move a
        // running epic onto a roster nobody looked at.
        let target = self.published_core_team(project_id, request.target.version)?;
        let effects = roster_upgrade_effects(&current, &target)
            .map_err(|error| self.refuse_domain(&error))?;
        Ok(RosterUpgradePreviewDto {
            realm_id: state.realm_id(),
            epic_id,
            preview_hash: self.roster_upgrade_hash(project_id, epic_id, &target, &effects)?,
            effects,
        })
    }
    async fn apply_roster_upgrade(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &TopologyUpgradeApplyRequest,
    ) -> Result<CoreTeamOutcomeDto, ApiError> {
        let state = self.state()?;
        let epic = self.epic_row(project_id, epic_id)?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "epic_roster_upgrade",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "preview": request.preview_hash.as_str(),
        }))?;
        let replayed = self
            .replayed(
                key,
                &intent,
                Some(&AggregateRef::MiniProject {
                    mini_project_id: epic_id,
                }),
            )?
            .is_some();

        let roster = if replayed {
            self.frozen_roster(project_id, epic_id)?
        } else {
            let current = self.frozen_roster(project_id, epic_id)?;
            if current.revision_of_epic != request.expected_revision {
                return Err(self
                    .deny(
                        ApiErrorCode::RevisionConflict,
                        "the epic's roster moved since the caller previewed it",
                    )
                    .with_revision(Some(current.revision_of_epic)));
            }
            // The target is recovered from the preview digest rather than
            // remembered, exactly as a topology upgrade does it: what the
            // caller authorized is a diff, named by its hash.
            let target =
                self.target_of_roster_preview(project_id, epic_id, &request.preview_hash)?;
            let now = kontor_api::now();
            // The pin moves before the seats are materialized, so a failure in
            // between leaves the epic pinned to the target with some of its new
            // seats missing. That is the safe half to lose: materialization is
            // additive and reconciles by role slot, so re-running it finishes
            // the job and never disturbs a seat already held.
            //
            // The cost is a confusing refusal. `put_epic_roster` bumps the
            // roster revision, so the caller's own failed attempt moves it, and
            // their next try is refused with "the epic's roster moved since the
            // caller previewed it" — naming an edit that was theirs. Re-reading
            // the epic and previewing again clears it.
            self.freeze_roster(project_id, epic_id, &target, current.quick_session_id, now)?;
            let control = self.ensure_scope_chain(
                project_id,
                &self.resolve_scope(
                    project_id,
                    &SemanticTopologyTargetDto::EpicControl { epic_id },
                )?,
            )?;
            // Additions only. A role that left the project's roster is not
            // retired here: an agent is attached to that seat, and silently
            // closing it is not an upgrade, it is a dismissal.
            self.materialize_roster_seats(project_id, &control, &target, now)?;
            self.frozen_roster(project_id, epic_id)?
        };

        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::UpgradeEpicRoster,
            AggregateRef::MiniProject {
                mini_project_id: epic_id,
            },
            epic.revision,
            &intent,
        )?;
        Ok(CoreTeamOutcomeDto {
            core_team: self.epic_core_team_dto(project_id, epic_id, &roster)?,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: epic.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }
    fn advisor_profiles(&self, _project_id: ProjectId) -> Result<ProfileCatalogDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Advisor service is not composed in this build",
        ))
    }
    fn preview_advisor_profile(
        &self,
        _project_id: ProjectId,
        _request: &ProfilePreviewRequest,
    ) -> Result<ProfilePreviewDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Advisor service is not composed in this build",
        ))
    }
    async fn apply_advisor_profile(
        &self,
        _key: &IdempotencyKey,
        _project_id: ProjectId,
        _request: &ProfileApplyRequest,
    ) -> Result<AppliedProfileDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Advisor service is not composed in this build",
        ))
    }
    async fn invoke_advisor_run(
        &self,
        _key: &IdempotencyKey,
        _project_id: ProjectId,
        _epic_id: MiniProjectId,
        _request: &InvokeConsultationRequest,
    ) -> Result<AdvisorRunDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Advisor service is not composed in this build",
        ))
    }
    async fn settle_advisor_run(
        &self,
        _key: &IdempotencyKey,
        _project_id: ProjectId,
        _advisor_run_id: AdvisorRunId,
        _request: &SettleConsultationRequest,
    ) -> Result<AdvisorRunDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Advisor service is not composed in this build",
        ))
    }
    fn committee_templates(&self, _project_id: ProjectId) -> Result<ProfileCatalogDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Committee service is not composed in this build",
        ))
    }
    fn preview_committee_template(
        &self,
        _project_id: ProjectId,
        _request: &ProfilePreviewRequest,
    ) -> Result<ProfilePreviewDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Committee service is not composed in this build",
        ))
    }
    async fn apply_committee_template(
        &self,
        _key: &IdempotencyKey,
        _project_id: ProjectId,
        _request: &ProfileApplyRequest,
    ) -> Result<AppliedProfileDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Committee service is not composed in this build",
        ))
    }
    async fn invoke_committee_run(
        &self,
        _key: &IdempotencyKey,
        _project_id: ProjectId,
        _epic_id: MiniProjectId,
        _request: &InvokeConsultationRequest,
    ) -> Result<CommitteeRunDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Committee service is not composed in this build",
        ))
    }
    async fn record_committee_findings(
        &self,
        _key: &IdempotencyKey,
        _project_id: ProjectId,
        _committee_run_id: CommitteeRunId,
        _request: &RecordFindingsRequest,
    ) -> Result<CommitteeRunDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Committee service is not composed in this build",
        ))
    }
    async fn settle_committee_run(
        &self,
        _key: &IdempotencyKey,
        _project_id: ProjectId,
        _committee_run_id: CommitteeRunId,
        _request: &SettleConsultationRequest,
    ) -> Result<CommitteeRunDto, ApiError> {
        Err(self.deny(
            ApiErrorCode::Unavailable,
            "the Committee service is not composed in this build",
        ))
    }
    fn completion_profiles(&self, project_id: ProjectId) -> Result<ProfileCatalogDto, ApiError> {
        let state = self.state()?;
        self.project_row(project_id)?;
        let (revisions, revision) = self.completion_catalog(project_id)?;
        Ok(ProfileCatalogDto {
            realm_id: state.realm_id(),
            project_id,
            revisions,
            revision,
            snapshot_cursor: self.cursor()?,
        })
    }

    fn preview_completion_profile(
        &self,
        project_id: ProjectId,
        request: &ProfilePreviewRequest,
    ) -> Result<ProfilePreviewDto, ApiError> {
        let state = self.state()?;
        self.project_row(project_id)?;
        // Violations are returned, not raised: preview exists to tell a caller
        // what is wrong with a definition, and a refusal would leave it guessing.
        // A definition that cannot even be decoded has no compiled hash to
        // answer with, so that one does refuse.
        let compiled = self.compile_completion_definition(&request.definition)?;
        let mut violations = Vec::new();
        if self
            .completion_catalog(project_id)?
            .0
            .iter()
            .any(|published| {
                published.id == compiled.profile.id.as_str()
                    && published.version == compiled.profile.version
            })
        {
            violations.push("a revision with this id and version is already published".to_owned());
        }
        Ok(ProfilePreviewDto {
            realm_id: state.realm_id(),
            violations,
            preview_hash: compiled.definition_hash,
        })
    }

    async fn apply_completion_profile(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &ProfileApplyRequest,
    ) -> Result<AppliedProfileDto, ApiError> {
        let state = self.state()?;
        let project = self.project_row(project_id)?;
        let now = kontor_api::now();
        // Recompiled from the definition the caller is publishing, then compared
        // with the hash they were answered with. This is what stops an apply from
        // publishing bytes the preview never judged.
        let compiled = self.compile_completion_definition(&request.definition)?;
        if compiled.definition_hash != request.preview_hash {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the definition does not compile to the hash the preview answered with",
            ));
        }
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "completion_profiles_apply",
            "profile": compiled.profile.id.as_str(),
            "version": compiled.profile.version.get(),
            "definition_hash": compiled.definition_hash.as_str(),
        }))?;
        // The key is judged before the revision is. A replay's effect already
        // happened, under the revision that was current *then*; holding the
        // retry to today's revision would make a lost acknowledgement
        // permanently unrecoverable, which is the one failure an idempotency key
        // exists to prevent.
        let replayed = self.replayed(key, &intent, None)?.is_some();
        if !replayed {
            let (published, revision) = self.completion_catalog(project_id)?;
            if revision != request.expected_revision {
                return Err(self
                    .deny(
                        ApiErrorCode::RevisionConflict,
                        "the completion profile catalog moved since the caller read it",
                    )
                    .with_revision(Some(revision)));
            }
            if published.iter().any(|row| {
                row.id == compiled.profile.id.as_str() && row.version == compiled.profile.version
            }) {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "a revision with this id and version is already published",
                ));
            }
            self.state()?
                .with_store(|store| {
                    store.publish_completion_profile(&StoredCompletionProfile {
                        project_id,
                        id: compiled.profile.id.clone(),
                        version: compiled.profile.version,
                        name: compiled.profile.name.clone(),
                        definition: request.definition.clone(),
                        definition_hash: compiled.definition_hash.clone(),
                        published_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::ApplyCompletionProfile,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        Ok(AppliedProfileDto {
            published: Self::completion_profile_dto(&compiled),
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                // Re-read rather than incremented: the catalog's revision is how
                // many publications it holds, and a replay wrote none.
                revision: self.completion_catalog(project_id)?.1,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    fn completion(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<CompletionStateDto, ApiError> {
        self.epic_row(project_id, epic_id)?;
        let stored = self.require_completion(project_id, epic_id)?;
        let compiled = self.pinned_completion(&stored)?;
        self.completion_dto(&stored, &compiled)
    }

    async fn advance_completion(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &AdvanceCompletionRequest,
    ) -> Result<CompletionOutcomeDto, ApiError> {
        let state = self.state()?;
        let epic = self.epic_row(project_id, epic_id)?;
        let now = kontor_api::now();
        // Read first, create nothing. A first advance does start the run, but
        // only once this call has earned the right to: see the guard below.
        let existing = state
            .with_store(|store| store.get_epic_completion(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?;
        // Keyed by the revision the *caller* named, not by the one standing now:
        // a retry after a lost acknowledgement presents the same key and the same
        // expected revision, and that pair has to reproduce the same canonical
        // intent for the replay to be recognized at all.
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "completion_advance",
            "epic": epic_id.to_string(),
            "from_revision": request.expected_revision.get(),
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        if self.replayed(key, &intent, Some(&target))?.is_some() {
            // A recorded advance receipt always has a run behind it, because the
            // receipt is written after the transition commits. An epic that has
            // one and no run is a corrupt ledger, not a replay to satisfy.
            let stored = existing.ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "an advance receipt stands for an epic that has no completion run",
                )
            })?;
            let compiled = self.pinned_completion(&stored)?;
            let receipt_id = self.record(
                key,
                project_id,
                CommandKind::AdvanceCompletion,
                target,
                epic.revision,
                &intent,
            )?;
            // No observation is derived on a replay. The effect already
            // happened, and re-deriving it could refuse for a reason that has
            // nothing to do with this call — an integration outcome that has
            // since become unobservable, say.
            return Ok(CompletionOutcomeDto {
                state: self.completion_dto(&stored, &compiled)?,
                receipt: MutationReceiptDto {
                    realm_id: state.realm_id(),
                    receipt_id: receipt_id.to_string(),
                    applied: AppliedDto::Unchanged,
                    revision: stored.revision,
                    snapshot_cursor: self.cursor()?,
                },
            });
        }
        // The revision is judged before anything durable exists. Starting the run
        // first and guarding afterwards would let a refused call pin the epic's
        // profile and create its run, with no receipt naming the write it had just
        // performed — the ledger has to stay total over durable state.
        match &existing {
            Some(stored) => {
                let current = self.completion_state(stored)?;
                if current.revision != request.expected_revision {
                    return Err(self
                        .deny(
                            ApiErrorCode::RevisionConflict,
                            "the completion run moved since the caller read it",
                        )
                        .with_revision(Some(current.revision)));
                }
            }
            None => {
                // There was nothing to read: the read route answers `404` until
                // this call creates the row. So the only revision a first advance
                // may present is the initial one, and saying *that* is honest
                // where "it moved since you read it" would describe a race that
                // could not have happened.
                if request.expected_revision != AggregateRevision::INITIAL {
                    return Err(self
                        .deny(
                            ApiErrorCode::RevisionConflict,
                            "this epic has no completion run yet, so a first advance must \
                             present the initial revision",
                        )
                        .with_revision(Some(AggregateRevision::INITIAL)));
                }
            }
        }
        // Authorized, so the run may now come into existence. A start that is
        // followed by a refusing transition leaves the run standing: it is
        // deterministic initialization of the epic's own declared contract, it is
        // re-derived identically on the next call, and the command receipt still
        // covers only the transition that actually committed.
        let (stored, compiled) = match existing {
            Some(stored) => {
                let compiled = self.pinned_completion(&stored)?;
                (stored, compiled)
            }
            None => self.start_completion(project_id, epic_id, now)?,
        };
        let current = self.completion_state(&stored)?;
        let observation = self.observe_completion(&stored, &current)?;
        // The signal id is the canonical intent's digest, so the same observation
        // presented twice is the same signal and the pure machine answers
        // `replayed` rather than transitioning again.
        let signal = CompletionSignal {
            id: intent.hash().clone(),
            expected_revision: current.revision,
            delivery: SignalDelivery::Callback,
            observation,
        };
        let transition = kontor_scheduler::advance(&compiled, &current, &signal)
            .map_err(|error| self.refuse_domain(&error))?;
        let next = self.commit_completion(&stored, &transition, "completion_advanced", now)?;
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::AdvanceCompletion,
            target,
            epic.revision,
            &intent,
        )?;
        Ok(CompletionOutcomeDto {
            state: self.completion_dto(&next, &compiled)?,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                // The key was not a replay — the early return above took that
                // path — so only the pure machine can still report one, when the
                // same observation digest had already been applied.
                applied: if transition.replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: next.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    async fn remediate_completion(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &RemediateCompletionRequest,
    ) -> Result<CompletionOutcomeDto, ApiError> {
        let state = self.state()?;
        let epic = self.epic_row(project_id, epic_id)?;
        let now = kontor_api::now();
        let stored = self.require_completion(project_id, epic_id)?;
        let compiled = self.pinned_completion(&stored)?;
        let current = self.completion_state(&stored)?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        // The canonical intent describes what the caller asked for, and nothing
        // the server looked up. That is what lets it be rebuilt on a retry
        // without first reaching the state the original call has since moved.
        let intent = match &request.action {
            RemediationActionDto::LsaProposal {
                round, proposal, ..
            } => self.intent(&serde_json::json!({
                "schema_version": 1,
                "operation": "completion_remediate_propose",
                "epic": epic_id.to_string(),
                "round": round,
                "proposal": proposal.as_str(),
            }))?,
            RemediationActionDto::TpmRoute { round, route } => self.intent(&serde_json::json!({
                "schema_version": 1,
                "operation": "completion_remediate_route",
                "epic": epic_id.to_string(),
                "round": round,
                "route": route.as_str(),
            }))?,
        };
        // Every guard below is judged only when this is not a replay. A route
        // that already committed left the run in `remediation`, so its retry
        // presents both a stale revision and a phase that is no longer awaiting
        // one — and refusing it on either would make a lost acknowledgement
        // unrecoverable, which is the whole point of the key.
        if self.replayed(key, &intent, Some(&target))?.is_some() {
            let receipt_id = self.record(
                key,
                project_id,
                CommandKind::RemediateCompletion,
                target,
                epic.revision,
                &intent,
            )?;
            return Ok(CompletionOutcomeDto {
                state: self.completion_dto(&stored, &compiled)?,
                receipt: MutationReceiptDto {
                    realm_id: state.realm_id(),
                    receipt_id: receipt_id.to_string(),
                    applied: AppliedDto::Unchanged,
                    revision: stored.revision,
                    snapshot_cursor: self.cursor()?,
                },
            });
        }
        if current.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the completion run moved since the caller read it",
                )
                .with_revision(Some(current.revision)));
        }
        let CompletionPhase::AwaitRemediation(awaiting) = current.phase else {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this completion is not waiting for a remediation authority",
            ));
        };

        match &request.action {
            RemediationActionDto::LsaProposal {
                round,
                failed_round_evidence,
                proposal,
            } => {
                if *round != awaiting {
                    return Err(self.deny(
                        ApiErrorCode::InvalidRequest,
                        "the proposal names a round this completion is not waiting on",
                    ));
                }
                // The proposal must answer the round's own immutable evidence.
                // Without this a proposal filed against the right round number
                // could carry another round's findings and still be routed.
                let failed = current
                    .rounds
                    .iter()
                    .find(|recorded| recorded.round == *round)
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::InvalidRequest,
                            "this completion has no recorded round with that number",
                        )
                    })?;
                if failed.evidence != *failed_round_evidence {
                    return Err(self.deny(
                        ApiErrorCode::InvalidRequest,
                        "the proposal does not name the failed round's own evidence",
                    ));
                }
                let lsa = self.epic_control_seat(project_id, epic_id, MANDATORY_LEAD_ROLE)?;
                self.state()?
                    .with_store(|store| {
                        store.insert_remediation_proposal(&StoredRemediationProposal {
                            project_id,
                            mini_project_id: epic_id,
                            round: *round,
                            failed_round_evidence: failed_round_evidence.clone(),
                            proposal: proposal.clone(),
                            lsa_seat_binding_id: lsa,
                            proposed_at: now,
                        })
                    })
                    .map_err(|error| self.refuse(&error))?;
                let receipt_id = self.record(
                    key,
                    project_id,
                    CommandKind::RemediateCompletion,
                    target,
                    epic.revision,
                    &intent,
                )?;
                // The phase does not move on a proposal alone: no remediation
                // launches until the TPM has routed it, so the run stays where it
                // is and the recorded proposal is the only thing that changed.
                Ok(CompletionOutcomeDto {
                    state: self.completion_dto(&stored, &compiled)?,
                    receipt: MutationReceiptDto {
                        realm_id: state.realm_id(),
                        receipt_id: receipt_id.to_string(),
                        applied: AppliedDto::Created,
                        revision: stored.revision,
                        snapshot_cursor: self.cursor()?,
                    },
                })
            }
            RemediationActionDto::TpmRoute { round, route } => {
                if *round != awaiting {
                    return Err(self.deny(
                        ApiErrorCode::InvalidRequest,
                        "the route names a round this completion is not waiting on",
                    ));
                }
                let proposal = self
                    .state()?
                    .with_store(|store| store.get_remediation_proposal(project_id, epic_id, *round))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::InvalidRequest,
                            "no LSA proposal stands for this round, so there is nothing to route",
                        )
                    })?;
                // Both authorities are re-resolved against the live control
                // plane. A proposal made by a seat that has since been replaced
                // is not routable: the authority that approved the correction no
                // longer exists.
                let lsa = self.epic_control_seat(project_id, epic_id, MANDATORY_LEAD_ROLE)?;
                if lsa != proposal.lsa_seat_binding_id {
                    return Err(self.deny(
                        ApiErrorCode::StaleBinding,
                        "the LSA seat that proposed this correction has been replaced",
                    ));
                }
                self.epic_control_seat(project_id, epic_id, MANDATORY_PROGRAM_ROLE)?;
                let signal = CompletionSignal {
                    id: intent.hash().clone(),
                    expected_revision: current.revision,
                    delivery: SignalDelivery::Callback,
                    observation: CompletionObservation::RemediationApproved(RemediationApproval {
                        round: *round,
                        authorization: RemediationAuthorization {
                            lsa_proposal: proposal.proposal.clone(),
                            tpm_routing: route.clone(),
                        },
                    }),
                };
                let transition = kontor_scheduler::advance(&compiled, &current, &signal)
                    .map_err(|error| self.refuse_domain(&error))?;
                let next =
                    self.commit_completion(&stored, &transition, "remediation_routed", now)?;
                let receipt_id = self.record(
                    key,
                    project_id,
                    CommandKind::RemediateCompletion,
                    target,
                    epic.revision,
                    &intent,
                )?;
                Ok(CompletionOutcomeDto {
                    state: self.completion_dto(&next, &compiled)?,
                    receipt: MutationReceiptDto {
                        realm_id: state.realm_id(),
                        receipt_id: receipt_id.to_string(),
                        applied: if transition.replayed {
                            AppliedDto::Unchanged
                        } else {
                            AppliedDto::Created
                        },
                        revision: next.revision,
                        snapshot_cursor: self.cursor()?,
                    },
                })
            }
        }
    }

    async fn apply_epic(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &ApplyEpicRequest,
    ) -> Result<AppliedEpicDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let project = state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such project exists in this realm",
                )
            })?;
        if project.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the project moved since the caller read it",
                )
                .with_revision(Some(project.revision)));
        }
        if state.runtimes().get(&request.runtime_family).is_none() {
            return Err(self.deny(
                ApiErrorCode::UnsupportedCapability,
                "this realm is not configured with the requested runtime family",
            ));
        }
        if let Some(account) = request.account_profile_id {
            state
                .with_store(|store| store.get_account_profile(project_id, account))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "the selected account profile does not exist in this project",
                    )
                })?;
        }

        let bundle = self.bundle(&request.work_profile_category, now)?;
        // A caller that pinned a team revision pinned it against a catalog read.
        // If the profile now pins another, the selection it authorized no longer
        // exists, and applying the current one would substitute a team closure it
        // never saw.
        if let Some(pinned) = &request.team_template {
            let matches = bundle.team.as_ref().is_some_and(|team| {
                team.template_id.to_string() == pinned.id && team.version == pinned.version
            });
            if !matches {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the profile does not pin the team revision the caller selected",
                ));
            }
        }

        // The key is judged before the graph is written. `apply_epic` is atomic,
        // so a conflict discovered afterwards would have to be reported against a
        // graph this call had already created.
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "epics_apply",
            "epic": request.name.as_str(),
            "work_profile_category": request.work_profile_category,
            "runtime_family": request.runtime_family.as_str(),
            "tasks": request
                .tasks
                .iter()
                .map(|task| {
                    serde_json::json!({
                        "title": task.title.as_str(),
                        "module": task.module,
                        "depends_on": task
                            .depends_on
                            .iter()
                            .map(ExternalName::as_str)
                            .collect::<Vec<_>>(),
                        "ticket_links": task
                            .ticket_links
                            .iter()
                            .map(|link| {
                                serde_json::json!({
                                    "connector": link.connector,
                                    "external_issue_key": link.external_issue_key,
                                })
                            })
                            .collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>(),
        }))?;
        if let Some(receipt) = self.replayed(key, &intent, None)? {
            let AggregateRef::MiniProject { mini_project_id } = receipt.target else {
                return Err(self.deny(
                    ApiErrorCode::IdempotencyConflict,
                    "the idempotency key was already used for a different operation",
                ));
            };
            return self.applied_epic_replay(project_id, mini_project_id, &bundle);
        }

        let mut tasks = Vec::with_capacity(request.tasks.len());
        for task in &request.tasks {
            let module = task
                .module
                .as_deref()
                .map(ModuleKey::parse)
                .transpose()
                .map_err(|error| self.refuse_domain(&error))?;
            let mut links = Vec::with_capacity(task.ticket_links.len());
            for link in &task.ticket_links {
                links.push(EpicTicketLink {
                    connector: ConnectorKey::parse(&link.connector)
                        .map_err(|error| self.refuse_domain(&error))?,
                    external_issue_key: ExternalId::parse(&link.external_issue_key)
                        .map_err(|error| self.refuse_domain(&error))?,
                });
            }
            // Parsed here, where `WorkspaceRoot` exists, and stored as the
            // `ExternalName` it wraps. `kontor-core` cannot depend on
            // `kontor-runtime`, so the path's rules — absolute, no `.`, no `..`,
            // no repeated separators — are enforced at this boundary and the
            // store holds a value already known to satisfy them.
            let worktree = task
                .worktree
                .as_deref()
                .map(|root| {
                    WorkspaceRoot::parse(root)
                        .map(|root| ExternalName::parse(root.as_str()))
                        .map_err(|error| self.refuse_domain(&error))
                })
                .transpose()?
                .transpose()
                .map_err(|error| self.refuse_domain(&error))?;
            tasks.push(EpicTask {
                title: task.title.clone(),
                module,
                state: TaskState::Ready,
                depends_on: task.depends_on.clone(),
                ticket_links: links,
                worktree,
            });
        }

        // The epic and the admission position it starts at are written under one
        // hold of the store. An epic that existed without a position would be
        // scheduled against a window started fresh on every pass, which is the
        // very state this ticket is removing — so the position is part of
        // applying an epic, not something a later call remembers to add.
        let seed = AdaptivePosition::initial(self.capacity.adaptive);
        let applied = state
            .with_store(|store| {
                let applied = store.apply_epic(&EpicApplication {
                    project_id,
                    name: request.name.clone(),
                    tasks: &tasks,
                    profile: &bundle.profile,
                    definition: &bundle.profile.definition,
                    team: bundle.team.as_ref(),
                    applied_at: now,
                })?;
                // Seeded once, when the epic first exists. Applying the same
                // epic again is idempotent and must stay that way — and it must
                // not reset a position later observations have already moved.
                if store
                    .get_adaptive_admission_state(project_id, applied.mini_project_id)?
                    .is_none()
                {
                    store.create_adaptive_admission_state(&NewAdaptiveAdmissionState {
                        project_id,
                        mini_project_id: applied.mini_project_id,
                        current_window: seed.current_window,
                        clean_observation_streak: seed.clean_observation_streak,
                        last_observation_id: seed.last_observation_id.clone(),
                        created_at: now,
                    })?;
                }
                Ok::<_, RepositoryError>(applied)
            })
            .map_err(|error| self.refuse(&error))?;

        // The receipt is recorded *after* the graph, because the goal it targets
        // has to exist for the target reference to resolve. It is what makes the
        // apply attributable, and what makes a reused key with different bytes a
        // conflict rather than a second epic.
        self.record(
            key,
            project_id,
            CommandKind::ApplyEpicGraph,
            AggregateRef::MiniProject {
                mini_project_id: applied.mini_project_id,
            },
            applied.revision,
            &intent,
        )?;

        // Digested from what was *stored*, not from what resolved it. The
        // resolved bundle's own digest covers `resolved_at`, so returning it here
        // made an unchanged reapply look like drift on every replay.
        self.sealed(AppliedEpicDto {
            realm_id: state.realm_id(),
            project_id,
            epic_id: applied.mini_project_id,
            applied: applied_dto(applied.applied),
            revision: applied.revision,
            work_profile: RevisionRefDto {
                id: applied.profile.0.as_str().to_owned(),
                version: applied.profile.1,
            },
            team_template: applied.team.map(|(id, version)| RevisionRefDto {
                id: id.to_string(),
                version,
            }),
            bundle_hash: String::new(),
            tasks: applied
                .tasks
                .into_iter()
                .map(|task| AppliedTaskDto {
                    title: task.title,
                    task_id: task.task_id,
                    applied: applied_dto(task.applied),
                    state: task.state.as_str().to_owned(),
                    revision: task.revision,
                    workflow_id: task.workflow_id.to_string(),
                    depends_on: task.depends_on.into_iter().collect(),
                    worktree: task.worktree,
                    links: task
                        .links
                        .into_iter()
                        .map(|link| AppliedLinkDto {
                            link_id: link.id.to_string(),
                            connector: link.connector.as_str().to_owned(),
                            external_issue_key: link.external_issue_key.as_str().to_owned(),
                            applied: applied_dto(link.applied),
                        })
                        .collect(),
                })
                .collect(),
        })
    }

    fn read_epic(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<EpicProjectionDto, ApiError> {
        let state = self.state()?;
        let epic = self.epic_row(project_id, epic_id)?;
        let tasks = state
            .with_store(|store| store.list_epic_tasks(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?;
        let edges = state
            .with_store(|store| store.task_dependency_graph(project_id))
            .map_err(|error| self.refuse(&error))?;
        let authorizations = state
            .with_store(|store| store.list_authorizations(project_id))
            .map_err(|error| self.refuse(&error))?;

        let mut profile = None;
        let mut team = None;
        let mut projected = Vec::with_capacity(tasks.len());
        let mut cursor: Option<kontor_core::id::EventCursor> = None;
        for task in &tasks {
            let snapshot = state
                .with_store(|store| store.snapshot_task_inspection(project_id, task.id))
                .map_err(|error| self.refuse(&error))?;
            cursor = Some(cursor.map_or(snapshot.snapshot_cursor, |held| {
                held.max(snapshot.snapshot_cursor)
            }));
            let inspection = snapshot
                .open(state.realm_id())
                .map_err(|error| self.refuse_domain(&error))?;
            let (phase, workflow_revision, gates, required_artifacts) =
                inspection.as_ref().map_or_else(
                    || (None, None, Vec::new(), Vec::new()),
                    |inspection| {
                        if let Some(workflow) = &inspection.workflow {
                            profile.get_or_insert(RevisionRefDto {
                                id: workflow.snapshot.definition.id.as_str().to_owned(),
                                version: workflow.snapshot.definition.version,
                            });
                            if let Some(reference) = &workflow.snapshot.definition.team_template {
                                team.get_or_insert(RevisionRefDto {
                                    id: reference.template_id.to_string(),
                                    version: reference.version,
                                });
                            }
                        }
                        let Some(workflow) = inspection.workflow.as_ref() else {
                            return (None, None, Vec::new(), Vec::new());
                        };
                        let definition = &workflow.snapshot.definition;
                        // Every gate the pinned profile *declares*, with the authority
                        // and the evidence it declares alongside it, at whatever state
                        // has actually been reduced. A projection listing only the
                        // gates something already recorded a verdict against would tell
                        // a Lead that a task with no evidence has no obligations.
                        let gates: Vec<GateProjectionDto> = definition
                            .gates
                            .iter()
                            .map(|gate| GateProjectionDto {
                                gate: gate.id.as_str().to_owned(),
                                phase: gate.phase.as_str().to_owned(),
                                state: inspection
                                    .gates
                                    .get(&gate.id)
                                    .copied()
                                    .unwrap_or(kontor_core::state::GateState::NotReady)
                                    .as_str()
                                    .to_owned(),
                                evaluator_roles: gate
                                    .evaluator_roles
                                    .iter()
                                    .map(|role| role.as_str().to_owned())
                                    .collect(),
                                required_evidence: gate
                                    .required_evidence
                                    .iter()
                                    .map(|artifact| artifact.as_str().to_owned())
                                    .collect(),
                                waiver_allowed: gate.waiver_allowed,
                                waiver_roles: gate
                                    .waiver_roles
                                    .iter()
                                    .map(|role| role.as_str().to_owned())
                                    .collect(),
                            })
                            .collect();
                        // Every artifact the profile requires anywhere: its phases' own
                        // outputs and the evidence its gates cite. It is what a
                        // completion has to be able to produce, so it is reported rather
                        // than left to be re-derived from the pack out of band.
                        let mut needed: BTreeSet<String> = definition
                            .phases
                            .iter()
                            .flat_map(|phase| &phase.required_artifacts)
                            .map(|artifact| artifact.as_str().to_owned())
                            .collect();
                        needed.extend(
                            definition
                                .gates
                                .iter()
                                .flat_map(|gate| &gate.required_evidence)
                                .map(|artifact| artifact.as_str().to_owned()),
                        );
                        (
                            Some(workflow.current_phase.as_str().to_owned()),
                            Some(workflow.revision),
                            gates,
                            needed.into_iter().collect(),
                        )
                    },
                );
            let links = state
                .with_store(|store| store.list_task_ticket_links(project_id, task.id))
                .map_err(|error| self.refuse(&error))?;
            let team_runs = state
                .with_store(|store| store.list_team_runs_for_task(project_id, task.id))
                .map_err(|error| self.refuse(&error))?;
            let mut runs = Vec::with_capacity(team_runs.len());
            for (team_run_id, lifecycle) in team_runs {
                let seats = state
                    .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
                    .map_err(|error| self.refuse(&error))?;
                runs.push(TeamRunProjectionDto {
                    team_run_id: team_run_id.to_string(),
                    lifecycle: lifecycle.as_str().to_owned(),
                    seats: seats
                        .into_iter()
                        .map(|seat| SeatProjectionDto {
                            role_slot: seat.role.as_str().to_owned(),
                            agent_run_id: seat.agent_run_id.to_string(),
                            runtime_kind: seat.runtime_kind.map(|kind| kind.as_str().to_owned()),
                            native_id: seat.native_id.map(|id| id.as_str().to_owned()),
                            attached: seat
                                .binding_id
                                .is_some_and(|id| state.sessions().get(id).is_some()),
                        })
                        .collect(),
                });
            }
            let worktree = state
                .with_store(|store| store.task_worktree(project_id, task.id))
                .map_err(|error| self.refuse(&error))?;
            projected.push(EpicTaskProjectionDto {
                task_id: task.id,
                title: task.title.clone(),
                worktree,
                state: task.state.as_str().to_owned(),
                revision: task.revision,
                module: task.module.as_ref().map(|key| key.as_str().to_owned()),
                depends_on: edges
                    .get(&task.id)
                    .map(|set| set.iter().copied().collect())
                    .unwrap_or_default(),
                current_phase: phase,
                workflow_revision,
                gates,
                required_artifacts,
                links: links
                    .into_iter()
                    .map(|link| AppliedLinkDto {
                        link_id: link.id.to_string(),
                        connector: link.connector.as_str().to_owned(),
                        external_issue_key: link.external_issue_key.as_str().to_owned(),
                        applied: AppliedDto::Unchanged,
                    })
                    .collect(),
                team_runs: runs,
            });
        }

        Ok(EpicProjectionDto {
            realm_id: state.realm_id(),
            snapshot_cursor: match cursor {
                Some(cursor) => cursor,
                // An epic with no task has no task snapshot to take a position
                // from, so the position is the Realm's own newest: a subscriber
                // resuming strictly after it still sees everything that follows.
                None => {
                    state
                        .with_store(|store| store.realm_event_page(None, 1))
                        .map_err(|error| self.refuse(&error))?
                        .newest
                        .cursor
                }
            },
            project_id,
            epic_id,
            name: epic.name,
            revision: epic.revision,
            work_profile: profile,
            team_template: team,
            tasks: projected,
            authorizations: authorizations
                .iter()
                .filter(|stored| {
                    stored.authorization.scope.covers(Some(epic_id), None)
                        || matches!(stored.authorization.scope, WorkScope::Project)
                })
                .map(authorization_dto)
                .collect(),
            scheduling_open: state.barrier().state().is_open(),
        })
    }

    async fn arm(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &ArmRequest,
    ) -> Result<AuthorizationProjectionDto, ApiError> {
        let state = self.state()?;
        let epic = self.epic_row(project_id, epic_id)?;
        if epic.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the epic moved since the caller read it",
                )
                .with_revision(Some(epic.revision)));
        }
        // Every named task has to be in *this* epic. A scope-wide arm that
        // silently included a task from another goal would arm work the caller
        // never looked at.
        let owned: BTreeSet<TaskId> = state
            .with_store(|store| store.list_epic_tasks(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .map(|task| task.id)
            .collect();
        for task in &request.tasks {
            if !owned.contains(task) {
                return Err(self.deny(
                    ApiErrorCode::NotFound,
                    "a named task does not belong to this epic",
                ));
            }
        }
        state
            .with_store(|store| store.get_account_profile(project_id, request.granted_by))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the granting account profile does not exist in this project",
                )
            })?;

        // The whole bounded grant is in the intent, so a second call under the
        // same key with a wider window, a bigger budget or a different scope is a
        // conflict rather than a replay of the narrow one.
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "execution_arm",
            "tasks": request.tasks.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "allowed_start": request.allowed_start.to_string(),
            "allowed_end": request.allowed_end.to_string(),
            "max_concurrency": request.max_concurrency,
            "max_tokens": request.budget.max_tokens,
            "max_commands": request.budget.max_commands,
            "max_duration_seconds": request.budget.max_duration_seconds,
            "max_cost_minor_units": request.budget.max_cost_minor_units,
            "cost_currency": request.budget.cost_currency,
            "granted_by": request.granted_by.to_string(),
            "reason": request.reason.as_str(),
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        // A replay answers with the authorization the original call granted rather
        // than granting a second one over the same scope.
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            let granted = state
                .with_store(|store| store.list_authorizations(project_id))
                .map_err(|error| self.refuse(&error))?
                .into_iter()
                .find(|stored| stored.authorization.capability_receipt == receipt.id)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "the replayed receipt names an authorization this realm no longer has",
                    )
                })?;
            return Ok(authorization_dto(&granted));
        }

        let receipt = self.record(
            key,
            project_id,
            CommandKind::AuthorizeExecution,
            target,
            epic.revision,
            &intent,
        )?;

        let authorization = ExecutionAuthorization {
            id: ExecutionAuthorizationId::generate(),
            project_id,
            scope: WorkScope::MiniProject {
                mini_project_id: epic_id,
            },
            selected_tasks: request.tasks.clone(),
            allowed_start: TimeRange {
                start: request.allowed_start,
                end: request.allowed_end,
            },
            max_concurrency: request.max_concurrency,
            budget: kontor_core::spec::BudgetBounds {
                max_tokens: request.budget.max_tokens,
                max_commands: request.budget.max_commands,
                max_duration_seconds: request.budget.max_duration_seconds,
                max_cost: Money {
                    minor_units: request.budget.max_cost_minor_units,
                    currency: CurrencyCode::parse(&request.budget.cost_currency)
                        .map_err(|error| self.refuse_domain(&error))?,
                },
            },
            created_by: request.granted_by,
            capability_receipt: receipt,
            created_at: kontor_api::now(),
        };
        state
            .with_store(|store| store.insert_authorization(&authorization))
            .map_err(|error| self.refuse(&error))?;
        Ok(authorization_dto(&kontor_store::StoredAuthorization {
            authorization,
            revocation: None,
        }))
    }

    async fn disarm(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &DisarmRequest,
    ) -> Result<AuthorizationProjectionDto, ApiError> {
        let state = self.state()?;
        let epic = self.epic_row(project_id, epic_id)?;
        let id = ExecutionAuthorizationId::parse(&request.authorization_id)
            .map_err(|error| self.refuse_domain(&error))?;
        // The key and the body are judged *first*. Answering an already-revoked
        // authorization before that would let a changed request wear a used key
        // and be reported as the replay of something it is not.
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "execution_disarm",
            "authorization_id": request.authorization_id,
            "revoked_by": request.revoked_by.to_string(),
            "reason": request.reason.as_str(),
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        let replay = self.replayed(key, &intent, Some(&target))?.is_some();

        let stored = state
            .with_store(|store| store.list_authorizations(project_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|stored| stored.authorization.id == id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such execution authorization exists in this project",
                )
            })?;
        if !stored.authorization.scope.covers(Some(epic_id), None) {
            return Err(self.deny(
                ApiErrorCode::NotFound,
                "that authorization does not arm this epic",
            ));
        }
        if stored.revocation.is_some() {
            // Already disarmed, and the request that says so has been proved to be
            // the same one. Re-asserting something already true is an answer.
            return Ok(authorization_dto(&stored));
        }
        if replay {
            // The key recorded this disarm, but the authorization is not revoked:
            // the receipt landed and the revocation did not. Finishing it is the
            // convergent answer, so the write below runs.
            tracing::info!(
                authorization = %id,
                "a recorded disarm had not been applied; completing it"
            );
        }

        let receipt = self.record(
            key,
            project_id,
            CommandKind::RevokeExecutionAuthorization,
            target,
            epic.revision,
            &intent,
        )?;
        let revocation = AuthorizationRevocation {
            revoked_at: kontor_api::now(),
            revoked_by: request.revoked_by,
            receipt,
            reason: request.reason.clone(),
        };
        state
            .with_store(|store| store.revoke_authorization(project_id, id, &revocation))
            .map_err(|error| self.refuse(&error))?;
        Ok(authorization_dto(&kontor_store::StoredAuthorization {
            authorization: stored.authorization,
            revocation: Some(revocation),
        }))
    }

    async fn plan(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<SchedulerPlanDto, ApiError> {
        let state = self.state()?;
        self.epic_row(project_id, epic_id)?;
        let snapshot = self.snapshot(project_id, epic_id).await?;
        let plan =
            kontor_scheduler::ready::plan(&snapshot).map_err(|error| self.refuse_domain(&error))?;
        let document = plan_digest(&plan).map_err(|error| self.refuse_domain(&error))?;
        let mut ready = Vec::new();
        let mut blocked = Vec::new();
        let mut authorizations = BTreeSet::new();
        for decision in &plan.decisions {
            match decision {
                CandidateDecision::Admit(admitted) => {
                    authorizations.insert(admitted.authorization_id.to_string());
                    ready.push(ReadyTaskDto {
                        task_id: admitted.task_id,
                        authorization_id: admitted.authorization_id.to_string(),
                        runtime_kind: admitted.runtime_kind.clone(),
                        account_profile_id: admitted.account_profile_id,
                    });
                }
                CandidateDecision::Reject {
                    task_id,
                    code,
                    evidence,
                    ..
                } => blocked.push(BlockedTaskDto {
                    task_id: *task_id,
                    code: code.as_str().to_owned(),
                    evidence: evidence
                        .iter()
                        .map(|item| serde_json::to_value(item).unwrap_or(serde_json::Value::Null))
                        .collect(),
                }),
            }
        }
        Ok(SchedulerPlanDto {
            realm_id: state.realm_id(),
            plan_hash: document.hash().as_str().to_owned(),
            taken_at: plan.taken_at,
            scheduling_open: state.barrier().state().is_open(),
            ready,
            blocked,
            authorizations: authorizations.into_iter().collect(),
        })
    }

    async fn start(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &StartRequest,
    ) -> Result<SchedulerStartDto, ApiError> {
        let state = self.state()?;
        self.epic_row(project_id, epic_id)?;
        if !state.barrier().state().is_open() {
            return Err(self.deny(
                ApiErrorCode::ReconciliationPending,
                "startup reconciliation has not finished, so nothing may be admitted",
            ));
        }
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "scheduler_start",
            "epic_id": epic_id.to_string(),
            "plan_hash": request.plan_hash,
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        let replayed = self.replayed(key, &intent, Some(&target))?.is_some();

        // Admission is durable before the runtime is contacted. If that native
        // call failed, a fresh plan correctly says `task_already_in_flight`; it
        // cannot be the input for recovering the already-recorded launch. An
        // exact command replay therefore resumes only the immutable admissions
        // whose launch keys were derived from this scheduler key.
        if replayed {
            let tasks = state
                .with_store(|store| store.list_epic_tasks(project_id, epic_id))
                .map_err(|error| self.refuse(&error))?;
            let mut admitted = Vec::new();
            for task in tasks {
                let launch_key = IdempotencyKey::parse(&format!("{}-{}", key.as_str(), task.id))
                    .map_err(|error| self.refuse_domain(&error))?;
                if let Some(candidate) = state
                    .with_store(|store| {
                        store.admitted_candidate_by_launch_key(project_id, &launch_key)
                    })
                    .map_err(|error| self.refuse(&error))?
                {
                    admitted.push(candidate);
                }
            }
            if !admitted.is_empty() {
                let mut started = Vec::new();
                let mut blocked = Vec::new();
                for candidate in &admitted {
                    match self.seat(key, project_id, candidate).await {
                        Ok(seats) => started.extend(seats),
                        Err(refusal) => blocked.push(BlockedTaskDto {
                            task_id: candidate.task_id,
                            code: refusal.code.as_str().to_owned(),
                            evidence: vec![serde_json::json!({
                                "kind": "seat",
                                "rule": refusal.rule,
                            })],
                        }),
                    }
                }
                self.mark_started_tasks_in_progress(project_id, &started)?;
                state.signals().appended();
                return Ok(SchedulerStartDto {
                    realm_id: state.realm_id(),
                    plan_hash: request.plan_hash.clone(),
                    started,
                    blocked,
                });
            }
        }
        let snapshot = self.snapshot(project_id, epic_id).await?;
        let plan =
            kontor_scheduler::ready::plan(&snapshot).map_err(|error| self.refuse_domain(&error))?;
        let document = plan_digest(&plan).map_err(|error| self.refuse_domain(&error))?;
        // The plan is re-derived rather than stored, and the hash is what makes
        // that safe: a Realm that moved since the caller looked produces a
        // different set of decisions, and starting it would start a batch nobody
        // authorized.
        if document.hash().as_str() != request.plan_hash {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the named plan no longer describes this realm",
            ));
        }

        // The start itself is a recorded command, so a key reused with a *different*
        // plan is a conflict rather than a second batch. The per-seat launch
        // intents below are derived from this key and are the admission path's own
        // idempotency, which is a different question from this one.
        if !replayed {
            let epic = self.epic_row(project_id, epic_id)?;
            self.record(
                key,
                project_id,
                CommandKind::StartScheduledWork,
                target,
                epic.revision,
                &intent,
            )?;
        }

        let mut started = Vec::new();
        let mut blocked = Vec::new();
        for decision in &plan.decisions {
            match decision {
                CandidateDecision::Admit(admitted) => {
                    match self.seat(key, project_id, admitted).await {
                        Ok(seats) => started.extend(seats),
                        // The rule travels with the refusal: a seat that could not
                        // be created is the one thing a Lead most needs named, and
                        // a bare code would make every such failure look alike.
                        Err(refusal) => blocked.push(BlockedTaskDto {
                            task_id: admitted.task_id,
                            code: refusal.code.as_str().to_owned(),
                            evidence: vec![serde_json::json!({
                                "kind": "seat",
                                "rule": refusal.rule,
                            })],
                        }),
                    }
                }
                CandidateDecision::Reject {
                    task_id,
                    code,
                    evidence,
                    ..
                } => blocked.push(BlockedTaskDto {
                    task_id: *task_id,
                    code: code.as_str().to_owned(),
                    evidence: evidence
                        .iter()
                        .map(|item| serde_json::to_value(item).unwrap_or(serde_json::Value::Null))
                        .collect(),
                }),
            }
        }
        // A task with a live seat is being worked on, and the task's own state has
        // to say so. Admission deliberately requires `ready` and leaves the row
        // alone — it is the same transaction that creates the run, and moving the
        // task there would make the admission check depend on its own write — so
        // the transition belongs here, after the seat exists.
        //
        // Without it a started task stays `ready` forever. That used to be
        // unrecoverable — `ready → done` was not in the transition table, so a
        // started task could never legally be completed. It is now legal, on the
        // same closure certificate `in_progress` needs, so a task that misses
        // this transition is merely mislabelled rather than stuck. Moving it
        // here is still the point: a task being worked on should say so.
        self.mark_started_tasks_in_progress(project_id, &started)?;
        state.signals().appended();
        Ok(SchedulerStartDto {
            realm_id: state.realm_id(),
            plan_hash: request.plan_hash.clone(),
            started,
            blocked,
        })
    }

    async fn lifecycle(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &LifecycleRequest,
    ) -> Result<LifecycleOutcomeDto, ApiError> {
        let state = self.state()?;
        let epic = self.epic_row(project_id, epic_id)?;
        match request.action {
            LifecycleAction::CloseEpic | LifecycleAction::ReopenEpic => {
                self.epic_lifecycle(key, project_id, &epic, request)
            }
            _ => {
                let task_id = request.task_id.ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::InvalidRequest,
                        "this lifecycle action names the task it applies to",
                    )
                })?;
                let task = state
                    .with_store(|store| store.get_task(project_id, task_id))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::NotFound,
                            "no such task exists in this project",
                        )
                    })?;
                if task.mini_project_id != Some(epic_id) {
                    return Err(self.deny(
                        ApiErrorCode::NotFound,
                        "that task does not belong to this epic",
                    ));
                }
                if task.revision != request.expected_revision {
                    return Err(self
                        .deny(
                            ApiErrorCode::RevisionConflict,
                            "the task moved since the caller read it",
                        )
                        .with_revision(Some(task.revision)));
                }
                self.task_lifecycle(key, project_id, &task, request)
            }
        }
    }

    async fn resolve_context(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &ResolveContextRequest,
    ) -> Result<ResolvedContextDto, ApiError> {
        let state = self.state()?;
        let realm_id = state.realm_id();
        let task = self.task_row(project_id, task_id)?;
        let workflow = state
            .with_store(|store| store.get_active_task_workflow(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the task has no active workflow, so there is no context to resolve",
                )
            })?;

        // The layers are built from what the task *is*, not from caller-supplied
        // content: a route that accepted arbitrary context would be a route
        // through which anything could be handed to a run.
        let sources = context_sources(realm_id, &task, &workflow)?;
        let references = kontor_context::model::ReferenceInputs::new();
        let resolution = kontor_context::resolve::ResolutionRequest {
            realm_id,
            sources: &sources,
            references: &references,
        };
        let pack = kontor_context::resolve::preview(&resolution)
            .map_err(|error| self.refuse_domain(&error))?;

        let mut context_pack_id = None;
        let mut agent_run_id = None;
        if request.snapshot {
            // Freezing needs a run to belong to. A pack that belongs to no run is
            // evidence about nothing, so the honest answer is a refusal rather
            // than a pack with an invented binding.
            let seat = self.live_seat(project_id, task_id)?.ok_or_else(|| {
                self.deny(
                    ApiErrorCode::UnsupportedCapability,
                    "a context snapshot belongs to a run, and this task has none",
                )
            })?;
            // The task's declared worktree, not a synthesized one: a frozen
            // context pack that named a directory nobody chose would be evidence
            // about a place the run never worked in.
            let root = self.task_root(project_id, task_id)?;
            let workspace = kontor_context::model::WorkspaceRef {
                root: kontor_core::id::BoundedText::parse(root.as_str())
                    .map_err(|error| self.refuse_domain(&error))?,
                branch: ExternalName::parse("main").map_err(|error| self.refuse_domain(&error))?,
                baseline_commit: ExternalId::parse(pack.hash().as_str())
                    .map_err(|error| self.refuse_domain(&error))?,
            };
            let snapshot = kontor_context::resolve::start_run(
                &resolution,
                kontor_core::id::ContextPackId::generate(),
                kontor_context::model::RunBinding {
                    agent_run_id: seat,
                    workspace,
                    started_at: kontor_api::now(),
                },
            )
            .map_err(|error| self.refuse_domain(&error))?;
            context_pack_id = Some(snapshot.context_pack_id().to_string());
            agent_run_id = Some(seat.to_string());
            let intent = self.intent(&serde_json::json!({
                "schema_version": 1,
                "operation": "context_resolve",
                "task_id": task_id.to_string(),
                "context_hash": snapshot.hash().as_str(),
            }))?;
            let target = AggregateRef::Task { task_id };
            if self.replayed(key, &intent, Some(&target))?.is_none() {
                self.record(
                    key,
                    project_id,
                    CommandKind::ResolveContext,
                    target,
                    task.revision,
                    &intent,
                )?;
            }
        }

        Ok(ResolvedContextDto {
            realm_id,
            task_id,
            context_hash: pack.hash().as_str().to_owned(),
            context_pack_id,
            agent_run_id,
            provenance: pack
                .provenance()
                .iter()
                .map(|entry| ProvenanceDto {
                    path: entry.path.as_str().to_owned(),
                    layer: layer_name(entry.layer).to_owned(),
                    source_id: entry.source_id.clone(),
                    revision: entry.revision,
                })
                .collect(),
            redactions: pack
                .redactions()
                .iter()
                .map(|record| RedactionDto {
                    path: record.path.as_str().to_owned(),
                    source_id: record.source_id.clone(),
                    reason: format!("{:?}", record.reason).to_lowercase(),
                })
                .collect(),
        })
    }

    async fn record_gate(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        gate: &str,
        request: &RecordGateRequest,
    ) -> Result<GateVerdictDto, ApiError> {
        let state = self.state()?;
        let task = self.task_row(project_id, task_id)?;
        let workflow = state
            .with_store(|store| store.get_active_task_workflow(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the task has no active workflow to record a verdict against",
                )
            })?;
        if workflow.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the task's workflow moved since the caller read it",
                )
                .with_revision(Some(workflow.revision)));
        }
        let gate_key = GateKey::parse(gate).map_err(|error| self.refuse_domain(&error))?;
        let verdict =
            GateVerdict::parse(&request.verdict).map_err(|error| self.refuse_domain(&error))?;
        let evaluator_role = kontor_core::id::RoleKey::parse(&request.evaluator_role)
            .map_err(|error| self.refuse_domain(&error))?;
        let evidence: Vec<kontor_core::id::ArtifactKey> = request
            .evidence
            .iter()
            .map(|artifact| kontor_core::id::ArtifactKey::parse(artifact))
            .collect::<Result<_, _>>()
            .map_err(|error| self.refuse_domain(&error))?;
        let reviewer_principal = request
            .reviewer_principal
            .as_deref()
            .map(ExternalId::parse)
            .transpose()
            .map_err(|error| self.refuse_domain(&error))?;
        state
            .with_store(|store| store.get_account_profile(project_id, request.evaluator_account))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the evaluating account profile does not exist in this project",
                )
            })?;

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "gate_record",
            "task_id": task_id.to_string(),
            "gate": gate_key.as_str(),
            "verdict": verdict.as_str(),
            "evaluator_role": evaluator_role.as_str(),
            "evaluator_account": request.evaluator_account.to_string(),
            "evidence": request.evidence,
        }))?;
        let target = AggregateRef::Task { task_id };
        // A replay answers from the append-only history rather than appending a
        // second identical verdict: the history is evidence, and evidence that
        // duplicates itself under a retry is evidence about the retry.
        if self.replayed(key, &intent, Some(&target))?.is_some() {
            return self.gate_verdict_replay(project_id, task_id, &workflow.id, &gate_key, key);
        }
        let receipt = self.record(
            key,
            project_id,
            CommandKind::RecordGateVerdict,
            target,
            task.revision,
            &intent,
        )?;

        // Read *before* the write opens the store: `with_store` takes one
        // process-wide lock, and asking for the seat from inside the closure
        // would be this thread waiting for a lock it is already holding.
        let seat = self.live_seat(project_id, task_id)?;
        let sequence = state
            .with_store(|store| {
                store.append_gate_evaluation(&NewGateEvaluation {
                    project_id,
                    workflow_id: workflow.id,
                    gate: gate_key.clone(),
                    verdict,
                    evaluator_role,
                    evaluator_account: request.evaluator_account,
                    evidence,
                    agent_run_id: seat,
                    reviewer_principal,
                    policy_evaluation_id: None,
                    recorded_at: kontor_api::now(),
                })
            })
            .map_err(|error| self.refuse(&error))?;
        let gates = state
            .with_store(|store| store.gate_states(project_id, workflow.id))
            .map_err(|error| self.refuse(&error))?;
        state.signals().appended();
        Ok(GateVerdictDto {
            realm_id: state.realm_id(),
            task_id,
            gate: gate_key.as_str().to_owned(),
            sequence,
            verdict: verdict.as_str().to_owned(),
            state: gates
                .get(&gate_key)
                .map_or("not_ready", |state| state.as_str())
                .to_owned(),
            receipt_id: receipt.to_string(),
        })
    }

    async fn select_profile(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &SelectionRequest,
    ) -> Result<SelectionDto, ApiError> {
        let state = self.state()?;
        let task = self.task_row(project_id, task_id)?;
        self.ensure_pre_run(project_id, task_id)?;
        if task.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the task moved since the caller read it",
                )
                .with_revision(Some(task.revision)));
        }
        let category = request.work_profile_category.as_deref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "a profile correction names the category to pin",
            )
        })?;
        let bundle = self.bundle(category, kontor_api::now())?;
        let current = state
            .with_store(|store| store.get_active_task_workflow(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        let unchanged = current.as_ref().is_some_and(|workflow| {
            workflow.snapshot.definition.id == bundle.profile.definition.id
                && workflow.snapshot.definition.version == bundle.profile.definition.version
        });

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "profile_selection",
            "task_id": task_id.to_string(),
            "work_profile_category": category,
            "reason": request.reason.as_str(),
        }))?;
        let target = AggregateRef::Task { task_id };
        let receipt = if let Some(existing) = self.replayed(key, &intent, Some(&target))? {
            existing.id
        } else {
            self.record(
                key,
                project_id,
                CommandKind::SelectTaskProfile,
                target,
                task.revision,
                &intent,
            )?
        };
        if !unchanged {
            state
                .with_store(|store| {
                    store.replace_task_workflow(
                        project_id,
                        task_id,
                        &kontor_core::repository::NewTaskWorkflow {
                            id: kontor_core::id::TaskWorkflowId::generate(),
                            project_id,
                            task_id,
                            snapshot: bundle.profile.clone(),
                            current_phase: bundle.profile.definition.entry_phase.clone(),
                            created_at: kontor_api::now(),
                        },
                        &bundle.profile.definition,
                        bundle.team.as_ref(),
                    )
                })
                .map_err(|error| self.refuse(&error))?;
            state.signals().appended();
        }
        Ok(SelectionDto {
            realm_id: state.realm_id(),
            task_id,
            work_profile: Some(RevisionRefDto {
                id: bundle.profile.definition.id.as_str().to_owned(),
                version: bundle.profile.definition.version,
            }),
            team_template: bundle.team.as_ref().map(|team| RevisionRefDto {
                id: team.template_id.to_string(),
                version: team.version,
            }),
            account_profile_id: None,
            applied: if unchanged {
                AppliedDto::Unchanged
            } else {
                AppliedDto::Created
            },
            receipt_id: receipt.to_string(),
        })
    }

    async fn select_team(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &SelectionRequest,
    ) -> Result<SelectionDto, ApiError> {
        let state = self.state()?;
        let task = self.task_row(project_id, task_id)?;
        self.ensure_pre_run(project_id, task_id)?;
        if task.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the task moved since the caller read it",
                )
                .with_revision(Some(task.revision)));
        }
        let workflow = state
            .with_store(|store| store.get_active_task_workflow(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the task pins no work profile, so it prescribes no team",
                )
            })?;
        // A team is not an independently selectable pin in this data model: a work
        // profile *prescribes* one, and a run freezes what the profile prescribed.
        // So the correction this operation can honestly make is to confirm the
        // caller's belief and refuse a mismatch — changing the team means changing
        // the profile, which is what `profile-selection` is for.
        let pinned = workflow
            .snapshot
            .definition
            .team_template
            .as_ref()
            .map(|pin| RevisionRefDto {
                id: pin.template_id.to_string(),
                version: pin.version,
            });
        let requested = request.team_template.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "a team correction names the team revision it expects",
            )
        })?;
        if pinned.as_ref() != Some(requested) {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the task's pinned work profile prescribes a different team revision",
            ));
        }
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "team_selection",
            "task_id": task_id.to_string(),
            "team_template": requested.id,
            "team_version": requested.version.get(),
            "reason": request.reason.as_str(),
        }))?;
        let target = AggregateRef::Task { task_id };
        let receipt = if let Some(existing) = self.replayed(key, &intent, Some(&target))? {
            existing.id
        } else {
            self.record(
                key,
                project_id,
                CommandKind::SelectTaskTeam,
                target,
                task.revision,
                &intent,
            )?
        };
        Ok(SelectionDto {
            realm_id: state.realm_id(),
            task_id,
            work_profile: Some(RevisionRefDto {
                id: workflow.snapshot.definition.id.as_str().to_owned(),
                version: workflow.snapshot.definition.version,
            }),
            team_template: pinned,
            account_profile_id: None,
            applied: AppliedDto::Unchanged,
            receipt_id: receipt.to_string(),
        })
    }

    async fn select_account(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &SelectionRequest,
    ) -> Result<SelectionDto, ApiError> {
        let state = self.state()?;
        let task = self.task_row(project_id, task_id)?;
        self.ensure_pre_run(project_id, task_id)?;
        if task.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the task moved since the caller read it",
                )
                .with_revision(Some(task.revision)));
        }
        let account = request.account_profile_id.ok_or_else(|| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "an account correction names the profile to pin",
            )
        })?;
        let profile = state
            .with_store(|store| store.get_account_profile(project_id, account))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the selected account profile does not exist in this project",
                )
            })?;
        if !profile.enabled {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the selected account profile is disabled and may not be pinned",
            ));
        }
        // The capability check is the runtime's, and it is the whole point of the
        // operation: pinning a run to an account is a claim that the runtime can
        // prove which account it executed as. Paseo 1.0 declares
        // `account_env = false`, so the honest answer there is a typed refusal
        // rather than a pin nothing will honour.
        let adapter = state.runtimes().get(&profile.harness).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "this daemon is not configured with the runtime that account authenticates against",
            )
        })?;
        let capabilities = adapter
            .discover_capabilities()
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let mut context =
            kontor_runtime::capability::OperationContext::new(RuntimeCapability::Launch);
        context.account_pinned = true;
        kontor_runtime::capability::preflight(&capabilities, &context)
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "account_selection",
            "task_id": task_id.to_string(),
            "account_profile_id": account.to_string(),
            "account_revision": profile.revision.get(),
            "reason": request.reason.as_str(),
        }))?;
        let target = AggregateRef::Task { task_id };
        let (receipt, applied) =
            if let Some(existing) = self.replayed(key, &intent, Some(&target))? {
                (existing.id, AppliedDto::Unchanged)
            } else {
                let receipt = self.record(
                    key,
                    project_id,
                    CommandKind::SelectTaskAccount,
                    target,
                    task.revision,
                    &intent,
                )?;
                (receipt, AppliedDto::Created)
            };
        let stored = state
            .with_store(|store| {
                store.set_task_account_selection(project_id, task_id, account, profile.revision)
            })
            .map_err(|error| self.refuse(&error))?;
        state.signals().appended();
        Ok(SelectionDto {
            realm_id: state.realm_id(),
            task_id,
            work_profile: None,
            team_template: None,
            account_profile_id: Some(account),
            applied: if stored == Applied::Created {
                applied
            } else {
                AppliedDto::Unchanged
            },
            receipt_id: receipt.to_string(),
        })
    }

    async fn ticket_reconcile_plan(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<TicketReconcilePlanDto, ApiError> {
        let state = self.state()?;
        let (links, diff, hash) = self.reconcile_projection(project_id, task_id)?;
        Ok(TicketReconcilePlanDto {
            realm_id: state.realm_id(),
            task_id,
            projection_hash: hash,
            links: links.iter().map(ToString::to_string).collect(),
            converged: diff.is_empty(),
            diff,
        })
    }

    async fn ticket_reconcile_apply(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &TicketReconcileApplyRequest,
    ) -> Result<TicketReconcileAppliedDto, ApiError> {
        let state = self.state()?;
        let task = self.task_row(project_id, task_id)?;
        let (links, diff, hash) = self.reconcile_projection(project_id, task_id)?;
        // The plan is re-derived and its digest compared, exactly as a scheduler
        // start re-derives its batch: applying a plan the realm has moved past
        // would converge a ticket towards something nobody looked at.
        if hash != request.projection_hash {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the named reconciliation plan no longer describes this realm",
            ));
        }
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "ticket_reconcile_apply",
            "task_id": task_id.to_string(),
            "projection_hash": request.projection_hash,
        }))?;
        let target = AggregateRef::Task { task_id };
        let receipt = if let Some(existing) = self.replayed(key, &intent, Some(&target))? {
            existing.id
        } else {
            self.record(
                key,
                project_id,
                CommandKind::ReconcileTicket,
                target,
                task.revision,
                &intent,
            )?
        };
        if !diff.is_empty() {
            // Converging a real difference means transitioning a ticket in the
            // external system, and that needs the connector this Realm is
            // configured with. Reporting success without one would be a claim
            // about a system nothing contacted.
            return Err(self.deny(
                ApiErrorCode::Unavailable,
                "this realm is not configured with a connector that can converge that ticket",
            ));
        }
        Ok(TicketReconcileAppliedDto {
            realm_id: state.realm_id(),
            task_id,
            projection_hash: request.projection_hash.clone(),
            converged: links.iter().map(ToString::to_string).collect(),
            receipt_id: receipt.to_string(),
        })
    }

    async fn retry_undelivered_dispatches(&self) -> Result<usize, ApiError> {
        let state = self.state()?;
        let projects = state
            .with_store(SqliteStore::list_projects)
            .map_err(|error| self.refuse(&error))?;
        let now = kontor_api::now();
        let mut delivered = 0;
        for project in &projects {
            let pending: Vec<_> = state
                .with_store(|store| store.list_turn_dispatches(project.project_id))
                .map_err(|error| self.refuse(&error))?
                .into_iter()
                .filter(|row| !row.dispatched)
                .collect();
            for row in pending {
                // A slot waived since the dispatch was derived has no seat to
                // hand anything to, and never will. The row stays undelivered on
                // purpose — it is the durable record that the handoff was decided
                // and then excused — but retrying it forever would be a process
                // waiting on a session that was explicitly declared absent.
                if self.slot_is_waived(project.project_id, row.team_run_id, &row.to_role_slot_id)? {
                    continue;
                }
                // The follow-up already exists as a decision; this only finishes
                // handing it over. Nothing here re-derives, so a restart cannot
                // turn one decision into two effects.
                let Some(settled) = self.settled_turn(project.project_id, row.settled_turn_id)?
                else {
                    continue;
                };
                let Some(handoff) =
                    self.handoff_for(project.project_id, &settled, &row.to_role_slot_id)?
                else {
                    continue;
                };
                let Ok(message_id) = kontor_runtime::request::MessageId::parse(&row.message_id)
                else {
                    continue;
                };
                if self
                    .deliver_follow_up(
                        project.project_id,
                        &settled,
                        &handoff,
                        self.seat_for_slot(
                            project.project_id,
                            row.team_run_id,
                            &row.to_role_slot_id,
                        )?,
                        message_id,
                        now,
                    )
                    .await?
                {
                    delivered += 1;
                }
            }
        }
        Ok(delivered)
    }

    async fn waive_role_slot(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        team_run_id: TeamRunId,
        role_slot: &str,
        request: &WaiveRoleSlotRequest,
    ) -> Result<RoleSlotWaiverDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let role_slot_id =
            RoleSlotId::parse(role_slot).map_err(|error| self.refuse_domain(&error))?;
        if request.evidence.is_empty() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "a waiver must cite the evidence its slot requires",
            ));
        }
        let team = state
            .with_store(|store| store.get_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such team run exists in this project",
                )
            })?;
        // The frozen definition decides whether this slot exists at all, and a
        // slot the pinned revision never declared is simply not addressable.
        if !team
            .snapshot
            .declared_role_slots()
            .map_err(|error| self.refuse_domain(&error))?
            .contains(&role_slot_id)
        {
            return Err(self.deny(
                ApiErrorCode::NotFound,
                "the pinned team template declares no such role slot",
            ));
        }
        let task_id = self.task_for_team_run(project_id, team_run_id)?;

        // Canonical, and deliberately free of anything incidental: no waiver id,
        // no idempotency key, no timestamp. An identical retry hashes identically,
        // which is what makes a replay recognisable as one.
        let mut sorted: Vec<String> = request.evidence.clone();
        sorted.sort();
        sorted.dedup();
        let evidence_hash = self
            .intent(&serde_json::json!({
                "schema_version": 1,
                "operation": "role_slot_waiver",
                "project_id": project_id.to_string(),
                "task_id": task_id.to_string(),
                "team_run_id": team_run_id.to_string(),
                "role_slot_id": role_slot_id.as_str(),
                "team_run_revision": request.expected_team_revision.get(),
                "authorized_role": request.authorized_by_role,
                "authority_tier": "admin",
                "evidence": sorted,
            }))?
            .hash()
            .clone();

        let (stored, applied, _revision) = state
            .with_store(|store| {
                store.waive_role_slot(&kontor_store::NewRoleSlotWaiver {
                    id: kontor_core::id::RoleSlotWaiverId::generate(),
                    project_id,
                    task_id,
                    team_run_id,
                    role_slot_id: role_slot_id.clone(),
                    idempotency_key: key.as_str().to_owned(),
                    expected_team_revision: request.expected_team_revision,
                    authorized_role: request.authorized_by_role.clone(),
                    evidence: request.evidence.clone(),
                    evidence_hash: evidence_hash.clone(),
                    recorded_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;

        // A waiver may be the last thing a team was waiting for. It may equally
        // not be, and then this reports nothing rather than guessing: the field
        // is null until every *other* declared slot is accounted for too.
        //
        // A replay finds the team already closed — by the call this one is a
        // replay of — and reports that, rather than attempting a second closure
        // the aggregate would rightly refuse.
        let already = state
            .with_store(|store| store.get_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
            .is_some_and(|team| team.lifecycle.is_terminal());
        if already {
            return Ok(self.waiver_dto(state.realm_id(), stored, applied, Some(team_run_id)));
        }
        let team_run_closed = match self.certify_team(project_id, team_run_id)? {
            Ok(certificate) => {
                let evidence = certificate
                    .into_disposition_evidence(now)
                    .map_err(|error| self.refuse_domain(&error))?;
                let team = state
                    .with_store(|store| store.get_team_run(project_id, team_run_id))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::NotFound,
                            "no such team run exists in this project",
                        )
                    })?;
                state
                    .with_store(|store| {
                        store.close_team_run(&kontor_core::repository::TeamRunClosure {
                            project_id,
                            team_run_id,
                            expected_revision: team.revision,
                            evidence,
                        })
                    })
                    .map_err(|error| self.refuse(&error))?;
                state.signals().appended();
                Some(team_run_id.to_string())
            }
            Err(_) => None,
        };

        Ok(self.waiver_dto(
            state.realm_id(),
            stored,
            applied,
            team_run_closed.map(|_| team_run_id),
        ))
    }

    async fn settle_turn(
        &self,
        key: &IdempotencyKey,
        authority: kontor_api::auth::CallerCapability,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        request: &SettleTurnRequest,
    ) -> Result<SettledTurnDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();

        // The seat, and the binding that makes it a seat. A run that was never
        // bound has no turn to settle: there was no session to take one in.
        let run = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such agent run exists in this project",
                )
            })?;
        // A known run that was never bound is a *slot* problem, not a missing
        // address: the caller found the right thing and it has no session. Its
        // own code says so, and points at the only two ways forward — bind and
        // settle, or waive under the template's policy.
        let binding = run.binding.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::RoleSlotUnbound,
                "this run was never bound to a session, so it has no turn to settle",
            )
        })?;
        // A settled turn must never be a way to keep working a closed run. The
        // run staying *open* is the postcondition; a run already terminal is a
        // refusal rather than a turn.
        if run.terminal.is_some() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "this run is closed, so there is no bounded turn left to settle",
            ));
        }

        let team_run_id = run.team_run_id;
        let task_id = self.task_for_team_run(project_id, team_run_id)?;
        let task = self.task_row(project_id, task_id)?;
        // The revision is the caller's statement about *which* task state this
        // turn was taken against. A task that moved underneath the turn means the
        // work was judged against something else.
        if task.revision != request.expected_task_revision {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the task moved since the caller read it",
            ));
        }

        let role_slot =
            RoleSlotId::parse(&request.role_slot).map_err(|error| self.refuse_domain(&error))?;
        if run.role != role_slot.clone().into_role_key() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "this seat does not hold that role slot",
            ));
        }
        // The settling authority is the tier the caller *authenticated at*, not a
        // name from the request body. A caller-supplied account id proves nothing
        // about who is asking, so there is no longer a field for one.
        //
        // The provider account is derived from the bound run — it is the account
        // the seat actually runs as — and is operational context rather than
        // attribution. A run with none contributes none; nothing is invented.
        let account_profile = run.account_profile_id;

        let artifacts = self.artifact_keys(&request.artifacts)?;
        // The digest covers exactly what identifies this turn, so a replay under
        // the same key with different content is a conflict and not a second
        // position in the seat's sequence.
        let evidence = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "settle_role_turn",
            "task_id": task_id.to_string(),
            "team_run_id": team_run_id.to_string(),
            "agent_run_id": agent_run_id.to_string(),
            "role_slot": role_slot.as_role_key().as_str(),
            "task_revision": task.revision.get(),
            "binding_generation": binding.identity.generation,
            "authority_tier": authority.as_str(),
            "account_profile": account_profile.map(|id| id.to_string()),
            "artifacts": artifacts.iter().map(|key| key.as_str()).collect::<Vec<_>>(),
        }))?;

        let (settled, applied) = state
            .with_store(|store| {
                store.settle_role_turn(&NewRoleTurn {
                    id: RoleTurnId::generate(),
                    project_id,
                    task_id,
                    team_run_id,
                    agent_run_id,
                    role_slot_id: role_slot.clone(),
                    idempotency_key: key.as_str().to_owned(),
                    task_revision: task.revision,
                    binding_generation: binding.identity.generation,
                    authority_tier: authority.as_str(),
                    account_profile,
                    artifacts: artifacts.clone(),
                    evidence_hash: evidence.hash().clone(),
                    settled_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;

        // A settled turn is the one thing that proves *activity*: the seat took
        // a turn and it landed. An inspect that merely finds the session alive
        // is attachment, recorded elsewhere and deliberately not here — a seat
        // that answers `running` forever while doing nothing must read as
        // stalled, not as busy.
        self.observe_seat(
            project_id,
            task_id,
            team_run_id,
            &role_slot,
            &SeatLivenessObservation {
                attached_at: Some(now),
                activity_at: Some(now),
                ..SeatLivenessObservation::default()
            },
            now,
        )?;

        // The postcondition, asserted rather than assumed: settling a turn must
        // leave the seat's session live. If this process no longer holds the
        // frozen snapshot the seat is not reusable, and saying so is the honest
        // answer.
        let seat_live = state.sessions().get(binding.id).is_some();

        // Closing the team is attempted on every settlement, not only the last
        // one, because "was that the last slot?" is not a question the caller can
        // be trusted to answer — the certifier decides it from the template's
        // declared slots. Until every one is accounted for this is a no-op.
        let (team_run_closed, _) = self.settle_team(project_id, &run, now)?;
        let follow_ups = self.derive_follow_ups(project_id, &settled, now).await?;

        Ok(SettledTurnDto {
            realm_id: state.realm_id(),
            turn_id: settled.id.to_string(),
            task_id,
            agent_run_id: agent_run_id.to_string(),
            role_slot: settled.role_slot_id.as_role_key().as_str().to_owned(),
            turn_ordinal: settled.turn_ordinal,
            binding_generation: settled.binding_generation,
            artifacts: settled
                .artifacts
                .iter()
                .map(|key| key.as_str().to_owned())
                .collect(),
            settled_by: authority.as_str().to_owned(),
            account_profile: account_profile.map(|id| id.to_string()),
            evidence_hash: settled.evidence_hash.as_str().to_owned(),
            applied: applied_dto(applied),
            seat_live,
            team_run_closed,
            follow_ups,
        })
    }

    async fn attest_late_handoff(
        &self,
        key: &IdempotencyKey,
        authority: kontor_api::auth::CallerCapability,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        request: &AttestLateHandoffRequest,
    ) -> Result<LateHandoffAttestationDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let run = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such agent run exists in this project",
                )
            })?;
        let binding = run.binding.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::StaleBinding,
                "the run has no immutable native binding to attest",
            )
        })?;
        let terminal = run.terminal.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::RevisionConflict,
                "late handoff attestation requires a terminal run",
            )
        })?;
        if terminal.outcome != TerminalOutcome::Cancelled
            || !matches!(
                terminal.source,
                TerminalEvidenceSource::RuntimeObservation { .. }
            )
        {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "late handoff attestation is limited to runtime-observed cancellation",
            ));
        }
        if request.binding_generation != binding.identity.generation {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the immutable binding generation differs from the attestation",
            ));
        }

        let role_slot =
            RoleSlotId::parse(&request.role_slot).map_err(|error| self.refuse_domain(&error))?;
        if run.role != role_slot.clone().into_role_key() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "this run did not hold the attested role slot",
            ));
        }
        let task_id = self.task_for_team_run(project_id, run.team_run_id)?;
        let task = self.task_row(project_id, task_id)?;
        if task.revision != request.expected_task_revision {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the task moved since the handoff was produced",
            ));
        }

        let handoff_hash = ContentHash::parse(&request.handoff_hash).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "handoff_hash must be a lowercase 64-character SHA-256 digest",
            )
        })?;
        let receipt = self
            .best_effort_handoff_receipt(project_id, &run)?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the run has no matching best-effort handoff compaction receipt",
                )
            })?;
        if receipt.handoff_hash.as_ref() != Some(&handoff_hash) {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the attested handoff hash differs from the durable receipt",
            ));
        }
        let artifacts = self.artifact_keys(&request.artifacts)?;
        let evidence = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "attest_late_handoff",
            "task_id": task_id.to_string(),
            "team_run_id": run.team_run_id.to_string(),
            "agent_run_id": agent_run_id.to_string(),
            "role_slot": role_slot.as_role_key().as_str(),
            "task_revision": task.revision.get(),
            "binding_generation": binding.identity.generation,
            "terminal_outcome": terminal.outcome.as_str(),
            "compaction_receipt_id": receipt.id.to_string(),
            "handoff_hash": handoff_hash.as_str(),
            "authority_tier": authority.as_str(),
            "artifacts": artifacts.iter().map(|key| key.as_str()).collect::<Vec<_>>(),
        }))?;
        let (settled, applied) = state
            .with_store(|store| {
                store.attest_late_role_turn(&NewRoleTurn {
                    id: RoleTurnId::generate(),
                    project_id,
                    task_id,
                    team_run_id: run.team_run_id,
                    agent_run_id,
                    role_slot_id: role_slot.clone(),
                    idempotency_key: key.as_str().to_owned(),
                    task_revision: task.revision,
                    binding_generation: binding.identity.generation,
                    authority_tier: authority.as_str(),
                    account_profile: run.account_profile_id,
                    artifacts: artifacts.clone(),
                    evidence_hash: evidence.hash().clone(),
                    settled_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        let (team_run_closed, _) = self.settle_team(project_id, &run, now)?;
        let follow_ups = self.derive_follow_ups(project_id, &settled, now).await?;

        Ok(LateHandoffAttestationDto {
            realm_id: state.realm_id(),
            turn_id: settled.id.to_string(),
            task_id,
            agent_run_id: agent_run_id.to_string(),
            role_slot: settled.role_slot_id.as_role_key().as_str().to_owned(),
            binding_generation: settled.binding_generation,
            compaction_receipt_id: receipt.id.to_string(),
            handoff_hash: handoff_hash.as_str().to_owned(),
            artifacts: settled
                .artifacts
                .iter()
                .map(|artifact| artifact.as_str().to_owned())
                .collect(),
            terminal_outcome: terminal.outcome.as_str().to_owned(),
            seat_live: false,
            applied: applied_dto(applied),
            attested_by: authority.as_str().to_owned(),
            team_run_closed,
            follow_ups,
        })
    }

    async fn replace_seat(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        request: &ReplaceSeatRequest,
    ) -> Result<ReplacedSeatDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let predecessor = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such predecessor run exists in this project",
                )
            })?;
        let binding = predecessor.binding.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::StaleBinding,
                "the predecessor has no immutable native binding to replace",
            )
        })?;
        if request.binding_generation != binding.identity.generation {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the immutable binding generation differs from the replacement request",
            ));
        }
        let terminal = predecessor.terminal.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::UnsupportedCapability,
                "the predecessor is not terminal, so its seat cannot be replaced",
            )
        })?;
        if terminal.outcome != TerminalOutcome::Cancelled
            || !matches!(
                terminal.source,
                TerminalEvidenceSource::RuntimeObservation { .. }
            )
        {
            return Err(self.deny(
                ApiErrorCode::UnsupportedCapability,
                "seat replacement requires runtime-observed cancellation",
            ));
        }
        if state.sessions().get(binding.id).is_some() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the predecessor seat is still live and must be reused",
            ));
        }

        let role_slot =
            RoleSlotId::parse(&request.role_slot).map_err(|error| self.refuse_domain(&error))?;
        if predecessor.role != role_slot.clone().into_role_key() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the predecessor did not hold the requested role slot",
            ));
        }
        let task_id = self.task_for_team_run(project_id, predecessor.team_run_id)?;
        let task = self.task_row(project_id, task_id)?;
        if task.revision != request.expected_task_revision {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the task moved since the replacement was authorized",
            ));
        }
        let team = state
            .with_store(|store| store.get_team_run(project_id, predecessor.team_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the team run no longer exists"))?;
        if team.lifecycle.is_terminal() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the team run is terminal and cannot receive a successor",
            ));
        }

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "replace_seat",
            "predecessor_agent_run_id": agent_run_id.to_string(),
            "team_run_id": predecessor.team_run_id.to_string(),
            "role_slot": role_slot.as_role_key().as_str(),
            "task_revision": task.revision.get(),
            "binding_generation": binding.identity.generation,
        }))?;
        let target = AggregateRef::TeamRun {
            team_run_id: predecessor.team_run_id,
        };
        let replayed = self.replayed(key, &intent, Some(&target))?.is_some();
        if !replayed {
            self.record(
                key,
                project_id,
                CommandKind::ReplaceSeat,
                target,
                team.revision,
                &intent,
            )?;
        }

        let members = self.team_members(project_id, predecessor.team_run_id)?;
        let recorded_successor = members
            .iter()
            .find(|run| run.parent_agent_run_id == Some(agent_run_id));
        if let Some((successor, successor_binding)) = recorded_successor.and_then(|successor| {
            successor
                .binding
                .as_ref()
                .map(|binding| (successor, binding))
        }) {
            return Ok(ReplacedSeatDto {
                realm_id: state.realm_id(),
                task_id,
                team_run_id: predecessor.team_run_id.to_string(),
                predecessor_agent_run_id: agent_run_id.to_string(),
                successor_agent_run_id: successor.id.to_string(),
                role_slot: role_slot.as_role_key().as_str().to_owned(),
                runtime_kind: successor_binding.identity.runtime_kind.as_str().to_owned(),
                native_id: successor_binding.identity.native_id.as_str().to_owned(),
                applied: AppliedDto::Unchanged,
            });
        }

        let recorded_successor_id = recorded_successor.map(|successor| successor.id);
        let slot_members = recorded_successor_id.map_or_else(
            || members.clone(),
            |successor_id| {
                members
                    .iter()
                    .filter(|run| run.id != successor_id)
                    .cloned()
                    .collect()
            },
        );

        let bindings: Vec<_> = members
            .iter()
            .filter_map(|run| run.binding.as_ref())
            .filter_map(|held| state.sessions().get(held.id))
            .collect();
        let lease = TeamRunLease::acquire(predecessor.team_run_id)
            .map_err(|error| self.refuse_domain(&error))?;
        let mut slots = TeamRunSlots::hydrate(lease, &team.snapshot, &slot_members, &bindings)
            .map_err(|error| self.refuse_domain(&error))?;
        let closed = slots
            .latest_closed(&role_slot)
            .map_err(|error| self.refuse_domain(&error))?;
        if closed.agent_run_id() != agent_run_id {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the predecessor is not the role slot's latest closed attempt",
            ));
        }

        let successor_agent_run_id = recorded_successor_id.unwrap_or_else(AgentRunId::generate);
        let permit = slots
            .reserve_successor(closed, successor_agent_run_id)
            .map_err(|error| self.refuse_domain(&error))?;
        // The run id is durable before runtime launch. Derive a distinct UUIDv7
        // binding id from it so a retry can reclaim the runtime's exact admission.
        let mut binding_id = successor_agent_run_id.to_string();
        let last = binding_id.pop().expect("an entity id is not empty");
        binding_id.push(
            char::from_digit(
                last.to_digit(16).expect("an entity id is hexadecimal") ^ 1,
                16,
            )
            .expect("a hexadecimal digit remains hexadecimal"),
        );
        let binding_id = kontor_core::id::RuntimeBindingId::parse(&binding_id)
            .map_err(|error| self.refuse_domain(&error))?;
        let adapter = state
            .runtimes()
            .get(&binding.identity.runtime_kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "this daemon is not configured with the predecessor's runtime",
                )
            })?;
        if recorded_successor_id.is_none() {
            let successor_row = permit
                .new_agent_run(project_id, predecessor.account_profile_id, None, now)
                .map_err(|error| self.refuse_domain(&error))?;
            state
                .with_store(|store| store.create_agent_run(&successor_row))
                .map_err(|error| self.refuse(&error))?;
        }

        adapter
            .prepare_plane()
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        // The replacement is placed in the *same* container as the seat it
        // replaces. Preparing a fresh one keyed by anything else is how a
        // successor ends up working somewhere its predecessor never was.
        let task_root = self.task_root(project_id, task_id)?;
        let node = self.ensure_task_node(project_id, task_id)?;
        let workspace = self
            .ensure_container(project_id, &node, &task_root, adapter.as_ref())
            .await?;
        let context_policy = freeze_seat_context_policy(&adapter, &team.snapshot, &role_slot, now)
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let launch = SlotLaunch {
            task_id,
            binding_id,
            placement: Some(LaunchPlacement::Container(workspace.clone())),
            cwd: task_root.clone(),
            account_profile_id: predecessor.account_profile_id,
            prompt: slot_prompt(&role_slot, &eligible_roots(slots.template()))
                .map_err(|error| self.refuse_domain(&error))?,
            model_rung: freeze_seat_model_rung(&team.snapshot, &role_slot)
                .map_err(|error| self.refuse_domain(&error))?,
            context_policy: context_policy.clone(),
            requested_at: now,
        };
        let admission = permit.admission_request(&launch);
        let admitted = match adapter.admit_launch(&admission).await {
            Err(RuntimeError::ReplacementNotEvidenced {
                rule: "this seat holds no session to replace",
            }) => {
                adapter
                    .admit_launch(&AdmissionRequest {
                        replaces: None,
                        ..admission
                    })
                    .await
            }
            answer => answer,
        };
        let authority = admitted
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?
            .into_authority()
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let prepared = permit.launch_request(authority, launch);
        let outcome = adapter
            .launch(prepared.request())
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        slots
            .bind(prepared, &outcome.snapshot)
            .map_err(|error| self.refuse_domain(&error))?;
        let successor_binding = RuntimeBinding {
            id: outcome.snapshot.binding_id(),
            agent_run_id: successor_agent_run_id,
            identity: outcome.snapshot.identity().clone(),
            bound_at: now,
        };
        state
            .with_store(|store| {
                store.bind_agent_run(project_id, successor_agent_run_id, &successor_binding)
            })
            .map_err(|error| self.refuse(&error))?;
        state
            .with_store(|store| {
                store.record_run_context_policy(project_id, successor_agent_run_id, &context_policy)
            })
            .map_err(|error| self.refuse(&error))?;
        self.hold(&outcome.snapshot)?;
        self.retry_undelivered_dispatches().await?;

        Ok(ReplacedSeatDto {
            realm_id: state.realm_id(),
            task_id,
            team_run_id: predecessor.team_run_id.to_string(),
            predecessor_agent_run_id: agent_run_id.to_string(),
            successor_agent_run_id: successor_agent_run_id.to_string(),
            role_slot: role_slot.as_role_key().as_str().to_owned(),
            runtime_kind: successor_binding.identity.runtime_kind.as_str().to_owned(),
            native_id: successor_binding.identity.native_id.as_str().to_owned(),
            applied: if recorded_successor_id.is_some() {
                AppliedDto::Unchanged
            } else {
                AppliedDto::Created
            },
        })
    }

    async fn abandon_run(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        request: &AbandonRunRequest,
    ) -> Result<AbandonedRunDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let run = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such agent run exists in this project",
                )
            })?;

        // Already closed: report the stored closure rather than closing twice.
        // A terminal row is immutable, so a repeated abandon is answered from it.
        if let Some(terminal) = run.terminal.as_ref() {
            let receipt_id = match terminal.source {
                TerminalEvidenceSource::OperatorAbandon { receipt_id } => Some(receipt_id),
                TerminalEvidenceSource::RuntimeObservation { .. } => None,
            };
            // A repeat converges rather than reporting. The run is immutable and
            // nothing about it moves again, but a lease it still holds is state
            // this operation promised to give back — an abandonment that closed
            // the run and then failed before the release would otherwise be
            // unrepeatable, and the task would wait out an expiry with no way to
            // ask again.
            if let Some(receipt_id) = receipt_id {
                self.release_run_leases(project_id, agent_run_id, receipt_id, now)?;
            }
            let (team_run_closed, team_pending) = self.team_closure_state(project_id, &run)?;
            return Ok(AbandonedRunDto {
                realm_id: state.realm_id(),
                agent_run_id: agent_run_id.to_string(),
                outcome: terminal.outcome.as_str().to_owned(),
                applied: AppliedDto::Unchanged,
                revision: run.revision,
                team_run_closed,
                team_pending,
                receipt_id: receipt_id.map(|id| id.to_string()).unwrap_or_default(),
            });
        }

        // The one rule that makes this operation safe to expose. A bound run
        // holds a native session: closing Kontor's row would leave an agent
        // running that nothing is steering, and Kontor would have no record that
        // it is there. Those runs settle against their runtime, which is the
        // only thing that can say what the session is doing.
        if run.binding.is_some() {
            return Err(self.deny(
                ApiErrorCode::UnsupportedCapability,
                "this run holds a native session, so it is settled against its runtime rather than abandoned",
            ));
        }

        if run.revision.get() != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the run moved since the caller read it",
                )
                .with_revision(Some(run.revision)));
        }

        let reason =
            BoundedText::parse(&request.reason).map_err(|error| self.refuse_domain(&error))?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "runtime_abandon",
            "agent_run_id": agent_run_id.to_string(),
            "expected_revision": request.expected_revision,
            "reason": reason.as_str(),
        }))?;
        // Recorded against the revision being closed, because that is what the
        // store re-proves: a receipt naming another revision authorizes nothing
        // here, which stops a decision made about an older run from closing this
        // one.
        //
        // Deliberately not a command intent. An intent moves desired state under
        // compare-and-swap, which would bump the very revision this receipt has
        // to stay bound to — the receipt would invalidate itself. The gate
        // rejection path reached the same conclusion and writes its abandon
        // receipt the same way.
        let receipt_id = state
            .with_store(|store| {
                store.record_abandon_receipt(&kontor_core::repository::NewAbandonReceipt {
                    project_id,
                    receipt_id: CommandReceiptId::generate(),
                    idempotency_key: key.clone(),
                    target: AggregateRef::AgentRun { agent_run_id },
                    target_revision: run.revision,
                    intent: intent.clone(),
                    recorded_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        state
            .with_store(|store| {
                store.close_agent_run(&kontor_core::repository::RunClosure {
                    project_id,
                    agent_run_id,
                    expected_revision: run.revision,
                    evidence: kontor_core::state::TerminalEvidence {
                        outcome: TerminalOutcome::Abandoned,
                        source: TerminalEvidenceSource::OperatorAbandon { receipt_id },
                        evidence_hash: intent.hash().clone(),
                        closed_at: now,
                    },
                })
            })
            .map_err(|error| self.refuse(&error))?;

        self.release_run_leases(project_id, agent_run_id, receipt_id, now)?;

        // The task is only schedulable again once its *team* run is terminal
        // too, so the same certified closure the settle path uses is attempted
        // here. It is attempted, not asserted: a team with other live runs stays
        // open and says why.
        let closed = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the run disappeared while it was being abandoned",
                )
            })?;
        let (mut team_run_closed, mut team_pending) = self.settle_team(project_id, &closed, now)?;
        // A team whose every run has ended, and which no certificate can close,
        // is abandoned under the same operator decision. That is the whole
        // reason the task stays stuck: a certified closure proves every declared
        // slot is accounted for, and a launch refused at the first seat never
        // created the rest of them.
        //
        // Only when nothing is left running. A team with a live run keeps it:
        // abandoning one phantom must never close the work beside it.
        if team_run_closed.is_none()
            && let Some(abandoned) = self.abandon_team_run(key, project_id, &closed, now)?
        {
            team_run_closed = Some(abandoned);
            team_pending = None;
        }
        state.signals().appended();
        Ok(AbandonedRunDto {
            realm_id: state.realm_id(),
            agent_run_id: agent_run_id.to_string(),
            outcome: TerminalOutcome::Abandoned.as_str().to_owned(),
            applied: AppliedDto::Created,
            revision: closed.revision,
            team_run_closed,
            team_pending,
            receipt_id: receipt_id.to_string(),
        })
    }

    async fn settle_runtime(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> Result<RuntimeSettlementDto, ApiError> {
        let state = self.state()?;
        let realm_id = state.realm_id();
        let now = kontor_api::now();

        // (1) The run and its *immutable* binding. A run that was never bound has
        // no session to ask about, which is not the same as a session that ended.
        let run = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such agent run exists in this project",
                )
            })?;
        let binding = run.binding.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this run was never bound to a native session, so there is nothing to settle",
            )
        })?;

        if run.terminal.is_none() && self.latest_handoff_receipt(project_id, &run)?.is_some() {
            let task_id = self.task_for_team_run(project_id, run.team_run_id)?;
            let role_slot = RoleSlotId::new(run.role.clone());
            if !self.role_slot_has_disposition(project_id, task_id, &run, &role_slot)? {
                return Err(self.deny(
                    ApiErrorCode::HandoffUnsettled,
                    "a durable handoff must be settled or attested before runtime settlement",
                ));
            }
        }

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "runtime_settle",
            "agent_run_id": agent_run_id.to_string(),
        }))?;
        let target = AggregateRef::AgentRun { agent_run_id };
        let receipt = if let Some(existing) = self.replayed(key, &intent, Some(&target))? {
            existing.id
        } else {
            self.record(
                key,
                project_id,
                CommandKind::SettleRuntime,
                target,
                run.revision,
                &intent,
            )?
        };

        // Idempotence, decided before the runtime is touched. A closed run has
        // already been settled, and asking again would take a second observation
        // about a session that is no longer this run's business.
        if let Some(terminal) = run.terminal.as_ref() {
            let cursor = match terminal.source {
                kontor_core::state::TerminalEvidenceSource::RuntimeObservation { cursor } => {
                    Some(cursor)
                }
                kontor_core::state::TerminalEvidenceSource::OperatorAbandon { .. } => None,
            };
            let (team_run_closed, team_pending) = self.team_closure_state(project_id, &run)?;
            return Ok(RuntimeSettlementDto {
                realm_id,
                agent_run_id: agent_run_id.to_string(),
                observed: run.projection.observed.as_str().to_owned(),
                outcome: Some(terminal.outcome.as_str().to_owned()),
                evidence_cursor: cursor,
                applied: AppliedDto::Unchanged,
                team_run_closed,
                team_pending,
                receipt_id: receipt.to_string(),
            });
        }

        // (1, continued) The runtime's own copy of the binding. A snapshot is a
        // plain value with public fields, so a caller holding one could write a
        // better trust grade into it; only the runtime that issued a binding can
        // say what it was issued at, and `terminal_evidence` takes that and
        // nothing else.
        let adapter = state
            .runtimes()
            .get(&binding.identity.runtime_kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "this daemon is not configured with the runtime that owns the session",
                )
            })?;
        let held = state.sessions().get(binding.id).ok_or_else(|| {
            self.deny(
                ApiErrorCode::StaleBinding,
                "this process holds no frozen capability snapshot for the session",
            )
        })?;
        let issued = adapter
            .issued_binding(&held)
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

        // The frozen preflight, before any request is built: an operation the
        // binding's own capabilities never covered produces no runtime effect.
        let mut context = kontor_runtime::capability::OperationContext::new(
            kontor_runtime::capability::RuntimeCapability::Inspect,
        );
        context.autonomous = false;
        context.binding = Some(issued.snapshot());
        kontor_runtime::capability::preflight(&issued.snapshot().capabilities, &context)
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

        // (2) A *fresh* read. Not the last thing the session said, not a cached
        // projection: closure is a claim about now, and an observation left to age
        // is a description of the past whatever it says.
        let observation = adapter
            .inspect(&kontor_runtime::request::InspectRequest {
                binding: issued.snapshot().clone(),
                requested_at: now,
            })
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;

        // (3) Persist it first. The raw event lands before anything is concluded
        // from it, so the closure below can cite a row that already exists and the
        // store can re-load and re-prove it.
        //
        // What is persisted is *control metadata* about the observation, not the
        // adapter's own evidence document. The durable log holds a closed
        // vocabulary of scalar control fields on purpose — it is the one place a
        // transcript could accumulate — and an adapter's document is free to carry
        // whatever the runtime told it. The digest below is therefore the digest of
        // this document, which is what the closure cites and what the store
        // re-loads and compares.
        let payload = self.intent(&serde_json::json!({
            "schema_version": 1,
            "observed_state": observation.state.as_str(),
            "contact": observation.contact.as_str(),
            "native_sequence": observation.native_sequence,
            "observed_at": observation.observed_at.to_string(),
        }))?;
        let projection = state
            .with_store(|store| {
                store.record_observation(&kontor_core::repository::NewObservation {
                    event: kontor_core::repository::NewRuntimeEvent {
                        project_id,
                        agent_run_id,
                        identity: observation.identity.clone(),
                        native_event_id: observation.native_event_id.clone(),
                        native_sequence: observation.native_sequence,
                        payload: payload.clone(),
                        observed_at: observation.observed_at,
                    },
                    observed: observation.state,
                    contact: observation.contact,
                    freshness: kontor_core::state::Freshness::evaluate(
                        Some(observation.observed_at),
                        now,
                        jiff::SignedDuration::from_secs(state.evidence_window_seconds()),
                    ),
                    expected_revision: run.revision,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        state.signals().appended();

        // (3b) The same readback, against the seat's own binding. A successful
        // inspect proves the session is *there*, so it records attachment and
        // quotes what the runtime said about itself — and it deliberately
        // records no activity. Treating `running` as activity is the shortcut
        // that makes a hung seat look busy for as long as its process survives.
        self.observe_seat(
            project_id,
            self.task_for_team_run(project_id, run.team_run_id)?,
            run.team_run_id,
            &RoleSlotId::new(run.role.clone()),
            &SeatLivenessObservation {
                attached_at: Some(observation.observed_at),
                runtime_reported: Some(observation.state),
                ..SeatLivenessObservation::default()
            },
            now,
        )?;

        // (4) The only place an outcome comes from. It is derived from the
        // observation against the *issued* binding, and it refuses every uncertain
        // input: a broken channel, another run's session, an evidence class that
        // only acknowledges, a grade that may not evidence closure, an observation
        // older than the window, and any non-terminal state.
        let Some(outcome) =
            observation.terminal_evidence(&issued, now, state.evidence_window_seconds())
        else {
            return Err(self.deny(
                ApiErrorCode::UnsupportedCapability,
                "the runtime does not currently evidence a terminal state for this run",
            ));
        };
        let cursor = projection.last_cursor.ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the observation was not reduced into this run's projection",
            )
        })?;

        // (5) Close on the persisted observation's own cursor. The store re-loads
        // that row inside the closing transaction and re-proves it belongs to this
        // run, was emitted by this binding, and is the event the projection
        // actually reduced.
        let settled = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the run vanished mid-settlement"))?;
        state
            .with_store(|store| {
                store.close_agent_run(&kontor_core::repository::RunClosure {
                    project_id,
                    agent_run_id,
                    expected_revision: settled.revision,
                    evidence: kontor_core::state::TerminalEvidence {
                        outcome,
                        source: kontor_runtime::observation::ControlPlaneObservation::
                            terminal_evidence_source(cursor),
                        // The digest of what was *stored*, which is what the
                        // store re-loads and compares inside the closing
                        // transaction.
                        evidence_hash: payload.hash().clone(),
                        closed_at: now,
                    },
                })
            })
            .map_err(|error| self.refuse(&error))?;
        state.signals().appended();

        // (6) And the team, once every *declared* slot is terminal. Walking the
        // template rather than the runs that happen to exist is what makes an
        // omitted seat fail instead of pass silently.
        let closed = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the run vanished mid-settlement"))?;
        let (team_run_closed, team_pending) = self.settle_team(project_id, &closed, now)?;
        // The frozen snapshot is released only once the team has been certified:
        // certification reads every seat's binding, and forgetting this one first
        // would make the seat that just closed look like one that never ran.
        self.release(binding.id)?;

        Ok(RuntimeSettlementDto {
            realm_id,
            agent_run_id: agent_run_id.to_string(),
            observed: observation.state.as_str().to_owned(),
            outcome: Some(outcome.as_str().to_owned()),
            evidence_cursor: Some(cursor),
            applied: AppliedDto::Created,
            team_run_closed,
            team_pending,
            receipt_id: receipt.to_string(),
        })
    }

    async fn register_pack(
        &self,
        key: &IdempotencyKey,
        request: &RegisterPackRequest,
    ) -> Result<ProfilePackDto, ApiError> {
        let state = self.state()?;
        // Validated in full *before* anything is stored, by the pack's own
        // validator and not a second one written here: a pack that resolves in
        // this process and refuses in the next would be a catalogue entry an
        // epic could freeze and never run.
        let document = serde_json::to_string(&request.pack).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "the pack is not a serializable document",
            )
        })?;
        let parsed = parse_pack(&document).map_err(|error| self.refuse_domain(&error))?;

        // The seeds win every category they advertise. Registration widens the
        // catalogue; it never redefines what an already-frozen epic pinned.
        if let Some(shadowed) = parsed
            .manifest
            .iter()
            .find(|entry| self.pack.category(&entry.category).is_some())
        {
            let _ = shadowed;
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "this pack re-advertises a category the build already ships",
            ));
        }

        // The key is bound to a *fingerprint of this logical operation*, not to
        // the pack alone. Content immutability answers "may these bytes be this
        // revision?"; it cannot answer "was this key already used for something
        // else?", because two registrations of two different packs are each
        // independently valid and nothing was comparing them. The fingerprint is
        // what makes the key mean one operation: a digest of a canonical
        // document — the same convention a command intent is digested by, so the
        // two cannot disagree about what identity means.
        let fingerprint = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": REGISTER_PACK,
            "pack_id": parsed.pack_id.as_str(),
            "version": parsed.version.get(),
            "content_hash": ContentHash::of(document.as_bytes()).as_str(),
        }))?;
        let binding = IdempotencyBinding {
            key: key.as_str().to_owned(),
            operation: REGISTER_PACK,
            fingerprint: fingerprint.hash().clone(),
            bound_at: kontor_api::now(),
        };
        let registered = RegisteredPack {
            pack_id: parsed.pack_id.as_str().to_owned(),
            version: parsed.version,
            document_hash: ContentHash::of(document.as_bytes()),
            document,
            registered_at: kontor_api::now(),
        };
        let (stored, applied) = state
            .with_store(|store| store.register_profile_pack(&registered, &binding))
            .map_err(|error| self.refuse(&error))?;
        Ok(pack_dto(
            &parsed,
            "registered",
            Some(stored.document_hash.as_str().to_owned()),
            applied_dto(applied),
        ))
    }

    fn profile_packs(&self) -> Result<Vec<ProfilePackDto>, ApiError> {
        let state = self.state()?;
        let registered = state
            .with_store(SqliteStore::list_profile_packs)
            .map_err(|error| self.refuse(&error))?;
        let mut packs = vec![pack_dto(&self.pack, "bundled", None, AppliedDto::Unchanged)];
        for pack in &registered {
            let parsed = parse_pack(&pack.document).map_err(|error| self.refuse_domain(&error))?;
            packs.push(pack_dto(
                &parsed,
                "registered",
                Some(pack.document_hash.as_str().to_owned()),
                AppliedDto::Unchanged,
            ));
        }
        Ok(packs)
    }

    fn work_profile(&self, category: &str) -> Result<WorkProfileDetailDto, ApiError> {
        let now = kontor_api::now();
        // A category the pack does not advertise is *absent*, not malformed:
        // the key parses fine, and reporting it as a bad request would send a
        // caller looking for a typo in a name that is simply not shipped here.
        self.advertised(category)?;
        let bundle = self.bundle(category, now)?;
        let definition = &bundle.profile.definition;
        // The team's handoffs live on the pack's template, not on the pinned
        // revision the bundle carries: a revision holds the canonical bytes and
        // the role authority, and re-parsing those to recover a DAG the pack
        // already has in hand would be a second answer to the same question.
        // Every pack, not only the compiled one: a registered pack carries the
        // team its own profile pins, and looking only in the seeds would report
        // a custom profile as having no handoffs and therefore every slot as an
        // eligible root.
        let packs = self.packs()?;
        let template = bundle.team.as_ref().and_then(|team| {
            packs.iter().flat_map(|pack| &pack.teams).find(|candidate| {
                candidate.template_id == team.template_id && candidate.version == team.version
            })
        });
        let handoffs = template.map_or_else(Vec::new, |team| {
            team.handoffs
                .iter()
                .map(|handoff| ProfileHandoffDto {
                    from_slot: handoff.from_slot.as_role_key().as_str().to_owned(),
                    to_slot: handoff.to_slot.as_role_key().as_str().to_owned(),
                    // A handoff without a declared phase is available as soon as
                    // its artifacts exist; reporting a phase it does not name
                    // would invent one.
                    after_phase: handoff
                        .after_phase
                        .as_ref()
                        .map_or_else(|| "any".to_owned(), |phase| phase.as_str().to_owned()),
                    required_artifacts: handoff
                        .required_artifacts
                        .iter()
                        .map(|artifact| artifact.as_str().to_owned())
                        .collect(),
                })
                .collect()
        });
        let roots = template.map_or_else(Vec::new, |team| {
            eligible_roots(team)
                .iter()
                .map(|slot| slot.as_role_key().as_str().to_owned())
                .collect()
        });
        Ok(WorkProfileDetailDto {
            category: bundle.category.as_str().to_owned(),
            name: definition.name.clone(),
            profile: RevisionRefDto {
                id: definition.id.as_str().to_owned(),
                version: definition.version,
            },
            team: bundle.team.as_ref().map(|team| RevisionRefDto {
                id: team.template_id.to_string(),
                version: team.version,
            }),
            entry_phase: definition.entry_phase.as_str().to_owned(),
            phases: definition
                .phases
                .iter()
                .map(|phase| ProfilePhaseDto {
                    phase: phase.id.as_str().to_owned(),
                    label: phase.label.clone(),
                    required_artifacts: phase
                        .required_artifacts
                        .iter()
                        .map(|artifact| artifact.as_str().to_owned())
                        .collect(),
                    gates: phase
                        .gates
                        .iter()
                        .map(|gate| gate.as_str().to_owned())
                        .collect(),
                    rejection_route: phase
                        .rejection_route
                        .as_ref()
                        .map(|phase| phase.as_str().to_owned()),
                })
                .collect(),
            terminal_phases: definition
                .terminal_phases
                .iter()
                .map(|phase| phase.as_str().to_owned())
                .collect(),
            // Every gate is reported at `not_ready`: this is the profile, not a
            // task running it, and there is no evidence to reduce a state from.
            gates: definition
                .gates
                .iter()
                .map(|gate| GateProjectionDto {
                    gate: gate.id.as_str().to_owned(),
                    phase: gate.phase.as_str().to_owned(),
                    state: kontor_core::state::GateState::NotReady.as_str().to_owned(),
                    evaluator_roles: gate
                        .evaluator_roles
                        .iter()
                        .map(|role| role.as_str().to_owned())
                        .collect(),
                    required_evidence: gate
                        .required_evidence
                        .iter()
                        .map(|artifact| artifact.as_str().to_owned())
                        .collect(),
                    waiver_allowed: gate.waiver_allowed,
                    waiver_roles: gate
                        .waiver_roles
                        .iter()
                        .map(|role| role.as_str().to_owned())
                        .collect(),
                })
                .collect(),
            artifacts: definition
                .artifacts
                .iter()
                .map(|artifact| ProfileArtifactDto {
                    artifact: artifact.key.as_str().to_owned(),
                    label: artifact.label.clone(),
                    producer_phase: artifact.producer_phase.as_str().to_owned(),
                    evidence_required: artifact.evidence_required,
                })
                .collect(),
            handoffs,
            eligible_roots: roots,
            definition_hash: bundle.profile.definition_hash.as_str().to_owned(),
            bundle_hash: bundle.bundle_hash.as_str().to_owned(),
        })
    }

    fn validate_work_profile(&self, category: &str) -> Result<ProfileValidationDto, ApiError> {
        let (parsed, availability) = self.advertised(category)?;
        let (owner, _) = self.owning_pack(category)?;
        let pack_valid = validate_pack(&owner).is_ok();
        // Resolution runs the pack's invariants *and* the category's own
        // availability rule, then the bundle re-derives every digest it pins.
        // A category that resolves but does not verify is the interesting case:
        // it means the pack drifted from what it says it is.
        let (bundle_hash, bundle_verified, refused) =
            match resolve_profile(&owner, &parsed, kontor_api::now()) {
                Ok(bundle) => match bundle.verify() {
                    Ok(()) => (Some(bundle.bundle_hash.as_str().to_owned()), true, None),
                    Err(_) => (
                        Some(bundle.bundle_hash.as_str().to_owned()),
                        false,
                        Some("the resolved bundle no longer matches its own pinned digests"),
                    ),
                },
                Err(_) => (
                    None,
                    false,
                    Some("this category does not resolve to a runnable profile revision"),
                ),
            };
        Ok(ProfileValidationDto {
            category: parsed.as_str().to_owned(),
            availability: match availability {
                PackAvailability::Seeded => "seeded".to_owned(),
                PackAvailability::ManifestOnly => "manifest_only".to_owned(),
            },
            pack_valid,
            bundle_verified,
            bundle_hash,
            refused: refused.map(ToOwned::to_owned),
        })
    }

    fn trigger(
        &self,
        project_id: ProjectId,
        trigger: &str,
        version: SpecVersion,
    ) -> Result<TriggerSpecDto, ApiError> {
        let spec = self.trigger_spec(project_id, trigger, version)?;
        Ok(trigger_dto(&spec))
    }

    async fn submit_intake(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &SubmitIntakeRequest,
    ) -> Result<IntakeReceiptDto, ApiError> {
        let state = self.state()?;
        let spec = self.trigger_spec(project_id, &request.trigger, request.trigger_version)?;
        let envelope = self.intent(&request.envelope)?;
        let identity = SourceIdentity {
            source_kind: spec.source_kind.clone(),
            source_connection: spec.source_connection.clone(),
            external_event_id: request.external_event_id.clone(),
        };
        // The identity is looked up before anything is built, so a replay
        // answers from the decision already recorded rather than from a second
        // evaluation that would have to agree with it.
        if let Some(original) = state
            .with_store(|store| store.find_intake_receipt(project_id, &identity))
            .map_err(|error| self.refuse(&error))?
        {
            return Ok(intake_dto(
                state.realm_id(),
                &original,
                AppliedDto::Unchanged,
            ));
        }
        let matched = spec
            .matches(&envelope)
            .map_err(|error| self.refuse_domain(&error))?;
        let dedup_key = spec
            .dedup
            .evaluate(&envelope)
            .map_err(|error| self.refuse_domain(&error))?;
        let now = kontor_api::now();
        let event = CanonicalSourceEvent {
            id: SourceEventId::generate(),
            identity,
            envelope,
            external_observed_at: request.external_observed_at,
            ingested_at: now,
            processing_state: if matched {
                SourceProcessingState::Evaluated
            } else {
                SourceProcessingState::Ignored
            },
        };
        let receipt = IntakeReceipt {
            id: IntakeReceiptId::generate(),
            source_event_id: event.id,
            source_event_hash: event.envelope.hash().clone(),
            trigger: spec.id.clone(),
            trigger_version: spec.version,
            // A matched event is *proposed* and never approved. Approving one
            // means arming work without a human, which is the trigger's
            // auto-arm policy talking — evaluated by the intake service that
            // owns it, not by the operation that submitted the event.
            result: if matched {
                IntakeResult::Proposed
            } else {
                IntakeResult::Ignored
            },
            approval: None,
            proposed: None,
            idempotency_key: key.clone(),
            dedup_key,
            duplicate_of: None,
            predecessor_receipt_id: None,
            decided_at: now,
        };
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "submit_intake",
            "trigger": spec.id.as_str(),
            "trigger_version": spec.version.get(),
            "source_event_hash": event.envelope.hash().as_str(),
        }))?;
        let target = AggregateRef::Project { project_id };
        if self.replayed(key, &intent, Some(&target))?.is_none() {
            let project = self.project_row(project_id)?;
            self.record(
                key,
                project_id,
                CommandKind::SubmitIntake,
                target,
                project.revision,
                &intent,
            )?;
        }
        let outcome = state
            .with_store(|store| {
                store.record_source_event(&NewSourceEvent {
                    project_id,
                    event: event.clone(),
                    receipt: receipt.clone(),
                })
            })
            .map_err(|error| self.refuse(&error))?;
        let (stored, applied) = match outcome {
            IntakeOutcome::Recorded(receipt) => (receipt, AppliedDto::Created),
            IntakeOutcome::Duplicate(receipt) => (receipt, AppliedDto::Unchanged),
        };
        Ok(intake_dto(state.realm_id(), &stored, applied))
    }

    fn intake_receipt(
        &self,
        project_id: ProjectId,
        receipt_id: &str,
    ) -> Result<IntakeReceiptDto, ApiError> {
        let state = self.state()?;
        let id = IntakeReceiptId::parse(receipt_id).map_err(|error| self.refuse_domain(&error))?;
        let receipt = state
            .with_store(|store| store.get_intake_receipt(project_id, id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such intake decision exists in this project",
                )
            })?;
        Ok(intake_dto(
            state.realm_id(),
            &receipt,
            AppliedDto::Unchanged,
        ))
    }

    fn connector_field_specs(
        &self,
        project_id: ProjectId,
        connector: &str,
    ) -> Result<Vec<ConnectorSpecDto>, ApiError> {
        let state = self.state()?;
        let connector =
            ConnectorKey::parse(connector).map_err(|error| self.refuse_domain(&error))?;
        let catalog = self.connector_catalog()?;
        let mut specs = Vec::new();
        for compiled in catalog.field_specs() {
            let spec = compiled.spec();
            if spec.connector != connector {
                continue;
            }
            let selector = kontor_core::repository::ConnectorSpecSelector {
                project_id,
                connector: spec.connector.clone(),
                project: spec.project.clone(),
                issue_type: spec.issue_type.clone(),
                version: spec.version,
            };
            let installed = state
                .with_store(|store| store.get_ticket_field_spec(&selector))
                .map_err(|error| self.refuse(&error))?
                .is_some();
            specs.push(ConnectorSpecDto {
                connector: spec.connector.as_str().to_owned(),
                external_project: spec.project.as_str().to_owned(),
                issue_type: spec.issue_type.as_str().to_owned(),
                version: spec.version,
                definition_hash: compiled.hash().as_str().to_owned(),
                covers: spec
                    .mappings
                    .iter()
                    .map(|mapping| mapping.key.as_str().to_owned())
                    .collect(),
                installed,
            });
        }
        Ok(specs)
    }

    fn connector_workflow_specs(
        &self,
        project_id: ProjectId,
        connector: &str,
    ) -> Result<Vec<ConnectorSpecDto>, ApiError> {
        let state = self.state()?;
        let connector =
            ConnectorKey::parse(connector).map_err(|error| self.refuse_domain(&error))?;
        let catalog = self.connector_catalog()?;
        let mut specs = Vec::new();
        for compiled in catalog.workflow_specs() {
            let spec = compiled.spec();
            if spec.connector != connector {
                continue;
            }
            let installed = state
                .with_store(|store| {
                    store.get_external_workflow_spec(
                        &kontor_core::repository::ConnectorSpecSelector {
                            project_id,
                            connector: spec.connector.clone(),
                            project: spec.project.clone(),
                            issue_type: spec.issue_type.clone(),
                            version: spec.version,
                        },
                    )
                })
                .map_err(|error| self.refuse(&error))?
                .is_some();
            specs.push(ConnectorSpecDto {
                connector: spec.connector.as_str().to_owned(),
                external_project: spec.project.as_str().to_owned(),
                issue_type: spec.issue_type.as_str().to_owned(),
                version: spec.version,
                definition_hash: compiled.hash().as_str().to_owned(),
                covers: spec
                    .milestones
                    .iter()
                    .map(|rule| rule.milestone.as_str().to_owned())
                    .collect(),
                installed,
            });
        }
        Ok(specs)
    }

    fn ticket_conflicts(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<Vec<TicketConflictDto>, ApiError> {
        let state = self.state()?;
        self.task_row(project_id, task_id)?;
        let conflicts = state
            .with_store(|store| store.list_task_ticket_conflicts(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        Ok(conflicts.iter().map(conflict_dto).collect())
    }

    async fn resolve_ticket_conflict(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
        request: &ResolveConflictRequest,
    ) -> Result<TicketConflictDto, ApiError> {
        let state = self.state()?;
        self.task_row(project_id, task_id)?;
        let id = StatusConflictId::parse(&request.conflict_id)
            .map_err(|error| self.refuse_domain(&error))?;
        let conflicts = state
            .with_store(|store| store.list_task_ticket_conflicts(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        let conflict = conflicts
            .iter()
            .find(|candidate| candidate.id == id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such conflict is recorded against this task's tickets",
                )
            })?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "resolve_ticket_conflict",
            "conflict_id": conflict.id.to_string(),
        }))?;
        let target = AggregateRef::TicketLink {
            link_id: conflict.link_id,
        };
        // The key is judged before the already-resolved short-circuit, so a
        // changed request under a used key is a conflict rather than a replay
        // wearing the previous answer's clothes.
        let replay = self.replayed(key, &intent, Some(&target))?;
        if conflict.resolved_at.is_some() {
            return Ok(conflict_dto(conflict));
        }
        let receipt = if let Some(existing) = replay {
            existing.id
        } else {
            self.record(
                key,
                project_id,
                CommandKind::ResolveStatusConflict,
                target,
                conflict.task_revision,
                &intent,
            )?
        };
        let resolved_at = kontor_api::now();
        state
            .with_store(|store| store.resolve_conflict(project_id, id, receipt, resolved_at))
            .map_err(|error| self.refuse(&error))?;
        Ok(TicketConflictDto {
            resolved_at: Some(resolved_at),
            ..conflict_dto(conflict)
        })
    }

    async fn pull_ticket_comments(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<TicketCommentPullDto, ApiError> {
        let state = self.state()?;
        let task = self.task_row(project_id, task_id)?;
        let links = state
            .with_store(|store| store.list_task_ticket_links(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "pull_ticket_comments",
            "task_id": task_id.to_string(),
            "links": links.iter().map(|link| link.id.to_string()).collect::<Vec<_>>(),
        }))?;
        let target = AggregateRef::Task { task_id };
        let receipt = if let Some(existing) = self.replayed(key, &intent, Some(&target))? {
            existing.id
        } else {
            self.record(
                key,
                project_id,
                CommandKind::PullTicketComments,
                target,
                task.revision,
                &intent,
            )?
        };
        if !links.is_empty() {
            // Mirroring a revision means reading it out of the external system,
            // and that needs the connector this Realm is configured with.
            // Answering `mirrored: 0` without one would be a claim about a
            // system nothing contacted, which is indistinguishable from "there
            // were no new comments".
            return Err(self.deny(
                ApiErrorCode::Unavailable,
                "this realm is not configured with a connector that can read those tickets",
            ));
        }
        let held = state
            .with_store(|store| store.list_task_inbound_comments(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        Ok(TicketCommentPullDto {
            realm_id: state.realm_id(),
            task_id,
            links: Vec::new(),
            mirrored: 0,
            held: u32::try_from(held.len()).unwrap_or(u32::MAX),
            receipt_id: receipt.to_string(),
        })
    }

    fn ticket_comments(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<Vec<TicketCommentDto>, ApiError> {
        let state = self.state()?;
        self.task_row(project_id, task_id)?;
        let comments = state
            .with_store(|store| store.list_task_inbound_comments(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        Ok(comments
            .iter()
            .map(|comment| TicketCommentDto {
                link_id: comment.link_id.to_string(),
                external_comment_id: comment.external_comment_id.clone(),
                body_hash: comment.body_hash.as_str().to_owned(),
                author_account_id: comment.author_account_id.clone(),
                external_created_at: comment.external_created_at,
                external_updated_at: comment.external_updated_at,
                observed_at: comment.observed_at,
                supersedes: comment
                    .supersedes
                    .as_ref()
                    .map(|hash| hash.as_str().to_owned()),
            })
            .collect())
    }

    async fn claim_ticket(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<TicketClaimDto, ApiError> {
        let state = self.state()?;
        let task = self.task_row(project_id, task_id)?;
        let links = state
            .with_store(|store| store.list_task_ticket_links(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        if links.is_empty() {
            return Err(self.deny(
                ApiErrorCode::NotFound,
                "this task is linked to no external ticket to claim",
            ));
        }
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "claim_ticket",
            "task_id": task_id.to_string(),
            "action": ownership_action_name(OwnershipAction::ReassignToPrincipal),
            "links": links.iter().map(|link| link.id.to_string()).collect::<Vec<_>>(),
        }))?;
        let target = AggregateRef::Task { task_id };
        let receipt = if let Some(existing) = self.replayed(key, &intent, Some(&target))? {
            existing.id
        } else {
            self.record(
                key,
                project_id,
                CommandKind::ClaimTicket,
                target,
                task.revision,
                &intent,
            )?
        };
        // What is recorded is Kontor's own decision to hold these tickets, and
        // that is the whole of what this operation may decide. Writing the
        // assignee is convergence, and convergence needs the principal — which
        // is read from the external system in the same exchange that observes
        // the issue, because "is the holder me?" cannot be answered by guessing
        // an account id. `ticket:reconcile-apply` is where that write happens,
        // and where its absence is already refused.
        Ok(TicketClaimDto {
            realm_id: state.realm_id(),
            task_id,
            links: links.iter().map(|link| link.id.to_string()).collect(),
            action: ownership_action_name(OwnershipAction::ReassignToPrincipal),
            receipt_id: receipt.to_string(),
        })
    }
}

impl Services {
    fn mark_started_tasks_in_progress(
        &self,
        project_id: ProjectId,
        started: &[StartedSeatDto],
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let seated: BTreeSet<TaskId> = started.iter().map(|seat| seat.task_id).collect();
        for task_id in seated {
            let task = self.task_row(project_id, task_id)?;
            if task.state != TaskState::Ready {
                continue;
            }
            state
                .with_store(|store| {
                    store.transition_task(&TaskTransitionRequest {
                        project_id,
                        task_id,
                        expected_revision: task.revision,
                        to: TaskState::InProgress,
                        resume_receipt: None,
                        run_outcome: None,
                        produced_artifacts: BTreeSet::new(),
                        completed_phases: BTreeSet::new(),
                        team_closure: TaskTeamClosure::NoTeam,
                        occurred_at: kontor_api::now(),
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }
        Ok(())
    }

    /// Create or reuse every seat one admitted task's team declares.
    ///
    /// The order is the whole point. `admit_candidate` commits the team run, the
    /// agent run, the launch intent and the leases first, because a crash after
    /// that leaves a run with no session — visible to reconciliation, and
    /// finishable. Only then is the workspace prepared, the seat admitted by the
    /// runtime and the session launched; the binding is attached last. A replayed
    /// start finds the runtime already holding the seat and reuses it rather than
    /// starting a second session.
    async fn seat(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        admitted: &AdmittedCandidate,
    ) -> Result<Vec<StartedSeatDto>, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let workflow = state
            .with_store(|store| store.get_active_task_workflow(project_id, admitted.task_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the admitted task has no active workflow to run under",
                )
            })?;
        let pinned = workflow
            .snapshot
            .definition
            .team_template
            .as_ref()
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::UnsupportedCapability,
                    "the task's work profile prescribes no team, so there is no seat to fill",
                )
            })?;
        // Every pack this realm holds, not only the compiled one. A registered
        // pack's profile can be selected and frozen onto a task, so its team has
        // to be seatable too — resolving only the seeds here made a registered
        // profile applicable and unrunnable, which is worse than refusing it.
        let team = self
            .packs()?
            .into_iter()
            .find_map(|pack| pack.team(pinned.template_id, pinned.version).cloned())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the pinned team template revision is in no pack this realm holds",
                )
            })?;
        let revision = team
            .to_revision()
            .map_err(|error| self.refuse_domain(&error))?;
        // *Every* declared slot, not the first one. A team run that seated one of
        // five roles can never be certified closed — the closure walks the frozen
        // template's declared slots, and a seat that never ran is unaccounted for
        // rather than absent — so a task started that way could never legally
        // finish. One team run, one seat per declared role.
        //
        // Materializing every seat is not the same as *starting* every seat, and
        // the difference is the handoff DAG the template already carries. A slot
        // that is some other slot's `to_slot` is downstream: it waits for that
        // handoff. Giving it the same "begin the admitted task" instruction as the
        // roots would have five agents start the same work at once, each of them
        // told it is theirs to do.
        let declared: Vec<RoleSlotId> = team.slots.iter().map(|slot| slot.id.clone()).collect();
        if declared.is_empty() {
            return Err(self.deny(
                ApiErrorCode::UnsupportedCapability,
                "the pinned team template seats no role slot",
            ));
        }
        if team.slots.iter().any(|slot| {
            slot.model_chain
                .as_ref()
                .is_none_or(|chain| chain.rungs.is_empty())
        }) {
            return Err(self.deny(
                ApiErrorCode::UnsupportedCapability,
                "every role slot must declare a model route",
            ));
        }
        let roots = eligible_roots(&team);
        // A root leads: the seat that no handoff feeds is the one the work starts
        // at. Ordering the roots first is not required for correctness — every
        // seat is created either way — but it keeps the admitted seat, the one
        // committed atomically with the leases, an active one.
        let mut ordered: Vec<RoleSlotId> = declared
            .iter()
            .filter(|slot| roots.contains(*slot))
            .cloned()
            .collect();
        ordered.extend(
            declared
                .iter()
                .filter(|slot| !roots.contains(*slot))
                .cloned(),
        );
        let slot = ordered[0].clone();

        // A team run this task already has is the seat's home. Creating a second
        // one would give the same work two teams.
        let existing = state
            .with_store(|store| store.list_team_runs_for_task(project_id, admitted.task_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|(_, lifecycle)| !lifecycle.is_terminal())
            .map(|(id, _)| id);

        let team_run_id = existing.unwrap_or_else(TeamRunId::generate);
        let launch_key = IdempotencyKey::parse(&format!("{}-{}", key.as_str(), admitted.task_id))
            .map_err(|error| self.refuse_domain(&error))?;
        let agent_run_id = state
            .with_store(|store| store.get_receipt_by_key(&launch_key))
            .map_err(|error| self.refuse(&error))?
            .map(|receipt| match receipt.target {
                AggregateRef::AgentRun { agent_run_id } => Ok(agent_run_id),
                _ => Err(self.deny(
                    ApiErrorCode::IdempotencyConflict,
                    "the admission launch key already names another operation",
                )),
            })
            .transpose()?
            .unwrap_or_else(AgentRunId::generate);
        let binding_id = kontor_core::id::RuntimeBindingId::generate();
        let adapter = state
            .runtimes()
            .get(&admitted.runtime_kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "this daemon is not configured with the runtime the plan admitted",
                )
            })?;

        let intent = CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "operation": "scheduler_start",
            "task_id": admitted.task_id.to_string(),
        }))
        .map_err(|error| self.refuse_domain(&error))?;
        // Wrapped rather than serialized bare: a canonical document declares its
        // own generation, and the scheduler's decision type does not carry one.
        let evidence = CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1,
            "admitted": serde_json::to_value(admitted).unwrap_or(serde_json::Value::Null),
        }))
        .map_err(|error| self.refuse_domain(&error))?;
        let lease_expires = now
            .checked_add(jiff::SignedDuration::from_secs(LEASE_SECONDS))
            .unwrap_or(now);
        let holder = ExternalId::parse("kontord").map_err(|error| self.refuse_domain(&error))?;
        // Frozen once, here: every seat of this team run resolves its context
        // window against this copy, so the answer cannot drift as packs change.
        let seeded_roles: BTreeSet<kontor_core::id::RoleKey> = team
            .roles
            .iter()
            .map(|requirement| requirement.role.role.clone())
            .collect();
        let team_snapshot = TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION)
            .with_context_policy(
                self.pack
                    .context_policy_for(&workflow.snapshot.definition, &seeded_roles),
            )
            .map_err(|error| self.refuse_domain(&error))?;

        let commit = state.with_store(|store| {
            store.admit_candidate(&AdmissionCommit {
                admitted,
                serializes_with: &BTreeSet::new(),
                capacity: self.capacity,
                team_run: NewTeamRun {
                    id: team_run_id,
                    project_id,
                    task_id: admitted.task_id,
                    snapshot: team_snapshot.clone(),
                    created_at: now,
                },
                agent_run: NewAgentRun {
                    id: agent_run_id,
                    project_id,
                    team_run_id,
                    parent_agent_run_id: None,
                    role: slot.clone().into_role_key(),
                    account_profile_id: admitted.account_profile_id,
                    // Unbound on purpose: the session does not exist yet, and a
                    // binding written before the runtime issued one would be a
                    // claim about a session nothing created.
                    binding: None,
                    created_at: now,
                },
                launch: NewCommandIntent {
                    project_id,
                    receipt_id: CommandReceiptId::generate(),
                    idempotency_key: launch_key,
                    kind: CommandKind::LaunchRun,
                    target: AggregateRef::AgentRun { agent_run_id },
                    target_revision: AggregateRevision::INITIAL,
                    intent: intent.clone(),
                    payload: intent,
                    desired: Some(kontor_core::state::DesiredRunState::RunRequested),
                    not_before: now,
                    created_at: now,
                },
                admission_event_id: AdmissionEventId::generate(),
                module_lease_id: admitted
                    .module
                    .as_ref()
                    .map(|_| kontor_core::id::ResourceLeaseId::generate()),
                worktree_lease_id: None,
                holder_instance: holder,
                lease_expires_at: lease_expires,
                evidence,
                decided_at: now,
            })
        });
        commit.map_err(|error| self.refuse(&error))?;

        // Where this seat belongs is settled before the runtime is touched at
        // all. A placement that cannot be resolved stops here, with nothing
        // dispatched and nothing to undo.
        let task_root = self.task_root(project_id, admitted.task_id)?;
        let placement =
            self.resolve_placement(project_id, admitted.task_id, &ordered, &task_root)?;

        // A container is prepared *inside* the runtime's plane, so the plane has
        // to exist first. This is idempotent and re-attests a binding the
        // adapter already holds, so the cost of asking on every admission is one
        // readback — and the cost of not asking is a seat that can never be
        // materialized on a runtime whose plane nothing else creates.
        adapter
            .prepare_plane()
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let workspace = self
            .ensure_container(project_id, &placement, &task_root, adapter.as_ref())
            .await?;
        // The seat that owns this task's seats, opened once per epic. Every
        // delivery binding names it, so closing it orphans them all at once
        // instead of leaving each to be judged on its own liveness.
        let owner = self.ensure_epic_control_seat(project_id, &placement)?;
        for slot in &ordered {
            self.ensure_seat_binding(
                project_id,
                &placement,
                admitted.task_id,
                team_run_id,
                slot,
                owner,
            )?;
        }
        let existing_binding = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .and_then(|run| run.binding);
        let first = if let Some(binding) = existing_binding {
            StartedSeatDto {
                task_id: admitted.task_id,
                team_run_id: team_run_id.to_string(),
                agent_run_id: agent_run_id.to_string(),
                role_slot: slot.as_role_key().as_str().to_owned(),
                runtime_kind: binding.identity.runtime_kind,
                native_id: binding.identity.native_id.as_str().to_owned(),
                applied: AppliedDto::Unchanged,
            }
        } else {
            let authority = adapter
                .admit_launch(&AdmissionRequest {
                    slot: RoleSlotKey::new(team_run_id, slot.clone()),
                    agent_run_id,
                    binding_id,
                    replaces: None,
                    requested_at: now,
                })
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?
                .into_authority()
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            let context_policy = freeze_seat_context_policy(&adapter, &team_snapshot, &slot, now)
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            let model_rung = freeze_seat_model_rung(&team_snapshot, &slot)
                .map_err(|error| self.refuse_domain(&error))?;
            let outcome = adapter
                .launch(&authority.into_request(LaunchParts {
                    agent_run_id,
                    team_run_id,
                    role_slot_id: slot.clone(),
                    task_id: admitted.task_id,
                    binding_id,
                    placement: Some(LaunchPlacement::Container(workspace.clone())),
                    cwd: task_root.clone(),
                    account_profile_id: admitted.account_profile_id,
                    prompt:
                        slot_prompt(&slot, &roots).map_err(|error| self.refuse_domain(&error))?,
                    model_rung,
                    context_policy: context_policy.clone(),
                    requested_at: now,
                }))
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;

            let binding = RuntimeBinding {
                id: outcome.snapshot.binding_id(),
                agent_run_id,
                identity: outcome.snapshot.identity().clone(),
                bound_at: now,
            };
            state
                .with_store(|store| store.bind_agent_run(project_id, agent_run_id, &binding))
                .map_err(|error| self.refuse(&error))?;
            state
                .with_store(|store| {
                    store.record_run_context_policy(project_id, agent_run_id, &context_policy)
                })
                .map_err(|error| self.refuse(&error))?;
            // The launch read a native session back for this seat: it is
            // attached, and starting is itself an observed runtime event, so it
            // is the seat's first activity. Recording only attachment here would
            // make every seat read as stalled from the instant it started until
            // its first turn, because never-observed activity is deliberately
            // not a pass.
            //
            // This is a discrete event with its own native session id, not a
            // generic confirmation: a seat that starts and then does nothing
            // still stalls once the idle window closes.
            self.observe_seat(
                project_id,
                admitted.task_id,
                team_run_id,
                &slot,
                &SeatLivenessObservation {
                    attached_at: Some(now),
                    activity_at: Some(now),
                    ..SeatLivenessObservation::default()
                },
                now,
            )?;
            self.hold(&outcome.snapshot)?;
            StartedSeatDto {
                task_id: admitted.task_id,
                team_run_id: team_run_id.to_string(),
                agent_run_id: agent_run_id.to_string(),
                role_slot: slot.as_role_key().as_str().to_owned(),
                runtime_kind: binding.identity.runtime_kind,
                native_id: binding.identity.native_id.as_str().to_owned(),
                applied: AppliedDto::Created,
            }
        };

        let mut filled = vec![first];
        // The remaining declared slots join the team run admission already
        // committed. They are additional runs inside an admitted team rather than
        // additional admissions: the leases, the capacity and the launch intent
        // were decided once, for the task.
        let seating = Seating {
            project_id,
            admitted,
            team_run_id,
            roots: &roots,
            adapter: &adapter,
            container: &workspace,
            cwd: &task_root,
            now,
        };
        for role in ordered.iter().skip(1) {
            filled.push(self.fill_slot(&seating, role).await?);
        }
        Ok(filled)
    }

    /// Resolve where this task's seats belong, before anything is started.
    ///
    /// Every accepted seat is placed through the Operational topology, so this
    /// is total: it answers with a node or it refuses. The task's node is the
    /// locator, and every check below is a question about *where*, answered from
    /// Kontor's own rows — a node that hosts no session, a working directory
    /// that is not the bound one, or a slot that already holds a live seat all
    /// stop here as `placement_blocked`, with nothing dispatched.
    ///
    /// **There is deliberately no escape for a project that has no topology
    /// yet.** An earlier revision answered `Ok(None)` there and let admission
    /// fall back to a TeamRun-keyed task workspace. That escape existed only
    /// because nothing wrote topology nodes; now [`Self::ensure_task_node`]
    /// does, seeding the project's revision and creating the chain on first
    /// admission. Keeping the escape would mean keeping a second, TeamRun-keyed
    /// way to place a production seat, which is the whole defect OP-02 removes.
    ///
    /// The worry the escape answered is still answered — a project that never
    /// selected a topology is given one rather than refused, so no task becomes
    /// unrunnable by not having been configured. What changed is *how*: by
    /// seeding, not by placing the seat somewhere unmodelled.
    ///
    /// Nothing here repairs a disagreement. Rewriting either side to match the
    /// other is what turns "these two disagree about where the work is" into
    /// "the work is now in two places".
    fn resolve_placement(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        declared: &[RoleSlotId],
        worktree: &kontor_runtime::workspace::WorkspaceRoot,
    ) -> Result<SessionTopologyNode, ApiError> {
        let state = self.state()?;
        let node = self.ensure_task_node(project_id, task_id)?;

        // The kind's capabilities come from the pinned specification revision,
        // never from the kind's name: the vocabulary is data a revision owns,
        // and a daemon holding its own copy of it is one no revision can
        // correct.
        let spec = state
            .with_store(|store| {
                store.get_topology_spec(project_id, node.topology.spec_id, node.topology.version)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the node's pinned topology revision is not published in this project",
                )
            })?;
        let kind = spec
            .node_kinds
            .iter()
            .find(|declared| declared.kind == node.kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the node's kind is not declared by its pinned topology revision",
                )
            })?;
        if !kind
            .projection_capabilities
            .contains(&NodeProjectionCapability::SessionHost)
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the task's node kind does not host sessions",
            ));
        }

        // A child node's container lives below its parent's, so a node with no
        // parent is a seat with nowhere to be.
        //
        // Whether that parent *holds* a container is deliberately not asked
        // here. Preparation walks the lineage from the root down and presents
        // each level's exact binding to the next, so by the time a child is
        // created its parent is bound or the whole preparation failed loudly.
        // Asking before preparation would refuse the ordinary first admission of
        // an epic; asking after it would be asking whether the call that just
        // returned had returned.
        if node.parent_id.is_none() {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "a task's node is placed below a parent and this one has none",
            ));
        }

        // Where the node is bound, compared against where the seat is about to
        // work. A container bound elsewhere is not corrected to match the
        // request; the disagreement is reported.
        if let Some(bound) = state
            .with_store(|store| store.get_topology_node_container(project_id, node.id))
            .map_err(|error| self.refuse(&error))?
            && bound
                .canonical_cwd
                .as_ref()
                .is_none_or(|cwd| cwd.as_str() != worktree.as_str())
        {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the node's bound container works in another directory than this task",
            ));
        }

        // One live seat per `(node, slot)`. A second would give one role two
        // sessions, and the runtime's own admission ledger cannot see the first
        // one across a restart.
        let held = state
            .with_store(|store| store.list_seat_bindings(project_id, node.id))
            .map_err(|error| self.refuse(&error))?;
        for slot in declared {
            if held
                .iter()
                .any(|binding| &binding.role_slot_id == slot && binding.is_non_terminal())
            {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a live seat already holds one of this team's role slots on that node",
                ));
            }
        }
        Ok(node)
    }

    /// The exact Operational topology revision this project places against,
    /// publishing and selecting the bundled one the first time it is asked for.
    ///
    /// Seeding here rather than at project creation is what gives every project
    /// a topology, including the ones created before there was one to give.
    /// Publication is by `(spec_id, version)` and selection is an upsert, so a
    /// project that already chose a revision keeps it: this never re-points a
    /// project at the bundled data.
    fn project_topology(&self, project_id: ProjectId) -> Result<TopologySnapshot, ApiError> {
        let state = self.state()?;
        if let Some(selected) = state
            .with_store(|store| store.get_project_topology_default(project_id))
            .map_err(|error| self.refuse(&error))?
        {
            return Ok(selected.topology);
        }

        let now = kontor_api::now();
        let spec =
            self.domain.topology_specs.first().ok_or_else(|| {
                self.deny(ApiErrorCode::Unavailable, "this build ships no topology")
            })?;
        let catalog = self.domain.role_catalogs.first().ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "this build ships no role catalog",
            )
        })?;
        let stamp = Shareability::default_for(ShareabilityTier::ProjectKnowledge)
            .map_err(|error| self.refuse_domain(&error))?;
        let canonical_hash = if let Some(published) = state
            .with_store(|store| store.get_topology_spec(project_id, spec.spec_id, spec.version))
            .map_err(|error| self.refuse(&error))?
        {
            published
                .canonicalize()
                .map_err(|error| self.refuse_domain(&error))?
                .hash()
                .clone()
        } else {
            state
                .with_store(|store| store.publish_topology_spec(project_id, spec, &stamp, now))
                .map_err(|error| self.refuse(&error))?
        };
        if state
            .with_store(|store| store.get_role_catalog(catalog.catalog_id, catalog.version))
            .map_err(|error| self.refuse(&error))?
            .is_none()
        {
            state
                .with_store(|store| store.publish_role_catalog(catalog, &stamp, now))
                .map_err(|error| self.refuse(&error))?;
        }
        let topology = TopologySnapshot {
            spec_id: spec.spec_id,
            version: spec.version,
            canonical_hash,
        };
        state
            .with_store(|store| {
                store.set_project_topology_default(&ProjectTopologyDefault {
                    project_id,
                    topology: topology.clone(),
                    selected_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(topology)
    }

    /// The node one delivery task is placed on, creating the chain above it the
    /// first time the task is admitted.
    ///
    /// The chain is project root, then the task's epic, then the task itself.
    /// Every kind comes from data — the specification's own `root_kind` and the
    /// seeded delivery binding — because several kinds in the bundled vocabulary
    /// are `native_child` session hosts below an epic, so which one serves a task
    /// is a choice the data makes and not one derivable from capabilities.
    ///
    /// Idempotent by construction: each level is looked up before it is created,
    /// and the task level is unique per `(project, task)` in the schema, so a
    /// concurrent admission loses the insert rather than producing a second node.
    fn ensure_task_node(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> Result<SessionTopologyNode, ApiError> {
        let state = self.state()?;
        if let Some(node) = state
            .with_store(|store| store.get_task_topology_node(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
        {
            return Ok(node);
        }

        // A task outside an epic has no place in this topology: the delivery
        // kind is declared below the epic kind, and inventing an epic for it
        // would be Kontor deciding what work an operator grouped together.
        //
        // Every admission reaches this through an epic-scoped start, so no
        // caller can currently arrive here without one. It is kept as the guard
        // that says so rather than as an `expect`: the day a second admission
        // route exists, this refuses instead of placing the work at a guess.
        let epic_id = self
            .task_row(project_id, task_id)?
            .mini_project_id
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "this task belongs to no epic, so it has no place in the session topology",
                )
            })?;
        let topology = self.project_topology(project_id)?;
        let spec = state
            .with_store(|store| {
                store.get_topology_spec(project_id, topology.spec_id, topology.version)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the selected topology revision is not published in this project",
                )
            })?;
        let now = kontor_api::now();

        // An epic-scoped node may only carry the revision its epic is pinned to,
        // so the pin has to exist before the node does. A pin already there is
        // never rewritten: repinning an epic to a different revision would
        // silently move every node already placed under it.
        match state
            .with_store(|store| store.get_mini_project_topology(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
        {
            Some(pinned) if pinned.topology != topology => {
                return Err(self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "this epic is pinned to another topology revision than the project selects",
                ));
            }
            Some(_) => {}
            None => state
                .with_store(|store| {
                    store.pin_mini_project_topology(&MiniProjectTopologySnapshot {
                        project_id,
                        mini_project_id: epic_id,
                        topology: topology.clone(),
                        pinned_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?,
        }

        // Two reads, because the listing is scoped: the project root carries no
        // epic and every epic-scoped node carries exactly this one.
        let unscoped = state
            .with_store(|store| store.list_topology_nodes(project_id, None))
            .map_err(|error| self.refuse(&error))?;
        let scoped = state
            .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
            .map_err(|error| self.refuse(&error))?;

        let root = self.ensure_node(
            unscoped
                .iter()
                .find(|node| node.kind == spec.root_kind && node.parent_id.is_none()),
            NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id,
                mini_project_id: None,
                topology: topology.clone(),
                kind: spec.root_kind.clone(),
                parent_id: None,
                task_id: None,
                created_at: now,
            },
        )?;
        let epic = self.ensure_node(
            scoped
                .iter()
                .find(|node| node.kind == self.domain.delivery.epic_kind),
            NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id,
                mini_project_id: Some(epic_id),
                topology: topology.clone(),
                kind: self.domain.delivery.epic_kind.clone(),
                parent_id: Some(root.id),
                task_id: None,
                created_at: now,
            },
        )?;
        // The epic's control plane, which is what a delivery seat belongs to.
        // Created with the task's node rather than lazily beside it: the seat
        // that owns this task's seats has to exist before they can name it.
        self.ensure_node(
            scoped
                .iter()
                .find(|node| node.kind == self.domain.delivery.control_kind),
            NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id,
                mini_project_id: Some(epic_id),
                topology: topology.clone(),
                kind: self.domain.delivery.control_kind.clone(),
                parent_id: Some(epic.id),
                task_id: None,
                created_at: now,
            },
        )?;

        self.ensure_node(
            None,
            NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id,
                mini_project_id: Some(epic_id),
                topology,
                kind: self.domain.delivery.task_kind.clone(),
                parent_id: Some(epic.id),
                task_id: Some(task_id),
                created_at: now,
            },
        )
    }

    /// The seat that owns one epic's delivery seats, opened once per epic.
    ///
    /// OP-REQ-039 derives orphanhood from the *owner's* Kontor lifecycle, so
    /// every delivery seat needs an exact owning seat to name. That owner is a
    /// control-plane seat: an epic node materializes as a native root and hosts
    /// no sessions, so it cannot be the owner itself.
    ///
    /// A released owner is not reused and not repaired. Releasing retires the
    /// row, which frees the `(node, role slot)` key, so a reopened epic opens a
    /// fresh control seat while the seats the old one owned stay orphaned —
    /// which is true: their owner is gone.
    fn ensure_epic_control_seat(
        &self,
        project_id: ProjectId,
        task_node: &SessionTopologyNode,
    ) -> Result<Option<SeatBindingId>, ApiError> {
        let state = self.state()?;
        let Some(epic_id) = task_node.mini_project_id else {
            return Ok(None);
        };
        let Some(control) = state
            .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|node| node.kind == self.domain.delivery.control_kind)
        else {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "this epic has no control plane for its delivery seats to belong to",
            ));
        };
        let slot = self.control_slot()?;
        if let Some(held) = state
            .with_store(|store| store.list_seat_bindings(project_id, control.id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|binding| binding.role_slot_id == slot && binding.is_non_terminal())
        {
            return Ok(Some(held.id));
        }
        let role = self.catalog_role_for_code(&self.domain.delivery.control_role_code)?;
        let now = kontor_api::now();
        let deadline = now
            .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
            .unwrap_or(now);
        let opened = state
            .with_store(|store| {
                store.create_seat_binding(&NewSeatBinding {
                    id: SeatBindingId::generate(),
                    project_id,
                    topology_node_id: control.id,
                    role_slot_id: slot.clone(),
                    role: role.clone(),
                    // The control seat serves the epic, not one delivery. Naming
                    // a task here would make it look like one task's seat and
                    // put it in that task's progress evidence.
                    task_id: None,
                    team_run_id: None,
                    attach_deadline: deadline,
                    // The control seat is the root of the ownership chain, and a
                    // root is not an orphan.
                    parent_seat_binding_id: None,
                    created_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(Some(opened.id))
    }

    /// Record what was observed about one seat's own binding.
    ///
    /// The counterpart to the seat-binding writer: without this, `last_attached_at`
    /// and `last_activity_at` stay null for the life of every seat, so every
    /// binding passes its attachment deadline and `certify_task_progress` refuses
    /// work that is plainly running. The rows exist to be written, and this is
    /// what writes them.
    ///
    /// **What counts as what is the requirement, not a detail.** A readback
    /// proves the seat is *attached*; only an observed runtime event or turn
    /// position proves *activity* (OP-REQ-039). Calling this with `activity_at`
    /// from a generic confirmation would be exactly the shortcut the negative
    /// proofs forbid, and would make a hung seat read as healthy forever.
    ///
    /// A seat with no binding is not an error. A team run whose slots the seeded
    /// delivery data does not spell has no rows to observe, and observing
    /// nothing is the honest outcome rather than a refused settle.
    fn observe_seat(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        team_run_id: TeamRunId,
        slot: &RoleSlotId,
        observation: &SeatLivenessObservation,
        observed_at: Timestamp,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let Some(node) = state
            .with_store(|store| store.get_task_topology_node(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(());
        };
        let Some(binding) = state
            .with_store(|store| store.list_seat_bindings(project_id, node.id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|binding| {
                &binding.role_slot_id == slot
                    && binding.team_run_id == Some(team_run_id)
                    && binding.is_non_terminal()
            })
        else {
            return Ok(());
        };
        state
            .with_store(|store| {
                store.observe_seat_binding(project_id, binding.id, observation, observed_at)
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(())
    }

    /// Release the seat that owns one epic's delivery seats.
    ///
    /// The live half of parent-derived orphanhood. Until something closes the
    /// owner, every delivery seat reads as owned by an open seat and nothing can
    /// ever conclude `Orphaned` — the derivation would be correct and never
    /// exercised. Closing the epic is what closes its control plane.
    ///
    /// Only the owner is touched. The delivery seats are not rewritten to say
    /// they are orphans; they are read as orphans, from their owner's row, which
    /// is the difference between recording a conclusion and deriving one.
    fn release_epic_control_seat(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let Some(control) = state
            .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|node| node.kind == self.domain.delivery.control_kind)
        else {
            // An epic nothing was ever admitted under has no control plane, and
            // closing it is not the moment to build one.
            return Ok(());
        };
        let slot = self.control_slot()?;
        let now = kontor_api::now();
        for binding in state
            .with_store(|store| store.list_seat_bindings(project_id, control.id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .filter(|binding| binding.role_slot_id == slot && binding.is_non_terminal())
        {
            state
                .with_store(|store| {
                    store.observe_seat_binding(
                        project_id,
                        binding.id,
                        &SeatLivenessObservation {
                            released_at: Some(now),
                            ..SeatLivenessObservation::default()
                        },
                        now,
                    )
                })
                .map_err(|error| self.refuse(&error))?;
        }
        Ok(())
    }

    /// The role slot an epic's control seat occupies.
    fn control_slot(&self) -> Result<RoleSlotId, ApiError> {
        let code = self.domain.delivery.control_role_code.as_str();
        RoleSlotId::parse(&code.to_ascii_lowercase()).map_err(|error| self.refuse_domain(&error))
    }

    /// Return the node already there, or create the requested one.
    fn ensure_node(
        &self,
        found: Option<&SessionTopologyNode>,
        request: NewSessionTopologyNode,
    ) -> Result<SessionTopologyNode, ApiError> {
        if let Some(node) = found {
            return Ok(node.clone());
        }
        let state = self.state()?;
        state
            .with_store(|store| store.create_topology_node(&request))
            .map_err(|error| self.refuse(&error))
    }

    /// Record the `(topology node, role slot)` seat this admission is filling.
    ///
    /// The deadline is fixed here and never recomputed, which is the whole
    /// point of persisting it: derived from `created_at` at read time it would
    /// move every time the row was read (OP-REQ-039a).
    ///
    /// A role slot the seeded delivery data does not spell as a standard code
    /// gets no binding, and says so. Recording one under a guessed standard role
    /// would be worse evidence than recording nothing: the whole value of the
    /// row is that the code in it came from a published catalog.
    ///
    /// Skipping rather than refusing is deliberate. The correspondence is seeded
    /// data an operator's own team templates can outrun, and a slot with no
    /// entry is a gap in that data — not a reason the work cannot run.
    fn ensure_seat_binding(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        task_id: TaskId,
        team_run_id: TeamRunId,
        slot: &RoleSlotId,
        parent: Option<SeatBindingId>,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let Some(role) = self.catalog_role(slot)? else {
            tracing::warn!(
                role_slot = %slot.as_str(),
                "no seeded standard role for this slot, so its seat is not recorded in the topology"
            );
            return Ok(());
        };
        let held = state
            .with_store(|store| store.list_seat_bindings(project_id, node.id))
            .map_err(|error| self.refuse(&error))?;
        if held
            .iter()
            .any(|binding| &binding.role_slot_id == slot && binding.is_non_terminal())
        {
            return Ok(());
        }
        let now = kontor_api::now();
        let deadline = now
            .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
            .unwrap_or(now);
        state
            .with_store(|store| {
                store.create_seat_binding(&NewSeatBinding {
                    id: SeatBindingId::generate(),
                    project_id,
                    topology_node_id: node.id,
                    role_slot_id: slot.clone(),
                    role: role.clone(),
                    task_id: Some(task_id),
                    team_run_id: Some(team_run_id),
                    attach_deadline: deadline,
                    // The exact owning seat, so orphanhood is read from that
                    // row's lifecycle rather than guessed. `None` here would
                    // make every delivery seat a root, and a root is never
                    // orphaned however dead its epic is.
                    parent_seat_binding_id: parent,
                    created_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(())
    }

    /// The standard catalog role one Foundation slot is recorded under, when the
    /// seeded delivery data spells one for it.
    fn catalog_role(&self, slot: &RoleSlotId) -> Result<Option<CatalogRoleRef>, ApiError> {
        let Some(code) = self.domain.delivery.role_code(slot.as_role_key()) else {
            return Ok(None);
        };
        self.catalog_role_for_code(code).map(Some)
    }

    /// The pinned catalog projection of one standard role code.
    fn catalog_role_for_code(
        &self,
        code: &kontor_core::id::RoleCode,
    ) -> Result<CatalogRoleRef, ApiError> {
        let catalog = self.domain.role_catalogs.first().ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "this build ships no role catalog",
            )
        })?;
        // A code the catalog does not declare *is* a refusal rather than a gap:
        // the seeded data contradicts itself, and a seat placed under it would
        // be recorded against a role no revision defines.
        let entry = catalog.role(code).ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the seeded delivery binding names a role the catalog does not declare",
            )
        })?;
        Ok(CatalogRoleRef {
            catalog_id: catalog.catalog_id,
            catalog_revision: catalog.version,
            role_code: entry.role_code.clone(),
            standard_title: entry.standard_title.clone(),
            custom_display_name: None,
        })
    }

    /// Make one node's native container exist, and persist what came back.
    ///
    /// The chain is prepared from the root down rather than recursively from the
    /// leaf, because a `native_child` must present the *exact* parent binding and
    /// the only way to have one is to have already prepared the parent. Depth is
    /// bounded by the specification's own kind graph.
    ///
    /// Every preparation carries the native id Kontor already holds for that
    /// node, when it holds one. That is the whole restart contract: a daemon
    /// restart destroys the adapter's ledger while the native container carries
    /// on existing, and the persisted id is the only way back to it.
    async fn ensure_container(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        cwd: &kontor_runtime::workspace::WorkspaceRoot,
        adapter: &dyn RuntimeAdapter,
    ) -> Result<ContainerBindingSnapshot, ApiError> {
        let state = self.state()?;
        let spec = state
            .with_store(|store| {
                store.get_topology_spec(project_id, node.topology.spec_id, node.topology.version)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the node's pinned topology revision is not published in this project",
                )
            })?;

        // Walk to the root first, then build downwards. Reading the ancestry
        // from stored rows rather than from the request is what stops a child
        // being placed below "whatever the adapter last touched".
        let mut known = state
            .with_store(|store| store.list_topology_nodes(project_id, None))
            .map_err(|error| self.refuse(&error))?;
        if let Some(epic_id) = node.mini_project_id {
            known.extend(
                state
                    .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
                    .map_err(|error| self.refuse(&error))?,
            );
        }
        let mut lineage = vec![node.clone()];
        while let Some(parent_id) = lineage.last().and_then(|node| node.parent_id) {
            let parent = known
                .iter()
                .find(|known| known.id == parent_id)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the node's parent is not in this project's topology",
                    )
                })?;
            lineage.push(parent.clone());
        }
        lineage.reverse();

        let mut parent: Option<ContainerBinding> = None;
        let mut prepared = None;
        for level in &lineage {
            let capabilities = spec
                .node_kinds
                .iter()
                .find(|declared| declared.kind == level.kind)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "a node's kind is not declared by its pinned topology revision",
                    )
                })?
                .projection_capabilities
                .clone();
            let bound = state
                .with_store(|store| store.get_topology_node_container(project_id, level.id))
                .map_err(|error| self.refuse(&error))?;
            let leaf = level.id == node.id;
            // Only a `native_child` is created below anything. A `native_root`
            // is its own root even when it sits below another node logically —
            // handing it the ancestor's binding would be asking the runtime to
            // nest something it declared unnestable.
            let projection = ContainerProjection::resolve(&capabilities)
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            let request = ContainerRequest {
                container_binding_id: ContainerBindingId::generate(),
                topology_node_id: level.id,
                topology: level.topology.clone(),
                capabilities,
                display_name: self.container_name(&spec, level)?,
                parent: match projection {
                    ContainerProjection::NativeChild => parent.clone(),
                    ContainerProjection::NativeRoot | ContainerProjection::LogicalOnly => None,
                },
                // Only the seat's own container needs a working directory. An
                // epic or a project root is a place to put things, not a tree to
                // edit in.
                cwd: leaf.then(|| cwd.clone()),
                bound_native_id: bound.map(|binding| binding.identity.native_id),
                task_id: level.task_id,
                team_run_id: None,
                requested_at: kontor_api::now(),
            };
            let outcome = adapter
                .prepare_container(&request)
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            outcome
                .snapshot
                .ensure_node(level.id)
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            outcome
                .snapshot
                .ensure_correlated()
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            self.bind_container(project_id, level.id, &outcome.snapshot)?;
            parent = Some(outcome.snapshot.binding.clone());
            prepared = Some(outcome.snapshot);
        }
        prepared.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the node has no lineage to prepare a container along",
            )
        })
    }

    /// The display name one node's container carries, from its kind's template.
    fn container_name(
        &self,
        spec: &kontor_core::spec::ProjectSessionTopologySpec,
        node: &SessionTopologyNode,
    ) -> Result<ExternalName, ApiError> {
        let template = spec
            .node_kinds
            .iter()
            .find(|declared| declared.kind == node.kind)
            .map(|declared| declared.name_template.as_str())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a node's kind is not declared by its pinned topology revision",
                )
            })?;
        ExternalName::parse(&format!("{template} · {}", node.id))
            .map_err(|error| self.refuse_domain(&error))
    }

    /// Persist the native container a runtime read back for one node.
    fn bind_container(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        snapshot: &ContainerBindingSnapshot,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let observed_kind = match snapshot.binding.projection {
            ContainerProjection::NativeChild => ObservedContainerKind::Workspace,
            _ => ObservedContainerKind::Project,
        };
        let canonical_cwd = snapshot
            .binding
            .root
            .as_ref()
            .map(|root| ExternalName::parse(root.as_str()))
            .transpose()
            .map_err(|error| self.refuse_domain(&error))?;
        let binding_id = ExternalId::parse(&snapshot.binding.id.to_string())
            .map_err(|error| self.refuse_domain(&error))?;
        state
            .with_store(|store| {
                store.bind_topology_node_container(&NewNativeContainerBinding {
                    topology_node_id,
                    project_id,
                    container_binding_id: binding_id.clone(),
                    identity: snapshot.binding.identity.clone(),
                    observed_kind,
                    canonical_cwd: canonical_cwd.clone(),
                    observed_at: snapshot.binding.bound_at,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        Ok(())
    }

    /// Fill one more declared slot inside a team run admission already committed.
    async fn fill_slot(
        &self,
        seating: &Seating<'_>,
        slot: &RoleSlotId,
    ) -> Result<StartedSeatDto, ApiError> {
        let Seating {
            project_id,
            admitted,
            team_run_id,
            roots,
            adapter,
            container,
            cwd,
            now,
        } = *seating;
        let state = self.state()?;
        let realm_id = state.realm_id();
        let existing = state
            .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|seat| &seat.role == slot.as_role_key());
        if let Some(seat) = existing.as_ref()
            && let (Some(kind), Some(native)) = (seat.runtime_kind.clone(), seat.native_id.clone())
        {
            return Ok(StartedSeatDto {
                task_id: admitted.task_id,
                team_run_id: team_run_id.to_string(),
                agent_run_id: seat.agent_run_id.to_string(),
                role_slot: seat.role.as_str().to_owned(),
                runtime_kind: kind,
                native_id: native.as_str().to_owned(),
                applied: AppliedDto::Unchanged,
            });
        }

        let agent_run_id = existing
            .as_ref()
            .map_or_else(AgentRunId::generate, |seat| seat.agent_run_id);
        let binding_id = kontor_core::id::RuntimeBindingId::generate();
        if existing.is_none() {
            state
                .with_store(|store| {
                    store.create_agent_run(&NewAgentRun {
                        id: agent_run_id,
                        project_id,
                        team_run_id,
                        parent_agent_run_id: None,
                        role: slot.clone().into_role_key(),
                        account_profile_id: admitted.account_profile_id,
                        binding: None,
                        created_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }

        // The seat resolves its context window against the team run's own frozen
        // inputs, read back from storage rather than recomposed from whatever
        // the profile pack says now.
        let team_snapshot = state
            .with_store(|store| store.get_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.refuse_domain(&kontor_core::DomainError::invalid(
                    "StartSeat",
                    "the team run this seat joins does not exist",
                ))
            })?
            .snapshot;

        // Neither the plane nor the container is prepared again here.
        // `fill_slot` is reached from `seat` and from nowhere else, and `seat`
        // prepares both immediately before the first slot — so a second call
        // could never observe a different answer, and a line no test can kill is
        // worse than no line.
        let authority = adapter
            .admit_launch(&AdmissionRequest {
                slot: RoleSlotKey::new(team_run_id, slot.clone()),
                agent_run_id,
                binding_id,
                replaces: None,
                requested_at: now,
            })
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?
            .into_authority()
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
        let context_policy = freeze_seat_context_policy(adapter, &team_snapshot, slot, now)
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
        let model_rung = freeze_seat_model_rung(&team_snapshot, slot)
            .map_err(|error| self.refuse_domain(&error))?;
        let outcome = adapter
            .launch(&authority.into_request(LaunchParts {
                agent_run_id,
                team_run_id,
                role_slot_id: slot.clone(),
                task_id: admitted.task_id,
                binding_id,
                placement: Some(LaunchPlacement::Container(container.clone())),
                cwd: cwd.clone(),
                account_profile_id: admitted.account_profile_id,
                prompt: slot_prompt(slot, roots).map_err(|error| self.refuse_domain(&error))?,
                model_rung,
                context_policy: context_policy.clone(),
                requested_at: now,
            }))
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
        let binding = RuntimeBinding {
            id: outcome.snapshot.binding_id(),
            agent_run_id,
            identity: outcome.snapshot.identity().clone(),
            bound_at: now,
        };
        state
            .with_store(|store| store.bind_agent_run(project_id, agent_run_id, &binding))
            .map_err(|error| self.refuse(&error))?;
        state
            .with_store(|store| {
                store.record_run_context_policy(project_id, agent_run_id, &context_policy)
            })
            .map_err(|error| self.refuse(&error))?;
        // As in `seat`: the launch read a session back, and starting is the
        // seat's first observed activity.
        self.observe_seat(
            project_id,
            admitted.task_id,
            team_run_id,
            slot,
            &SeatLivenessObservation {
                attached_at: Some(now),
                activity_at: Some(now),
                ..SeatLivenessObservation::default()
            },
            now,
        )?;
        self.hold(&outcome.snapshot)?;
        Ok(StartedSeatDto {
            task_id: admitted.task_id,
            team_run_id: team_run_id.to_string(),
            agent_run_id: agent_run_id.to_string(),
            role_slot: slot.as_role_key().as_str().to_owned(),
            runtime_kind: binding.identity.runtime_kind.clone(),
            native_id: binding.identity.native_id.as_str().to_owned(),
            applied: AppliedDto::Created,
        })
    }

    /// Move one task through a legal transition, with the evidence it demands.
    fn task_lifecycle(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        task: &kontor_core::repository::Task,
        request: &LifecycleRequest,
    ) -> Result<LifecycleOutcomeDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "lifecycle",
            "action": action_name(request.action),
            "task_id": task.id.to_string(),
            "expected_revision": request.expected_revision.get(),
            "reason": request.reason.as_str(),
            "evidence": request.evidence,
        }))?;
        let target = AggregateRef::Task { task_id: task.id };
        // A replayed transition answers from the task rather than attempting the
        // move again: the second attempt would be judged against a revision the
        // first one already advanced past.
        if self.replayed(key, &intent, Some(&target))?.is_some() {
            let current = state
                .with_store(|store| store.get_task(project_id, task.id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "no such task exists in this project",
                    )
                })?;
            return Ok(LifecycleOutcomeDto {
                realm_id: state.realm_id(),
                target: current.id.to_string(),
                state: current.state.as_str().to_owned(),
                revision: current.revision,
                receipt_id: self
                    .replayed(key, &intent, Some(&target))?
                    .map(|receipt| receipt.id.to_string())
                    .unwrap_or_default(),
            });
        }
        // Only a resume records `resume_task`: that receipt is *consumed* as the
        // authority to leave a held state, and a block that shared the kind could
        // be cited as the permission to undo itself.
        let kind = if matches!(
            request.action,
            LifecycleAction::Resume | LifecycleAction::ReopenTask
        ) {
            CommandKind::ResumeTask
        } else {
            CommandKind::TransitionTask
        };
        let receipt = self.record(key, project_id, kind, target, task.revision, &intent)?;

        let to = match request.action {
            LifecycleAction::Block => TaskState::Blocked,
            LifecycleAction::Resume => TaskState::Ready,
            LifecycleAction::CompleteTask => TaskState::Done,
            LifecycleAction::ReopenTask => TaskState::Ready,
            LifecycleAction::CloseEpic | LifecycleAction::ReopenEpic => {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "an epic action does not name a task",
                ));
            }
        };
        let artifacts: BTreeSet<kontor_core::id::ArtifactKey> = request
            .evidence
            .iter()
            .map(|key| kontor_core::id::ArtifactKey::parse(key))
            .collect::<Result<_, _>>()
            .map_err(|error| self.refuse_domain(&error))?;
        let completed = state
            .with_store(|store| store.get_active_task_workflow(project_id, task.id))
            .map_err(|error| self.refuse(&error))?
            .map(|workflow| {
                workflow
                    .snapshot
                    .definition
                    .phases
                    .iter()
                    .map(|phase| phase.id.clone())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        // A task that ran a team closes with that team's own certificate, derived
        // here from the frozen template's declared slots. It is not fabricated and
        // it is not accepted from the caller: an uncertified team is a refusal
        // naming what is still outstanding, which is the settlement the operator
        // has not done yet rather than a missing feature.
        let team_closure = if to == TaskState::Done {
            match self.task_team_closure(project_id, task.id)? {
                Ok(closure) => closure,
                Err(pending) => {
                    return Err(self.deny(ApiErrorCode::UnsupportedCapability, pending));
                }
            }
        } else {
            TaskTeamClosure::NoTeam
        };

        let moved = state
            .with_store(|store| {
                store.transition_task(&TaskTransitionRequest {
                    project_id,
                    task_id: task.id,
                    expected_revision: task.revision,
                    to,
                    resume_receipt: matches!(
                        request.action,
                        LifecycleAction::Resume | LifecycleAction::ReopenTask
                    )
                    .then_some(receipt),
                    run_outcome: None,
                    produced_artifacts: artifacts.clone(),
                    completed_phases: if to == TaskState::Done {
                        completed.clone()
                    } else {
                        BTreeSet::new()
                    },
                    team_closure: team_closure.clone(),
                    occurred_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        state.signals().appended();
        Ok(LifecycleOutcomeDto {
            realm_id: state.realm_id(),
            target: moved.id.to_string(),
            state: moved.state.as_str().to_owned(),
            revision: moved.revision,
            receipt_id: receipt.to_string(),
        })
    }

    /// Close or re-open one epic, against every task it carries.
    ///
    /// An epic is closed when its work is: there is no separate terminal column
    /// on a goal, so closure is the statement that every task under it reached a
    /// terminal state. Refusing before that is what stops "close the epic" from
    /// becoming a way to declare unfinished work done.
    fn epic_lifecycle(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic: &kontor_core::repository::MiniProject,
        request: &LifecycleRequest,
    ) -> Result<LifecycleOutcomeDto, ApiError> {
        let state = self.state()?;
        if epic.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the epic moved since the caller read it",
                )
                .with_revision(Some(epic.revision)));
        }
        let tasks = state
            .with_store(|store| store.list_epic_tasks(project_id, epic.id))
            .map_err(|error| self.refuse(&error))?;
        if request.action == LifecycleAction::CloseEpic {
            if tasks.is_empty() {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "an epic with no tasks has nothing to close",
                ));
            }
            if let Some(open) = tasks.iter().find(|task| !task.state.is_terminal()) {
                let _ = open;
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "every task in the epic must be terminal before it closes",
                ));
            }
            for task in &tasks {
                let runs = state
                    .with_store(|store| store.list_team_runs_for_task(project_id, task.id))
                    .map_err(|error| self.refuse(&error))?;
                if runs.iter().any(|(_, lifecycle)| !lifecycle.is_terminal()) {
                    return Err(self.deny(
                        ApiErrorCode::RevisionConflict,
                        "a team run in this epic has not closed",
                    ));
                }
            }
        }
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "lifecycle",
            "action": action_name(request.action),
            "epic_id": epic.id.to_string(),
            "expected_revision": request.expected_revision.get(),
            "reason": request.reason.as_str(),
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic.id,
        };
        let receipt = if let Some(existing) = self.replayed(key, &intent, Some(&target))? {
            existing.id
        } else {
            self.record(
                key,
                project_id,
                CommandKind::TransitionEpic,
                target,
                epic.revision,
                &intent,
            )?
        };
        // A closed epic's control seat is released, which is what makes its
        // delivery seats orphans rather than seats that merely stopped being
        // observed. It runs after the receipt so the release cites a transition
        // that is already recorded, and it is idempotent: `released_at` is only
        // ever filled in once.
        if request.action == LifecycleAction::CloseEpic {
            self.release_epic_control_seat(project_id, epic.id)?;
        }

        state.signals().appended();
        Ok(LifecycleOutcomeDto {
            realm_id: state.realm_id(),
            target: epic.id.to_string(),
            state: if request.action == LifecycleAction::CloseEpic {
                "closed".to_owned()
            } else {
                "open".to_owned()
            },
            revision: epic.revision,
            receipt_id: receipt.to_string(),
        })
    }
}

/// One profile pack, as the catalogue advertises it.
fn pack_dto(
    pack: &ProfilePackSpec,
    source: &str,
    document_hash: Option<String>,
    applied: AppliedDto,
) -> ProfilePackDto {
    ProfilePackDto {
        pack_id: pack.pack_id.as_str().to_owned(),
        version: pack.version,
        source: source.to_owned(),
        document_hash,
        categories: pack
            .manifest
            .iter()
            .map(|entry| entry.category.as_str().to_owned())
            .collect(),
        team_templates: pack
            .teams
            .iter()
            .map(|team| RevisionRefDto {
                id: team.template_id.to_string(),
                version: team.version,
            })
            .collect(),
        applied,
    }
}

/// One pinned trigger revision, as an operator reads it back.
fn trigger_dto(spec: &TriggerSpec) -> TriggerSpecDto {
    TriggerSpecDto {
        trigger: spec.id.as_str().to_owned(),
        version: spec.version,
        source_kind: spec.source_kind.as_str().to_owned(),
        source_connection: spec.source_connection.as_str().to_owned(),
        event_schema: RevisionRefDto {
            id: spec.event_schema.as_str().to_owned(),
            version: spec.event_schema_version,
        },
        filter_pointers: spec
            .filter
            .iter()
            .map(|clause| clause.pointer.as_str().to_owned())
            .collect(),
        dedup_pointers: spec
            .dedup
            .pointers
            .iter()
            .map(|pointer| pointer.as_str().to_owned())
            .collect(),
        work_profile: RevisionRefDto {
            id: spec.work_profile.as_str().to_owned(),
            version: spec.work_profile_version,
        },
        auto_arm: matches!(spec.approval, AutoArmPolicy::BoundedAutoArm { .. }),
    }
}

/// One recorded intake decision.
fn intake_dto(
    realm_id: kontor_core::id::RealmId,
    receipt: &IntakeReceipt,
    applied: AppliedDto,
) -> IntakeReceiptDto {
    IntakeReceiptDto {
        realm_id,
        receipt_id: receipt.id.to_string(),
        source_event_id: receipt.source_event_id.to_string(),
        source_event_hash: receipt.source_event_hash.as_str().to_owned(),
        trigger: RevisionRefDto {
            id: receipt.trigger.as_str().to_owned(),
            version: receipt.trigger_version,
        },
        result: receipt.result.as_str().to_owned(),
        dedup_key: receipt.dedup_key.as_str().to_owned(),
        duplicate_of: receipt.duplicate_of.map(|id| id.to_string()),
        applied,
    }
}

/// One recorded reconciliation conflict.
fn conflict_dto(conflict: &StoredConflict) -> TicketConflictDto {
    TicketConflictDto {
        conflict_id: conflict.id.to_string(),
        link_id: conflict.link_id.to_string(),
        kind: conflict.kind.as_str().to_owned(),
        observation_id: conflict.observation_id.to_string(),
        task_revision: conflict.task_revision,
        spec_version: conflict.spec_version,
        detected_at: conflict.detected_at,
        resolved_at: conflict.resolved_at,
    }
}

/// The domain's own name for one ownership action.
///
/// It goes through the serializer rather than a hand-written string, so a
/// variant renamed in the domain cannot keep an old spelling alive out here.
fn ownership_action_name(action: OwnershipAction) -> String {
    serde_json::to_value(action)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "reassign_to_principal".to_owned())
}

/// Whether a workflow has reached at least `target` in its profile's phase order.
///
/// Phases are an ordered list on the pinned profile, so "reached" is position and
/// not name matching: a handoff that waits for `review` is satisfied once the
/// task is at `review` or past it. A phase the profile does not declare is never
/// reached, because a condition nothing can satisfy must not silently pass.
fn phase_reached(
    definition: &kontor_core::spec::WorkProfileSpec,
    current: &kontor_core::id::PhaseKey,
    target: &kontor_core::id::PhaseKey,
) -> bool {
    let position = |key: &kontor_core::id::PhaseKey| {
        definition.phases.iter().position(|phase| &phase.id == key)
    };
    match (position(current), position(target)) {
        (Some(current), Some(target)) => current >= target,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{eligible_roots, slot_prompt};

    /// The bundled team is a chain — architect → builder → inspector → tester →
    /// verifier — so exactly one slot is a root, and the other four are waiting on
    /// a handoff.
    ///
    /// The mutants this kills: treating every declared slot as a root (five agents
    /// all told to start the same work), and treating the *first declared* slot as
    /// the root regardless of the DAG.
    #[test]
    fn only_the_slots_no_handoff_feeds_are_eligible_roots() {
        let teams = kontor_teams::spec::bundled_teams().expect("the bundled teams load");
        for team in &teams.teams {
            let roots = eligible_roots(team);
            assert!(!roots.is_empty(), "a team always starts somewhere");
            for handoff in &team.handoffs {
                assert!(
                    !roots.contains(&handoff.to_slot),
                    "`{}` receives a handoff and is not a root",
                    handoff.to_slot
                );
            }
            for slot in &team.slots {
                let fed = team
                    .handoffs
                    .iter()
                    .any(|handoff| handoff.to_slot == slot.id);
                assert_eq!(
                    roots.contains(&slot.id),
                    !fed,
                    "`{}` is a root exactly when nothing hands work to it",
                    slot.id
                );
            }
            // And a root is given the work while everything downstream is told, in
            // so many words, to wait. Silence would be indistinguishable from a
            // start instruction to the agent receiving it.
            for slot in &team.slots {
                let prompt = slot_prompt(&slot.id, &roots).expect("a bounded prompt");
                if roots.contains(&slot.id) {
                    assert!(
                        !prompt.as_str().contains("wait"),
                        "`{}` is a root and is given the work",
                        slot.id
                    );
                } else {
                    assert!(
                        prompt.as_str().starts_with("wait"),
                        "`{}` is downstream and is told to wait, not left silent",
                        slot.id
                    );
                }
            }
        }
    }

    /// A template with no handoffs at all has nothing to be downstream *of*, so
    /// every slot leads. The mutant this kills is returning an empty root set —
    /// which would idle every seat of such a team and start nothing.
    #[test]
    fn a_team_with_no_handoffs_starts_every_slot() {
        let teams = kontor_teams::spec::bundled_teams().expect("the bundled teams load");
        let mut team = teams.teams[0].clone();
        team.handoffs.clear();
        let roots = eligible_roots(&team);
        assert_eq!(roots.len(), team.slots.len(), "every slot leads");
    }
}
