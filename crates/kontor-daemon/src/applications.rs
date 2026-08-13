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

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use kontor_api::applications::{
    AccountProfileDto, ApplicationOperations, AppliedDto, AppliedEpicDto, AppliedLinkDto,
    AppliedTaskDto, ApplyEpicRequest, ArmRequest, AuthorizationProjectionDto, BlockedTaskDto,
    DisarmRequest, EnsureAccountProfileRequest, EnsureProjectRequest, EpicProjectionDto,
    EpicTaskProjectionDto, LifecycleAction, LifecycleOutcomeDto, LifecycleRequest, ProjectDto,
    ReadyTaskDto, RevisionRefDto, RuntimeCapabilityDto, SchedulerPlanDto, SchedulerStartDto,
    SeatProjectionDto, StartRequest, StartedSeatDto, TeamRunProjectionDto, TeamTemplateCatalogDto,
    WorkProfileCatalogDto,
};
use kontor_api::applications::{
    ConnectorSpecDto, IntakeReceiptDto, ProfileArtifactDto, ProfileHandoffDto, ProfilePackDto,
    ProfilePhaseDto, ProfileValidationDto, RegisterPackRequest, ResolveConflictRequest,
    SubmitIntakeRequest, TicketClaimDto, TicketCommentDto, TicketCommentPullDto, TicketConflictDto,
    TriggerSpecDto, WorkProfileDetailDto,
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
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CanonicalDocument, CommandReceiptId,
    ConnectorKey, ContentHash, CurrencyCode, ExecutionAuthorizationId, ExternalId, ExternalName,
    GateKey, IdempotencyKey, IntakeReceiptId, MiniProjectId, ModuleKey, Money, ProjectId,
    RoleSlotId, RuntimeKindKey, SCHEMA_VERSION, SourceEventId, SpecVersion, StatusConflictId,
    TaskId, TeamRunId, Timestamp, TriggerKey,
};
use kontor_core::realm::ReceiptEnvelope;
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CalendarRepository, CommandRepository, CredentialReference, CredentialReferenceKind,
    IntakeOutcome, IntakeRepository, NewAccountProfile, NewAgentRun, NewCommandIntent,
    NewGateEvaluation, NewSourceEvent, NewTeamRun, ProjectRepository, RealmRepository,
    RepositoryError, RunRepository, RuntimeBinding, SpecRepository, TaskTransitionRequest,
    TicketRepository, WorkflowRepository,
};
use kontor_core::spec::{
    AutoArmPolicy, CanonicalSourceEvent, ContextPolicySnapshot, EffectiveContextPolicy,
    IntakeReceipt, IntakeResult, RequestedContextPolicy, SourceIdentity, SourceProcessingState,
    TeamRunSnapshot, TriggerSpec,
};
use kontor_core::state::{GateVerdict, TaskState, TaskTeamClosure};
use kontor_core::ticket::OwnershipAction;
use kontor_integrations_asma::jira::SpecCatalog;
use kontor_profiles::pack::{
    PackAvailability, PackCategoryKey, ProfilePackSpec, ResolvedProfileBundle, parse_pack,
    resolve_profile, validate_pack,
};
use kontor_runtime::admission::{AdmissionRequest, RoleSlotKey};
use kontor_runtime::capability::{RuntimeBindingSnapshot, RuntimeCapability};
use kontor_runtime::request::LaunchParts;
use kontor_runtime::workspace::{WorkspaceBindingId, WorkspacePrepareRequest, WorkspaceRoot};
use kontor_scheduler::model::{
    AccountAdmissionEvidence, AdaptiveWindow, AdaptiveWindowConfig, AdmissionEventId,
    AdmittedCandidate, AuthorizationEvidence, CalendarAdmission, Candidate, CandidateDecision,
    CapacityConfig, CapacityUsage, ExternalWorkEvidence, ReconciliationEvidence,
    ReconciliationScope, RuntimeAdmissionEvidence, RuntimeHealth, SchedulingSnapshot, TaskOrigin,
};
use kontor_store::{
    AdmissionCommit, Applied, AuthorizationRevocation, EpicApplication, EpicTask, EpicTicketLink,
    IdempotencyBinding, ProjectEnsure, RegisteredPack, SqliteStore, StoredConflict,
};
use kontor_teams::run::{TeamClosureCertificate, TeamRunLease, TeamRunSlots};

/// The realm-scoped operation a pack registration binds its key to.
///
/// A `&'static str` and not a free string: it is half of what a key is bound to,
/// and it is also a closed `CHECK` value in the schema, so the two spellings have
/// to be the same one.
const REGISTER_PACK: &str = "register_profile_pack";

