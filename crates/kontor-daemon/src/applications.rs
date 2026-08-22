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
//! A delivery native session is created in exactly one place in this file:
//! inside the shared seating path reached by [`Services::start`] and exact
//! admission recovery, after `admit_candidate` has committed. Persistent Core
//! Team seats use their separate, explicitly routed materialization surface;
//! they have no TeamRun and are keyed by their durable SeatBinding. Neither
//! path can create the other's kind of session.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use kontor_accounts::{AccountAvailability, AdaptivePosition, CapacityReading, ProbeOutcome};
use kontor_api::applications::{
    AmendAccountProfileRequest,
    AbandonRunRequest, AbandonedRunDto, AccountProfileDto, ApplicationOperations, AppliedDto,
    AppliedEpicDto, AppliedLinkDto, AppliedTaskDto, ApplyEpicRequest, ArmRequest,
    AuthorizationProjectionDto, BlockedTaskDto, BudgetBoundsDto, BudgetBoundsRequest,
    CreditBalanceDto, DisarmRequest, EnsureAccountProfileRequest, EnsureProjectRequest,
    EpicExecutionScopeDto, EpicImportStateDto, EpicProjectionDto, EpicTaskProjectionDto,
    HeadroomCeilingsDto, LifecycleAction, LifecycleOutcomeDto, LifecycleRequest, ModelCatalogDto,
    PreviewEpicDto, PreviewEpicTaskDto, ProjectDto, ProviderQuotaStateDto,
    PublishedTeamRevisionDto, QuotaWindowDto, ReadyTaskDto, ResumeAdmissionsRequest,
    RevisionRefDto, RuntimeCapabilityDto, SchedulerPlanDto, SchedulerResumeDto, SchedulerStartDto,
    SeatProjectionDto, StartRequest, StartedSeatDto, TeamDraftDto, TeamDraftRequest,
    TeamDraftSlotDto, TeamRunProjectionDto, TeamTemplateCatalogDto, TeamsProjectionDto,
    WorkProfileCatalogDto,
};
use kontor_api::applications::{
    AccountAvailabilityDto, AdaptiveWindowDto, AvailabilityOverrideDto,
    AvailabilityOverrideRequest, CapacityCeilingsDto, CapacityConfigurationDto,
    CapacityConfigurationPreviewDto, CapacityConfigurationRequest, CapacityObservationDto,
    CapacityRefreshRequest, MutationReceiptDto, ObservedBindingDto, ProjectCapacityDto,
    PublishTriggerRequest, RecordProviderQuotaRequest, ResolvedRoleRefDto, SeatBindingOutcomeDto,
    SeatBindingRequest, TopologySeatDto,
};
use kontor_api::applications::{
    AdvanceCompletionRequest, AdvisorRunDto, AppliedProfileDto, CloseoutEvidenceDto,
    CloseoutRequirementDto, CommitteeFindingDto, CommitteeRunDto, CommitteeVerdictDto,
    CompletionBlockerDto, CompletionOutcomeDto, CompletionPhaseDto, CompletionRoundDto,
    CompletionStateDto, CompletionWakeDto, ConsultationSeatDto, ConsultationVerdictDto,
    CoreTeamApplyRequest, CoreTeamDto, CoreTeamMaterializeRequest, CoreTeamNativeSeatDto,
    CoreTeamOutcomeDto, CoreTeamPreviewDto, CoreTeamPreviewRequest, CoreTeamRouteApplyRequest,
    CoreTeamRouteOutcomeDto, CoreTeamRoutePreviewDto, CoreTeamRoutePreviewRequest, CoreTeamSeatDto,
    CoreTeamSeatRouteRequest, CoreTeamSeatSelectionDto, DeliberationStepDto,
    EnsureQuickSessionRequest, HostedSeatMessageDto, HostedSeatMessageRequestDto,
    IntegrationRecordDto, InvokeConsultationRequest, NeedsHumanDto, ProfileApplyRequest,
    ProfileCatalogDto, ProfilePreviewDto, ProfilePreviewRequest, ProfileRevisionDto,
    PromotedSessionDto, PromotionApplyRequest, PromotionPreviewDto, QuickRolesDto, QuickSessionDto,
    RecordFindingsRequest, RemediateCompletionRequest, RemediationActionDto, RepositoryOutcomeDto,
    RosterUpgradePreviewDto, RosterUpgradePreviewRequest, SettleConsultationRequest,
};
use kontor_api::applications::{
    AppliedContainerRetitleDto, AppliedNativeNamesDto, AppliedTopologyUpgradeDto, CodeHelpEntryDto,
    ContainerRetitlePreviewDto, ContainerRetitleRequest, DesiredBindingDto,
    NativeNameSubjectKindDto, NativeNameTargetDto, NativeNamesApplyRequest, NativeNamesPreviewDto,
    NativeNamesPreviewRequest, PinnedSpecDto, SemanticTopologyRequest, SemanticTopologyTargetDto,
    SessionLabelsReconcileRequest, SessionLabelsReconciledDto, ShareabilityDto,
    TopologyMutationDto, TopologyNodeDto, TopologyNodeRequest, TopologyProjectionDto,
    TopologyUpgradeApplyRequest, TopologyUpgradeEffectDto, TopologyUpgradePreviewDto,
    TopologyUpgradePreviewRequest,
};
use kontor_api::applications::{
    AttestLateHandoffRequest, ConnectorSpecDto, IntakeReceiptDto, LateHandoffAttestationDto,
    ProfileArtifactDto, ProfileHandoffDto, ProfilePackDto, ProfilePhaseDto, ProfileValidationDto,
    RegisterPackRequest, ReplaceSeatRequest, ReplacedSeatDto, ResolveConflictRequest,
    RoleSlotWaiverDto, RuntimeModelRouteRequest, SettleTurnRequest, SettledTurnDto,
    SubmitIntakeRequest, TicketClaimDto, TicketCommentDto, TicketCommentPullDto, TicketConflictDto,
    TriggerSpecDto, TurnFollowUpDto, WaiveRoleSlotRequest, WorkProfileDetailDto,
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
use kontor_core::consultation::{
    AdvisorProfileSpec, CommitteeRole, CommitteeTemplateSpec,
    CommitteeVerdict as ConsultationVerdict, ConsultationFamily, ConsultationRunId,
    ConsultationRunState, ConsultationScope, RecordedFinding, conjunctive_outcome,
};
use kontor_core::id::{
    AccountProfileId, AdvisorRunId, AgentRunId, AggregateRevision, ArtifactKey, BoundedText,
    CanonicalDocument, CommandReceiptId, CommitteeRunId, ConnectorKey, ContentHash, CurrencyCode,
    ExecutionAuthorizationId, ExternalId, ExternalName, GateKey, IdempotencyKey, IntakeReceiptId,
    MiniProjectId, ModuleKey, Money, ProjectId, QuickSessionId, RoleCatalogId, RoleCode, RoleKey,
    RoleSlotId, RoleTurnId, RuntimeKindKey, SCHEMA_VERSION, SeatBindingId, SourceEventId,
    SpecVersion, StatusConflictId, TaskId, TeamRunId, TicketProjectionId, Timestamp,
    TopologyKindKey, TopologyNodeId, TopologySpecId, TriggerKey,
};
use kontor_core::naming::{NativeNameTemplate, NativeNameValues};
use kontor_core::realm::ReceiptEnvelope;
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    AccountProfileUpdate,
    AdaptiveAdmissionAdvance, CalendarRepository, CapacityRepository, CommandRepository,
    CredentialReference, CredentialReferenceKind, IntakeOutcome, IntakeRepository, MiniProject,
    MiniProjectTopologySnapshot, NewAccountProfile, NewAdaptiveAdmissionState, NewAgentRun,
    NewAvailabilityOverride, NewCapacityObservation, NewCommandIntent, NewGateEvaluation,
    NewLocalCommand, NewMiniProject, NewNativeContainerBinding, NewProviderQuotaState,
    NewSeatBinding, NewSessionTopologyNode, NewSourceEvent, NewTeamRun, ProjectRepository,
    ProjectTopologyDefault, RealmRepository, RepositoryError, RunRepository, RuntimeBinding,
    SeatLivenessObservation, SourceDisposition, SpecRepository, StoredCommitteeFinding,
    StoredCompletionProfile, StoredCompletionWake, StoredConsultationProfileRevision,
    StoredConsultationRun, StoredConsultationSeat, StoredCoreTeamRevision, StoredEpicCompletion,
    StoredEpicRoster, StoredHostedTopologySeat, StoredPromotion, StoredQuickSession,
    StoredRemediationProposal, TaskTransitionRequest, TicketLink, TicketRepository,
    TopologyRepository, WorkflowRepository,
};
use kontor_core::spec::{
    AutoArmPolicy, CanonicalSourceEvent, CatalogRoleRef, CodeCategory, ContextEnforcement,
    ContextPolicySnapshot, EffectiveContextPolicy, EffortLevel, EpicPresence, IntakeReceipt,
    IntakeResult, ModelRef, ModelRung, NodeProjectionCapability, ProjectSessionTopologySpec,
    ProviderRef, RequestedContextPolicy, RoleCatalogRevision, SeatAutonomy, Shareability,
    ShareabilityTier, SourceIdentity, SourceProcessingState, TeamRunSnapshot, TopologySnapshot,
    TriggerSpec,
};
use kontor_core::state::{
    GateVerdict, ImportedTaskState, ObservedContainerKind, RuntimeContact, SeatBinding,
    SessionTopologyNode, TaskState, TaskTeamClosure, TerminalEvidenceSource, TerminalOutcome,
    TopologyLifecycle,
};
use kontor_core::ticket::{
    CommentPolicy, InternalTaskFacts, OwnershipAction, ReconciliationOutcome, StatusConflictKind,
    TicketSyncProjection, TransitionPlan,
};
use kontor_integrations_asma::AsmaExecutable;
use kontor_integrations_asma::jira::{
    ApplyAuthority, CompiledFieldSpec, CompiledWorkflowSpec, FieldSpecKey, JiraOutcome, Observed,
    PinnedProfile, SpecCatalog, TicketDelegation, WorkflowSpecKey,
};
use kontor_policy::{
    CloseoutEvidence, CloseoutRequirement, DeliberationStep, NeedsHumanPayload,
    OpenQuestionBlocker, TicketEvidence, TicketGateBlocker, TicketRequirement,
};
use kontor_profiles::pack::{
    OperationalDomainPack, PackAvailability, PackCategoryKey, ProfilePackSpec,
    ResolvedProfileBundle, parse_pack, resolve_profile, validate_pack,
};
use kontor_runtime::adapter::{
    ConsultationCredential, ConsultationLaunchRequest, ConsultationMessageRequest,
    HostedSeatLaunchRequest, HostedSeatMessageRequest, HostedSeatRetireRequest, RetitleSeatRequest,
    RuntimeAdapter, RuntimeError,
};
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{RuntimeBindingSnapshot, RuntimeCapability};
use kontor_runtime::container::{
    ContainerBinding, ContainerBindingId, ContainerBindingSnapshot, ContainerProjection,
    ContainerRequest, RetitleContainerRequest,
};
use kontor_runtime::observation::ControlPlaneObservation;
use kontor_runtime::request::{
    LaunchParts, LaunchPlacement, MessageId, ReconcileSessionLabelsRequest,
};
use kontor_runtime::scope::{EpicScope, ExecutionScope, TaskScope};
use kontor_runtime::workspace::WorkspaceRoot;
use kontor_scheduler::headroom::HeadroomConfig;
use kontor_scheduler::model::{
    AccountAdmissionEvidence, AdaptiveWindow, AdmissionEventId, AdmittedCandidate,
    AuthorizationEvidence, CalendarAdmission, Candidate, CandidateDecision, CapacityConfig,
    CapacityUsage, ExternalWorkEvidence, ReconciliationEvidence, ReconciliationScope,
    RuntimeAdmissionEvidence, RuntimeHealth, SchedulingSnapshot, TaskOrigin, WorktreeClaim,
    WorktreeVerification,
};
use kontor_scheduler::{
    CommitteeVerdict, CompiledCompletion, CompletionBlocker, CompletionCommand,
    CompletionObservation, CompletionPhase, CompletionProfile, CompletionSignal, CompletionState,
    CompletionTransition, RemediationApproval, RemediationAuthorization, SignalDelivery,
};
use kontor_store::{
    AdmissionCommit, Applied, AuthorizationRevocation, EpicApplication,
    EpicExecutionScopeDeclaration, EpicTask, EpicTicketLink, IdempotencyBinding, NewRoleTurn,
    ProjectEnsure, RegisteredPack, SettledTurn, SqliteStore, StoredConflict, StoredTeamDraft,
    StoredTeamsProjection, TurnDispatch,
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

/// The already-validated authority and policy inputs frozen into one Committee.
struct CommitteeInvocation<'a> {
    project_id: ProjectId,
    epic_id: MiniProjectId,
    request: &'a InvokeConsultationRequest,
    template_revision: &'a StoredConsultationProfileRevision,
    template: &'a CommitteeTemplateSpec,
    caller: &'a kontor_core::state::SeatBinding,
}

/// The adopted session base a Quick session is placed under.
struct SessionBase {
    /// The logical root node.
    node: SessionTopologyNode,
    /// The native project its runtime reported for that node, when it has
    /// reported one.
    native_id: Option<ExternalId>,
}

/// One Jira link prepared by the connector plan and reusable by its apply.
struct PreparedTicket {
    link: TicketLink,
    wire_key: IdempotencyKey,
    projection: TicketSyncProjection,
    facts: InternalTaskFacts,
    observed: Observed,
    transition: Option<TransitionPlan>,
}

/// The complete, externally observed plan one reconcile response names.
struct PreparedTicketPlan {
    links: Vec<kontor_core::id::TicketLinkId>,
    diff: Vec<TicketFieldDiffDto>,
    hash: String,
    tickets: Vec<PreparedTicket>,
}

/// One validated epic request, ready for preview or apply under the same rules.
struct PreparedEpic {
    bundle: ResolvedProfileBundle,
    execution_scope: Option<EpicExecutionScopeDeclaration>,
    tasks: Vec<EpicTask>,
}

enum NativeNameAction {
    Container {
        request: RetitleContainerRequest,
        adapter: Arc<dyn RuntimeAdapter>,
    },
    Seat {
        request: RetitleSeatRequest,
        adapter: Arc<dyn RuntimeAdapter>,
        hosted_seat_binding_id: Option<SeatBindingId>,
    },
}

struct PreparedNativeNames {
    preview: NativeNamesPreviewDto,
    actions: Vec<NativeNameAction>,
}

