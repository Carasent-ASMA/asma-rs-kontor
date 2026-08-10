//! SQLite implementations of the [`kontor_core::repository`] ports.
//!
//! Every mutation opens exactly one transaction. Reads always carry the project
//! id in the predicate, so a valid id belonging to another project resolves to
//! `None` rather than to somebody else's row.
//!
//! The transactions are opened with [`rusqlite::Connection::unchecked_transaction`]
//! so the methods can take `&self`. That is sound here precisely because the
//! connection is private and no method ever calls another method that opens a
//! transaction: each one is a single flat unit of work.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::DomainError;
use kontor_core::calendar::{
    CalendarExceptionRevision, CalendarProfileSpec, ExceptionKind, ExceptionProvenance,
    ExecutionAuthorization, HolidayProviderKind, HolidaySourceRevision, IanaTimeZone,
    OverrideExpiry, OverrideRevocation, ScheduleOverride, WeeklyWindow, WorkCalendarAssignment,
    WorkScope,
};
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ArtifactKey, CalendarExceptionId,
    CalendarProfileId, CanonicalDocument, CommandReceiptId, ContentHash, CurrencyCode, EventCursor,
    ExternalId, ExternalName, GateKey, IdempotencyKey, IntakeReceiptId, MiniProjectId, ModuleKey,
    Money, PersonaScenarioId, PhaseKey, ProjectId, RealmId, RoleKey, RuntimeBindingId,
    RuntimeKindKey, ScheduleOverrideId, SpecVersion, StatusConflictId, TaskId, TaskWorkflowId,
    TeamRunId, TeamTemplateId, TicketLinkId, Timestamp, TriggerKey, WorkCalendarId, WorkProfileKey,
    format_utc_timestamp, parse_utc_timestamp,
};
use kontor_core::realm::{EventEnvelope, RealmCursor, ReceiptEnvelope, SnapshotEnvelope};
use kontor_core::receipt::{
    AggregateRef, CommandKind, CommandOutboxEntry, CommandReceipt, ReceiptAuthority,
};
use kontor_core::repository::{
    AccountProfile, AgentRun, CalendarRepository, CommandRepository, ConnectorSpecSelector,
    GateEvaluation, IntakeOutcome, IntakeRepository, MiniProject, NewAgentRun, NewCommandIntent,
    NewGateEvaluation, NewIntakeReevaluation, NewMiniProject, NewObservation, NewProject,
    NewRuntimeEvent, NewSourceEvent, NewTask, NewTaskPersonaSnapshot, NewTaskWorkflow, NewTeamRun,
    NewTicketLink, PhaseAdvance, Project, ProjectRepository, RealmRepository, ReceiptAdvance,
    ReevaluationOutcome, RepositoryError, RepositoryResult, RunClosure, RunRepository,
    RuntimeBinding, RuntimeEvent, SpecRepository, Task, TaskTransitionRequest, TaskWorkflow,
    TeamRun, TeamRunAdvance, TeamRunClosure, TicketLink, TicketRepository, WorkflowRepository,
    validate_dependency_graph,
};
use kontor_core::spec::{
    IntakeReceipt, PersonaScenarioSnapshot, PersonaScenarioSpec, ResolvedWorkProfileSnapshot,
    SourceIdentity, TeamRunSnapshot, TeamTemplateRevision, TriggerSpec, WorkProfileSpec,
};
use kontor_core::state::{
    AbandonReceiptFacts, DerivedRunState, DesiredRunState, GateState, GateVerdict,
    NativeRuntimeIdentity, ObservedRunState, RunLifecycle, RunProjection, TaskState,
    TaskTransition, TeamChildEvidence, TeamEvidenceSource, TeamTerminalEvidence, TerminalEvidence,
    TerminalEvidenceSource, TerminalOutcome, plan_team_advance, plan_team_closure,
};
use kontor_core::ticket::{
    ExternalCommentRevision, ExternalTicketObservation, ExternalWorkflowSpec, StatusConflict,
    StatusTransitionReceipt, TicketFieldSpec, TicketSyncProjection,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::SqliteStore;

/// Maximum length of an agent-run parent chain that is walked when checking for
/// a lineage cycle.
const MAX_PARENT_CHAIN: usize = 1_024;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn backend(error: rusqlite::Error) -> RepositoryError {
    if let rusqlite::Error::SqliteFailure(failure, _) = &error
        && failure.code == rusqlite::ErrorCode::ConstraintViolation
    {
        // Constraint text is deliberately not propagated: SQLite includes the
        // offending row's context in some messages and this crate never lets a
        // persisted value reach an error string.
        return RepositoryError::Conflict {
            subject: "storage",
            rule: "a uniqueness, check or immutability constraint refused the write",
        };
    }
    RepositoryError::Backend {
        detail: error.to_string(),
    }
}

pub(crate) fn conflict(subject: &'static str, rule: &'static str) -> RepositoryError {
    RepositoryError::Conflict { subject, rule }
}

pub(crate) fn text(timestamp: Timestamp) -> String {
    format_utc_timestamp(timestamp)
}

pub(crate) fn read_timestamp(value: &str) -> RepositoryResult<Timestamp> {
    Ok(parse_utc_timestamp(value)?)
}

pub(crate) fn to_json<T: Serialize>(value: &T) -> RepositoryResult<String> {
    serde_json::to_string(value).map_err(|_| RepositoryError::Backend {
        detail: "value could not be serialized as JSON".to_owned(),
    })
}

pub(crate) fn from_json<T: DeserializeOwned>(value: &str) -> RepositoryResult<T> {
    serde_json::from_str(value).map_err(|_| RepositoryError::Backend {
        detail: "stored JSON does not match the expected shape".to_owned(),
    })
}

pub(crate) fn revision_of(value: i64) -> RepositoryResult<AggregateRevision> {
    let unsigned = u64::try_from(value).map_err(|_| RepositoryError::Backend {
        detail: "stored revision is negative".to_owned(),
    })?;
    Ok(AggregateRevision::parse(unsigned)?)
}

pub(crate) fn revision_column(revision: AggregateRevision) -> RepositoryResult<i64> {
    i64::try_from(revision.get()).map_err(|_| RepositoryError::Backend {
        detail: "revision exceeds the storable range".to_owned(),
    })
}

fn version_column(version: SpecVersion) -> i64 {
    i64::from(version.get())
}

fn read_version(value: i64) -> RepositoryResult<SpecVersion> {
    let narrowed = u32::try_from(value).map_err(|_| RepositoryError::Backend {
        detail: "stored version is out of range".to_owned(),
    })?;
    Ok(SpecVersion::parse(narrowed)?)
}

/// Read a versioned document back, verifying its canonical bytes and digest
/// before it is trusted.
fn stored_document<T: DeserializeOwned>(json: &str, hash: &str) -> RepositoryResult<T> {
    let digest = ContentHash::parse(hash)?;
    let document = CanonicalDocument::from_stored(json, &digest)?;
    Ok(document.deserialize::<T>()?)
}

fn scope_columns(scope: WorkScope) -> (&'static str, Option<String>, Option<String>) {
    match scope {
        WorkScope::Project => ("project", None, None),
        WorkScope::MiniProject { mini_project_id } => {
            ("mini_project", Some(mini_project_id.to_string()), None)
        }
        WorkScope::Task { task_id } => ("task", None, Some(task_id.to_string())),
    }
}

fn read_scope(
    kind: &str,
    mini_project: Option<String>,
    task: Option<String>,
) -> RepositoryResult<WorkScope> {
    match kind {
        "project" => Ok(WorkScope::Project),
        "mini_project" => {
            let id = mini_project.ok_or_else(|| RepositoryError::Backend {
                detail: "scope row is missing its goal".to_owned(),
            })?;
            Ok(WorkScope::MiniProject {
                mini_project_id: MiniProjectId::parse(&id)?,
            })
        }
        "task" => {
            let id = task.ok_or_else(|| RepositoryError::Backend {
                detail: "scope row is missing its task".to_owned(),
            })?;
            Ok(WorkScope::Task {
                task_id: TaskId::parse(&id)?,
            })
        }
        _ => Err(RepositoryError::Backend {
            detail: "scope row has an unknown kind".to_owned(),
        }),
    }
}

/// Split a typed target into its discriminator and its seven mutually exclusive
/// id columns.
pub(crate) fn target_columns(target: &AggregateRef) -> (&'static str, [Option<String>; 7]) {
    let mut columns: [Option<String>; 7] = Default::default();
    let kind = match target {
        AggregateRef::Project { project_id } => {
            columns[0] = Some(project_id.to_string());
            "project"
        }
        AggregateRef::MiniProject { mini_project_id } => {
            columns[1] = Some(mini_project_id.to_string());
            "mini_project"
        }
        AggregateRef::Task { task_id } => {
            columns[2] = Some(task_id.to_string());
            "task"
        }
        AggregateRef::TeamRun { team_run_id } => {
            columns[3] = Some(team_run_id.to_string());
            "team_run"
        }
        AggregateRef::AgentRun { agent_run_id } => {
            columns[4] = Some(agent_run_id.to_string());
            "agent_run"
        }
        AggregateRef::TicketLink { link_id } => {
            columns[5] = Some(link_id.to_string());
            "ticket_link"
        }
        AggregateRef::WorkCalendar { work_calendar_id } => {
            columns[6] = Some(work_calendar_id.to_string());
            "work_calendar"
        }
    };
    (kind, columns)
}

pub(crate) fn target_project(target: &AggregateRef) -> Option<ProjectId> {
    match target {
        AggregateRef::Project { project_id } => Some(*project_id),
        _ => None,
    }
}

impl SqliteStore {
    /// Open one short transaction.
    ///
    /// `IMMEDIATE`, not the default deferred behaviour, and that matters under
    /// concurrency rather than in a single-process test. A deferred transaction
    /// takes its read snapshot first and only asks for the write lock when it
    /// reaches its first write — and in WAL mode, if anyone committed in
    /// between, SQLite refuses that upgrade with `SQLITE_BUSY` *immediately*,
    /// without consulting the busy timeout, because retrying could deadlock.
    /// Two appenders would then fail each other rather than queue.
    ///
    /// Taking the write lock up front means the second writer waits out the
    /// bounded busy timeout and proceeds. Every transaction in this store is
    /// short and none is held across a native call, so serializing them is the
    /// cheap half of the trade.
    pub(crate) fn begin(&self) -> RepositoryResult<Transaction<'_>> {
        Transaction::new_unchecked(&self.connection, rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)
    }
}

// ---------------------------------------------------------------------------
// Projects, goals and tasks
// ---------------------------------------------------------------------------

fn read_project(row: &Row<'_>) -> RepositoryResult<Project> {
    Ok(Project {
        id: ProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        name: ExternalName::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        root_path: ExternalName::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        revision: revision_of(row.get::<_, i64>(3).map_err(backend)?)?,
        created_at: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
    })
}

fn read_task(row: &Row<'_>) -> RepositoryResult<Task> {
    let mini_project: Option<String> = row.get(2).map_err(backend)?;
    let module: Option<String> = row.get(4).map_err(backend)?;
    Ok(Task {
        id: TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        mini_project_id: mini_project
            .as_deref()
            .map(MiniProjectId::parse)
            .transpose()?,
        title: ExternalName::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        module: module.as_deref().map(ModuleKey::parse).transpose()?,
        state: TaskState::parse(&row.get::<_, String>(5).map_err(backend)?)?,
        revision: revision_of(row.get::<_, i64>(6).map_err(backend)?)?,
        created_at: read_timestamp(&row.get::<_, String>(7).map_err(backend)?)?,
        updated_at: read_timestamp(&row.get::<_, String>(8).map_err(backend)?)?,
    })
}

const TASK_COLUMNS: &str =
    "id, project_id, mini_project_id, title, module_key, state, revision, created_at, updated_at";

impl ProjectRepository for SqliteStore {
    fn create_project(&self, request: &NewProject) -> RepositoryResult<Project> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO projects (id, name, root_path, revision, created_at)
                 VALUES (?1, ?2, ?3, 1, ?4)",
                params![
                    request.id.to_string(),
                    request.name.as_str(),
                    request.root_path.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(Project {
            id: request.id,
            name: request.name.clone(),
            root_path: request.root_path.clone(),
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
        })
    }