/// How many simultaneous runs a Realm allows before the planner refuses.
///
/// ponytail: one fixed capacity configuration rather than a settings file. A
/// Realm is a single-operator control plane on one machine, and nothing in the
/// journey this ticket owns varies it. The upgrade, if a deployment ever needs
/// one, is to read it out of the state root next to `runtimes.json` — not to
/// spread the numbers across handlers.
const CAPACITY: CapacityConfig = CapacityConfig {
    global_max_in_flight: 16,
    project_max_in_flight: 8,
    mission_max_in_flight: 8,
    account_max_in_flight: 4,
    provider_max_in_flight: 4,
    runtime_max_in_flight: 8,
    adaptive: AdaptiveWindowConfig {
        initial: 4,
        floor: 1,
        ceiling: 8,
        growth_step: 1,
    },
};

/// How long a scheduler-held module or worktree lease lives, in seconds.
const LEASE_SECONDS: i64 = 3_600;

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
    /// The connector specifications this build ships, parsed on first use.
    connectors: OnceLock<SpecCatalog>,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Services")
            .field("attached", &self.state.get().is_some())
            .field("pack", &self.pack.pack_id.as_str())
            .finish_non_exhaustive()
    }
}

impl Services {
    /// Compose the services around the profile pack this build ships.
    ///
    /// # Errors
    /// Returns the domain's own refusal when the bundled pack does not validate,
    /// which is a defect in the shipped data and not a runtime condition.
    pub fn new(realm_id: kontor_core::id::RealmId) -> Result<Arc<Self>, kontor_core::DomainError> {
        Ok(Arc::new(Self {
            realm_id,
            state: OnceLock::new(),
            pack: kontor_profiles::seeds::bundled_pack()?,
            connectors: OnceLock::new(),
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
        match slots.certify_team_closure(&[]) {
            Ok(certificate) => Ok(Ok(certificate)),
            Err(_) => Ok(Err(
                "a declared role slot is still live or produced no terminal run",
            )),
        }
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
        let evidence = certificate
            .into_terminal_evidence(now)
            .map_err(|error| self.refuse_domain(&error))?;
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
        if !lifecycle.is_terminal() {
            return Ok(Err(
                "the task's team run has not closed; settle its runs first",
            ));
        }
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
            usage: CapacityUsage::default(),
            capacity: CAPACITY,
            adaptive_window: AdaptiveWindow::start(CAPACITY.adaptive),
            freshness: jiff::SignedDuration::from_secs(state.evidence_window_seconds()),
        })
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
const fn applied_dto(applied: Applied) -> AppliedDto {
    match applied {
        Applied::Created => AppliedDto::Created,
        Applied::Unchanged => AppliedDto::Unchanged,
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

        let applied = state
            .with_store(|store| {
                store.apply_epic(&EpicApplication {
                    project_id,
                    name: request.name.clone(),
                    tasks: &tasks,
                    profile: &bundle.profile,
                    definition: &bundle.profile.definition,
                    team: bundle.team.as_ref(),
                    applied_at: now,
                })
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
            team_template: None,
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
        let intent = self.intent(&serde_json::json!({
            "schema_version": 1,
            "operation": "scheduler_start",
            "epic_id": epic_id.to_string(),
            "plan_hash": request.plan_hash,
        }))?;
        let target = AggregateRef::MiniProject {
            mini_project_id: epic_id,
        };
        if self.replayed(key, &intent, Some(&target))?.is_none() {
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
        // Without it a started task stays `ready` forever, which is not a
        // cosmetic problem: `ready → done` is not in the transition table, so a
        // task could be started and could never legally be completed.
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
        let team = self
            .pack
            .team(pinned.template_id, pinned.version)
            .ok_or_else(|| {
                self.deny(
                    ApiErrorCode::NotFound,
                    "the pinned team template revision is not in this build's pack",
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
        let roots = eligible_roots(team);
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
        let agent_run_id = AgentRunId::generate();
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

        let seat_exists = state
            .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|seat| seat.role == slot.clone().into_role_key());
        if let Some(seat) = seat_exists
            && let (Some(kind), Some(native)) = (seat.runtime_kind.clone(), seat.native_id.clone())
        {
            // The seat is already filled and bound. A start that replayed, or a
            // restart that reconciled, converges here rather than launching a
            // second session for the same `(team run, role slot)`.
            return Ok(vec![StartedSeatDto {
                task_id: admitted.task_id,
                team_run_id: team_run_id.to_string(),
                agent_run_id: seat.agent_run_id.to_string(),
                role_slot: seat.role.as_str().to_owned(),
                runtime_kind: kind,
                native_id: native.as_str().to_owned(),
                applied: AppliedDto::Unchanged,
            }]);
        }

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
        let launch_key = IdempotencyKey::parse(&format!("{}-{}", key.as_str(), admitted.task_id))
            .map_err(|error| self.refuse_domain(&error))?;

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
                capacity: CAPACITY,
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

        // A workspace is prepared *inside* the runtime's plane, so the plane has
        // to exist first. This is idempotent and re-attests a binding the
        // adapter already holds, so the cost of asking on every admission is one
        // readback — and the cost of not asking is a seat that can never be
        // materialized on a runtime whose plane nothing else creates.
        adapter
            .prepare_plane()
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?;
        let workspace = adapter
            .prepare_workspace(&WorkspacePrepareRequest {
                team_run_id,
                task_id: admitted.task_id,
                workspace_binding_id: WorkspaceBindingId::generate(),
                root: self.task_root(project_id, admitted.task_id)?,
                requested_at: now,
            })
            .await
            .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?
            .snapshot;
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
        let outcome = adapter
            .launch(
                &authority.into_request(LaunchParts {
                    agent_run_id,
                    team_run_id,
                    role_slot_id: slot.clone(),
                    task_id: admitted.task_id,
                    binding_id,
                    workspace: Some(workspace.clone()),
                    cwd: workspace.root().clone(),
                    account_profile_id: admitted.account_profile_id,
                    prompt: slot_prompt(&slot, &roots)
                        .map_err(|error| self.refuse_domain(&error))?,
                    context_policy: freeze_seat_context_policy(
                        &adapter,
                        &team_snapshot,
                        &slot,
                        now,
                    )
                    .await
                    .map_err(|error| ApiError::from_runtime(state.realm_id(), &error))?,
                    requested_at: now,
                }),
            )
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
        // Freeze the requested/effective pair onto the run, beside the binding.
        // The record of what a seat was launched under has to outlive this
        // process, or an audit after a restart has only the session to go on.
        state
            .with_store(|store| {
                store.record_run_context_policy(project_id, agent_run_id, &context_policy)
            })
            .map_err(|error| self.refuse(&error))?;
        // The frozen snapshot lives in this process: it is what lets the session
        // routes address the seat at the evidence quality it was bound at.
        self.hold(&outcome.snapshot)?;

        let mut filled = vec![StartedSeatDto {
            task_id: admitted.task_id,
            team_run_id: team_run_id.to_string(),
            agent_run_id: agent_run_id.to_string(),
            role_slot: slot.as_role_key().as_str().to_owned(),
            runtime_kind: binding.identity.runtime_kind.clone(),
            native_id: binding.identity.native_id.as_str().to_owned(),
            applied: AppliedDto::Created,
        }];
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
            now,
        };
        for role in ordered.iter().skip(1) {
            filled.push(self.fill_slot(&seating, role).await?);
        }
        Ok(filled)
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
            now,
        } = *seating;
        let state = self.state()?;
        let realm_id = state.realm_id();
        let existing = state
            .with_store(|store| store.list_agent_runs_for_team_run(project_id, team_run_id))
            .map_err(|error| self.refuse(&error))?
            .into_iter()
            .find(|seat| &seat.role == slot.as_role_key());
        if let Some(seat) = existing
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

        let agent_run_id = AgentRunId::generate();
        let binding_id = kontor_core::id::RuntimeBindingId::generate();
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

        // The plane is deliberately *not* prepared again here. `fill_slot` is
        // reached from `seat` and from nowhere else, and `seat` prepares it
        // immediately before the first slot — so a second call could never
        // observe a different answer, and a line no test can kill is worse than
        // no line.
        //
        // The workspace is idempotent per team run, so every seat of the team
        // lands in the one verified root rather than each preparing its own.
        let workspace = adapter
            .prepare_workspace(&WorkspacePrepareRequest {
                team_run_id,
                task_id: admitted.task_id,
                workspace_binding_id: WorkspaceBindingId::generate(),
                root: self.task_root(project_id, admitted.task_id)?,
                requested_at: now,
            })
            .await
            .map_err(|error| ApiError::from_runtime(realm_id, &error))?
            .snapshot;
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
        let outcome = adapter
            .launch(
                &authority.into_request(LaunchParts {
                    agent_run_id,
                    team_run_id,
                    role_slot_id: slot.clone(),
                    task_id: admitted.task_id,
                    binding_id,
                    workspace: Some(workspace.clone()),
                    cwd: workspace.root().clone(),
                    account_profile_id: admitted.account_profile_id,
                    prompt: slot_prompt(slot, roots).map_err(|error| self.refuse_domain(&error))?,
                    context_policy: freeze_seat_context_policy(adapter, &team_snapshot, slot, now)
                        .await
                        .map_err(|error| ApiError::from_runtime(realm_id, &error))?,
                    requested_at: now,
                }),
            )
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
