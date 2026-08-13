//! The declarative work graph: ensuring a project exists, applying a whole epic
//! atomically, and reading back the parts no single-aggregate port exposes.
//!
//! Everything here exists because an empty Realm has to be brought to a runnable
//! state through the public API alone. The single-aggregate ports in
//! [`kontor_core::repository`] each open their own transaction, so composing
//! them from outside this crate would create a work graph one row at a time —
//! and a refused dependency edge would leave the tasks it refused behind. The
//! operations below open *one* transaction and either write the whole graph or
//! none of it.
//!
//! # Natural identity, and why it is the name
//!
//! An `ensure`/`apply` operation has to answer "is this the same thing I was
//! asked to create last time?" without a caller-supplied surrogate key, because
//! there is nowhere in the schema to put one and inventing a column for it would
//! be a second identity that can disagree with the first. So the identity is the
//! one the schema already enforces:
//!
//! * a project is its `root_path`, which is `UNIQUE` across the database;
//! * a goal is its `name` inside its project;
//! * a task is its `title` inside its goal.
//!
//! Presenting the same identity with different immutable content is drift, and
//! drift is refused rather than reconciled: silently updating would rewrite what
//! a running task was admitted under.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::DomainError;
use kontor_core::calendar::ExecutionAuthorization;
use kontor_core::id::{
    AccountProfileId, AggregateRevision, CommandReceiptId, ConnectorKey, ExecutionAuthorizationId,
    ExternalId, ExternalName, MiniProjectId, ModuleKey, ProjectId, SpecVersion, TaskId,
    TaskWorkflowId, TeamTemplateId, TicketLinkId, Timestamp,
};
use kontor_core::repository::{
    MiniProject, Project, RepositoryError, RepositoryResult, Task, TicketLink,
    validate_dependency_graph,
};
use kontor_core::spec::{ResolvedWorkProfileSnapshot, TeamTemplateRevision, WorkProfileSpec};
use kontor_core::state::TaskState;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::SqliteStore;
use crate::repository::{
    TASK_COLUMNS, backend, conflict, read_project, read_scope, read_task, read_timestamp,
    read_version, revision_of, text, version_column,
};

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

/// The project one `ensure` names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEnsure {
    /// The id to use *if* this call is the one that creates the project.
    pub id: ProjectId,
    /// Human name. Immutable once the project exists.
    pub name: ExternalName,
    /// Absolute root path. The natural identity.
    pub root_path: ExternalName,
    /// Creation instant, used only when the project is created.
    pub created_at: Timestamp,
}

/// One external ticket an applied task is linked to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicTicketLink {
    /// The connector.
    pub connector: ConnectorKey,
    /// The external issue key.
    pub external_issue_key: ExternalId,
}

/// One task in a declarative epic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicTask {
    /// The title, which is this task's natural identity inside the epic.
    pub title: ExternalName,
    /// The module the task contends for, if any. Immutable.
    pub module: Option<ModuleKey>,
    /// The lifecycle state a newly created task starts in.
    pub state: TaskState,
    /// The titles of the sibling tasks this one depends on.
    pub depends_on: BTreeSet<ExternalName>,
    /// The external tickets to link. Immutable as a set.
    pub ticket_links: Vec<EpicTicketLink>,
}

/// One whole epic, stated declaratively.
///
/// The profile is not a name to look up: the caller resolves it first and hands
/// the *frozen* snapshot here, so what is written onto each task is byte-identical
/// to what the caller validated. `definition` is the same profile in its
/// pre-resolution spelling, stored so the pin has a stored revision to point at.
#[derive(Debug, Clone)]
pub struct EpicApplication<'a> {
    /// The project the epic belongs to.
    pub project_id: ProjectId,
    /// The epic's name, which is its natural identity inside the project.
    pub name: ExternalName,
    /// The tasks, in the order they were stated.
    pub tasks: &'a [EpicTask],
    /// The frozen work profile every task in the epic pins.
    pub profile: &'a ResolvedWorkProfileSnapshot,
    /// That profile's stored revision.
    pub definition: &'a WorkProfileSpec,
    /// The team revision the profile pins, when it prescribes one.
    pub team: Option<&'a TeamTemplateRevision>,
    /// When the application happened.
    pub applied_at: Timestamp,
}

// ---------------------------------------------------------------------------
// Outcomes
// ---------------------------------------------------------------------------

/// Whether an `ensure`/`apply` created the row or found it already matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// This call wrote the row.
    Created,
    /// The row already existed and matched, so nothing was written.
    Unchanged,
}

impl Applied {
    /// The stable spelling used on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Unchanged => "unchanged",
        }
    }

    /// `Created` when `created`, otherwise `Unchanged`.
    #[must_use]
    pub const fn of(created: bool) -> Self {
        if created {
            Self::Created
        } else {
            Self::Unchanged
        }
    }
}

/// One task after an epic was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedTask {
    /// The title it was addressed by.
    pub title: ExternalName,
    /// The task.
    pub task_id: TaskId,
    /// Whether this call created it.
    pub applied: Applied,
    /// Its lifecycle state.
    pub state: TaskState,
    /// The revision a write must present.
    pub revision: AggregateRevision,
    /// The workflow that froze the epic's profile onto it.
    pub workflow_id: TaskWorkflowId,
    /// The tasks it depends on, resolved from titles to ids.
    pub depends_on: BTreeSet<TaskId>,
    /// Its external ticket links.
    pub links: Vec<AppliedLink>,
}

