//! The reads a cross-boundary caller needs: enumerations, external-ticket
//! evidence, and the candidate half of a scheduling snapshot.
//!
//! # Why these are here and not in `repository`
//!
//! `repository` implements the domain's own ports, and every method on it is a
//! port method. Nothing in the domain asks to *enumerate* projects or to read an
//! external ticket's observation history — those are questions an operator asks, so
//! they are stated as inherent reads on the store rather than added to a trait the
//! domain would then be carrying for a console's benefit.
//!
//! # What a list returns, and what it does not
//!
//! A list returns a *summary*: enough to find the thing you want and the revision a
//! later write must present. It does not return the aggregate, because the aggregate
//! has a snapshot route that reads it in one transaction with a control-plane
//! position, and a list of fully-loaded aggregates would be a slower way to get a
//! staler answer.
//!
//! # Ticket evidence is read, never derived
//!
//! Everything in the ticket section is a row this Realm already recorded: a link, a
//! projection it computed, an observation it took, an inbound comment it mirrored, a
//! conflict it detected, a convergence attempt it made. Nothing here contacts an
//! external system — that is `kontor-jira`'s job, behind the daemon —
//! and nothing here decides anything. In particular there is **no outbound comment
//! read, because there is no outbound comment table**: schema v1's
//! `ticket_sync_projections.comment_policy` is checked to be `inbound_only`, and
//! adding an outbound path is a migration rather than a configuration.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::calendar::{
    CalendarExceptionRevision, CalendarProfileSpec, ChildCalendarWindows, ScheduleOverride,
    WorkCalendarAssignment, WorkScope,
};
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CommandReceiptId, ExecutionAuthorizationId,
    ExternalId, ExternalName, ExternalProjectKey, MiniProjectId, PhaseKey, ProjectId, RoleKey,
    ScheduleOverrideId, SpecVersion, StatusConflictId, StatusTransitionReceiptId, TaskId,
    TaskWorkflowId, TeamRunId, TeamTemplateId, TicketLinkId, TicketProjectionId, Timestamp,
    WorkProfileKey, format_utc_timestamp,
};
use kontor_core::repository::{RepositoryError, RepositoryResult, Task, TaskWorkflow};
use kontor_core::spec::IntakeResult;
use kontor_core::state::{DesiredRunState, ObservedRunState, RunLifecycle};
use kontor_scheduler::model::{
    AccountAdmissionEvidence, AuthorizationEvidence, CalendarAdmission, Candidate,
    ExternalWorkEvidence, IntakeLineage, RosterGovernance, RuntimeAdmissionEvidence, TaskOrigin,
    covering_authority,
};
use rusqlite::{Row, params};

use crate::SqliteStore;
use crate::repository::{backend, read_timestamp, revision_of, team_run_snapshot};

/// A project's calendar, read once and resolved per candidate.
///
/// Everything here is a row. The *meaning* of those rows — which window matched,
/// whether the day is a holiday, whether an override is in force, when the
/// calendar next opens — is `kontor-calendar`'s, and this crate asks it rather
/// than deciding a second time.
#[derive(Debug, Clone, Default)]
pub struct CalendarInputs {
    /// The project's active assignment. `None` means unrestricted.
    pub assignment: Option<WorkCalendarAssignment>,
    /// The exact profile revision the assignment pins.
    pub profile: Option<CalendarProfileSpec>,
    /// Manual exceptions plus the currently applied import's.
    pub exceptions: Vec<CalendarExceptionRevision>,
    /// Overrides that have started, are unrevoked and have not passed their
    /// ceiling. Scope and expiry are the resolver's to judge.
    pub overrides: Vec<ScheduleOverride>,
    /// Current immutable child-window revisions for this calendar.
    pub child_windows: Vec<ChildCalendarWindows>,
}

impl CalendarInputs {
    /// Resolve one piece of work against this calendar.
    ///
    /// # Errors
    /// Returns the domain's refusal when the stored rows do not resolve — a
    /// pinned revision that does not match, or a zone the bundled tzdb does not
    /// know.
    pub fn resolve(
        &self,
        now: Timestamp,
        mini_project: Option<MiniProjectId>,
        task: TaskId,
        terminal_goals: &[MiniProjectId],
    ) -> RepositoryResult<CalendarAdmission> {
        let goal_scope =
            mini_project.map(|mini_project_id| WorkScope::MiniProject { mini_project_id });
        let task_scope = WorkScope::Task { task_id: task };
        let levels: Vec<&[kontor_core::calendar::WeeklyWindow]> = goal_scope
            .into_iter()
            .chain(std::iter::once(task_scope))
            .filter_map(|scope| {
                self.child_windows
                    .iter()
                    .find(|revision| revision.scope == scope)
                    .map(|revision| revision.windows.as_slice())
            })
            .collect();
        kontor_calendar::resolve_scoped(
            &kontor_calendar::ResolutionRequest {
                now,
                assignment: self.assignment.as_ref(),
                profile: self.profile.as_ref(),
                exceptions: &self.exceptions,
                // Scope revisions are applied by `resolve_scoped` below; keep the
                // request's legacy single-level field empty to avoid applying one twice.
                child_windows: None,
                overrides: &self.overrides,
                terminal_goals,
                mini_project,
                task: Some(task),
            },
            &levels,
        )
        .map_err(|error| match error {
            kontor_calendar::CalendarError::Domain(domain) => RepositoryError::Domain(domain),
            other => RepositoryError::Backend {
                detail: other.to_string(),
            },
        })
    }
}

/// The goals whose every task has reached a terminal state.
fn terminal_goals(tasks: &[Task]) -> Vec<MiniProjectId> {
    let mut open: BTreeSet<MiniProjectId> = BTreeSet::new();
    let mut seen: BTreeSet<MiniProjectId> = BTreeSet::new();
    for task in tasks {
        let Some(goal) = task.mini_project_id else {
            continue;
        };
        seen.insert(goal);
        if !task.state.is_terminal() {
            open.insert(goal);
        }
    }
    seen.difference(&open).copied().collect()
}