struct CoreTeamRoutePlan {
    epic: MiniProject,
    roster: FrozenRoster,
    binding: SeatBinding,
    predecessor: StoredHostedTopologySeat,
    successor: Option<StoredHostedTopologySeat>,
    desired: ModelRung,
    preview_hash: ContentHash,
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
    /// The one configured Jira wire boundary, when this Realm has one.
    asma: Option<AsmaExecutable>,
    /// How many simultaneous runs this Realm admits, from its configuration.
    ///
    /// Held here and read at both the planning and the admission call site, so a
    /// plan and the commit that follows it are judged against the same ceilings.
    /// It arrives at construction and never changes: a Realm that re-read its
    /// ceilings mid-flight could refuse a candidate the plan it is executing had
    /// already admitted.
    capacity: CapacityConfig,
    /// Stable native-root marker directories, derived by project and epic id.
    runtime_roots: PathBuf,
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
        asma: Option<AsmaExecutable>,
        runtime_roots: PathBuf,
    ) -> Result<Arc<Self>, kontor_core::DomainError> {
        Ok(Arc::new(Self {
            realm_id,
            state: OnceLock::new(),
            pack: kontor_profiles::seeds::bundled_pack()?,
            domain: kontor_profiles::bundled_operational_domain()?,
            connectors: OnceLock::new(),
            asma,
            capacity,
            runtime_roots,
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

    /// Build the runtime-neutral scope for one epic or task from durable state.
    ///
    /// A legacy adapter may supply its exact configured scope only while the
    /// epic has no durable declaration. If neither exists, a legacy import in
    /// canonical `<external key> · <short title>` form is recovered without
    /// leaking internal ids. Older non-imported epics retain their historical
    /// id/title fallback until they gain an explicit scope; an explicit
    /// declaration always wins.
    fn execution_scope(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        task_id: Option<TaskId>,
        adapter: &dyn RuntimeAdapter,
    ) -> Result<ExecutionScope, ApiError> {
        let state = self.state()?;
        let stored = state
            .with_store(|store| store.get_epic_execution_scope(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?;
        if stored.is_none()
            && let Some(configured) = adapter.configured_execution_scope(epic_id, task_id)
        {
            return Ok(configured);
        }
        let epic = match stored {
            Some(stored) => EpicScope {
                mini_project_id: epic_id,
                external_epic_key: stored.external_epic_key,
                short_title: stored.short_title,
            },
            None => {
                let epic = state
                    .with_store(|store| store.get_mini_project(project_id, epic_id))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::PlacementBlocked,
                            "the runtime execution scope names no durable epic",
                        )
                    })?;
                self.legacy_epic_scope(epic)?
            }
        };
        let Some(task_id) = task_id else {
            return Ok(ExecutionScope::for_epic(epic));
        };
        let task = state
            .with_store(|store| store.get_task(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|task| task.mini_project_id == Some(epic_id))
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the task does not belong to the epic execution scope",
                )
            })?;
        let worktree = state
            .with_store(|store| store.task_worktree(project_id, task.id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the task has no declared canonical worktree",
                )
            })?;
        let worktree =
            WorkspaceRoot::parse(worktree.as_str()).map_err(|error| self.refuse_domain(&error))?;
        let mut jira = state
            .with_store(|store| store.list_task_ticket_links(project_id, task.id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .filter(|link| link.connector.as_str() == "jira")
            .map(|link| link.external_issue_key);
        let external_issue_key = jira.next().unwrap_or(
            ExternalId::parse(&task.id.to_string()).map_err(|error| self.refuse_domain(&error))?,
        );
        if jira.next().is_some() {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the task has more than one Jira link for its runtime execution scope",
            ));
        }
        let short_code = state
            .with_store(|store| store.task_short_code(project_id, task.id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the task has no durable short code; preview and apply an explicit epic task mapping before materialization or retitle",
                )
            })?;
        Ok(ExecutionScope::for_task(
            epic,
            TaskScope {
                task_id,
                short_code,
                external_issue_key,
                worktree,
            },
        ))
    }

    /// Recover the runtime identity of an imported epic that predates the typed
    /// execution-scope column. Only the published canonical spelling is
    /// accepted; an arbitrary display name is not identity.
    fn legacy_epic_scope(
        &self,
        epic: kontor_core::repository::MiniProject,
    ) -> Result<EpicScope, ApiError> {
        if let Some((external, short)) = epic.name.as_str().split_once(" · ")
            && let (Ok(external_epic_key), Ok(short_title)) =
                (ExternalId::parse(external), ExternalName::parse(short))
        {
            return Ok(EpicScope {
                mini_project_id: epic.id,
                external_epic_key,
                short_title,
            });
        }
        // Pre-schema-43 realms used the internal id and whole stored title.
        // Preserve that compatibility for unrelated legacy epics. Imported
        // canonical names take the branch above and no longer leak ids.
        Ok(EpicScope {
            mini_project_id: epic.id,
            external_epic_key: ExternalId::parse(&epic.id.to_string())
                .map_err(|error| self.refuse_domain(&error))?,
            short_title: epic.name,
        })
    }

    /// Validate one explicit compact display identity at the import boundary.
    /// The database repeats the size/alphabet rule; these semantic refusals keep
    /// ticket keys and internal ids from becoming plausible-looking titles.
    fn validate_task_short_code(
        &self,
        code: &ExternalId,
        links: &[EpicTicketLink],
    ) -> Result<ExternalId, ApiError> {
        if code.as_str().len() > 32 {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "a task short code is longer than 32 bytes",
            ));
        }
        if links
            .iter()
            .any(|link| link.external_issue_key.as_str() == code.as_str())
        {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "a task short code must be distinct from its external issue key",
            ));
        }
        if uuid::Uuid::parse_str(code.as_str()).is_ok() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "an internal identifier is not a task short code",
            ));
        }
        Ok(code.clone())
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

    /// Validate and type one epic request once for both preview and apply.
    fn prepare_epic(
        &self,
        project_id: ProjectId,
        request: &ApplyEpicRequest,
        now: Timestamp,
    ) -> Result<PreparedEpic, ApiError> {
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
            // `kontor-runtime`, so the path's rules are enforced at this boundary.
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
            let imported_state = match task.import_state {
                EpicImportStateDto::Ready => ImportedTaskState::Ready,
                EpicImportStateDto::Completed => ImportedTaskState::Completed,
            };
            tasks.push(EpicTask {
                title: task.title.clone(),
                short_code: task
                    .short_code
                    .as_ref()
                    .map(|code| self.validate_task_short_code(code, &links))
                    .transpose()?,
                ai_short_name: task.ai_short_name.clone(),
                module,
                imported_state,
                depends_on: task.depends_on.clone(),
                ticket_links: links,
                worktree,
            });
        }

        let execution_scope =
            request
                .execution_scope
                .as_ref()
                .map(|scope| EpicExecutionScopeDeclaration {
                    external_epic_key: scope.external_epic_key.clone(),
                    short_title: scope.short_title.clone(),
                    kontor_backlog_code: scope.kontor_backlog_code.clone(),
                    ai_short_name: scope.ai_short_name.clone(),
                });

        Ok(PreparedEpic {
            bundle,
            execution_scope,
            tasks,
        })
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
        let execution_scope = state
            .with_store(|store| store.get_epic_execution_scope(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?;
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
                short_code: state
                    .with_store(|store| store.task_short_code(project_id, task.id))
                    .map_err(|error| self.refuse(&error))?,
                ai_short_name: state
                    .with_store(|store| store.task_ai_short_name(project_id, task.id))
                    .map_err(|error| self.refuse(&error))?,
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
            execution_scope: execution_scope.map(|scope| EpicExecutionScopeDto {
                external_epic_key: scope.external_epic_key,
                short_title: scope.short_title,
                kontor_backlog_code: scope.kontor_backlog_code,
                ai_short_name: scope.ai_short_name,
            }),
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

    /// Resolve the one current run at the leaf of a delivery role's replacement
    /// chain. Repository enumeration is oldest-first, so selecting the first
    /// same-role row would target an archived predecessor after replacement.
    fn current_delivery_role_leaf(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
        role: &RoleKey,
    ) -> Result<Option<kontor_core::repository::AgentRun>, ApiError> {
        let state = self.state()?;
        let rows = state
            .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?;
        let mut runs = Vec::new();
        for row in rows.into_iter().filter(|row| &row.role == role) {
            let run = state
                .with_store(|store| store.get_agent_run(project_id, row.agent_run_id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::StaleBinding,
                        "a delivery role census row names a missing AgentRun",
                    )
                })?;
            if run.team_run_id != team_run_id || &run.role != role {
                return Err(self.deny(
                    ApiErrorCode::StaleBinding,
                    "a delivery role census row drifted from its team or role slot",
                ));
            }
            runs.push(run);
        }

        // A logical role slot is created before its first AgentRun. It has no
        // native title target yet; absence here is not a broken replacement
        // chain and must not block repair of the epic's existing containers.
        if runs.is_empty() {
            return Ok(None);
        }

        let named_parents: BTreeSet<AgentRunId> = runs
            .iter()
            .filter_map(|run| run.parent_agent_run_id)
            .collect();
        let mut leaves = runs
            .into_iter()
            .filter(|run| !named_parents.contains(&run.id));
        let leaf = leaves.next().ok_or_else(|| {
            self.deny(
                ApiErrorCode::StaleBinding,
                "a delivery role has no current replacement-chain leaf",
            )
        })?;
        if leaves.next().is_some() {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "a delivery role has ambiguous current replacement-chain leaves",
            ));
        }
        if leaf
            .binding
            .as_ref()
            .is_some_and(|binding| binding.agent_run_id != leaf.id)
        {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the current delivery role leaf's native binding names another AgentRun",
            ));
        }
        Ok(Some(leaf))
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
        self.release_team_seats(project_id, run.team_run_id, now)?;
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
            self.release_team_seats(project_id, run.team_run_id, now)?;
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
        self.release_team_seats(project_id, run.team_run_id, now)?;
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

    /// Resolve the ticket-link shorthand to the catalogue's canonical key.
    fn canonical_connector(&self, connector: &str) -> Result<ConnectorKey, ApiError> {
        let canonical = if connector == "jira" {
            "connector.jira"
        } else {
            connector
        };
        ConnectorKey::parse(canonical).map_err(|error| self.refuse_domain(&error))
    }

    /// The configured Jira boundary, or the honest answer for a Realm without it.
    fn asma(&self) -> Result<&AsmaExecutable, ApiError> {
        self.asma.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "this realm is not configured with the ASMA Jira connector boundary",
            )
        })
    }

    /// Turn the connector crate's typed refusal into the closed API vocabulary.
    fn refuse_asma(&self, error: &kontor_integrations_asma::AsmaError) -> ApiError {
        tracing::warn!(detail = %error, "the configured Jira connector refused reconciliation");
        match error {
            kontor_integrations_asma::AsmaError::Conflict { kind, .. } => {
                self.deny(ApiErrorCode::RevisionConflict, jira_conflict_rule(*kind))
            }
            kontor_integrations_asma::AsmaError::Domain(_)
            | kontor_integrations_asma::AsmaError::Selection { .. }
            | kontor_integrations_asma::AsmaError::Refused { .. } => self.deny(
                ApiErrorCode::UnsupportedCapability,
                "the pinned Jira specification cannot represent this reconciliation",
            ),
            kontor_integrations_asma::AsmaError::Unavailable { .. } => self.deny(
                ApiErrorCode::Unavailable,
                "the configured ASMA Jira connector boundary could not answer",
            ),
            _ => self.deny(
                ApiErrorCode::Unavailable,
                "the configured ASMA Jira connector boundary refused reconciliation",
            ),
        }
    }

    /// Select the exact bundled mapping pinned by one task's frozen profile.
    fn jira_specs(
        &self,
        workflow: &kontor_core::repository::TaskWorkflow,
    ) -> Result<(CompiledFieldSpec, CompiledWorkflowSpec), ApiError> {
        let catalog = self.connector_catalog()?;
        let seed_field = catalog.field_specs().first().ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "this build ships no Jira ticket-field specification",
            )
        })?;
        let field = catalog
            .select_field_spec(&FieldSpecKey {
                connector: seed_field.spec().connector.clone(),
                project: seed_field.spec().project.clone(),
                issue_type: seed_field.spec().issue_type.clone(),
                version: seed_field.spec().version,
            })
            .map_err(|error| self.refuse_asma(&error))?
            .clone();
        let seed_workflow = catalog.workflow_specs().first().ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "this build ships no Jira external-workflow specification",
            )
        })?;
        let selector = kontor_core::repository::ConnectorSpecSelector {
            project_id: workflow.project_id,
            connector: seed_workflow.spec().connector.clone(),
            project: seed_workflow.spec().project.clone(),
            issue_type: seed_workflow.spec().issue_type.clone(),
            version: seed_workflow.spec().version,
        };
        let installed = self
            .state()?
            .with_store(|store| store.get_external_workflow_spec(&selector))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::UnsupportedCapability,
                    "install the canonical connector.jira external-workflow revision before reconciling Jira links",
                )
            })?;
        let installed_json = serde_json::to_string(&installed).map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the installed external-workflow specification could not be compiled",
            )
        })?;
        let mut installed_catalog = SpecCatalog::empty();
        installed_catalog
            .load_workflow_spec(&installed_json)
            .map_err(|error| self.refuse_asma(&error))?;
        let external = installed_catalog
            .select_workflow_spec(&WorkflowSpecKey {
                connector: selector.connector,
                project: selector.project,
                issue_type: selector.issue_type,
                version: selector.version,
                work_profile: Some(PinnedProfile {
                    key: workflow.snapshot.definition.id.clone(),
                    version: workflow.snapshot.definition.version,
                }),
            })
            .map_err(|error| self.refuse_asma(&error))?
            .clone();
        Ok((field, external))
    }

    /// Compile the internal facts the pure Jira policy is allowed to inspect.
    fn ticket_facts(
        &self,
        project_id: ProjectId,
        task: &kontor_core::repository::Task,
        workflow: &kontor_core::repository::TaskWorkflow,
        projection_revision: AggregateRevision,
    ) -> Result<InternalTaskFacts, ApiError> {
        let state = self.state()?;
        let gate_states = state
            .with_store(|store| store.gate_states(project_id, workflow.id))
            .map_err(|error| self.refuse(&error))?;
        // Native `done` is itself a closure certificate: the store cannot
        // transition a task there until every required phase, gate and artifact
        // is proven. An imported historical completion deliberately is not that
        // certificate, even though it projects as `done` for dependency and
        // backlog-count continuity.
        let native_done = task.state == TaskState::Done
            && !task
                .imported_state
                .is_some_and(ImportedTaskState::is_historical_completion);
        let all_required_gates_passed = native_done
            || workflow.snapshot.definition.gates.iter().all(|gate| {
                gate_states
                    .get(&gate.id)
                    .is_some_and(|state| state.satisfies_requirement())
            });
        let run_outcome = state
            .with_store(|store| store.list_team_runs_for_task(project_id, task.id))
            .map_err(|error| self.refuse(&error))?
            .last()
            .copied()
            .map(|(team_run_id, _)| {
                Ok(state
                    .with_store(|store| store.get_team_run(project_id, team_run_id))
                    .map_err(|error| self.refuse(&error))?
                    .and_then(|run| run.terminal.map(|terminal| terminal.outcome)))
            })
            .transpose()?
            .flatten()
            .or_else(|| native_done.then_some(TerminalOutcome::Succeeded));
        let completed_phases = if native_done {
            workflow
                .snapshot
                .definition
                .phases
                .iter()
                .map(|phase| phase.id.clone())
                .collect()
        } else {
            BTreeSet::new()
        };
        Ok(InternalTaskFacts {
            task_id: task.id,
            task_state: task.state,
            task_revision: task.revision,
            workflow_revision: workflow.revision,
            projection_revision,
            completed_phases,
            gate_states: gate_states.into_iter().collect(),
            all_required_gates_passed,
            run_outcome,
        })
    }

    /// Observe Jira, run the pure policy, and validate each proposed write.
    async fn prepare_ticket_plan(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        idempotency_key: &IdempotencyKey,
    ) -> Result<PreparedTicketPlan, ApiError> {
        let state = self.state()?;
        let task = self.task_row(project_id, task_id)?;
        let links = state
            .with_store(|store| store.list_task_ticket_links(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;
        let workflow = state
            .with_store(|store| store.get_active_task_workflow(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;

        if links.is_empty() {
            let document = self.intent(&serde_json::json!({
                "schema_version": 1,
                "task_id": task_id.to_string(),
                "task_revision": task.revision.get(),
                "links": [],
            }))?;
            return Ok(PreparedTicketPlan {
                links: Vec::new(),
                diff: Vec::new(),
                hash: document.hash().as_str().to_owned(),
                tickets: Vec::new(),
            });
        }

        let workflow = workflow.ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "a Jira-linked task has no active workflow specification",
            )
        })?;
        let (field_spec, workflow_spec) = self.jira_specs(&workflow)?;
        let asma = self.asma()?;
        let mut diff = Vec::new();
        let mut tickets = Vec::new();
        for link in links {
            if !matches!(link.connector.as_str(), "jira" | "connector.jira") {
                return Err(self.deny(
                    ApiErrorCode::UnsupportedCapability,
                    "this build cannot reconcile the linked ticket's connector",
                ));
            }
            let facts = self.ticket_facts(project_id, &task, &workflow, link.revision)?;
            let wire_key_text = format!("{}:{}", idempotency_key.as_str(), link.id);
            let wire_key = IdempotencyKey::parse(&wire_key_text)
                .map_err(|error| self.refuse_domain(&error))?;
            let projection = TicketSyncProjection {
                schema_version: SCHEMA_VERSION,
                id: TicketProjectionId::generate(),
                link_id: link.id,
                link_revision: link.revision,
                connector: field_spec.spec().connector.clone(),
                field_spec_project: field_spec.spec().project.clone(),
                field_spec_issue_type: field_spec.spec().issue_type.clone(),
                field_spec_version: field_spec.spec().version,
                external_issue_key: link.external_issue_key.clone(),
                fields: Vec::new(),
                comment_policy: CommentPolicy::InboundOnly,
                external_comment_cursor: None,
                computed_at: kontor_api::now(),
            };
            let delegation = TicketDelegation {
                asma,
                field_spec: &field_spec,
                workflow_spec: &workflow_spec,
                projection: &projection,
                facts: &facts,
                link_id: link.id,
                idempotency_key: &wire_key,
            };
            let observed = delegation
                .observe()
                .await
                .map_err(|error| self.refuse_asma(&error))?;
            let transition = match delegation.plan(&observed) {
                ReconciliationOutcome::NoOp => None,
                ReconciliationOutcome::Transition(plan) => {
                    let dry_run = delegation
                        .dry_run(&observed, &plan)
                        .await
                        .map_err(|error| self.refuse_asma(&error))?;
                    if !matches!(dry_run.outcome, JiraOutcome::Planned | JiraOutcome::NoOp) {
                        return Err(self.deny(
                            ApiErrorCode::Unavailable,
                            "the Jira boundary did not validate the planned reconciliation",
                        ));
                    }
                    diff.push(TicketFieldDiffDto {
                        milestone: plan.milestone.as_str().to_owned(),
                        kontor: plan.target.status_name.as_str().to_owned(),
                        external: Some(observed.observation.status.status_name.as_str().to_owned()),
                    });
                    Some(*plan)
                }
                ReconciliationOutcome::Conflict(_) => {
                    return Err(self.deny(
                        ApiErrorCode::RevisionConflict,
                        "fresh Jira evidence conflicts with the pinned external-workflow policy",
                    ));
                }
            };
            tickets.push(PreparedTicket {
                link,
                wire_key,
                projection,
                facts,
                observed,
                transition,
            });
        }

        let document = self.intent(&serde_json::json!({
            "schema_version": 1,
            "task_id": task_id.to_string(),
            "task_revision": task.revision.get(),
            "workflow_revision": workflow.revision.get(),
            "tickets": tickets.iter().map(|ticket| serde_json::json!({
                "link_id": ticket.link.id.to_string(),
                "link_revision": ticket.link.revision.get(),
                "observation_hash": ticket.observed.observation.payload_hash.as_str(),
                "milestone": ticket.transition.as_ref().map(|plan| plan.milestone.as_str()),
                "destination": ticket.transition.as_ref().map(|plan| plan.target.status_id.as_str()),
            })).collect::<Vec<_>>(),
        }))?;
        Ok(PreparedTicketPlan {
            links: tickets.iter().map(|ticket| ticket.link.id).collect(),
            diff,
            hash: document.hash().as_str().to_owned(),
            tickets,
        })
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
        // Delivery is a new turn in the same persistent seat. A closed native
        // process is reloadable without changing that identity, but `send`
        // alone cannot revive it. Resume first; any refusal leaves the durable
        // dispatch row untouched for reconciliation or explicit replacement.
        if adapter
            .resume(&kontor_runtime::request::ResumeRequest {
                binding: request.binding.clone(),
                requested_at: now,
            })
            .await
            .is_err()
        {
            return Ok(false);
        }
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
        for (team_run_id, lifecycle) in runs {
            if lifecycle.is_terminal() {
                continue;
            }
            let seats = state
                .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
                .map_err(|error| self.refuse(&error))?;
            for seat in seats {
                let run = state
                    .with_store(|store| store.get_agent_run(project_id, seat.agent_run_id))
                    .map_err(|error| self.refuse(&error))?;
                if run.is_some_and(|run| !run.projection.lifecycle.is_terminal()) {
                    return Ok(Some(seat.agent_run_id));
                }
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

    /// The bounds an arming grant is taken under.
    ///
    /// Absent bounds default from the epic's **pinned** work profile, which is
    /// read from a task's frozen workflow snapshot rather than from the profile
    /// catalog: the snapshot is what every gate and closure check in that epic
    /// is already judged against, and a later profile revision must not silently
    /// re-grade a grant.
    ///
    /// Supplied bounds may only narrow. `BudgetBounds::within` is the same
    /// comparison the rest of the system uses, so a caller cannot arm wider than
    /// the profile allows by routing around a different check — and it refuses a
    /// cross-currency cost outright rather than comparing two currencies.
    fn armed_budget(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        requested: Option<&BudgetBoundsRequest>,
    ) -> Result<kontor_core::spec::BudgetBounds, ApiError> {
        let state = self.state()?;
        // Every task in an epic pins the same profile revision — `ensure_workflow`
        // refuses an epic that re-applies with a different one — so the first
        // task carrying an active workflow answers for the epic.
        let tasks = state
            .with_store(|store| store.list_epic_tasks(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?;
        let mut defaults = None;
        for task in &tasks {
            if let Some(workflow) = state
                .with_store(|store| store.get_active_task_workflow(project_id, task.id))
                .map_err(|error| self.refuse(&error))?
            {
                defaults = Some(workflow.snapshot.definition.budget_defaults);
                break;
            }
        }
        let Some(defaults) = defaults else {
            // Nothing to default from. A caller that stated its own bounds is
            // still served; one that did not is told what is missing rather than
            // handed a number this endpoint invented.
            return match requested {
                Some(requested) => self.budget_of(requested),
                None => Err(self.deny(
                    ApiErrorCode::NotFound,
                    "this epic pins no work profile to default the budget from; state the bounds                      explicitly or apply the epic graph first",
                )),
            };
        };
        let Some(requested) = requested else {
            return Ok(defaults);
        };
        let requested = self.budget_of(requested)?;
        if !requested.within(&defaults) {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the stated budget is wider than the pinned work profile allows, or is in another                  currency; explicit bounds may only narrow the profile's defaults",
            ));
        }
        Ok(requested)
    }

    /// One wire budget as the domain type, validated.
    fn budget_of(
        &self,
        requested: &BudgetBoundsRequest,
    ) -> Result<kontor_core::spec::BudgetBounds, ApiError> {
        let budget = kontor_core::spec::BudgetBounds {
            max_tokens: requested.max_tokens,
            max_commands: requested.max_commands,
            max_duration_seconds: requested.max_duration_seconds,
            max_cost: Money {
                minor_units: requested.max_cost_minor_units,
                currency: CurrencyCode::parse(&requested.cost_currency)
                    .map_err(|error| self.refuse_domain(&error))?,
            },
        };
        budget
            .validate()
            .map_err(|error| self.refuse_domain(&error))?;
        Ok(budget)
    }

    /// Every account a launch may still be resolved across, in the shape the
    /// rung walk takes.
    ///
    /// The governed pin lives in each profile's immutable `routing` document, so
    /// an account is addressable per provider only where a deployment said so.
    /// A malformed pin is a refusal rather than an empty set: the two differ by
    /// whether the account is routable at all, and quietly choosing the safer
    /// reading hides a typo an operator needs to see.
    fn eligible_accounts(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<kontor_scheduler::headroom::EligibleAccount>, ApiError> {
        let profiles = self
            .state()?
            .with_store(|store| store.list_account_profiles(project_id))
            .map_err(|error| self.refuse(&error))?;
        kontor_accounts::eligible_accounts(&profiles).map_err(|error| self.refuse_domain(&error))
    }

    /// The headroom policy in force, or the state-only fallback.
    ///
    /// A realm configured before OP-REQ-042 has no policy, and that is not a
    /// reason to invent a window threshold for it — `state_only` gates on the
    /// recorded provider state exactly as this realm already did.
    fn headroom_policy(&self) -> HeadroomConfig {
        self.capacity
            .headroom
            .unwrap_or_else(HeadroomConfig::state_only)
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
                NewLocalCommand {
                    project_id,
                    receipt_id: CommandReceiptId::generate(),
                    idempotency_key: key.clone(),
                    kind,
                    target,
                    target_revision,
                    intent: document.clone(),
                    created_at: now,
                },
            );
            store
                .record_local_command_in_realm(&envelope)
                .map(|receipt| receipt.id)
                .map_err(|error| self.refuse(&error))
        })?;
        state.signals().appended();
        Ok(receipt)
    }

    /// Ensure an open native-bound run has the launch intent Kontor actually
    /// exercised when it created that session.
    ///
    /// New downstream and replacement seats call this before runtime contact.
    /// The bound-run reconciliation path calls it only after loading the exact
    /// immutable binding. That second use repairs the historical write omission;
    /// it never authorizes another launch.
    fn ensure_launch_intent(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> Result<kontor_core::repository::AgentRun, ApiError> {
        let state = self.state()?;
        let run = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the agent run no longer exists"))?;
        if run.terminal.is_some() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "a terminal run cannot receive launch intent",
            ));
        }
        let team = state
            .with_store(|store| store.get_team_run(project_id, run.team_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the team run no longer exists"))?;
        if team.lifecycle.is_terminal() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "a terminal team run cannot receive launch intent",
            ));
        }
        match run.projection.desired {
            kontor_core::state::DesiredRunState::RunRequested => return Ok(run),
            kontor_core::state::DesiredRunState::NoIntent => {}
            _ => {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the run already carries a contradictory desired state",
                ));
            }
        }

        let key = IdempotencyKey::parse(&format!("launch-run-{agent_run_id}"))
            .map_err(|error| self.refuse_domain(&error))?;
        let document = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "launch_run",
            "agent_run_id": agent_run_id.to_string(),
            "team_run_id": run.team_run_id.to_string(),
            "role_slot": run.role.as_str(),
        }))?;
        let now = kontor_api::now();
        let envelope = ReceiptEnvelope::new(
            state.realm_id(),
            NewCommandIntent {
                project_id,
                receipt_id: CommandReceiptId::generate(),
                idempotency_key: key,
                kind: CommandKind::LaunchRun,
                target: AggregateRef::AgentRun { agent_run_id },
                target_revision: run.revision,
                intent: document.clone(),
                payload: document,
                desired: Some(kontor_core::state::DesiredRunState::RunRequested),
                not_before: now,
                created_at: now,
            },
        );
        state
            .with_store(|store| store.record_intent_in_realm(&envelope))
            .map_err(|error| self.refuse(&error))?;
        state.signals().appended();
        state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the agent run vanished"))
    }

    /// Persist one runtime-issued control observation and reduce both the child
    /// and its owning TeamRun in the same store transaction.
    fn persist_run_observation(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        observation: &ControlPlaneObservation,
        now: Timestamp,
    ) -> Result<(kontor_core::state::RunProjection, CanonicalDocument), ApiError> {
        let state = self.state()?;
        let run = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "the agent run no longer exists"))?;
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
        Ok((projection, payload))
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
            let worktree = state
                .with_store(|store| store.task_worktree(project_id, task.id))
                .map_err(|error| self.refuse(&error))?
                .map(|worktree| WorktreeClaim {
                    worktree,
                    verification: WorktreeVerification::Verified,
                });
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
                worktree,
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

    /// The roster one epic froze, when it has one.
    fn optional_frozen_roster(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<Option<FrozenRoster>, ApiError> {
        self.state()?
            .with_store(|store| store.get_epic_roster(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
            .map(|stored| {
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
            })
            .transpose()
    }

    /// The roster one epic froze, or a refusal that it has none.
    fn frozen_roster(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
    ) -> Result<FrozenRoster, ApiError> {
        self.optional_frozen_roster(project_id, epic_id)?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "this epic has frozen no Core Team roster",
                )
            })
    }

    /// The comparison side of a roster preview.
    ///
    /// A legacy epic may predate Core Team publication. Its first explicit
    /// preview compares the named published target with an empty roster, but
    /// still carries the epic's revision as the caller's concurrency witness.
    fn roster_upgrade_baseline(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        target: &CoreTeamRevision,
    ) -> Result<(FrozenRoster, bool), ApiError> {
        if let Some(current) = self.optional_frozen_roster(project_id, epic_id)? {
            return Ok((current, false));
        }
        let epic = self.epic_row(project_id, epic_id)?;
        Ok((
            FrozenRoster {
                revision: CoreTeamRevision {
                    version: target.version,
                    catalog_hash: target.catalog_hash.clone(),
                    seats: Vec::new(),
                },
                revision_of_epic: epic.revision,
                quick_session_id: None,
            },
            true,
        ))
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
        current: &FrozenRoster,
        bootstrap: bool,
        target: &CoreTeamRevision,
        effects: &[TopologyUpgradeEffectDto],
    ) -> Result<ContentHash, ApiError> {
        self.preview_hash(&serde_json::json!({
            "schema_version": 1,
            "operation": "epic_roster_upgrade_preview",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "bootstrap": bootstrap,
            "source_version": current.revision.version.get(),
            "source_revision": current.revision_of_epic.get(),
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
        let target = self.stored_core_team(project_id)?.ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this project has published no Core Team revision",
            )
        })?;
        let (current, bootstrap) = self.roster_upgrade_baseline(project_id, epic_id, &target)?;
        let effects = roster_upgrade_effects(&current, &target)
            .map_err(|error| self.refuse_domain(&error))?;
        if &self.roster_upgrade_hash(project_id, epic_id, &current, bootstrap, &target, &effects)?
            != preview_hash
        {
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
            seat.native_seat = seat
                .seat_binding_id
                .map(|seat_binding_id| {
                    state
                        .with_store(|store| {
                            store.get_hosted_topology_seat(project_id, seat_binding_id)
                        })
                        .map_err(|error| self.refuse(&error))
                })
                .transpose()?
                .flatten()
                .map(|native| CoreTeamNativeSeatDto {
                    runtime_kind: native.native_identity.runtime_kind,
                    host: native.native_identity.host.as_str().to_owned(),
                    generation: native.native_identity.generation,
                    native_id: native.native_identity.native_id,
                    provider_session_id: native.provider_session_id,
                    model_route: RuntimeModelRouteRequest {
                        provider: native.model_rung.provider.0,
                        model: native.model_rung.model.0,
                        effort: native
                            .model_rung
                            .effort
                            .map(|effort| effort.as_str().to_owned()),
                    },
                    observed_at: native.observed_at,
                });
        }
        Ok(CoreTeamDto {
            realm_id: state.realm_id(),
            project_id,
            seats,
            revision: roster.revision_of_epic,
            snapshot_cursor: self.cursor()?,
        })
    }

    fn core_team_route_plan(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &CoreTeamRoutePreviewRequest,
    ) -> Result<CoreTeamRoutePlan, ApiError> {
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
        let roster = self.frozen_roster(project_id, epic_id)?;
        let binding = state
            .with_store(|store| store.get_seat_binding(project_id, request.seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|binding| binding.is_non_terminal())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the requested persistent Core Team SeatBinding is not active",
                )
            })?;
        let node = state
            .with_store(|store| store.get_topology_node(project_id, binding.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|node| {
                node.mini_project_id == Some(epic_id)
                    && node.kind == self.domain.delivery.control_kind
                    && node.task_id.is_none()
            })
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the SeatBinding is not hosted by this epic's control plane",
                )
            })?;
        let _ = node;
        if !roster.revision.seats.iter().any(|seat| {
            seat.presence != EpicPresence::OnDemand
                && seat.role_slot_id == binding.role_slot_id
                && seat.role.role_code == binding.role.role_code
        }) {
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "the SeatBinding is not one of this epic's frozen Core Team roles",
            ));
        }
        let active = state
            .with_store(|store| store.get_hosted_topology_seat(project_id, request.seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::StaleBinding,
                    "the logical Core Team seat has no exact native session to reroute",
                )
            })?;
        let desired = parse_runtime_model_route(&request.desired_model_route)
            .map_err(|error| self.refuse_domain(&error))?;
        let (predecessor, successor) = if active.native_identity.native_id
            == request.expected_native_id
            && active.native_identity.generation == request.expected_generation
        {
            (active, None)
        } else {
            let predecessor = state
                .with_store(|store| {
                    store.get_hosted_topology_seat_history(
                        project_id,
                        request.seat_binding_id,
                        &request.expected_native_id,
                    )
                })
                .map_err(|error| self.refuse(&error))?
                .filter(|predecessor| {
                    predecessor.native_identity.generation == request.expected_generation
                })
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::RevisionConflict,
                        "the native Core Team predecessor moved since the caller read it",
                    )
                })?;
            if active.model_rung != desired {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the logical Core Team seat is active on another unpreviewed route",
                ));
            }
            (predecessor, Some(active))
        };
        let runtime_kind = self.node_runtime_kind()?;
        let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime selected for Core Team placement is not configured",
            )
        })?;
        if !adapter.provider_available(desired.provider.0.as_str()) {
            return Err(ApiError::from_runtime(
                state.realm_id(),
                &RuntimeError::ProviderUnavailable {
                    provider: desired.provider.0.clone(),
                },
            ));
        }
        let preview_hash = self.preview_hash(&serde_json::json!({
            "schema_version": 1,
            "operation": "core_team_route_correction",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "epic_revision": epic.revision.get(),
            "seat_binding": binding.id.to_string(),
            "predecessor": {
                "runtime_kind": predecessor.native_identity.runtime_kind.as_str(),
                "host": predecessor.native_identity.host.as_str(),
                "generation": predecessor.native_identity.generation,
                "native_id": predecessor.native_identity.native_id.as_str(),
                "model": predecessor.model_rung,
            },
            "desired": desired,
        }))?;
        Ok(CoreTeamRoutePlan {
            epic,
            roster,
            binding,
            predecessor,
            successor,
            desired,
            preview_hash,
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
                    native_seat: None,
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

    // -----------------------------------------------------------------------
    // Consultation profile catalogs
    // -----------------------------------------------------------------------
    //
    // Publishing a policy document is the whole of this surface. Invoking one,
    // recording findings and settling belong to the durable services that own
    // the runs, and those remain refused until they exist: a published profile
    // creates no ASW, no CSW and no seat.

    /// Every published revision of one consultation family, oldest first.
    fn consultation_catalog(
        &self,
        project_id: ProjectId,
        family: ConsultationFamily,
    ) -> Result<ProfileCatalogDto, ApiError> {
        let state = self.state()?;
        // Resolved first so an unknown project is refused rather than answered
        // with the empty catalog every unknown project would appear to have.
        self.project_row(project_id)?;
        let stored = self.stored_consultation_profiles(project_id, family)?;
        Ok(ProfileCatalogDto {
            realm_id: state.realm_id(),
            project_id,
            revisions: stored.iter().map(consultation_revision_dto).collect(),
            revision: consultation_catalog_revision(stored.len()),
            snapshot_cursor: self.cursor()?,
        })
    }

    fn stored_consultation_profiles(
        &self,
        project_id: ProjectId,
        family: ConsultationFamily,
    ) -> Result<Vec<StoredConsultationProfileRevision>, ApiError> {
        let state = self.state()?;
        if family == ConsultationFamily::Committee {
            let published = state
                .with_store(|store| store.list_consultation_profile_revisions(project_id, family))
                .map_err(|error| self.refuse(&error))?;
            let presets = kontor_profiles::seeds::bundled_consultation_presets()
                .map_err(|error| self.refuse_domain(&error))?;
            for template in presets.committee_templates {
                let canonical = template
                    .canonicalize()
                    .map_err(|error| self.refuse_domain(&error))?;
                let id = template.template_id.to_string();
                if let Some(existing) = published.iter().find(|revision| {
                    revision.profile_id == id && revision.version == template.version
                }) {
                    if existing.definition_hash != *canonical.hash() {
                        return Err(self.deny(
                            ApiErrorCode::PlacementBlocked,
                            "a built-in Committee preset identity names different published bytes",
                        ));
                    }
                    continue;
                }
                state
                    .with_store(|store| {
                        store.publish_consultation_profile_revision(
                            &StoredConsultationProfileRevision {
                                project_id,
                                family,
                                profile_id: id.clone(),
                                version: template.version,
                                name: template.name.clone(),
                                definition: canonical.json().to_owned(),
                                definition_hash: canonical.hash().clone(),
                                published_at: kontor_api::now(),
                            },
                        )
                    })
                    .map_err(|error| self.refuse(&error))?;
            }
        }
        state
            .with_store(|store| store.list_consultation_profile_revisions(project_id, family))
            .map_err(|error| self.refuse(&error))
    }

    /// Judge one candidate definition and commit nothing.
    ///
    /// A pure read: it deserializes into the family schema, validates and
    /// canonicalizes it, and returns violations plus the hash an apply must
    /// name. No draft, receipt, id or aggregate is written.
    fn preview_consultation_profile(
        &self,
        project_id: ProjectId,
        family: ConsultationFamily,
        request: &ProfilePreviewRequest,
    ) -> Result<ProfilePreviewDto, ApiError> {
        let state = self.state()?;
        self.project_row(project_id)?;
        Ok(ProfilePreviewDto {
            realm_id: state.realm_id(),
            violations: consultation_definition(family, &request.definition)
                .err()
                .map_or_else(Vec::new, |violation| vec![violation]),
            preview_hash: self.consultation_preview_hash(
                project_id,
                family,
                &request.definition,
            )?,
        })
    }

    /// The token a preview hands out and its apply must present.
    ///
    /// Bound to the project and the family as well as to the document, so a
    /// preview taken against one catalog cannot authorize a publish into the
    /// other.
    fn consultation_preview_hash(
        &self,
        project_id: ProjectId,
        family: ConsultationFamily,
        definition: &serde_json::Value,
    ) -> Result<ContentHash, ApiError> {
        self.preview_hash(&serde_json::json!({
            "schema_version": 1,
            "operation": "consultation_profile_preview",
            "project": project_id.to_string(),
            "family": family.as_str(),
            "definition": definition,
        }))
    }

    /// Publish one revalidated definition as the next immutable revision.
    fn apply_consultation_profile(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        family: ConsultationFamily,
        request: &ProfileApplyRequest,
    ) -> Result<AppliedProfileDto, ApiError> {
        let state = self.state()?;
        let project = self.project_row(project_id)?;
        // Revalidated here rather than trusted from the preview. What may be
        // published is this exact document, and an unpublishable one is refused
        // whatever revision the catalog is at.
        // The refusal is deliberately detail-free: a rejection reason travels
        // through `preview`, whose `violations` are typed for it, and not
        // through an error message that would carry document text into logs.
        let definition = consultation_definition(family, &request.definition).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "the definition is not publishable; preview it to see why",
            )
        })?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "consultation_profile_apply",
            "project": project_id.to_string(),
            "family": family.as_str(),
            "preview": request.preview_hash.as_str(),
        }))?;
        // Replay is judged before the expected revision, as in `apply_core_team`
        // and for the same reason: publishing moves this catalog, so a retry
        // after a lost acknowledgement necessarily presents the revision it read
        // before the first attempt. Checking that first would refuse the retry
        // for the sole reason that the original call succeeded.
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();

        if !replayed {
            let stored = self.stored_consultation_profiles(project_id, family)?;
            let current = consultation_catalog_revision(stored.len());
            if current != request.expected_revision {
                return Err(self
                    .deny(
                        ApiErrorCode::RevisionConflict,
                        "the profile catalog moved since the caller read it",
                    )
                    .with_revision(Some(current)));
            }
            if self.consultation_preview_hash(project_id, family, &request.definition)?
                != request.preview_hash
            {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "the apply does not match the named preview",
                ));
            }
            let canonical = definition
                .canonicalize()
                .map_err(|error| self.refuse_domain(&error))?;
            // The store holds the gap check: version one starts a profile and
            // every later version must be exactly the next one, so a caller
            // cannot publish over a revision a run has already pinned.
            state
                .with_store(|store| {
                    store.publish_consultation_profile_revision(
                        &StoredConsultationProfileRevision {
                            project_id,
                            family,
                            profile_id: definition.profile_id(),
                            version: definition.version(),
                            name: definition.name(),
                            definition: canonical.json().to_owned(),
                            definition_hash: canonical.hash().clone(),
                            published_at: kontor_api::now(),
                        },
                    )
                })
                .map_err(|error| self.refuse(&error))?;
        }

        // Read back rather than echoed, so a replay answers with the revision
        // the original call published instead of with the request it repeated.
        let stored = self.stored_consultation_profiles(project_id, family)?;
        let published = stored
            .iter()
            .find(|candidate| {
                candidate.profile_id == definition.profile_id()
                    && candidate.version == definition.version()
            })
            .map(consultation_revision_dto)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the published revision could not be read back",
                )
            })?;
        let receipt_id = self.record(
            key,
            project_id,
            match family {
                ConsultationFamily::Advisor => CommandKind::ApplyAdvisorProfile,
                ConsultationFamily::Committee => CommandKind::ApplyCommitteeTemplate,
            },
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        Ok(AppliedProfileDto {
            published,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: consultation_catalog_revision(stored.len()),
                snapshot_cursor: self.cursor()?,
            },
        })
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
                node_id: None,
                kind: None,
                epic_id: None,
                task_id: None,
                key: "project_root".to_owned(),
            },
            SemanticTopologyTargetDto::QuickSession { quick_session_id } => TopologyScope {
                node_id: None,
                kind: Some(delivery.quick_kind.clone()),
                epic_id: None,
                task_id: None,
                key: format!("quick_session:{quick_session_id}"),
            },
            SemanticTopologyTargetDto::Epic { epic_id } => TopologyScope {
                node_id: None,
                kind: Some(delivery.epic_kind.clone()),
                epic_id: Some(self.epic_row(project_id, *epic_id)?.id),
                task_id: None,
                key: format!("epic:{epic_id}"),
            },
            SemanticTopologyTargetDto::EpicControl { epic_id } => TopologyScope {
                node_id: None,
                kind: Some(delivery.control_kind.clone()),
                epic_id: Some(self.epic_row(project_id, *epic_id)?.id),
                task_id: None,
                key: format!("epic_control:{epic_id}"),
            },
            SemanticTopologyTargetDto::Ticket { task_id } => {
                let task = self.task_row(project_id, *task_id)?;
                TopologyScope {
                    node_id: None,
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
            SemanticTopologyTargetDto::AdvisorConsultation { advisor_run_id } => {
                let run =
                    self.consultation_run(project_id, ConsultationRunId::Advisor(*advisor_run_id))?;
                TopologyScope {
                    node_id: Some(run.topology_node_id),
                    kind: Some(delivery.advisor_kind.clone()),
                    epic_id: Some(run.mini_project_id),
                    task_id: None,
                    key: format!("advisor_consultation:{advisor_run_id}"),
                }
            }
            SemanticTopologyTargetDto::CommitteeConsultation { committee_run_id } => {
                let run = self.consultation_run(
                    project_id,
                    ConsultationRunId::Committee(*committee_run_id),
                )?;
                TopologyScope {
                    node_id: Some(run.topology_node_id),
                    kind: Some(delivery.committee_kind.clone()),
                    epic_id: Some(run.mini_project_id),
                    task_id: None,
                    key: format!("committee_consultation:{committee_run_id}"),
                }
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

        if let Some(node_id) = scope.node_id {
            return state
                .with_store(|store| store.get_topology_node(project_id, node_id))
                .map_err(|error| self.refuse(&error))?
                .filter(|node| {
                    node.mini_project_id == scope.epic_id && Some(&node.kind) == scope.kind.as_ref()
                })
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the consultation's frozen topology node is missing or changed scope",
                    )
                });
        }

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
    /// Everything a container retitle needs, derived from what Kontor holds.
    ///
    /// Shared by the preview and the apply, so a preview cannot describe a
    /// different container, a different title or a different refusal than the
    /// apply would.
    ///
    /// Nothing here comes from the caller except the node and the revision it was
    /// read at. The container is the one this node is *bound* to, the structural
    /// name comes from the node kind's template in the pinned specification, and
    /// the task scope that completes it is the plane's — which is why the finished
    /// title is the runtime's to render and not this function's.
    ///
    /// # Errors
    /// * [`ApiErrorCode::NotFound`] — no such project, no such node, or a node
    ///   holding no native container to repair.
    /// * [`ApiErrorCode::RevisionConflict`] — the project moved since the caller
    ///   read it.
    /// * [`ApiErrorCode::Unavailable`] — this daemon is not configured with the
    ///   runtime family the container was bound by.
    fn retitle_request(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        request: &ContainerRetitleRequest,
    ) -> Result<(RetitleContainerRequest, Arc<dyn RuntimeAdapter>), ApiError> {
        let state = self.state()?;
        // The project's revision, like every other semantic topology write: it is
        // the aggregate this authority is over, and the one revision a caller can
        // actually read before presenting it.
        self.project_at(project_id, request.expected_revision)?;
        let node = state
            .with_store(|store| store.get_topology_node(project_id, topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such topology node exists in this project",
                )
            })?;
        let spec = state
            .with_store(|store| {
                store.get_topology_spec(project_id, node.topology.spec_id, node.topology.version)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the node's pinned topology revision is not published in this project",
                )
            })?;
        let binding = state
            .with_store(|store| store.get_topology_node_container(project_id, topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "this topology node holds no native container to retitle",
                )
            })?;
        let adapter = state
            .runtimes()
            .get(&binding.identity.runtime_kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "this daemon is not configured with the runtime that holds this container",
                )
            })?;
        let scope = node
            .mini_project_id
            .map(|epic_id| {
                self.execution_scope(project_id, epic_id, node.task_id, adapter.as_ref())
            })
            .transpose()?;
        let projection = match binding.observed_kind {
            ObservedContainerKind::Project => ContainerProjection::NativeRoot,
            ObservedContainerKind::Workspace => ContainerProjection::NativeChild,
        };
        // Paseo can fetch a workspace only inside an exact native project. That
        // ancestry is durable Kontor state, whereas the adapter's prepared-plane
        // cache is intentionally empty after a daemon restart. Walk the logical
        // parents until their persisted binding names the native project; never
        // scan every runtime project or infer the parent from a mutable title.
        let bound_project_native_id = if projection == ContainerProjection::NativeChild {
            let mut parent_id = node.parent_id;
            let mut found = None;
            while let Some(candidate_id) = parent_id {
                let candidate = state
                    .with_store(|store| store.get_topology_node(project_id, candidate_id))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::PlacementBlocked,
                            "the container's parent is not in this project's topology",
                        )
                    })?;
                if let Some(candidate_binding) = state
                    .with_store(|store| store.get_topology_node_container(project_id, candidate_id))
                    .map_err(|error| self.refuse(&error))?
                    && candidate_binding.observed_kind == ObservedContainerKind::Project
                {
                    found = Some(candidate_binding.identity.native_id);
                    break;
                }
                parent_id = candidate.parent_id;
            }
            Some(found.ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the native child has no persisted native project ancestor",
                )
            })?)
        } else {
            None
        };
        Ok((
            RetitleContainerRequest {
                topology_node_id,
                container_binding_id: ContainerBindingId::parse(
                    binding.container_binding_id.as_str(),
                )
                .map_err(|error| self.refuse_domain(&error))?,
                projection,
                bound_native_id: binding.identity.native_id.clone(),
                bound_project_native_id,
                generation: binding.identity.generation,
                desired_title: self.container_name(&spec, &node, scope.as_ref())?,
                requested_at: kontor_api::now(),
            },
            adapter,
        ))
    }

    /// Preflight every existing native container and persistent seat in one
    /// epic. The returned actions are usable only after the whole loop has
    /// completed. Persisted seats that have no native session yet are omitted;
    /// exact seats whose runtime session is temporarily unavailable remain in
    /// the plan as `rename_pending` and produce no native action. Structural
    /// ambiguity and identity drift still refuse the complete plan before any
    /// write.
    async fn prepare_native_names(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        expected_revision: AggregateRevision,
    ) -> Result<PreparedNativeNames, ApiError> {
        let state = self.state()?;
        let project = self.project_at(project_id, expected_revision)?;
        state
            .with_store(|store| store.get_mini_project(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such epic exists in this project",
                )
            })?;

        let nodes = state
            .with_store(|store| store.list_topology_nodes(project_id, Some(epic_id)))
            .map_err(|error| self.refuse(&error))?;
        let mut targets = Vec::new();
        let mut actions = Vec::new();

        for node in nodes {
            if state
                .with_store(|store| store.get_topology_node_container(project_id, node.id))
                .map_err(|error| self.refuse(&error))?
                .is_some()
            {
                let (request, adapter) = self.retitle_request(
                    project_id,
                    node.id,
                    &ContainerRetitleRequest { expected_revision },
                )?;
                let outcome = adapter
                    .preview_retitle_container(&request)
                    .await
                    .map_err(|error| {
                        if request.projection == ContainerProjection::NativeRoot
                            && matches!(error, RuntimeError::UnsupportedCapability { .. })
                        {
                            self.deny(
                                ApiErrorCode::RenamePending,
                                "the native root requires an identity-preserving project rename this runtime cannot prove",
                            )
                        } else {
                            ApiError::from_runtime(state.realm_id(), &error)
                        }
                    })?;
                if outcome.snapshot.binding.identity.native_id != request.bound_native_id
                    || outcome.desired_title != request.desired_title
                {
                    return Err(self.deny(
                        ApiErrorCode::StaleBinding,
                        "container-name preflight read back another identity or desired title",
                    ));
                }
                targets.push(NativeNameTargetDto {
                    subject_kind: NativeNameSubjectKindDto::Container,
                    topology_node_id: node.id,
                    seat_binding_id: None,
                    agent_run_id: None,
                    native_id: request.bound_native_id.clone(),
                    provider_session_id: None,
                    observed_title: Some(outcome.observed_title),
                    desired_title: request.desired_title.clone(),
                    would_change: outcome.changed,
                    capability: if outcome.changed {
                        "ready".to_owned()
                    } else {
                        "unchanged".to_owned()
                    },
                });
                if outcome.changed {
                    actions.push(NativeNameAction::Container { request, adapter });
                }
            }

            let seats = state
                .with_store(|store| store.list_seat_bindings(project_id, node.id))
                .map_err(|error| self.refuse(&error))?;
            for seat in seats {
                if seat.lifecycle != TopologyLifecycle::Active {
                    continue;
                }
                let hosted = state
                    .with_store(|store| store.get_hosted_topology_seat(project_id, seat.id))
                    .map_err(|error| self.refuse(&error))?;
                let consultation = if hosted.is_none() {
                    state
                        .with_store(|store| {
                            store.get_consultation_seat_by_binding(project_id, seat.id)
                        })
                        .map_err(|error| self.refuse(&error))?
                } else {
                    None
                };
                let delivery = if hosted.is_none() && consultation.is_none() {
                    seat.team_run_id
                        .map(|team_run_id| {
                            self.current_delivery_role_leaf(
                                project_id,
                                team_run_id,
                                seat.role_slot_id.as_role_key(),
                            )
                        })
                        .transpose()?
                        .flatten()
                } else {
                    None
                };
                let hosted_seat_binding_id = hosted.as_ref().map(|seat| seat.seat_binding_id);
                let persisted_hosted_provider_session = hosted
                    .as_ref()
                    .and_then(|seat| seat.provider_session_id.clone());
                let (identity, agent_run_id) = if let Some(hosted) = hosted {
                    (hosted.native_identity, None)
                } else if let Some(consultation) = consultation {
                    let Some(identity) = consultation.native_identity else {
                        continue;
                    };
                    (identity, None)
                } else if let Some(delivery) = delivery {
                    let agent_run_id = delivery.id;
                    let Some(binding) = delivery.binding else {
                        // Only the unique current leaf may be genuinely
                        // unbound. A bound predecessor never substitutes for it.
                        continue;
                    };
                    (binding.identity, Some(agent_run_id))
                } else {
                    // A declared but not-yet-materialized seat has no native
                    // title to repair and therefore is not an apply target.
                    continue;
                };
                let container = state
                    .with_store(|store| store.get_topology_node_container(project_id, node.id))
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::PlacementBlocked,
                            "a native seat has no persisted native host container",
                        )
                    })?;
                let adapter = state
                    .runtimes()
                    .get(&identity.runtime_kind)
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::Unavailable,
                            "a persistent seat's runtime is not configured in this daemon",
                        )
                    })?;
                let scope =
                    self.execution_scope(project_id, epic_id, node.task_id, adapter.as_ref())?;
                let desired_title =
                    self.seat_name(project_id, &node, &scope, &seat.role.role_code)?;
                let mut request = RetitleSeatRequest {
                    identity,
                    // The durable native agent and its exact host are the stable
                    // identity. Paseo may resume that same agent onto a new
                    // provider conversation, so preview learns the current
                    // handle and freezes it into the apply request below.
                    provider_session_id: None,
                    container_native_id: container.identity.native_id,
                    desired_title,
                    requested_at: kontor_api::now(),
                };
                let outcome = match adapter.preview_retitle_seat(&request).await {
                    Ok(outcome) => outcome,
                    Err(
                        RuntimeError::StaleBinding { .. }
                        | RuntimeError::ProviderUnavailable { .. },
                    ) => {
                        targets.push(NativeNameTargetDto {
                            subject_kind: NativeNameSubjectKindDto::Seat,
                            topology_node_id: node.id,
                            seat_binding_id: Some(seat.id),
                            agent_run_id,
                            native_id: request.identity.native_id.clone(),
                            provider_session_id: request.provider_session_id.clone(),
                            observed_title: None,
                            desired_title: request.desired_title.clone(),
                            would_change: false,
                            capability: "rename_pending".to_owned(),
                        });
                        continue;
                    }
                    Err(error) => {
                        return Err(ApiError::from_runtime(state.realm_id(), &error));
                    }
                };
                if outcome.identity != request.identity
                    || outcome.container_native_id != request.container_native_id
                    || outcome.observed_title == request.desired_title.as_str() && outcome.changed
                    || outcome.observed_title != request.desired_title.as_str() && !outcome.changed
                {
                    return Err(self.deny(
                        ApiErrorCode::StaleBinding,
                        "seat-name preflight returned inconsistent identity or change evidence",
                    ));
                }
                // A provider id learned on readback is part of the exact apply
                // correlation. The caller never supplies or changes it.
                request.provider_session_id = outcome.provider_session_id.clone();
                targets.push(NativeNameTargetDto {
                    subject_kind: NativeNameSubjectKindDto::Seat,
                    topology_node_id: node.id,
                    seat_binding_id: Some(seat.id),
                    agent_run_id,
                    native_id: request.identity.native_id.clone(),
                    provider_session_id: outcome.provider_session_id.clone(),
                    observed_title: Some(outcome.observed_title),
                    desired_title: request.desired_title.clone(),
                    would_change: outcome.changed,
                    capability: if outcome.changed {
                        "ready".to_owned()
                    } else {
                        "unchanged".to_owned()
                    },
                });
                if outcome.changed
                    || (hosted_seat_binding_id.is_some()
                        && persisted_hosted_provider_session != outcome.provider_session_id)
                {
                    actions.push(NativeNameAction::Seat {
                        request,
                        adapter,
                        hosted_seat_binding_id,
                    });
                }
            }
        }

        let preview_hash = self.preview_hash(&serde_json::json!({
            "schema_version": 1,
            "project_id": project_id.to_string(),
            "epic_id": epic_id.to_string(),
            "project_revision": project.revision.get(),
            "targets": targets,
        }))?;
        Ok(PreparedNativeNames {
            preview: NativeNamesPreviewDto {
                realm_id: state.realm_id(),
                project_id,
                epic_id,
                targets,
                preview_hash,
                snapshot_cursor: self.cursor()?,
            },
            actions,
        })
    }

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
        // An epic-scoped projection reports the epic's immutable pin, not the
        // project's default. The two intentionally diverge after an authorized
        // per-epic upgrade; reading the default here made a successful apply
        // appear to remain on the old revision and interpreted every node
        // through the wrong vocabulary.
        let topology = match epic_id {
            Some(epic_id) => self.epic_pin(project_id, epic_id)?,
            None => self.project_topology(project_id)?,
        };
        let spec = state
            .with_store(|store| {
                store.get_topology_spec(project_id, topology.spec_id, topology.version)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the projected topology revision is not published in this project",
                )
            })?;
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

    /// Read one durable consultation by its family-qualified identity.
    fn consultation_run(
        &self,
        project_id: ProjectId,
        run_id: ConsultationRunId,
    ) -> Result<StoredConsultationRun, ApiError> {
        self.state()?
            .with_store(|store| store.get_consultation_run(project_id, run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such consultation run exists in this project",
                )
            })
    }

    /// Resolve and revalidate the immutable Advisor profile a run pins.
    fn advisor_profile(
        &self,
        run: &StoredConsultationRun,
    ) -> Result<(StoredConsultationProfileRevision, AdvisorProfileSpec), ApiError> {
        if run.id.family() != ConsultationFamily::Advisor {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this operation requires an Advisor run",
            ));
        }
        let stored = self
            .stored_consultation_profiles(run.project_id, ConsultationFamily::Advisor)?
            .into_iter()
            .find(|revision| {
                revision.profile_id == run.profile_id
                    && revision.version == run.profile_version
                    && revision.definition_hash == run.definition_hash
            })
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the Advisor run's pinned profile revision is unavailable",
                )
            })?;
        let profile: AdvisorProfileSpec =
            serde_json::from_str(&stored.definition).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the stored Advisor profile cannot be read by this build",
                )
            })?;
        profile
            .validate()
            .map_err(|error| self.refuse_domain(&error))?;
        Ok((stored, profile))
    }

    /// Prove that the exact active epic seat may invoke this policy at scope.
    fn authorize_consultation_caller(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &InvokeConsultationRequest,
        allowed_scopes: &[ConsultationScope],
        allowed_roles: &[RoleKey],
    ) -> Result<kontor_core::state::SeatBinding, ApiError> {
        let state = self.state()?;
        if let Some(task_id) = request.task_id {
            let task = self.task_row(project_id, task_id)?;
            if task.mini_project_id != Some(epic_id) {
                return Err(self.deny(
                    ApiErrorCode::Forbidden,
                    "the requested ticket does not belong to this epic",
                ));
            }
            if !allowed_scopes.contains(&ConsultationScope::Ticket) {
                return Err(self.deny(
                    ApiErrorCode::Forbidden,
                    "the pinned consultation policy does not permit ticket scope",
                ));
            }
        } else if !allowed_scopes.contains(&ConsultationScope::Epic) {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "the pinned consultation policy does not permit epic scope",
            ));
        }
        let seat = state
            .with_store(|store| store.get_seat_binding(project_id, request.caller_seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|seat| seat.is_non_terminal() && !seat.closes_children())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::StaleBinding,
                    "the caller SeatBinding is not active",
                )
            })?;
        let node = state
            .with_store(|store| store.get_topology_node(project_id, seat.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|node| node.mini_project_id == Some(epic_id))
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Forbidden,
                    "the caller SeatBinding does not belong to this epic",
                )
            })?;
        if node.lifecycle != TopologyLifecycle::Active {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the caller SeatBinding's topology node is not active",
            ));
        }
        let slot_role = seat.role_slot_id.as_role_key().as_str();
        let catalog_role = seat.role.role_code.as_str();
        if !allowed_roles.iter().any(|allowed| {
            allowed.as_str().eq_ignore_ascii_case(slot_role)
                || allowed.as_str().eq_ignore_ascii_case(catalog_role)
        }) {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "the caller SeatBinding's role may not invoke this consultation",
            ));
        }
        Ok(seat)
    }

    /// Resolve and revalidate the immutable Committee template a run pins.
    fn committee_template(
        &self,
        run: &StoredConsultationRun,
    ) -> Result<(StoredConsultationProfileRevision, CommitteeTemplateSpec), ApiError> {
        if run.id.family() != ConsultationFamily::Committee {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this operation requires a Committee run",
            ));
        }
        let stored = self
            .stored_consultation_profiles(run.project_id, ConsultationFamily::Committee)?
            .into_iter()
            .find(|revision| {
                revision.profile_id == run.profile_id
                    && revision.version == run.profile_version
                    && revision.definition_hash == run.definition_hash
            })
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the Committee run's pinned template revision is unavailable",
                )
            })?;
        let template: CommitteeTemplateSpec =
            serde_json::from_str(&stored.definition).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the stored Committee template cannot be read by this build",
                )
            })?;
        template
            .validate()
            .map_err(|error| self.refuse_domain(&error))?;
        Ok((stored, template))
    }

    /// Prove that the exact active seat may invoke this template at this scope.
    fn authorize_committee_caller(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &InvokeConsultationRequest,
        template: &CommitteeTemplateSpec,
    ) -> Result<kontor_core::state::SeatBinding, ApiError> {
        let state = self.state()?;
        if let Some(task_id) = request.task_id {
            let task = self.task_row(project_id, task_id)?;
            if task.mini_project_id != Some(epic_id) {
                return Err(self.deny(
                    ApiErrorCode::Forbidden,
                    "the requested ticket does not belong to this epic",
                ));
            }
            if !template.allowed_scopes.contains(&ConsultationScope::Ticket) {
                return Err(self.deny(
                    ApiErrorCode::Forbidden,
                    "the pinned Committee template does not permit ticket-scoped invocation",
                ));
            }
        } else if !template.allowed_scopes.contains(&ConsultationScope::Epic) {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "the pinned Committee template does not permit epic-scoped invocation",
            ));
        }
        let seat = state
            .with_store(|store| store.get_seat_binding(project_id, request.caller_seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|seat| seat.is_non_terminal() && !seat.closes_children())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::StaleBinding,
                    "the caller SeatBinding is not active",
                )
            })?;
        let node = state
            .with_store(|store| store.get_topology_node(project_id, seat.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|node| node.mini_project_id == Some(epic_id))
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Forbidden,
                    "the caller SeatBinding does not belong to this epic",
                )
            })?;
        if node.lifecycle != TopologyLifecycle::Active {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the caller SeatBinding's topology node is not active",
            ));
        }
        let slot_role = seat.role_slot_id.as_role_key().as_str();
        let catalog_role = seat.role.role_code.as_str();
        if !template.allowed_caller_roles.iter().any(|allowed| {
            allowed.as_str().eq_ignore_ascii_case(slot_role)
                || allowed.as_str().eq_ignore_ascii_case(catalog_role)
        }) {
            return Err(self.deny(
                ApiErrorCode::Forbidden,
                "the caller SeatBinding's role may not invoke this Committee template",
            ));
        }
        Ok(seat)
    }

    /// Freeze one Advisor and its sole logical seat before the native effect.
    #[allow(clippy::too_many_arguments)]
    fn freeze_advisor_run(
        &self,
        key: &IdempotencyKey,
        intent: &CanonicalDocument,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &InvokeConsultationRequest,
        revision: &StoredConsultationProfileRevision,
        profile: &AdvisorProfileSpec,
        caller: &kontor_core::state::SeatBinding,
    ) -> Result<StoredConsultationRun, ApiError> {
        let state = self.state()?;
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
        let run_id = AdvisorRunId::generate();
        let node_id = TopologyNodeId::generate();
        let question_hash = ContentHash::of(request.question.as_str().as_bytes());
        let context = self.intent(&serde_json::json!({
            "schema_version": 1,
            "realm_id": state.realm_id().to_string(),
            "project_id": project_id.to_string(),
            "epic_id": epic_id.to_string(),
            "task_id": request.task_id.map(|id| id.to_string()),
            "caller_seat_binding_id": caller.id.to_string(),
            "profile_id": revision.profile_id,
            "profile_version": revision.version.get(),
            "profile_hash": revision.definition_hash.as_str(),
            "question_hash": question_hash.as_str(),
        }))?;
        let run = StoredConsultationRun {
            id: ConsultationRunId::Advisor(run_id),
            project_id,
            mini_project_id: epic_id,
            profile_id: revision.profile_id.clone(),
            profile_version: revision.version,
            definition_hash: revision.definition_hash.clone(),
            question: request.question.clone(),
            question_hash,
            context: serde_json::from_str(context.json()).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the frozen Advisor context could not be decoded",
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
            kind: self.domain.delivery.advisor_kind.clone(),
            parent_id: Some(epic_node.id),
            task_id: request.task_id,
            created_at: now,
        };
        let logical_role = RoleKey::parse("advisor").map_err(|error| self.refuse_domain(&error))?;
        let role_slot_id =
            RoleSlotId::parse("advisor").map_err(|error| self.refuse_domain(&error))?;
        let role_code = self
            .domain
            .delivery
            .role_code(&logical_role)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the delivery binding has no Advisor role",
                )
            })?;
        let seat_binding_id = SeatBindingId::generate();
        let deadline = now
            .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
            .unwrap_or(now);
        let seat = StoredConsultationSeat {
            run_id: run.id,
            role_slot_id: role_slot_id.clone(),
            committee_role: None,
            logical_role,
            seat_binding_id,
            model_rung: profile.models.rungs.first().cloned().ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the Advisor profile has no model route",
                )
            })?,
            native_identity: None,
            provider_session_id: None,
            observed_at: None,
        };
        let binding = NewSeatBinding {
            id: seat_binding_id,
            project_id,
            topology_node_id: node_id,
            role_slot_id,
            role: self.catalog_role_for_code(role_code)?,
            task_id: request.task_id,
            team_run_id: None,
            attach_deadline: deadline,
            parent_seat_binding_id: Some(caller.id),
            created_at: now,
        };
        state
            .with_store(|store| store.create_consultation_run(&run, &node, &[(&seat, &binding)]))
            .map_err(|error| self.refuse(&error))?;
        Ok(run)
    }

    /// Launch or exact-label recover the one Advisor seat.
    async fn materialize_advisor_seat(
        &self,
        run: &StoredConsultationRun,
        profile: &AdvisorProfileSpec,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let project = self.project_row(run.project_id)?;
        let node = state
            .with_store(|store| store.get_topology_node(run.project_id, run.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the Advisor topology node is missing",
                )
            })?;
        let runtime_kind = self.node_runtime_kind()?;
        let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime selected for Advisor placement is not configured",
            )
        })?;
        let cwd = WorkspaceRoot::parse(project.root_path.as_str())
            .map_err(|error| self.refuse_domain(&error))?;
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
        let mut seats = state
            .with_store(|store| store.list_consultation_seats(run.project_id, run.id))
            .map_err(|error| self.refuse(&error))?;
        let seat = seats.first_mut().ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the Advisor run has no frozen seat",
            )
        })?;
        if seat.native_identity.is_some() {
            return Ok(());
        }
        let seat_credential = state
            .credentials()
            .consultation_seat_credential(seat.seat_binding_id);
        let prompt = BoundedText::parse(&format!(
            "Read-only Advisor seat. You may inspect evidence but must not mutate code, Jira, topology, scheduling, or runtime state. Expertise: {} Behavior: {} Question: {} Output requirements: {} Submit only this seat's immutable output using the KONTOR_AUTH environment value. It is valid only for SeatBinding {} and must not be disclosed.",
            profile.expertise.as_str(),
            profile.behavior.as_str(),
            run.question.as_str(),
            profile.output_requirements.as_str(),
            seat.seat_binding_id,
        ))
        .map_err(|error| self.refuse_domain(&error))?;
        let topology_seat = state
            .with_store(|store| store.get_seat_binding(run.project_id, seat.seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the Advisor native seat has no persistent topology binding",
                )
            })?;
        let display_name =
            self.seat_name(run.project_id, &node, &scope, &topology_seat.role.role_code)?;
        let outcome = adapter
            .launch_consultation(&ConsultationLaunchRequest {
                scope,
                run_id: run.id,
                seat_binding_id: seat.seat_binding_id,
                role_slot_id: seat.role_slot_id.clone(),
                display_name,
                container,
                cwd,
                prompt,
                credential: ConsultationCredential::new(seat_credential),
                model_rung: seat.model_rung.clone(),
                context_policy,
                requested_at: kontor_api::now(),
            })
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
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
        Ok(())
    }

    /// Stable wire projection of one Advisor run.
    fn advisor_run_dto(
        &self,
        run: &StoredConsultationRun,
        receipt_id: Option<CommandReceiptId>,
        applied: AppliedDto,
    ) -> Result<AdvisorRunDto, ApiError> {
        let state = self.state()?;
        let (revision, _) = self.advisor_profile(run)?;
        let seats = state
            .with_store(|store| store.list_consultation_seats(run.project_id, run.id))
            .map_err(|error| self.refuse(&error))?;
        let advisor_run_id = match run.id {
            ConsultationRunId::Advisor(id) => id,
            ConsultationRunId::Committee(_) => {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "this projection requires an Advisor run",
                ));
            }
        };
        let advice = state
            .with_store(|store| store.get_advisor_advice(run.project_id, advisor_run_id))
            .map_err(|error| self.refuse(&error))?;
        let receipt = receipt_id
            .map(|receipt_id| {
                Ok(MutationReceiptDto {
                    realm_id: state.realm_id(),
                    receipt_id: receipt_id.to_string(),
                    applied,
                    revision: run.revision,
                    snapshot_cursor: self.cursor()?,
                })
            })
            .transpose()?;
        Ok(AdvisorRunDto {
            realm_id: state.realm_id(),
            advisor_run_id,
            epic_id: run.mini_project_id,
            profile: consultation_revision_dto(&revision),
            topology_node_id: run.topology_node_id,
            seats: seats
                .into_iter()
                .map(|seat| ConsultationSeatDto {
                    role_slot_id: seat.role_slot_id.as_str().to_owned(),
                    logical_role: seat.logical_role.as_str().to_owned(),
                    seat_binding_id: seat.seat_binding_id,
                    observed_binding: seat.native_identity.map(|identity| ObservedBindingDto {
                        runtime_kind: identity.runtime_kind,
                        native_id: identity.native_id,
                        native_name: None,
                        cwd: None,
                        observed_at: seat.observed_at.unwrap_or(run.updated_at),
                    }),
                })
                .collect(),
            state: run.state.as_str().to_owned(),
            advice: advice.map(|advice| advice.document),
            result: run.result.clone(),
            receipt,
        })
    }

    /// Freeze a new Committee and every logical seat before the first native effect.
    fn freeze_committee_run(
        &self,
        key: &IdempotencyKey,
        intent: &CanonicalDocument,
        invocation: CommitteeInvocation<'_>,
    ) -> Result<StoredConsultationRun, ApiError> {
        let CommitteeInvocation {
            project_id,
            epic_id,
            request,
            template_revision,
            template,
            caller,
        } = invocation;
        let state = self.state()?;
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
        let run_id = CommitteeRunId::generate();
        let node_id = TopologyNodeId::generate();
        let question_hash = ContentHash::of(request.question.as_str().as_bytes());
        let context = self.intent(&serde_json::json!({
            "schema_version": 1,
            "realm_id": state.realm_id().to_string(),
            "project_id": project_id.to_string(),
            "epic_id": epic_id.to_string(),
            "task_id": request.task_id.map(|id| id.to_string()),
            "caller_seat_binding_id": caller.id.to_string(),
            "caller_role_slot": caller.role_slot_id.as_str(),
            "template_id": template_revision.profile_id,
            "template_version": template_revision.version.get(),
            "template_hash": template_revision.definition_hash.as_str(),
            "question_hash": question_hash.as_str(),
        }))?;
        let run = StoredConsultationRun {
            id: ConsultationRunId::Committee(run_id),
            project_id,
            mini_project_id: epic_id,
            profile_id: template_revision.profile_id.clone(),
            profile_version: template_revision.version,
            definition_hash: template_revision.definition_hash.clone(),
            question: request.question.clone(),
            question_hash,
            context: serde_json::from_str(context.json()).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the frozen Committee context could not be decoded",
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
            kind: self.domain.delivery.committee_kind.clone(),
            parent_id: Some(epic_node.id),
            task_id: None,
            created_at: now,
        };
        let deadline = now
            .checked_add(jiff::SignedDuration::from_secs(SEAT_ATTACH_SECONDS))
            .unwrap_or(now);
        let mut stored_seats = Vec::with_capacity(template.slots.len());
        let mut bindings = Vec::with_capacity(template.slots.len());
        for slot in &template.slots {
            let code = self
                .domain
                .delivery
                .role_code(&slot.logical_role)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the seeded delivery binding does not map a Committee logical role",
                    )
                })?;
            let seat_binding_id = SeatBindingId::generate();
            stored_seats.push(StoredConsultationSeat {
                run_id: run.id,
                role_slot_id: slot.id.clone(),
                committee_role: Some(slot.role),
                logical_role: slot.logical_role.clone(),
                seat_binding_id,
                model_rung: slot.models.rungs.first().cloned().ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "a Committee slot has no model route",
                    )
                })?,
                native_identity: None,
                provider_session_id: None,
                observed_at: None,
            });
            bindings.push(NewSeatBinding {
                id: seat_binding_id,
                project_id,
                topology_node_id: node_id,
                role_slot_id: slot.id.clone(),
                role: self.catalog_role_for_code(code)?,
                task_id: None,
                team_run_id: None,
                attach_deadline: deadline,
                parent_seat_binding_id: Some(caller.id),
                created_at: now,
            });
        }
        let pairs: Vec<_> = stored_seats.iter().zip(bindings.iter()).collect();
        state
            .with_store(|store| store.create_consultation_run(&run, &node, &pairs))
            .map_err(|error| self.refuse(&error))?;
        Ok(run)
    }

    /// Launch or exact-label recover the Committee seats currently eligible.
    /// Reviewers launch at invoke; the Judge launches only after all independent
    /// findings are durable, so it cannot observe them early or race them.
    async fn materialize_committee_seats(
        &self,
        run: &StoredConsultationRun,
        template: &CommitteeTemplateSpec,
        include_judge: bool,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let project = self.project_row(run.project_id)?;
        let node = state
            .with_store(|store| store.get_topology_node(run.project_id, run.topology_node_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the Committee's frozen topology node is missing",
                )
            })?;
        let runtime_kind = self.node_runtime_kind()?;
        let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime selected for Committee placement is not configured",
            )
        })?;
        let cwd = WorkspaceRoot::parse(project.root_path.as_str())
            .map_err(|error| self.refuse_domain(&error))?;
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
        let findings = if include_judge {
            state
                .with_store(|store| {
                    store.list_committee_findings(
                        run.project_id,
                        match run.id {
                            ConsultationRunId::Committee(id) => id,
                            ConsultationRunId::Advisor(_) => unreachable!(),
                        },
                        run.round,
                    )
                })
                .map_err(|error| self.refuse(&error))?
        } else {
            Vec::new()
        };
        let mut seats = state
            .with_store(|store| store.list_consultation_seats(run.project_id, run.id))
            .map_err(|error| self.refuse(&error))?;
        for seat in &mut seats {
            let slot = template
                .slots
                .iter()
                .find(|slot| slot.id == seat.role_slot_id)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "a frozen Committee seat is absent from its pinned template",
                    )
                })?;
            if seat.native_identity.is_some()
                || (slot.role == CommitteeRole::Judge && !include_judge)
            {
                continue;
            }
            let evidence = if slot.role == CommitteeRole::Judge {
                serde_json::to_string(
                    &findings
                        .iter()
                        .filter(|finding| finding.role == CommitteeRole::Reviewer)
                        .map(|finding| &finding.document)
                        .collect::<Vec<_>>(),
                )
                .map_err(|_| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the durable Committee findings could not be delivered to the Judge",
                    )
                })?
            } else {
                "[]".to_owned()
            };
            let seat_credential = state
                .credentials()
                .consultation_seat_credential(seat.seat_binding_id);
            let prompt = BoundedText::parse(&format!(
                "Read-only Committee seat. You may inspect evidence but must not mutate code, \
                 Jira, topology, scheduling, or runtime state. Charter: {} Role instructions: {} \
                 Question: {} Durable reviewer findings available to this seat: {} \
                 Submit this seat's own finding using the KONTOR_AUTH environment value. \
                 It is valid only for SeatBinding {} and must not be disclosed.",
                template.charter.as_str(),
                slot.behavior.as_str(),
                run.question.as_str(),
                evidence,
                seat.seat_binding_id,
            ))
            .map_err(|error| self.refuse_domain(&error))?;
            let topology_seat = state
                .with_store(|store| store.get_seat_binding(run.project_id, seat.seat_binding_id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the Committee native seat has no persistent topology binding",
                    )
                })?;
            let display_name =
                self.seat_name(run.project_id, &node, &scope, &topology_seat.role.role_code)?;
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
                    credential: ConsultationCredential::new(seat_credential),
                    model_rung: seat.model_rung.clone(),
                    context_policy: context_policy.clone(),
                    requested_at: kontor_api::now(),
                })
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
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

    /// Re-engage the same native Committee seats for the bounded second round.
    async fn dispatch_committee_round_two(
        &self,
        run: &StoredConsultationRun,
        template: &CommitteeTemplateSpec,
        judge_only: bool,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let runtime_kind = self.node_runtime_kind()?;
        let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime selected for Committee re-review is not configured",
            )
        })?;
        let remediation = state
            .with_store(|store| {
                store.get_committee_remediation(
                    run.project_id,
                    match run.id {
                        ConsultationRunId::Committee(id) => id,
                        ConsultationRunId::Advisor(_) => unreachable!(),
                    },
                )
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "round two has no durable remediation brief",
                )
            })?;
        let findings = if judge_only {
            state
                .with_store(|store| {
                    store.list_committee_findings(
                        run.project_id,
                        match run.id {
                            ConsultationRunId::Committee(id) => id,
                            ConsultationRunId::Advisor(_) => unreachable!(),
                        },
                        run.round,
                    )
                })
                .map_err(|error| self.refuse(&error))?
        } else {
            Vec::new()
        };
        let seats = state
            .with_store(|store| store.list_consultation_seats(run.project_id, run.id))
            .map_err(|error| self.refuse(&error))?;
        for seat in seats {
            let role = seat.committee_role.ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "a Committee seat has no frozen role",
                )
            })?;
            if (judge_only && role != CommitteeRole::Judge)
                || (!judge_only && role != CommitteeRole::Reviewer)
            {
                continue;
            }
            let identity = seat.native_identity.clone().ok_or_else(|| {
                self.deny(
                    ApiErrorCode::StaleBinding,
                    "a Committee re-review seat has no attested native session",
                )
            })?;
            let slot = template
                .slots
                .iter()
                .find(|slot| slot.id == seat.role_slot_id)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "a Committee re-review seat is absent from its pinned template",
                    )
                })?;
            let body = BoundedText::parse(&format!(
                "Read-only Committee round {} re-review. Do not mutate code, Jira, topology, scheduling, or runtime state. Role instructions: {} Question: {} Durable remediation brief: {} Durable reviewer findings available to this seat: {} Submit this seat's finding using the existing KONTOR_AUTH environment value.",
                run.round,
                slot.behavior.as_str(),
                run.question.as_str(),
                remediation,
                serde_json::to_string(&findings.iter().map(|finding| &finding.document).collect::<Vec<_>>())
                    .map_err(|_| self.deny(ApiErrorCode::Unavailable, "round-two findings could not be encoded"))?,
            ))
            .map_err(|error| self.refuse_domain(&error))?;
            adapter
                .message_consultation(&ConsultationMessageRequest {
                    run_id: run.id,
                    seat_binding_id: seat.seat_binding_id,
                    identity,
                    message_id: MessageId::generate(),
                    body,
                    sent_at: kontor_api::now(),
                })
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        }
        Ok(())
    }

    /// Stable wire projection of one Committee run and its durable evidence.
    fn committee_run_dto(
        &self,
        run: &StoredConsultationRun,
        receipt_id: Option<CommandReceiptId>,
        applied: AppliedDto,
    ) -> Result<CommitteeRunDto, ApiError> {
        let state = self.state()?;
        let (revision, _) = self.committee_template(run)?;
        let seats = state
            .with_store(|store| store.list_consultation_seats(run.project_id, run.id))
            .map_err(|error| self.refuse(&error))?;
        let committee_run_id = match run.id {
            ConsultationRunId::Committee(id) => id,
            ConsultationRunId::Advisor(_) => {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "this projection requires a Committee run",
                ));
            }
        };
        let findings = state
            .with_store(|store| {
                store.list_committee_findings(run.project_id, committee_run_id, run.round)
            })
            .map_err(|error| self.refuse(&error))?;
        let remediation = state
            .with_store(|store| store.get_committee_remediation(run.project_id, committee_run_id))
            .map_err(|error| self.refuse(&error))?;
        let outcome = run
            .result
            .as_ref()
            .and_then(|value| value.get("verdict"))
            .and_then(serde_json::Value::as_str)
            .map(|value| match ConsultationVerdict::parse(value) {
                Ok(ConsultationVerdict::Compliant) => Ok(ConsultationVerdictDto::Compliant),
                Ok(ConsultationVerdict::NonCompliant) => Ok(ConsultationVerdictDto::NonCompliant),
                Err(error) => Err(self.refuse_domain(&error)),
            })
            .transpose()?;
        let receipt = receipt_id
            .map(|receipt_id| {
                Ok(MutationReceiptDto {
                    realm_id: state.realm_id(),
                    receipt_id: receipt_id.to_string(),
                    applied,
                    revision: run.revision,
                    snapshot_cursor: self.cursor()?,
                })
            })
            .transpose()?;
        Ok(CommitteeRunDto {
            realm_id: state.realm_id(),
            committee_run_id,
            epic_id: run.mini_project_id,
            template: consultation_revision_dto(&revision),
            topology_node_id: run.topology_node_id,
            seats: seats
                .into_iter()
                .map(|seat| ConsultationSeatDto {
                    role_slot_id: seat.role_slot_id.as_str().to_owned(),
                    logical_role: seat.logical_role.as_str().to_owned(),
                    seat_binding_id: seat.seat_binding_id,
                    observed_binding: seat.native_identity.map(|identity| ObservedBindingDto {
                        runtime_kind: identity.runtime_kind,
                        native_id: identity.native_id,
                        native_name: None,
                        cwd: None,
                        observed_at: seat.observed_at.unwrap_or(run.updated_at),
                    }),
                })
                .collect(),
            state: run.state.as_str().to_owned(),
            findings_recorded: u32::try_from(findings.len()).unwrap_or(u32::MAX),
            round: run.round,
            outcome,
            findings: findings
                .iter()
                .map(|finding| CommitteeFindingDto {
                    round: finding.round,
                    role_slot_id: finding.role_slot_id.as_str().to_owned(),
                    role: finding.role.as_str().to_owned(),
                    verdict: match finding.verdict {
                        ConsultationVerdict::Compliant => ConsultationVerdictDto::Compliant,
                        ConsultationVerdict::NonCompliant => ConsultationVerdictDto::NonCompliant,
                    },
                    evidence_complete: finding.evidence_complete,
                    rationale: finding
                        .document
                        .get("rationale")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    evidence_refs: finding
                        .document
                        .get("evidence_refs")
                        .and_then(serde_json::Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .collect(),
                    document_hash: finding.document_hash.clone(),
                })
                .collect(),
            remediation,
            result: run.result.clone(),
            receipt,
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
        budget: BudgetBoundsDto {
            max_tokens: stored.authorization.budget.max_tokens,
            max_commands: stored.authorization.budget.max_commands,
            max_duration_seconds: stored.authorization.budget.max_duration_seconds,
            max_cost_minor_units: stored.authorization.budget.max_cost.minor_units,
            cost_currency: stored
                .authorization
                .budget
                .max_cost
                .currency
                .as_str()
                .to_owned(),
        },
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
        LifecycleAction::WithdrawTask => "withdraw_task",
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
    /// Durable epic/task identity used for placement, labels and titles.
    scope: &'a ExecutionScope,
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
    let instruction = if roots.contains(slot) {
        "begin the admitted task."
    } else {
        "wait: this seat is downstream of a handoff. Do no work until you are \
         handed the artifacts your role requires."
    };
    kontor_core::id::BoundedText::parse(&format!("{instruction}{OPEN_QUESTION_DUTY}"))
}

/// The open-question duty every ordinary team seat is launched with (OP-REQ-038).
///
/// A duty, not a mechanism: this adds no scanner, no capability, no role and no
/// standing run. The seat that trips over an ambiguity is the only one that
/// knows it did, so an instruction is the only surface that can catch it — a
/// service scanning for ambiguity would be looking for something that only
/// exists in somebody's reasoning.
///
/// It is appended to *both* branches. A downstream seat is told to wait and is
/// still given the duty, because the ambiguity it has to record is frequently in
/// what it was just handed, and a seat that waits silently over a contradiction
/// is the exact failure this requirement exists to stop. The wait instruction
/// itself is unchanged.
const OPEN_QUESTION_DUTY: &str = " If you must proceed on an assumption you cannot \
     evidence, record an open question before you do — its subject, the record or \
     document it attaches to, why the state is ambiguous and the options you saw. \
     An unresolved ambiguity belongs in the ledger, not in this transcript.";

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

/// Freeze how much one seat may do before it has to ask a human.
///
/// Read from the team run's *own frozen* template for the same reason the
/// context policy and the model rung are: a later edit to a template must not
/// change the authority a running seat was launched under. A seat that declares
/// nothing is [`SeatAutonomy::standard`] — supervised — so a template written
/// before this policy existed keeps behaving exactly as it did.
fn freeze_seat_autonomy(
    snapshot: &TeamRunSnapshot,
    slot: &RoleSlotId,
) -> kontor_core::DomainResult<SeatAutonomy> {
    Ok(
        kontor_teams::spec::TeamTemplateSpec::from_snapshot(snapshot)?
            .slot(slot)
            .and_then(|seat| seat.autonomy)
            .unwrap_or_else(SeatAutonomy::standard),
    )
}

/// Select the primary model rung from the team run's immutable template.
/// The recorded quota states, asked one provider at a time during a rung walk.
///
/// This is the durable half of provider availability. The adapter's own
/// `provider_available` reads `unavailable_providers`, a settings field resolved
/// when the adapter is composed: excluding a provider that way needs a daemon
/// restart, applies to every account at once, and never stops being true. A
/// recorded state is per account, survives a restart, and — for an allowance
/// that returns at a known instant — stops blocking on its own.
///
/// Both are consulted, and either can hold a rung back. An operator who
/// excluded a provider in settings does not want a stored row overriding them,
/// and a provider observed out of quota is out whatever the settings say.
struct QuotaOutlook<'a> {
    states: &'a [kontor_core::repository::ProviderQuotaState],
    /// The account the run is pinned to, when it is pinned to one.
    ///
    /// A pin is the run's, not the resolver's. When it is present the walk
    /// considers that account alone, because `admit_pinned_launch` refuses a
    /// launch naming any other one — resolving to a second account here would
    /// only produce a placement dispatch then throws away.
    account: Option<AccountProfileId>,
    /// Every account a launch may still be resolved across, with the provider
    /// aliases each one is addressable under. Empty for a pinned run.
    accounts: &'a [kontor_scheduler::headroom::EligibleAccount],
    /// The declared headroom policy, or the state-only fallback when a realm has
    /// declared none.
    headroom: HeadroomConfig,
    now: Timestamp,
}

impl QuotaOutlook<'_> {
    /// The accounts this launch may be placed on.
    ///
    /// A pinned run yields exactly its pin. The pin carries no provider aliases
    /// of its own here, so every rung's provider is considered for it: the
    /// governed alias set narrows *which account* a launch may move to, and a
    /// run that is already pinned is not moving.
    fn candidates(&self, rungs: &[ModelRung]) -> Vec<kontor_scheduler::headroom::EligibleAccount> {
        match self.account {
            Some(account_profile_id) => vec![kontor_scheduler::headroom::EligibleAccount {
                account_profile_id,
                selectable_providers: rungs.iter().map(|rung| rung.provider.0.clone()).collect(),
            }],
            None => self.accounts.to_vec(),
        }
    }
}