/// One ticket link after an epic was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedLink {
    /// The link.
    pub id: TicketLinkId,
    /// The connector.
    pub connector: ConnectorKey,
    /// The external issue key.
    pub external_issue_key: ExternalId,
    /// Whether this call created it.
    pub applied: Applied,
}

/// One epic after it was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedEpic {
    /// The goal that carries the epic.
    pub mini_project_id: MiniProjectId,
    /// Whether this call created it.
    pub applied: Applied,
    /// The revision a write must present.
    pub revision: AggregateRevision,
    /// The work profile revision frozen onto every task.
    pub profile: (kontor_core::id::WorkProfileKey, SpecVersion),
    /// The team revision the profile pins, when it prescribes one.
    pub team: Option<(TeamTemplateId, SpecVersion)>,
    /// The tasks, in the order they were stated.
    pub tasks: Vec<AppliedTask>,
}

/// One seat inside a team run: the role slot, its run and its native session.
///
/// The native id is correlation evidence and never an identity: nothing keys off
/// it, and a seat with no binding is a run that was admitted and has not launched
/// rather than one that finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatRow {
    /// The agent run filling the slot.
    pub agent_run_id: kontor_core::id::AgentRunId,
    /// The role slot it fills, as the run persists it.
    pub role: kontor_core::id::RoleKey,
    /// The runtime family that owns the session, once bound.
    pub runtime_kind: Option<kontor_core::id::RuntimeKindKey>,
    /// The runtime's own session id, once bound.
    pub native_id: Option<kontor_core::id::ExternalId>,
    /// The binding, once bound.
    pub binding_id: Option<kontor_core::id::RuntimeBindingId>,
}

/// Why an authorization is no longer arming anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRevocation {
    /// When it was revoked.
    pub revoked_at: Timestamp,
    /// Who revoked it.
    pub revoked_by: AccountProfileId,
    /// The command receipt that recorded the revocation.
    pub receipt: CommandReceiptId,
    /// The stated reason.
    pub reason: ExternalName,
}

/// One stored authorization together with its revocation, if it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAuthorization {
    /// The authorization exactly as it was granted.
    pub authorization: ExecutionAuthorization,
    /// Its revocation, once it has been disarmed.
    pub revocation: Option<AuthorizationRevocation>,
}