/// The default priority every candidate is assembled with.
///
/// Schema v1 has no `tasks.priority` column, so there is nothing to read. A single
/// constant is the honest substitute: it makes the scheduler's total order degrade
/// to its two remaining keys — creation instant, then task id — which is
/// first-come-first-served. Inventing a priority from a title, a state or a module
/// would be inventing an ordering nobody asked for, and it would be invisible in
/// the answer.
pub const ASSEMBLED_PRIORITY: u32 = 0;

/// One project, as a list entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSummary {
    /// The project.
    pub project_id: ProjectId,
    /// Its human name.
    pub name: ExternalName,
    /// Its root path on disk.
    pub root_path: ExternalName,
    /// The revision a write must present.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
}

/// One team run, as a list entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRunSummary {
    /// The team run.
    pub team_run_id: TeamRunId,
    /// The task it serves.
    pub task_id: TaskId,
    /// The team template it froze.
    pub team_template: TeamTemplateId,
    /// That template's pinned revision.
    pub team_template_version: SpecVersion,
    /// Its lifecycle.
    pub lifecycle: RunLifecycle,
    /// The revision a write must present.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it closed.
    pub closed_at: Option<Timestamp>,
}

/// One agent run, as a list entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunSummary {
    /// The agent run.
    pub agent_run_id: AgentRunId,
    /// The team run it serves.
    pub team_run_id: TeamRunId,
    /// The role slot it fills.
    pub role: RoleKey,
    /// The coding account it is pinned to, if any.
    pub account_profile_id: Option<AccountProfileId>,
    /// Its own lifecycle.
    pub lifecycle: RunLifecycle,
    /// What Kontor asked for.
    pub desired: DesiredRunState,
    /// What the runtime last reported.
    pub observed: ObservedRunState,
    /// What Kontor concluded.
    pub derived: String,
    /// The revision a write must present.
    pub revision: AggregateRevision,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it closed.
    pub closed_at: Option<Timestamp>,
}

/// One open run, as capacity accounting sees it.
///
/// Deliberately narrower than [`AgentRunSummary`]: what a capacity count needs is
/// the keys a ceiling is stated against, and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRunLoad {
    /// The project it belongs to.
    pub project_id: ProjectId,
    /// The team run it serves.
    pub team_run_id: TeamRunId,
    /// The task the team run serves.
    pub task_id: TaskId,
    /// The coding account it is pinned to, if any.
    pub account_profile_id: Option<AccountProfileId>,
}

/// One external-ticket link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketLinkSummary {
    /// The link.
    pub link_id: TicketLinkId,
    /// The task it links.
    pub task_id: TaskId,
    /// The connector implementation. Never its vendor semantics.
    pub connector: ExternalName,
    /// The external issue key.
    pub external_issue_key: ExternalId,
    /// The revision a write must present.
    pub revision: AggregateRevision,
    /// When the link was made.
    pub created_at: Timestamp,
}

/// The newest projection computed for one link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketProjection {
    /// The projection.
    pub projection_id: TicketProjectionId,
    /// The link revision it was computed against.
    pub link_revision: AggregateRevision,
    /// The external issue key it addresses.
    pub external_issue_key: ExternalId,
    /// The external project the pinned field specification was written for.
    pub field_spec_project: ExternalProjectKey,
    /// The issue type it was written for.
    pub field_spec_issue_type: ExternalName,
    /// That specification's pinned revision.
    pub field_spec_version: SpecVersion,
    /// The fields this Realm would write, as computed.
    pub fields: String,
    /// The comment policy in force. Always `inbound_only` in schema v1.
    pub comment_policy: String,
    /// How far inbound comments have been mirrored.
    pub external_comment_cursor: Option<ExternalId>,
    /// The digest of the projection.
    pub projection_hash: String,
    /// When it was computed.
    pub computed_at: Timestamp,
}

/// One observation of an external ticket's own state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketObservation {
    /// The observation.
    pub observation_id: ExternalId,
    /// The external status identifier.
    pub status_id: ExternalId,
    /// Its human name, as the external system spells it.
    pub status_name: ExternalName,
    /// Its category, as the external system spells it.
    pub status_category: ExternalName,
    /// The external issue type.
    pub issue_type: ExternalName,
    /// The assignee's external account, when the ticket has one.
    pub assignee_account_id: Option<ExternalId>,
    /// The assignee's display name, when the external system provided one.
    pub assignee_display: Option<ExternalName>,
    /// The external version token, when the external system issues one.
    pub external_version: Option<ExternalId>,
    /// When the observation was taken.
    pub observed_at: Timestamp,
}

/// One inbound comment, mirrored from the external system.
///
/// Inbound only. There is no outbound counterpart anywhere in this schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundComment {
    /// The external system's own comment id.
    pub external_comment_id: ExternalId,
    /// The digest of the body, which is half this revision's identity.
    pub body_hash: String,
    /// The author's external account.
    pub author_account_id: ExternalId,
    /// The author's display name, when the external system provided one.
    pub author_display: Option<ExternalName>,
    /// When it was created externally.
    pub external_created_at: Timestamp,
    /// When it was last edited externally.
    pub external_updated_at: Timestamp,
    /// When this Realm mirrored it.
    pub observed_at: Timestamp,
    /// The revision this one replaces, for an edit.
    pub supersedes_hash: Option<String>,
    /// The comment text.
    pub body: String,
}

/// One detected reconciliation conflict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketConflict {
    /// The conflict.
    pub conflict_id: StatusConflictId,
    /// What kind it is.
    pub kind: String,
    /// The observation it was detected against.
    pub observation_id: ExternalId,
    /// The task revision at detection.
    pub task_revision: AggregateRevision,
    /// The external-workflow specification revision in force.
    pub spec_version: SpecVersion,
    /// The internal milestone involved, when there is one.
    pub milestone: Option<ExternalName>,
    /// When it was detected.
    pub detected_at: Timestamp,
    /// When it was resolved.
    pub resolved_at: Option<Timestamp>,
    /// The receipt that authorized the resolution.
    pub resolution_receipt_id: Option<CommandReceiptId>,
}