    fn get_project(&self, id: ProjectId) -> RepositoryResult<Option<Project>> {
        self.connection
            .query_row(
                "SELECT id, name, root_path, revision, created_at FROM projects WHERE id = ?1",
                params![id.to_string()],
                |row| Ok(read_project(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn create_mini_project(&self, request: &NewMiniProject) -> RepositoryResult<MiniProject> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO mini_projects (id, project_id, name, revision, created_at)
                 VALUES (?1, ?2, ?3, 1, ?4)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.name.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(MiniProject {
            id: request.id,
            project_id: request.project_id,
            name: request.name.clone(),
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
        })
    }

    fn create_task(&self, request: &NewTask) -> RepositoryResult<Task> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO tasks
                     (id, project_id, mini_project_id, title, module_key, state,
                      revision, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.mini_project_id.map(|id| id.to_string()),
                    request.title.as_str(),
                    request.module.as_ref().map(ModuleKey::as_str),
                    request.state.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(Task {
            id: request.id,
            project_id: request.project_id,
            mini_project_id: request.mini_project_id,
            title: request.title.clone(),
            module: request.module.clone(),
            state: request.state,
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
            updated_at: request.created_at,
        })
    }

    fn get_task(&self, project_id: ProjectId, id: TaskId) -> RepositoryResult<Option<Task>> {
        self.connection
            .query_row(
                &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE project_id = ?1 AND id = ?2"),
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_task(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn list_tasks(&self, project_id: ProjectId) -> RepositoryResult<Vec<Task>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {TASK_COLUMNS} FROM tasks WHERE project_id = ?1 ORDER BY created_at, id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            tasks.push(read_task(row)?);
        }
        Ok(tasks)
    }

    fn set_task_dependencies(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        depends_on: &[TaskId],
    ) -> RepositoryResult<()> {
        if depends_on.contains(&task_id) {
            return Err(DomainError::invalid(
                "task dependency",
                "a task must not depend on itself",
            )
            .into());
        }
        let transaction = self.begin()?;
        transaction
            .execute(
                "DELETE FROM task_dependencies WHERE project_id = ?1 AND task_id = ?2",
                params![project_id.to_string(), task_id.to_string()],
            )
            .map_err(backend)?;
        let now = text(Timestamp::now());
        for dependency in depends_on {
            transaction
                .execute(
                    "INSERT INTO task_dependencies
                         (project_id, task_id, depends_on_task_id, created_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        project_id.to_string(),
                        task_id.to_string(),
                        dependency.to_string(),
                        now
                    ],
                )
                .map_err(backend)?;
        }

        // Acyclicity is checked over the whole project graph inside this
        // transaction: SQLite can enforce the pair and the self-edge, but not
        // reachability.
        let mut edges: BTreeMap<TaskId, BTreeSet<TaskId>> = BTreeMap::new();
        {
            let mut statement = transaction
                .prepare(
                    "SELECT task_id, depends_on_task_id FROM task_dependencies
                     WHERE project_id = ?1",
                )
                .map_err(backend)?;
            let mut rows = statement
                .query(params![project_id.to_string()])
                .map_err(backend)?;
            while let Some(row) = rows.next().map_err(backend)? {
                let task = TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?;
                let dependency = TaskId::parse(&row.get::<_, String>(1).map_err(backend)?)?;
                edges.entry(task).or_default().insert(dependency);
            }
        }
        validate_dependency_graph(&edges)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn create_account_profile(&self, profile: &AccountProfile) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO account_profiles
                     (id, project_id, label, external_account_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    profile.id.to_string(),
                    profile.project_id.to_string(),
                    profile.label.as_str(),
                    profile.external_account_id.as_ref().map(ExternalId::as_str),
                    text(profile.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Specification revisions
// ---------------------------------------------------------------------------

impl SpecRepository for SqliteStore {
    fn insert_work_profile(
        &self,
        project_id: ProjectId,
        spec: &WorkProfileSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO work_profiles
                     (project_id, profile_key, version, definition, definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    project_id.to_string(),
                    spec.id.as_str(),
                    version_column(spec.version),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_work_profile(
        &self,
        project_id: ProjectId,
        id: &WorkProfileKey,
        version: SpecVersion,
    ) -> RepositoryResult<Option<WorkProfileSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM work_profiles
                 WHERE project_id = ?1 AND profile_key = ?2 AND version = ?3",
                params![project_id.to_string(), id.as_str(), version_column(version)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<WorkProfileSpec>(&json, &hash))
            .transpose()
    }

    fn insert_team_template(
        &self,
        project_id: ProjectId,
        revision: &TeamTemplateRevision,
    ) -> RepositoryResult<ContentHash> {
        let authority = to_json(&revision.role_authority)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO team_templates
                     (project_id, template_id, version, name, definition, definition_hash,
                      role_authority, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    project_id.to_string(),
                    revision.template_id.to_string(),
                    version_column(revision.version),
                    revision.name.as_str(),
                    revision.definition.json(),
                    revision.definition.hash().as_str(),
                    authority,
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(revision.definition.hash().clone())
    }

    fn get_team_template(
        &self,
        project_id: ProjectId,
        id: TeamTemplateId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<TeamTemplateRevision>> {
        let found: Option<(String, String, String, String)> = self
            .connection
            .query_row(
                "SELECT name, definition, definition_hash, role_authority FROM team_templates
                 WHERE project_id = ?1 AND template_id = ?2 AND version = ?3",
                params![
                    project_id.to_string(),
                    id.to_string(),
                    version_column(version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((name, definition, hash, authority)) = found else {
            return Ok(None);
        };
        let digest = ContentHash::parse(&hash)?;
        Ok(Some(TeamTemplateRevision {
            template_id: id,
            version,
            name: ExternalName::parse(&name)?,
            definition: CanonicalDocument::from_stored(&definition, &digest)?,
            role_authority: from_json(&authority)?,
        }))
    }

    fn insert_persona_scenario(
        &self,
        project_id: ProjectId,
        spec: &PersonaScenarioSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO persona_scenarios
                     (project_id, scenario_id, version, persona_key, gate_key, definition,
                      definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    project_id.to_string(),
                    spec.scenario_id.to_string(),
                    version_column(spec.version),
                    spec.persona.as_str(),
                    spec.gate_under_test.as_str(),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_persona_scenario(
        &self,
        project_id: ProjectId,
        id: PersonaScenarioId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<PersonaScenarioSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM persona_scenarios
                 WHERE project_id = ?1 AND scenario_id = ?2 AND version = ?3",
                params![
                    project_id.to_string(),
                    id.to_string(),
                    version_column(version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<PersonaScenarioSpec>(&json, &hash))
            .transpose()
    }

    fn insert_trigger_spec(
        &self,
        project_id: ProjectId,
        spec: &TriggerSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO trigger_specs
                     (project_id, trigger_key, version, source_kind, source_connection,
                      work_profile_key, work_profile_version, team_template_id,
                      team_template_version, context_template, context_version,
                      calendar_profile_id, calendar_version,
                      definition, definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    project_id.to_string(),
                    spec.id.as_str(),
                    version_column(spec.version),
                    spec.source_kind.as_str(),
                    spec.source_connection.as_str(),
                    spec.work_profile.as_str(),
                    version_column(spec.work_profile_version),
                    spec.team_template.template_id.to_string(),
                    version_column(spec.team_template.version),
                    spec.context_template.template.as_str(),
                    version_column(spec.context_template.version),
                    spec.calendar_policy
                        .as_ref()
                        .map(|policy| policy.profile_id.to_string()),
                    spec.calendar_policy
                        .as_ref()
                        .map(|policy| version_column(policy.version)),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_trigger_spec(
        &self,
        project_id: ProjectId,
        id: &TriggerKey,
        version: SpecVersion,
    ) -> RepositoryResult<Option<TriggerSpec>> {
        type TriggerRow = (
            String,
            String,
            String,
            i64,
            String,
            i64,
            String,
            i64,
            Option<String>,
            Option<i64>,
        );
        let found: Option<TriggerRow> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash, work_profile_key, work_profile_version,
                        team_template_id, team_template_version, context_template,
                        context_version, calendar_profile_id, calendar_version
                 FROM trigger_specs
                 WHERE project_id = ?1 AND trigger_key = ?2 AND version = ?3",
                params![project_id.to_string(), id.as_str(), version_column(version)],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|row| {
                let spec = stored_document::<TriggerSpec>(&row.0, &row.1)?;
                // The canonical definition is authoritative, but the normalized
                // columns are what the foreign keys act on. If the two ever
                // disagreed, the pins SQLite enforced would not be the pins the
                // domain believes in — so a disagreement is a hard read failure
                // rather than a silent preference for one side.
                let agrees = spec.work_profile.as_str() == row.2
                    && version_column(spec.work_profile_version) == row.3
                    && spec.team_template.template_id.to_string() == row.4
                    && version_column(spec.team_template.version) == row.5
                    && spec.context_template.template.as_str() == row.6
                    && version_column(spec.context_template.version) == row.7
                    && spec
                        .calendar_policy
                        .as_ref()
                        .map(|policy| policy.profile_id.to_string())
                        == row.8
                    && spec
                        .calendar_policy
                        .as_ref()
                        .map(|policy| version_column(policy.version))
                        == row.9;
                if !agrees {
                    return Err(RepositoryError::from(DomainError::invalid(
                        "TriggerSpec",
                        "the stored pins disagree with the canonical definition",
                    )));
                }
                Ok(spec)
            })
            .transpose()
    }

    fn insert_calendar_profile(&self, spec: &CalendarProfileSpec) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO calendar_profiles
                     (profile_id, version, name, definition, definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    spec.profile_id.to_string(),
                    version_column(spec.version),
                    spec.name.as_str(),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_calendar_profile(
        &self,
        id: CalendarProfileId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<CalendarProfileSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM calendar_profiles
                 WHERE profile_id = ?1 AND version = ?2",
                params![id.to_string(), version_column(version)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<CalendarProfileSpec>(&json, &hash))
            .transpose()
    }

    fn insert_ticket_field_spec(
        &self,
        project_id: ProjectId,
        spec: &TicketFieldSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO ticket_field_specs
                     (project_id, connector, external_project, issue_type, version,
                      definition, definition_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    project_id.to_string(),
                    spec.connector.as_str(),
                    spec.project.as_str(),
                    spec.issue_type.as_str(),
                    version_column(spec.version),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_ticket_field_spec(
        &self,
        selector: &ConnectorSpecSelector,
    ) -> RepositoryResult<Option<TicketFieldSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM ticket_field_specs
                 WHERE project_id = ?1 AND connector = ?2 AND external_project = ?3
                   AND issue_type = ?4 AND version = ?5",
                params![
                    selector.project_id.to_string(),
                    selector.connector.as_str(),
                    selector.project.as_str(),
                    selector.issue_type.as_str(),
                    version_column(selector.version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<TicketFieldSpec>(&json, &hash))
            .transpose()
    }

    fn insert_external_workflow_spec(
        &self,
        project_id: ProjectId,
        spec: &ExternalWorkflowSpec,
    ) -> RepositoryResult<ContentHash> {
        let document = spec.canonicalize()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO external_workflow_specs
                     (project_id, connector, external_project, issue_type, version,
                      work_profile_key, work_profile_version, definition, definition_hash,
                      created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    project_id.to_string(),
                    spec.connector.as_str(),
                    spec.project.as_str(),
                    spec.issue_type.as_str(),
                    version_column(spec.version),
                    spec.work_profile.as_ref().map(WorkProfileKey::as_str),
                    spec.work_profile_version.map(version_column),
                    document.json(),
                    document.hash().as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(document.hash().clone())
    }

    fn get_external_workflow_spec(
        &self,
        selector: &ConnectorSpecSelector,
    ) -> RepositoryResult<Option<ExternalWorkflowSpec>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT definition, definition_hash FROM external_workflow_specs
                 WHERE project_id = ?1 AND connector = ?2 AND external_project = ?3
                   AND issue_type = ?4 AND version = ?5",
                params![
                    selector.project_id.to_string(),
                    selector.connector.as_str(),
                    selector.project.as_str(),
                    selector.issue_type.as_str(),
                    version_column(selector.version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<ExternalWorkflowSpec>(&json, &hash))
            .transpose()
    }
}

// ---------------------------------------------------------------------------
// Workflows, gates and the task lifecycle
// ---------------------------------------------------------------------------

fn load_workflow(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    workflow_id: TaskWorkflowId,
) -> RepositoryResult<(TaskWorkflow, AggregateRevision)> {
    let row: Option<(String, String, String, i64, String, i64, String)> = transaction
        .query_row(
            "SELECT task_id, snapshot, snapshot_hash, active, current_phase, revision, created_at
             FROM task_workflows WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), workflow_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((task, snapshot, hash, active, phase, revision, created_at)) = row else {
        return Err(RepositoryError::NotFound {
            subject: "task workflow",
        });
    };
    let snapshot: ResolvedWorkProfileSnapshot = stored_document(&snapshot, &hash)?;
    snapshot.verify()?;
    let revision = revision_of(revision)?;
    Ok((
        TaskWorkflow {
            id: workflow_id,
            project_id,
            task_id: TaskId::parse(&task)?,
            snapshot,
            current_phase: PhaseKey::parse(&phase)?,
            active: active == 1,
            revision,
            created_at: read_timestamp(&created_at)?,
        },
        revision,
    ))
}

fn reduce_gate_states(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    workflow_id: TaskWorkflowId,
) -> RepositoryResult<BTreeMap<GateKey, GateState>> {
    let mut statement = transaction
        .prepare(
            "SELECT gate_key, verdict FROM task_gate_evaluations
             WHERE project_id = ?1 AND workflow_id = ?2 ORDER BY gate_key, sequence",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), workflow_id.to_string()])
        .map_err(backend)?;
    let mut states = BTreeMap::new();
    while let Some(row) = rows.next().map_err(backend)? {
        let gate = GateKey::parse(&row.get::<_, String>(0).map_err(backend)?)?;
        let verdict = GateVerdict::parse(&row.get::<_, String>(1).map_err(backend)?)?;
        states.insert(gate, verdict.resulting_state());
    }
    Ok(states)
}

impl WorkflowRepository for SqliteStore {
    fn create_task_workflow(&self, request: &NewTaskWorkflow) -> RepositoryResult<TaskWorkflow> {
        request.snapshot.verify()?;
        if !request
            .snapshot
            .definition
            .phases
            .iter()
            .any(|phase| phase.id == request.current_phase)
        {
            return Err(DomainError::invalid(
                "task workflow",
                "the starting phase is not declared by the pinned profile",
            )
            .into());
        }
        let document = CanonicalDocument::from_serializable(&request.snapshot)?;
        let transaction = self.begin()?;
        // The snapshot is self-consistent by construction, which says nothing
        // about whether the revision it pins was ever stored. Prove it here, in
        // the same transaction, so the failure is a domain error rather than a
        // bare foreign-key violation — and so the digest is checked too, which
        // no foreign key can do.
        let pinned: Option<String> = transaction
            .query_row(
                "SELECT definition_hash FROM work_profiles
                 WHERE project_id = ?1 AND profile_key = ?2 AND version = ?3",
                params![
                    request.project_id.to_string(),
                    request.snapshot.definition.id.as_str(),
                    version_column(request.snapshot.definition.version)
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(pinned) = pinned else {
            return Err(DomainError::MissingEvidence {
                subject: "task workflow",
                rule: "the pinned work-profile revision is not stored in this project",
            }
            .into());
        };
        if ContentHash::parse(&pinned)? != request.snapshot.definition_hash {
            return Err(DomainError::MissingEvidence {
                subject: "task workflow",
                rule: "the pinned work-profile revision has a different canonical digest",
            }
            .into());
        }
        transaction
            .execute(
                "INSERT INTO task_workflows
                     (id, project_id, task_id, profile_key, profile_version, snapshot,
                      snapshot_hash, current_phase, active, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 1, ?9)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    request.snapshot.definition.id.as_str(),
                    version_column(request.snapshot.definition.version),
                    document.json(),
                    document.hash().as_str(),
                    request.current_phase.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(TaskWorkflow {
            id: request.id,
            project_id: request.project_id,
            task_id: request.task_id,
            snapshot: request.snapshot.clone(),
            current_phase: request.current_phase.clone(),
            active: true,
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
        })
    }

    fn get_active_task_workflow(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<TaskWorkflow>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT id FROM task_workflows
                 WHERE project_id = ?1 AND task_id = ?2 AND active = 1",
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(id) = found else {
            return Ok(None);
        };
        let transaction = self.begin()?;
        let (workflow, _) = load_workflow(&transaction, project_id, TaskWorkflowId::parse(&id)?)?;
        Ok(Some(workflow))
    }

    fn advance_phase(&self, request: &PhaseAdvance) -> RepositoryResult<AggregateRevision> {
        let transaction = self.begin()?;
        let (workflow, revision) =
            load_workflow(&transaction, request.project_id, request.workflow_id)?;
        revision.expect("task workflow", request.expected_revision)?;
        let declared = workflow
            .snapshot
            .definition
            .edges
            .iter()
            .any(|edge| edge.from == workflow.current_phase && edge.to == request.next_phase);
        if !declared {
            return Err(DomainError::invalid(
                "phase advance",
                "the pinned profile declares no edge between these phases",
            )
            .into());
        }
        let next = revision.next()?;
        transaction
            .execute(
                "UPDATE task_workflows SET current_phase = ?1, revision = ?2
                 WHERE project_id = ?3 AND id = ?4 AND revision = ?5",
                params![
                    request.next_phase.as_str(),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.workflow_id.to_string(),
                    revision_column(revision)?
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(next)
    }

    fn append_gate_evaluation(&self, request: &NewGateEvaluation) -> RepositoryResult<u32> {
        let transaction = self.begin()?;
        let (workflow, _) = load_workflow(&transaction, request.project_id, request.workflow_id)?;
        let gate = workflow
            .snapshot
            .definition
            .gate(&request.gate)
            .ok_or(RepositoryError::NotFound { subject: "gate" })?;

        let authorized = if request.verdict == GateVerdict::Waived {
            gate.waiver_allowed && gate.waiver_roles.contains(&request.evaluator_role)
        } else {
            gate.evaluator_roles.contains(&request.evaluator_role)
        };
        if !authorized {
            return Err(DomainError::MissingAuthority {
                subject: "gate evaluation",
                rule: "the acting role is not an authority for this gate",
            }
            .into());
        }
        if request.verdict.requires_evidence() {
            if request.evidence.is_empty() {
                return Err(DomainError::MissingEvidence {
                    subject: "gate evaluation",
                    rule: "passing or waiving a gate requires evidence",
                }
                .into());
            }
            let provided: BTreeSet<&ArtifactKey> = request.evidence.iter().collect();
            if !gate
                .required_evidence
                .iter()
                .all(|required| provided.contains(required))
            {
                return Err(DomainError::MissingEvidence {
                    subject: "gate evaluation",
                    rule: "the evidence required by the pinned profile is incomplete",
                }
                .into());
            }
        }

        let previous: Option<i64> = transaction
            .query_row(
                "SELECT MAX(sequence) FROM task_gate_evaluations
                 WHERE project_id = ?1 AND workflow_id = ?2 AND gate_key = ?3",
                params![
                    request.project_id.to_string(),
                    request.workflow_id.to_string(),
                    request.gate.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?
            .flatten();
        let sequence = previous.unwrap_or(0) + 1;
        let evidence = to_json(&request.evidence)?;
        transaction
            .execute(
                "INSERT INTO task_gate_evaluations
                     (project_id, workflow_id, gate_key, sequence, verdict, evaluator_role,
                      evaluator_account, evidence, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    request.project_id.to_string(),
                    request.workflow_id.to_string(),
                    request.gate.as_str(),
                    sequence,
                    request.verdict.as_str(),
                    request.evaluator_role.as_str(),
                    request.evaluator_account.to_string(),
                    evidence,
                    text(request.recorded_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        u32::try_from(sequence).map_err(|_| RepositoryError::Backend {
            detail: "gate evaluation sequence exceeded its range".to_owned(),
        })
    }

    fn gate_states(
        &self,
        project_id: ProjectId,
        workflow_id: TaskWorkflowId,
    ) -> RepositoryResult<BTreeMap<GateKey, GateState>> {
        let transaction = self.begin()?;
        reduce_gate_states(&transaction, project_id, workflow_id)
    }

    fn list_gate_evaluations(
        &self,
        project_id: ProjectId,
        workflow_id: TaskWorkflowId,
    ) -> RepositoryResult<Vec<GateEvaluation>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT gate_key, sequence, verdict, evaluator_role, evaluator_account,
                        evidence, recorded_at
                 FROM task_gate_evaluations
                 WHERE project_id = ?1 AND workflow_id = ?2
                 ORDER BY recorded_at, gate_key, sequence",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), workflow_id.to_string()])
            .map_err(backend)?;
        let mut evaluations = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let sequence: i64 = row.get(1).map_err(backend)?;
            evaluations.push(GateEvaluation {
                project_id,
                workflow_id,
                gate: GateKey::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                sequence: u32::try_from(sequence).unwrap_or(u32::MAX),
                verdict: GateVerdict::parse(&row.get::<_, String>(2).map_err(backend)?)?,
                evaluator_role: RoleKey::parse(&row.get::<_, String>(3).map_err(backend)?)?,
                evaluator_account: AccountProfileId::parse(
                    &row.get::<_, String>(4).map_err(backend)?,
                )?,
                evidence: from_json(&row.get::<_, String>(5).map_err(backend)?)?,
                recorded_at: read_timestamp(&row.get::<_, String>(6).map_err(backend)?)?,
            });
        }
        Ok(evaluations)
    }

    fn create_task_persona_snapshot(
        &self,
        request: &NewTaskPersonaSnapshot,
    ) -> RepositoryResult<PersonaScenarioSnapshot> {
        let transaction = self.begin()?;

        // The workflow must be this task's, in this project: that is what makes
        // the pinned profile the right place to resolve authority from.
        let (workflow, _) = load_workflow(&transaction, request.project_id, request.workflow_id)?;
        if workflow.task_id != request.task_id {
            return Err(RepositoryError::CrossProject {
                subject: "persona workflow",
            });
        }

        let stored: Option<(String, String)> = transaction
            .query_row(
                "SELECT definition, definition_hash FROM persona_scenarios
                 WHERE project_id = ?1 AND scenario_id = ?2 AND version = ?3",
                params![
                    request.project_id.to_string(),
                    request.scenario_id.to_string(),
                    version_column(request.version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((json, hash)) = stored else {
            return Err(RepositoryError::NotFound {
                subject: "persona scenario revision",
            });
        };
        let spec: PersonaScenarioSpec = stored_document(&json, &hash)?;

        // Authority is proved against the gate the pinned profile declares, not
        // against anything the scenario asserts about itself.
        let snapshot = PersonaScenarioSnapshot::freeze_onto_task(&spec, &workflow.snapshot)?;
        let document = CanonicalDocument::from_serializable(&snapshot)?;

        transaction
            .execute(
                "INSERT INTO task_persona_snapshots
                     (project_id, task_id, scenario_id, version, workflow_id, gate_key,
                      snapshot, snapshot_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    request.scenario_id.to_string(),
                    version_column(request.version),
                    request.workflow_id.to_string(),
                    spec.gate_under_test.as_str(),
                    document.json(),
                    document.hash().as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(snapshot)
    }

    fn get_task_persona_snapshot(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        scenario_id: PersonaScenarioId,
        version: SpecVersion,
    ) -> RepositoryResult<Option<PersonaScenarioSnapshot>> {
        let found: Option<(String, String)> = self
            .connection
            .query_row(
                "SELECT snapshot, snapshot_hash FROM task_persona_snapshots
                 WHERE project_id = ?1 AND task_id = ?2 AND scenario_id = ?3 AND version = ?4",
                params![
                    project_id.to_string(),
                    task_id.to_string(),
                    scenario_id.to_string(),
                    version_column(version)
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|(json, hash)| stored_document::<PersonaScenarioSnapshot>(&json, &hash))
            .transpose()
    }

    fn transition_task(&self, request: &TaskTransitionRequest) -> RepositoryResult<Task> {
        let transaction = self.begin()?;
        let row: Option<(String, i64)> = transaction
            .query_row(
                "SELECT state, revision FROM tasks WHERE project_id = ?1 AND id = ?2",
                params![request.project_id.to_string(), request.task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((state, revision)) = row else {
            return Err(RepositoryError::NotFound { subject: "task" });
        };
        let current = TaskState::parse(&state)?;
        let revision = revision_of(revision)?;
        revision.expect("task", request.expected_revision)?;

        // Closure is certified by the pinned profile, never asserted by the
        // caller: `TaskClosureCertificate` has no public constructor.
        let certificate = if request.to == TaskState::Done {
            let workflow_id: Option<String> = transaction
                .query_row(
                    "SELECT id FROM task_workflows
                     WHERE project_id = ?1 AND task_id = ?2 AND active = 1",
                    params![request.project_id.to_string(), request.task_id.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            let workflow_id = workflow_id.ok_or(DomainError::MissingEvidence {
                subject: "task completion",
                rule: "a task without an active workflow has no closure evidence",
            })?;
            let workflow_id = TaskWorkflowId::parse(&workflow_id)?;
            let (workflow, _) = load_workflow(&transaction, request.project_id, workflow_id)?;
            let states = reduce_gate_states(&transaction, request.project_id, workflow_id)?;
            Some(workflow.snapshot.certify_closure(
                &request.completed_phases,
                &states,
                &request.produced_artifacts,
            )?)
        } else {
            None
        };

        let transition = TaskTransition {
            to: request.to,
            resume_receipt: request.resume_receipt,
            run_outcome: request.run_outcome,
            closure: certificate.as_ref(),
        };
        let next_state = kontor_core::state::apply_task_transition(current, &transition)?;
        let next_revision = revision.next()?;
        let changed = transaction
            .execute(
                "UPDATE tasks SET state = ?1, revision = ?2, updated_at = ?3
                 WHERE project_id = ?4 AND id = ?5 AND revision = ?6",
                params![
                    next_state.as_str(),
                    revision_column(next_revision)?,
                    text(request.occurred_at),
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    revision_column(revision)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict("task", "the task revision moved during the write"));
        }
        transaction.commit().map_err(backend)?;
        self.get_task(request.project_id, request.task_id)?
            .ok_or(RepositoryError::NotFound { subject: "task" })
    }
}

// ---------------------------------------------------------------------------
// Runs, runtime events and closure
// ---------------------------------------------------------------------------

fn read_binding(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    agent_run_id: AgentRunId,
) -> RepositoryResult<Option<RuntimeBinding>> {
    let row: Option<(String, String, String, i64, String, String)> = transaction
        .query_row(
            "SELECT id, runtime_kind, host, generation, native_id, bound_at
             FROM runtime_bindings WHERE project_id = ?1 AND agent_run_id = ?2",
            params![project_id.to_string(), agent_run_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((id, kind, host, generation, native, bound_at)) = row else {
        return Ok(None);
    };
    Ok(Some(RuntimeBinding {
        id: RuntimeBindingId::parse(&id)?,
        agent_run_id,
        identity: NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse(&kind)?,
            host: ExternalName::parse(&host)?,
            generation: u64::try_from(generation).unwrap_or_default(),
            native_id: ExternalId::parse(&native)?,
        },
        bound_at: read_timestamp(&bound_at)?,
    }))
}

const AGENT_RUN_COLUMNS: &str = "team_run_id, parent_agent_run_id, role_key, account_profile_id, \
     lifecycle, desired_state, observed_state, derived_state, last_confirmed_at, last_cursor, \
     terminal_outcome, terminal_source_kind, terminal_event_cursor, terminal_receipt_id, \
     terminal_evidence_hash, closed_at, revision, created_at";
const AGENT_RUN_COLUMN_COUNT: usize = 18;

#[allow(clippy::too_many_lines)]
pub(crate) fn read_agent_run(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: AgentRunId,
) -> RepositoryResult<Option<AgentRun>> {
    let row: Option<Vec<rusqlite::types::Value>> = transaction
        .query_row(
            &format!(
                "SELECT {AGENT_RUN_COLUMNS} FROM agent_runs WHERE project_id = ?1 AND id = ?2"
            ),
            params![project_id.to_string(), id.to_string()],
            |row| {
                let mut values = Vec::new();
                for index in 0..AGENT_RUN_COLUMN_COUNT {
                    values.push(row.get::<_, rusqlite::types::Value>(index)?);
                }
                Ok(values)
            },
        )
        .optional()
        .map_err(backend)?;
    let Some(values) = row else {
        return Ok(None);
    };

    let as_text = |index: usize| -> RepositoryResult<String> {
        match &values[index] {
            rusqlite::types::Value::Text(text) => Ok(text.clone()),
            _ => Err(RepositoryError::Backend {
                detail: "agent run column is not text".to_owned(),
            }),
        }
    };
    let as_optional_text = |index: usize| -> Option<String> {
        match &values[index] {
            rusqlite::types::Value::Text(text) => Some(text.clone()),
            _ => None,
        }
    };
    let as_integer = |index: usize| -> Option<i64> {
        match &values[index] {
            rusqlite::types::Value::Integer(value) => Some(*value),
            _ => None,
        }
    };

    // Terminal evidence is rebuilt from its normalized, FK-bound columns rather
    // than from a blob, so what the run claims and what the database can prove
    // are the same thing.
    let terminal_evidence: Option<TerminalEvidence> = match as_optional_text(11) {
        None => None,
        Some(kind) => {
            let outcome = TerminalOutcome::parse(&as_text(10)?)?;
            let evidence_hash = ContentHash::parse(&as_text(14)?)?;
            let source = match kind.as_str() {
                "runtime_observation" => TerminalEvidenceSource::RuntimeObservation {
                    cursor: EventCursor::parse(as_integer(12).unwrap_or_default())?,
                },
                "operator_abandon" => TerminalEvidenceSource::OperatorAbandon {
                    receipt_id: CommandReceiptId::parse(&as_text(13)?)?,
                },
                _ => {
                    return Err(RepositoryError::Backend {
                        detail: "agent run has an unknown terminal evidence source".to_owned(),
                    });
                }
            };
            Some(TerminalEvidence {
                outcome,
                source,
                evidence_hash,
                closed_at: read_timestamp(&as_text(15)?)?,
            })
        }
    };
    let derived = match as_text(7)?.as_str() {
        "terminal" => {
            let evidence = terminal_evidence.as_ref().ok_or(RepositoryError::Backend {
                detail: "a terminal run is stored without evidence".to_owned(),
            })?;
            DerivedRunState::Terminal {
                outcome: evidence.outcome,
            }
        }
        "pending_confirmation" => DerivedRunState::PendingConfirmation,
        "confirmed" => DerivedRunState::Confirmed,
        "stale" => DerivedRunState::Stale,
        "diverged" => DerivedRunState::Diverged,
        "runtime_unavailable" => DerivedRunState::RuntimeUnavailable,
        "orphaned" => DerivedRunState::Orphaned,
        "lost_contact" => DerivedRunState::LostContact,
        _ => {
            return Err(RepositoryError::Backend {
                detail: "agent run has an unknown derived state".to_owned(),
            });
        }
    };

    let last_confirmed_at = as_optional_text(8)
        .as_deref()
        .map(read_timestamp)
        .transpose()?;
    let last_cursor = as_integer(9).map(EventCursor::parse).transpose()?;
    let closed_at = as_optional_text(15)
        .as_deref()
        .map(read_timestamp)
        .transpose()?;

    Ok(Some(AgentRun {
        id,
        project_id,
        team_run_id: TeamRunId::parse(&as_text(0)?)?,
        parent_agent_run_id: as_optional_text(1)
            .as_deref()
            .map(AgentRunId::parse)
            .transpose()?,
        role: RoleKey::parse(&as_text(2)?)?,
        account_profile_id: as_optional_text(3)
            .as_deref()
            .map(AccountProfileId::parse)
            .transpose()?,
        binding: read_binding(transaction, project_id, id)?,
        projection: RunProjection {
            lifecycle: RunLifecycle::parse(&as_text(4)?)?,
            desired: DesiredRunState::parse(&as_text(5)?)?,
            observed: ObservedRunState::parse(&as_text(6)?)?,
            derived,
            last_confirmed_at,
            last_cursor,
        },
        terminal: terminal_evidence,
        revision: revision_of(as_integer(16).unwrap_or_default())?,
        created_at: read_timestamp(&as_text(17)?)?,
        closed_at,
    }))
}

pub(crate) fn generation_column(generation: u64) -> RepositoryResult<i64> {
    i64::try_from(generation).map_err(|_| RepositoryError::Backend {
        detail: "runtime generation exceeds the storable range".to_owned(),
    })
}

impl RunRepository for SqliteStore {
    fn create_team_run(&self, request: &NewTeamRun) -> RepositoryResult<TeamRun> {
        let document = CanonicalDocument::from_serializable(&request.snapshot)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO team_runs
                     (id, project_id, task_id, template_id, template_version, snapshot,
                      snapshot_hash, lifecycle, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', 1, ?8)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    request.snapshot.template_id.to_string(),
                    version_column(request.snapshot.template_version),
                    document.json(),
                    document.hash().as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(TeamRun {
            id: request.id,
            project_id: request.project_id,
            task_id: request.task_id,
            snapshot: request.snapshot.clone(),
            lifecycle: RunLifecycle::Queued,
            terminal: None,
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
            closed_at: None,
        })
    }

    fn advance_team_run(&self, request: &TeamRunAdvance) -> RepositoryResult<AggregateRevision> {
        if request.to.is_terminal() {
            return Err(DomainError::invalid(
                "team run advance",
                "a terminal value is reached through evidence-bearing closure, not an advance",
            )
            .into());
        }
        let transaction = self.begin()?;
        let row: Option<(String, i64)> = transaction
            .query_row(
                "SELECT lifecycle, revision FROM team_runs WHERE project_id = ?1 AND id = ?2",
                params![
                    request.project_id.to_string(),
                    request.team_run_id.to_string()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((lifecycle, revision)) = row else {
            return Err(RepositoryError::NotFound {
                subject: "team run",
            });
        };
        let lifecycle = RunLifecycle::parse(&lifecycle)?;
        let revision = revision_of(revision)?;
        // Terminality, the CAS and the legal transition table are all decided by
        // the domain, so the rule this store enforces is the same one the core
        // unit tests exercise.
        let next = plan_team_advance(lifecycle, revision, request.expected_revision, request.to)?;
        let changed = transaction
            .execute(
                "UPDATE team_runs SET lifecycle = ?1, revision = ?2
                 WHERE project_id = ?3 AND id = ?4 AND revision = ?5",
                params![
                    request.to.as_str(),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.team_run_id.to_string(),
                    revision_column(revision)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "team run",
                "the run revision moved during the write",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(next)
    }

    fn close_team_run(&self, request: &TeamRunClosure) -> RepositoryResult<()> {
        request.evidence.validate()?;
        let transaction = self.begin()?;
        let row: Option<(String, i64)> = transaction
            .query_row(
                "SELECT lifecycle, revision FROM team_runs WHERE project_id = ?1 AND id = ?2",
                params![
                    request.project_id.to_string(),
                    request.team_run_id.to_string()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((lifecycle, revision)) = row else {
            return Err(RepositoryError::NotFound {
                subject: "team run",
            });
        };
        let lifecycle = RunLifecycle::parse(&lifecycle)?;
        let revision = revision_of(revision)?;

        // Both kinds of evidence are *loaded* here and decided by the domain.
        // The child digest in particular is recomputed from the persisted child
        // rows: recomputing only the outcome would still let a caller store an
        // arbitrary `evidence_hash`, leaving the stored evidence bound to
        // nothing.
        let children =
            read_team_child_evidence(&transaction, request.project_id, request.team_run_id)?;
        let receipt_column = match request.evidence.source {
            TeamEvidenceSource::ChildEvidence { .. } => None,
            TeamEvidenceSource::OperatorAbandon { receipt_id } => Some(receipt_id),
        };
        let receipt = receipt_column
            .map(|receipt_id| {
                read_abandon_receipt(
                    &transaction,
                    request.project_id,
                    receipt_id,
                    &AggregateRef::TeamRun {
                        team_run_id: request.team_run_id,
                    },
                )
            })
            .transpose()?;
        let next = plan_team_closure(
            lifecycle,
            revision,
            request.expected_revision,
            request.team_run_id,
            &request.evidence,
            &children,
            receipt.as_ref(),
        )?;
        let receipt_column = receipt_column.map(|id| id.to_string());

        let changed = transaction
            .execute(
                "UPDATE team_runs
                 SET lifecycle = ?1, terminal_outcome = ?2, terminal_source_kind = ?3,
                     terminal_receipt_id = ?4, terminal_evidence_hash = ?5, closed_at = ?6,
                     revision = ?7
                 WHERE project_id = ?8 AND id = ?9 AND revision = ?10",
                params![
                    request.evidence.outcome.lifecycle().as_str(),
                    request.evidence.outcome.as_str(),
                    match request.evidence.source {
                        TeamEvidenceSource::ChildEvidence { .. } => "child_evidence",
                        TeamEvidenceSource::OperatorAbandon { .. } => "operator_abandon",
                    },
                    receipt_column,
                    request.evidence.evidence_hash.as_str(),
                    text(request.evidence.closed_at),
                    revision_column(next)?,
                    request.project_id.to_string(),
                    request.team_run_id.to_string(),
                    revision_column(revision)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "team run",
                "the run revision moved during the write",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn create_agent_run(&self, request: &NewAgentRun) -> RepositoryResult<AgentRun> {
        let transaction = self.begin()?;

        // Lineage must be a chain, not a ring: walk it before inserting.
        let mut ancestor = request.parent_agent_run_id;
        let mut walked = 0usize;
        while let Some(current) = ancestor {
            if current == request.id {
                return Err(DomainError::invalid(
                    "agent run lineage",
                    "the parent chain would form a cycle",
                )
                .into());
            }
            walked += 1;
            if walked > MAX_PARENT_CHAIN {
                return Err(DomainError::invalid(
                    "agent run lineage",
                    "the parent chain is longer than the bound allows",
                )
                .into());
            }
            let parent: Option<Option<String>> = transaction
                .query_row(
                    "SELECT parent_agent_run_id FROM agent_runs
                     WHERE project_id = ?1 AND id = ?2",
                    params![request.project_id.to_string(), current.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            let Some(parent) = parent else {
                return Err(RepositoryError::CrossProject {
                    subject: "parent agent run",
                });
            };
            ancestor = parent.as_deref().map(AgentRunId::parse).transpose()?;
        }

        transaction
            .execute(
                "INSERT INTO agent_runs
                     (id, project_id, team_run_id, parent_agent_run_id, role_key,
                      account_profile_id, lifecycle, desired_state, observed_state,
                      derived_state, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', 'no_intent', 'unknown',
                         'pending_confirmation', 1, ?7)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.team_run_id.to_string(),
                    request.parent_agent_run_id.map(|id| id.to_string()),
                    request.role.as_str(),
                    request.account_profile_id.map(|id| id.to_string()),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;

        if let Some(binding) = &request.binding {
            transaction
                .execute(
                    "INSERT INTO runtime_bindings
                         (id, project_id, agent_run_id, runtime_kind, host, generation,
                          native_id, bound_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        binding.id.to_string(),
                        request.project_id.to_string(),
                        request.id.to_string(),
                        binding.identity.runtime_kind.as_str(),
                        binding.identity.host.as_str(),
                        generation_column(binding.identity.generation)?,
                        binding.identity.native_id.as_str(),
                        text(binding.bound_at)
                    ],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;

        let transaction = self.begin()?;
        read_agent_run(&transaction, request.project_id, request.id)?.ok_or(
            RepositoryError::NotFound {
                subject: "agent run",
            },
        )
    }

    fn get_team_run(
        &self,
        project_id: ProjectId,
        id: TeamRunId,
    ) -> RepositoryResult<Option<TeamRun>> {
        let row: Option<RepositoryResult<TeamRun>> = self
            .connection
            .query_row(
                "SELECT task_id, snapshot, snapshot_hash, lifecycle, created_at,
                        terminal_outcome, revision, closed_at, terminal_source_kind,
                        terminal_receipt_id, terminal_evidence_hash
                 FROM team_runs WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| {
                    let build = || -> RepositoryResult<TeamRun> {
                        let outcome: Option<String> = row.get(5).map_err(backend)?;
                        let closed_at: Option<String> = row.get(7).map_err(backend)?;
                        let source_kind: Option<String> = row.get(8).map_err(backend)?;
                        let receipt: Option<String> = row.get(9).map_err(backend)?;
                        let evidence_hash: Option<String> = row.get(10).map_err(backend)?;
                        Ok(TeamRun {
                            id,
                            project_id,
                            task_id: TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                            snapshot: team_run_snapshot(
                                &row.get::<_, String>(1).map_err(backend)?,
                                &row.get::<_, String>(2).map_err(backend)?,
                            )?,
                            lifecycle: RunLifecycle::parse(
                                &row.get::<_, String>(3).map_err(backend)?,
                            )?,
                            // Rebuilt from normalized, FK-bound columns, exactly
                            // like an agent run's.
                            terminal: match (source_kind, outcome, evidence_hash) {
                                (Some(kind), Some(outcome), Some(hash)) => {
                                    Some(TeamTerminalEvidence {
                                        outcome: TerminalOutcome::parse(&outcome)?,
                                        source: match kind.as_str() {
                                            "operator_abandon" => {
                                                TeamEvidenceSource::OperatorAbandon {
                                                    receipt_id: CommandReceiptId::parse(
                                                        receipt.as_deref().unwrap_or_default(),
                                                    )?,
                                                }
                                            }
                                            _ => TeamEvidenceSource::ChildEvidence {
                                                team_run_id: id,
                                            },
                                        },
                                        evidence_hash: ContentHash::parse(&hash)?,
                                        closed_at: read_timestamp(
                                            closed_at.as_deref().unwrap_or_default(),
                                        )?,
                                    })
                                }
                                _ => None,
                            },
                            revision: revision_of(row.get::<_, i64>(6).map_err(backend)?)?,
                            created_at: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
                            closed_at: closed_at.as_deref().map(read_timestamp).transpose()?,
                        })
                    };
                    Ok(build())
                },
            )
            .optional()
            .map_err(backend)?;
        row.transpose()
    }

    fn get_agent_run(
        &self,
        project_id: ProjectId,
        id: AgentRunId,
    ) -> RepositoryResult<Option<AgentRun>> {
        let transaction = self.begin()?;
        read_agent_run(&transaction, project_id, id)
    }

    fn append_runtime_event(&self, request: &NewRuntimeEvent) -> RepositoryResult<EventCursor> {
        crate::events::append::append_runtime_event(self, request)
    }

    fn record_observation(&self, request: &NewObservation) -> RepositoryResult<RunProjection> {
        crate::events::append::record_observation(self, request)
    }

    fn close_agent_run(&self, request: &RunClosure) -> RepositoryResult<()> {
        request.evidence.validate()?;
        let transaction = self.begin()?;
        let run = read_agent_run(&transaction, request.project_id, request.agent_run_id)?.ok_or(
            RepositoryError::NotFound {
                subject: "agent run",
            },
        )?;
        run.projection.ensure_open("agent run")?;
        run.revision
            .expect("agent run", request.expected_revision)?;

        // The cited evidence is loaded and re-proved here, inside the closing
        // transaction. A caller cannot close a run with a plausible-looking
        // blob, with another run's event, or with another project's receipt.
        let (source_kind, cursor_column, receipt_column) = match request.evidence.source {
            TerminalEvidenceSource::RuntimeObservation { cursor } => {
                type EvidenceRow = (
                    String,
                    String,
                    String,
                    String,
                    i64,
                    String,
                    String,
                    Option<String>,
                    i64,
                );
                let found: Option<EvidenceRow> = transaction
                    .query_row(
                        "SELECT agent_run_id, runtime_kind, host, native_id, generation,
                                    payload_hash, observed_at, observed_state, native_sequence
                             FROM runtime_events
                             WHERE project_id = ?1 AND cursor = ?2
                               AND event_kind = 'runtime_observation'",
                        params![request.project_id.to_string(), cursor.get()],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                                row.get(5)?,
                                row.get(6)?,
                                row.get(7)?,
                                row.get(8)?,
                            ))
                        },
                    )
                    .optional()
                    .map_err(backend)?;
                let Some((
                    event_run,
                    kind,
                    host,
                    native,
                    generation,
                    payload_hash,
                    observed_at,
                    observed_state,
                    native_sequence,
                )) = found
                else {
                    return Err(RepositoryError::NotFound {
                        subject: "terminal evidence event",
                    });
                };

                // 1. the event belongs to this run, in this project
                if AgentRunId::parse(&event_run)? != request.agent_run_id {
                    return Err(DomainError::MissingEvidence {
                        subject: "run closure",
                        rule: "the cited event belongs to a different run",
                    }
                    .into());
                }
                // 2. it was emitted by the run's immutable binding
                let binding = run.binding.as_ref().ok_or(DomainError::MissingEvidence {
                    subject: "run closure",
                    rule: "a runtime closure requires the run to be bound",
                })?;
                let identity = NativeRuntimeIdentity {
                    runtime_kind: RuntimeKindKey::parse(&kind)?,
                    host: ExternalName::parse(&host)?,
                    generation: u64::try_from(generation).unwrap_or_default(),
                    native_id: ExternalId::parse(&native)?,
                };
                if !binding.identity.same_session(&identity) {
                    return Err(DomainError::MissingEvidence {
                        subject: "run closure",
                        rule: "the cited event was not emitted by this run's binding",
                    }
                    .into());
                }
                let observed = ObservedRunState::parse(&observed_state.ok_or(
                    DomainError::MissingEvidence {
                        subject: "run closure",
                        rule: "the cited event records no observed state",
                    },
                )?)?;
                // 3. the event must be the one the projection actually
                //    reduced. A late, older terminal event is still appended as
                //    evidence, but the monotonic guard refused to reduce it —
                //    so it never became this run's observed truth and must not
                //    be able to close it either. Anything newer has not been
                //    reduced yet.
                let reduced: Option<i64> = transaction
                    .query_row(
                        "SELECT last_native_sequence FROM agent_runs
                         WHERE project_id = ?1 AND id = ?2",
                        params![
                            request.project_id.to_string(),
                            request.agent_run_id.to_string()
                        ],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(backend)?
                    .flatten();
                if reduced != Some(native_sequence) {
                    return Err(DomainError::MissingEvidence {
                        subject: "run closure",
                        rule: "the cited event never reduced this run's projection",
                    }
                    .into());
                }
                // 4-6. digest, terminal outcome and closure ordering
                request.evidence.verify_observation(
                    observed,
                    read_timestamp(&observed_at)?,
                    &ContentHash::parse(&payload_hash)?,
                )?;
                ("runtime_observation", Some(cursor.get()), None)
            }
            TerminalEvidenceSource::OperatorAbandon { receipt_id } => {
                // The receipt must target this exact run *and* the exact
                // revision being closed, not merely exist.
                let facts = read_abandon_receipt(
                    &transaction,
                    request.project_id,
                    receipt_id,
                    &AggregateRef::AgentRun {
                        agent_run_id: request.agent_run_id,
                    },
                )?;
                request.evidence.verify_abandon(run.revision, &facts)?;
                ("operator_abandon", None, Some(receipt_id.to_string()))
            }
        };

        let next_revision = run.revision.next()?;
        let changed = transaction
            .execute(
                "UPDATE agent_runs
                 SET lifecycle = ?1, derived_state = 'terminal', terminal_outcome = ?2,
                     terminal_source_kind = ?3, terminal_event_cursor = ?4,
                     terminal_receipt_id = ?5, terminal_evidence_hash = ?6, closed_at = ?7,
                     revision = ?8
                 WHERE project_id = ?9 AND id = ?10 AND revision = ?11",
                params![
                    request.evidence.outcome.lifecycle().as_str(),
                    request.evidence.outcome.as_str(),
                    source_kind,
                    cursor_column,
                    receipt_column,
                    request.evidence.evidence_hash.as_str(),
                    text(request.evidence.closed_at),
                    revision_column(next_revision)?,
                    request.project_id.to_string(),
                    request.agent_run_id.to_string(),
                    revision_column(run.revision)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "agent run",
                "the run revision moved during the write",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn read_runtime_events(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        after: Option<EventCursor>,
    ) -> RepositoryResult<Vec<RuntimeEvent>> {
        crate::events::replay::read_runtime_events(self, project_id, agent_run_id, after)
    }
}

// ---------------------------------------------------------------------------
// Source events and intake
// ---------------------------------------------------------------------------

fn load_receipt_for_event(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    source_event_id: &str,
) -> RepositoryResult<Option<IntakeReceipt>> {
    let found: Option<String> = transaction
        .query_row(
            "SELECT receipt FROM intake_receipts
             WHERE project_id = ?1 AND source_event_id = ?2 AND result <> 'duplicate'
             ORDER BY decided_at LIMIT 1",
            params![project_id.to_string(), source_event_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    found
        .map(|json| from_json::<IntakeReceipt>(&json))
        .transpose()
}

impl IntakeRepository for SqliteStore {
    fn record_source_event(&self, request: &NewSourceEvent) -> RepositoryResult<IntakeOutcome> {
        request.receipt.validate()?;
        // The decision must be about the event being inserted. Filing a receipt
        // against an event it never evaluated would make every later lineage
        // check meaningless.
        request
            .receipt
            .ensure_decides(request.event.id, request.event.envelope.hash())?;
        let transaction = self.begin()?;
        let identity = &request.event.identity;

        // A repeat of either the source identity or the canonical payload on the
        // same connection is a duplicate: return the original decision and do
        // not create a second work graph. The stored digest is read back so the
        // two cases can be told apart.
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT id, envelope_hash FROM source_events
                 WHERE project_id = ?1
                   AND ((source_kind = ?2 AND source_connection = ?3 AND external_event_id = ?4)
                        OR (source_connection = ?3 AND envelope_hash = ?5))
                 LIMIT 1",
                params![
                    request.project_id.to_string(),
                    identity.source_kind.as_str(),
                    identity.source_connection.as_str(),
                    identity.external_event_id.as_str(),
                    request.event.envelope.hash().as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((original_event, stored_hash)) = existing {
            // The same source identity carrying *different* canonical bytes is
            // not a replay at all: the upstream system changed what it said
            // under an id it already used. Returning the old decision would
            // silently discard the new content, so this is a conflict a human
            // has to look at.
            if ContentHash::parse(&stored_hash)? != *request.event.envelope.hash() {
                return Err(conflict(
                    "source event",
                    "the same source identity already exists with different canonical bytes",
                ));
            }
            let original =
                load_receipt_for_event(&transaction, request.project_id, &original_event)?.ok_or(
                    RepositoryError::NotFound {
                        subject: "original intake receipt",
                    },
                )?;
            return Ok(IntakeOutcome::Duplicate(Box::new(original)));
        }

        transaction
            .execute(
                "INSERT INTO source_events
                     (id, project_id, source_kind, source_connection, external_event_id,
                      envelope, envelope_hash, external_observed_at, ingested_at,
                      processing_state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    request.event.id.to_string(),
                    request.project_id.to_string(),
                    identity.source_kind.as_str(),
                    identity.source_connection.as_str(),
                    identity.external_event_id.as_str(),
                    request.event.envelope.json(),
                    request.event.envelope.hash().as_str(),
                    text(request.event.external_observed_at),
                    text(request.event.ingested_at),
                    request.event.processing_state.as_str()
                ],
            )
            .map_err(backend)?;

        let receipt = to_json(&request.receipt)?;
        transaction
            .execute(
                "INSERT INTO intake_receipts
                     (id, project_id, source_event_id, source_event_hash, trigger_key,
                      trigger_version, result, receipt, idempotency_key, dedup_key,
                       duplicate_of, predecessor_receipt_id, decided_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    request.receipt.id.to_string(),
                    request.project_id.to_string(),
                    request.receipt.source_event_id.to_string(),
                    request.receipt.source_event_hash.as_str(),
                    request.receipt.trigger.as_str(),
                    version_column(request.receipt.trigger_version),
                    request.receipt.result.as_str(),
                    receipt,
                    request.receipt.idempotency_key.as_str(),
                    request.receipt.dedup_key.as_str(),
                    request.receipt.duplicate_of.map(|id| id.to_string()),
                    request
                        .receipt
                        .predecessor_receipt_id
                        .map(|id| id.to_string()),
                    text(request.receipt.decided_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(IntakeOutcome::Recorded(Box::new(request.receipt.clone())))
    }

    fn reevaluate_source_event(
        &self,
        request: &NewIntakeReevaluation,
    ) -> RepositoryResult<ReevaluationOutcome> {
        request.receipt.validate()?;
        let transaction = self.begin()?;

        // The event must exist in *this* project and still hash to what the
        // caller believes; a changed digest means they are deciding about
        // something else.
        let stored_hash: Option<String> = transaction
            .query_row(
                "SELECT envelope_hash FROM source_events WHERE project_id = ?1 AND id = ?2",
                params![
                    request.project_id.to_string(),
                    request.source_event_id.to_string()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(stored_hash) = stored_hash else {
            return Err(RepositoryError::NotFound {
                subject: "source event",
            });
        };
        if ContentHash::parse(&stored_hash)? != request.source_event_hash {
            return Err(DomainError::invalid(
                "IntakeReevaluation",
                "the source event no longer has the cited digest",
            )
            .into());
        }

        // The newest decision so far, and the revision it used.
        let latest: Option<(String, i64, String)> = transaction
            .query_row(
                "SELECT id, trigger_version, receipt FROM intake_receipts
                 WHERE project_id = ?1 AND source_event_id = ?2 AND trigger_key = ?3
                 ORDER BY trigger_version DESC LIMIT 1",
                params![
                    request.project_id.to_string(),
                    request.source_event_id.to_string(),
                    request.receipt.trigger.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let Some((latest_id, latest_version, latest_json)) = latest else {
            return Err(RepositoryError::NotFound {
                subject: "existing intake receipt",
            });
        };
        let latest_version = read_version(latest_version)?;
        let incoming = request.receipt.trigger_version;

        // Same revision: the decision has already been made. It is only a replay
        // if it is the *same* decision — a trigger revision is deterministic, so
        // a differing verdict, idempotency key or proposed graph under the same
        // revision is a contradiction rather than a repeat.
        if incoming == latest_version {
            let stored: IntakeReceipt = from_json(&latest_json)?;
            if !request.receipt.decides_the_same_as(&stored) {
                return Err(conflict(
                    "intake re-evaluation",
                    "the same trigger revision already recorded a different decision",
                ));
            }
            return Ok(ReevaluationOutcome::AlreadyDecided(Box::new(stored)));
        }
        if incoming.get() < latest_version.get() {
            return Err(DomainError::invalid(
                "IntakeReevaluation",
                "a trigger revision older than the latest decision cannot supersede it",
            )
            .into());
        }

        // The successor must decide the very event this request named, at the
        // digest the request proved, before it is linked to anything.
        request
            .receipt
            .ensure_decides(request.source_event_id, &request.source_event_hash)?;

        // The successor is pinned to a revision that must actually exist.
        let predecessor = IntakeReceiptId::parse(&latest_id)?;
        let successor = IntakeReceipt {
            predecessor_receipt_id: Some(predecessor),
            ..request.receipt.clone()
        };
        successor.validate()?;
        let receipt_json = to_json(&successor)?;
        transaction
            .execute(
                "INSERT INTO intake_receipts
                     (id, project_id, source_event_id, source_event_hash, trigger_key,
                      trigger_version, result, receipt, idempotency_key, dedup_key,
                      duplicate_of, predecessor_receipt_id, decided_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    successor.id.to_string(),
                    request.project_id.to_string(),
                    successor.source_event_id.to_string(),
                    successor.source_event_hash.as_str(),
                    successor.trigger.as_str(),
                    version_column(successor.trigger_version),
                    successor.result.as_str(),
                    receipt_json,
                    successor.idempotency_key.as_str(),
                    successor.dedup_key.as_str(),
                    successor.duplicate_of.map(|id| id.to_string()),
                    predecessor.to_string(),
                    text(successor.decided_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(ReevaluationOutcome::Superseded(Box::new(successor)))
    }

    fn find_intake_receipt(
        &self,
        project_id: ProjectId,
        identity: &SourceIdentity,
    ) -> RepositoryResult<Option<IntakeReceipt>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT r.receipt FROM intake_receipts r
                 JOIN source_events e ON e.id = r.source_event_id AND e.project_id = r.project_id
                 WHERE r.project_id = ?1 AND e.source_kind = ?2 AND e.source_connection = ?3
                   AND e.external_event_id = ?4
                 ORDER BY r.decided_at LIMIT 1",
                params![
                    project_id.to_string(),
                    identity.source_kind.as_str(),
                    identity.source_connection.as_str(),
                    identity.external_event_id.as_str()
                ],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|json| from_json::<IntakeReceipt>(&json))
            .transpose()
    }

    fn get_intake_receipt(
        &self,
        project_id: ProjectId,
        id: IntakeReceiptId,
    ) -> RepositoryResult<Option<IntakeReceipt>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT receipt FROM intake_receipts WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        found
            .map(|json| from_json::<IntakeReceipt>(&json))
            .transpose()
    }
}

// ---------------------------------------------------------------------------
// Commands and outbox
// ---------------------------------------------------------------------------

pub(crate) const RECEIPT_COLUMNS: &str = "id, project_id, idempotency_key, kind, target, target_revision, \
     intent, intent_hash, state, correlation, native_identity, result_ref, attempts, created_at, \
     updated_at";

impl CommandRepository for SqliteStore {
    fn record_intent(&self, request: &NewCommandIntent) -> RepositoryResult<CommandReceipt> {
        crate::commands::intent::record_intent(self, request)
    }

    fn get_receipt_by_key(&self, key: &IdempotencyKey) -> RepositoryResult<Option<CommandReceipt>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {RECEIPT_COLUMNS} FROM command_receipts WHERE idempotency_key = ?1"
                ),
                params![key.as_str()],
                |row| Ok(crate::commands::receipts::read_receipt_row(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn advance_receipt(&self, request: &ReceiptAdvance) -> RepositoryResult<CommandReceipt> {
        crate::commands::receipts::advance_receipt(self, request)
    }

    fn claim_outbox(
        &self,
        project_id: ProjectId,
        now: Timestamp,
        limit: u32,
    ) -> RepositoryResult<Vec<CommandOutboxEntry>> {
        crate::commands::intent::read_outbox(self, project_id, now, limit)
    }
}

// ---------------------------------------------------------------------------
// External tickets
// ---------------------------------------------------------------------------

impl TicketRepository for SqliteStore {
    fn create_ticket_link(&self, request: &NewTicketLink) -> RepositoryResult<TicketLink> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO jira_links
                     (id, project_id, task_id, connector, external_issue_key, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![
                    request.id.to_string(),
                    request.project_id.to_string(),
                    request.task_id.to_string(),
                    request.connector.as_str(),
                    request.external_issue_key.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(TicketLink {
            id: request.id,
            project_id: request.project_id,
            task_id: request.task_id,
            connector: request.connector.clone(),
            external_issue_key: request.external_issue_key.clone(),
            revision: AggregateRevision::INITIAL,
            created_at: request.created_at,
        })
    }

    fn insert_projection(
        &self,
        project_id: ProjectId,
        projection: &TicketSyncProjection,
        spec: &TicketFieldSpec,
    ) -> RepositoryResult<()> {
        // `canonicalize` already checks the projection against the mapping. The
        // projection also has to *name* that exact specification, or the pin it
        // persists would point somewhere the check never looked.
        if projection.field_spec_project != spec.project
            || projection.field_spec_issue_type != spec.issue_type
            || projection.field_spec_version != spec.version
            || projection.connector != spec.connector
        {
            return Err(DomainError::invalid(
                "TicketSyncProjection",
                "the pinned field specification is not the one it was checked against",
            )
            .into());
        }
        let document = projection.canonicalize(spec)?;
        let fields = to_json(&projection.fields)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO ticket_sync_projections
                     (id, project_id, link_id, link_revision, connector, field_spec_project,
                      field_spec_issue_type, field_spec_version, external_issue_key,
                      fields, comment_policy, external_comment_cursor, projection_hash,
                      computed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    projection.id.to_string(),
                    project_id.to_string(),
                    projection.link_id.to_string(),
                    revision_column(projection.link_revision)?,
                    projection.connector.as_str(),
                    projection.field_spec_project.as_str(),
                    projection.field_spec_issue_type.as_str(),
                    version_column(projection.field_spec_version),
                    projection.external_issue_key.as_str(),
                    fields,
                    projection.comment_policy.as_str(),
                    projection
                        .external_comment_cursor
                        .as_ref()
                        .map(ExternalId::as_str),
                    document.hash().as_str(),
                    text(projection.computed_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn append_observation(
        &self,
        project_id: ProjectId,
        observation: &ExternalTicketObservation,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO external_ticket_observations
                     (id, project_id, link_id, status_id, status_name, status_category,
                      issue_type, assignee_account_id, assignee_display, external_version,
                      observed_at, payload_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    observation.id.to_string(),
                    project_id.to_string(),
                    observation.link_id.to_string(),
                    observation.status.status_id.as_str(),
                    observation.status.status_name.as_str(),
                    observation.status_category.as_str(),
                    observation.issue_type.as_str(),
                    observation
                        .assignee_account_id
                        .as_ref()
                        .map(ExternalId::as_str),
                    observation
                        .assignee_display
                        .as_ref()
                        .map(ExternalName::as_str),
                    observation
                        .external_version
                        .as_ref()
                        .map(ExternalId::as_str),
                    text(observation.observed_at),
                    observation.payload_hash.as_str()
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn append_comment(
        &self,
        project_id: ProjectId,
        comment: &ExternalCommentRevision,
    ) -> RepositoryResult<bool> {
        comment.verify()?;
        let transaction = self.begin()?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO external_comments
                     (project_id, link_id, external_comment_id, body_hash, author_account_id,
                      author_display, external_created_at, external_updated_at, body,
                      observed_at, supersedes_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    project_id.to_string(),
                    comment.link_id.to_string(),
                    comment.external_comment_id.as_str(),
                    comment.body_hash.as_str(),
                    comment.author_account_id.as_str(),
                    comment.author_display.as_ref().map(ExternalName::as_str),
                    text(comment.external_created_at),
                    text(comment.external_updated_at),
                    comment.body.as_str(),
                    text(comment.observed_at),
                    comment.supersedes.as_ref().map(ContentHash::as_str)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(inserted == 1)
    }

    fn insert_conflict(
        &self,
        project_id: ProjectId,
        conflict_record: &StatusConflict,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO status_conflicts
                     (id, project_id, link_id, kind, observation_id, task_revision,
                      spec_version, milestone, detected_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    conflict_record.id.to_string(),
                    project_id.to_string(),
                    conflict_record.link_id.to_string(),
                    conflict_record.kind.as_str(),
                    conflict_record.observation_id.to_string(),
                    revision_column(conflict_record.task_revision)?,
                    version_column(conflict_record.spec_version),
                    conflict_record
                        .milestone
                        .as_ref()
                        .map(kontor_core::id::SemanticMilestoneKey::as_str),
                    text(conflict_record.detected_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn resolve_conflict(
        &self,
        project_id: ProjectId,
        conflict_id: StatusConflictId,
        receipt: CommandReceiptId,
        resolved_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        // A conflict is a disagreement about one ticket link, so the receipt
        // that resolves it must be a resolution aimed at that same link. The
        // link is read from the stored conflict rather than taken on trust.
        let link: Option<String> = transaction
            .query_row(
                "SELECT link_id FROM status_conflicts
                 WHERE project_id = ?1 AND id = ?2 AND resolved_at IS NULL",
                params![project_id.to_string(), conflict_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(link) = link else {
            return Err(conflict(
                "status conflict",
                "the conflict is unknown or already resolved",
            ));
        };
        ensure_receipt_authorizes(
            &transaction,
            "StatusConflict",
            project_id,
            receipt,
            CommandKind::ResolveStatusConflict,
            AggregateRef::TicketLink {
                link_id: TicketLinkId::parse(&link)?,
            },
        )?;
        let changed = transaction
            .execute(
                "UPDATE status_conflicts SET resolved_at = ?1, resolution_receipt_id = ?2
                 WHERE project_id = ?3 AND id = ?4 AND resolved_at IS NULL",
                params![
                    text(resolved_at),
                    receipt.to_string(),
                    project_id.to_string(),
                    conflict_id.to_string()
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "status conflict",
                "the conflict is unknown or already resolved",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn insert_transition_receipt(
        &self,
        project_id: ProjectId,
        receipt: &StatusTransitionReceipt,
    ) -> RepositoryResult<()> {
        receipt.validate()?;
        let plan = to_json(&receipt.plan)?;
        let assignment = receipt
            .assignment_result
            .as_ref()
            .map(to_json)
            .transpose()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO status_transition_receipts
                     (id, project_id, link_id, task_id, task_revision, workflow_revision,
                      projection_revision, spec_version, prior_observation_id, milestone,
                      target_status_id, transition_id, principal_account_id,
                      assignment_prerequisite, assignment_result, plan, idempotency_key,
                      dispatched_at, acknowledged_at, confirmed_at, refetched_observation_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20, ?21)",
                params![
                    receipt.id.to_string(),
                    project_id.to_string(),
                    receipt.link_id.to_string(),
                    receipt.task_id.to_string(),
                    revision_column(receipt.task_revision)?,
                    revision_column(receipt.workflow_revision)?,
                    revision_column(receipt.projection_revision)?,
                    version_column(receipt.spec_version),
                    receipt.prior_observation_id.to_string(),
                    receipt.plan.milestone.as_str(),
                    receipt.plan.target.status_id.as_str(),
                    receipt
                        .plan
                        .transition
                        .as_ref()
                        .map(|selected| selected.transition_id.as_str()),
                    receipt.principal.account_id.as_str(),
                    i64::from(receipt.plan.assignment_prerequisite),
                    assignment,
                    plan,
                    receipt.idempotency_key.as_str(),
                    text(receipt.dispatched_at),
                    receipt.acknowledged_at.map(text),
                    receipt.confirmed_at.map(text),
                    receipt.refetched_observation_id.map(|id| id.to_string())
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Calendars, authorizations and overrides
// ---------------------------------------------------------------------------

fn money_columns(money: Money) -> RepositoryResult<(i64, String)> {
    let minor = i64::try_from(money.minor_units).map_err(|_| RepositoryError::Backend {
        detail: "monetary amount exceeds the storable range".to_owned(),
    })?;
    Ok((minor, money.currency.as_str().to_owned()))
}

fn budget_columns(
    budget: kontor_core::spec::BudgetBounds,
) -> RepositoryResult<(i64, i64, i64, i64, String)> {
    let (cost, currency) = money_columns(budget.max_cost)?;
    Ok((
        i64::try_from(budget.max_tokens).unwrap_or(i64::MAX),
        i64::try_from(budget.max_commands).unwrap_or(i64::MAX),
        i64::try_from(budget.max_duration_seconds).unwrap_or(i64::MAX),
        cost,
        currency,
    ))
}

fn read_budget(
    tokens: i64,
    commands: i64,
    duration: i64,
    cost: i64,
    currency: &str,
) -> RepositoryResult<kontor_core::spec::BudgetBounds> {
    Ok(kontor_core::spec::BudgetBounds {
        max_tokens: u64::try_from(tokens).unwrap_or_default(),
        max_commands: u64::try_from(commands).unwrap_or_default(),
        max_duration_seconds: u64::try_from(duration).unwrap_or_default(),
        max_cost: Money {
            minor_units: u64::try_from(cost).unwrap_or_default(),
            currency: CurrencyCode::parse(currency)?,
        },
    })
}

fn read_assignment(row: &Row<'_>) -> RepositoryResult<WorkCalendarAssignment> {
    let window_override: Option<String> = row.get(5).map_err(backend)?;
    let retired_at: Option<String> = row.get(8).map_err(backend)?;
    let active: i64 = row.get(6).map_err(backend)?;
    Ok(WorkCalendarAssignment {
        id: WorkCalendarId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        profile_id: CalendarProfileId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        profile_version: read_version(row.get::<_, i64>(3).map_err(backend)?)?,
        timezone: IanaTimeZone::parse(&row.get::<_, String>(4).map_err(backend)?)?,
        window_override: window_override
            .as_deref()
            .map(from_json::<Vec<WeeklyWindow>>)
            .transpose()?,
        active: active == 1,
        created_at: read_timestamp(&row.get::<_, String>(7).map_err(backend)?)?,
        retired_at: retired_at.as_deref().map(read_timestamp).transpose()?,
    })
}

const ASSIGNMENT_COLUMNS: &str = "id, project_id, profile_id, profile_version, timezone, \
     window_override, active, created_at, retired_at";

impl CalendarRepository for SqliteStore {
    fn assign_calendar(&self, assignment: &WorkCalendarAssignment) -> RepositoryResult<()> {
        assignment.validate()?;
        let window_override = assignment
            .window_override
            .as_ref()
            .map(to_json)
            .transpose()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "UPDATE work_calendars SET active = 0, retired_at = ?1
                 WHERE project_id = ?2 AND active = 1",
                params![
                    text(assignment.created_at),
                    assignment.project_id.to_string()
                ],
            )
            .map_err(backend)?;
        transaction
            .execute(
                "INSERT INTO work_calendars
                     (id, project_id, profile_id, profile_version, timezone, window_override,
                      active, created_at, retired_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    assignment.id.to_string(),
                    assignment.project_id.to_string(),
                    assignment.profile_id.to_string(),
                    version_column(assignment.profile_version),
                    assignment.timezone.as_str(),
                    window_override,
                    i64::from(assignment.active),
                    text(assignment.created_at),
                    assignment.retired_at.map(text)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn retire_calendar(
        &self,
        project_id: ProjectId,
        id: WorkCalendarId,
        retired_at: Timestamp,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        transaction
            .execute(
                "UPDATE work_calendars SET active = 0, retired_at = ?1
                 WHERE project_id = ?2 AND id = ?3 AND active = 1",
                params![text(retired_at), project_id.to_string(), id.to_string()],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn active_assignment(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Option<WorkCalendarAssignment>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {ASSIGNMENT_COLUMNS} FROM work_calendars
                     WHERE project_id = ?1 AND active = 1"
                ),
                params![project_id.to_string()],
                |row| Ok(read_assignment(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    fn append_exception(&self, exception: &CalendarExceptionRevision) -> RepositoryResult<()> {
        exception.validate()?;
        let provenance = to_json(&exception.provenance)?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO calendar_exceptions
                     (id, project_id, work_calendar_id, start_date, end_date, kind, label,
                      provenance, supersedes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    exception.id.to_string(),
                    exception.project_id.to_string(),
                    exception.work_calendar_id.to_string(),
                    exception.start_date.to_string(),
                    exception.end_date.to_string(),
                    exception.kind.as_str(),
                    exception.label.as_str(),
                    provenance,
                    exception.supersedes.map(|id| id.to_string()),
                    text(exception.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn list_exceptions(
        &self,
        project_id: ProjectId,
        work_calendar_id: WorkCalendarId,
    ) -> RepositoryResult<Vec<CalendarExceptionRevision>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, start_date, end_date, kind, label, provenance, supersedes, created_at
                 FROM calendar_exceptions
                 WHERE project_id = ?1 AND work_calendar_id = ?2
                 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                work_calendar_id.to_string()
            ])
            .map_err(backend)?;
        let mut exceptions = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let supersedes: Option<String> = row.get(6).map_err(backend)?;
            exceptions.push(CalendarExceptionRevision {
                id: CalendarExceptionId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                project_id,
                work_calendar_id,
                start_date: row
                    .get::<_, String>(1)
                    .map_err(backend)?
                    .parse()
                    .map_err(|_| RepositoryError::Backend {
                        detail: "stored calendar date is not a civil date".to_owned(),
                    })?,
                end_date: row
                    .get::<_, String>(2)
                    .map_err(backend)?
                    .parse()
                    .map_err(|_| RepositoryError::Backend {
                        detail: "stored calendar date is not a civil date".to_owned(),
                    })?,
                kind: ExceptionKind::parse(&row.get::<_, String>(3).map_err(backend)?)?,
                label: ExternalName::parse(&row.get::<_, String>(4).map_err(backend)?)?,
                provenance: from_json::<ExceptionProvenance>(
                    &row.get::<_, String>(5).map_err(backend)?,
                )?,
                supersedes: supersedes
                    .as_deref()
                    .map(CalendarExceptionId::parse)
                    .transpose()?,
                created_at: read_timestamp(&row.get::<_, String>(7).map_err(backend)?)?,
            });
        }
        Ok(exceptions)
    }

    fn get_exception(
        &self,
        project_id: ProjectId,
        id: CalendarExceptionId,
    ) -> RepositoryResult<Option<CalendarExceptionRevision>> {
        let calendar: Option<String> = self
            .connection
            .query_row(
                "SELECT work_calendar_id FROM calendar_exceptions
                 WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(calendar) = calendar else {
            return Ok(None);
        };
        let calendar = WorkCalendarId::parse(&calendar)?;
        Ok(self
            .list_exceptions(project_id, calendar)?
            .into_iter()
            .find(|exception| exception.id == id))
    }

    fn insert_holiday_source(&self, revision: &HolidaySourceRevision) -> RepositoryResult<()> {
        revision.validate()?;
        let transaction = self.begin()?;
        transaction
            .execute(
                "INSERT INTO holiday_sources
                     (id, profile_id, profile_version, provider, country, subdivision,
                      reference, range_start, range_end, retrieved_at, raw_hash,
                      normalized_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    revision.id.to_string(),
                    revision.profile_id.to_string(),
                    version_column(revision.profile_version),
                    match revision.provider {
                        HolidayProviderKind::Ical => "ical",
                        HolidayProviderKind::Manual => "manual",
                        HolidayProviderKind::Bundled => "bundled",
                    },
                    revision.country.as_str(),
                    revision.subdivision.as_ref().map(ExternalName::as_str),
                    revision.reference.as_str(),
                    revision.range_start.to_string(),
                    revision.range_end.to_string(),
                    text(revision.retrieved_at),
                    revision.raw_hash.as_str(),
                    revision.normalized_hash.as_str()
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn insert_authorization(&self, authorization: &ExecutionAuthorization) -> RepositoryResult<()> {
        authorization.validate()?;
        let (kind, mini_project, task) = scope_columns(authorization.scope);
        let selected = to_json(&authorization.selected_tasks)?;
        let (tokens, commands, duration, cost, currency) = budget_columns(authorization.budget)?;
        let transaction = self.begin()?;
        // The capability receipt must be a receipt that actually grants this
        // capability over this scope. Existing in the project is not consent.
        ensure_receipt_authorizes(
            &transaction,
            "ExecutionAuthorization",
            authorization.project_id,
            authorization.capability_receipt,
            CommandKind::AuthorizeExecution,
            authorization.scope.aggregate(authorization.project_id),
        )?;
        // Every selected task must lie inside the authorization's own scope. A
        // task-scoped authorization may only arm that task; a goal-scoped one
        // may only arm tasks that belong to that goal — which is a fact about
        // the task row, not about the scope value, so it is read here in the
        // same transaction rather than assumed.
        for task in &authorization.selected_tasks {
            let inside = match authorization.scope {
                // The composite foreign key on the child rows already proves
                // every selected task belongs to this project.
                WorkScope::Project => true,
                WorkScope::Task { task_id } => task_id == *task,
                WorkScope::MiniProject { mini_project_id } => {
                    let owner: Option<Option<String>> = transaction
                        .query_row(
                            "SELECT mini_project_id FROM tasks WHERE project_id = ?1 AND id = ?2",
                            params![authorization.project_id.to_string(), task.to_string()],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(backend)?;
                    owner.flatten().as_deref() == Some(mini_project_id.to_string().as_str())
                }
            };
            if !inside {
                return Err(DomainError::invalid(
                    "ExecutionAuthorization",
                    "a selected task lies outside the authorization scope",
                )
                .into());
            }
        }
        transaction
            .execute(
                "INSERT INTO execution_authorizations
                     (id, project_id, scope_kind, scope_mini_project_id, scope_task_id,
                      selected_tasks, allowed_start, allowed_end, max_concurrency, max_tokens,
                      max_commands, max_duration_seconds, max_cost_minor_units, cost_currency,
                      created_by, capability_receipt_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17)",
                params![
                    authorization.id.to_string(),
                    authorization.project_id.to_string(),
                    kind,
                    mini_project,
                    task,
                    selected,
                    text(authorization.allowed_start.start),
                    text(authorization.allowed_start.end),
                    i64::from(authorization.max_concurrency),
                    tokens,
                    commands,
                    duration,
                    cost,
                    currency,
                    authorization.created_by.to_string(),
                    authorization.capability_receipt.to_string(),
                    text(authorization.created_at)
                ],
            )
            .map_err(backend)?;

        // The child set must equal the canonical value exactly, so it is written
        // from that value and nowhere else.
        for task in &authorization.selected_tasks {
            transaction
                .execute(
                    "INSERT INTO execution_authorization_tasks
                         (project_id, authorization_id, task_id)
                     VALUES (?1, ?2, ?3)",
                    params![
                        authorization.project_id.to_string(),
                        authorization.id.to_string(),
                        task.to_string()
                    ],
                )
                .map_err(backend)?;
        }
        // Re-read it and prove the agreement rather than assuming it.
        let stored: i64 = transaction
            .query_row(
                "SELECT count(*) FROM execution_authorization_tasks
                 WHERE project_id = ?1 AND authorization_id = ?2",
                params![
                    authorization.project_id.to_string(),
                    authorization.id.to_string()
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let expected: BTreeSet<TaskId> = authorization.selected_tasks.iter().copied().collect();
        if usize::try_from(stored).unwrap_or_default() != expected.len() {
            return Err(DomainError::invalid(
                "ExecutionAuthorization",
                "the stored task set does not match the canonical value",
            )
            .into());
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn insert_override(&self, schedule_override: &ScheduleOverride) -> RepositoryResult<()> {
        schedule_override.validate()?;
        let (kind, mini_project, task) = scope_columns(schedule_override.scope);
        let (expiry_kind, expiry_at, expiry_goal) = match schedule_override.expiry {
            OverrideExpiry::FixedAt { at } => ("fixed_at", Some(text(at)), None),
            OverrideExpiry::GoalBound { mini_project_id } => {
                ("goal_bound", None, Some(mini_project_id.to_string()))
            }
        };
        let (tokens, commands, duration, cost, currency) =
            budget_columns(schedule_override.budget)?;
        let transaction = self.begin()?;
        // An override is only approved if an approval receipt says so, over
        // this exact scope.
        ensure_receipt_authorizes(
            &transaction,
            "ScheduleOverride",
            schedule_override.project_id,
            schedule_override.approval_receipt,
            CommandKind::ApproveScheduleOverride,
            schedule_override
                .scope
                .aggregate(schedule_override.project_id),
        )?;
        transaction
            .execute(
                "INSERT INTO schedule_overrides
                     (id, project_id, scope_kind, scope_mini_project_id, scope_task_id, reason,
                      start_at, expiry_kind, expiry_at, expiry_mini_project_id, hard_ceiling,
                      max_concurrency, max_tokens, max_commands, max_duration_seconds,
                      max_cost_minor_units, cost_currency, approved_by, approval_receipt_id,
                      created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20)",
                params![
                    schedule_override.id.to_string(),
                    schedule_override.project_id.to_string(),
                    kind,
                    mini_project,
                    task,
                    schedule_override.reason.as_str(),
                    text(schedule_override.start),
                    expiry_kind,
                    expiry_at,
                    expiry_goal,
                    text(schedule_override.hard_ceiling),
                    i64::from(schedule_override.max_concurrency),
                    tokens,
                    commands,
                    duration,
                    cost,
                    currency,
                    schedule_override.approved_by.to_string(),
                    schedule_override.approval_receipt.to_string(),
                    text(schedule_override.start)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    fn revoke_override(
        &self,
        project_id: ProjectId,
        id: ScheduleOverrideId,
        revocation: &OverrideRevocation,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        // The revocation receipt has to match the scope of the override it
        // revokes, so the scope is read from the stored row — the caller only
        // supplies an id. A live override is required for the read to succeed,
        // which is the same condition the update below enforces.
        let scope: Option<RepositoryResult<WorkScope>> = transaction
            .query_row(
                "SELECT scope_kind, scope_mini_project_id, scope_task_id
                 FROM schedule_overrides
                 WHERE project_id = ?1 AND id = ?2 AND revoked_at IS NULL",
                params![project_id.to_string(), id.to_string()],
                |row| {
                    let build = || -> RepositoryResult<WorkScope> {
                        read_scope(
                            &row.get::<_, String>(0).map_err(backend)?,
                            row.get(1).map_err(backend)?,
                            row.get(2).map_err(backend)?,
                        )
                    };
                    Ok(build())
                },
            )
            .optional()
            .map_err(backend)?;
        let Some(scope) = scope else {
            return Err(conflict(
                "schedule override",
                "the override is unknown or already revoked",
            ));
        };
        // A revocation is its own command: an approval receipt is not
        // permission to undo the thing it approved.
        ensure_receipt_authorizes(
            &transaction,
            "OverrideRevocation",
            project_id,
            revocation.receipt,
            CommandKind::RevokeScheduleOverride,
            scope?.aggregate(project_id),
        )?;
        let changed = transaction
            .execute(
                "UPDATE schedule_overrides
                 SET revoked_at = ?1, revoked_by = ?2, revocation_receipt_id = ?3
                 WHERE project_id = ?4 AND id = ?5 AND revoked_at IS NULL",
                params![
                    text(revocation.revoked_at),
                    revocation.revoked_by.to_string(),
                    revocation.receipt.to_string(),
                    project_id.to_string(),
                    id.to_string()
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "schedule override",
                "the override is unknown or already revoked",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn get_override(
        &self,
        project_id: ProjectId,
        id: ScheduleOverrideId,
    ) -> RepositoryResult<Option<ScheduleOverride>> {
        let row: Option<RepositoryResult<ScheduleOverride>> = self
            .connection
            .query_row(
                "SELECT scope_kind, scope_mini_project_id, scope_task_id, reason, start_at,
                        expiry_kind, expiry_at, expiry_mini_project_id, hard_ceiling,
                        max_concurrency, max_tokens, max_commands, max_duration_seconds,
                        max_cost_minor_units, cost_currency, approved_by, approval_receipt_id,
                        revoked_at, revoked_by, revocation_receipt_id
                 FROM schedule_overrides WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| {
                    let build = || -> RepositoryResult<ScheduleOverride> {
                        let scope = read_scope(
                            &row.get::<_, String>(0).map_err(backend)?,
                            row.get(1).map_err(backend)?,
                            row.get(2).map_err(backend)?,
                        )?;
                        let expiry_kind: String = row.get(5).map_err(backend)?;
                        let expiry_at: Option<String> = row.get(6).map_err(backend)?;
                        let expiry_goal: Option<String> = row.get(7).map_err(backend)?;
                        let expiry = match expiry_kind.as_str() {
                            "fixed_at" => OverrideExpiry::FixedAt {
                                at: read_timestamp(expiry_at.as_deref().unwrap_or_default())?,
                            },
                            _ => OverrideExpiry::GoalBound {
                                mini_project_id: MiniProjectId::parse(
                                    expiry_goal.as_deref().unwrap_or_default(),
                                )?,
                            },
                        };
                        let revoked_at: Option<String> = row.get(17).map_err(backend)?;
                        let revoked_by: Option<String> = row.get(18).map_err(backend)?;
                        let revocation_receipt: Option<String> = row.get(19).map_err(backend)?;
                        let revocations = match (revoked_at, revoked_by, revocation_receipt) {
                            (Some(at), Some(by), Some(receipt)) => vec![OverrideRevocation {
                                revoked_at: read_timestamp(&at)?,
                                revoked_by: AccountProfileId::parse(&by)?,
                                receipt: CommandReceiptId::parse(&receipt)?,
                            }],
                            _ => Vec::new(),
                        };
                        Ok(ScheduleOverride {
                            id,
                            project_id,
                            scope,
                            reason: ExternalName::parse(
                                &row.get::<_, String>(3).map_err(backend)?,
                            )?,
                            start: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
                            expiry,
                            hard_ceiling: read_timestamp(
                                &row.get::<_, String>(8).map_err(backend)?,
                            )?,
                            max_concurrency: u32::try_from(row.get::<_, i64>(9).map_err(backend)?)
                                .unwrap_or(u32::MAX),
                            budget: read_budget(
                                row.get(10).map_err(backend)?,
                                row.get(11).map_err(backend)?,
                                row.get(12).map_err(backend)?,
                                row.get(13).map_err(backend)?,
                                &row.get::<_, String>(14).map_err(backend)?,
                            )?,
                            approved_by: AccountProfileId::parse(
                                &row.get::<_, String>(15).map_err(backend)?,
                            )?,
                            approval_receipt: CommandReceiptId::parse(
                                &row.get::<_, String>(16).map_err(backend)?,
                            )?,
                            revocations,
                        })
                    };
                    Ok(build())
                },
            )
            .optional()
            .map_err(backend)?;
        row.transpose()
    }
}

/// Rebuild a team-run snapshot from a stored row. Kept next to the run
/// repository so the read path and the write path share one shape.
pub(crate) fn team_run_snapshot(json: &str, hash: &str) -> RepositoryResult<TeamRunSnapshot> {
    stored_document(json, hash)
}

/// Load a team's own child runs as immutable evidence rows.
///
/// Scoped by project *and* team, so a globally valid run id belonging to another
/// team or project is simply not in the result set.
fn read_team_child_evidence(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    team_run_id: TeamRunId,
) -> RepositoryResult<Vec<TeamChildEvidence>> {
    let mut statement = transaction
        .prepare(
            "SELECT id, lifecycle, terminal_evidence_hash FROM agent_runs
             WHERE project_id = ?1 AND team_run_id = ?2 ORDER BY id",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), team_run_id.to_string()])
        .map_err(backend)?;
    let mut children = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        children.push(TeamChildEvidence {
            agent_run_id: AgentRunId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
            lifecycle: RunLifecycle::parse(&row.get::<_, String>(1).map_err(backend)?)?,
            evidence_hash: row
                .get::<_, Option<String>>(2)
                .map_err(backend)?
                .as_deref()
                .map(ContentHash::parse)
                .transpose()?,
        });
    }
    Ok(children)
}

/// Load the facts an operator-abandon closure is proved against.
///
/// Only *facts* are returned; whether they authorize the closure is decided by
/// [`AbandonReceiptFacts::verify`] in the domain, so the agent and team paths
/// cannot drift apart.
fn read_abandon_receipt(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
    expected_target: &AggregateRef,
) -> RepositoryResult<AbandonReceiptFacts> {
    let found: Option<(String, String, String, i64, String)> = transaction
        .query_row(
            "SELECT kind, intent_hash, target, target_revision, created_at FROM command_receipts
             WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), receipt_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((kind, intent_hash, target, target_revision, created_at)) = found else {
        return Err(RepositoryError::NotFound {
            subject: "abandon receipt",
        });
    };
    let target: AggregateRef = from_json(&target)?;
    Ok(AbandonReceiptFacts {
        kind_is_abandon: kind == CommandKind::AbandonRun.as_str(),
        targets_aggregate: &target == expected_target,
        target_revision: revision_of(target_revision)?,
        intent_hash: ContentHash::parse(&intent_hash)?,
        recorded_at: read_timestamp(&created_at)?,
    })
}

/// Prove a cited receipt actually authorizes `kind` against `target`, inside
/// the transaction that is about to consume it.
///
/// The foreign key already proves the receipt exists in this project. It says
/// nothing about *what the receipt is for*, and a receipt for one command
/// against one aggregate is not permission to do a different thing elsewhere.
/// The check re-reads the stored row rather than trusting anything the caller
/// passed alongside the id.
fn ensure_receipt_authorizes(
    transaction: &Transaction<'_>,
    subject: &'static str,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
    kind: CommandKind,
    target: AggregateRef,
) -> RepositoryResult<()> {
    let found: Option<(String, String, String)> = transaction
        .query_row(
            "SELECT project_id, kind, target FROM command_receipts WHERE id = ?1",
            params![receipt_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(backend)?;
    let Some((stored_project, stored_kind, stored_target)) = found else {
        return Err(RepositoryError::NotFound {
            subject: "authorizing receipt",
        });
    };
    let authority = ReceiptAuthority {
        project_id: ProjectId::parse(&stored_project)?,
        kind: CommandKind::parse(&stored_kind)?,
        target: from_json(&stored_target)?,
    };
    authority.authorizes(subject, project_id, kind, target)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Realm ingress
// ---------------------------------------------------------------------------

impl RealmRepository for SqliteStore {
    fn realm(&self) -> RealmId {
        self.realm_id()
    }

    fn record_intent_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewCommandIntent>,
    ) -> RepositoryResult<CommandReceipt> {
        // The Realm is proved before a transaction opens, so a foreign envelope
        // never reaches SQL at all.
        let request = envelope.peek(self.realm_id())?;
        self.record_intent(request)
    }

    fn record_observation_in_realm(
        &self,
        envelope: &EventEnvelope<NewObservation>,
    ) -> RepositoryResult<RunProjection> {
        let request = envelope.peek(self.realm_id())?;
        self.record_observation(request)
    }

    fn record_source_event_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewSourceEvent>,
    ) -> RepositoryResult<IntakeOutcome> {
        let request = envelope.peek(self.realm_id())?;
        self.record_source_event(request)
    }

    fn reevaluate_source_event_in_realm(
        &self,
        envelope: &ReceiptEnvelope<NewIntakeReevaluation>,
    ) -> RepositoryResult<ReevaluationOutcome> {
        let request = envelope.peek(self.realm_id())?;
        self.reevaluate_source_event(request)
    }

    fn import_receipt_in_realm(
        &self,
        envelope: &ReceiptEnvelope<CommandReceipt>,
    ) -> RepositoryResult<CommandReceipt> {
        let presented = envelope.peek(self.realm_id())?;
        // A receipt is *found*, never re-created: importing one is a lookup of
        // something this Realm already minted. An id minted elsewhere simply has
        // no row here, which is the isolation argument working as intended.
        let stored = self.get_receipt_by_key(&presented.idempotency_key)?.ok_or(
            RepositoryError::NotFound {
                subject: "command receipt",
            },
        )?;
        if stored.id != presented.id
            || stored.project_id != presented.project_id
            || stored.kind != presented.kind
            || stored.target != presented.target
            || stored.intent.hash() != presented.intent.hash()
        {
            return Err(DomainError::invalid(
                "CommandReceipt",
                "an idempotency key may not be reused for a different command",
            )
            .into());
        }
        Ok(stored)
    }

    fn read_events_after(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        after: Option<RealmCursor>,
    ) -> RepositoryResult<Vec<EventEnvelope<RuntimeEvent>>> {
        let realm = self.realm_id();
        // A cursor from another Realm counts in a different space entirely;
        // resolving it here is the whole point of the qualified pair.
        let resolved = after.map(|cursor| cursor.resolve(realm)).transpose()?;
        let events = self.read_runtime_events(project_id, agent_run_id, resolved)?;
        Ok(events
            .into_iter()
            .map(|event| EventEnvelope::new(realm, event.cursor, event))
            .collect())
    }

    fn snapshot_agent_run(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
    ) -> RepositoryResult<SnapshotEnvelope<Option<AgentRun>>> {
        let transaction = self.begin()?;
        let run = read_agent_run(&transaction, project_id, agent_run_id)?;
        // The snapshot is taken with the highest allocated cursor so a
        // subscriber can resume strictly after it.
        let highest: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(cursor), 0) FROM runtime_events WHERE project_id = ?1",
                params![project_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let cursor = EventCursor::parse(highest.max(1))?;
        Ok(SnapshotEnvelope::new(self.realm_id(), cursor, run))
    }
}