fn freeze_seat_model_rung(
    adapter: &dyn RuntimeAdapter,
    snapshot: &TeamRunSnapshot,
    slot: &RoleSlotId,
    quota: &QuotaOutlook<'_>,
) -> kontor_core::DomainResult<ModelRung> {
    let template = kontor_teams::spec::TeamTemplateSpec::from_snapshot(snapshot)?;
    let chain = template
        .slot(slot)
        .and_then(|seat| seat.model_chain.as_ref())
        .ok_or_else(|| {
            kontor_core::DomainError::invalid("TeamRunSnapshot", "the role slot has no model route")
        })?;
    // Account before rung. The walk exhausts every account eligible for the
    // current rung before taking the next one, because a second account on the
    // same rung costs nothing while descending costs quality on every turn that
    // follows. This is also what makes rungs three and four reachable: the
    // previous selection took the first clear rung and otherwise fell back to
    // the frozen primary, so a four-rung chain had two useful entries.
    let candidates = quota.candidates(&chain.rungs);
    let placement = kontor_scheduler::headroom::resolve(
        &chain.rungs,
        &candidates,
        quota.states,
        &quota.headroom,
        // These are delivery seats by construction: each of this function's
        // callers launches task work under a TeamRun. An epic's control seats
        // live on its ECP and are created without a task or a TeamRun, so they
        // do not pass through here. The reserve is what holds headroom open for
        // them, and subtracting it from delivery is how that is done.
        kontor_scheduler::headroom::SeatClass::Delivery,
        quota.now,
        |provider| adapter.provider_available(provider),
    )?;
    Ok(match placement {
        kontor_scheduler::headroom::Placement::Admit { rung, .. } => rung,
        // Nothing is admissible. Preserve master's refusal shape rather than
        // inventing a route: an adapter-declared fallback if one is clear, and
        // otherwise the frozen primary, so the adapter emits its own typed
        // provider-outage refusal. Deciding here to descend anyway is exactly
        // what a near reset must not cause, and substituting a model the
        // template never declared would weaken the template.
        //
        // ponytail: the wait instant and the escalation payload are computed and
        // then dropped on this path, because this function's contract is to
        // return a rung. Parking the work on them needs a launch path that can
        // hold a seat instead of routing it — see the handoff's open risks.
        kontor_scheduler::headroom::Placement::Wait { .. }
        | kontor_scheduler::headroom::Placement::NeedsHuman { .. } => chain
            .rungs
            .iter()
            .find_map(|rung| adapter.fallback_model_rung(rung))
            .or_else(|| chain.rungs.first().cloned())
            .expect("a validated model chain is non-empty"),
    })
}