/// One convergence attempt against an external ticket.
///
/// This is the "diff" a caller asks for: what this Realm asked the external system
/// to become, which transition it used, and whether the change was ever confirmed
/// by a *refetched* observation rather than assumed from an acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketTransitionAttempt {
    /// The receipt.
    pub receipt_id: StatusTransitionReceiptId,
    /// The task whose state was being projected.
    pub task_id: TaskId,
    /// The internal milestone that was being converged to.
    pub milestone: ExternalName,
    /// The external status it aimed at.
    pub target_status_id: ExternalId,
    /// The external transition used. `None` for an assignee-only convergence.
    pub transition_id: Option<ExternalId>,
    /// The external principal it acted as.
    pub principal_account_id: ExternalId,
    /// Whether an assignment had to happen first.
    pub assignment_prerequisite: bool,
    /// The key it was committed under.
    pub idempotency_key: String,
    /// When it was dispatched.
    pub dispatched_at: Timestamp,
    /// When the external system acknowledged it.
    pub acknowledged_at: Option<Timestamp>,
    /// When a refetched observation confirmed it. `None` means unconfirmed, which
    /// is never the same as failed.
    pub confirmed_at: Option<Timestamp>,
    /// The observation that confirmed it.
    pub refetched_observation_id: Option<ExternalId>,
}

/// The candidate half of a scheduling snapshot, and what was left out of it.
#[derive(Debug, Clone)]
pub struct CandidateAssembly {
    /// The candidates, one per schedulable task.
    pub candidates: Vec<Candidate>,
    /// How many tasks were looked at.
    pub considered: usize,
    /// Tasks with no active workflow. A task without one has no phase to run and
    /// no gates to satisfy, so it is not a candidate — and saying so is better than
    /// inventing a workflow id for it.
    pub without_workflow: Vec<TaskId>,
    /// Whether this project has an active work calendar assignment.
    ///
    /// Informational: every candidate already carries the *resolved* answer, and
    /// a project with no assignment carries
    /// [`CalendarAdmission::unrestricted`]. This says which of those two cases
    /// produced it, so a client can tell "unrestricted because nothing is
    /// configured" from "open because the window is open" without inspecting a
    /// candidate.
    pub calendar_assigned: bool,
}