impl StoredAuthorization {
    /// Whether this authorization still arms `task` at `now`.
    ///
    /// A revoked authorization arms nothing, whatever its time range says: that
    /// is the whole point of disarming, and reading the range alone would let a
    /// revoked grant keep admitting work until it happened to expire.
    #[must_use]
    pub fn arms(
        &self,
        now: Timestamp,
        mini_project: Option<MiniProjectId>,
        task: Option<TaskId>,
    ) -> bool {
        self.revocation.is_none() && self.authorization.arms(now, mini_project, task)
    }
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

impl SqliteStore {
    /// Create a project, or return the one that already stands at that root.
    ///
    /// The root path is the natural identity because the schema already makes it
    /// unique. A second call naming the same root with a different name is drift
    /// and is refused; it is not an update, because a project's name is part of
    /// what every task under it was created against.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the root exists under a
    /// different name, or when the id or name is already taken by another root.
    pub fn ensure_project(&self, request: &ProjectEnsure) -> RepositoryResult<(Project, Applied)> {
        let transaction = self.begin()?;
        let existing: Option<Project> = transaction
            .query_row(
                "SELECT id, name, root_path, revision, created_at
                 FROM projects WHERE root_path = ?1",
                params![request.root_path.as_str()],
                |row| Ok(read_project(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()?;
        if let Some(project) = existing {
            if project.name != request.name {
                return Err(conflict(
                    "project",
                    "a project already stands at that root under a different name",
                ));
            }
            return Ok((project, Applied::Unchanged));
        }
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
        Ok((
            Project {
                id: request.id,
                name: request.name.clone(),
                root_path: request.root_path.clone(),
                revision: AggregateRevision::INITIAL,
                created_at: request.created_at,
            },
            Applied::Created,
        ))
    }

    /// Apply one whole epic — goal, tasks, dependency edges, ticket links and the
    /// frozen profile on every task — in a single transaction.
    ///
    /// Every reference is resolved and every rule is checked *inside* that
    /// transaction, so a cycle, a dangling dependency title or a duplicate ticket
    /// link rolls the entire application back. There is no state in which half an
    /// epic exists.
    ///
    /// Re-applying the identical epic writes nothing and reports every item as
    /// [`Applied::Unchanged`]. Re-applying a *different* epic under the same names
    /// is refused rather than reconciled.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] for drift and for a duplicate link,
    /// and [`DomainError::Invalid`] for a duplicate title, a dependency naming no
    /// sibling, and a cyclic graph.
    pub fn apply_epic(&self, request: &EpicApplication<'_>) -> RepositoryResult<AppliedEpic> {
        request.profile.verify()?;
        ensure_titles_unique(request.tasks)?;
        ensure_dependencies_named(request.tasks)?;

        let transaction = self.begin()?;
        ensure_project_exists(&transaction, request.project_id)?;
        store_specifications(&transaction, request)?;

        let (mini_project, epic_applied) = ensure_mini_project(&transaction, request)?;
        let mut applied: Vec<AppliedTask> = Vec::with_capacity(request.tasks.len());
        let mut by_title: BTreeMap<&ExternalName, TaskId> = BTreeMap::new();

        for plan in request.tasks {
            let outcome = ensure_task(&transaction, request, mini_project.id, plan)?;
            by_title.insert(&plan.title, outcome.task_id);
            applied.push(outcome);
        }

        // The edges are written only once every task in the epic has an id, so a
        // dependency may name a sibling stated later in the same request.
        for (plan, outcome) in request.tasks.iter().zip(applied.iter_mut()) {
            let mut resolved = BTreeSet::new();
            for title in &plan.depends_on {
                let Some(dependency) = by_title.get(title).copied() else {
                    return Err(DomainError::invalid(
                        "task dependency",
                        "names a task this epic does not state",
                    )
                    .into());
                };
                resolved.insert(dependency);
            }
            write_dependencies(&transaction, request.project_id, outcome.task_id, &resolved)?;
            outcome.depends_on = resolved;
        }
        ensure_acyclic(&transaction, request.project_id)?;

        for (plan, outcome) in request.tasks.iter().zip(applied.iter_mut()) {
            outcome.links = ensure_links(&transaction, request, outcome.task_id, plan)?;
        }

        transaction.commit().map_err(backend)?;
        Ok(AppliedEpic {
            mini_project_id: mini_project.id,
            applied: epic_applied,
            revision: mini_project.revision,
            profile: (request.definition.id.clone(), request.definition.version),
            team: request
                .team
                .map(|revision| (revision.template_id, revision.version)),
            tasks: applied,
        })
    }

    /// Read one goal inside a project.
    ///
    /// # Errors
    /// Backend failures only; a goal from another project is `Ok(None)`.
    pub fn get_mini_project(
        &self,
        project_id: ProjectId,
        id: MiniProjectId,
    ) -> RepositoryResult<Option<MiniProject>> {
        self.connection
            .query_row(
                "SELECT id, project_id, name, revision, created_at
                 FROM mini_projects WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| Ok(read_mini_project(row)),
            )
            .optional()
            .map_err(backend)?
            .transpose()
    }

    /// Every task belonging to one goal, oldest first.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list_epic_tasks(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Vec<Task>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT {TASK_COLUMNS} FROM tasks
                 WHERE project_id = ?1 AND mini_project_id = ?2
                 ORDER BY created_at, id"
            ))
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), mini_project_id.to_string()])
            .map_err(backend)?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            tasks.push(read_task(row)?);
        }
        Ok(tasks)
    }

    /// The whole dependency graph of one project, as `task → depends on`.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn task_dependency_graph(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<BTreeMap<TaskId, BTreeSet<TaskId>>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT task_id, depends_on_task_id FROM task_dependencies
                 WHERE project_id = ?1 ORDER BY task_id, depends_on_task_id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut edges: BTreeMap<TaskId, BTreeSet<TaskId>> = BTreeMap::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let task = TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?;
            let dependency = TaskId::parse(&row.get::<_, String>(1).map_err(backend)?)?;
            edges.entry(task).or_default().insert(dependency);
        }
        Ok(edges)
    }

    /// Every external ticket link one task carries.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list_ticket_links(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Vec<TicketLink>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, project_id, task_id, connector, external_issue_key, revision, created_at
                 FROM jira_links WHERE project_id = ?1 AND task_id = ?2
                 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), task_id.to_string()])
            .map_err(backend)?;
        let mut links = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            links.push(read_ticket_link(row)?);
        }
        Ok(links)
    }

    /// Every execution authorization in a project, with its revocation.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list_authorizations(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<StoredAuthorization>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT a.id, a.scope_kind, a.scope_mini_project_id, a.scope_task_id,
                        a.allowed_start, a.allowed_end, a.max_concurrency,
                        a.max_tokens, a.max_commands, a.max_duration_seconds,
                        a.max_cost_minor_units, a.cost_currency,
                        a.created_by, a.capability_receipt_id, a.created_at,
                        r.revoked_at, r.revoked_by, r.revocation_receipt_id, r.reason
                 FROM execution_authorizations a
                 LEFT JOIN execution_authorization_revocations r
                   ON r.project_id = a.project_id AND r.authorization_id = a.id
                 WHERE a.project_id = ?1
                 ORDER BY a.created_at, a.id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut authorizations = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let id = ExecutionAuthorizationId::parse(&row.get::<_, String>(0).map_err(backend)?)?;
            let scope = read_scope(
                &row.get::<_, String>(1).map_err(backend)?,
                row.get(2).map_err(backend)?,
                row.get(3).map_err(backend)?,
            )?;
            let selected_tasks = self.selected_tasks(project_id, id)?;
            let revoked_at: Option<String> = row.get(15).map_err(backend)?;
            let revocation = match revoked_at {
                None => None,
                Some(instant) => Some(AuthorizationRevocation {
                    revoked_at: read_timestamp(&instant)?,
                    revoked_by: AccountProfileId::parse(
                        &row.get::<_, String>(16).map_err(backend)?,
                    )?,
                    receipt: CommandReceiptId::parse(&row.get::<_, String>(17).map_err(backend)?)?,
                    reason: ExternalName::parse(&row.get::<_, String>(18).map_err(backend)?)?,
                }),
            };
            authorizations.push(StoredAuthorization {
                authorization: ExecutionAuthorization {
                    id,
                    project_id,
                    scope,
                    selected_tasks,
                    allowed_start: kontor_core::calendar::TimeRange {
                        start: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
                        end: read_timestamp(&row.get::<_, String>(5).map_err(backend)?)?,
                    },
                    max_concurrency: u32::try_from(row.get::<_, i64>(6).map_err(backend)?)
                        .map_err(|_| out_of_range("concurrency"))?,
                    budget: kontor_core::spec::BudgetBounds {
                        max_tokens: u64::try_from(row.get::<_, i64>(7).map_err(backend)?)
                            .map_err(|_| out_of_range("token bound"))?,
                        max_commands: u64::try_from(row.get::<_, i64>(8).map_err(backend)?)
                            .map_err(|_| out_of_range("command bound"))?,
                        max_duration_seconds: u64::try_from(row.get::<_, i64>(9).map_err(backend)?)
                            .map_err(|_| out_of_range("duration bound"))?,
                        max_cost: kontor_core::id::Money {
                            minor_units: u64::try_from(row.get::<_, i64>(10).map_err(backend)?)
                                .map_err(|_| out_of_range("cost bound"))?,
                            currency: kontor_core::id::CurrencyCode::parse(
                                &row.get::<_, String>(11).map_err(backend)?,
                            )?,
                        },
                    },
                    created_by: AccountProfileId::parse(
                        &row.get::<_, String>(12).map_err(backend)?,
                    )?,
                    capability_receipt: CommandReceiptId::parse(
                        &row.get::<_, String>(13).map_err(backend)?,
                    )?,
                    created_at: read_timestamp(&row.get::<_, String>(14).map_err(backend)?)?,
                },
                revocation,
            });
        }
        Ok(authorizations)
    }

    /// The tasks one authorization names explicitly, if it is not scope-wide.
    fn selected_tasks(
        &self,
        project_id: ProjectId,
        id: ExecutionAuthorizationId,
    ) -> RepositoryResult<Vec<TaskId>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT task_id FROM execution_authorization_tasks
                 WHERE project_id = ?1 AND authorization_id = ?2 ORDER BY task_id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), id.to_string()])
            .map_err(backend)?;
        let mut tasks = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            tasks.push(TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?);
        }
        Ok(tasks)
    }

    /// Bind an already-admitted run to the native session a runtime just issued.
    ///
    /// Admission has to commit *before* the first native effect — that is what
    /// makes "one seat, one session" hold across a crash — so the run exists
    /// unbound for the moment between the two. This is the second half: it
    /// attaches the identity the runtime handed back, and nothing else about the
    /// run changes.
    ///
    /// Presenting the identical identity again is a replay and writes nothing, so
    /// a lost answer costs a duplicate call and not a second binding. A
    /// *different* identity for the same run is refused: a run owns one session,
    /// and rebinding it would silently orphan the first.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] for an unknown run and
    /// [`RepositoryError::Conflict`] when the run is already bound to another
    /// native session.
    pub fn bind_agent_run(
        &self,
        project_id: ProjectId,
        agent_run_id: kontor_core::id::AgentRunId,
        binding: &kontor_core::repository::RuntimeBinding,
    ) -> RepositoryResult<Applied> {
        let transaction = self.begin()?;
        let known: Option<String> = transaction
            .query_row(
                "SELECT id FROM agent_runs WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), agent_run_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if known.is_none() {
            return Err(RepositoryError::NotFound {
                subject: "agent run",
            });
        }
        let existing: Option<(String, String, i64, String)> = transaction
            .query_row(
                "SELECT id, runtime_kind, generation, native_id FROM runtime_bindings
                 WHERE project_id = ?1 AND agent_run_id = ?2",
                params![project_id.to_string(), agent_run_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((id, runtime_kind, generation, native_id)) = existing {
            let same = id.as_str() == binding.id.to_string()
                && runtime_kind.as_str() == binding.identity.runtime_kind.as_str()
                && u64::try_from(generation).unwrap_or(u64::MAX) == binding.identity.generation
                && native_id.as_str() == binding.identity.native_id.as_str();
            if same {
                return Ok(Applied::Unchanged);
            }
            return Err(conflict(
                "runtime binding",
                "the run already owns a different native session",
            ));
        }
        transaction
            .execute(
                "INSERT INTO runtime_bindings
                     (id, project_id, agent_run_id, runtime_kind, host, generation,
                      native_id, bound_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    binding.id.to_string(),
                    project_id.to_string(),
                    agent_run_id.to_string(),
                    binding.identity.runtime_kind.as_str(),
                    binding.identity.host.as_str(),
                    i64::try_from(binding.identity.generation).map_err(|_| {
                        RepositoryError::Backend {
                            detail: "runtime generation exceeds the storable range".to_owned(),
                        }
                    })?,
                    binding.identity.native_id.as_str(),
                    text(binding.bound_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(Applied::Created)
    }

    /// Every team run serving one task, oldest first.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list_team_runs_for_task(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Vec<(kontor_core::id::TeamRunId, kontor_core::state::RunLifecycle)>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, lifecycle FROM team_runs
                 WHERE project_id = ?1 AND task_id = ?2 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), task_id.to_string()])
            .map_err(backend)?;
        let mut runs = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            runs.push((
                kontor_core::id::TeamRunId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                kontor_core::state::RunLifecycle::parse(
                    &row.get::<_, String>(1).map_err(backend)?,
                )?,
            ));
        }
        Ok(runs)
    }

    /// Every agent run inside one team run, oldest first.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list_agent_runs_for_team_run(
        &self,
        project_id: ProjectId,
        team_run_id: kontor_core::id::TeamRunId,
    ) -> RepositoryResult<Vec<SeatRow>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT run.id, run.role_key, binding.runtime_kind, binding.native_id, binding.id
                 FROM agent_runs AS run
                 LEFT JOIN runtime_bindings AS binding
                   ON binding.project_id = run.project_id AND binding.agent_run_id = run.id
                 WHERE run.project_id = ?1 AND run.team_run_id = ?2
                 ORDER BY run.created_at, run.id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), team_run_id.to_string()])
            .map_err(backend)?;
        let mut seats = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let runtime_kind: Option<String> = row.get(2).map_err(backend)?;
            let native_id: Option<String> = row.get(3).map_err(backend)?;
            let binding_id: Option<String> = row.get(4).map_err(backend)?;
            seats.push(SeatRow {
                agent_run_id: kontor_core::id::AgentRunId::parse(
                    &row.get::<_, String>(0).map_err(backend)?,
                )?,
                role: kontor_core::id::RoleKey::parse(&row.get::<_, String>(1).map_err(backend)?)?,
                runtime_kind: runtime_kind
                    .as_deref()
                    .map(kontor_core::id::RuntimeKindKey::parse)
                    .transpose()?,
                native_id: native_id
                    .as_deref()
                    .map(kontor_core::id::ExternalId::parse)
                    .transpose()?,
                binding_id: binding_id
                    .as_deref()
                    .map(kontor_core::id::RuntimeBindingId::parse)
                    .transpose()?,
            });
        }
        Ok(seats)
    }

    /// Replace a task's active work profile with another, before any run froze
    /// it.
    ///
    /// The old workflow is deactivated rather than deleted — it is the record of
    /// what the task was going to be judged against, and a correction is a new
    /// fact rather than an erasure of the old one. The unique index on
    /// `(project_id, task_id) WHERE active` is what makes "exactly one active
    /// workflow" hold across the swap, and it is why both writes are in one
    /// transaction.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] for an unknown task and the
    /// domain's own refusal when the profile snapshot does not verify.
    pub fn replace_task_workflow(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        request: &kontor_core::repository::NewTaskWorkflow,
        definition: &WorkProfileSpec,
        team: Option<&TeamTemplateRevision>,
    ) -> RepositoryResult<TaskWorkflowId> {
        request.snapshot.verify()?;
        let transaction = self.begin()?;
        let known: Option<String> = transaction
            .query_row(
                "SELECT id FROM tasks WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if known.is_none() {
            return Err(RepositoryError::NotFound { subject: "task" });
        }
        // The revisions the new pin names have to be stored before the pin can
        // point at them, exactly as they do for an epic application.
        let application = EpicApplication {
            project_id,
            name: ExternalName::parse("selection").map_err(RepositoryError::Domain)?,
            tasks: &[],
            profile: &request.snapshot,
            definition,
            team,
            applied_at: request.created_at,
        };
        store_specifications(&transaction, &application)?;
        transaction
            .execute(
                "UPDATE task_workflows SET active = 0
                 WHERE project_id = ?1 AND task_id = ?2 AND active = 1",
                params![project_id.to_string(), task_id.to_string()],
            )
            .map_err(backend)?;
        let document = kontor_core::id::CanonicalDocument::from_serializable(&request.snapshot)?;
        transaction
            .execute(
                "INSERT INTO task_workflows
                     (id, project_id, task_id, profile_key, profile_version, snapshot,
                      snapshot_hash, current_phase, active, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 1, ?9)",
                params![
                    request.id.to_string(),
                    project_id.to_string(),
                    task_id.to_string(),
                    definition.id.as_str(),
                    version_column(definition.version),
                    document.json(),
                    document.hash().as_str(),
                    request.current_phase.as_str(),
                    text(request.created_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(request.id)
    }

    /// Pin, or re-pin, the provider account a task will run under.
    ///
    /// Replaceable on purpose: it is a decision made *before* a run exists, and
    /// the run's own column is what records what actually happened. Presenting
    /// the same profile and revision again writes nothing.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] for an unknown task or profile.
    pub fn set_task_account_selection(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        account_profile_id: AccountProfileId,
        account_revision: AggregateRevision,
    ) -> RepositoryResult<Applied> {
        let transaction = self.begin()?;
        let existing: Option<(String, i64)> = transaction
            .query_row(
                "SELECT account_profile_id, account_revision FROM task_account_selections
                 WHERE project_id = ?1 AND task_id = ?2",
                params![project_id.to_string(), task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((pinned, revision)) = &existing
            && pinned.as_str() == account_profile_id.to_string()
            && revision_of(*revision)? == account_revision
        {
            return Ok(Applied::Unchanged);
        }
        let selected_at = text(Timestamp::now());
        let revision = crate::repository::revision_column(account_revision)?;
        if existing.is_some() {
            transaction
                .execute(
                    "UPDATE task_account_selections
                     SET account_profile_id = ?3, account_revision = ?4, selected_at = ?5
                     WHERE project_id = ?1 AND task_id = ?2",
                    params![
                        project_id.to_string(),
                        task_id.to_string(),
                        account_profile_id.to_string(),
                        revision,
                        selected_at
                    ],
                )
                .map_err(backend)?;
        } else {
            transaction
                .execute(
                    "INSERT INTO task_account_selections
                         (project_id, task_id, account_profile_id, account_revision, selected_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        project_id.to_string(),
                        task_id.to_string(),
                        account_profile_id.to_string(),
                        revision,
                        selected_at
                    ],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;
        Ok(Applied::Created)
    }

    /// The provider account one task is pinned to, if it has been selected.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn task_account_selection(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<(AccountProfileId, AggregateRevision)>> {
        self.connection
            .query_row(
                "SELECT account_profile_id, account_revision FROM task_account_selections
                 WHERE project_id = ?1 AND task_id = ?2",
                params![project_id.to_string(), task_id.to_string()],
                |row| Ok((row.get::<_, String>(0), row.get::<_, i64>(1))),
            )
            .optional()
            .map_err(backend)?
            .map(|(id, revision)| {
                Ok((
                    AccountProfileId::parse(&id.map_err(backend)?)?,
                    revision_of(revision.map_err(backend)?)?,
                ))
            })
            .transpose()
    }

    /// Revoke one authorization, appending the evidence that says who and why.
    ///
    /// The row itself is never touched: an authorization is what was granted, and
    /// a revocation is a second fact about it. Revoking twice is refused by the
    /// child table's primary key, so a replayed disarm cannot record two.
    ///
    /// # Errors
    /// Returns [`RepositoryError::NotFound`] for an unknown authorization and
    /// [`RepositoryError::Conflict`] when it has already been revoked.
    pub fn revoke_authorization(
        &self,
        project_id: ProjectId,
        id: ExecutionAuthorizationId,
        revocation: &AuthorizationRevocation,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let known: Option<String> = transaction
            .query_row(
                "SELECT id FROM execution_authorizations WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if known.is_none() {
            return Err(RepositoryError::NotFound {
                subject: "execution authorization",
            });
        }
        let already: Option<String> = transaction
            .query_row(
                "SELECT authorization_id FROM execution_authorization_revocations
                 WHERE project_id = ?1 AND authorization_id = ?2",
                params![project_id.to_string(), id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if already.is_some() {
            return Err(conflict(
                "execution authorization",
                "has already been revoked",
            ));
        }
        transaction
            .execute(
                "INSERT INTO execution_authorization_revocations
                     (project_id, authorization_id, revoked_at, revoked_by,
                      revocation_receipt_id, reason)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    project_id.to_string(),
                    id.to_string(),
                    text(revocation.revoked_at),
                    revocation.revoked_by.to_string(),
                    revocation.receipt.to_string(),
                    revocation.reason.as_str()
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Row readers
// ---------------------------------------------------------------------------

fn read_mini_project(row: &rusqlite::Row<'_>) -> RepositoryResult<MiniProject> {
    Ok(MiniProject {
        id: MiniProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        name: ExternalName::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        revision: revision_of(row.get::<_, i64>(3).map_err(backend)?)?,
        created_at: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
    })
}

fn read_ticket_link(row: &rusqlite::Row<'_>) -> RepositoryResult<TicketLink> {
    Ok(TicketLink {
        id: TicketLinkId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        task_id: TaskId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        connector: ConnectorKey::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        external_issue_key: ExternalId::parse(&row.get::<_, String>(4).map_err(backend)?)?,
        revision: revision_of(row.get::<_, i64>(5).map_err(backend)?)?,
        created_at: read_timestamp(&row.get::<_, String>(6).map_err(backend)?)?,
    })
}

fn out_of_range(subject: &'static str) -> RepositoryError {
    RepositoryError::Backend {
        detail: format!("stored {subject} is out of range"),
    }
}

// ---------------------------------------------------------------------------
// Application steps, all inside one transaction
// ---------------------------------------------------------------------------

/// Refuse two tasks stated under the same title.
///
/// The title is the natural identity, so two of them would make "the task called
/// X" ambiguous — and a dependency naming X would silently pick one.
fn ensure_titles_unique(tasks: &[EpicTask]) -> RepositoryResult<()> {
    let mut seen = BTreeSet::new();
    for task in tasks {
        if !seen.insert(&task.title) {
            return Err(DomainError::invalid(
                "epic task",
                "two tasks are stated under the same title",
            )
            .into());
        }
    }
    Ok(())
}

/// Refuse a dependency on a title this epic does not state.
///
/// Checked before anything is written as well as while the edges are resolved,
/// because a dangling edge is a defect in the request rather than a race, and
/// saying so before the transaction opens is the cheaper answer.
fn ensure_dependencies_named(tasks: &[EpicTask]) -> RepositoryResult<()> {
    let titles: BTreeSet<&ExternalName> = tasks.iter().map(|task| &task.title).collect();
    for task in tasks {
        for dependency in &task.depends_on {
            if dependency == &task.title {
                return Err(DomainError::invalid(
                    "task dependency",
                    "a task must not depend on itself",
                )
                .into());
            }
            if !titles.contains(dependency) {
                return Err(DomainError::invalid(
                    "task dependency",
                    "names a task this epic does not state",
                )
                .into());
            }
        }
    }
    Ok(())
}

fn ensure_project_exists(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
) -> RepositoryResult<()> {
    let found: Option<String> = transaction
        .query_row(
            "SELECT id FROM projects WHERE id = ?1",
            params![project_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    found
        .map(|_| ())
        .ok_or(RepositoryError::NotFound { subject: "project" })
}

/// Store the profile and team revisions the epic pins, if they are not stored.
///
/// Both tables are insert-only, so a revision that is already there is left
/// alone and one that differs at the same `(id, version)` is refused by the
/// unique index — which is the drift check this needs and could not do better
/// itself.
fn store_specifications(
    transaction: &Transaction<'_>,
    request: &EpicApplication<'_>,
) -> RepositoryResult<()> {
    let document = request.definition.canonicalize()?;
    let stored: Option<String> = transaction
        .query_row(
            "SELECT definition_hash FROM work_profiles
             WHERE project_id = ?1 AND profile_key = ?2 AND version = ?3",
            params![
                request.project_id.to_string(),
                request.definition.id.as_str(),
                version_column(request.definition.version)
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    match stored {
        Some(hash) if hash.as_str() == document.hash().as_str() => {}
        Some(_) => {
            return Err(conflict(
                "work profile",
                "that revision is already stored with different content",
            ));
        }
        None => {
            transaction
                .execute(
                    "INSERT INTO work_profiles
                         (project_id, profile_key, version, definition, definition_hash,
                          created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        request.project_id.to_string(),
                        request.definition.id.as_str(),
                        version_column(request.definition.version),
                        document.json(),
                        document.hash().as_str(),
                        text(request.applied_at)
                    ],
                )
                .map_err(backend)?;
        }
    }

    let Some(team) = request.team else {
        return Ok(());
    };
    let stored: Option<String> = transaction
        .query_row(
            "SELECT definition_hash FROM team_templates
             WHERE project_id = ?1 AND template_id = ?2 AND version = ?3",
            params![
                request.project_id.to_string(),
                team.template_id.to_string(),
                version_column(team.version)
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    match stored {
        Some(hash) if hash.as_str() == team.definition.hash().as_str() => Ok(()),
        Some(_) => Err(conflict(
            "team template",
            "that revision is already stored with different content",
        )),
        None => {
            let authority = crate::repository::to_json(&team.role_authority)?;
            transaction
                .execute(
                    "INSERT INTO team_templates
                         (project_id, template_id, version, name, definition,
                          definition_hash, role_authority, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        request.project_id.to_string(),
                        team.template_id.to_string(),
                        version_column(team.version),
                        team.name.as_str(),
                        team.definition.json(),
                        team.definition.hash().as_str(),
                        authority,
                        text(request.applied_at)
                    ],
                )
                .map_err(backend)?;
            Ok(())
        }
    }
}

fn ensure_mini_project(
    transaction: &Transaction<'_>,
    request: &EpicApplication<'_>,
) -> RepositoryResult<(MiniProject, Applied)> {
    let existing: Option<MiniProject> = transaction
        .query_row(
            "SELECT id, project_id, name, revision, created_at
             FROM mini_projects WHERE project_id = ?1 AND name = ?2",
            params![request.project_id.to_string(), request.name.as_str()],
            |row| Ok(read_mini_project(row)),
        )
        .optional()
        .map_err(backend)?
        .transpose()?;
    if let Some(goal) = existing {
        return Ok((goal, Applied::Unchanged));
    }
    let id = MiniProjectId::generate();
    transaction
        .execute(
            "INSERT INTO mini_projects (id, project_id, name, revision, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)",
            params![
                id.to_string(),
                request.project_id.to_string(),
                request.name.as_str(),
                text(request.applied_at)
            ],
        )
        .map_err(backend)?;
    Ok((
        MiniProject {
            id,
            project_id: request.project_id,
            name: request.name.clone(),
            revision: AggregateRevision::INITIAL,
            created_at: request.applied_at,
        },
        Applied::Created,
    ))
}

fn ensure_task(
    transaction: &Transaction<'_>,
    request: &EpicApplication<'_>,
    mini_project_id: MiniProjectId,
    plan: &EpicTask,
) -> RepositoryResult<AppliedTask> {
    let existing: Option<Task> = transaction
        .query_row(
            &format!(
                "SELECT {TASK_COLUMNS} FROM tasks
                 WHERE project_id = ?1 AND mini_project_id = ?2 AND title = ?3"
            ),
            params![
                request.project_id.to_string(),
                mini_project_id.to_string(),
                plan.title.as_str()
            ],
            |row| Ok(read_task(row)),
        )
        .optional()
        .map_err(backend)?
        .transpose()?;

    let (task, applied) = match existing {
        Some(task) => {
            // The module is what the scheduler serializes work on, so changing it
            // under an existing task would change what that task contends for
            // without anything having transitioned.
            if task.module != plan.module {
                return Err(conflict(
                    "epic task",
                    "already exists in this epic contending for a different module",
                ));
            }
            (task, Applied::Unchanged)
        }
        None => {
            let id = TaskId::generate();
            transaction
                .execute(
                    "INSERT INTO tasks
                         (id, project_id, mini_project_id, title, module_key, state,
                          revision, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
                    params![
                        id.to_string(),
                        request.project_id.to_string(),
                        mini_project_id.to_string(),
                        plan.title.as_str(),
                        plan.module.as_ref().map(ModuleKey::as_str),
                        plan.state.as_str(),
                        text(request.applied_at)
                    ],
                )
                .map_err(backend)?;
            (
                Task {
                    id,
                    project_id: request.project_id,
                    mini_project_id: Some(mini_project_id),
                    title: plan.title.clone(),
                    module: plan.module.clone(),
                    state: plan.state,
                    revision: AggregateRevision::INITIAL,
                    created_at: request.applied_at,
                    updated_at: request.applied_at,
                },
                Applied::Created,
            )
        }
    };

    let workflow_id = ensure_workflow(transaction, request, task.id)?;
    Ok(AppliedTask {
        title: plan.title.clone(),
        task_id: task.id,
        applied,
        state: task.state,
        revision: task.revision,
        workflow_id,
        depends_on: BTreeSet::new(),
        links: Vec::new(),
    })
}

/// Freeze the epic's profile onto one task, or prove the frozen one matches.
///
/// A task's active workflow is what every gate, phase and closure check is
/// judged against, so an epic that re-applies with a different profile revision
/// is refused: reassigning it would silently re-grade work already done.
fn ensure_workflow(
    transaction: &Transaction<'_>,
    request: &EpicApplication<'_>,
    task_id: TaskId,
) -> RepositoryResult<TaskWorkflowId> {
    let existing: Option<(String, String, i64)> = transaction
        .query_row(
            "SELECT id, profile_key, profile_version FROM task_workflows
             WHERE project_id = ?1 AND task_id = ?2 AND active = 1",
            params![request.project_id.to_string(), task_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(backend)?;
    if let Some((id, profile, version)) = existing {
        if profile.as_str() != request.definition.id.as_str()
            || read_version(version)? != request.definition.version
        {
            return Err(conflict(
                "task workflow",
                "the task already pins a different work-profile revision",
            ));
        }
        return Ok(TaskWorkflowId::parse(&id)?);
    }

    let id = TaskWorkflowId::generate();
    let document = kontor_core::id::CanonicalDocument::from_serializable(request.profile)?;
    transaction
        .execute(
            "INSERT INTO task_workflows
                 (id, project_id, task_id, profile_key, profile_version, snapshot,
                  snapshot_hash, current_phase, active, revision, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 1, ?9)",
            params![
                id.to_string(),
                request.project_id.to_string(),
                task_id.to_string(),
                request.definition.id.as_str(),
                version_column(request.definition.version),
                document.json(),
                document.hash().as_str(),
                request.definition.entry_phase.as_str(),
                text(request.applied_at)
            ],
        )
        .map_err(backend)?;
    Ok(id)
}

fn write_dependencies(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
    depends_on: &BTreeSet<TaskId>,
) -> RepositoryResult<()> {
    transaction
        .execute(
            "DELETE FROM task_dependencies WHERE project_id = ?1 AND task_id = ?2",
            params![project_id.to_string(), task_id.to_string()],
        )
        .map_err(backend)?;
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
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
    }
    Ok(())
}

/// Prove the *whole project's* graph is still acyclic, inside the transaction.
///
/// The check is project-wide rather than epic-wide because a cycle can run
/// through a task an earlier epic created, and SQLite can enforce the pair and
/// the self-edge but never reachability.
fn ensure_acyclic(transaction: &Transaction<'_>, project_id: ProjectId) -> RepositoryResult<()> {
    let mut edges: BTreeMap<TaskId, BTreeSet<TaskId>> = BTreeMap::new();
    let mut statement = transaction
        .prepare("SELECT task_id, depends_on_task_id FROM task_dependencies WHERE project_id = ?1")
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string()])
        .map_err(backend)?;
    while let Some(row) = rows.next().map_err(backend)? {
        let task = TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?;
        let dependency = TaskId::parse(&row.get::<_, String>(1).map_err(backend)?)?;
        edges.entry(task).or_default().insert(dependency);
    }
    validate_dependency_graph(&edges)?;
    Ok(())
}

fn ensure_links(
    transaction: &Transaction<'_>,
    request: &EpicApplication<'_>,
    task_id: TaskId,
    plan: &EpicTask,
) -> RepositoryResult<Vec<AppliedLink>> {
    let mut applied = Vec::with_capacity(plan.ticket_links.len());
    let mut stated = BTreeSet::new();
    for link in &plan.ticket_links {
        if !stated.insert((&link.connector, &link.external_issue_key)) {
            return Err(conflict(
                "ticket link",
                "the same external issue is linked twice to one task",
            ));
        }
        // A link is unique per `(project, connector, issue)`, so a second task
        // claiming the same issue is refused — including one in another epic.
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT id, task_id FROM jira_links
                 WHERE project_id = ?1 AND connector = ?2 AND external_issue_key = ?3",
                params![
                    request.project_id.to_string(),
                    link.connector.as_str(),
                    link.external_issue_key.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((id, owner)) = existing {
            if TaskId::parse(&owner)? != task_id {
                return Err(conflict(
                    "ticket link",
                    "that external issue is already linked to another task",
                ));
            }
            applied.push(AppliedLink {
                id: TicketLinkId::parse(&id)?,
                connector: link.connector.clone(),
                external_issue_key: link.external_issue_key.clone(),
                applied: Applied::Unchanged,
            });
            continue;
        }
        let id = TicketLinkId::generate();
        transaction
            .execute(
                "INSERT INTO jira_links
                     (id, project_id, task_id, connector, external_issue_key, revision, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
                params![
                    id.to_string(),
                    request.project_id.to_string(),
                    task_id.to_string(),
                    link.connector.as_str(),
                    link.external_issue_key.as_str(),
                    text(request.applied_at)
                ],
            )
            .map_err(backend)?;
        applied.push(AppliedLink {
            id,
            connector: link.connector.clone(),
            external_issue_key: link.external_issue_key.clone(),
            applied: Applied::Created,
        });
    }
    Ok(applied)
}