fn parse_runtime_model_route(
    route: &RuntimeModelRouteRequest,
) -> kontor_core::DomainResult<ModelRung> {
    if route.provider.trim().is_empty() || route.model.trim().is_empty() {
        return Err(kontor_core::DomainError::invalid(
            "RuntimeModelRouteRequest",
            "provider and model are required",
        ));
    }
    let effort = route
        .effort
        .as_deref()
        .map(|effort| match effort {
            "off" => Ok(EffortLevel::Off),
            "low" => Ok(EffortLevel::Low),
            "medium" => Ok(EffortLevel::Medium),
            "high" => Ok(EffortLevel::High),
            "xhigh" => Ok(EffortLevel::Xhigh),
            "max" => Ok(EffortLevel::Max),
            "ultra" => Ok(EffortLevel::Ultra),
            "ultracode" => Ok(EffortLevel::Ultracode),
            _ => Err(kontor_core::DomainError::invalid(
                "RuntimeModelRouteRequest",
                "effort is not in the runtime effort vocabulary",
            )),
        })
        .transpose()?;
    Ok(ModelRung {
        provider: ProviderRef(route.provider.clone()),
        model: ModelRef(route.model.clone()),
        effort,
    })
}

fn runtime_model_route_dto(rung: &ModelRung) -> RuntimeModelRouteRequest {
    RuntimeModelRouteRequest {
        provider: rung.provider.0.clone(),
        model: rung.model.0.clone(),
        effort: rung.effort.map(|effort| effort.as_str().to_owned()),
    }
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
    /// Exact already-frozen node for consultation scopes.
    node_id: Option<TopologyNodeId>,
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
        headroom: config.headroom.map(|headroom| HeadroomCeilingsDto {
            session_percent: headroom.thresholds.session_percent,
            daily_percent: headroom.thresholds.daily_percent,
            weekly_percent: headroom.thresholds.weekly_percent,
            monthly_percent: headroom.thresholds.monthly_percent,
            control_plane_reserve_percent: headroom.control_plane_reserve_percent,
            short_horizon_seconds: headroom.short_horizon_seconds,
            escalation_horizon_seconds: headroom.escalation_horizon_seconds,
        }),
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
        headroom: ceilings.headroom.map(|headroom| HeadroomConfig {
            thresholds: kontor_core::quota::HeadroomThresholds {
                session_percent: headroom.session_percent,
                daily_percent: headroom.daily_percent,
                weekly_percent: headroom.weekly_percent,
                monthly_percent: headroom.monthly_percent,
            },
            control_plane_reserve_percent: headroom.control_plane_reserve_percent,
            short_horizon_seconds: headroom.short_horizon_seconds,
            escalation_horizon_seconds: headroom.escalation_horizon_seconds,
        }),
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
/// One consultation family's catalog revision.
///
/// How many revisions the project has published into that family, plus one.
/// Derived from the rows for the same reason `core_team_revision_of` is: a
/// stored counter would need its own transaction to stay true, and the first
/// partial write would leave the catalog claiming a revision it cannot show.
fn consultation_catalog_revision(published: usize) -> AggregateRevision {
    AggregateRevision::parse(
        u64::try_from(published)
            .unwrap_or(u64::MAX)
            .saturating_add(1),
    )
    .unwrap_or(AggregateRevision::INITIAL)
}

fn consultation_revision_dto(stored: &StoredConsultationProfileRevision) -> ProfileRevisionDto {
    ProfileRevisionDto {
        id: stored.profile_id.clone(),
        version: stored.version,
        name: stored.name.clone(),
        definition_hash: stored.definition_hash.clone(),
    }
}

/// Normalize a human Committee name and a profile reference to one catalog key.
/// The built-in Completion profile says `independent_review@1`, while the
/// published template's display name is `Independent review`; neither spelling
/// is an identity on its own, but the normalized name plus pinned version is.
fn normalize_committee_reference(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '@' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// One candidate definition, parsed into the type its route selected.
enum ConsultationDefinition {
    /// An Advisor profile.
    Advisor(Box<AdvisorProfileSpec>),
    /// A Committee template.
    Committee(Box<CommitteeTemplateSpec>),
}

impl ConsultationDefinition {
    /// The stable logical id every revision of this document shares.
    fn profile_id(&self) -> String {
        match self {
            Self::Advisor(spec) => spec.profile_id.to_string(),
            Self::Committee(spec) => spec.template_id.to_string(),
        }
    }

    fn version(&self) -> SpecVersion {
        match self {
            Self::Advisor(spec) => spec.version,
            Self::Committee(spec) => spec.version,
        }
    }

    fn name(&self) -> ExternalName {
        match self {
            Self::Advisor(spec) => spec.name.clone(),
            Self::Committee(spec) => spec.name.clone(),
        }
    }

    /// The canonical typed value, which is what gets stored and hashed.
    fn canonicalize(&self) -> Result<CanonicalDocument, kontor_core::DomainError> {
        match self {
            Self::Advisor(spec) => spec.canonicalize(),
            Self::Committee(spec) => spec.canonicalize(),
        }
    }
}

/// Parse and validate one candidate definition, or say why it cannot be
/// published.
///
/// The family comes from the route, never from the document, so a caller cannot
/// publish an Advisor profile into the Committee catalog by labelling it one.
/// Unknown fields are rejected by the specifications themselves, which means a
/// typo in a policy document fails here rather than being silently dropped and
/// published as a permission nobody wrote.
///
/// The violation describes the caller's own submission back to it. It reaches
/// the caller only through a preview's typed `violations`, never through a
/// refusal message: a preview exists to tell an Admin what to fix, and an
/// `ApiError` carries a static string precisely so document text cannot ride
/// out in one.
fn consultation_definition(
    family: ConsultationFamily,
    definition: &serde_json::Value,
) -> Result<ConsultationDefinition, String> {
    match family {
        ConsultationFamily::Advisor => {
            let spec: AdvisorProfileSpec = serde_json::from_value(definition.clone())
                .map_err(|error| format!("not a valid Advisor profile: {error}"))?;
            spec.validate().map_err(|error| error.to_string())?;
            Ok(ConsultationDefinition::Advisor(Box::new(spec)))
        }
        ConsultationFamily::Committee => {
            let spec: CommitteeTemplateSpec = serde_json::from_value(definition.clone())
                .map_err(|error| format!("not a valid Committee template: {error}"))?;
            spec.validate().map_err(|error| error.to_string())?;
            Ok(ConsultationDefinition::Committee(Box::new(spec)))
        }
    }
}

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
        Applied::Updated => AppliedDto::Updated,
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

/// One quota state as a projection reports it.
///
/// `blocking` is computed at read time rather than stored: an exhausted
/// allowance stops holding work back the moment its instant passes, and a
/// projection that reported a stale `true` would have an operator hunting for a
/// block that had already lifted.
fn provider_quota_state_dto(
    entry: &kontor_core::repository::ProviderQuotaState,
    now: Timestamp,
) -> ProviderQuotaStateDto {
    ProviderQuotaStateDto {
        account_profile_id: entry.account_profile_id,
        provider: entry.provider.clone(),
        state: entry.state.as_str().to_owned(),
        resets_at: entry.resets_at.map(|instant| instant.to_string()),
        source: entry.source.as_str().to_owned(),
        observed_at: entry.observed_at.to_string(),
        blocking: entry.blocks_at(now),
        windows: entry.windows().iter().map(quota_window_dto).collect(),
        credit: entry.credit.map(credit_balance_dto),
        revision: entry.revision,
    }
}

fn quota_window_dto(window: &kontor_core::quota::QuotaWindow) -> QuotaWindowDto {
    QuotaWindowDto {
        kind: window.kind.as_str().to_owned(),
        resets_at: window.resets_at.to_string(),
        used_percent: window.used_percent,
    }
}

/// One currency on the wire, because the balance and its floor share one in the
/// schema. A projection that offered two would be inviting the comparison the
/// headroom predicate refuses.
fn credit_balance_dto(credit: kontor_core::quota::CreditBalance) -> CreditBalanceDto {
    CreditBalanceDto {
        remaining_minor_units: credit.remaining.minor_units,
        reserve_minor_units: credit.reserve.minor_units,
        currency: credit.remaining.currency.as_str().to_owned(),
    }
}

/// Read one window off the wire.
fn quota_window_of(
    dto: &QuotaWindowDto,
) -> kontor_core::DomainResult<kontor_core::quota::QuotaWindow> {
    Ok(kontor_core::quota::QuotaWindow {
        kind: kontor_core::quota::QuotaWindowKind::parse(&dto.kind)?,
        resets_at: kontor_core::id::parse_utc_timestamp(&dto.resets_at)?,
        used_percent: dto.used_percent,
    })
}

/// Read a balance off the wire, giving both amounts the one stated currency.
fn credit_balance_of(
    dto: &CreditBalanceDto,
) -> kontor_core::DomainResult<kontor_core::quota::CreditBalance> {
    let currency = kontor_core::id::CurrencyCode::parse(&dto.currency)?;
    Ok(kontor_core::quota::CreditBalance {
        remaining: Money {
            minor_units: dto.remaining_minor_units,
            currency,
        },
        reserve: Money {
            minor_units: dto.reserve_minor_units,
            currency,
        },
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
    /// keys. Ordinary lifecycle values are deliberately not consulted — `done`
    /// is a state a task can reach, not evidence that the things it promised
    /// exist. `withdrawn` is the one exception because it explicitly removes
    /// never-started work from the epic's completion contract.
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
        for task in tasks.into_iter().filter(|task| {
            task.mini_project_id == Some(epic_id) && counts_towards_completion(task.state)
        }) {
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
    /// derived. Committee outcomes come exclusively from settled durable runs;
    /// a native session finishing or a caller claiming a verdict is never enough.
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
            CompletionPhase::Verdict(round) => {
                let runs = self
                    .state()?
                    .with_store(|store| {
                        store.list_consultation_runs(
                            stored.project_id,
                            stored.mini_project_id,
                            ConsultationFamily::Committee,
                        )
                    })
                    .map_err(|error| self.refuse(&error))?;
                let compiled = self.pinned_completion(stored)?;
                let expected_template =
                    normalize_committee_reference(compiled.profile.verdict_committee.as_str());
                let settled = runs
                    .into_iter()
                    .filter(|run| {
                        run.state == ConsultationRunState::Settled
                            && u8::try_from(run.round).ok() == Some(round)
                    })
                    .find(|run| {
                        self.committee_template(run).is_ok_and(|(revision, _)| {
                            normalize_committee_reference(&format!(
                                "{}@{}",
                                revision.name.as_str(),
                                revision.version.get()
                            )) == expected_template
                        })
                    })
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::Unavailable,
                            "no settled Committee run matches this completion round",
                        )
                    })?;
                let result = settled.result.as_ref().ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the settled Committee run has no immutable result",
                    )
                })?;
                let verdict = match result.get("verdict").and_then(serde_json::Value::as_str) {
                    Some("compliant") => CommitteeVerdict::Pass,
                    Some("non_compliant") => CommitteeVerdict::Fail,
                    _ => {
                        return Err(self.deny(
                            ApiErrorCode::Unavailable,
                            "the settled Committee result has no recognized verdict",
                        ));
                    }
                };
                let evidence = result
                    .get("evidence_hash")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::Unavailable,
                            "the settled Committee result has no evidence digest",
                        )
                    })
                    .and_then(|value| {
                        ContentHash::parse(value).map_err(|error| self.refuse_domain(&error))
                    })?;
                let committee_run_id = match settled.id {
                    ConsultationRunId::Committee(id) => id,
                    ConsultationRunId::Advisor(_) => unreachable!(),
                };
                let findings = self
                    .state()?
                    .with_store(|store| {
                        store.list_committee_findings(
                            stored.project_id,
                            committee_run_id,
                            settled.round,
                        )
                    })
                    .map_err(|error| self.refuse(&error))?;
                let consultation = ExternalName::parse(&committee_run_id.to_string())
                    .map_err(|error| self.refuse_domain(&error))?;
                let deliberation = findings
                    .into_iter()
                    .map(|finding| {
                        Ok(DeliberationStep {
                            role: ExternalName::parse(finding.role_slot_id.as_str())
                                .map_err(|error| self.refuse_domain(&error))?,
                            consultation: consultation.clone(),
                            round,
                            outcome: ExternalName::parse(finding.verdict.as_str())
                                .map_err(|error| self.refuse_domain(&error))?,
                        })
                    })
                    .collect::<Result<Vec<_>, ApiError>>()?;
                Ok(CompletionObservation::VerdictRecorded {
                    round,
                    verdict,
                    evidence,
                    deliberation,
                })
            }
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