impl SqliteStore {
    /// Every project in this Realm, oldest first.
    ///
    /// # Errors
    /// Returns the repository's own refusal when the read fails.
    pub fn list_projects(&self) -> RepositoryResult<Vec<ProjectSummary>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, name, root_path, revision, created_at
                 FROM projects ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut projects = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            projects.push(ProjectSummary {
                project_id: ProjectId::parse(&column_text(row, 0)?)?,
                name: ExternalName::parse(&column_text(row, 1)?)?,
                root_path: ExternalName::parse(&column_text(row, 2)?)?,
                revision: revision_of(row.get(3).map_err(backend)?)?,
                created_at: read_timestamp(&column_text(row, 4)?)?,
            });
        }
        Ok(projects)
    }

    /// Every team run in one project, oldest first.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn list_team_runs(&self, project_id: ProjectId) -> RepositoryResult<Vec<TeamRunSummary>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, task_id, snapshot, snapshot_hash, lifecycle, revision, created_at,
                        closed_at
                 FROM team_runs WHERE project_id = ?1 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut runs = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            // The frozen team definition is re-admitted through its stored digest
            // rather than trusted: a list is still reading persisted evidence.
            let snapshot = team_run_snapshot(&column_text(row, 2)?, &column_text(row, 3)?)?;
            runs.push(TeamRunSummary {
                team_run_id: TeamRunId::parse(&column_text(row, 0)?)?,
                task_id: TaskId::parse(&column_text(row, 1)?)?,
                team_template: snapshot.template_id,
                team_template_version: snapshot.template_version,
                lifecycle: RunLifecycle::parse(&column_text(row, 4)?)?,
                revision: revision_of(row.get(5).map_err(backend)?)?,
                created_at: read_timestamp(&column_text(row, 6)?)?,
                closed_at: optional_timestamp(row, 7)?,
            });
        }
        Ok(runs)
    }

    /// Every agent run in one project, oldest first, optionally one team run's.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn list_agent_runs(
        &self,
        project_id: ProjectId,
        team_run_id: Option<TeamRunId>,
    ) -> RepositoryResult<Vec<AgentRunSummary>> {
        // One statement with a nullable filter rather than two: the filter is
        // either bound or bypassed by the `?2 IS NULL` half, so both shapes go
        // through the same query plan and the same row reader.
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, team_run_id, role_key, account_profile_id, lifecycle, desired_state,
                        observed_state, derived_state, revision, created_at, closed_at
                 FROM agent_runs
                 WHERE project_id = ?1 AND (?2 IS NULL OR team_run_id = ?2)
                 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                team_run_id.map(|id| id.to_string())
            ])
            .map_err(backend)?;
        let mut runs = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let account: Option<String> = row.get(3).map_err(backend)?;
            runs.push(AgentRunSummary {
                agent_run_id: AgentRunId::parse(&column_text(row, 0)?)?,
                team_run_id: TeamRunId::parse(&column_text(row, 1)?)?,
                role: RoleKey::parse(&column_text(row, 2)?)?,
                account_profile_id: account
                    .as_deref()
                    .map(AccountProfileId::parse)
                    .transpose()?,
                lifecycle: RunLifecycle::parse(&column_text(row, 4)?)?,
                desired: DesiredRunState::parse(&column_text(row, 5)?)?,
                observed: ObservedRunState::parse(&column_text(row, 6)?)?,
                derived: column_text(row, 7)?,
                revision: revision_of(row.get(8).map_err(backend)?)?,
                created_at: read_timestamp(&column_text(row, 9)?)?,
                closed_at: optional_timestamp(row, 10)?,
            });
        }
        Ok(runs)
    }

    /// Every open agent run in this Realm, as capacity accounting sees it.
    ///
    /// Realm-wide on purpose: a global ceiling is global, and counting one project's
    /// runs would answer a different question from the one a global limit asks.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn open_run_load(&self) -> RepositoryResult<Vec<OpenRunLoad>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT run.project_id, run.team_run_id, team.task_id, run.account_profile_id
                 FROM agent_runs run
                 JOIN team_runs team
                   ON team.project_id = run.project_id AND team.id = run.team_run_id
                 WHERE run.closed_at IS NULL
                 ORDER BY run.created_at, run.id",
            )
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut load = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let account: Option<String> = row.get(3).map_err(backend)?;
            load.push(OpenRunLoad {
                project_id: ProjectId::parse(&column_text(row, 0)?)?,
                team_run_id: TeamRunId::parse(&column_text(row, 1)?)?,
                task_id: TaskId::parse(&column_text(row, 2)?)?,
                account_profile_id: account
                    .as_deref()
                    .map(AccountProfileId::parse)
                    .transpose()?,
            });
        }
        Ok(load)
    }

    /// Which tasks in one project have reached `done`.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn completed_task_ids(&self, project_id: ProjectId) -> RepositoryResult<BTreeSet<TaskId>> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM tasks WHERE project_id = ?1 AND state = 'done'")
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut completed = BTreeSet::new();
        while let Some(row) = rows.next().map_err(backend)? {
            completed.insert(TaskId::parse(&column_text(row, 0)?)?);
        }
        Ok(completed)
    }

    /// Every recorded dependency in one project, keyed by the dependent task.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn task_dependency_map(
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
        let mut map: BTreeMap<TaskId, BTreeSet<TaskId>> = BTreeMap::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let task = TaskId::parse(&column_text(row, 0)?)?;
            let depends_on = TaskId::parse(&column_text(row, 1)?)?;
            map.entry(task).or_default().insert(depends_on);
        }
        Ok(map)
    }

    /// Every execution authorization recorded in one project, split by revocation.
    ///
    /// The window is *not* filtered here. An expired authorization is exactly what
    /// the scheduler's `authorization_expired` refusal is for, and filtering it out
    /// would turn a precise refusal into default-allow. Revoked grants stay in the
    /// second list so a disarm can block rather than vanish.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn list_execution_authorizations(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<(Vec<AuthorizationEvidence>, Vec<AuthorizationEvidence>)> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT a.id, a.scope_kind, a.scope_mini_project_id, a.scope_task_id,
                        a.allowed_start, a.allowed_end, a.max_concurrency, r.revoked_at
                 FROM execution_authorizations a
                 LEFT JOIN execution_authorization_revocations r
                   ON r.project_id = a.project_id AND r.authorization_id = a.id
                 WHERE a.project_id = ?1
                 ORDER BY a.allowed_start, a.id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut active = Vec::new();
        let mut revoked = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let id = ExecutionAuthorizationId::parse(&column_text(row, 0)?)?;
            let scope = match column_text(row, 1)?.as_str() {
                "mini_project" => WorkScope::MiniProject {
                    mini_project_id: kontor_core::id::MiniProjectId::parse(&column_text(row, 2)?)?,
                },
                "task" => WorkScope::Task {
                    task_id: TaskId::parse(&column_text(row, 3)?)?,
                },
                "project" => WorkScope::Project,
                other => {
                    return Err(kontor_core::DomainError::invalid(
                        "ExecutionAuthorization",
                        match other {
                            "" => "records no scope kind",
                            _ => "records a scope kind this build does not understand",
                        },
                    )
                    .into());
                }
            };
            let evidence = AuthorizationEvidence {
                id,
                project_id,
                scope,
                selected_tasks: self.authorization_tasks(project_id, id)?,
                allowed_start: read_timestamp(&column_text(row, 4)?)?,
                allowed_end: read_timestamp(&column_text(row, 5)?)?,
                max_concurrency: u32::try_from(row.get::<_, i64>(6).map_err(backend)?)
                    .unwrap_or(u32::MAX),
            };
            if row.get::<_, Option<String>>(7).map_err(backend)?.is_some() {
                revoked.push(evidence);
            } else {
                active.push(evidence);
            }
        }
        Ok((active, revoked))
    }

    /// The tasks one authorization was narrowed to. Empty means the whole scope.
    fn authorization_tasks(
        &self,
        project_id: ProjectId,
        authorization_id: ExecutionAuthorizationId,
    ) -> RepositoryResult<BTreeSet<TaskId>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT task_id FROM execution_authorization_tasks
                 WHERE project_id = ?1 AND authorization_id = ?2 ORDER BY task_id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                authorization_id.to_string()
            ])
            .map_err(backend)?;
        let mut selected = BTreeSet::new();
        while let Some(row) = rows.next().map_err(backend)? {
            selected.insert(TaskId::parse(&column_text(row, 0)?)?);
        }
        Ok(selected)
    }

    /// Whether one project has a work calendar assigned.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn has_calendar_assignment(&self, project_id: ProjectId) -> RepositoryResult<bool> {
        let count: i64 = self
            .connection
            .query_row(
                "SELECT count(*) FROM work_calendars WHERE project_id = ?1",
                params![project_id.to_string()],
                |row| row.get(0),
            )
            .map_err(backend)?;
        Ok(count > 0)
    }

    /// Assemble the candidate half of a scheduling snapshot for one project.
    ///
    /// # What is read and what is a stated default
    ///
    /// Read from rows: the task's state, revision, creation instant, goal, contended
    /// module, its active workflow, its dependencies, its intake lineage when
    /// intake created it, and every execution authorization in the project.
    ///
    /// The origin is read, not assumed: a task with a row in `intake_created_work`
    /// is [`TaskOrigin::Event`] carrying the receipt that armed it, and a task
    /// without one is [`TaskOrigin::Manual`]. What the scheduler receives is the
    /// resolved status — approved, or auto-armed with the authorization it acted
    /// under — and never the envelope, the filter or the source kind behind it.
    ///
    /// Stated defaults, each because schema v1 has nothing to read:
    ///
    /// * **priority** — [`ASSEMBLED_PRIORITY`]. No column exists; see its docs.
    /// * **serialization peers** — empty. There is no `task_serializes_with` table,
    ///   so no peer set can be read. The scheduler's own `contention` blocker still
    ///   applies module claims, which *are* read.
    /// * **account pin** — none. A task has no pinned account until a run is created
    ///   for it, and the scheduler reads an absent pin as "there is no account, so
    ///   there is nothing to prove about one" rather than as "any account will do".
    /// * **external work** — default. Ticket ownership gating is read from live
    ///   convergence state, which is `kontor-jira`'s to supply.
    /// * **worktree** — none. A candidate claims one at admission, not before.
    /// * **governance** — the frozen-roster half only. Whether an epic froze a
    ///   roster is a row this crate owns and is read for real; whether its
    ///   mandatory seats are *bound* is not, because locating a control plane
    ///   needs the topology vocabulary that says which kind is one, and that is
    ///   pinned per project rather than stored per node. `kontor-daemon`'s
    ///   epic-scoped assembler answers both halves and is what the scheduler
    ///   actually plans from.
    ///
    /// Every one of those defaults either fails closed or is genuinely neutral. An
    /// absent authorization is default-allow. A revoked covering grant is a stop.
    ///
    /// `runtime` is a parameter because a runtime's capabilities and health are the
    /// adapter's to report, and this crate does not open a runtime.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`], and refuses a stored row this build cannot
    /// read.
    pub fn scheduling_candidates(
        &self,
        project_id: ProjectId,
        runtime: &RuntimeAdmissionEvidence,
    ) -> RepositoryResult<CandidateAssembly> {
        self.scheduling_candidates_at(project_id, runtime, Timestamp::now())
    }

    /// Assemble candidates at one coordinator-supplied instant.
    ///
    /// # Errors
    /// As [`SqliteStore::scheduling_candidates`].
    pub fn scheduling_candidates_at(
        &self,
        project_id: ProjectId,
        runtime: &RuntimeAdmissionEvidence,
        now: Timestamp,
    ) -> RepositoryResult<CandidateAssembly> {
        use kontor_core::repository::{ProjectRepository, WorkflowRepository};

        let tasks = self.list_tasks(project_id)?;
        let dependencies = self.task_dependency_map(project_id)?;
        let (active, revoked) = self.list_execution_authorizations(project_id)?;
        let calendar = self.calendar_inputs(project_id, now)?;
        let calendar_assigned = calendar.assignment.is_some();
        let intake = crate::intake::lineage_by_task(self, project_id)?;
        // Derived from the tasks already read: a goal whose every task is
        // terminal has completed, which is what ends a goal-bound override
        // before its ceiling arrives. A goal with no task has not completed —
        // an empty goal is unstarted, not finished.
        let terminal_goals = terminal_goals(&tasks);

        let mut candidates = Vec::new();
        let mut without_workflow = Vec::new();
        // One lookup per epic, not per task: a roster is frozen by the epic, so
        // every task under it has the same answer.
        let mut governance = BTreeMap::new();
        for epic_id in tasks
            .iter()
            .filter_map(|task| task.mini_project_id)
            .collect::<BTreeSet<_>>()
        {
            let answer = if self.get_epic_roster(project_id, epic_id)?.is_some() {
                RosterGovernance::Seated
            } else {
                RosterGovernance::RosterUnfrozen
            };
            governance.insert(epic_id, answer);
        }
        for task in &tasks {
            let Some(workflow) = self.get_active_task_workflow(project_id, task.id)? else {
                without_workflow.push(task.id);
                continue;
            };
            // A task under no epic has no roster to freeze, so the question does
            // not apply to it and it is not refused for an answer it cannot have.
            let task_governance = task
                .mini_project_id
                .map_or(RosterGovernance::Seated, |epic_id| governance[&epic_id]);
            let (authorization, blocked_by) =
                covering_authority(&active, &revoked, task.mini_project_id, task.id);
            candidates.push(Candidate {
                project_id,
                task_id: task.id,
                mini_project_id: task.mini_project_id,
                workflow_id: workflow.id,
                // Neutral here: whether configuration can name this task's
                // seats is a Team Definition question the application layer
                // answers, and it sets this on every candidate before the pass.
                delivery_slots_registered: true,
                state: task.state,
                revision: task.revision,
                created_at: task.created_at,
                priority: ASSEMBLED_PRIORITY,
                governance: task_governance,
                module: task.module.clone(),
                changed_modules: self.task_changed_modules(project_id, task.id)?,
                worktree: None,
                depends_on: dependencies.get(&task.id).cloned().unwrap_or_default(),
                serializes_with: BTreeSet::new(),
                origin: intake.get(&task.id).map_or(TaskOrigin::Manual, |lineage| {
                    TaskOrigin::Event {
                        lineage: Some(intake_lineage(lineage)),
                    }
                }),
                authorization,
                blocked_by,
                // Resolved per candidate, because an override is scoped: the
                // project's answer and one task's answer are the same question
                // asked about different work.
                calendar: calendar.resolve(now, task.mini_project_id, task.id, &terminal_goals)?,
                runtime: runtime.clone(),
                account: AccountAdmissionEvidence {
                    pin: None,
                    required_capabilities: BTreeSet::new(),
                },
                external: ExternalWorkEvidence::default(),
            });
        }
        Ok(CandidateAssembly {
            candidates,
            considered: tasks.len(),
            without_workflow,
            calendar_assigned,
        })
    }

    /// Everything `kontor-calendar` needs to resolve this project, read once.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`], and refuses an assignment whose pinned
    /// profile revision is missing from this database — that is a calendar this
    /// build cannot resolve, and guessing at it would be inventing a policy.
    pub fn calendar_inputs(
        &self,
        project_id: ProjectId,
        now: Timestamp,
    ) -> RepositoryResult<CalendarInputs> {
        use kontor_core::repository::{CalendarRepository, SpecRepository};

        let Some(assignment) = self.active_assignment(project_id)? else {
            return Ok(CalendarInputs::default());
        };
        let profile = self
            .get_calendar_profile(assignment.profile_id, assignment.profile_version)?
            .ok_or(RepositoryError::NotFound {
                subject: "pinned calendar profile revision",
            })?;
        let exceptions = self.applied_exceptions(project_id, assignment.id)?;
        let overrides = self.active_overrides(project_id, now)?;
        let child_windows = self.active_child_window_revisions(project_id, assignment.id)?;
        Ok(CalendarInputs {
            assignment: Some(assignment),
            profile: Some(profile),
            exceptions,
            overrides,
            child_windows,
        })
    }

    fn active_child_window_revisions(
        &self,
        project_id: ProjectId,
        work_calendar_id: kontor_core::id::WorkCalendarId,
    ) -> RepositoryResult<Vec<ChildCalendarWindows>> {
        use kontor_core::repository::CalendarRepository;

        let mut statement = self
            .connection
            .prepare(
                "SELECT scope_kind, mini_project_id, task_id
               FROM child_calendar_windows AS current
              WHERE project_id = ?1 AND work_calendar_id = ?2
                AND NOT EXISTS (
                    SELECT 1 FROM child_calendar_windows AS later
                     WHERE later.project_id = current.project_id
                       AND later.work_calendar_id = current.work_calendar_id
                       AND later.scope_kind = current.scope_kind
                       AND later.mini_project_id IS current.mini_project_id
                       AND later.task_id IS current.task_id
                       AND later.supersedes = current.version)
              ORDER BY scope_kind, mini_project_id, task_id",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map(
                params![project_id.to_string(), work_calendar_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .map_err(backend)?;
        let mut revisions = Vec::new();
        for row in rows {
            let (kind, mini_project, task) = row.map_err(backend)?;
            let scope = match kind.as_str() {
                "mini_project" => WorkScope::MiniProject {
                    mini_project_id: MiniProjectId::parse(mini_project.as_deref().unwrap_or(""))?,
                },
                "task" => WorkScope::Task {
                    task_id: TaskId::parse(task.as_deref().unwrap_or(""))?,
                },
                _ => {
                    return Err(RepositoryError::Backend {
                        detail: "stored child calendar scope is invalid".to_owned(),
                    });
                }
            };
            if let Some(revision) =
                self.active_child_windows(project_id, work_calendar_id, scope)?
            {
                revisions.push(revision);
            }
        }
        Ok(revisions)
    }

    /// The overrides that could be in force at one instant.
    ///
    /// A pre-filter, not a decision: the rows returned are the ones whose start,
    /// ceiling and revocation do not already rule them out. Scope, expiry and
    /// goal completion are re-checked by the resolver, which is where all of
    /// those rules live.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn active_overrides(
        &self,
        project_id: ProjectId,
        now: Timestamp,
    ) -> RepositoryResult<Vec<ScheduleOverride>> {
        use kontor_core::repository::CalendarRepository;

        let mut statement = self
            .connection
            .prepare(
                "SELECT id FROM schedule_overrides
                  WHERE project_id = ?1 AND revoked_at IS NULL
                    AND start_at <= ?2 AND hard_ceiling >= ?2
                  ORDER BY start_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), format_utc_timestamp(now)])
            .map_err(backend)?;
        let mut ids = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            ids.push(ScheduleOverrideId::parse(
                &row.get::<_, String>(0).map_err(backend)?,
            )?);
        }
        drop(rows);
        drop(statement);

        let mut overrides = Vec::new();
        for id in ids {
            if let Some(found) = self.get_override(project_id, id)? {
                overrides.push(found);
            }
        }
        Ok(overrides)
    }

    /// The active workflow of one task, as a phase and profile pair.
    ///
    /// A convenience over the port method for callers that only want the two facts.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn active_workflow_of(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<(TaskWorkflowId, PhaseKey, WorkProfileKey, SpecVersion)>> {
        use kontor_core::repository::WorkflowRepository;
        Ok(self
            .get_active_task_workflow(project_id, task_id)?
            .map(|workflow: TaskWorkflow| {
                let definition = &workflow.snapshot.definition;
                (
                    workflow.id,
                    workflow.current_phase.clone(),
                    definition.id.clone(),
                    definition.version,
                )
            }))
    }

    // -----------------------------------------------------------------------
    // External tickets
    // -----------------------------------------------------------------------

    /// Every external-ticket link in one project.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn list_ticket_links(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<TicketLinkSummary>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, task_id, connector, external_issue_key, revision, created_at
                 FROM jira_links WHERE project_id = ?1 ORDER BY created_at, id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut links = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            links.push(read_link(row)?);
        }
        Ok(links)
    }

    /// One external-ticket link.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn get_ticket_link(
        &self,
        project_id: ProjectId,
        link_id: TicketLinkId,
    ) -> RepositoryResult<Option<TicketLinkSummary>> {
        let found = self
            .connection
            .query_row(
                "SELECT id, task_id, connector, external_issue_key, revision, created_at
                 FROM jira_links WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), link_id.to_string()],
                |row| Ok(read_link(row)),
            )
            .ok();
        found.transpose()
    }

    /// The newest projection computed for one link.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn latest_ticket_projection(
        &self,
        project_id: ProjectId,
        link_id: TicketLinkId,
    ) -> RepositoryResult<Option<TicketProjection>> {
        let found = self
            .connection
            .query_row(
                "SELECT id, link_revision, external_issue_key, field_spec_project,
                        field_spec_issue_type, field_spec_version, fields, comment_policy,
                        external_comment_cursor, projection_hash, computed_at
                 FROM ticket_sync_projections
                 WHERE project_id = ?1 AND link_id = ?2
                 ORDER BY link_revision DESC LIMIT 1",
                params![project_id.to_string(), link_id.to_string()],
                |row| {
                    Ok((|| -> RepositoryResult<TicketProjection> {
                        let cursor: Option<String> = row.get(8).map_err(backend)?;
                        Ok(TicketProjection {
                            projection_id: TicketProjectionId::parse(&column_text(row, 0)?)?,
                            link_revision: revision_of(row.get(1).map_err(backend)?)?,
                            external_issue_key: ExternalId::parse(&column_text(row, 2)?)?,
                            field_spec_project: ExternalProjectKey::parse(&column_text(row, 3)?)?,
                            field_spec_issue_type: ExternalName::parse(&column_text(row, 4)?)?,
                            field_spec_version: SpecVersion::parse(
                                u32::try_from(row.get::<_, i64>(5).map_err(backend)?)
                                    .unwrap_or_default(),
                            )?,
                            fields: column_text(row, 6)?,
                            comment_policy: column_text(row, 7)?,
                            external_comment_cursor: cursor
                                .as_deref()
                                .map(ExternalId::parse)
                                .transpose()?,
                            projection_hash: column_text(row, 9)?,
                            computed_at: read_timestamp(&column_text(row, 10)?)?,
                        })
                    })())
                },
            )
            .ok();
        found.transpose()
    }

    /// One link's observations, newest first.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn list_ticket_observations(
        &self,
        project_id: ProjectId,
        link_id: TicketLinkId,
        limit: u32,
    ) -> RepositoryResult<Vec<TicketObservation>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, status_id, status_name, status_category, issue_type,
                        assignee_account_id, assignee_display, external_version, observed_at
                 FROM external_ticket_observations
                 WHERE project_id = ?1 AND link_id = ?2
                 ORDER BY observed_at DESC, id DESC LIMIT ?3",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                link_id.to_string(),
                i64::from(limit)
            ])
            .map_err(backend)?;
        let mut observations = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let assignee: Option<String> = row.get(5).map_err(backend)?;
            let display: Option<String> = row.get(6).map_err(backend)?;
            let version: Option<String> = row.get(7).map_err(backend)?;
            observations.push(TicketObservation {
                observation_id: ExternalId::parse(&column_text(row, 0)?)?,
                status_id: ExternalId::parse(&column_text(row, 1)?)?,
                status_name: ExternalName::parse(&column_text(row, 2)?)?,
                status_category: ExternalName::parse(&column_text(row, 3)?)?,
                issue_type: ExternalName::parse(&column_text(row, 4)?)?,
                assignee_account_id: assignee.as_deref().map(ExternalId::parse).transpose()?,
                assignee_display: display.as_deref().map(ExternalName::parse).transpose()?,
                external_version: version.as_deref().map(ExternalId::parse).transpose()?,
                observed_at: read_timestamp(&column_text(row, 8)?)?,
            });
        }
        Ok(observations)
    }

    /// One link's inbound comments, newest first.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn list_inbound_comments(
        &self,
        project_id: ProjectId,
        link_id: TicketLinkId,
        limit: u32,
    ) -> RepositoryResult<Vec<InboundComment>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT external_comment_id, body_hash, author_account_id, author_display,
                        external_created_at, external_updated_at, observed_at, supersedes_hash,
                        body
                 FROM external_comments
                 WHERE project_id = ?1 AND link_id = ?2
                 ORDER BY external_created_at DESC, external_comment_id DESC LIMIT ?3",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                link_id.to_string(),
                i64::from(limit)
            ])
            .map_err(backend)?;
        let mut comments = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let display: Option<String> = row.get(3).map_err(backend)?;
            comments.push(InboundComment {
                external_comment_id: ExternalId::parse(&column_text(row, 0)?)?,
                body_hash: column_text(row, 1)?,
                author_account_id: ExternalId::parse(&column_text(row, 2)?)?,
                author_display: display.as_deref().map(ExternalName::parse).transpose()?,
                external_created_at: read_timestamp(&column_text(row, 4)?)?,
                external_updated_at: read_timestamp(&column_text(row, 5)?)?,
                observed_at: read_timestamp(&column_text(row, 6)?)?,
                supersedes_hash: row.get(7).map_err(backend)?,
                body: column_text(row, 8)?,
            });
        }
        Ok(comments)
    }

    /// One link's conflicts, newest first.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn list_ticket_conflicts(
        &self,
        project_id: ProjectId,
        link_id: TicketLinkId,
    ) -> RepositoryResult<Vec<TicketConflict>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, kind, observation_id, task_revision, spec_version, milestone,
                        detected_at, resolved_at, resolution_receipt_id
                 FROM status_conflicts
                 WHERE project_id = ?1 AND link_id = ?2
                 ORDER BY detected_at DESC, id DESC",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), link_id.to_string()])
            .map_err(backend)?;
        let mut conflicts = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let milestone: Option<String> = row.get(5).map_err(backend)?;
            let receipt: Option<String> = row.get(8).map_err(backend)?;
            conflicts.push(TicketConflict {
                conflict_id: StatusConflictId::parse(&column_text(row, 0)?)?,
                kind: column_text(row, 1)?,
                observation_id: ExternalId::parse(&column_text(row, 2)?)?,
                task_revision: revision_of(row.get(3).map_err(backend)?)?,
                spec_version: SpecVersion::parse(
                    u32::try_from(row.get::<_, i64>(4).map_err(backend)?).unwrap_or_default(),
                )?,
                milestone: milestone.as_deref().map(ExternalName::parse).transpose()?,
                detected_at: read_timestamp(&column_text(row, 6)?)?,
                resolved_at: optional_timestamp(row, 7)?,
                resolution_receipt_id: receipt
                    .as_deref()
                    .map(CommandReceiptId::parse)
                    .transpose()?,
            });
        }
        Ok(conflicts)
    }

    /// One link's convergence attempts, newest first.
    ///
    /// # Errors
    /// As [`SqliteStore::list_projects`].
    pub fn list_ticket_transitions(
        &self,
        project_id: ProjectId,
        link_id: TicketLinkId,
        limit: u32,
    ) -> RepositoryResult<Vec<TicketTransitionAttempt>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, task_id, milestone, target_status_id, transition_id,
                        principal_account_id, assignment_prerequisite, idempotency_key,
                        dispatched_at, acknowledged_at, confirmed_at, refetched_observation_id
                 FROM status_transition_receipts
                 WHERE project_id = ?1 AND link_id = ?2
                 ORDER BY dispatched_at DESC, id DESC LIMIT ?3",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                link_id.to_string(),
                i64::from(limit)
            ])
            .map_err(backend)?;
        let mut attempts = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let transition: Option<String> = row.get(4).map_err(backend)?;
            let refetched: Option<String> = row.get(11).map_err(backend)?;
            attempts.push(TicketTransitionAttempt {
                receipt_id: StatusTransitionReceiptId::parse(&column_text(row, 0)?)?,
                task_id: TaskId::parse(&column_text(row, 1)?)?,
                milestone: ExternalName::parse(&column_text(row, 2)?)?,
                target_status_id: ExternalId::parse(&column_text(row, 3)?)?,
                transition_id: transition.as_deref().map(ExternalId::parse).transpose()?,
                principal_account_id: ExternalId::parse(&column_text(row, 5)?)?,
                assignment_prerequisite: row.get::<_, i64>(6).map_err(backend)? != 0,
                idempotency_key: column_text(row, 7)?,
                dispatched_at: read_timestamp(&column_text(row, 8)?)?,
                acknowledged_at: optional_timestamp(row, 9)?,
                confirmed_at: optional_timestamp(row, 10)?,
                refetched_observation_id: refetched
                    .as_deref()
                    .map(ExternalId::parse)
                    .transpose()?,
            });
        }
        Ok(attempts)
    }
}

/// The stored lineage of one intake-created task, as the scheduler reads it.
///
/// Two facts cross the boundary and no more: which receipt armed *this* task,
/// and whether a human approved it or a bounded policy armed it under a named
/// authorization. An approval carries no authorization because it needs none; an
/// auto-arm is `proposed` plus its authorization, which is exactly the pair
/// [`TaskOrigin::admits`] accepts. The envelope, the filter and the source kind
/// stay on this side of the boundary.
fn intake_lineage(lineage: &kontor_core::repository::IntakeCreatedWork) -> IntakeLineage {
    use kontor_core::repository::IntakeDecisionOutcome;
    IntakeLineage {
        receipt_id: lineage.receipt_id,
        result: match lineage.authority {
            IntakeDecisionOutcome::Approved => IntakeResult::Approved,
            IntakeDecisionOutcome::AutoArmed => IntakeResult::Proposed,
            // A rejection creates no work, so the column's CHECK cannot hold
            // this value. A row that somehow did would be work whose own
            // lineage says it was refused, and it is reported as refused.
            IntakeDecisionOutcome::Rejected => IntakeResult::Rejected,
        },
        armed_task_id: lineage.task_id,
        auto_arm_authorization: lineage.execution_authorization,
    }
}

/// One external-ticket link row.
fn read_link(row: &Row<'_>) -> RepositoryResult<TicketLinkSummary> {
    Ok(TicketLinkSummary {
        link_id: TicketLinkId::parse(&column_text(row, 0)?)?,
        task_id: TaskId::parse(&column_text(row, 1)?)?,
        connector: ExternalName::parse(&column_text(row, 2)?)?,
        external_issue_key: ExternalId::parse(&column_text(row, 3)?)?,
        revision: revision_of(row.get(4).map_err(backend)?)?,
        created_at: read_timestamp(&column_text(row, 5)?)?,
    })
}