/// The static rule sentence for one typed reconciliation conflict.
///
/// Reconciliation has always produced a *typed* conflict; this surface used to
/// collapse every one of them into a single sentence about "the pinned
/// external-workflow policy". That told an operator that a policy refused without
/// saying which, and the live ASMA-7877 run is what it costs: a ticket that could
/// not move from `DRAFT` to `In Development` reported the same prose as an
/// ownership dispute or a stale read.
///
/// An error's `rule` is `&'static str` by contract — never a stored value — so the
/// kind is surfaced by choosing a sentence per kind rather than by formatting one.
/// The match is exhaustive over a closed enum, so a new conflict kind cannot be
/// added without deciding what it tells a caller.
const fn jira_conflict_rule(kind: StatusConflictKind) -> &'static str {
    match kind {
        StatusConflictKind::StaleObservation => {
            "the newest Jira observation is too old to act on; observe again"
        }
        StatusConflictKind::NoLiveTransition => {
            "this Jira workflow offers no route Kontor may take to the target status"
        }
        StatusConflictKind::MultipleLiveTransitions => {
            "several Jira transitions reach the target status; the pinned specification does not \
             say which"
        }
        StatusConflictKind::IncompatibleHumanMove => {
            "the ticket was moved to a status the pinned specification cannot start from"
        }
        StatusConflictKind::ExternalTerminalBeforeInternalEvidence => {
            "the ticket is closed in Jira while Kontor holds no closure evidence"
        }
        StatusConflictKind::UnknownStatusClass => {
            "the observed Jira status is not declared by the pinned specification"
        }
        StatusConflictKind::UnknownTransitionPath => {
            "the target status is not declared by the pinned specification"
        }
        StatusConflictKind::OwnershipUnresolved => {
            "Kontor should hold this ticket but no Jira assignee could be resolved"
        }
        StatusConflictKind::OwnershipMismatch => "somebody else holds this Jira ticket",
        StatusConflictKind::TerminalOwnershipViolation => {
            "a closed ticket's owner changed while the pinned policy preserves it"
        }
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
        CompletionBlocker::OpenQuestion(OpenQuestionBlocker::Undispositioned {
            question_id,
            subject,
        }) => CompletionBlockerDto::OpenQuestionUndispositioned {
            question_id: *question_id,
            subject: subject.clone(),
        },
        CompletionBlocker::OpenQuestion(OpenQuestionBlocker::Reopened {
            question_id,
            subject,
        }) => CompletionBlockerDto::OpenQuestionReopened {
            question_id: *question_id,
            subject: subject.clone(),
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
    fn complete_local_command(&self, key: &IdempotencyKey) -> Result<(), ApiError> {
        let state = self.state()?;
        let completed = state
            .with_store(|store| store.complete_local_command(key, kontor_api::now()))
            .map_err(|error| self.refuse(&error))?;
        if completed.is_some() {
            state.signals().appended();
        }
        Ok(())
    }

    fn persist_session_observation(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        observation: &ControlPlaneObservation,
        reduced_at: Timestamp,
    ) -> Result<(), ApiError> {
        self.persist_run_observation(project_id, agent_run_id, observation, reduced_at)
            .map(|_| ())
    }

    fn projects(&self) -> Result<Vec<kontor_api::applications::ProjectReadDto>, ApiError> {
        let state = self.state()?;
        Ok(state
            .with_store(SqliteStore::list_projects)
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .map(|project| kontor_api::applications::ProjectReadDto {
                realm_id: state.realm_id(),
                project_id: project.project_id,
                name: project.name,
                root_path: project.root_path,
                revision: project.revision,
                created_at: project.created_at,
            })
            .collect())
    }

    fn project(
        &self,
        project_id: ProjectId,
    ) -> Result<kontor_api::applications::ProjectReadDto, ApiError> {
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
        Ok(kontor_api::applications::ProjectReadDto {
            realm_id: state.realm_id(),
            project_id: project.id,
            name: project.name,
            root_path: project.root_path,
            revision: project.revision,
            created_at: project.created_at,
        })
    }

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

    fn provider_quota_states(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<ProviderQuotaStateDto>, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let states = state
            .with_store(|store| store.list_provider_quota_states(project_id))
            .map_err(|error| self.refuse(&error))?;
        Ok(states
            .iter()
            .map(|entry| provider_quota_state_dto(entry, now))
            .collect())
    }

    async fn record_provider_quota(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &RecordProviderQuotaRequest,
    ) -> Result<ProviderQuotaStateDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let project = self.project_row(project_id)?;
        let account_profile_id = AccountProfileId::parse(&request.account_profile_id)
            .map_err(|error| self.refuse_domain(&error))?;
        let quota_state = kontor_core::spec::ProviderQuotaKind::parse(&request.state)
            .map_err(|error| self.refuse_domain(&error))?;
        let resets_at = request
            .resets_at
            .as_deref()
            .map(kontor_core::id::parse_utc_timestamp)
            .transpose()
            .map_err(|error| self.refuse_domain(&error))?;
        let windows = request
            .windows
            .iter()
            .map(quota_window_of)
            .collect::<kontor_core::DomainResult<Vec<_>>>()
            .map_err(|error| self.refuse_domain(&error))?;
        let credit = request
            .credit
            .as_ref()
            .map(credit_balance_of)
            .transpose()
            .map_err(|error| self.refuse_domain(&error))?;
        // The windows and the balance are in the intent, so a second call under
        // the same key reporting different headroom conflicts rather than
        // replaying the first reading.
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "record_provider_quota",
            "project": project_id.to_string(),
            "account": account_profile_id.to_string(),
            "provider": request.provider.as_str(),
            "state": quota_state.as_str(),
            "resets_at": resets_at.map(|instant| instant.to_string()),
            "windows": windows
                .iter()
                .map(|window: &kontor_core::quota::QuotaWindow| serde_json::json!({
                    "kind": window.kind.as_str(),
                    "resets_at": window.resets_at.to_string(),
                    "used_percent": window.used_percent,
                }))
                .collect::<Vec<_>>(),
            "credit": credit.map(|credit: kontor_core::quota::CreditBalance| serde_json::json!({
                "remaining_minor_units": credit.remaining.minor_units,
                "reserve_minor_units": credit.reserve.minor_units,
                "currency": credit.remaining.currency.as_str(),
            })),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();
        if !replayed {
            state
                .with_store(|store| {
                    store.set_provider_quota_state(&NewProviderQuotaState {
                        project_id,
                        account_profile_id,
                        provider: request.provider.clone(),
                        state: quota_state,
                        resets_at,
                        windows: windows.clone(),
                        credit,
                        // The operator's own assertion is the evidence, so the
                        // intent digest is what a record can honestly cite. A
                        // parsed runtime message will cite the frame instead —
                        // never the message text, which is vendor output.
                        evidence_hash: intent.hash().clone(),
                        source: kontor_core::spec::ProviderQuotaSource::Operator,
                        observed_at: now,
                        expected_revision: request.expected_revision,
                        updated_at: now,
                    })
                })
                .map_err(|error| self.refuse(&error))?;
        }
        self.record(
            key,
            project_id,
            CommandKind::OverrideAvailability,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;
        let stored = state
            .with_store(|store| store.list_provider_quota_states(project_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|entry| {
                entry.account_profile_id == account_profile_id && entry.provider == request.provider
            })
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the provider quota state could not be read back after the record",
                )
            })?;
        Ok(provider_quota_state_dto(&stored, now))
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
                    ApiErrorCode::EnsureMismatch,
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

    async fn amend_account_profile(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        account_profile_id: AccountProfileId,
        request: &AmendAccountProfileRequest,
    ) -> Result<AccountProfileDto, ApiError> {
        let state = self.state()?;
        let profile = state
            .with_store(|store| store.get_account_profile(project_id, account_profile_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such provider-account profile exists in this project",
                )
            })?;
        let label = request.label.clone().unwrap_or_else(|| profile.label.clone());
        let enabled = request.enabled.unwrap_or(profile.enabled);
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "provider_account_profile_amend",
            "project": project_id.to_string(),
            "account": account_profile_id.to_string(),
            "label": label.as_str(),
            "enabled": enabled,
            "expected_revision": request.expected_revision.get(),
        }))?;
        let target = AggregateRef::Project { project_id };
        if self.replayed(key, &intent, Some(&target))?.is_some() {
            return Ok(account_profile_dto(&profile, AppliedDto::Unchanged));
        }

        // An amend that changes nothing is still recorded, deliberately. The
        // caller asked for a state and got it, and a receipt that only exists
        // when a value happened to move would make "did anyone try this" an
        // unanswerable question.
        let amended = if label == profile.label && enabled == profile.enabled {
            profile.clone()
        } else {
            state
                .with_store(|store| {
                    store.update_account_profile(&AccountProfileUpdate {
                        project_id,
                        id: account_profile_id,
                        expected_revision: request.expected_revision,
                        label,
                        enabled,
                        updated_at: kontor_api::now(),
                    })
                })
                .map_err(|error| self.refuse(&error))?
        };
        self.record(
            key,
            project_id,
            CommandKind::EnsureAccountProfile,
            target,
            AggregateRevision::INITIAL,
            &intent,
        )?;
        Ok(account_profile_dto(&amended, AppliedDto::Updated))
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
        let epic_id = scope.epic_id().ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "this scope has no epic for a materialization receipt to name",
            )
        })?;
        let replayed = self
            .replayed(
                key,
                &intent,
                Some(&AggregateRef::MiniProject {
                    mini_project_id: epic_id,
                }),
            )?
            .is_some();

        // Idempotency suppresses a second native effect; it does not freeze a
        // repairable hole in Kontor's own logical topology. Older admissions
        // could persist a task node without its sibling ECP, so a replay must
        // re-ensure that durable chain before returning the unchanged receipt.
        // `ensure_task_node` is store-only and idempotent: the runtime remains
        // untouched and the already bound TSW keeps its native identity.
        if replayed && let Some(task_id) = scope.task_id {
            let leaf = self.ensure_task_node(project_id, task_id)?;
            self.retire_unrouted_task_persistent_seats(project_id, task_id, leaf.id)?;
        }

        if !replayed {
            // A ticket's semantic identity is a placement prerequisite, not a
            // value to improvise after the root and workspace have already
            // been contacted. Resolve it before ensuring the logical chain or
            // asking the runtime to prepare its plane, so a legacy task that
            // lacks an explicit short-code mapping fails with zero native
            // effects and can be repaired through `epics:preview/apply`.
            if let (Some(epic_id), Some(task_id)) = (scope.epic_id, scope.task_id) {
                let runtime_kind = self.node_runtime_kind()?;
                let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the node's configured runtime family is unavailable",
                    )
                })?;
                self.execution_scope(project_id, epic_id, Some(task_id), adapter.as_ref())?;
            }
            // Materializing is ensuring plus preparing the exact native
            // container and binding the seats the scope hosts. The chain comes
            // first because neither a native binding nor a seat can ever
            // belong to a node that does not exist.
            let leaf = match scope.task_id {
                // Ticket admission also needs the ECP node that owns delivery
                // seats. `ensure_task_node` creates that durable chain without
                // admitting a TeamRun or opening a delivery seat.
                Some(task_id) => self.ensure_task_node(project_id, task_id)?,
                None => self.ensure_scope_chain(project_id, &scope)?,
            };
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
            let projection = ContainerProjection::resolve(&declared.projection_capabilities)
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            if projection != ContainerProjection::LogicalOnly {
                let runtime_kind = self.node_runtime_kind()?;
                let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the node's configured runtime family is unavailable",
                    )
                })?;
                adapter
                    .prepare_plane()
                    .await
                    .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
                let cwd = match leaf.task_id {
                    Some(task_id) => self.task_root(project_id, task_id)?,
                    None => self.runtime_root(project_id, leaf.mini_project_id)?,
                };
                self.ensure_container(project_id, &leaf, &cwd, adapter.as_ref())
                    .await?;
            }
            // Capability-dispatched, exactly as OP-02 does it: only a kind the
            // specification declares as a session host may hold a seat. A kind
            // that is a native root hosts nothing, and opening a seat on one
            // would be Kontor placing work where the vocabulary says it cannot
            // go.
            // Ticket materialization binds the durable TSW container but does
            // not admit a TeamRun or pre-create a delivery seat. Scheduler
            // start owns delivery-seat creation. Structural session hosts keep
            // their control seat.
            if scope.task_id.is_none()
                && declared
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
            if let Some(task_id) = leaf.task_id {
                self.retire_unrouted_task_persistent_seats(project_id, task_id, leaf.id)?;
            }
        }

        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::StartScheduledWork,
            AggregateRef::MiniProject {
                mini_project_id: epic_id,
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

    async fn preview_container_retitle(
        &self,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        request: &ContainerRetitleRequest,
    ) -> Result<ContainerRetitlePreviewDto, ApiError> {
        let state = self.state()?;
        let (retitle, adapter) = self.retitle_request(project_id, topology_node_id, request)?;
        // A read all the way down: the adapter's preview reaches nothing that
        // writes, and this operation records no receipt for it.
        let outcome = adapter
            .preview_retitle_container(&retitle)
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        Ok(ContainerRetitlePreviewDto {
            realm_id: state.realm_id(),
            topology_node_id,
            bound_native_id: retitle.bound_native_id,
            desired_title: outcome.desired_title,
            observed_title: outcome.observed_title,
            would_change: outcome.changed,
            snapshot_cursor: self.cursor()?,
        })
    }

    async fn apply_container_retitle(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        topology_node_id: TopologyNodeId,
        request: &ContainerRetitleRequest,
    ) -> Result<AppliedContainerRetitleDto, ApiError> {
        let state = self.state()?;
        let (retitle, adapter) = self.retitle_request(project_id, topology_node_id, request)?;
        let project = self.project_at(project_id, request.expected_revision)?;

        // The intent names the node and the container, and not the title. What
        // the caller authorized is "make this container's title what the pinned
        // topology and the plane's scope say it is" — a title in the key would
        // make a second repair under a corrected specification look like a replay
        // of the first one.
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "container_retitle",
            "project": project_id.to_string(),
            "node": topology_node_id.to_string(),
            "native": retitle.bound_native_id.as_str(),
        }))?;
        let replayed = self
            .replayed(key, &intent, Some(&AggregateRef::Project { project_id }))?
            .is_some();
        // A replay still reads the container back, through the preview that
        // changes nothing. Answering `changed: false` from the ledger alone would
        // be reporting a title nobody looked at.
        let outcome = if replayed {
            adapter.preview_retitle_container(&retitle).await
        } else {
            adapter.retitle_container(&retitle).await
        }
        .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::RetitleContainer,
            AggregateRef::Project { project_id },
            project.revision,
            &intent,
        )?;

        Ok(AppliedContainerRetitleDto {
            topology_node_id,
            // Read back from the runtime rather than echoed from the request:
            // this is the id the container still has after being renamed.
            bound_native_id: ExternalId::parse(
                outcome.snapshot.binding.identity.native_id.as_str(),
            )
            .map_err(|error| self.refuse_domain(&error))?,
            observed_title: outcome.observed_title,
            changed: !replayed && outcome.changed,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: project.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    async fn preview_native_names(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &NativeNamesPreviewRequest,
    ) -> Result<NativeNamesPreviewDto, ApiError> {
        Ok(self
            .prepare_native_names(project_id, epic_id, request.expected_revision)
            .await?
            .preview)
    }

    async fn apply_native_names(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &NativeNamesApplyRequest,
    ) -> Result<AppliedNativeNamesDto, ApiError> {
        let state = self.state()?;
        let project = self.project_at(project_id, request.expected_revision)?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "reconcile_native_names",
            "project_id": project_id.to_string(),
            "epic_id": epic_id.to_string(),
            "preview_hash": request.preview_hash.as_str(),
        }))?;
        let replayed = self.replayed(key, &intent, Some(&target))?.is_some();
        let prepared = self
            .prepare_native_names(project_id, epic_id, request.expected_revision)
            .await?;

        if !replayed && prepared.preview.preview_hash != request.preview_hash {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "native names or identities changed since the caller's complete preview",
            ));
        }

        // Persist the exact authorization before the first external effect.
        // If an acknowledgement is lost after a rename commits, the same key
        // re-enters through `replayed`, re-preflights the entire epic and only
        // dispatches targets whose fresh readback is still stale.
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::ReconcileNativeNames,
            target,
            project.revision,
            &intent,
        )?;
        let mut changed = 0_u64;
        for action in prepared.actions {
            match action {
                NativeNameAction::Container { request, adapter } => {
                    let outcome = adapter
                        .retitle_container(&request)
                        .await
                        .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
                    if outcome.snapshot.binding.identity.native_id != request.bound_native_id
                        || outcome.observed_title != request.desired_title.as_str()
                    {
                        return Err(self.deny(
                            ApiErrorCode::StaleBinding,
                            "container retitle did not preserve identity and read back the requested name",
                        ));
                    }
                    changed += u64::from(outcome.changed);
                }
                NativeNameAction::Seat {
                    request,
                    adapter,
                    hosted_seat_binding_id,
                } => {
                    let outcome = adapter
                        .retitle_seat(&request)
                        .await
                        .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
                    if outcome.identity != request.identity
                        || outcome.provider_session_id != request.provider_session_id
                        || outcome.container_native_id != request.container_native_id
                        || outcome.observed_title != request.desired_title.as_str()
                    {
                        return Err(self.deny(
                            ApiErrorCode::StaleBinding,
                            "seat retitle did not preserve every native identity and host",
                        ));
                    }
                    if let Some(seat_binding_id) = hosted_seat_binding_id {
                        let mut hosted = state
                            .with_store(|store| {
                                store.get_hosted_topology_seat(project_id, seat_binding_id)
                            })
                            .map_err(|error| self.refuse(&error))?
                            .ok_or_else(|| {
                                self.deny(
                                    ApiErrorCode::StaleBinding,
                                    "the hosted seat disappeared during its native-name repair",
                                )
                            })?;
                        hosted.native_identity = outcome.identity.clone();
                        hosted.provider_session_id = outcome.provider_session_id.clone();
                        hosted.observed_at = kontor_api::now();
                        state
                            .with_store(|store| store.bind_hosted_topology_seat(&hosted))
                            .map_err(|error| self.refuse(&error))?;
                    }
                    changed += u64::from(outcome.changed);
                }
            }
        }
        // A replay and a first apply both answer from fresh runtime readback.
        // The new plan hash may differ because its observed titles are now the
        // desired titles; it is evidence of current state, not an echo of the
        // authorization hash.
        let readback = self
            .prepare_native_names(project_id, epic_id, request.expected_revision)
            .await?
            .preview;
        Ok(AppliedNativeNamesDto {
            readback,
            changed,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: project.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    async fn reconcile_session_labels(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        request: &SessionLabelsReconcileRequest,
    ) -> Result<SessionLabelsReconciledDto, ApiError> {
        let state = self.state()?;
        let run = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "no such agent run exists"))?;
        if run.revision != request.expected_revision {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the run moved since the caller read it",
            ));
        }
        if run.terminal.is_some() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "a terminal run's native labels are immutable evidence",
            ));
        }
        let binding = run.binding.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::StaleBinding,
                "the run has no immutable native binding to repair",
            )
        })?;
        if binding.identity.generation != request.binding_generation {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the immutable binding generation differs from the label repair",
            ));
        }
        let held = state.sessions().get(binding.id).ok_or_else(|| {
            self.deny(
                ApiErrorCode::StaleBinding,
                "this process holds no frozen capability snapshot for the run",
            )
        })?;
        let adapter = state
            .runtimes()
            .get(&binding.identity.runtime_kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the run's runtime is not configured in this daemon",
                )
            })?;
        let task_id = self.task_for_team_run(project_id, run.team_run_id)?;
        let task = self.task_row(project_id, task_id)?;
        let epic_id = task.mini_project_id.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the run's task is not scoped to an epic",
            )
        })?;
        let node = state
            .with_store(|store| store.get_task_topology_node(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the run's task has no ticket session workspace",
                )
            })?;
        let root = self.task_root(project_id, task_id)?;
        let container = self
            .ensure_container(project_id, &node, &root, adapter.as_ref())
            .await?;
        let scope = self.execution_scope(project_id, epic_id, Some(task_id), adapter.as_ref())?;
        let role_slot_id =
            RoleSlotId::parse(run.role.as_str()).map_err(|error| self.refuse_domain(&error))?;
        let team_snapshot = state
            .with_store(|store| store.get_team_run(project_id, run.team_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::StaleBinding,
                    "the run's frozen team snapshot is unavailable",
                )
            })?
            .snapshot;
        let desired_title =
            self.delivery_seat_name(project_id, task_id, &scope, &team_snapshot, &role_slot_id)?;
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "reconcile_session_labels",
            "agent_run_id": agent_run_id.to_string(),
            "runtime_binding_id": binding.id.to_string(),
            "native_id": binding.identity.native_id.as_str(),
            "binding_generation": binding.identity.generation,
        }))?;
        // This is the session half of the existing native-projection repair
        // family. Schema 44 deliberately has one durable command kind for
        // correcting runtime-owned display/correlation projection; the exact
        // AgentRun, binding and native id remain in the canonical intent while
        // its authority is witnessed against the owning project, just like a
        // container retitle.
        let project = state
            .with_store(|store| store.get_project(project_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| self.deny(ApiErrorCode::NotFound, "no such project exists"))?;
        let target = AggregateRef::Project { project_id };
        let replayed = self.replayed(key, &intent, Some(&target))?.is_some();
        let outcome = adapter
            .reconcile_session_labels(&ReconcileSessionLabelsRequest {
                binding: held,
                scope,
                team_run_id: run.team_run_id,
                role_slot_id,
                desired_title,
                container,
                requested_at: kontor_api::now(),
            })
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        if outcome.identity != binding.identity {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the label repair read back another native session identity",
            ));
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::RetitleContainer,
            target,
            project.revision,
            &intent,
        )?;
        Ok(SessionLabelsReconciledDto {
            agent_run_id: agent_run_id.to_string(),
            native_id: outcome.identity.native_id.as_str().to_owned(),
            title: outcome.title,
            labels: outcome.labels,
            changed: !replayed && outcome.changed,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Created
                },
                revision: run.revision,
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
        let routes: BTreeMap<String, ModelRung> = request
            .routes
            .iter()
            .map(|route: &CoreTeamSeatRouteRequest| {
                Ok((
                    route.role_code.clone(),
                    parse_runtime_model_route(&route.model_route)?,
                ))
            })
            .collect::<kontor_core::DomainResult<_>>()
            .map_err(|error| self.refuse_domain(&error))?;
        if routes.len() != request.routes.len() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "a Core Team role may be routed only once",
            ));
        }
        for role_code in routes.keys() {
            if !roster.revision.seats.iter().any(|seat| {
                seat.presence != EpicPresence::OnDemand && seat.role.role_code.as_str() == role_code
            }) {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "a native Core Team route names no materialized role in the frozen roster",
                ));
            }
        }
        let mut intent_document = serde_json::json!({
            "schema_version": 1,
            "operation": "core_team_materialize",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
        });
        // Preserve byte-for-byte replay compatibility for the historical
        // logical-only request. Native routing is an explicit new intent.
        if !request.routes.is_empty() {
            intent_document["routes"] = serde_json::to_value(&request.routes).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "Core Team routes could not be encoded",
                )
            })?;
        }
        let intent = self.intent(&intent_document)?;
        let replayed = self
            .replayed(
                key,
                &intent,
                Some(&AggregateRef::MiniProject {
                    mini_project_id: epic_id,
                }),
            )?
            .is_some();
        let control = self.ensure_scope_chain(
            project_id,
            &self.resolve_scope(
                project_id,
                &SemanticTopologyTargetDto::EpicControl { epic_id },
            )?,
        )?;
        // Missing seats only. Every seat already there keeps its identity,
        // because a seat binding is what a running agent is attached to. This
        // also runs on replay so an old receipt whose process died between the
        // logical and native halves can converge without creating topology.
        let materialized =
            self.materialize_roster_seats(project_id, &control, &roster, kontor_api::now())?;
        if !routes.is_empty() {
            let runtime_kind = self.node_runtime_kind()?;
            let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the runtime selected for Core Team placement is not configured",
                )
            })?;
            let cwd = self.runtime_root(project_id, Some(epic_id))?;
            let container = self
                .ensure_container(project_id, &control, &cwd, adapter.as_ref())
                .await?;
            let scope = self.execution_scope(project_id, epic_id, None, adapter.as_ref())?;
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
            for (seat, seat_binding_id) in materialized {
                let Some(model_rung) = routes.get(seat.role.role_code.as_str()) else {
                    continue;
                };
                if !adapter.provider_available(model_rung.provider.0.as_str()) {
                    return Err(ApiError::from_runtime(
                        state.realm_id(),
                        &RuntimeError::ProviderUnavailable {
                            provider: model_rung.provider.0.clone(),
                        },
                    ));
                }
                let display_name =
                    self.seat_name(project_id, &control, &scope, &seat.role.role_code)?;
                let prompt = BoundedText::parse(&format!(
                    "Persistent {} seat for epic {}. Continue only work authorized through Kontor. Await or act on the bounded handoff supplied with this launch, then remain reusable under SeatBinding {}.",
                    seat.role.role_code.as_str(),
                    scope.epic.external_epic_key.as_str(),
                    seat_binding_id,
                ))
                .map_err(|error| self.refuse_domain(&error))?;
                let outcome = adapter
                    .launch_hosted_seat(&HostedSeatLaunchRequest {
                        seat_binding_id,
                        role_slot_id: seat.role_slot_id.clone(),
                        display_name,
                        container: container.clone(),
                        cwd: cwd.clone(),
                        scope: scope.clone(),
                        prompt,
                        model_rung: model_rung.clone(),
                        context_policy: context_policy.clone(),
                        requested_at: kontor_api::now(),
                    })
                    .await
                    .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
                let hosted = StoredHostedTopologySeat {
                    project_id,
                    seat_binding_id,
                    model_rung: model_rung.clone(),
                    native_identity: outcome.identity,
                    provider_session_id: outcome.provider_session_id,
                    observed_at: outcome.observed_at,
                };
                state
                    .with_store(|store| store.bind_hosted_topology_seat(&hosted))
                    .map_err(|error| self.refuse(&error))?;
                state
                    .with_store(|store| {
                        store.observe_seat_binding(
                            project_id,
                            seat_binding_id,
                            &SeatLivenessObservation {
                                attached_at: Some(hosted.observed_at),
                                runtime_reported: Some(
                                    kontor_core::state::ObservedRunState::Running,
                                ),
                                ..SeatLivenessObservation::default()
                            },
                            hosted.observed_at,
                        )
                    })
                    .map_err(|error| self.refuse(&error))?;
            }
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

    fn preview_core_team_route(
        &self,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &CoreTeamRoutePreviewRequest,
    ) -> Result<CoreTeamRoutePreviewDto, ApiError> {
        let state = self.state()?;
        let plan = self.core_team_route_plan(project_id, epic_id, request)?;
        Ok(CoreTeamRoutePreviewDto {
            realm_id: state.realm_id(),
            project_id,
            epic_id,
            seat_binding_id: plan.binding.id,
            predecessor_native_id: plan.predecessor.native_identity.native_id.clone(),
            current_model_route: runtime_model_route_dto(&plan.predecessor.model_rung),
            desired_model_route: runtime_model_route_dto(&plan.desired),
            would_replace_native: plan.successor.is_none()
                && plan.predecessor.model_rung != plan.desired,
            preview_hash: plan.preview_hash,
            snapshot_cursor: self.cursor()?,
        })
    }

    async fn apply_core_team_route(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &CoreTeamRouteApplyRequest,
    ) -> Result<CoreTeamRouteOutcomeDto, ApiError> {
        let state = self.state()?;
        let plan = self.core_team_route_plan(project_id, epic_id, &request.correction())?;
        if plan.preview_hash != request.preview_hash {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "the Core Team route correction no longer matches its preview",
            ));
        }
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "core_team_route_correction",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "seat_binding": plan.binding.id.to_string(),
            "predecessor": plan.predecessor.native_identity.native_id.as_str(),
            "preview": request.preview_hash.as_str(),
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        let replayed = self.replayed(key, &intent, Some(&target))?.is_some();

        let successor = if let Some(successor) = plan.successor.clone() {
            successor
        } else if plan.predecessor.model_rung == plan.desired {
            plan.predecessor.clone()
        } else {
            let runtime_kind = plan.predecessor.native_identity.runtime_kind.clone();
            let adapter = state.runtimes().get(&runtime_kind).ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the hosted seat runtime is not configured in this daemon",
                )
            })?;
            let retired = adapter
                .retire_hosted_seat(&HostedSeatRetireRequest {
                    seat_binding_id: plan.binding.id,
                    identity: plan.predecessor.native_identity.clone(),
                    model_rung: plan.predecessor.model_rung.clone(),
                    requested_at: kontor_api::now(),
                })
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            let control = state
                .with_store(|store| {
                    store.get_topology_node(project_id, plan.binding.topology_node_id)
                })
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the Core Team control-plane node no longer exists",
                    )
                })?;
            let cwd = self.runtime_root(project_id, Some(epic_id))?;
            let container = self
                .ensure_container(project_id, &control, &cwd, adapter.as_ref())
                .await?;
            let scope = self.execution_scope(project_id, epic_id, None, adapter.as_ref())?;
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
            let display_name =
                self.seat_name(project_id, &control, &scope, &plan.binding.role.role_code)?;
            let prompt = BoundedText::parse(&format!(
                "Persistent {} seat for epic {}. This native session replaces archived predecessor {} under the same Kontor SeatBinding {}. Continue only bounded work authorized through Kontor.",
                plan.binding.role.role_code.as_str(),
                scope.epic.external_epic_key.as_str(),
                plan.predecessor.native_identity.native_id.as_str(),
                plan.binding.id,
            ))
            .map_err(|error| self.refuse_domain(&error))?;
            let outcome = adapter
                .launch_hosted_seat(&HostedSeatLaunchRequest {
                    seat_binding_id: plan.binding.id,
                    role_slot_id: plan.binding.role_slot_id.clone(),
                    display_name,
                    container,
                    cwd,
                    scope,
                    prompt,
                    model_rung: plan.desired.clone(),
                    context_policy,
                    requested_at: kontor_api::now(),
                })
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            let successor = StoredHostedTopologySeat {
                project_id,
                seat_binding_id: plan.binding.id,
                model_rung: plan.desired.clone(),
                native_identity: outcome.identity,
                provider_session_id: outcome.provider_session_id,
                observed_at: outcome.observed_at,
            };
            state
                .with_store(|store| {
                    store.replace_hosted_topology_seat_route(
                        &plan.predecessor,
                        &successor,
                        retired.archived_at,
                        "authorized Core Team provider/model route correction",
                    )
                })
                .map_err(|error| self.refuse(&error))?;
            state
                .with_store(|store| {
                    store.observe_seat_binding(
                        project_id,
                        plan.binding.id,
                        &SeatLivenessObservation {
                            attached_at: Some(successor.observed_at),
                            runtime_reported: Some(kontor_core::state::ObservedRunState::Running),
                            ..SeatLivenessObservation::default()
                        },
                        successor.observed_at,
                    )
                })
                .map_err(|error| self.refuse(&error))?;
            successor
        };
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::CorrectCoreTeamRoute,
            target,
            plan.epic.revision,
            &intent,
        )?;
        Ok(CoreTeamRouteOutcomeDto {
            core_team: self.epic_core_team_dto(project_id, epic_id, &plan.roster)?,
            seat_binding_id: plan.binding.id,
            predecessor_native_id: plan.predecessor.native_identity.native_id,
            successor_native_id: successor.native_identity.native_id,
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: if replayed || plan.predecessor.model_rung == plan.desired {
                    AppliedDto::Unchanged
                } else {
                    AppliedDto::Updated
                },
                revision: plan.epic.revision,
                snapshot_cursor: self.cursor()?,
            },
        })
    }

    async fn message_hosted_seat(
        &self,
        project_id: ProjectId,
        seat_binding_id: SeatBindingId,
        message_id: MessageId,
        request: &HostedSeatMessageRequestDto,
    ) -> Result<HostedSeatMessageDto, ApiError> {
        let state = self.state()?;
        let binding = state
            .with_store(|store| store.get_seat_binding(project_id, seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .filter(|binding| binding.is_non_terminal())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the persistent Core Team seat is not active in this project",
                )
            })?;
        if binding.team_run_id.is_some() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "TeamRun delivery seats are messaged through the session message surface",
            ));
        }
        let hosted = state
            .with_store(|store| store.get_hosted_topology_seat(project_id, seat_binding_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the persistent Core Team seat has no native session",
                )
            })?;
        let adapter = state
            .runtimes()
            .get(&hosted.native_identity.runtime_kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the runtime holding this Core Team seat is not configured",
                )
            })?;
        let body = BoundedText::parse(&request.body).map_err(|error| self.refuse_domain(&error))?;
        let outcome = adapter
            .message_hosted_seat(&HostedSeatMessageRequest {
                seat_binding_id,
                identity: hosted.native_identity.clone(),
                message_id,
                body,
                sent_at: kontor_api::now(),
            })
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        Ok(HostedSeatMessageDto {
            realm_id: state.realm_id(),
            seat_binding_id,
            native_id: hosted.native_identity.native_id,
            message_id: outcome.message_id.to_string(),
            accepted_at: outcome.accepted_at,
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

        // Promotion creates the epic outside declarative `epics:apply`, so its
        // caller must carry the same explicit, immutable runtime-facing tokens
        // that import would store. Older bodies remain decodable; if they omit
        // the declaration, the pinned topology renderer below fails closed
        // before contacting the runtime rather than deriving a name from the
        // Quick-session purpose or generated epic id.
        if let Some(scope) = &request.execution_scope {
            let declaration = EpicExecutionScopeDeclaration {
                external_epic_key: scope.external_epic_key.clone(),
                short_title: scope.short_title.clone(),
                kontor_backlog_code: scope.kontor_backlog_code.clone(),
                ai_short_name: scope.ai_short_name.clone(),
            };
            state
                .with_store(|store| {
                    store.declare_epic_execution_scope(project_id, epic_id, &declaration, now)
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
        let mut receipt_intent = serde_json::json!({
            "schema_version": 1,
            "operation": "promotion_apply",
            "project": project_id.to_string(),
            "source": quick_session_id.to_string(),
            "preview": request.preview_hash.as_str(),
        });
        if let Some(scope) = &request.execution_scope {
            receipt_intent
                .as_object_mut()
                .expect("a promotion intent is an object")
                .insert("execution_scope".to_owned(), serde_json::json!(scope));
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::PromoteQuickSession,
            AggregateRef::MiniProject {
                mini_project_id: epic_id,
            },
            epic.revision,
            &self.intent(&receipt_intent)?,
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
        // The target the caller named, not whichever revision happens to be
        // current. An upgrade that silently retargeted itself would move a
        // running epic onto a roster nobody looked at.
        let target = self.published_core_team(project_id, request.target.version)?;
        let (current, bootstrap) = self.roster_upgrade_baseline(project_id, epic_id, &target)?;
        let effects = roster_upgrade_effects(&current, &target)
            .map_err(|error| self.refuse_domain(&error))?;
        Ok(RosterUpgradePreviewDto {
            realm_id: state.realm_id(),
            epic_id,
            preview_hash: self
                .roster_upgrade_hash(project_id, epic_id, &current, bootstrap, &target, &effects)?,
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
            // Recover the target first: this also reconstructs either the
            // frozen source roster or the explicit empty bootstrap baseline.
            let target =
                self.target_of_roster_preview(project_id, epic_id, &request.preview_hash)?;
            if target.revision_of_epic != request.expected_revision {
                return Err(self
                    .deny(
                        ApiErrorCode::RevisionConflict,
                        "the epic's roster moved since the caller previewed it",
                    )
                    .with_revision(Some(target.revision_of_epic)));
            }
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
            self.freeze_roster(project_id, epic_id, &target, target.quick_session_id, now)?;
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
    fn advisor_profiles(&self, project_id: ProjectId) -> Result<ProfileCatalogDto, ApiError> {
        self.consultation_catalog(project_id, ConsultationFamily::Advisor)
    }
    fn preview_advisor_profile(
        &self,
        project_id: ProjectId,
        request: &ProfilePreviewRequest,
    ) -> Result<ProfilePreviewDto, ApiError> {
        self.preview_consultation_profile(project_id, ConsultationFamily::Advisor, request)
    }
    async fn apply_advisor_profile(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &ProfileApplyRequest,
    ) -> Result<AppliedProfileDto, ApiError> {
        self.apply_consultation_profile(key, project_id, ConsultationFamily::Advisor, request)
    }
    async fn invoke_advisor_run(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &InvokeConsultationRequest,
    ) -> Result<AdvisorRunDto, ApiError> {
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "invoke_advisor_run",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "profile": [request.profile.id.as_str(), request.profile.version.get()],
            "question": request.question.as_str(),
            "caller_seat_binding_id": request.caller_seat_binding_id.to_string(),
            "task_id": request.task_id.map(|id| id.to_string()),
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
                        "the invocation receipt has no durable Advisor run",
                    )
                })?;
            return self.advisor_run_dto(&run, Some(receipt.id), AppliedDto::Unchanged);
        }
        let run = if let Some(existing) = self
            .state()?
            .with_store(|store| store.get_consultation_run_by_key(project_id, key))
            .map_err(|error| self.refuse(&error))?
        {
            if existing.id.family() != ConsultationFamily::Advisor
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
                        "the epic moved since the Advisor invocation was prepared",
                    )
                    .with_revision(Some(epic.revision)));
            }
            let revision = self
                .stored_consultation_profiles(project_id, ConsultationFamily::Advisor)?
                .into_iter()
                .find(|revision| {
                    revision.profile_id == request.profile.id
                        && revision.version == request.profile.version
                })
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "no such Advisor profile revision is published in this project",
                    )
                })?;
            let profile: AdvisorProfileSpec =
                serde_json::from_str(&revision.definition).map_err(|_| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the stored Advisor profile cannot be read by this build",
                    )
                })?;
            profile
                .validate()
                .map_err(|error| self.refuse_domain(&error))?;
            let spent = self
                .state()?
                .with_store(|store| {
                    store.list_consultation_runs(project_id, epic_id, ConsultationFamily::Advisor)
                })
                .map_err(|error| self.refuse(&error))?
                .into_iter()
                .filter(|run| run.profile_id == revision.profile_id)
                .count();
            if spent >= usize::try_from(profile.max_consultations).unwrap_or(usize::MAX) {
                return Err(self.deny(
                    ApiErrorCode::Forbidden,
                    "the pinned Advisor profile's per-epic consultation budget is exhausted",
                ));
            }
            let caller = self.authorize_consultation_caller(
                project_id,
                epic_id,
                request,
                &profile.allowed_scopes,
                &profile.allowed_caller_roles,
            )?;
            self.freeze_advisor_run(
                key, &intent, project_id, epic_id, request, &revision, &profile, &caller,
            )?
        };
        let (_, profile) = self.advisor_profile(&run)?;
        self.materialize_advisor_seat(&run, &profile).await?;
        let run = if run.state == ConsultationRunState::Materializing {
            self.state()?
                .with_store(|store| {
                    store.advance_consultation_run(
                        project_id,
                        run.id,
                        run.revision,
                        ConsultationRunState::Running,
                        None,
                        kontor_api::now(),
                    )
                })
                .map_err(|error| self.refuse(&error))?
        } else {
            run
        };
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::InvokeAdvisorRun,
            target,
            run.revision,
            &intent,
        )?;
        self.advisor_run_dto(&run, Some(receipt_id), AppliedDto::Created)
    }

    fn advisor_run(
        &self,
        project_id: ProjectId,
        advisor_run_id: AdvisorRunId,
    ) -> Result<AdvisorRunDto, ApiError> {
        let run = self.consultation_run(project_id, ConsultationRunId::Advisor(advisor_run_id))?;
        self.advisor_run_dto(&run, None, AppliedDto::Unchanged)
    }

    async fn settle_advisor_run(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        advisor_run_id: AdvisorRunId,
        request: &SettleConsultationRequest,
    ) -> Result<AdvisorRunDto, ApiError> {
        if request.recommendation.is_some() || request.tried_path.is_some() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "Committee remediation fields are not accepted on the Advisor route",
            ));
        }
        if request.seat_binding_id.is_some() {
            if request.disposition.is_some()
                || request.rationale.is_some()
                || !request.receipt_ids.is_empty()
            {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "the Advisor output step cannot record its requester's disposition",
                ));
            }
            let run =
                self.consultation_run(project_id, ConsultationRunId::Advisor(advisor_run_id))?;
            let target = AggregateRef::MiniProject {
                mini_project_id: run.mini_project_id,
            };
            let seat_binding_id = request.seat_binding_id.ok_or_else(|| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "Advisor output requires its authenticated seat binding",
                )
            })?;
            let output = request.output.as_ref().ok_or_else(|| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "the Advisor output step requires immutable output",
                )
            })?;
            let intent = self.intent(&serde_json::json!({
                "schema_version": 1,
                "operation": "record_advisor_advice",
                "project": project_id.to_string(),
                "advisor_run": advisor_run_id.to_string(),
                "seat_binding_id": seat_binding_id.to_string(),
                "output": output.as_str(),
            }))?;
            if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
                return self.advisor_run_dto(&run, Some(receipt.id), AppliedDto::Unchanged);
            }
            if run.state == ConsultationRunState::Settled {
                return Err(self.deny(
                    ApiErrorCode::IdempotencyConflict,
                    "this Advisor was settled under another idempotency key",
                ));
            }
            if run.revision != request.expected_revision {
                return Err(self
                    .deny(
                        ApiErrorCode::RevisionConflict,
                        "the Advisor run moved since the caller read it",
                    )
                    .with_revision(Some(run.revision)));
            }
            let seats = self
                .state()?
                .with_store(|store| store.list_consultation_seats(project_id, run.id))
                .map_err(|error| self.refuse(&error))?;
            let seat = seats
                .iter()
                .find(|seat| seat.seat_binding_id == seat_binding_id)
                .filter(|seat| seat.native_identity.is_some())
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::StaleBinding,
                        "the Advisor output does not come from its attested seat",
                    )
                })?;
            let advice_document = self.intent(&serde_json::json!({
                "schema_version": 1,
                "seat_binding_id": seat.seat_binding_id.to_string(),
                "output": output.as_str(),
            }))?;
            let advice: serde_json::Value =
                serde_json::from_str(advice_document.json()).map_err(|_| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the Advisor output could not be frozen",
                    )
                })?;
            let now = kontor_api::now();
            let (recorded, inserted) = self
                .state()?
                .with_store(|store| {
                    store.record_advisor_advice(
                        project_id,
                        advisor_run_id,
                        seat.seat_binding_id,
                        &advice,
                        advice_document.hash(),
                        run.revision,
                        now,
                    )
                })
                .map_err(|error| self.refuse(&error))?;
            let receipt_id = self.record(
                key,
                project_id,
                CommandKind::SettleAdvisorRun,
                target,
                recorded.revision,
                &intent,
            )?;
            return self.advisor_run_dto(
                &recorded,
                Some(receipt_id),
                if inserted {
                    AppliedDto::Created
                } else {
                    AppliedDto::Unchanged
                },
            );
        }

        if request.output.is_some() || request.seat_binding_id.is_some() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "the Advisor disposition step cannot author or identify the advice",
            ));
        }
        let run = self.consultation_run(project_id, ConsultationRunId::Advisor(advisor_run_id))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: run.mini_project_id,
        };
        let advice = self
            .state()?
            .with_store(|store| store.get_advisor_advice(project_id, advisor_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the Advisor seat has not submitted immutable advice yet",
                )
            })?;
        let disposition = request.disposition.ok_or_else(|| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "Advisor disposition requires a typed decision",
            )
        })?;
        let rationale = request.rationale.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "Advisor disposition requires bounded rationale",
            )
        })?;
        let disposition_text = match disposition {
            kontor_api::applications::AdviceDispositionDto::Accepted => "accepted",
            kontor_api::applications::AdviceDispositionDto::PartiallyAccepted => {
                "partially_accepted"
            }
            kontor_api::applications::AdviceDispositionDto::Rejected => "rejected",
            kontor_api::applications::AdviceDispositionDto::Superseded => "superseded",
        };
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "disposition_advisor_advice",
            "project": project_id.to_string(),
            "advisor_run": advisor_run_id.to_string(),
            "advice_hash": advice.document_hash.as_str(),
            "disposition": disposition_text,
            "rationale": rationale.as_str(),
            "receipt_ids": request.receipt_ids,
        }))?;
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            return self.advisor_run_dto(&run, Some(receipt.id), AppliedDto::Unchanged);
        }
        if run.state == ConsultationRunState::Settled {
            return Err(self.deny(
                ApiErrorCode::IdempotencyConflict,
                "this Advisor was dispositioned under another idempotency key",
            ));
        }
        if run.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the Advisor run moved since the caller read its advice",
                )
                .with_revision(Some(run.revision)));
        }
        let output = advice.document["output"].as_str().ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the stored Advisor advice has no bounded output",
            )
        })?;
        let result_document = self.intent(&serde_json::json!({
            "schema_version": 1,
            "seat_binding_id": advice.seat_binding_id.to_string(),
            "advice_hash": advice.document_hash.as_str(),
            "output": output,
            "disposition": match disposition {
                kontor_api::applications::AdviceDispositionDto::Accepted => "accepted",
                kontor_api::applications::AdviceDispositionDto::PartiallyAccepted => "partially_accepted",
                kontor_api::applications::AdviceDispositionDto::Rejected => "rejected",
                kontor_api::applications::AdviceDispositionDto::Superseded => "superseded",
            },
            "rationale": rationale.as_str(),
            "receipt_ids": request.receipt_ids,
        }))?;
        let result: serde_json::Value =
            serde_json::from_str(result_document.json()).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the Advisor result could not be frozen",
                )
            })?;
        let settled = self
            .state()?
            .with_store(|store| {
                store.advance_consultation_run(
                    project_id,
                    run.id,
                    run.revision,
                    ConsultationRunState::Settled,
                    Some((&result, result_document.hash())),
                    kontor_api::now(),
                )
            })
            .map_err(|error| self.refuse(&error))?;
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::SettleAdvisorRun,
            target,
            settled.revision,
            &intent,
        )?;
        self.advisor_run_dto(&settled, Some(receipt_id), AppliedDto::Created)
    }
    fn committee_templates(&self, project_id: ProjectId) -> Result<ProfileCatalogDto, ApiError> {
        self.consultation_catalog(project_id, ConsultationFamily::Committee)
    }
    fn preview_committee_template(
        &self,
        project_id: ProjectId,
        request: &ProfilePreviewRequest,
    ) -> Result<ProfilePreviewDto, ApiError> {
        self.preview_consultation_profile(project_id, ConsultationFamily::Committee, request)
    }
    async fn apply_committee_template(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &ProfileApplyRequest,
    ) -> Result<AppliedProfileDto, ApiError> {
        self.apply_consultation_profile(key, project_id, ConsultationFamily::Committee, request)
    }
    async fn invoke_committee_run(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &InvokeConsultationRequest,
    ) -> Result<CommitteeRunDto, ApiError> {
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "invoke_committee_run",
            "project": project_id.to_string(),
            "epic": epic_id.to_string(),
            "profile": [request.profile.id.as_str(), request.profile.version.get()],
            "question": request.question.as_str(),
            "caller_seat_binding_id": request.caller_seat_binding_id.to_string(),
            "task_id": request.task_id.map(|id| id.to_string()),
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
                        "the invocation receipt has no durable Committee run",
                    )
                })?;
            return self.committee_run_dto(&run, Some(receipt.id), AppliedDto::Unchanged);
        }

        let run = if let Some(existing) = self
            .state()?
            .with_store(|store| store.get_consultation_run_by_key(project_id, key))
            .map_err(|error| self.refuse(&error))?
        {
            if existing.id.family() != ConsultationFamily::Committee
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
                        "the epic moved since the Committee invocation was prepared",
                    )
                    .with_revision(Some(epic.revision)));
            }
            let revision = self
                .stored_consultation_profiles(project_id, ConsultationFamily::Committee)?
                .into_iter()
                .find(|revision| {
                    revision.profile_id == request.profile.id
                        && revision.version == request.profile.version
                })
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "no such Committee template revision is published in this project",
                    )
                })?;
            let template: CommitteeTemplateSpec = serde_json::from_str(&revision.definition)
                .map_err(|_| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the stored Committee template cannot be read by this build",
                    )
                })?;
            template
                .validate()
                .map_err(|error| self.refuse_domain(&error))?;
            let caller =
                self.authorize_committee_caller(project_id, epic_id, request, &template)?;
            self.freeze_committee_run(
                key,
                &intent,
                CommitteeInvocation {
                    project_id,
                    epic_id,
                    request,
                    template_revision: &revision,
                    template: &template,
                    caller: &caller,
                },
            )?
        };
        let (_, template) = self.committee_template(&run)?;
        self.materialize_committee_seats(&run, &template, false)
            .await?;
        let run = if run.state == ConsultationRunState::Materializing {
            self.state()?
                .with_store(|store| {
                    store.advance_consultation_run(
                        project_id,
                        run.id,
                        run.revision,
                        ConsultationRunState::Running,
                        None,
                        kontor_api::now(),
                    )
                })
                .map_err(|error| self.refuse(&error))?
        } else {
            run
        };
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::InvokeCommitteeRun,
            target,
            run.revision,
            &intent,
        )?;
        self.committee_run_dto(&run, Some(receipt_id), AppliedDto::Created)
    }

    fn committee_run(
        &self,
        project_id: ProjectId,
        committee_run_id: CommitteeRunId,
    ) -> Result<CommitteeRunDto, ApiError> {
        let run =
            self.consultation_run(project_id, ConsultationRunId::Committee(committee_run_id))?;
        self.committee_run_dto(&run, None, AppliedDto::Unchanged)
    }
    async fn record_committee_findings(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        committee_run_id: CommitteeRunId,
        seat_binding_id: SeatBindingId,
        request: &RecordFindingsRequest,
    ) -> Result<CommitteeRunDto, ApiError> {
        let mut run =
            self.consultation_run(project_id, ConsultationRunId::Committee(committee_run_id))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: run.mini_project_id,
        };
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "record_committee_findings",
            "project": project_id.to_string(),
            "committee_run": committee_run_id.to_string(),
            "seat_binding_id": seat_binding_id.to_string(),
            "round": request.round,
            "verdict": match request.verdict {
                ConsultationVerdictDto::Compliant => "compliant",
                ConsultationVerdictDto::NonCompliant => "non_compliant",
            },
            "evidence_complete": request.evidence_complete,
            "rationale": request.rationale.as_str(),
            "evidence_refs": request.evidence_refs,
        }))?;
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            return self.committee_run_dto(&run, Some(receipt.id), AppliedDto::Unchanged);
        }
        if run.state == ConsultationRunState::Settled
            || run.state == ConsultationRunState::NeedsHuman
        {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "this Committee run no longer accepts findings",
            ));
        }
        if request.round != run.round {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "the finding does not name the Committee's current round",
            ));
        }
        if request.evidence_complete && request.evidence_refs.is_empty() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "a finding cannot claim complete evidence without an evidence reference",
            ));
        }
        let (_, template) = self.committee_template(&run)?;
        let seats = self
            .state()?
            .with_store(|store| store.list_consultation_seats(project_id, run.id))
            .map_err(|error| self.refuse(&error))?;
        let seat = seats
            .iter()
            .find(|seat| seat.seat_binding_id == seat_binding_id)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Forbidden,
                    "the submitting SeatBinding is not a seat of this Committee",
                )
            })?;
        if seat.native_identity.is_none() {
            return Err(self.deny(
                ApiErrorCode::StaleBinding,
                "the submitting Committee seat has no attested native session",
            ));
        }
        let role = seat.committee_role.ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "a Committee seat has no frozen Committee role",
            )
        })?;
        let verdict = match request.verdict {
            ConsultationVerdictDto::Compliant => ConsultationVerdict::Compliant,
            ConsultationVerdictDto::NonCompliant => ConsultationVerdict::NonCompliant,
        };
        let before = self
            .state()?
            .with_store(|store| {
                store.list_committee_findings(project_id, committee_run_id, run.round)
            })
            .map_err(|error| self.refuse(&error))?;
        let reviewer_findings: Vec<RecordedFinding> = before
            .iter()
            .filter(|finding| finding.role == CommitteeRole::Reviewer)
            .map(|finding| RecordedFinding {
                slot: finding.role_slot_id.clone(),
                verdict: finding.verdict,
                evidence_complete: finding.evidence_complete,
            })
            .collect();
        if role == CommitteeRole::Judge {
            let recomputed = conjunctive_outcome(
                &template
                    .reviewer_slots()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>(),
                &reviewer_findings,
            )
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "the Judge cannot submit before every independent finding is durable",
                )
            })?;
            if verdict != recomputed {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "the Judge verdict contradicts the server-recomputed conjunction",
                ));
            }
        }
        let document = self.intent(&serde_json::json!({
            "schema_version": 1,
            "committee_run_id": committee_run_id.to_string(),
            "round": request.round,
            "role_slot_id": seat.role_slot_id.as_str(),
            "role": role.as_str(),
            "verdict": verdict.as_str(),
            "evidence_complete": request.evidence_complete,
            "rationale": request.rationale.as_str(),
            "evidence_refs": request.evidence_refs,
        }))?;
        let finding = StoredCommitteeFinding {
            committee_run_id,
            round: request.round,
            role_slot_id: seat.role_slot_id.clone(),
            role,
            verdict,
            evidence_complete: request.evidence_complete,
            document: serde_json::from_str(document.json()).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the Committee finding could not be frozen",
                )
            })?,
            document_hash: document.hash().clone(),
            recorded_at: kontor_api::now(),
        };
        let mut projected = reviewer_findings;
        if role == CommitteeRole::Reviewer
            && !projected
                .iter()
                .any(|item| item.slot == finding.role_slot_id)
        {
            projected.push(RecordedFinding {
                slot: finding.role_slot_id.clone(),
                verdict: finding.verdict,
                evidence_complete: finding.evidence_complete,
            });
        }
        let reviewers_complete = conjunctive_outcome(
            &template
                .reviewer_slots()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            &projected,
        )
        .is_some();
        let next_state = if reviewers_complete {
            ConsultationRunState::AwaitingJudge
        } else {
            ConsultationRunState::Running
        };
        let (updated, inserted) = self
            .state()?
            .with_store(|store| {
                store.record_committee_finding(
                    project_id,
                    run.id,
                    &finding,
                    request.expected_revision,
                    next_state,
                    kontor_api::now(),
                )
            })
            .map_err(|error| self.refuse(&error))?;
        run = updated;
        if reviewers_complete && template.judge_slot().is_some() {
            self.materialize_committee_seats(&run, &template, true)
                .await?;
            if run.round == 2 {
                self.dispatch_committee_round_two(&run, &template, true)
                    .await?;
            }
        }
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::RecordCommitteeFindings,
            target,
            run.revision,
            &intent,
        )?;
        self.committee_run_dto(
            &run,
            Some(receipt_id),
            if inserted {
                AppliedDto::Created
            } else {
                AppliedDto::Unchanged
            },
        )
    }
    async fn settle_committee_run(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        committee_run_id: CommitteeRunId,
        request: &SettleConsultationRequest,
    ) -> Result<CommitteeRunDto, ApiError> {
        let run =
            self.consultation_run(project_id, ConsultationRunId::Committee(committee_run_id))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: run.mini_project_id,
        };
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "settle_committee_run",
            "project": project_id.to_string(),
            "committee_run": committee_run_id.to_string(),
            "recommendation": request.recommendation.as_ref().map(BoundedText::as_str),
            "tried_path": request.tried_path.as_ref().map(BoundedText::as_str),
        }))?;
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            return self.committee_run_dto(&run, Some(receipt.id), AppliedDto::Unchanged);
        }
        if request.seat_binding_id.is_some()
            || request.output.is_some()
            || request.disposition.is_some()
            || request.rationale.is_some()
            || !request.receipt_ids.is_empty()
        {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "Advisor settlement fields are not accepted on the Committee route",
            ));
        }
        if run.state == ConsultationRunState::Settled {
            return Err(self.deny(
                ApiErrorCode::IdempotencyConflict,
                "this Committee was settled under another idempotency key",
            ));
        }
        if run.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the Committee run moved since the caller read it",
                )
                .with_revision(Some(run.revision)));
        }
        let (_, template) = self.committee_template(&run)?;
        let findings = self
            .state()?
            .with_store(|store| {
                store.list_committee_findings(project_id, committee_run_id, run.round)
            })
            .map_err(|error| self.refuse(&error))?;
        let reviewers: Vec<RecordedFinding> = findings
            .iter()
            .filter(|finding| finding.role == CommitteeRole::Reviewer)
            .map(|finding| RecordedFinding {
                slot: finding.role_slot_id.clone(),
                verdict: finding.verdict,
                evidence_complete: finding.evidence_complete,
            })
            .collect();
        let outcome = conjunctive_outcome(
            &template
                .reviewer_slots()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
            &reviewers,
        )
        .ok_or_else(|| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "a Committee cannot settle before every independent finding is durable",
            )
        })?;
        if let Some(judge_slot) = template.judge_slot() {
            let judge = findings
                .iter()
                .find(|finding| &finding.role_slot_id == judge_slot)
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::InvalidRequest,
                        "the Committee cannot settle before its Judge aggregate is durable",
                    )
                })?;
            if judge.role != CommitteeRole::Judge || judge.verdict != outcome {
                return Err(self.deny(
                    ApiErrorCode::InvalidRequest,
                    "the durable Judge aggregate contradicts the recomputed outcome",
                ));
            }
        }
        let evidence_hash = self
            .intent(&serde_json::json!({
                "schema_version": 1,
                "committee_run_id": committee_run_id.to_string(),
                "round": run.round,
                "findings": findings
                    .iter()
                    .map(|finding| finding.document_hash.as_str())
                    .collect::<Vec<_>>(),
            }))?
            .hash()
            .clone();
        let result_document = self.intent(&serde_json::json!({
            "schema_version": 1,
            "verdict": outcome.as_str(),
            "evidence_hash": evidence_hash.as_str(),
            "round": run.round,
            "finding_hashes": findings
                .iter()
                .map(|finding| finding.document_hash.as_str())
                .collect::<Vec<_>>(),
        }))?;
        let result: serde_json::Value =
            serde_json::from_str(result_document.json()).map_err(|_| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "the Committee result could not be frozen",
                )
            })?;
        if outcome == ConsultationVerdict::NonCompliant {
            let recommendation = request.recommendation.as_ref().ok_or_else(|| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "a non-compliant Committee requires the LSA recommendation",
                )
            })?;
            let tried_path = request.tried_path.as_ref().ok_or_else(|| {
                self.deny(
                    ApiErrorCode::InvalidRequest,
                    "a non-compliant Committee requires the exact tried path",
                )
            })?;
            if run.round < template.round_limit {
                let remediation_document = self.intent(&serde_json::json!({
                    "schema_version": 1,
                    "committee_run_id": committee_run_id.to_string(),
                    "from_round": run.round,
                    "recommendation": recommendation.as_str(),
                    "tried_path": tried_path.as_str(),
                    "failed_result_hash": result_document.hash().as_str(),
                }))?;
                let remediation: serde_json::Value =
                    serde_json::from_str(remediation_document.json()).map_err(|_| {
                        self.deny(
                            ApiErrorCode::Unavailable,
                            "the Committee remediation could not be frozen",
                        )
                    })?;
                let remediating = self
                    .state()?
                    .with_store(|store| {
                        store.remediate_committee_run(
                            project_id,
                            committee_run_id,
                            run.revision,
                            recommendation,
                            tried_path,
                            &remediation,
                            remediation_document.hash(),
                            kontor_api::now(),
                        )
                    })
                    .map_err(|error| self.refuse(&error))?;
                self.dispatch_committee_round_two(&remediating, &template, false)
                    .await?;
                let receipt_id = self.record(
                    key,
                    project_id,
                    CommandKind::SettleCommitteeRun,
                    target,
                    remediating.revision,
                    &intent,
                )?;
                return self.committee_run_dto(&remediating, Some(receipt_id), AppliedDto::Created);
            }
            let needs_human_document = self.intent(&serde_json::json!({
                "schema_version": 1,
                "verdict": outcome.as_str(),
                "reason": "remediation_budget_exhausted",
                "round": run.round,
                "recommendation": recommendation.as_str(),
                "tried_path": tried_path.as_str(),
                "failed_result_hash": result_document.hash().as_str(),
            }))?;
            let needs_human: serde_json::Value = serde_json::from_str(needs_human_document.json())
                .map_err(|_| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "the needs-human result could not be frozen",
                    )
                })?;
            let escalated = self
                .state()?
                .with_store(|store| {
                    store.advance_consultation_run(
                        project_id,
                        run.id,
                        run.revision,
                        ConsultationRunState::NeedsHuman,
                        Some((&needs_human, needs_human_document.hash())),
                        kontor_api::now(),
                    )
                })
                .map_err(|error| self.refuse(&error))?;
            let receipt_id = self.record(
                key,
                project_id,
                CommandKind::SettleCommitteeRun,
                target,
                escalated.revision,
                &intent,
            )?;
            return self.committee_run_dto(&escalated, Some(receipt_id), AppliedDto::Created);
        }
        if request.recommendation.is_some() || request.tried_path.is_some() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "a compliant Committee does not accept a remediation brief",
            ));
        }
        let settled = self
            .state()?
            .with_store(|store| {
                store.advance_consultation_run(
                    project_id,
                    run.id,
                    run.revision,
                    ConsultationRunState::Settled,
                    Some((&result, result_document.hash())),
                    kontor_api::now(),
                )
            })
            .map_err(|error| self.refuse(&error))?;
        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::SettleCommitteeRun,
            target,
            settled.revision,
            &intent,
        )?;
        self.committee_run_dto(&settled, Some(receipt_id), AppliedDto::Created)
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
        let PreparedEpic {
            bundle,
            execution_scope,
            tasks,
        } = self.prepare_epic(project_id, request, now)?;

        // The key is judged before the graph is written. `apply_epic` is atomic,
        // so a conflict discovered afterwards would have to be reported against a
        // graph this call had already created.
        let mut intent_document = serde_json::json!({
            "schema_version": 1,
            "operation": "epics_apply",
            "epic": request.name.as_str(),
            "work_profile_category": request.work_profile_category,
            "runtime_family": request.runtime_family.as_str(),
            "tasks": request
                .tasks
                .iter()
                .map(|task| {
                    let mut intent = serde_json::json!({
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
                    });
                    // Preserve the pre-ASMA-7941 intent bytes for the omitted
                    // compatibility default, so an old apply receipt remains
                    // replayable after upgrade. Only the new historical fact
                    // widens the intent.
                    if task.import_state != EpicImportStateDto::Ready {
                        intent
                            .as_object_mut()
                            .expect("an epic task intent is an object")
                            .insert(
                                "import_state".to_owned(),
                                serde_json::json!(task.import_state),
                            );
                    }
                    if let Some(short_code) = &task.short_code {
                        intent
                            .as_object_mut()
                            .expect("an epic task intent is an object")
                            .insert("short_code".to_owned(), serde_json::json!(short_code));
                    }
                    if let Some(ai_short_name) = &task.ai_short_name {
                        intent
                            .as_object_mut()
                            .expect("an epic task intent is an object")
                            .insert(
                                "ai_short_name".to_owned(),
                                serde_json::json!(ai_short_name),
                            );
                    }
                    intent
                })
                .collect::<Vec<_>>(),
        });
        // Preserve old receipt bytes when the new declaration is omitted. A
        // replay created before schema 43 must remain the same operation.
        if let Some(scope) = &request.execution_scope {
            let mut scope_intent = serde_json::json!({
                "external_epic_key": scope.external_epic_key.as_str(),
                "short_title": scope.short_title.as_str(),
            });
            if let Some(backlog_code) = &scope.kontor_backlog_code {
                scope_intent
                    .as_object_mut()
                    .expect("an execution-scope intent is an object")
                    .insert(
                        "kontor_backlog_code".to_owned(),
                        serde_json::json!(backlog_code),
                    );
            }
            if let Some(ai_short_name) = &scope.ai_short_name {
                scope_intent
                    .as_object_mut()
                    .expect("an execution-scope intent is an object")
                    .insert("ai_short_name".to_owned(), serde_json::json!(ai_short_name));
            }
            intent_document
                .as_object_mut()
                .expect("an epic intent is an object")
                .insert("execution_scope".to_owned(), scope_intent);
        }
        let intent = self.intent(&intent_document)?;
        if let Some(receipt) = self.replayed(key, &intent, None)? {
            let AggregateRef::MiniProject { mini_project_id } = receipt.target else {
                return Err(self.deny(
                    ApiErrorCode::IdempotencyConflict,
                    "the idempotency key was already used for a different operation",
                ));
            };
            return self.applied_epic_replay(project_id, mini_project_id, &bundle);
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
                    execution_scope: execution_scope.as_ref(),
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
            execution_scope: applied.execution_scope.map(|scope| EpicExecutionScopeDto {
                external_epic_key: scope.external_epic_key,
                short_title: scope.short_title,
                kontor_backlog_code: scope.kontor_backlog_code,
                ai_short_name: scope.ai_short_name,
            }),
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
                    short_code: task.short_code,
                    ai_short_name: task.ai_short_name,
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

    async fn preview_epic(
        &self,
        project_id: ProjectId,
        request: &ApplyEpicRequest,
    ) -> Result<PreviewEpicDto, ApiError> {
        let state = self.state()?;
        let now = kontor_api::now();
        let PreparedEpic {
            bundle,
            execution_scope,
            tasks,
        } = self.prepare_epic(project_id, request, now)?;
        let preview = state
            .with_store(|store| {
                store.preview_epic(&EpicApplication {
                    project_id,
                    name: request.name.clone(),
                    execution_scope: execution_scope.as_ref(),
                    tasks: &tasks,
                    profile: &bundle.profile,
                    definition: &bundle.profile.definition,
                    team: bundle.team.as_ref(),
                    applied_at: now,
                })
            })
            .map_err(|error| self.refuse(&error))?;

        Ok(PreviewEpicDto {
            realm_id: state.realm_id(),
            project_id,
            epic_id: (preview.applied != Applied::Created).then_some(preview.mini_project_id),
            applied: applied_dto(preview.applied),
            execution_scope: preview.execution_scope.map(|scope| EpicExecutionScopeDto {
                external_epic_key: scope.external_epic_key,
                short_title: scope.short_title,
                kontor_backlog_code: scope.kontor_backlog_code,
                ai_short_name: scope.ai_short_name,
            }),
            work_profile: RevisionRefDto {
                id: preview.profile.0.as_str().to_owned(),
                version: preview.profile.1,
            },
            team_template: preview.team.map(|(id, version)| RevisionRefDto {
                id: id.to_string(),
                version,
            }),
            tasks: preview
                .tasks
                .into_iter()
                .map(|task| PreviewEpicTaskDto {
                    title: task.title,
                    task_id: (task.applied != Applied::Created).then_some(task.task_id),
                    short_code: task.short_code,
                    ai_short_name: task.ai_short_name,
                    applied: applied_dto(task.applied),
                    state: task.state.as_str().to_owned(),
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
        let execution_scope = state
            .with_store(|store| store.get_epic_execution_scope(project_id, epic_id))
            .map_err(|error| self.refuse(&error))?;
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
                short_code: state
                    .with_store(|store| store.task_short_code(project_id, task.id))
                    .map_err(|error| self.refuse(&error))?,
                ai_short_name: state
                    .with_store(|store| store.task_ai_short_name(project_id, task.id))
                    .map_err(|error| self.refuse(&error))?,
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
            execution_scope: execution_scope.map(|scope| EpicExecutionScopeDto {
                external_epic_key: scope.external_epic_key,
                short_title: scope.short_title,
                kontor_backlog_code: scope.kontor_backlog_code,
                ai_short_name: scope.ai_short_name,
            }),
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

        let budget = self.armed_budget(project_id, epic_id, request.budget.as_ref())?;
        // The *resolved* grant is in the intent, not the request's optional
        // shape. A replay must converge on what was actually authorized, so a
        // second call that omits the budget and one that restates the same
        // numbers are the same command — while one that narrows them differently
        // is a conflict rather than a replay of the first.
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "execution_arm",
            "tasks": request.tasks.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "allowed_start": request.allowed_start.to_string(),
            "allowed_end": request.allowed_end.to_string(),
            "max_concurrency": request.max_concurrency,
            "max_tokens": budget.max_tokens,
            "max_commands": budget.max_commands,
            "max_duration_seconds": budget.max_duration_seconds,
            "max_cost_minor_units": budget.max_cost.minor_units,
            "cost_currency": budget.max_cost.currency.as_str(),
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
            // The bounds the grant was taken under, kept verbatim. A receipt
            // records what was authorized and is not rewritten by a later change
            // of the profile it defaulted from.
            budget,
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
                // A previous partial admission may have let an already-bound
                // predecessor settle while a downstream seat was still
                // unbound. Its handoff is durable and deliberately
                // undelivered. Once this exact replay finishes binding the
                // missing seat, complete that existing dispatch now; waiting
                // for a daemon restart would leave a successfully recovered
                // team idle indefinitely.
                self.retry_undelivered_dispatches().await?;
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

    async fn resume_admissions(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        epic_id: MiniProjectId,
        request: &ResumeAdmissionsRequest,
    ) -> Result<SchedulerResumeDto, ApiError> {
        let state = self.state()?;
        if !state.barrier().state().is_open() {
            return Err(self.deny(
                ApiErrorCode::ReconciliationPending,
                "startup reconciliation has not finished, so nothing may be resumed",
            ));
        }
        if request.admissions.is_empty() {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "exact admission recovery names at least one TeamRun and AgentRun",
            ));
        }
        let team_runs: BTreeSet<TeamRunId> = request
            .admissions
            .iter()
            .map(|admission| admission.team_run_id)
            .collect();
        let agent_runs: BTreeSet<AgentRunId> = request
            .admissions
            .iter()
            .map(|admission| admission.agent_run_id)
            .collect();
        if team_runs.len() != request.admissions.len()
            || agent_runs.len() != request.admissions.len()
        {
            return Err(self.deny(
                ApiErrorCode::InvalidRequest,
                "exact admission recovery cannot name a TeamRun or AgentRun twice",
            ));
        }

        let epic = self.epic_row(project_id, epic_id)?;
        if epic.revision != request.expected_revision {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the epic changed after these admissions were selected for recovery",
            ));
        }
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "scheduler_resume",
            "epic_id": epic_id.to_string(),
            "expected_revision": request.expected_revision.get(),
            "admissions": request.admissions,
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        let replayed = self.replayed(key, &intent, Some(&target))?.is_some();

        // Resolve and validate the whole set before any runtime is contacted.
        // This makes the operation atomic at the authority boundary: a drifted
        // fourth pair cannot launch the first three before it is refused.
        let mut recoverable = Vec::with_capacity(request.admissions.len());
        for address in &request.admissions {
            let recovered = state
                .with_store(|store| {
                    store.recoverable_admission(
                        project_id,
                        address.team_run_id,
                        address.agent_run_id,
                    )
                })
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "no immutable admission event names that TeamRun and AgentRun pair",
                    )
                })?;
            let team = state
                .with_store(|store| store.get_team_run(project_id, address.team_run_id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "the admitted TeamRun no longer exists",
                    )
                })?;
            let agent = state
                .with_store(|store| store.get_agent_run(project_id, address.agent_run_id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "the admitted AgentRun no longer exists",
                    )
                })?;
            let task = self.task_row(project_id, recovered.admitted.task_id)?;
            if team.task_id != recovered.admitted.task_id
                || agent.team_run_id != team.id
                || task.mini_project_id != Some(epic_id)
            {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the exact admission identities no longer agree with their task and epic",
                ));
            }
            if team.lifecycle.is_terminal() || agent.terminal.is_some() {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "a terminal admission cannot be resumed",
                ));
            }
            if !replayed
                && (team.lifecycle != kontor_core::state::RunLifecycle::Queued
                    || agent.projection.lifecycle != kontor_core::state::RunLifecycle::Queued
                    || agent.projection.desired
                        != kontor_core::state::DesiredRunState::RunRequested
                    || agent.binding.is_some())
            {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "only an exact queued and unbound admission may be freshly resumed",
                ));
            }
            recoverable.push((address, recovered));
        }

        let receipt_id = self.record(
            key,
            project_id,
            CommandKind::StartScheduledWork,
            target,
            epic.revision,
            &intent,
        )?;
        let mut started = Vec::new();
        let mut blocked = Vec::new();
        for (address, recovered) in recoverable {
            match self
                .seat_with_address(
                    project_id,
                    &recovered.admitted,
                    &recovered.launch_key,
                    Some(address.team_run_id),
                    Some(address.agent_run_id),
                )
                .await
            {
                Ok(seats) => started.extend(seats),
                Err(refusal) => blocked.push(BlockedTaskDto {
                    task_id: recovered.admitted.task_id,
                    code: refusal.code.as_str().to_owned(),
                    evidence: vec![serde_json::json!({
                        "kind": "seat",
                        "rule": refusal.rule,
                    })],
                }),
            }
        }
        self.mark_started_tasks_in_progress(project_id, &started)?;
        self.retry_undelivered_dispatches().await?;
        state.signals().appended();
        Ok(SchedulerResumeDto {
            realm_id: state.realm_id(),
            started,
            blocked,
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
        let plan_key_text = format!(
            "ticket-plan:{}:{}",
            task_id,
            self.task_row(project_id, task_id)?.revision.get()
        );
        let plan_key =
            IdempotencyKey::parse(&plan_key_text).map_err(|error| self.refuse_domain(&error))?;
        let plan = self
            .prepare_ticket_plan(project_id, task_id, &plan_key)
            .await?;
        Ok(TicketReconcilePlanDto {
            realm_id: state.realm_id(),
            task_id,
            projection_hash: plan.hash,
            links: plan.links.iter().map(ToString::to_string).collect(),
            converged: plan.diff.is_empty(),
            diff: plan.diff,
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
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "ticket_reconcile_apply",
            "task_id": task_id.to_string(),
            "projection_hash": request.projection_hash,
        }))?;
        let target = AggregateRef::Task { task_id };
        let replayed = self.replayed(key, &intent, Some(&target))?;
        let plan = self.prepare_ticket_plan(project_id, task_id, key).await?;

        // A replay whose first attempt already converged is successful even
        // though the live observation now produces a different (empty) plan.
        // A replay that still sees the original difference resumes the same
        // idempotent external operation instead of papering over a lost effect.
        if plan.diff.is_empty()
            && let Some(existing) = replayed.as_ref()
        {
            return Ok(TicketReconcileAppliedDto {
                realm_id: state.realm_id(),
                task_id,
                projection_hash: request.projection_hash.clone(),
                converged: plan.links.iter().map(ToString::to_string).collect(),
                receipt_id: existing.id.to_string(),
            });
        }
        // The plan is re-derived and its digest compared, exactly as a scheduler
        // start re-derives its batch: applying a plan the realm has moved past
        // would converge a ticket towards something nobody looked at.
        if plan.hash != request.projection_hash {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the named reconciliation plan no longer describes this realm",
            ));
        }
        let receipt = if let Some(existing) = replayed {
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

        // An unlinked task has no external boundary to invoke. Its empty plan
        // is still a valid, durable reconciliation receipt, and composing Jira
        // must not become a prerequisite for projects that have no Jira links.
        if plan.tickets.is_empty() {
            return Ok(TicketReconcileAppliedDto {
                realm_id: state.realm_id(),
                task_id,
                projection_hash: request.projection_hash.clone(),
                converged: Vec::new(),
                receipt_id: receipt.to_string(),
            });
        }

        let (field_spec, workflow_spec) = self.jira_specs(
            &state
                .with_store(|store| store.get_active_task_workflow(project_id, task_id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "a Jira-linked task has no active workflow specification",
                    )
                })?,
        )?;
        let asma = self.asma()?;
        for ticket in &plan.tickets {
            let Some(transition) = &ticket.transition else {
                continue;
            };
            let delegation = TicketDelegation {
                asma,
                field_spec: &field_spec,
                workflow_spec: &workflow_spec,
                projection: &ticket.projection,
                facts: &ticket.facts,
                link_id: ticket.link.id,
                idempotency_key: &ticket.wire_key,
            };
            let response = delegation
                .apply(
                    &ticket.observed,
                    transition,
                    ApplyAuthority {
                        authorized_by: receipt,
                    },
                )
                .await
                .map_err(|error| self.refuse_asma(&error))?;
            let transition_receipt = delegation
                .receipt(&ticket.observed, transition, &response)
                .map_err(|error| self.refuse_asma(&error))?;
            state
                .with_store(|store| {
                    store.append_observation(project_id, &ticket.observed.observation)
                })
                .map_err(|error| self.refuse(&error))?;
            if let Some(confirmation) = &response.confirmation {
                let mut confirmed = confirmation
                    .observation
                    .to_core(ticket.link.id, confirmation.confirmed_at)
                    .map_err(|error| self.refuse_domain(&error))?;
                confirmed.id = transition_receipt.refetched_observation_id.ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::Unavailable,
                        "a confirmed Jira transition receipt names no refetched observation",
                    )
                })?;
                state
                    .with_store(|store| store.append_observation(project_id, &confirmed))
                    .map_err(|error| self.refuse(&error))?;
            }
            state
                .with_store(|store| {
                    store.insert_transition_receipt(project_id, &transition_receipt)
                })
                .map_err(|error| self.refuse(&error))?;
        }
        Ok(TicketReconcileAppliedDto {
            realm_id: state.realm_id(),
            task_id,
            projection_hash: request.projection_hash.clone(),
            converged: plan.links.iter().map(ToString::to_string).collect(),
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
            self.release_team_seats(project_id, team_run_id, now)?;
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
                self.release_team_seats(project_id, team_run_id, now)?;
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
        let mut predecessor = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such predecessor run exists in this project",
                )
            })?;
        let binding = predecessor.binding.clone().ok_or_else(|| {
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

        let mut intent_document = serde_json::json!({
            "schema_version": 1,
            "operation": "replace_seat",
            "predecessor_agent_run_id": agent_run_id.to_string(),
            "team_run_id": predecessor.team_run_id.to_string(),
            "role_slot": role_slot.as_role_key().as_str(),
            "task_revision": task.revision.get(),
            "binding_generation": binding.identity.generation,
        });
        if let Some(evidence) = &request.unavailable_provider {
            intent_document["unavailable_provider"] = serde_json::json!({
                "runtime_binding_id": evidence.runtime_binding_id,
                "native_id": evidence.native_id,
                "provider": evidence.provider,
            });
        }
        let intent = self.intent(&intent_document)?;
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

        if predecessor.terminal.is_none() {
            predecessor = self
                .retire_predecessor_for_replacement(
                    project_id,
                    &predecessor,
                    &binding,
                    request.unavailable_provider.as_ref(),
                    now,
                )
                .await?;
        }
        let terminal = predecessor.terminal.as_ref().ok_or_else(|| {
            self.deny(
                ApiErrorCode::UnsupportedCapability,
                "the predecessor retirement produced no terminal evidence",
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
        // A crash may persist the terminal observation just before releasing
        // the in-process snapshot. Replaying the same Admin command completes
        // that release rather than wedging an already-retired predecessor.
        if state.sessions().get(binding.id).is_some() {
            self.release(binding.id)?;
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
        self.ensure_launch_intent(project_id, successor_agent_run_id)?;

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
        let epic_id = task.mini_project_id.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the replacement task is not scoped to an epic",
            )
        })?;
        let scope = self.execution_scope(project_id, epic_id, Some(task_id), adapter.as_ref())?;
        let quota_states = state
            .with_store(|store| store.list_provider_quota_states(project_id))
            .map_err(|error| self.refuse(&error))?;
        let eligible = self.eligible_accounts(project_id)?;
        let outlook = QuotaOutlook {
            states: &quota_states,
            account: predecessor.account_profile_id,
            accounts: &eligible,
            headroom: self.headroom_policy(),
            now,
        };
        let model_rung = request
            .model_route
            .as_ref()
            .map_or_else(
                || freeze_seat_model_rung(adapter.as_ref(), &team.snapshot, &role_slot, &outlook),
                parse_runtime_model_route,
            )
            .map_err(|error| self.refuse_domain(&error))?;
        let context_policy = freeze_seat_context_policy(&adapter, &team.snapshot, &role_slot, now)
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let launch = SlotLaunch {
            display_name: self.delivery_seat_name(
                project_id,
                task_id,
                &scope,
                &team.snapshot,
                &role_slot,
            )?,
            scope,
            task_id,
            binding_id,
            placement: Some(LaunchPlacement::Container(workspace.clone())),
            cwd: task_root.clone(),
            account_profile_id: predecessor.account_profile_id,
            prompt: slot_prompt(&role_slot, &eligible_roots(slots.template()))
                .map_err(|error| self.refuse_domain(&error))?,
            model_rung,
            context_policy: context_policy.clone(),
            autonomy: freeze_seat_autonomy(&team.snapshot, &role_slot)
                .map_err(|error| self.refuse_domain(&error))?,
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
        self.persist_run_observation(
            project_id,
            successor_agent_run_id,
            &outcome.observation,
            now,
        )?;
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
        let mut run = state
            .with_store(|store| store.get_agent_run(project_id, agent_run_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "no such agent run exists in this project",
                )
            })?;
        let binding = run.binding.clone().ok_or_else(|| {
            self.deny(
                ApiErrorCode::NotFound,
                "this run was never bound to a native session, so there is nothing to settle",
            )
        })?;

        // A bound open session with `no_intent` is the historical replay gap:
        // Kontor launched and bound it, but omitted the desired-state write.
        // Repair that durable half before reducing the runtime's exact binding.
        if run.terminal.is_none()
            && run.projection.desired == kontor_core::state::DesiredRunState::NoIntent
        {
            run = self.ensure_launch_intent(project_id, agent_run_id)?;
        }

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
        let (projection, payload) =
            self.persist_run_observation(project_id, agent_run_id, &observation, now)?;

        // (4) The only place an outcome comes from. It is derived from the
        // observation against the *issued* binding, and it refuses every uncertain
        // input: a broken channel, another run's session, an evidence class that
        // only acknowledges, a grade that may not evidence closure, an observation
        // older than the window, and any non-terminal state.
        let Some(outcome) =
            observation.terminal_evidence(&issued, now, state.evidence_window_seconds())
        else {
            let reconciled = state
                .with_store(|store| store.get_agent_run(project_id, agent_run_id))
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::NotFound,
                        "the run vanished during reconciliation",
                    )
                })?;
            let (team_run_closed, team_pending) =
                self.team_closure_state(project_id, &reconciled)?;
            return Ok(RuntimeSettlementDto {
                realm_id,
                agent_run_id: agent_run_id.to_string(),
                observed: observation.state.as_str().to_owned(),
                outcome: None,
                evidence_cursor: projection.last_cursor,
                applied: AppliedDto::Created,
                team_run_closed,
                team_pending,
                receipt_id: receipt.to_string(),
            });
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

    /// Install one immutable trigger revision into a project.
    ///
    /// Until this existed, `TriggerSpec` could be validated, canonicalized,
    /// hashed and stored — and reached only by a backup import. That made
    /// [`kontor_core::spec::AutoArmPolicy::BoundedAutoArm`] unreachable in
    /// practice: the rule that lets a trigger arm its own work was implemented
    /// and tested, and no supported path could declare one. Every arm was
    /// therefore a human calling `execution:arm`, which is a policy nobody chose.
    ///
    /// It is `Admin` at the route, because publishing a bounded auto-arm is
    /// granting a capability to act without a human, and that is exactly the
    /// class of decision the admin tier exists for.
    fn publish_trigger(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        request: &PublishTriggerRequest,
    ) -> Result<TriggerSpecDto, ApiError> {
        let state = self.state()?;
        let project = self.project_row(project_id)?;

        // The document is parsed from the caller's JSON by the domain type, so a
        // field this build does not know is refused here rather than stored and
        // silently ignored by a later reader.
        let spec: TriggerSpec = serde_json::from_value(request.spec.clone()).map_err(|_| {
            self.deny(
                ApiErrorCode::InvalidRequest,
                "the trigger document is not a trigger specification of this generation",
            )
        })?;
        spec.validate()
            .map_err(|error| self.refuse_domain(&error))?;

        // A published revision is immutable, so re-publishing one is either an
        // idempotent replay or an attempt to change history. The digest tells the
        // two apart: the same bytes replay, different bytes are refused.
        let canonical = spec
            .canonicalize()
            .map_err(|error| self.refuse_domain(&error))?;
        if let Some(existing) = state
            .with_store(|store| store.get_trigger_spec(project_id, &spec.id, spec.version))
            .map_err(|error| self.refuse(&error))?
        {
            let existing_hash = existing
                .canonicalize()
                .map_err(|error| self.refuse_domain(&error))?;
            if existing_hash.hash() != canonical.hash() {
                return Err(self.deny(
                    ApiErrorCode::IdempotencyConflict,
                    "this trigger revision is already installed with different bytes; \
                     a published revision is immutable, so publish a new version instead",
                ));
            }
            return Ok(trigger_dto(&existing));
        }

        // The pinned profile and team revisions are proved to exist *here*, by
        // name. Both are foreign keys, so letting the insert discover it would
        // surface a missing revision as a `revision_conflict` against "the
        // presented state" — which names neither the reference nor the fix, and
        // reads as a concurrency problem the caller might retry forever.
        if state
            .with_store(|store| {
                store.get_work_profile(project_id, &spec.work_profile, spec.work_profile_version)
            })
            .map_err(|error| self.refuse(&error))?
            .is_none()
        {
            return Err(self.deny(
                ApiErrorCode::NotFound,
                "the work profile revision this trigger pins is not installed in this project",
            ));
        }
        if state
            .with_store(|store| {
                store.get_team_template(
                    project_id,
                    spec.team_template.template_id,
                    spec.team_template.version,
                )
            })
            .map_err(|error| self.refuse(&error))?
            .is_none()
        {
            return Err(self.deny(
                ApiErrorCode::NotFound,
                "the team template revision this trigger pins is not installed in this project",
            ));
        }

        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "publish_trigger",
            "trigger": spec.id.to_string(),
            "version": spec.version.get(),
            "document_hash": canonical.hash().as_str(),
        }))?;
        let target = AggregateRef::Project { project_id };
        if self.replayed(key, &intent, Some(&target))?.is_some() {
            return Ok(trigger_dto(&spec));
        }
        self.record(
            key,
            project_id,
            CommandKind::PublishTrigger,
            target,
            project.revision,
            &intent,
        )?;
        state
            .with_store(|store| store.insert_trigger_spec(project_id, &spec))
            .map_err(|error| self.refuse(&error))?;
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
        let connector = self.canonical_connector(connector)?;
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

    fn install_connector_workflow_spec(
        &self,
        key: &IdempotencyKey,
        project_id: ProjectId,
        connector: &str,
        request: &kontor_api::applications::InstallWorkflowSpecRequest,
    ) -> Result<kontor_api::applications::InstalledWorkflowSpecDto, ApiError> {
        let state = self.state()?;
        let connector = self.canonical_connector(connector)?;
        let external_project =
            kontor_core::id::ExternalProjectKey::parse(&request.external_project)
                .map_err(|error| self.refuse_domain(&error))?;
        let issue_type = kontor_core::id::ExternalIssueTypeKey::parse(&request.issue_type)
            .map_err(|error| self.refuse_domain(&error))?;
        let matches: Vec<&CompiledWorkflowSpec> = self
            .connector_catalog()?
            .workflow_specs()
            .iter()
            .filter(|compiled| {
                let spec = compiled.spec();
                spec.connector == connector
                    && spec.project == external_project
                    && spec.issue_type == issue_type
                    && spec.version == request.version
            })
            .collect();
        let compiled = match matches.as_slice() {
            [compiled] => *compiled,
            [] => {
                return Err(self.deny(
                    ApiErrorCode::NotFound,
                    "this build ships no external-workflow specification for that exact selector",
                ));
            }
            _ => {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the shipped external-workflow selector is ambiguous",
                ));
            }
        };
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "install_workflow_spec",
            "project_id": project_id.to_string(),
            "connector": connector.as_str(),
            "external_project": external_project.as_str(),
            "issue_type": issue_type.as_str(),
            "version": request.version.get(),
            "definition_hash": compiled.hash().as_str(),
            "expected_revision": request.expected_revision.get(),
        }))?;
        let target = AggregateRef::Project { project_id };
        let replayed = self.replayed(key, &intent, Some(&target))?;
        let (receipt_id, applied, revision) = if let Some(receipt) = replayed {
            let revision = state
                .with_store(|store| store.workflow_install_result_revision(&receipt))
                .map_err(|error| self.refuse(&error))?;
            (receipt.id, Applied::Unchanged, revision)
        } else {
            let now = kontor_api::now();
            let command = ReceiptEnvelope::new(
                state.realm_id(),
                NewCommandIntent {
                    project_id,
                    receipt_id: CommandReceiptId::generate(),
                    idempotency_key: key.clone(),
                    kind: CommandKind::InstallWorkflowSpec,
                    target,
                    target_revision: request.expected_revision,
                    intent: intent.clone(),
                    payload: intent.clone(),
                    desired: None,
                    not_before: now,
                    created_at: now,
                },
            );
            let (_, revision, applied, receipt) = state
                .with_store(|store| {
                    store.install_external_workflow_spec_with_intent(
                        project_id,
                        request.expected_revision,
                        compiled.spec(),
                        &command,
                    )
                })
                .map_err(|error| self.refuse(&error))?;
            state.signals().appended();
            (receipt.id, applied, revision)
        };
        let spec = compiled.spec();
        Ok(kontor_api::applications::InstalledWorkflowSpecDto {
            spec: ConnectorSpecDto {
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
                installed: true,
            },
            receipt: MutationReceiptDto {
                realm_id: state.realm_id(),
                receipt_id: receipt_id.to_string(),
                applied: applied_dto(applied),
                revision,
                snapshot_cursor: self.cursor()?,
            },
        })
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
    /// Retire one still-bound predecessor under the Admin replacement command
    /// and persist the runtime's fresh archive readback as its cancellation.
    ///
    /// A missing process is not itself terminal evidence: it may be reloadable.
    /// The explicit replacement decision authorizes retirement, and only the
    /// runtime's readback of that exact archived native identity closes the run.
    async fn retire_predecessor_for_replacement(
        &self,
        project_id: ProjectId,
        predecessor: &kontor_core::repository::AgentRun,
        binding: &RuntimeBinding,
        unavailable: Option<&kontor_api::applications::UnavailableProviderSeatRequest>,
        now: Timestamp,
    ) -> Result<kontor_core::repository::AgentRun, ApiError> {
        let state = self.state()?;
        let held = state.sessions().get(binding.id).ok_or_else(|| {
            self.deny(
                ApiErrorCode::StaleBinding,
                "this process holds no frozen capability snapshot for the predecessor",
            )
        })?;
        let adapter = state
            .runtimes()
            .get(&binding.identity.runtime_kind)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::Unavailable,
                    "this daemon is not configured with the predecessor's runtime",
                )
            })?;
        if let Some(evidence) = unavailable {
            if evidence.runtime_binding_id != binding.id.to_string()
                || evidence.native_id != binding.identity.native_id.as_str()
            {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the provider-outage evidence names another immutable binding",
                ));
            }
            ExternalId::parse(&evidence.provider).map_err(|error| self.refuse_domain(&error))?;
            if predecessor.projection.lifecycle != kontor_core::state::RunLifecycle::Launching
                || predecessor.projection.desired
                    != kontor_core::state::DesiredRunState::RunRequested
                || predecessor.projection.observed
                    != kontor_core::state::ObservedRunState::Launching
            {
                return Err(self.deny(
                    ApiErrorCode::UnsupportedCapability,
                    "provider-unavailable retirement is limited to a seat evidenced only at launch",
                ));
            }
            if adapter.provider_available(&evidence.provider) {
                return Err(self.deny(
                    ApiErrorCode::UnsupportedCapability,
                    "the evidenced provider is currently available and the persistent seat must be reused",
                ));
            }
        }
        let issued = adapter
            .issued_binding(&held)
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let liveness = adapter
            .inspect(&kontor_runtime::request::InspectRequest {
                binding: issued.snapshot().clone(),
                requested_at: now,
            })
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let observation =
            if liveness.terminal_evidence(&issued, now, state.evidence_window_seconds())
                == Some(TerminalOutcome::Cancelled)
            {
                // A previous attempt may have archived the native seat and crashed
                // before persisting that readback. The fresh archive evidence is
                // sufficient; repeating the native effect is unnecessary.
                liveness
            } else if let Some(evidence) = unavailable {
                adapter
                    .retire_unavailable_provider(issued.snapshot(), &evidence.provider, now)
                    .await
                    .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?
            } else {
                if liveness.contact != RuntimeContact::ProcessMissing {
                    return Err(self.deny(
                        ApiErrorCode::UnsupportedCapability,
                        "the predecessor is still reachable and must be reused",
                    ));
                }
                // A closed process is normally only between turns. Give the runtime
                // one chance to prove same-seat continuity before retirement; only
                // a process it both reports missing and cannot resume is unusable.
                if adapter
                    .resume(&kontor_runtime::request::ResumeRequest {
                        binding: issued.snapshot().clone(),
                        requested_at: now,
                    })
                    .await
                    .is_ok()
                {
                    return Err(self.deny(
                        ApiErrorCode::UnsupportedCapability,
                        "the predecessor resumed in place and must be reused",
                    ));
                }
                adapter
                    .retire(issued.snapshot(), now)
                    .await
                    .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?
            };
        let Some(outcome) =
            observation.terminal_evidence(&issued, now, state.evidence_window_seconds())
        else {
            return Err(self.deny(
                ApiErrorCode::UnsupportedCapability,
                "the runtime did not evidence the retired predecessor as terminal",
            ));
        };
        if outcome != TerminalOutcome::Cancelled {
            return Err(self.deny(
                ApiErrorCode::UnsupportedCapability,
                "seat replacement requires runtime-observed cancellation",
            ));
        }

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
                        agent_run_id: predecessor.id,
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
                    expected_revision: predecessor.revision,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        let cursor = projection.last_cursor.ok_or_else(|| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the retirement observation was not reduced into the predecessor",
            )
        })?;
        self.observe_seat(
            project_id,
            self.task_for_team_run(project_id, predecessor.team_run_id)?,
            predecessor.team_run_id,
            &RoleSlotId::new(predecessor.role.clone()),
            &SeatLivenessObservation {
                attached_at: Some(observation.observed_at),
                runtime_reported: Some(observation.state),
                ..SeatLivenessObservation::default()
            },
            now,
        )?;

        let reduced = state
            .with_store(|store| store.get_agent_run(project_id, predecessor.id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the predecessor vanished while its retirement was recorded",
                )
            })?;
        state
            .with_store(|store| {
                store.close_agent_run(&kontor_core::repository::RunClosure {
                    project_id,
                    agent_run_id: predecessor.id,
                    expected_revision: reduced.revision,
                    evidence: kontor_core::state::TerminalEvidence {
                        outcome,
                        source: kontor_runtime::observation::ControlPlaneObservation::
                            terminal_evidence_source(cursor),
                        evidence_hash: payload.hash().clone(),
                        closed_at: now,
                    },
                })
            })
            .map_err(|error| self.refuse(&error))?;
        self.release(binding.id)?;
        state.signals().appended();
        state
            .with_store(|store| store.get_agent_run(project_id, predecessor.id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the predecessor vanished after its retirement was recorded",
                )
            })
    }

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
                        reopen: false,
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
        let launch_key = IdempotencyKey::parse(&format!("{}-{}", key.as_str(), admitted.task_id))
            .map_err(|error| self.refuse_domain(&error))?;
        self.seat_with_address(project_id, admitted, &launch_key, None, None)
            .await
    }

    /// Re-enter the admission path at one immutable launch address.
    ///
    /// Ordinary scheduler starts derive that address from their command key.
    /// Exact recovery supplies the stored launch key and both preserved run ids;
    /// every supplied identity is then checked again where the launch is used.
    async fn seat_with_address(
        &self,
        project_id: ProjectId,
        admitted: &AdmittedCandidate,
        launch_key: &IdempotencyKey,
        expected_team_run_id: Option<TeamRunId>,
        expected_agent_run_id: Option<AgentRunId>,
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
        let existing: Vec<TeamRunId> = state
            .with_store(|store| store.list_team_runs_for_task(project_id, admitted.task_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .filter(|(_, lifecycle)| !lifecycle.is_terminal())
            .map(|(id, _)| id)
            .collect();
        let team_run_id = if let Some(expected) = expected_team_run_id {
            if existing.as_slice() != [expected] {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "the task's live TeamRun set no longer equals the exact recovery address",
                ));
            }
            expected
        } else {
            existing
                .first()
                .copied()
                .unwrap_or_else(TeamRunId::generate)
        };
        let agent_run_id = state
            .with_store(|store| store.get_receipt_by_key(launch_key))
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
        if expected_agent_run_id.is_some_and(|expected| expected != agent_run_id) {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "the launch receipt no longer names the exact recovery AgentRun",
            ));
        }
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
        // Resolve every durable placement fact before committing a TeamRun. A
        // missing epic/task scope is an admission refusal, not a queued run that
        // can never acquire a native seat.
        let task = self.task_row(project_id, admitted.task_id)?;
        let epic_id = task.mini_project_id.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the admitted task is not scoped to an epic",
            )
        })?;
        let task_root = self.task_root(project_id, admitted.task_id)?;
        let placement = self.resolve_placement(
            project_id,
            admitted.task_id,
            team_run_id,
            &ordered,
            &task_root,
        )?;
        let scope = self.execution_scope(
            project_id,
            epic_id,
            Some(admitted.task_id),
            adapter.as_ref(),
        )?;

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
                    idempotency_key: launch_key.clone(),
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
                worktree_lease_id: admitted
                    .worktree
                    .as_ref()
                    .map(|_| kontor_core::id::ResourceLeaseId::generate()),
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
                &placement,
                admitted.task_id,
                team_run_id,
                &team_snapshot,
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
            let quota_states = state
                .with_store(|store| store.list_provider_quota_states(project_id))
                .map_err(|error| self.refuse(&error))?;
            let model_rung = freeze_seat_model_rung(
                adapter.as_ref(),
                &team_snapshot,
                &slot,
                &QuotaOutlook {
                    states: &quota_states,
                    account: admitted.account_profile_id,
                    accounts: &self.eligible_accounts(project_id)?,
                    headroom: self.headroom_policy(),
                    now,
                },
            )
            .map_err(|error| self.refuse_domain(&error))?;
            let context_policy = freeze_seat_context_policy(&adapter, &team_snapshot, &slot, now)
                .await
                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
            let autonomy = freeze_seat_autonomy(&team_snapshot, &slot)
                .map_err(|error| self.refuse_domain(&error))?;
            let outcome = adapter
                .launch(&authority.into_request(LaunchParts {
                    scope: scope.clone(),
                    display_name: self.delivery_seat_name(
                        project_id,
                        admitted.task_id,
                        &scope,
                        &team_snapshot,
                        &slot,
                    )?,
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
                    autonomy,
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
            self.persist_run_observation(project_id, agent_run_id, &outcome.observation, now)?;
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
            scope: &scope,
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
    /// that is not the bound one, or a slot held by another live team all stop
    /// here as `placement_blocked`, with nothing dispatched. A seat already held
    /// by this exact task and TeamRun is the idempotent recovery case, not a
    /// second placement.
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
        team_run_id: TeamRunId,
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
        // one across a restart. The seat this exact TeamRun already owns is not
        // a second seat, however: topology materialization may have committed it
        // before a launch acknowledgement was lost. Admission must be allowed to
        // re-enter so the adapter can recover the exactly labelled native agent
        // and attach the still-unbound AgentRun.
        let held = state
            .with_store(|store| store.list_seat_bindings(project_id, node.id))
            .map_err(|error| self.refuse(&error))?;
        for slot in declared {
            if held.iter().any(|binding| {
                &binding.role_slot_id == slot
                    && binding.is_non_terminal()
                    && (binding.task_id != Some(task_id)
                        || binding.team_run_id != Some(team_run_id))
            }) {
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
        let existing_task = state
            .with_store(|store| store.get_task_topology_node(project_id, task_id))
            .map_err(|error| self.refuse(&error))?;

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

        // A task row can predate the ECP repair above. Return it only after the
        // whole owning chain has been ensured; returning at function entry
        // would make the legacy hole permanent, including on idempotent
        // materialization replays.
        if let Some(node) = existing_task {
            return Ok(node);
        }

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

    /// Retire legacy task control rows that have no supported message route.
    ///
    /// Current Operational semantics admit TSW delivery seats only with a
    /// TeamRun; persistent LSA/TPM control seats belong to the epic Core Team.
    /// Older ticket materialization could leave an active task-bound TPM with
    /// neither a TeamRun/AgentRun session route nor a hosted native route. A
    /// materialization replay reconciles that durable row in place: it remains
    /// evidence under the same `SeatBindingId`, but no longer publishes itself
    /// active. An already-hosted task seat is left alone and remains addressable
    /// through topology messaging; this repair never invents its route.
    fn retire_unrouted_task_persistent_seats(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        task_node_id: TopologyNodeId,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let bindings = state
            .with_store(|store| store.list_seat_bindings(project_id, task_node_id))
            .map_err(|error| self.refuse(&error))?;
        let now = kontor_api::now();
        for binding in bindings.into_iter().filter(|binding| {
            binding.is_non_terminal()
                && binding.task_id == Some(task_id)
                && binding.team_run_id.is_none()
        }) {
            let hosted = state
                .with_store(|store| store.get_hosted_topology_seat(project_id, binding.id))
                .map_err(|error| self.refuse(&error))?;
            if hosted.is_some() {
                continue;
            }
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

    /// Retire every topology seat owned by one terminal TeamRun.
    ///
    /// The rows remain historical evidence; retiring them only releases the
    /// active `(node, role slot)` key so a later admitted generation receives
    /// fresh bindings instead of adopting the old team's seats.
    fn release_team_seats(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
        released_at: Timestamp,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let Some(team) = state
            .with_store(|store| store.get_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(());
        };
        if !team.lifecycle.is_terminal() {
            return Err(self.deny(
                ApiErrorCode::RevisionConflict,
                "a non-terminal TeamRun's seats cannot be released",
            ));
        }
        let Some(node) = state
            .with_store(|store| store.get_task_topology_node(project_id, team.task_id))
            .map_err(|error| self.refuse(&error))?
        else {
            return Ok(());
        };
        let bindings = state
            .with_store(|store| store.list_seat_bindings(project_id, node.id))
            .map_err(|error| self.refuse(&error))?;
        for binding in bindings
            .into_iter()
            .filter(|binding| binding.team_run_id == Some(team_run_id) && binding.is_non_terminal())
        {
            state
                .with_store(|store| {
                    store.observe_seat_binding(
                        project_id,
                        binding.id,
                        &SeatLivenessObservation {
                            released_at: Some(released_at),
                            ..SeatLivenessObservation::default()
                        },
                        released_at,
                    )
                })
                .map_err(|error| self.refuse(&error))?;
        }
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
        node: &SessionTopologyNode,
        task_id: TaskId,
        team_run_id: TeamRunId,
        team_snapshot: &TeamRunSnapshot,
        slot: &RoleSlotId,
        parent: Option<SeatBindingId>,
    ) -> Result<(), ApiError> {
        let state = self.state()?;
        let project_id = node.project_id;
        let Some(role) = self.catalog_role(team_snapshot, slot)? else {
            tracing::warn!(
                role_slot = %slot.as_str(),
                "no seeded standard role for this slot, so its seat is not recorded in the topology"
            );
            return Ok(());
        };
        let held = state
            .with_store(|store| store.list_seat_bindings(project_id, node.id))
            .map_err(|error| self.refuse(&error))?;
        if let Some(binding) = held
            .iter()
            .find(|binding| &binding.role_slot_id == slot && binding.is_non_terminal())
        {
            if binding.task_id == Some(task_id) && binding.team_run_id == Some(team_run_id) {
                return Ok(());
            }
            return Err(self.deny(
                ApiErrorCode::PlacementBlocked,
                "this role slot is held by another live task or TeamRun",
            ));
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
    fn catalog_role(
        &self,
        snapshot: &TeamRunSnapshot,
        slot: &RoleSlotId,
    ) -> Result<Option<CatalogRoleRef>, ApiError> {
        let team = kontor_teams::spec::TeamTemplateSpec::from_snapshot(snapshot)
            .map_err(|error| self.refuse_domain(&error))?;
        let logical_role = team.slot(slot).ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the frozen team snapshot does not declare this role slot",
            )
        })?;
        let Some(code) = self.domain.delivery.role_code(&logical_role.role.role) else {
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
        let epic_id = node.mini_project_id.ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the requested container is not scoped to an epic",
            )
        })?;
        let epic_scope = self.execution_scope(project_id, epic_id, None, adapter)?;
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
            let level_scope = match level.task_id {
                Some(task_id) => {
                    self.execution_scope(project_id, epic_id, Some(task_id), adapter)?
                }
                None => epic_scope.clone(),
            };
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
            let level_cwd = match projection {
                ContainerProjection::LogicalOnly => None,
                _ => {
                    if let Some(root) = bound
                        .as_ref()
                        .and_then(|binding| binding.canonical_cwd.as_ref())
                    {
                        Some(
                            WorkspaceRoot::parse(root.as_str())
                                .map_err(|error| self.refuse_domain(&error))?,
                        )
                    } else if level.task_id.is_some() {
                        Some(
                            level_scope
                                .require_task()
                                .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?
                                .worktree
                                .clone(),
                        )
                    } else if projection == ContainerProjection::NativeRoot {
                        Some(self.runtime_root(project_id, level.mini_project_id)?)
                    } else if leaf {
                        Some(cwd.clone())
                    } else {
                        Some(self.runtime_root(project_id, level.mini_project_id)?)
                    }
                }
            };
            let request = ContainerRequest {
                container_binding_id: ContainerBindingId::generate(),
                topology_node_id: level.id,
                topology: level.topology.clone(),
                scope: level_scope.clone(),
                capabilities,
                display_name: self.container_name(&spec, level, Some(&level_scope))?,
                parent: match projection {
                    ContainerProjection::NativeChild => parent.clone(),
                    ContainerProjection::NativeRoot | ContainerProjection::LogicalOnly => None,
                },
                // Every native root has an explicit, stable directory. Existing
                // roots keep the directory stored with their binding; an
                // unbound epic gets its own marker rather than the shared repo.
                cwd: level_cwd,
                bound_native_id: bound.map(|binding| binding.identity.native_id),
                epic_container: projection == ContainerProjection::NativeRoot
                    && level.mini_project_id == Some(epic_scope.epic.mini_project_id),
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

    /// The stable registration directory for a project or one of its epics.
    fn runtime_root(
        &self,
        project_id: ProjectId,
        epic_id: Option<MiniProjectId>,
    ) -> Result<WorkspaceRoot, ApiError> {
        let mut root = self.runtime_roots.join(project_id.to_string());
        root.push(epic_id.map_or_else(|| "project".to_owned(), |id| id.to_string()));
        std::fs::create_dir_all(&root).map_err(|_| {
            self.deny(
                ApiErrorCode::Unavailable,
                "the runtime registration directory could not be created",
            )
        })?;
        let root = root.to_str().ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the runtime registration directory is not valid UTF-8",
            )
        })?;
        WorkspaceRoot::parse(root).map_err(|error| self.refuse_domain(&error))
    }

    /// The display name one node's container carries, from its kind's template.
    fn container_name(
        &self,
        spec: &kontor_core::spec::ProjectSessionTopologySpec,
        node: &SessionTopologyNode,
        scope: Option<&ExecutionScope>,
    ) -> Result<ExternalName, ApiError> {
        let template = spec
            .node_kinds
            .iter()
            .find(|declared| declared.kind == node.kind)
            .map(|declared| &declared.name_template)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "a node's kind is not declared by its pinned topology revision",
                )
            })?;
        if let NativeNameTemplate::Legacy(template) = template {
            return render_legacy_container_name(template, scope)
                .map_err(|error| self.refuse_domain(&error));
        }
        let mut values = NativeNameValues::new().with_area_code(node.kind.as_str());
        if let Some(scope) = scope {
            if let Some(task) = scope.task.as_ref() {
                values = values
                    .with_jira_code(task.external_issue_key.as_str())
                    .with_kontor_backlog_code(task.short_code.as_str());
                let ai_short_name = self
                    .state()?
                    .with_store(|store| store.task_ai_short_name(node.project_id, task.task_id))
                    .map_err(|error| self.refuse(&error))?;
                if let Some(ai_short_name) = ai_short_name.as_ref() {
                    values = values.with_ai_short_name(ai_short_name);
                }
            } else {
                values = values.with_jira_code(scope.epic.external_epic_key.as_str());
                let durable = self
                    .state()?
                    .with_store(|store| {
                        store.get_epic_execution_scope(node.project_id, scope.epic.mini_project_id)
                    })
                    .map_err(|error| self.refuse(&error))?
                    .ok_or_else(|| {
                        self.deny(
                            ApiErrorCode::PlacementBlocked,
                            "the epic has no durable native-name tokens",
                        )
                    })?;
                if let Some(backlog_code) = durable.kontor_backlog_code.as_ref() {
                    values = values.with_kontor_backlog_code(backlog_code.as_str());
                }
                if let Some(ai_short_name) = durable.ai_short_name.as_ref() {
                    values = values.with_ai_short_name(ai_short_name);
                }
            }
        }
        template
            .render(&spec.name_separator, &values)
            .map_err(|error| self.refuse_domain(&error))
    }

    /// Render one delivery seat from the exact template pinned by its TSW.
    fn delivery_seat_name(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        scope: &ExecutionScope,
        team_snapshot: &TeamRunSnapshot,
        slot: &RoleSlotId,
    ) -> Result<ExternalName, ApiError> {
        let node = self
            .state()?
            .with_store(|store| store.get_task_topology_node(project_id, task_id))
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the delivery seat has no pinned task topology node",
                )
            })?;
        let team = kontor_teams::spec::TeamTemplateSpec::from_snapshot(team_snapshot)
            .map_err(|error| self.refuse_domain(&error))?;
        let logical_role = team.slot(slot).ok_or_else(|| {
            self.deny(
                ApiErrorCode::PlacementBlocked,
                "the frozen team snapshot does not declare this role slot",
            )
        })?;
        let area_code = self
            .domain
            .delivery
            .role_code(&logical_role.role.role)
            .map_or_else(
                || logical_role.role.role.as_str(),
                kontor_core::id::RoleCode::as_str,
            );
        self.seat_name_with_area_code(project_id, &node, scope, area_code)
    }

    /// Render any persistent seat from its host kind's pinned seat template.
    fn seat_name(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        scope: &ExecutionScope,
        role_code: &kontor_core::id::RoleCode,
    ) -> Result<ExternalName, ApiError> {
        self.seat_name_with_area_code(project_id, node, scope, role_code.as_str())
    }

    /// Render a persistent seat from an explicit code frozen by its owning
    /// catalog or Team template. Custom Team roles do not have an Operational
    /// catalog projection, but their published logical role key remains a
    /// durable display code; descriptions, slots and native ids never stand in
    /// for it.
    fn seat_name_with_area_code(
        &self,
        project_id: ProjectId,
        node: &SessionTopologyNode,
        scope: &ExecutionScope,
        area_code: &str,
    ) -> Result<ExternalName, ApiError> {
        let state = self.state()?;
        let spec = state
            .with_store(|store| {
                store.get_topology_spec(project_id, node.topology.spec_id, node.topology.version)
            })
            .map_err(|error| self.refuse(&error))?
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the delivery seat's pinned topology revision is unavailable",
                )
            })?;
        let template = spec
            .node_kinds
            .iter()
            .find(|declared| declared.kind == node.kind)
            .and_then(|declared| declared.seat_name_template.as_ref())
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::PlacementBlocked,
                    "the delivery seat's pinned kind has no seat-name template",
                )
            })?;
        let mut values = NativeNameValues::new().with_area_code(area_code);
        if let Some(task) = scope.task.as_ref() {
            values = values
                .with_jira_code(task.external_issue_key.as_str())
                .with_kontor_backlog_code(task.short_code.as_str());
            let ai_short_name = state
                .with_store(|store| store.task_ai_short_name(project_id, task.task_id))
                .map_err(|error| self.refuse(&error))?;
            if let Some(ai_short_name) = ai_short_name.as_ref() {
                values = values.with_ai_short_name(ai_short_name);
            }
        } else {
            values = values.with_jira_code(scope.epic.external_epic_key.as_str());
            let durable = state
                .with_store(|store| {
                    store.get_epic_execution_scope(project_id, scope.epic.mini_project_id)
                })
                .map_err(|error| self.refuse(&error))?
                .ok_or_else(|| {
                    self.deny(
                        ApiErrorCode::PlacementBlocked,
                        "the epic seat has no durable native-name tokens",
                    )
                })?;
            if let Some(backlog_code) = durable.kontor_backlog_code.as_ref() {
                values = values.with_kontor_backlog_code(backlog_code.as_str());
            }
            if let Some(ai_short_name) = durable.ai_short_name.as_ref() {
                values = values.with_ai_short_name(ai_short_name);
            }
        }
        template
            .render(&spec.name_separator, &values)
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
            scope,
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
        self.ensure_launch_intent(project_id, agent_run_id)?;

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
        let quota_states = state
            .with_store(|store| store.list_provider_quota_states(project_id))
            .map_err(|error| self.refuse(&error))?;
        let model_rung = freeze_seat_model_rung(
            adapter.as_ref(),
            &team_snapshot,
            slot,
            &QuotaOutlook {
                states: &quota_states,
                account: admitted.account_profile_id,
                accounts: &self.eligible_accounts(project_id)?,
                headroom: self.headroom_policy(),
                now,
            },
        )
        .map_err(|error| self.refuse_domain(&error))?;
        let context_policy = freeze_seat_context_policy(adapter, &team_snapshot, slot, now)
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?;
        let autonomy = freeze_seat_autonomy(&team_snapshot, slot)
            .map_err(|error| self.refuse_domain(&error))?;
        let outcome = adapter
            .launch(&authority.into_request(LaunchParts {
                scope: scope.clone(),
                display_name: self.delivery_seat_name(
                    project_id,
                    admitted.task_id,
                    scope,
                    &team_snapshot,
                    slot,
                )?,
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
                autonomy,
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
        self.persist_run_observation(project_id, agent_run_id, &outcome.observation, now)?;
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
        // A replayed transition answers from the command's durable result rather
        // than attempting the move again or substituting the task's live state.
        if let Some(receipt) = self.replayed(key, &intent, Some(&target))? {
            let result = state
                .with_store(|store| store.task_transition_result(&receipt))
                .map_err(|error| self.refuse(&error))?;
            return Ok(LifecycleOutcomeDto {
                realm_id: state.realm_id(),
                target: result.task_id.to_string(),
                state: result.state.as_str().to_owned(),
                revision: result.revision,
                receipt_id: receipt.id.to_string(),
            });
        }
        if task.revision != request.expected_revision {
            return Err(self
                .deny(
                    ApiErrorCode::RevisionConflict,
                    "the task moved since the caller read it",
                )
                .with_revision(Some(task.revision)));
        }
        if request.action == LifecycleAction::WithdrawTask {
            if !matches!(
                task.state,
                TaskState::Draft | TaskState::Todo | TaskState::Ready | TaskState::Blocked
            ) {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "only never-started draft, todo, ready or blocked work may be withdrawn",
                ));
            }
            let runs = state
                .with_store(|store| store.list_team_runs_for_task(project_id, task.id))
                .map_err(|error| self.refuse(&error))?;
            if !runs.is_empty() {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "a task with any TeamRun history is not never-started and cannot be withdrawn",
                ));
            }
            let graph = state
                .with_store(|store| store.task_dependency_graph(project_id))
                .map_err(|error| self.refuse(&error))?;
            let tasks = state
                .with_store(|store| store.list_tasks(project_id))
                .map_err(|error| self.refuse(&error))?;
            let unresolved_dependent = graph.iter().any(|(candidate, dependencies)| {
                dependencies.contains(&task.id)
                    && tasks
                        .iter()
                        .find(|row| row.id == *candidate)
                        .is_some_and(|row| !row.state.is_terminal())
            });
            if unresolved_dependent {
                return Err(self.deny(
                    ApiErrorCode::RevisionConflict,
                    "an unresolved dependent task still requires this task",
                ));
            }
        }
        // Only a resume records `resume_task`: that receipt is *consumed* as the
        // authority to leave a held state, and a block that shared the kind could
        // be cited as the permission to undo itself.
        let kind = match request.action {
            LifecycleAction::Resume | LifecycleAction::ReopenTask => CommandKind::ResumeTask,
            LifecycleAction::WithdrawTask => CommandKind::WithdrawTask,
            _ => CommandKind::TransitionTask,
        };
        let to = match request.action {
            LifecycleAction::Block => TaskState::Blocked,
            LifecycleAction::Resume => TaskState::Ready,
            LifecycleAction::CompleteTask => TaskState::Done,
            LifecycleAction::ReopenTask => TaskState::Ready,
            LifecycleAction::WithdrawTask => TaskState::Withdrawn,
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

        let receipt_id = CommandReceiptId::generate();
        let transition = TaskTransitionRequest {
            project_id,
            task_id: task.id,
            expected_revision: task.revision,
            to,
            resume_receipt: matches!(
                request.action,
                LifecycleAction::Resume | LifecycleAction::ReopenTask
            )
            .then_some(receipt_id),
            // Only `reopen_task` may pass a terminal task's immutability,
            // and it says so here rather than letting the store infer it
            // from the receipt: a plain `resume` carries the same kind of
            // receipt and must keep being refused.
            reopen: matches!(request.action, LifecycleAction::ReopenTask),
            run_outcome: None,
            produced_artifacts: artifacts.clone(),
            completed_phases: if to == TaskState::Done {
                completed.clone()
            } else {
                BTreeSet::new()
            },
            team_closure: team_closure.clone(),
            occurred_at: now,
        };
        let command = ReceiptEnvelope::new(
            state.realm_id(),
            NewCommandIntent {
                project_id,
                receipt_id,
                idempotency_key: key.clone(),
                kind,
                target,
                target_revision: task.revision,
                intent: intent.clone(),
                payload: intent.clone(),
                desired: None,
                not_before: now,
                created_at: now,
            },
        );
        let (result, receipt, _) = state
            .with_store(|store| store.transition_task_with_intent(&transition, &command))
            .map_err(|error| self.refuse(&error))?;
        state.signals().appended();
        Ok(LifecycleOutcomeDto {
            realm_id: state.realm_id(),
            target: result.task_id.to_string(),
            state: result.state.as_str().to_owned(),
            revision: result.revision,
            receipt_id: receipt.id.to_string(),
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

/// Render a pre-v47 immutable template only when it names the old closed scope
/// placeholders explicitly. Opaque legacy prose remains read-only: it cannot be
/// guessed into a native identity after the typed naming contract exists.
fn render_legacy_container_name(
    template: &ExternalName,
    scope: Option<&ExecutionScope>,
) -> kontor_core::DomainResult<ExternalName> {
    let scope = scope.ok_or_else(|| {
        kontor_core::DomainError::invalid(
            "NativeNameTemplate",
            "a legacy placeholder template needs an explicit execution scope",
        )
    })?;
    let mut rendered = template
        .as_str()
        .replace("<Jira epic>", scope.epic.external_epic_key.as_str())
        .replace("<short title>", scope.epic.short_title.as_str());
    if let Some(task) = scope.task.as_ref() {
        rendered = rendered
            .replace("<Jira issue>", task.external_issue_key.as_str())
            .replace("<short ticket code>", task.short_code.as_str());
    }
    if rendered == template.as_str() || rendered.contains(['<', '>']) {
        return Err(kontor_core::DomainError::invalid(
            "NativeNameTemplate",
            "a legacy template must use only the recognized scope placeholders",
        ));
    }
    ExternalName::parse(&rendered)
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

/// Whether a task remains part of its epic's completion contract.
///
/// All ordinary states count, including other terminal outcomes: a failed or
/// cancelled task remains work the epic declared and its evidence stays in the
/// closeout census. Withdrawal alone is the audited removal from active scope.
const fn counts_towards_completion(state: TaskState) -> bool {
    !matches!(state, TaskState::Withdrawn)
}

#[cfg(test)]
mod tests {
    use super::{
        counts_towards_completion, eligible_roots, render_legacy_container_name, slot_prompt,
    };
    use kontor_core::id::{ExternalId, ExternalName, MiniProjectId};
    use kontor_core::state::TaskState;
    use kontor_runtime::scope::{EpicScope, ExecutionScope};

    #[test]
    fn a_pre_v47_placeholder_template_renders_from_the_exact_epic_scope() {
        let template = ExternalName::parse("ESW · <Jira epic> · <short title>")
            .expect("the historical immutable template");
        let scope = ExecutionScope::for_epic(EpicScope {
            mini_project_id: MiniProjectId::generate(),
            external_epic_key: ExternalId::parse("ASMA-7675").expect("the Jira epic"),
            short_title: ExternalName::parse("QNR-P1").expect("the short title"),
        });

        assert_eq!(
            render_legacy_container_name(&template, Some(&scope))
                .expect("the explicit historical placeholders render")
                .as_str(),
            "ESW · ASMA-7675 · QNR-P1"
        );
        assert!(
            render_legacy_container_name(
                &ExternalName::parse("Epic Session Workspace").expect("legacy prose"),
                Some(&scope),
            )
            .is_err(),
            "opaque legacy prose stays read-only"
        );
    }

    #[test]
    fn withdrawal_alone_leaves_the_epic_completion_census() {
        assert!(!counts_towards_completion(TaskState::Withdrawn));
        for state in [
            TaskState::Ready,
            TaskState::Blocked,
            TaskState::Done,
            TaskState::Failed,
            TaskState::Cancelled,
        ] {
            assert!(
                counts_towards_completion(state),
                "{} remains declared completion scope",
                state.as_str()
            );
        }
    }

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