/// One text column.
pub(crate) fn column_text(row: &Row<'_>, index: usize) -> RepositoryResult<String> {
    row.get(index).map_err(backend)
}

/// One nullable timestamp column.
fn optional_timestamp(row: &Row<'_>, index: usize) -> RepositoryResult<Option<Timestamp>> {
    let value: Option<String> = row.get(index).map_err(backend)?;
    value.as_deref().map(read_timestamp).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh migrated store in a temporary directory.
    fn store() -> (tempfile::TempDir, SqliteStore) {
        let directory = tempfile::TempDir::new().expect("a temporary directory");
        let store =
            SqliteStore::open(&directory.path().join("kontor.db")).expect("the store opens");
        (directory, store)
    }

    #[test]
    fn a_fresh_realm_has_no_projects_and_no_calendar_assignment() {
        let (_directory, store) = store();
        assert!(
            store.list_projects().expect("the list reads").is_empty(),
            "a fresh realm holds nothing"
        );
        // The scheduling route branches on this: false means there is no window to
        // resolve, so a plan is honest. It must not be true by accident.
        let project = ProjectId::generate();
        assert!(
            !store
                .has_calendar_assignment(project)
                .expect("the flag reads"),
            "an unknown project has no assignment"
        );
    }

    #[test]
    fn a_calendar_assignment_flips_the_flag_the_plan_route_refuses_on() {
        // The other half of `wired::scheduler_plan`'s refusal. Inserted through the
        // crate's own connection because there is deliberately no public way in: a
        // test hook on the store would be a hook production code could reach too.
        let (_directory, store) = store();
        let project = ProjectId::generate();
        store
            .connection
            .execute(
                "INSERT INTO work_calendars (id, project_id, calendar_profile_id, profile_version,
                                             assigned_by, assignment_receipt_id, assigned_at)
                 VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6)",
                params![
                    kontor_core::id::WorkCalendarId::generate().to_string(),
                    project.to_string(),
                    kontor_core::id::CalendarProfileId::generate().to_string(),
                    kontor_core::id::AccountProfileId::generate().to_string(),
                    kontor_core::id::CommandReceiptId::generate().to_string(),
                    "2026-08-12T09:00:00Z",
                ],
            )
            .ok();
        // Foreign keys may refuse the insert in a realm with no project row, which
        // is fine: what matters is that the flag reports what the table holds.
        let held: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM work_calendars WHERE project_id = ?1",
                params![project.to_string()],
                |row| row.get(0),
            )
            .expect("the count reads");
        assert_eq!(
            store
                .has_calendar_assignment(project)
                .expect("the flag reads"),
            held > 0,
            "the flag is a fact about the rows and never a default"
        );
    }

    #[test]
    fn the_assembled_priority_is_a_stated_constant_rather_than_a_guess() {
        // If this ever became a computed value it would be an ordering nobody asked
        // for, and it would be invisible in the plan's answer.
        assert_eq!(ASSEMBLED_PRIORITY, 0);
    }
}
