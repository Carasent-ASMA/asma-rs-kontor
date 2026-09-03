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
use kontor_core::authority::{AuthoritySubject, SubjectAuthority};
use kontor_core::backlog_identity::EpicBacklogCode;
use kontor_core::calendar::ExecutionAuthorization;
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, ArtifactKey, CommandReceiptId, ConnectorKey,
    ContentHash, ExecutionAuthorizationId, ExternalId, ExternalName, MiniProjectId, ModuleKey,
    ProjectId, RoleSlotId, RoleTurnId, RuntimeBindingId, SpecVersion, StatusConflictId, TaskId,
    TaskWorkflowId, TeamRunId, TeamTemplateId, TicketLinkId, TicketObservationId, Timestamp,
    WorkProfileKey,
};
use kontor_core::naming::AiShortName;
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    MiniProject, NewLocalCommand, NewTaskWorkflow, Project, RepositoryError, RepositoryResult,
    Task, TicketLink, validate_dependency_graph,
};
use kontor_core::spec::{ResolvedWorkProfileSnapshot, TeamTemplateRevision, WorkProfileSpec};
use kontor_core::state::{ImportedTaskState, TaskState};
use kontor_core::ticket::StatusConflictKind;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::SqliteStore;
use crate::authority::{
    SubjectAuthorityReceipt, SubjectImportManifest, SubjectImportRecord, SubjectOrigins,
    create_subject_authorities, record_subject_import_in, require_backlog_authority,
    subject_authority_in,
};
use crate::query::column_text;
use crate::repository::{
    TASK_COLUMNS, backend, canonical_jira_connector, conflict, from_json, is_jira_connector,
    read_project, read_scope, read_task, read_timestamp, read_version, revision_of, text, to_json,
    version_column,
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
    /// Where this project's memory and backlog facts come from.
    pub origins: SubjectOrigins,
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
    /// Compact operator-declared display identity used for native containers
    /// and seats. A description, ticket key or path is never substituted.
    pub short_code: Option<ExternalId>,
    /// Intake-time, exactly two-keyword AI summary. It is durable and is never
    /// derived again from mutable descriptive prose.
    pub ai_short_name: Option<AiShortName>,
    /// The module the task contends for, if any. Immutable.
    pub module: Option<ModuleKey>,
    /// Additional modules this task changes, besides [`Self::module`].
    ///
    /// `None` on apply leaves any existing extras alone. `Some` writes the set
    /// once and then treats it as immutable, the same promise `module` already
    /// makes. The primary is never stored here.
    pub changed_modules: Option<BTreeSet<ModuleKey>>,
    /// The historical source lifecycle state this import declares.
    pub imported_state: ImportedTaskState,
    /// The titles of the sibling tasks this one depends on.
    pub depends_on: BTreeSet<ExternalName>,
    /// The external tickets to link. Immutable as a set.
    pub ticket_links: Vec<EpicTicketLink>,
    /// Where this task's work happens. Absolute, validated by the caller.
    ///
    /// `None` leaves whatever was already declared alone rather than clearing
    /// it: an apply that omits the field is not a statement that the task has no
    /// worktree, and silently unplacing a task would be a strange way to say
    /// nothing.
    pub worktree: Option<ExternalName>,
}

/// The runtime-facing identity one epic declares at import time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicExecutionScopeDeclaration {
    /// The external tracker key, e.g. `ASMA-7869`.
    pub external_epic_key: ExternalId,
    /// The compact title used when a runtime renders the epic container.
    pub short_title: ExternalName,
    /// Kontor's immutable backlog identity for this epic (for example QNR-P1).
    pub kontor_backlog_code: Option<ExternalId>,
    /// The immutable two-keyword intake summary used only by templates that ask
    /// for `AI_SHORT_NAME`.
    pub ai_short_name: Option<AiShortName>,
}

/// One epic's durable runtime-facing identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicExecutionScope {
    /// The project containing the epic.
    pub project_id: ProjectId,
    /// The epic this scope belongs to.
    pub mini_project_id: MiniProjectId,
    /// The external tracker key, e.g. `ASMA-7869`.
    pub external_epic_key: ExternalId,
    /// The compact title used when a runtime renders the epic container.
    pub short_title: ExternalName,
    /// Kontor's immutable backlog identity, absent only on a legacy record.
    pub kontor_backlog_code: Option<ExternalId>,
    /// The intake-time two-keyword summary, absent only on a legacy record.
    pub ai_short_name: Option<AiShortName>,
    /// When this immutable scope was declared.
    pub created_at: Timestamp,
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
    /// Optional manual selection for the immutable project-scoped backlog code.
    /// Omission allocates deterministically from `name`.
    pub epic_backlog_code: Option<&'a EpicBacklogCode>,
    /// Runtime-facing epic identity. Omission preserves an existing declaration
    /// and keeps old apply requests byte-compatible.
    pub execution_scope: Option<&'a EpicExecutionScopeDeclaration>,
    /// The tasks, in the order they were stated.
    pub tasks: &'a [EpicTask],
    /// The frozen work profile every task in the epic pins.
    pub profile: &'a ResolvedWorkProfileSnapshot,
    /// That profile's stored revision.
    pub definition: &'a WorkProfileSpec,
    /// The team revision the profile pins, when it prescribes one.
    pub team: Option<&'a TeamTemplateRevision>,
    /// Where that team revision came from.
    ///
    /// Only the build's bundled bootstrap may reconcile an older immutable
    /// revision already stored at the same identity. Registered packs remain
    /// ordinary published input and may never adopt another pack's bytes.
    pub team_source: TeamTemplateSource,
    /// When the application happened.
    pub applied_at: Timestamp,
}

/// Authority behind a team revision presented to graph application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamTemplateSource {
    /// Compiled bootstrap/catalog data shipped by this build.
    Bundled,
    /// An operator-registered profile pack.
    Registered,
}

/// One profile-selection command and the exact policy it intends to freeze.
///
/// The store applies the local receipt, workflow replacement and immutable
/// result binding in one transaction. That makes a retry a read of the
/// historical result rather than another resolution of mutable catalog state.
#[derive(Debug)]
pub struct ProfileSelection<'a> {
    /// Durable local command identity.
    pub command: &'a NewLocalCommand,
    /// Workflow to create when this policy is not already active.
    pub workflow: &'a NewTaskWorkflow,
    /// Published work-profile revision.
    pub definition: &'a WorkProfileSpec,
    /// Published team revision pinned by the profile, when any.
    pub team: Option<&'a TeamTemplateRevision>,
    /// Authority behind the presented team revision.
    pub team_source: TeamTemplateSource,
}

/// One complete legacy backlog export resolved into the existing graph model.
#[derive(Debug)]
pub struct BacklogImport<'a> {
    /// The project receiving the graph.
    pub project_id: ProjectId,
    /// Authority-ledger revision the caller read.
    pub expected_authority_revision: AggregateRevision,
    /// The bounded source-system name.
    pub source: &'a str,
    /// Hash of the canonical submitted export.
    pub import_hash: &'a ContentHash,
    /// Canonical submitted export retained in the import manifest.
    pub canonical_manifest: &'a str,
    /// Every epic in the export, already resolved against native profiles.
    pub epics: &'a [EpicApplication<'a>],
}

/// The atomic result of importing a legacy backlog export.
#[derive(Debug)]
pub struct AppliedBacklogImport {
    /// Existing graph rows created or verified by the import.
    pub epics: Vec<AppliedEpic>,
    /// Durable hash-addressed import manifest.
    pub manifest: SubjectImportManifest,
    /// Durable authority-ledger receipt for the import.
    pub receipt: SubjectAuthorityReceipt,
}

// ---------------------------------------------------------------------------
// Outcomes
// ---------------------------------------------------------------------------

/// Whether an `ensure`/`apply` created the row or found it already matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// This call wrote the row.
    Created,
    /// The durable row already existed and this call added compatible metadata.
    Updated,
    /// The row already existed and matched, so nothing was written.
    Unchanged,
}

impl Applied {
    /// The stable spelling used on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
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
    /// Durable compact display identity, once declared.
    pub short_code: Option<ExternalId>,
    /// Durable intake-time two-keyword summary, once declared.
    pub ai_short_name: Option<AiShortName>,
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
    /// Where its work happens, once declared.
    pub worktree: Option<ExternalName>,
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
    /// Kontor's immutable project-scoped epic namespace.
    pub epic_backlog_code: EpicBacklogCode,
    /// Whether this call created it.
    pub applied: Applied,
    /// The revision a write must present.
    pub revision: AggregateRevision,
    /// The durable runtime-facing identity, once declared.
    pub execution_scope: Option<EpicExecutionScope>,
    /// The work profile revision frozen onto every task.
    pub profile: (kontor_core::id::WorkProfileKey, SpecVersion),
    /// The team revision the profile pins, when it prescribes one.
    pub team: Option<(TeamTemplateId, SpecVersion)>,
    /// The tasks, in the order they were stated.
    pub tasks: Vec<AppliedTask>,
}

/// The immutable historical result bound to one profile-selection receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProfileSelectionOutcome {
    /// Receipt whose exact result this row preserves.
    pub receipt_id: CommandReceiptId,
    /// Task the selection targeted.
    pub task_id: TaskId,
    /// Exact workflow the receipt selected, active or historical now.
    pub workflow_id: TaskWorkflowId,
    /// Exact stored work-profile revision and canonical hash.
    pub profile: (WorkProfileKey, SpecVersion, ContentHash),
    /// Exact stored team-template revision and canonical hash, when pinned.
    pub team: Option<(TeamTemplateId, SpecVersion, ContentHash)>,
    /// Whether the original call created a workflow or found the same one.
    pub applied: Applied,
    /// When the atomic selection was recorded.
    pub recorded_at: Timestamp,
}

/// One recorded reconciliation conflict, as a reader is told about it.
///
/// It carries the *classification* and the positions it was detected at, never
/// the external values that produced it: a conflict says Kontor and the external
/// system disagree, and reproducing the disagreement's content here would put
/// ticket prose in a control-plane read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredConflict {
    /// The conflict.
    pub id: StatusConflictId,
    /// The ticket link it concerns.
    pub link_id: TicketLinkId,
    /// Which classification the domain reached.
    pub kind: StatusConflictKind,
    /// The observation it was computed from.
    pub observation_id: TicketObservationId,
    /// The task revision at detection.
    pub task_revision: AggregateRevision,
    /// The pinned specification revision at detection.
    pub spec_version: SpecVersion,
    /// When it was detected.
    pub detected_at: Timestamp,
    /// When it was resolved, if it has been.
    pub resolved_at: Option<Timestamp>,
}

/// One stored inbound comment revision.
///
/// The body is deliberately absent. Kontor mirrors inbound comments so a
/// reviewer's words are not lost, and the *control plane* reads their identity,
/// authorship and digest — a route that returned the prose would make this the
/// place external ticket content leaves the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredComment {
    /// The ticket link.
    pub link_id: TicketLinkId,
    /// The comment's external id.
    pub external_comment_id: ExternalId,
    /// Digest of the normalized body. Identity of *this revision*.
    pub body_hash: ContentHash,
    /// The author's external account id.
    pub author_account_id: ExternalId,
    /// The author's display name, as the external system rendered it.
    pub author_display: Option<ExternalName>,
    /// When the external system created the comment.
    pub external_created_at: Timestamp,
    /// When the external system last updated it.
    pub external_updated_at: Timestamp,
    /// When Kontor observed this revision.
    pub observed_at: Timestamp,
    /// The revision this one supersedes, for an edit.
    pub supersedes: Option<ContentHash>,
}

impl SqliteStore {
    /// Assign or replay one epic's immutable project-scoped backlog namespace.
    ///
    /// Omission selects the deterministic title-derived candidate. The
    /// `IMMEDIATE` transaction owns both allocation and the case-insensitive
    /// unique insert, so racing allocators observe one another serially.
    pub fn assign_epic_backlog_code(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
        manual_override: Option<&EpicBacklogCode>,
        assigned_at: Timestamp,
    ) -> RepositoryResult<EpicBacklogCode> {
        let transaction = self.begin()?;
        let code = ensure_epic_backlog_code(
            &transaction,
            project_id,
            mini_project_id,
            manual_override,
            assigned_at,
        )?;
        transaction.commit().map_err(backend)?;
        Ok(code)
    }

    /// Read one active epic backlog namespace.
    pub fn epic_backlog_code(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<EpicBacklogCode>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT code FROM epic_backlog_codes
                 WHERE project_id = ?1 AND mini_project_id = ?2 AND status = 'active'",
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(found.as_deref().map(EpicBacklogCode::parse).transpose()?)
    }

    /// Every reconciliation conflict recorded against one task's links.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list_task_ticket_conflicts(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Vec<StoredConflict>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT c.id, c.link_id, c.kind, c.observation_id, c.task_revision,
                        c.spec_version, c.detected_at, c.resolved_at
                 FROM status_conflicts AS c
                 JOIN jira_links AS l
                   ON l.project_id = c.project_id AND l.id = c.link_id
                 WHERE c.project_id = ?1 AND l.task_id = ?2
                 ORDER BY c.detected_at, c.id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), task_id.to_string()])
            .map_err(backend)?;
        let mut conflicts = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let resolved: Option<String> = row.get(7).map_err(backend)?;
            conflicts.push(StoredConflict {
                id: StatusConflictId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                link_id: TicketLinkId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
                kind: StatusConflictKind::parse(&row.get::<_, String>(2).map_err(backend)?)?,
                observation_id: TicketObservationId::parse(
                    &row.get::<_, String>(3).map_err(backend)?,
                )?,
                task_revision: revision_of(row.get::<_, i64>(4).map_err(backend)?)?,
                spec_version: read_version(row.get::<_, i64>(5).map_err(backend)?)?,
                detected_at: read_timestamp(&row.get::<_, String>(6).map_err(backend)?)?,
                resolved_at: resolved.as_deref().map(read_timestamp).transpose()?,
            });
        }
        Ok(conflicts)
    }

    /// Every inbound comment revision mirrored for one task's links.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list_task_inbound_comments(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Vec<StoredComment>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT c.link_id, c.external_comment_id, c.body_hash, c.author_account_id,
                        c.author_display, c.external_created_at, c.external_updated_at,
                        c.observed_at, c.supersedes_hash
                 FROM external_comments AS c
                 JOIN jira_links AS l
                   ON l.project_id = c.project_id AND l.id = c.link_id
                 WHERE c.project_id = ?1 AND l.task_id = ?2
                 ORDER BY c.external_created_at, c.external_comment_id, c.body_hash",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), task_id.to_string()])
            .map_err(backend)?;
        let mut comments = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let display: Option<String> = row.get(4).map_err(backend)?;
            let supersedes: Option<String> = row.get(8).map_err(backend)?;
            comments.push(StoredComment {
                link_id: TicketLinkId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                external_comment_id: ExternalId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
                body_hash: ContentHash::parse(&row.get::<_, String>(2).map_err(backend)?)?,
                author_account_id: ExternalId::parse(&row.get::<_, String>(3).map_err(backend)?)?,
                author_display: display.as_deref().map(ExternalName::parse).transpose()?,
                external_created_at: read_timestamp(&row.get::<_, String>(5).map_err(backend)?)?,
                external_updated_at: read_timestamp(&row.get::<_, String>(6).map_err(backend)?)?,
                observed_at: read_timestamp(&row.get::<_, String>(7).map_err(backend)?)?,
                supersedes: supersedes.as_deref().map(ContentHash::parse).transpose()?,
            });
        }
        Ok(comments)
    }

    /// One task's ticket link, addressed by the task it belongs to.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn first_ticket_link(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<TicketLink>> {
        Ok(self
            .list_task_ticket_links(project_id, task_id)?
            .into_iter()
            .next())
    }
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
            for subject in AuthoritySubject::ALL.iter().copied() {
                let stored: String = transaction
                    .query_row(
                        "SELECT origin FROM project_subject_authority
                         WHERE project_id = ?1 AND subject = ?2",
                        params![project.id.to_string(), subject.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(backend)?;
                if stored != request.origins.for_subject(subject).as_str() {
                    return Err(conflict(
                        "project",
                        "the project exists with a different declared subject origin",
                    ));
                }
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
        create_subject_authorities(&transaction, request.id, request.origins).map_err(backend)?;
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
        self.evaluate_epic(request, true)
    }

    /// Declare the immutable runtime-facing identity of an epic that already
    /// exists, such as one created by Quick-session promotion.
    ///
    /// This reuses the same insert-once/conflicting-redeclaration rules as
    /// declarative epic import. It creates no task or topology identity.
    pub fn declare_epic_execution_scope(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
        declaration: &EpicExecutionScopeDeclaration,
        declared_at: Timestamp,
    ) -> RepositoryResult<EpicExecutionScope> {
        let transaction = self.begin()?;
        ensure_project_exists(&transaction, project_id)?;
        let exists = transaction
            .query_row(
                "SELECT 1 FROM mini_projects WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), mini_project_id.to_string()],
                |_| Ok(()),
            )
            .optional()
            .map_err(backend)?
            .is_some();
        if !exists {
            return Err(RepositoryError::NotFound {
                subject: "mini project",
            });
        }
        let scope = ensure_epic_execution_scope(
            &transaction,
            project_id,
            mini_project_id,
            Some(declaration),
            declared_at,
        )?
        .expect("an explicit declaration always returns a scope");
        transaction.commit().map_err(backend)?;
        Ok(scope)
    }

    /// Judge one whole epic with the exact apply rules, then roll every
    /// prospective write back.
    ///
    /// Existing matching rows retain their durable ids in the answer. Rows that
    /// would be created receive transaction-local ids so dependency, link and
    /// workflow validation can run normally; the API deliberately withholds
    /// those ids because rollback makes them non-authoritative.
    ///
    /// # Errors
    /// The same conflicts and invalid graph shapes as [`Self::apply_epic`].
    pub fn preview_epic(&self, request: &EpicApplication<'_>) -> RepositoryResult<AppliedEpic> {
        self.evaluate_epic(request, false)
    }

    fn evaluate_epic(
        &self,
        request: &EpicApplication<'_>,
        commit: bool,
    ) -> RepositoryResult<AppliedEpic> {
        let transaction = self.begin()?;
        ensure_project_exists(&transaction, request.project_id)?;
        if commit {
            // Preview remains available while a legacy system owns the backlog;
            // only an authoritative write is withheld.
            require_backlog_authority(&transaction, request.project_id)?;
        }
        let outcome = evaluate_epic_in(&transaction, request)?;
        if commit {
            transaction.commit().map_err(backend)?;
        } else {
            transaction.rollback().map_err(backend)?;
        }
        Ok(outcome)
    }

    /// Import one whole legacy backlog and its manifest in one transaction.
    ///
    /// # Errors
    /// Refuses native/already-switched subjects, mixed projects, invalid graphs,
    /// duplicate manifests and any storage failure without partial graph rows.
    pub fn import_backlog(
        &self,
        import: &BacklogImport<'_>,
    ) -> Result<AppliedBacklogImport, crate::authority::AuthorityError> {
        kontor_core::id::reject_sensitive_text("backlog import source", import.source)?;
        let transaction =
            Transaction::new_unchecked(&self.connection, rusqlite::TransactionBehavior::Immediate)?;
        let authority =
            subject_authority_in(&transaction, import.project_id, AuthoritySubject::Backlog)?;
        if !authority.origin.permits_cutover()
            || authority.authority != SubjectAuthority::Agentsroom
        {
            return Err(crate::authority::AuthorityError::Rule(
                "the backlog is not pending legacy import",
            ));
        }
        if authority.revision != import.expected_authority_revision {
            return Err(crate::authority::AuthorityError::RevisionConflict {
                expected: import.expected_authority_revision.get(),
                current: authority.revision.get(),
            });
        }
        ensure_project_exists(&transaction, import.project_id)?;
        let mut applied = Vec::with_capacity(import.epics.len());
        for epic in import.epics {
            if epic.project_id != import.project_id {
                return Err(crate::authority::AuthorityError::Rule(
                    "a backlog export may not cross projects",
                ));
            }
            applied.push(evaluate_epic_in(&transaction, epic)?);
        }
        let readback_hash = backlog_readback_hash_in(&transaction, import.project_id)?;
        let imported_count = applied.iter().try_fold(0_u64, |count, epic| {
            count
                .checked_add(1 + u64::try_from(epic.tasks.len()).unwrap_or(u64::MAX))
                .ok_or(crate::authority::AuthorityError::Rule(
                    "the backlog export contains too many items",
                ))
        })?;
        let (manifest, receipt) = record_subject_import_in(
            &transaction,
            &SubjectImportRecord {
                project_id: import.project_id,
                subject: AuthoritySubject::Backlog,
                source: import.source,
                import_hash: import.import_hash,
                canonical_manifest: import.canonical_manifest,
                imported_count,
                readback_hash: &readback_hash,
            },
        )?;
        transaction.commit()?;
        Ok(AppliedBacklogImport {
            epics: applied,
            manifest,
            receipt,
        })
    }

    /// Validate a complete legacy backlog export and compute its proposed
    /// stored readback, then roll every graph row back.
    pub fn preview_backlog(
        &self,
        import: &BacklogImport<'_>,
    ) -> Result<(Vec<AppliedEpic>, ContentHash), crate::authority::AuthorityError> {
        let transaction =
            Transaction::new_unchecked(&self.connection, rusqlite::TransactionBehavior::Immediate)?;
        let authority =
            subject_authority_in(&transaction, import.project_id, AuthoritySubject::Backlog)?;
        if !authority.origin.permits_cutover()
            || authority.authority != SubjectAuthority::Agentsroom
        {
            return Err(crate::authority::AuthorityError::Rule(
                "the backlog is not pending legacy import",
            ));
        }
        if authority.revision != import.expected_authority_revision {
            return Err(crate::authority::AuthorityError::RevisionConflict {
                expected: import.expected_authority_revision.get(),
                current: authority.revision.get(),
            });
        }
        ensure_project_exists(&transaction, import.project_id)?;
        let mut applied = Vec::with_capacity(import.epics.len());
        for epic in import.epics {
            if epic.project_id != import.project_id {
                return Err(crate::authority::AuthorityError::Rule(
                    "a backlog export may not cross projects",
                ));
            }
            applied.push(evaluate_epic_in(&transaction, epic)?);
        }
        let readback = backlog_readback_hash_in(&transaction, import.project_id)?;
        transaction.rollback()?;
        Ok((applied, readback))
    }

    /// Recompute the canonical hash of one project's stored backlog graph.
    pub fn backlog_readback_hash(
        &self,
        project_id: ProjectId,
    ) -> Result<ContentHash, crate::authority::AuthorityError> {
        let transaction = self.connection.unchecked_transaction()?;
        backlog_readback_hash_in(&transaction, project_id)
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

    /// List every epic in one project in stable identity order.
    ///
    /// The resident Jira controller uses the project graph itself as its
    /// durable queue, including epics that currently have no child tasks.
    pub fn list_mini_projects(&self, project_id: ProjectId) -> RepositoryResult<Vec<MiniProject>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, project_id, name, revision, created_at
                 FROM mini_projects WHERE project_id = ?1 ORDER BY id",
            )
            .map_err(backend)?;
        let rows = statement
            .query_map([project_id.to_string()], |row| Ok(read_mini_project(row)))
            .map_err(backend)?;
        rows.map(|row| row.map_err(backend)?).collect()
    }

    /// Read one epic's durable runtime-facing identity.
    ///
    /// # Errors
    /// Backend failures only; an epic without a declaration is `Ok(None)`.
    pub fn get_epic_execution_scope(
        &self,
        project_id: ProjectId,
        mini_project_id: MiniProjectId,
    ) -> RepositoryResult<Option<EpicExecutionScope>> {
        self.connection
            .query_row(
                "SELECT s.external_epic_key, s.short_title, s.created_at,
                        n.kontor_backlog_code, n.ai_short_name
                 FROM epic_execution_scopes AS s
                 LEFT JOIN epic_native_name_tokens AS n
                   ON n.project_id = s.project_id
                  AND n.mini_project_id = s.mini_project_id
                 WHERE s.project_id = ?1 AND s.mini_project_id = ?2",
                params![project_id.to_string(), mini_project_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?
            .map(
                |(external_epic_key, short_title, created_at, backlog_code, ai_short_name)| {
                    Ok(EpicExecutionScope {
                        project_id,
                        mini_project_id,
                        external_epic_key: ExternalId::parse(&external_epic_key)?,
                        short_title: ExternalName::parse(&short_title)?,
                        kontor_backlog_code: backlog_code
                            .as_deref()
                            .map(ExternalId::parse)
                            .transpose()?,
                        ai_short_name: ai_short_name
                            .as_deref()
                            .map(AiShortName::parse)
                            .transpose()?,
                        created_at: read_timestamp(&created_at)?,
                    })
                },
            )
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
    pub fn list_task_ticket_links(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Vec<TicketLink>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, project_id, task_id, connector, external_issue_key, revision, created_at
                 FROM jira_links AS link
                 WHERE project_id = ?1 AND task_id = ?2
                   AND (
                       connector NOT IN ('jira', 'connector.jira')
                       OR EXISTS (
                           SELECT 1
                           FROM canonical_jira_task_links AS ledger
                           WHERE ledger.project_id = link.project_id
                             AND ledger.link_id = link.id
                       )
                   )
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
        team_source: TeamTemplateSource,
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
            epic_backlog_code: None,
            execution_scope: None,
            tasks: &[],
            profile: &request.snapshot,
            definition,
            team,
            team_source,
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

    /// Apply one profile-selection receipt and its exact workflow result in one
    /// transaction.
    ///
    /// A later selection may deactivate the workflow this call chose, but it
    /// cannot change the immutable outcome bound to this receipt. Replays
    /// therefore return the original policy rather than whichever workflow is
    /// active when the retry arrives.
    ///
    /// # Errors
    /// Refuses cross-project/mismatched inputs, missing tasks, published
    /// revision drift and an idempotency replay whose historical outcome is not
    /// available (which is the safe answer for receipts created before schema
    /// v62).
    pub fn apply_profile_selection(
        &self,
        request: &ProfileSelection<'_>,
    ) -> RepositoryResult<StoredProfileSelectionOutcome> {
        let project_id = request.command.project_id;
        let task_id = request.workflow.task_id;
        if request.workflow.project_id != project_id
            || request.command.kind != CommandKind::SelectTaskProfile
            || request.command.target != (AggregateRef::Task { task_id })
        {
            return Err(RepositoryError::CrossProject {
                subject: "profile selection",
            });
        }
        request.workflow.snapshot.verify()?;
        if request.workflow.snapshot.definition != *request.definition {
            return Err(conflict(
                "profile selection",
                "the workflow snapshot does not match the presented work-profile revision",
            ));
        }
        if request.workflow.current_phase != request.definition.entry_phase
            || request.workflow.created_at != request.command.created_at
        {
            return Err(conflict(
                "profile selection",
                "the workflow start does not match the command's resolved policy and instant",
            ));
        }
        match (request.definition.team_template, request.team) {
            (None, None) => {}
            (Some(pin), Some(team))
                if pin.template_id == team.template_id && pin.version == team.version => {}
            _ => {
                return Err(conflict(
                    "profile selection",
                    "the presented team revision does not match the work-profile pin",
                ));
            }
        }

        let transaction = self.begin()?;
        if let Some(existing) =
            crate::commands::intent::insert_local_command(&transaction, request.command)?
        {
            return read_profile_selection_outcome(&transaction, project_id, existing.id)?.ok_or(
                RepositoryError::Conflict {
                    subject: "profile selection outcome",
                    rule: "the durable receipt predates exact selection-outcome binding",
                },
            );
        }

        let known: Option<i64> = transaction
            .query_row(
                "SELECT revision FROM tasks WHERE project_id = ?1 AND id = ?2",
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(revision) = known else {
            return Err(RepositoryError::NotFound { subject: "task" });
        };
        revision_of(revision)?.expect("task", request.command.target_revision)?;

        let application = EpicApplication {
            project_id,
            name: ExternalName::parse("selection").map_err(RepositoryError::Domain)?,
            epic_backlog_code: None,
            execution_scope: None,
            tasks: &[],
            profile: &request.workflow.snapshot,
            definition: request.definition,
            team: request.team,
            team_source: request.team_source,
            applied_at: request.command.created_at,
        };
        store_specifications(&transaction, &application)?;

        let active: Option<(String, String, i64)> = transaction
            .query_row(
                "SELECT id, profile_key, profile_version FROM task_workflows
                 WHERE project_id = ?1 AND task_id = ?2 AND active = 1",
                params![project_id.to_string(), task_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(backend)?;
        let unchanged = match active.as_ref() {
            Some((_, key, version)) => {
                key == request.definition.id.as_str()
                    && read_version(*version)? == request.definition.version
            }
            None => false,
        };
        let (workflow_id, applied) = if unchanged {
            let workflow_id = TaskWorkflowId::parse(
                &active.expect("unchanged means an active workflow exists").0,
            )?;
            let (workflow, _) =
                crate::repository::load_workflow(&transaction, project_id, workflow_id)?;
            if workflow.snapshot.definition_hash != request.workflow.snapshot.definition_hash {
                return Err(conflict(
                    "task workflow",
                    "the active workflow has different published work-profile content",
                ));
            }
            (workflow_id, Applied::Unchanged)
        } else {
            transaction
                .execute(
                    "UPDATE task_workflows SET active = 0
                     WHERE project_id = ?1 AND task_id = ?2 AND active = 1",
                    params![project_id.to_string(), task_id.to_string()],
                )
                .map_err(backend)?;
            let document =
                kontor_core::id::CanonicalDocument::from_serializable(&request.workflow.snapshot)?;
            transaction
                .execute(
                    "INSERT INTO task_workflows
                         (id, project_id, task_id, profile_key, profile_version, snapshot,
                          snapshot_hash, current_phase, active, revision, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, 1, ?9)",
                    params![
                        request.workflow.id.to_string(),
                        project_id.to_string(),
                        task_id.to_string(),
                        request.definition.id.as_str(),
                        version_column(request.definition.version),
                        document.json(),
                        document.hash().as_str(),
                        request.workflow.current_phase.as_str(),
                        text(request.workflow.created_at)
                    ],
                )
                .map_err(backend)?;
            (request.workflow.id, Applied::Created)
        };

        let profile_hash: String = transaction
            .query_row(
                "SELECT definition_hash FROM work_profiles
                 WHERE project_id = ?1 AND profile_key = ?2 AND version = ?3",
                params![
                    project_id.to_string(),
                    request.definition.id.as_str(),
                    version_column(request.definition.version)
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let team = request
            .definition
            .team_template
            .map(|pin| -> RepositoryResult<_> {
                let hash: String = transaction
                    .query_row(
                        "SELECT definition_hash FROM team_templates
                         WHERE project_id = ?1 AND template_id = ?2 AND version = ?3",
                        params![
                            project_id.to_string(),
                            pin.template_id.to_string(),
                            version_column(pin.version)
                        ],
                        |row| row.get(0),
                    )
                    .map_err(backend)?;
                Ok((pin.template_id, pin.version, ContentHash::parse(&hash)?))
            })
            .transpose()?;
        let outcome = StoredProfileSelectionOutcome {
            receipt_id: request.command.receipt_id,
            task_id,
            workflow_id,
            profile: (
                request.definition.id.clone(),
                request.definition.version,
                ContentHash::parse(&profile_hash)?,
            ),
            team,
            applied,
            recorded_at: request.command.created_at,
        };
        transaction
            .execute(
                "INSERT INTO profile_selection_outcomes
                     (project_id, receipt_id, task_id, workflow_id, profile_key,
                      profile_version, profile_hash, team_template_id,
                      team_template_version, team_template_hash, applied, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    project_id.to_string(),
                    outcome.receipt_id.to_string(),
                    task_id.to_string(),
                    workflow_id.to_string(),
                    outcome.profile.0.as_str(),
                    version_column(outcome.profile.1),
                    outcome.profile.2.as_str(),
                    outcome.team.as_ref().map(|team| team.0.to_string()),
                    outcome.team.as_ref().map(|team| version_column(team.1)),
                    outcome.team.as_ref().map(|team| team.2.as_str()),
                    outcome.applied.as_str(),
                    text(outcome.recorded_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(outcome)
    }

    /// Read the exact historical result of one profile-selection receipt.
    ///
    /// # Errors
    /// Backend or stored-domain failures only.
    pub fn get_profile_selection_outcome(
        &self,
        project_id: ProjectId,
        receipt_id: CommandReceiptId,
    ) -> RepositoryResult<Option<StoredProfileSelectionOutcome>> {
        read_profile_selection_outcome(&self.connection, project_id, receipt_id)
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

fn read_profile_selection_outcome(
    connection: &Connection,
    project_id: ProjectId,
    receipt_id: CommandReceiptId,
) -> RepositoryResult<Option<StoredProfileSelectionOutcome>> {
    type Row = (
        String,
        String,
        String,
        i64,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        String,
        String,
    );
    let row: Option<Row> = connection
        .query_row(
            "SELECT task_id, workflow_id, profile_key, profile_version, profile_hash,
                    team_template_id, team_template_version, team_template_hash,
                    applied, recorded_at
             FROM profile_selection_outcomes
             WHERE project_id = ?1 AND receipt_id = ?2",
            params![project_id.to_string(), receipt_id.to_string()],
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
    let Some((
        task_id,
        workflow_id,
        profile_key,
        profile_version,
        profile_hash,
        team_template_id,
        team_template_version,
        team_template_hash,
        applied,
        recorded_at,
    )) = row
    else {
        return Ok(None);
    };
    let team = match (team_template_id, team_template_version, team_template_hash) {
        (None, None, None) => None,
        (Some(id), Some(version), Some(hash)) => Some((
            TeamTemplateId::parse(&id)?,
            read_version(version)?,
            ContentHash::parse(&hash)?,
        )),
        _ => {
            return Err(RepositoryError::Backend {
                detail: "stored profile selection has an incomplete team-template pin".to_owned(),
            });
        }
    };
    let applied = match applied.as_str() {
        "created" => Applied::Created,
        "unchanged" => Applied::Unchanged,
        _ => {
            return Err(RepositoryError::Backend {
                detail: "stored profile selection has an invalid applied result".to_owned(),
            });
        }
    };
    Ok(Some(StoredProfileSelectionOutcome {
        receipt_id,
        task_id: TaskId::parse(&task_id)?,
        workflow_id: TaskWorkflowId::parse(&workflow_id)?,
        profile: (
            WorkProfileKey::parse(&profile_key)?,
            read_version(profile_version)?,
            ContentHash::parse(&profile_hash)?,
        ),
        team,
        applied,
        recorded_at: read_timestamp(&recorded_at)?,
    }))
}

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
    let connector = row.get::<_, String>(3).map_err(backend)?;
    Ok(TicketLink {
        id: TicketLinkId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        project_id: ProjectId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        task_id: TaskId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        connector: ConnectorKey::parse(if connector == "jira" {
            "connector.jira"
        } else {
            &connector
        })?,
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

fn evaluate_epic_in(
    transaction: &Transaction<'_>,
    request: &EpicApplication<'_>,
) -> RepositoryResult<AppliedEpic> {
    request.profile.verify()?;
    ensure_titles_unique(request.tasks)?;
    ensure_dependencies_named(request.tasks)?;
    store_specifications(transaction, request)?;

    let (mini_project, epic_applied) = ensure_mini_project(transaction, request)?;
    let epic_backlog_code = ensure_epic_backlog_code(
        transaction,
        request.project_id,
        mini_project.id,
        request.epic_backlog_code,
        request.applied_at,
    )?;
    let execution_scope = ensure_epic_execution_scope(
        transaction,
        request.project_id,
        mini_project.id,
        request.execution_scope,
        request.applied_at,
    )?;
    let mut applied = Vec::with_capacity(request.tasks.len());
    let mut by_title = BTreeMap::<&ExternalName, TaskId>::new();
    for plan in request.tasks {
        let outcome = ensure_task(transaction, request, mini_project.id, plan)?;
        by_title.insert(&plan.title, outcome.task_id);
        applied.push(outcome);
    }
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
        write_dependencies(transaction, request.project_id, outcome.task_id, &resolved)?;
        outcome.depends_on = resolved;
    }
    ensure_acyclic(transaction, request.project_id)?;
    for (plan, outcome) in request.tasks.iter().zip(applied.iter_mut()) {
        outcome.links = ensure_links(transaction, request, outcome.task_id, plan)?;
    }
    let epic_applied = if epic_applied == Applied::Unchanged
        && applied.iter().any(|task| task.applied == Applied::Updated)
    {
        Applied::Updated
    } else {
        epic_applied
    };
    Ok(AppliedEpic {
        mini_project_id: mini_project.id,
        epic_backlog_code,
        applied: epic_applied,
        revision: mini_project.revision,
        execution_scope,
        profile: (request.definition.id.clone(), request.definition.version),
        team: request
            .team
            .map(|revision| (revision.template_id, revision.version)),
        tasks: applied,
    })
}

fn backlog_readback_hash_in(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
) -> Result<ContentHash, crate::authority::AuthorityError> {
    let mut statement = transaction.prepare(
        "SELECT m.id, m.name, m.revision,
                t.id, t.title, t.state, t.imported_state, t.revision
         FROM mini_projects m
         LEFT JOIN tasks t ON t.project_id=m.project_id AND t.mini_project_id=m.id
         WHERE m.project_id=?1 ORDER BY m.name, m.id, t.title, t.id",
    )?;
    let rows = statement.query_map([project_id.to_string()], |row| {
        Ok(serde_json::json!({
            "epic_id": row.get::<_, String>(0)?,
            "epic_name": row.get::<_, String>(1)?,
            "epic_revision": row.get::<_, i64>(2)?,
            "task_id": row.get::<_, Option<String>>(3)?,
            "task_title": row.get::<_, Option<String>>(4)?,
            "task_state": row.get::<_, Option<String>>(5)?,
            "task_imported_state": row.get::<_, Option<String>>(6)?,
            "task_revision": row.get::<_, Option<i64>>(7)?,
        }))
    })?;
    let graph = rows.collect::<Result<Vec<_>, _>>()?;
    let mut statement = transaction.prepare(
        "SELECT d.task_id, d.depends_on_task_id
         FROM task_dependencies d JOIN tasks t
           ON t.project_id=d.project_id AND t.id=d.task_id
         WHERE d.project_id=?1 ORDER BY d.task_id, d.depends_on_task_id",
    )?;
    let dependencies = statement
        .query_map([project_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut statement = transaction.prepare(
        "SELECT task_id, connector, external_issue_key
         FROM jira_links WHERE project_id=?1
         ORDER BY task_id, connector, external_issue_key",
    )?;
    let links = statement
        .query_map([project_id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let document = kontor_core::id::CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "project_id": project_id.to_string(),
        "graph": graph,
        "dependencies": dependencies,
        "ticket_links": links,
    }))?;
    Ok(document.hash().clone())
}

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
/// The two tables follow different contracts. A work profile is a *contract*:
/// the same `(id, version)` may only ever name the bytes it was published
/// with, so drift at an existing identity is refused. Bundled team templates
/// are bootstrap data, exactly like bundled consultation presets: the identity
/// is insert-only and, once it exists, the stored bytes are authoritative even
/// when a newer daemon ships different bytes under it. Registered packs do not
/// get that reconciliation authority; they must still present identical bytes.
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
        // Bootstrap contract: the bundle is a lazy bootstrap source, not a
        // mutable source of truth. Once this immutable identity exists, the
        // stored bytes stay authoritative even if a later daemon ships
        // different bytes under it; changed policy belongs in the next bundled
        // version and is appended through the insert path below.
        Some(_) if request.team_source == TeamTemplateSource::Bundled => Ok(()),
        Some(_) => Err(conflict(
            "team template",
            "a registered pack revision collides with different published content",
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

fn ensure_epic_execution_scope(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    declaration: Option<&EpicExecutionScopeDeclaration>,
    created_at: Timestamp,
) -> RepositoryResult<Option<EpicExecutionScope>> {
    let existing = transaction
        .query_row(
            "SELECT s.external_epic_key, s.short_title, s.created_at,
                    n.kontor_backlog_code, n.ai_short_name
             FROM epic_execution_scopes AS s
             LEFT JOIN epic_native_name_tokens AS n
               ON n.project_id = s.project_id
              AND n.mini_project_id = s.mini_project_id
             WHERE s.project_id = ?1 AND s.mini_project_id = ?2",
            params![project_id.to_string(), mini_project_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    if let Some((external_epic_key, short_title, stored_at, backlog_code, ai_short_name)) = existing
    {
        let mut stored = EpicExecutionScope {
            project_id,
            mini_project_id,
            external_epic_key: ExternalId::parse(&external_epic_key)?,
            short_title: ExternalName::parse(&short_title)?,
            kontor_backlog_code: backlog_code.as_deref().map(ExternalId::parse).transpose()?,
            ai_short_name: ai_short_name
                .as_deref()
                .map(AiShortName::parse)
                .transpose()?,
            created_at: read_timestamp(&stored_at)?,
        };
        if declaration.is_some_and(|declared| {
            declared.external_epic_key != stored.external_epic_key
                || declared.short_title != stored.short_title
        }) {
            return Err(conflict(
                "epic execution scope",
                "the epic already declares a different runtime-facing identity",
            ));
        }
        match (
            stored.kontor_backlog_code.as_ref(),
            declaration.and_then(|value| value.kontor_backlog_code.as_ref()),
        ) {
            (Some(current), Some(declared)) if current != declared => {
                return Err(conflict(
                    "epic native-name tokens",
                    "the epic already has a different Kontor backlog code",
                ));
            }
            (None, Some(declared)) => {
                transaction
                    .execute(
                        "INSERT INTO epic_native_name_tokens
                             (project_id, mini_project_id, kontor_backlog_code,
                              ai_short_name, declared_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            project_id.to_string(),
                            mini_project_id.to_string(),
                            declared.as_str(),
                            declaration
                                .and_then(|value| value.ai_short_name.as_ref())
                                .map(AiShortName::as_str),
                            text(created_at),
                        ],
                    )
                    .map_err(backend)?;
                stored.kontor_backlog_code = Some(declared.clone());
                stored.ai_short_name = declaration.and_then(|value| value.ai_short_name.clone());
            }
            _ => {}
        }
        if stored.kontor_backlog_code.is_some()
            && declaration.and_then(|value| value.ai_short_name.as_ref())
                != stored.ai_short_name.as_ref()
            && declaration.is_some_and(|value| value.ai_short_name.is_some())
        {
            return Err(conflict(
                "epic native-name tokens",
                "the epic already has a different AI short name",
            ));
        }
        return Ok(Some(stored));
    }

    let Some(declaration) = declaration else {
        return Ok(None);
    };
    transaction
        .execute(
            "INSERT INTO epic_execution_scopes
                 (project_id, mini_project_id, external_epic_key, short_title, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                project_id.to_string(),
                mini_project_id.to_string(),
                declaration.external_epic_key.as_str(),
                declaration.short_title.as_str(),
                text(created_at),
            ],
        )
        .map_err(backend)?;
    if let Some(backlog_code) = &declaration.kontor_backlog_code {
        transaction
            .execute(
                "INSERT INTO epic_native_name_tokens
                     (project_id, mini_project_id, kontor_backlog_code,
                      ai_short_name, declared_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    project_id.to_string(),
                    mini_project_id.to_string(),
                    backlog_code.as_str(),
                    declaration.ai_short_name.as_ref().map(AiShortName::as_str),
                    text(created_at),
                ],
            )
            .map_err(backend)?;
    } else if declaration.ai_short_name.is_some() {
        return Err(DomainError::invalid(
            "epic native-name tokens",
            "AI_SHORT_NAME cannot be declared without KONTOR_BACKLOG_CODE",
        )
        .into());
    }
    Ok(Some(EpicExecutionScope {
        project_id,
        mini_project_id,
        external_epic_key: declaration.external_epic_key.clone(),
        short_title: declaration.short_title.clone(),
        kontor_backlog_code: declaration.kontor_backlog_code.clone(),
        ai_short_name: declaration.ai_short_name.clone(),
        created_at,
    }))
}

fn ensure_epic_backlog_code(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    manual_override: Option<&EpicBacklogCode>,
    assigned_at: Timestamp,
) -> RepositoryResult<EpicBacklogCode> {
    let existing: Option<String> = transaction
        .query_row(
            "SELECT code FROM epic_backlog_codes
             WHERE project_id = ?1 AND mini_project_id = ?2 AND status = 'active'",
            params![project_id.to_string(), mini_project_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    if let Some(code) = existing {
        let code = EpicBacklogCode::parse(code)?;
        if manual_override.is_some_and(|declared| declared != &code) {
            return Err(conflict(
                "epic backlog code",
                "the epic already has a different durable backlog code",
            ));
        }
        return Ok(code);
    }

    let title: Option<String> = transaction
        .query_row(
            "SELECT name FROM mini_projects WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), mini_project_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    let title = title.ok_or(RepositoryError::NotFound {
        subject: "mini project",
    })?;
    let mut statement = transaction
        .prepare(
            "SELECT code FROM epic_backlog_codes
             WHERE project_id = ?1 AND status = 'active' ORDER BY code COLLATE NOCASE",
        )
        .map_err(backend)?;
    let used = statement
        .query_map([project_id.to_string()], |row| row.get::<_, String>(0))
        .map_err(backend)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(backend)?;
    drop(statement);

    let (code, provenance) = if let Some(manual) = manual_override {
        if used
            .iter()
            .any(|current| current.eq_ignore_ascii_case(manual.as_str()))
        {
            return Err(conflict(
                "epic backlog code",
                "the requested code is already assigned in this project",
            ));
        }
        (manual.clone(), "manual")
    } else {
        (
            EpicBacklogCode::allocate(
                &ExternalName::parse(&title)?,
                used.iter().map(String::as_str),
            )?,
            "automatic",
        )
    };
    transaction
        .execute(
            "INSERT INTO epic_backlog_codes
                 (project_id, mini_project_id, code, provenance, status, assigned_at)
             VALUES (?1, ?2, ?3, ?4, 'active', ?5)",
            params![
                project_id.to_string(),
                mini_project_id.to_string(),
                code.as_str(),
                provenance,
                text(assigned_at),
            ],
        )
        .map_err(backend)?;
    Ok(code)
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

    let (task, mut applied) = match existing {
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
            // What a reapply is judged against is the *declaration*, never the
            // progress made since. The current state is deliberately not
            // compared: the first native transition clears the provenance and
            // moves the state, and an identical manifest has to stay replayable
            // after work has started — which is the oldest promise this contract
            // makes.
            if task
                .imported_state
                .is_some_and(|state| state != plan.imported_state)
            {
                return Err(conflict(
                    "epic task import state",
                    "the task already exists with a contradictory historical lifecycle fact",
                ));
            }
            // No provenance means one of two shapes: a task imported before v42,
            // or one this Realm has since transitioned itself. Reapplying the
            // compatibility default over either is idempotent. Declaring it
            // historically `completed` is not, because that relabels work Kontor
            // owns as work it merely inherited.
            if task.imported_state.is_none() && plan.imported_state != ImportedTaskState::Ready {
                return Err(conflict(
                    "epic task import state",
                    "an existing task without import provenance cannot be relabelled historical",
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
                          revision, created_at, updated_at, imported_state)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7, ?8)",
                    params![
                        id.to_string(),
                        request.project_id.to_string(),
                        mini_project_id.to_string(),
                        plan.title.as_str(),
                        plan.module.as_ref().map(ModuleKey::as_str),
                        plan.imported_state.task_state().as_str(),
                        text(request.applied_at),
                        plan.imported_state.as_str()
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
                    state: plan.imported_state.task_state(),
                    imported_state: Some(plan.imported_state),
                    revision: AggregateRevision::INITIAL,
                    created_at: request.applied_at,
                    updated_at: request.applied_at,
                },
                Applied::Created,
            )
        }
    };

    let short_code = ensure_task_short_code(
        transaction,
        request.project_id,
        task.id,
        plan.short_code.as_ref(),
        request.applied_at,
    )?;
    if short_code.inserted && applied == Applied::Unchanged {
        applied = Applied::Updated;
    }
    let ai_short_name = ensure_task_ai_short_name(
        transaction,
        request.project_id,
        task.id,
        plan.ai_short_name.as_ref(),
        request.applied_at,
    )?;
    if ai_short_name.inserted && applied == Applied::Unchanged {
        applied = Applied::Updated;
    }

    ensure_changed_modules(
        transaction,
        request.project_id,
        task.id,
        plan.module.as_ref(),
        plan.changed_modules.as_ref(),
        request.applied_at,
    )?;

    // Declared inside the epic's own transaction, so a graph never half-applies
    // into a state where a task exists and its placement does not.
    if let Some(worktree) = &plan.worktree {
        upsert_worktree(
            transaction,
            request.project_id,
            task.id,
            worktree,
            request.applied_at,
        )?;
    }

    let workflow_id = ensure_workflow(transaction, request, task.id)?;
    Ok(AppliedTask {
        title: plan.title.clone(),
        task_id: task.id,
        short_code: short_code.value,
        ai_short_name: ai_short_name.value,
        applied,
        state: task.state,
        revision: task.revision,
        workflow_id,
        depends_on: BTreeSet::new(),
        links: Vec::new(),
        worktree: read_worktree(transaction, request.project_id, task.id)?,
    })
}

struct EnsuredTaskAiShortName {
    value: Option<AiShortName>,
    inserted: bool,
}

fn ensure_task_ai_short_name(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
    declared: Option<&AiShortName>,
    declared_at: Timestamp,
) -> RepositoryResult<EnsuredTaskAiShortName> {
    let existing: Option<String> = transaction
        .query_row(
            "SELECT ai_short_name FROM task_ai_short_names
             WHERE project_id = ?1 AND task_id = ?2",
            params![project_id.to_string(), task_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    if let Some(existing) = existing {
        let existing = AiShortName::parse(&existing)?;
        if declared.is_some_and(|declared| declared != &existing) {
            return Err(conflict(
                "task AI short name",
                "the task already has a different durable AI short name",
            ));
        }
        return Ok(EnsuredTaskAiShortName {
            value: Some(existing),
            inserted: false,
        });
    }
    let Some(declared) = declared else {
        return Ok(EnsuredTaskAiShortName {
            value: None,
            inserted: false,
        });
    };
    transaction
        .execute(
            "INSERT INTO task_ai_short_names
                 (project_id, task_id, ai_short_name, declared_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                project_id.to_string(),
                task_id.to_string(),
                declared.as_str(),
                text(declared_at),
            ],
        )
        .map_err(backend)?;
    Ok(EnsuredTaskAiShortName {
        value: Some(declared.clone()),
        inserted: true,
    })
}

struct EnsuredTaskShortCode {
    value: Option<ExternalId>,
    inserted: bool,
}

fn ensure_task_short_code(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
    declared: Option<&ExternalId>,
    declared_at: Timestamp,
) -> RepositoryResult<EnsuredTaskShortCode> {
    let existing: Option<String> = transaction
        .query_row(
            "SELECT short_code FROM task_short_codes
             WHERE project_id = ?1 AND task_id = ?2",
            params![project_id.to_string(), task_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    if let Some(existing) = existing {
        let existing = ExternalId::parse(&existing)?;
        if declared.is_some_and(|declared| declared != &existing) {
            return Err(conflict(
                "task short code",
                "the task already has a different durable short code",
            ));
        }
        return Ok(EnsuredTaskShortCode {
            value: Some(existing),
            inserted: false,
        });
    }
    let Some(declared) = declared else {
        return Ok(EnsuredTaskShortCode {
            value: None,
            inserted: false,
        });
    };
    transaction
        .execute(
            "INSERT INTO task_short_codes
                 (project_id, task_id, short_code, source, declared_at)
             VALUES (?1, ?2, ?3, 'import', ?4)",
            params![
                project_id.to_string(),
                task_id.to_string(),
                declared.as_str(),
                text(declared_at),
            ],
        )
        .map_err(backend)?;
    Ok(EnsuredTaskShortCode {
        value: Some(declared.clone()),
        inserted: true,
    })
}

/// One task's declared worktree, read inside an open transaction.
fn read_worktree(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
) -> RepositoryResult<Option<ExternalName>> {
    let found: Option<String> = transaction
        .query_row(
            "SELECT worktree FROM task_worktrees WHERE project_id = ?1 AND task_id = ?2",
            params![project_id.to_string(), task_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    Ok(found.as_deref().map(ExternalName::parse).transpose()?)
}

/// Declare one task's worktree inside an open transaction.
fn upsert_worktree(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
    worktree: &ExternalName,
    declared_at: Timestamp,
) -> RepositoryResult<()> {
    transaction
        .execute(
            "INSERT INTO task_worktrees (project_id, task_id, worktree, declared_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (project_id, task_id)
             DO UPDATE SET worktree = excluded.worktree, declared_at = excluded.declared_at",
            params![
                project_id.to_string(),
                task_id.to_string(),
                worktree.as_str(),
                text(declared_at)
            ],
        )
        .map_err(backend)?;
    Ok(())
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

fn extras_without_primary(
    declared: &BTreeSet<ModuleKey>,
    primary: Option<&ModuleKey>,
) -> BTreeSet<ModuleKey> {
    let mut extras = BTreeSet::<ModuleKey>::new();
    for key in declared {
        if primary.is_some_and(|primary| primary.contends_with(key)) {
            continue;
        }
        if extras.iter().any(|existing| existing.contends_with(key)) {
            continue;
        }
        extras.insert(key.clone());
    }
    extras
}

fn read_changed_modules(
    connection: &Connection,
    project_id: ProjectId,
    task_id: TaskId,
) -> RepositoryResult<BTreeSet<ModuleKey>> {
    let mut statement = connection
        .prepare(
            "SELECT module_key FROM task_modules
             WHERE project_id = ?1 AND task_id = ?2
             ORDER BY module_key",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), task_id.to_string()])
        .map_err(backend)?;
    let mut extras = BTreeSet::<ModuleKey>::new();
    while let Some(row) = rows.next().map_err(backend)? {
        extras.insert(ModuleKey::parse(
            &row.get::<_, String>(0).map_err(backend)?,
        )?);
    }
    Ok(extras)
}

fn ensure_changed_modules(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
    primary: Option<&ModuleKey>,
    declared: Option<&BTreeSet<ModuleKey>>,
    declared_at: Timestamp,
) -> RepositoryResult<()> {
    let Some(declared) = declared else {
        return Ok(());
    };
    let declared = extras_without_primary(declared, primary);
    let existing = read_changed_modules(transaction, project_id, task_id)?;
    if existing == declared {
        return Ok(());
    }
    if !existing.is_empty() {
        return Err(conflict(
            "epic task",
            "already exists in this epic changing a different extra-module set",
        ));
    }
    for module in &declared {
        transaction
            .execute(
                "INSERT INTO task_modules (project_id, task_id, module_key, declared_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    project_id.to_string(),
                    task_id.to_string(),
                    module.as_str(),
                    text(declared_at)
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
        let connector = if is_jira_connector(&link.connector) {
            canonical_jira_connector()
        } else {
            link.connector.clone()
        };
        if !stated.insert((connector.clone(), link.external_issue_key.clone())) {
            return Err(conflict(
                "ticket link",
                "the same external issue is linked twice to one task",
            ));
        }
        if is_jira_connector(&connector) {
            let existing_for_task: Option<(String, String)> = transaction
                .query_row(
                    "SELECT link_id, external_issue_key
                     FROM canonical_jira_task_links
                     WHERE project_id = ?1 AND task_id = ?2",
                    params![request.project_id.to_string(), task_id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(backend)?;
            if let Some((id, issue_key)) = existing_for_task {
                if issue_key != link.external_issue_key.as_str() {
                    return Err(conflict(
                        "Jira task link",
                        "one task cannot be linked to more than one Jira issue",
                    ));
                }
                applied.push(AppliedLink {
                    id: TicketLinkId::parse(&id)?,
                    connector,
                    external_issue_key: link.external_issue_key.clone(),
                    applied: Applied::Unchanged,
                });
                continue;
            }
            let owner_for_key: Option<String> = transaction
                .query_row(
                    "SELECT task_id FROM canonical_jira_task_links
                     WHERE project_id = ?1 AND external_issue_key = ?2",
                    params![
                        request.project_id.to_string(),
                        link.external_issue_key.as_str()
                    ],
                    |row| row.get(0),
                )
                .optional()
                .map_err(backend)?;
            if owner_for_key.is_some() {
                return Err(conflict(
                    "Jira task link",
                    "one Jira issue cannot be linked to more than one task",
                ));
            }
        }
        // A link is unique per `(project, connector, issue)`, so a second task
        // claiming the same issue is refused — including one in another epic.
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT id, task_id FROM jira_links
                 WHERE project_id = ?1 AND connector = ?2 AND external_issue_key = ?3",
                params![
                    request.project_id.to_string(),
                    connector.as_str(),
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
                connector,
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
                    connector.as_str(),
                    link.external_issue_key.as_str(),
                    text(request.applied_at)
                ],
            )
            .map_err(backend)?;
        if is_jira_connector(&connector) {
            transaction
                .execute(
                    "INSERT INTO canonical_jira_task_links
                         (project_id, task_id, external_issue_key, link_id)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        request.project_id.to_string(),
                        task_id.to_string(),
                        link.external_issue_key.as_str(),
                        id.to_string(),
                    ],
                )
                .map_err(backend)?;
        }
        applied.push(AppliedLink {
            id,
            connector,
            external_issue_key: link.external_issue_key.clone(),
            applied: Applied::Created,
        });
    }
    Ok(applied)
}

// ---------------------------------------------------------------------------
// Registered profile packs
// ---------------------------------------------------------------------------

/// One operator-registered profile pack revision, as it was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredPack {
    /// The pack's open id.
    pub pack_id: String,
    /// This revision.
    pub version: SpecVersion,
    /// The canonical document, byte-for-byte as it was admitted.
    pub document: String,
    /// Its digest.
    pub document_hash: ContentHash,
    /// When it was registered.
    pub registered_at: Timestamp,
}

impl SqliteStore {
    /// Register one profile-pack revision, or prove the one already stored under
    /// that `(pack_id, version)` is the same document.
    ///
    /// Returns [`Applied::Unchanged`] with the stored revision when the digests
    /// match, and refuses with a conflict when they do not — a revision is
    /// immutable, so the same version carrying different bytes is a publishing
    /// mistake and never an update.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] for a revision that already exists
    /// with different content, and a backend error otherwise.
    pub fn register_profile_pack(
        &self,
        pack: &RegisteredPack,
        binding: &IdempotencyBinding,
    ) -> RepositoryResult<(RegisteredPack, Applied)> {
        let transaction = self.begin()?;

        // The key is judged first, and against the *logical operation* rather
        // than against the pack alone. A key already bound to a different
        // fingerprint was used for something else, and answering it here would
        // let one key stand for two registrations.
        let bound: Option<(String, String)> = transaction
            .query_row(
                "SELECT operation, fingerprint FROM realm_idempotency_bindings
                 WHERE idempotency_key = ?1",
                params![binding.key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        let replayed = match bound {
            Some((operation, fingerprint))
                if operation == binding.operation
                    && fingerprint == binding.fingerprint.as_str() =>
            {
                true
            }
            Some(_) => {
                return Err(conflict(
                    "idempotency key",
                    "this key is already bound to a different operation",
                ));
            }
            None => {
                transaction
                    .execute(
                        "INSERT INTO realm_idempotency_bindings
                             (idempotency_key, operation, fingerprint, bound_at)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![
                            binding.key,
                            binding.operation,
                            binding.fingerprint.as_str(),
                            text(binding.bound_at)
                        ],
                    )
                    .map_err(backend)?;
                false
            }
        };

        // Content is judged second, and independently: the same revision with
        // different bytes is a conflict whatever key it arrives under, because a
        // revision is immutable and a fresh key cannot buy an edit.
        let existing = read_pack_row(&transaction, &pack.pack_id, pack.version)?;
        if let Some(existing) = existing {
            if existing.document_hash != pack.document_hash {
                return Err(conflict(
                    "profile pack",
                    "this pack revision is already registered with different content",
                ));
            }
            transaction.commit().map_err(backend)?;
            return Ok((existing, Applied::Unchanged));
        }
        // A replayed key whose pack is absent means the first attempt bound the
        // key and then failed. Registering now is the convergent answer: the
        // binding already names this exact operation, so nothing else can be
        // claiming it.
        let _ = replayed;
        transaction
            .execute(
                "INSERT INTO registered_profile_packs
                     (pack_id, version, document, document_hash, registered_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    pack.pack_id,
                    version_column(pack.version),
                    pack.document,
                    pack.document_hash.as_str(),
                    text(pack.registered_at)
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok((pack.clone(), Applied::Created))
    }

    /// Read one registered pack revision.
    ///
    /// # Errors
    /// Refuses a stored document whose bytes no longer match the digest they
    /// were admitted under.
    pub fn get_profile_pack(
        &self,
        pack_id: &str,
        version: SpecVersion,
    ) -> RepositoryResult<Option<RegisteredPack>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT pack_id, version, document, document_hash, registered_at
                 FROM registered_profile_packs
                 WHERE pack_id = ?1 AND version = ?2",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![pack_id, version_column(version)])
            .map_err(backend)?;
        let Some(row) = rows.next().map_err(backend)? else {
            return Ok(None);
        };
        read_pack(row).map(Some)
    }

    /// Every registered pack revision, oldest first.
    ///
    /// # Errors
    /// As [`SqliteStore::get_profile_pack`].
    pub fn list_profile_packs(&self) -> RepositoryResult<Vec<RegisteredPack>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT pack_id, version, document, document_hash, registered_at
                 FROM registered_profile_packs
                 ORDER BY registered_at, pack_id, version",
            )
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut packs = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            packs.push(read_pack(row)?);
        }
        Ok(packs)
    }
}

/// One key, bound to the logical operation it was first used for.
///
/// The fingerprint is what makes a key mean something narrower than "some
/// registration happened": it is the digest of a canonical document naming the
/// operation and everything that identifies *this* one, so a key reused for a
/// different pack, a different revision or different bytes is refused rather
/// than quietly succeeding a second time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyBinding {
    /// The caller's key.
    pub key: String,
    /// Which realm-scoped operation it is bound to.
    pub operation: &'static str,
    /// The digest identifying this operation.
    pub fingerprint: ContentHash,
    /// When the binding was made.
    pub bound_at: Timestamp,
}

/// One pack row read inside an open transaction.
fn read_pack_row(
    transaction: &Transaction<'_>,
    pack_id: &str,
    version: SpecVersion,
) -> RepositoryResult<Option<RegisteredPack>> {
    let mut statement = transaction
        .prepare(
            "SELECT pack_id, version, document, document_hash, registered_at
             FROM registered_profile_packs
             WHERE pack_id = ?1 AND version = ?2",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![pack_id, version_column(version)])
        .map_err(backend)?;
    let Some(row) = rows.next().map_err(backend)? else {
        return Ok(None);
    };
    read_pack(row).map(Some)
}

/// One registered pack row, re-proved against its own digest.
///
/// The digest is re-derived rather than trusted. A catalogue is what a frozen
/// epic's pinned profile is resolved from, so bytes that drifted underneath it
/// must be refused rather than silently resolved into a different phase DAG.
fn read_pack(row: &rusqlite::Row<'_>) -> RepositoryResult<RegisteredPack> {
    let document: String = row.get(2).map_err(backend)?;
    let stored: String = row.get(3).map_err(backend)?;
    let document_hash = ContentHash::of(document.as_bytes());
    if document_hash.as_str() != stored {
        return Err(RepositoryError::Backend {
            detail: "a registered profile pack no longer matches its digest".to_owned(),
        });
    }
    Ok(RegisteredPack {
        pack_id: row.get(0).map_err(backend)?,
        version: read_version(row.get(1).map_err(backend)?)?,
        document,
        document_hash,
        registered_at: read_timestamp(&row.get::<_, String>(4).map_err(backend)?)?,
    })
}

// ---------------------------------------------------------------------------
// Task worktrees
// ---------------------------------------------------------------------------

impl SqliteStore {
    /// Declare, or re-declare, where a task's work happens.
    ///
    /// Replaceable until a run has snapshotted it, exactly like the account
    /// selection: correcting where a task will run is a pre-run decision. What a
    /// run *did* use is the workspace binding its runtime issued, which lives
    /// with the run and is never this row.
    ///
    /// # Errors
    /// Refuses a dangling or cross-project task, and a path SQL can see is not
    /// absolute.
    pub fn set_task_worktree(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
        worktree: &ExternalName,
    ) -> RepositoryResult<Applied> {
        let transaction = self.begin()?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT worktree FROM task_worktrees WHERE project_id = ?1 AND task_id = ?2",
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        if existing.as_deref() == Some(worktree.as_str()) {
            return Ok(Applied::Unchanged);
        }
        let declared_at = text(Timestamp::now());
        if existing.is_some() {
            transaction
                .execute(
                    "UPDATE task_worktrees SET worktree = ?3, declared_at = ?4
                     WHERE project_id = ?1 AND task_id = ?2",
                    params![
                        project_id.to_string(),
                        task_id.to_string(),
                        worktree.as_str(),
                        declared_at
                    ],
                )
                .map_err(backend)?;
        } else {
            transaction
                .execute(
                    "INSERT INTO task_worktrees (project_id, task_id, worktree, declared_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        project_id.to_string(),
                        task_id.to_string(),
                        worktree.as_str(),
                        declared_at
                    ],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;
        Ok(Applied::Created)
    }

    /// Additional modules this task changes, besides `tasks.module_key`.
    ///
    /// Empty when the task contends for at most its primary module. The primary
    /// is never returned here even if a caller stored it by mistake.
    ///
    /// # Errors
    /// Backend failures, and a stored key this build cannot parse.
    pub fn task_changed_modules(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<BTreeSet<ModuleKey>> {
        read_changed_modules(&self.connection, project_id, task_id)
    }

    /// Where a task's work happens, if it has been declared.
    ///
    /// `None` means nobody said, which is a refusal to seat rather than a
    /// licence to invent one: a control plane that guesses a path decides where
    /// code gets edited by string formatting.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn task_worktree(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<ExternalName>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT worktree FROM task_worktrees WHERE project_id = ?1 AND task_id = ?2",
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(found.as_deref().map(ExternalName::parse).transpose()?)
    }

    /// The task's compact, operator-declared runtime display identity.
    ///
    /// `None` is an intentional placement refusal. Callers must not derive a
    /// replacement from a description, ticket key, UUID or filesystem path.
    pub fn task_short_code(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<ExternalId>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT short_code FROM task_short_codes
                 WHERE project_id = ?1 AND task_id = ?2",
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(found.as_deref().map(ExternalId::parse).transpose()?)
    }

    /// The task's immutable intake-time two-keyword summary.
    ///
    /// `None` is a missing template token, never permission to regenerate it
    /// from the task title.
    pub fn task_ai_short_name(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Option<AiShortName>> {
        let found: Option<String> = self
            .connection
            .query_row(
                "SELECT ai_short_name FROM task_ai_short_names
                 WHERE project_id = ?1 AND task_id = ?2",
                params![project_id.to_string(), task_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        Ok(found.as_deref().map(AiShortName::parse).transpose()?)
    }
}

// ---------------------------------------------------------------------------
// Runtime binding snapshots
// ---------------------------------------------------------------------------

/// One binding's frozen snapshot, as it was persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredBindingSnapshot {
    /// The binding it belongs to.
    pub binding_id: RuntimeBindingId,
    /// The run that binding serves.
    pub agent_run_id: AgentRunId,
    /// The snapshot document, byte-for-byte as it was recorded.
    pub document: String,
}

impl SqliteStore {
    /// Keep the frozen snapshot a runtime issued for one binding.
    ///
    /// Replaceable, because a rebind for the same binding id issues a new
    /// snapshot and the newest one is the claim the next restart must present.
    ///
    /// # Errors
    /// Backend failures only. This is a claim, not authority, so nothing here
    /// judges it — the issuing runtime does that when it is handed back.
    pub fn persist_binding_snapshot(
        &self,
        binding_id: RuntimeBindingId,
        agent_run_id: AgentRunId,
        document: &str,
    ) -> RepositoryResult<()> {
        self.connection
            .execute(
                "INSERT INTO runtime_binding_snapshots
                     (binding_id, agent_run_id, document, document_hash, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (binding_id) DO UPDATE SET
                     agent_run_id = excluded.agent_run_id,
                     document = excluded.document,
                     document_hash = excluded.document_hash,
                     recorded_at = excluded.recorded_at",
                params![
                    binding_id.to_string(),
                    agent_run_id.to_string(),
                    document,
                    ContentHash::of(document.as_bytes()).as_str(),
                    text(Timestamp::now())
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Forget one binding's snapshot, as a closed run must.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn forget_binding_snapshot(&self, binding_id: RuntimeBindingId) -> RepositoryResult<()> {
        self.connection
            .execute(
                "DELETE FROM runtime_binding_snapshots WHERE binding_id = ?1",
                params![binding_id.to_string()],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Every persisted snapshot, in binding order.
    ///
    /// # Errors
    /// Refuses a document whose bytes no longer match the digest they were
    /// recorded under: a claim edited underneath the daemon is not the claim
    /// this Realm made, and handing it to a runtime to attest would at best
    /// waste the round trip and at worst present something nobody wrote.
    pub fn list_binding_snapshots(&self) -> RepositoryResult<Vec<StoredBindingSnapshot>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT binding_id, agent_run_id, document, document_hash
                 FROM runtime_binding_snapshots
                 ORDER BY binding_id",
            )
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let document: String = row.get(2).map_err(backend)?;
            let stored: String = row.get(3).map_err(backend)?;
            if ContentHash::of(document.as_bytes()).as_str() != stored {
                return Err(RepositoryError::Backend {
                    detail: "a runtime binding snapshot no longer matches its digest".to_owned(),
                });
            }
            found.push(StoredBindingSnapshot {
                binding_id: RuntimeBindingId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
                agent_run_id: AgentRunId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
                document,
            });
        }
        Ok(found)
    }
}

// ---------------------------------------------------------------------------
// Bounded role turns
// ---------------------------------------------------------------------------

/// One bounded Kontor role turn, as it is settled.
///
/// It attests that *Kontor's* turn finished, under a named actor's authority,
/// against a named task revision and native binding generation. It is not, and
/// must never be read as, evidence that the runtime ended anything: the seat is
/// A waiver about to be recorded for a declared, never-bound role slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRoleSlotWaiver {
    /// The waiver.
    pub id: kontor_core::id::RoleSlotWaiverId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task the team serves.
    pub task_id: TaskId,
    /// The team run whose slot is excused.
    pub team_run_id: TeamRunId,
    /// The slot being excused.
    pub role_slot_id: RoleSlotId,
    /// The caller's stable key.
    pub idempotency_key: String,
    /// The team revision the caller presented. Checked, never trusted.
    pub expected_team_revision: AggregateRevision,
    /// The role the waiver is attributed to. Policy attribution, not a person.
    pub authorized_role: String,
    /// The evidence the waiver cites.
    pub evidence: Vec<String>,
    /// The canonical digest of everything above that is not incidental.
    pub evidence_hash: ContentHash,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// One recorded waiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredWaiver {
    /// The waiver.
    pub id: kontor_core::id::RoleSlotWaiverId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task the team serves.
    pub task_id: TaskId,
    /// The team run whose slot was excused.
    pub team_run_id: TeamRunId,
    /// The excused slot.
    pub role_slot_id: RoleSlotId,
    /// The key it was recorded under.
    pub idempotency_key: String,
    /// The team revision it was taken against.
    pub team_run_revision: AggregateRevision,
    /// The role it is attributed to.
    pub authorized_role: String,
    /// The tier the credential proved. Always `admin`.
    pub authority_tier: String,
    /// The evidence cited.
    pub evidence: Vec<String>,
    /// The canonical digest.
    pub evidence_hash: ContentHash,
    /// When it was recorded.
    pub recorded_at: Timestamp,
}

/// expected to still be sitting there, ready for the next turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRoleTurn {
    /// The receipt id.
    pub id: RoleTurnId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The task the turn served.
    pub task_id: TaskId,
    /// The team run it belongs to.
    pub team_run_id: TeamRunId,
    /// The seat's agent run. It stays open.
    pub agent_run_id: AgentRunId,
    /// The role slot the turn was taken in.
    pub role_slot_id: RoleSlotId,
    /// The caller's stable key.
    pub idempotency_key: String,
    /// The task revision the caller presented.
    pub task_revision: AggregateRevision,
    /// The native binding generation the seat was bound under.
    pub binding_generation: u64,
    /// The tier the settling caller authenticated at. Truthful by construction:
    /// it is what the bearer proved, not what a request body claimed.
    pub authority_tier: &'static str,
    /// The provider account the seat runs as, derived from the bound run.
    pub account_profile: Option<AccountProfileId>,
    /// The artifacts the turn produced, in canonical order.
    pub artifacts: BTreeSet<ArtifactKey>,
    /// Digest over the settled turn's identifying content.
    pub evidence_hash: ContentHash,
    /// When it was settled.
    pub settled_at: Timestamp,
}

/// One settled role turn, as it was stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettledTurn {
    /// The receipt.
    pub id: RoleTurnId,
    /// The task it served.
    pub task_id: TaskId,
    /// The team run.
    pub team_run_id: TeamRunId,
    /// The seat's agent run.
    pub agent_run_id: AgentRunId,
    /// The role slot.
    pub role_slot_id: RoleSlotId,
    /// Its position in that seat's sequence of turns.
    pub turn_ordinal: u32,
    /// The artifacts it produced.
    pub artifacts: BTreeSet<ArtifactKey>,
    /// The digest it was settled under.
    pub evidence_hash: ContentHash,
    /// The native binding generation.
    pub binding_generation: u64,
    /// When it was settled.
    pub settled_at: Timestamp,
}

/// One follow-up a settled turn derived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnDispatch {
    /// The turn that derived it.
    pub settled_turn_id: RoleTurnId,
    /// The slot it hands to.
    pub to_role_slot_id: RoleSlotId,
    /// Owning project.
    pub project_id: ProjectId,
    /// The team run both slots belong to.
    pub team_run_id: TeamRunId,
    /// The message this follow-up is delivered as. Fixed at derivation, so a
    /// retry of an undelivered row is the same message and not a second effect.
    pub message_id: String,
    /// The seat it targeted, once one was found.
    pub target_agent_run: Option<AgentRunId>,
    /// Whether the effect actually reached the seat.
    pub dispatched: bool,
    /// When it was derived.
    pub derived_at: Timestamp,
}

impl SqliteStore {
    /// Settle one bounded role turn, or return the turn that key already
    /// settled.
    ///
    /// The ordinal is allocated inside the transaction from the seat's own
    /// sequence, so two settlements racing for the same seat cannot both take
    /// position *n*. Nothing here touches `agent_runs`: the seat stays open and
    /// its binding stays live, which is the whole point of a turn being a
    /// smaller thing than a run.
    ///
    /// # Errors
    /// Returns [`RepositoryError::Conflict`] when the key already settled a turn
    /// whose identifying content differs, and a backend error otherwise.
    pub fn settle_role_turn(&self, turn: &NewRoleTurn) -> RepositoryResult<(SettledTurn, Applied)> {
        self.record_role_turn(turn, false)
    }

    /// Record the sole late handoff disposition for a runtime-terminal run.
    ///
    /// The daemon proves why the run is terminal; this transaction only makes
    /// the no-prior-disposition rule atomic with the insert.
    ///
    /// # Errors
    /// Refuses a prior turn/waiver or a key reused for different content.
    pub fn attest_late_role_turn(
        &self,
        turn: &NewRoleTurn,
    ) -> RepositoryResult<(SettledTurn, Applied)> {
        self.record_role_turn(turn, true)
    }

    fn record_role_turn(
        &self,
        turn: &NewRoleTurn,
        require_unsettled_slot: bool,
    ) -> RepositoryResult<(SettledTurn, Applied)> {
        let transaction = self.begin()?;
        // Key first, and compared whole: a replay of the same settlement is the
        // original answer, and the same key naming a different turn is a
        // conflict rather than a second position in the sequence.
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT id, evidence_hash FROM role_turns WHERE idempotency_key = ?1",
                params![turn.idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((id, evidence)) = existing {
            if evidence != turn.evidence_hash.as_str() {
                return Err(conflict(
                    "role turn",
                    "this key already settled a turn with different content",
                ));
            }
            let settled = read_turn(&transaction, &id)?.ok_or(RepositoryError::NotFound {
                subject: "settled role turn",
            })?;
            return Ok((settled, Applied::Unchanged));
        }

        if require_unsettled_slot {
            let prior_turn: bool = transaction
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM role_turns
                         WHERE project_id = ?1 AND team_run_id = ?2
                           AND agent_run_id = ?3 AND role_slot_id = ?4
                     )",
                    params![
                        turn.project_id.to_string(),
                        turn.team_run_id.to_string(),
                        turn.agent_run_id.to_string(),
                        turn.role_slot_id.as_role_key().as_str(),
                    ],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            let prior_waiver: bool = transaction
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM role_slot_waivers
                         WHERE project_id = ?1 AND team_run_id = ?2 AND role_slot_id = ?3
                     )",
                    params![
                        turn.project_id.to_string(),
                        turn.team_run_id.to_string(),
                        turn.role_slot_id.as_role_key().as_str(),
                    ],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            if prior_turn || prior_waiver {
                return Err(conflict(
                    "late role handoff",
                    "the role slot already has a durable disposition",
                ));
            }
        }

        let ordinal: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(turn_ordinal), 0) + 1 FROM role_turns
                 WHERE project_id = ?1 AND agent_run_id = ?2 AND role_slot_id = ?3",
                params![
                    turn.project_id.to_string(),
                    turn.agent_run_id.to_string(),
                    turn.role_slot_id.as_role_key().as_str()
                ],
                |row| row.get(0),
            )
            .map_err(backend)?;
        let artifacts = to_json(
            &turn
                .artifacts
                .iter()
                .map(|key| key.as_str().to_owned())
                .collect::<Vec<_>>(),
        )?;
        transaction
            .execute(
                "INSERT INTO role_turns
                     (id, project_id, task_id, team_run_id, agent_run_id, role_slot_id,
                      turn_ordinal, idempotency_key, task_revision, binding_generation,
                      authority_tier, account_profile, artifacts, evidence_hash, settled_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    turn.id.to_string(),
                    turn.project_id.to_string(),
                    turn.task_id.to_string(),
                    turn.team_run_id.to_string(),
                    turn.agent_run_id.to_string(),
                    turn.role_slot_id.as_role_key().as_str(),
                    ordinal,
                    turn.idempotency_key,
                    crate::repository::revision_column(turn.task_revision)?,
                    i64::try_from(turn.binding_generation).unwrap_or(i64::MAX),
                    turn.authority_tier,
                    turn.account_profile.map(|id| id.to_string()),
                    artifacts,
                    turn.evidence_hash.as_str(),
                    text(turn.settled_at)
                ],
            )
            .map_err(backend)?;
        let settled =
            read_turn(&transaction, &turn.id.to_string())?.ok_or(RepositoryError::NotFound {
                subject: "settled role turn",
            })?;
        transaction.commit().map_err(backend)?;
        Ok((settled, Applied::Created))
    }

    /// Every turn settled on one task, oldest first.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list_settled_turns(
        &self,
        project_id: ProjectId,
        task_id: TaskId,
    ) -> RepositoryResult<Vec<SettledTurn>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, task_id, team_run_id, agent_run_id, role_slot_id, turn_ordinal,
                        artifacts, evidence_hash, binding_generation, settled_at
                 FROM role_turns
                 WHERE project_id = ?1 AND task_id = ?2
                 ORDER BY settled_at, turn_ordinal",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), task_id.to_string()])
            .map_err(backend)?;
        let mut turns = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            turns.push(turn_from_row(row)?);
        }
        Ok(turns)
    }

    /// Record a derived follow-up, or prove one was already derived.
    ///
    /// This is what makes successor activation *at most once*. The primary key
    /// is the settling turn plus the slot it hands to, so re-deriving the same
    /// follow-up — on a replayed settlement or on the next reconciliation
    /// re-reading the same facts — inserts nothing.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn derive_turn_dispatch(&self, dispatch: &TurnDispatch) -> RepositoryResult<Applied> {
        let changed = self
            .connection
            .execute(
                "INSERT INTO turn_dispatches
                     (settled_turn_id, to_role_slot_id, project_id, team_run_id,
                      message_id, target_agent_run, dispatched, derived_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (settled_turn_id, to_role_slot_id) DO NOTHING",
                params![
                    dispatch.settled_turn_id.to_string(),
                    dispatch.to_role_slot_id.as_role_key().as_str(),
                    dispatch.project_id.to_string(),
                    dispatch.team_run_id.to_string(),
                    dispatch.message_id,
                    dispatch.target_agent_run.map(|id| id.to_string()),
                    i64::from(dispatch.dispatched),
                    text(dispatch.derived_at)
                ],
            )
            .map_err(backend)?;
        Ok(if changed == 0 {
            Applied::Unchanged
        } else {
            Applied::Created
        })
    }

    /// Mark one derived follow-up as delivered.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn mark_turn_dispatched(
        &self,
        settled_turn_id: RoleTurnId,
        to_role_slot_id: &RoleSlotId,
        target: AgentRunId,
    ) -> RepositoryResult<()> {
        self.connection
            .execute(
                "UPDATE turn_dispatches
                 SET dispatched = 1, target_agent_run = ?3
                 WHERE settled_turn_id = ?1 AND to_role_slot_id = ?2",
                params![
                    settled_turn_id.to_string(),
                    to_role_slot_id.as_role_key().as_str(),
                    target.to_string()
                ],
            )
            .map_err(backend)?;
        Ok(())
    }

    /// Every follow-up derived from one task's turns.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn list_turn_dispatches(
        &self,
        project_id: ProjectId,
    ) -> RepositoryResult<Vec<TurnDispatch>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT settled_turn_id, to_role_slot_id, project_id, team_run_id,
                        message_id, target_agent_run, dispatched, derived_at
                 FROM turn_dispatches WHERE project_id = ?1
                 ORDER BY derived_at, to_role_slot_id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string()])
            .map_err(backend)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let target: Option<String> = row.get(5).map_err(backend)?;
            found.push(TurnDispatch {
                settled_turn_id: RoleTurnId::parse(&column_text(row, 0)?)?,
                to_role_slot_id: RoleSlotId::parse(&column_text(row, 1)?)?,
                project_id: ProjectId::parse(&column_text(row, 2)?)?,
                team_run_id: TeamRunId::parse(&column_text(row, 3)?)?,
                message_id: column_text(row, 4)?,
                target_agent_run: target.as_deref().map(AgentRunId::parse).transpose()?,
                dispatched: row.get::<_, i64>(6).map_err(backend)? == 1,
                derived_at: read_timestamp(&column_text(row, 7)?)?,
            });
        }
        Ok(found)
    }

    // -----------------------------------------------------------------------
    // Role slot waivers
    // -----------------------------------------------------------------------

    /// Record an authorized waiver for a declared slot that was never bound, and
    /// advance the team run's revision in the same transaction.
    ///
    /// Everything that makes the waiver legal is proved *here*, with the write
    /// lock held, and against the run's own frozen snapshot: the slot is
    /// declared, its frozen policy allows waiving, the citing role is one the
    /// policy authorizes and is not the slot's own role, every required evidence
    /// key is cited, the caller's revision is current, the team is not terminal,
    /// and the slot was never bound. Proving any of it earlier would prove it
    /// about a state that could have moved before the row landed.
    ///
    /// The never-bound predicate is re-checked here *and* enforced by trigger.
    /// The trigger is what makes it true of the data; this check is what makes
    /// the refusal say which rule refused.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] for an unknown team run.
    /// * [`RepositoryError::Conflict`] for a stale revision, a terminal team, a
    ///   slot already accounted for, or a slot that was ever bound.
    /// * [`DomainError`] for an undeclared slot, a slot the template does not
    ///   allow waiving, an unauthorized role, or missing evidence.
    pub fn waive_role_slot(
        &self,
        waiver: &NewRoleSlotWaiver,
    ) -> RepositoryResult<(StoredWaiver, Applied, AggregateRevision)> {
        let transaction = self.begin()?;
        let project = waiver.project_id.to_string();
        let team = waiver.team_run_id.to_string();
        let slot = waiver.role_slot_id.as_role_key().as_str();

        // A replay of the same waiver is the original answer. The same key with
        // different content is a conflict, never a second excuse.
        let existing: Option<(String, String)> = transaction
            .query_row(
                "SELECT id, evidence_hash FROM role_slot_waivers WHERE idempotency_key = ?1",
                params![waiver.idempotency_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(backend)?;
        if let Some((id, evidence)) = existing {
            if evidence != waiver.evidence_hash.as_str() {
                return Err(conflict(
                    "role slot waiver",
                    "this key already waived a role slot with different content",
                ));
            }
            let stored = read_waiver(&transaction, &id)?.ok_or(RepositoryError::NotFound {
                subject: "role slot waiver",
            })?;
            let current: i64 = transaction
                .query_row(
                    "SELECT revision FROM team_runs WHERE project_id = ?1 AND id = ?2",
                    params![project, team],
                    |row| row.get(0),
                )
                .map_err(backend)?;
            let revision = revision_of(current)?;
            return Ok((stored, Applied::Unchanged, revision));
        }

        let (snapshot, lifecycle, terminal, revision) = transaction
            .query_row(
                "SELECT snapshot, lifecycle, terminal_source_kind, revision
                 FROM team_runs WHERE project_id = ?1 AND id = ?2",
                params![project, team],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(backend)?
            .ok_or(RepositoryError::NotFound {
                subject: "team run",
            })?;
        let _ = lifecycle;
        if terminal.is_some() {
            return Err(conflict(
                "role slot waiver",
                "a closed team run cannot waive a role slot",
            ));
        }
        let stored_revision = AggregateRevision::parse(u64::try_from(revision).unwrap_or(1))?;
        if stored_revision != waiver.expected_team_revision {
            return Err(DomainError::RevisionConflict {
                subject: "team run",
                expected: waiver.expected_team_revision.get(),
                found: stored_revision.get(),
            }
            .into());
        }

        // The frozen policy, not the current catalog.
        let frozen: kontor_core::spec::TeamRunSnapshot = crate::repository::from_json(&snapshot)?;
        let policy = frozen.waiver_policy_for(&waiver.role_slot_id)?.ok_or(
            DomainError::MissingAuthority {
                subject: "role slot waiver",
                rule: "the frozen template does not allow this role slot to be waived",
            },
        )?;
        let citing = waiver.authorized_role.as_str();
        if citing == policy.own_role {
            return Err(DomainError::MissingAuthority {
                subject: "role slot waiver",
                rule: "a role slot cannot excuse itself",
            }
            .into());
        }
        if !policy.authorized_roles.iter().any(|role| role == citing) {
            return Err(DomainError::MissingAuthority {
                subject: "role slot waiver",
                rule: "the waiving role is not authorized for this role slot",
            }
            .into());
        }
        let cited: BTreeSet<&str> = waiver.evidence.iter().map(String::as_str).collect();
        if !policy
            .required_evidence
            .iter()
            .all(|required| cited.contains(required.as_str()))
        {
            return Err(DomainError::MissingEvidence {
                subject: "role slot waiver",
                rule: "a waiver must cite every evidence reference the slot requires",
            }
            .into());
        }

        // Already accounted for, either way round.
        let settled: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM role_turns
                 WHERE project_id = ?1 AND team_run_id = ?2 AND role_slot_id = ?3",
                params![project, team, slot],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if settled > 0 {
            return Err(conflict(
                "role slot waiver",
                "a role slot that settled a turn cannot be waived",
            ));
        }
        let waived: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM role_slot_waivers
                 WHERE project_id = ?1 AND team_run_id = ?2 AND role_slot_id = ?3",
                params![project, team, slot],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if waived > 0 {
            return Err(conflict(
                "role slot waiver",
                "this role slot is already waived",
            ));
        }

        // The binding *history*, which is the fact "unbound" is defined on. A
        // lost process or an unreachable runtime is not an unbound slot.
        let bound: i64 = transaction
            .query_row(
                "SELECT COUNT(*)
                 FROM runtime_bindings AS binding
                 JOIN agent_runs AS run
                   ON run.id = binding.agent_run_id AND run.project_id = binding.project_id
                 WHERE run.project_id = ?1 AND run.team_run_id = ?2 AND run.role_key = ?3",
                params![project, team, slot],
                |row| row.get(0),
            )
            .map_err(backend)?;
        if bound > 0 {
            return Err(conflict(
                "role slot waiver",
                "a role slot that was ever bound cannot be waived",
            ));
        }

        let evidence = to_json(&waiver.evidence)?;
        transaction
            .execute(
                "INSERT INTO role_slot_waivers
                     (id, project_id, task_id, team_run_id, role_slot_id, idempotency_key,
                      team_run_revision, authorized_role, authority_tier, evidence,
                      evidence_hash, recorded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    waiver.id.to_string(),
                    project,
                    waiver.task_id.to_string(),
                    team,
                    slot,
                    waiver.idempotency_key,
                    revision,
                    citing,
                    "admin",
                    evidence,
                    waiver.evidence_hash.as_str(),
                    text(waiver.recorded_at)
                ],
            )
            .map_err(backend)?;
        // Compare-and-set on the very revision that was validated above, so two
        // concurrent waivers cannot both believe they were current.
        let advanced = stored_revision.next()?;
        let changed = transaction
            .execute(
                "UPDATE team_runs SET revision = ?1
                 WHERE project_id = ?2 AND id = ?3 AND revision = ?4",
                params![
                    crate::repository::revision_column(advanced)?,
                    project,
                    team,
                    revision
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "role slot waiver",
                "the team run moved while the waiver was being recorded",
            ));
        }
        let stored = read_waiver(&transaction, &waiver.id.to_string())?.ok_or(
            RepositoryError::NotFound {
                subject: "role slot waiver",
            },
        )?;
        transaction.commit().map_err(backend)?;
        Ok((stored, Applied::Created, advanced))
    }

    /// Every waiver recorded against one team run, by slot.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the rows cannot be read.
    pub fn list_role_slot_waivers(
        &self,
        project_id: ProjectId,
        team_run_id: TeamRunId,
    ) -> RepositoryResult<Vec<StoredWaiver>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, project_id, task_id, team_run_id, role_slot_id, idempotency_key,
                        team_run_revision, authorized_role, authority_tier, evidence,
                        evidence_hash, recorded_at
                 FROM role_slot_waivers
                 WHERE project_id = ?1 AND team_run_id = ?2
                 ORDER BY role_slot_id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), team_run_id.to_string()])
            .map_err(backend)?;
        let mut found = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            found.push(waiver_from_row(row)?);
        }
        Ok(found)
    }
}

/// One waiver, read by id inside an open transaction.
fn read_waiver(transaction: &Transaction<'_>, id: &str) -> RepositoryResult<Option<StoredWaiver>> {
    let mut statement = transaction
        .prepare(
            "SELECT id, project_id, task_id, team_run_id, role_slot_id, idempotency_key,
                    team_run_revision, authorized_role, authority_tier, evidence,
                    evidence_hash, recorded_at
             FROM role_slot_waivers WHERE id = ?1",
        )
        .map_err(backend)?;
    let mut rows = statement.query(params![id]).map_err(backend)?;
    match rows.next().map_err(backend)? {
        Some(row) => Ok(Some(waiver_from_row(row)?)),
        None => Ok(None),
    }
}

fn waiver_from_row(row: &rusqlite::Row<'_>) -> RepositoryResult<StoredWaiver> {
    let evidence: Vec<String> = serde_json::from_str(&column_text(row, 9)?).map_err(|_| {
        DomainError::invalid("role slot waiver", "the cited evidence is unreadable")
    })?;
    Ok(StoredWaiver {
        id: kontor_core::id::RoleSlotWaiverId::parse(&column_text(row, 0)?)?,
        project_id: ProjectId::parse(&column_text(row, 1)?)?,
        task_id: TaskId::parse(&column_text(row, 2)?)?,
        team_run_id: TeamRunId::parse(&column_text(row, 3)?)?,
        role_slot_id: RoleSlotId::parse(&column_text(row, 4)?)?,
        idempotency_key: column_text(row, 5)?,
        team_run_revision: AggregateRevision::parse(
            u64::try_from(row.get::<_, i64>(6).map_err(backend)?).unwrap_or(1),
        )?,
        authorized_role: column_text(row, 7)?,
        authority_tier: column_text(row, 8)?,
        evidence,
        evidence_hash: ContentHash::parse(&column_text(row, 10)?)?,
        recorded_at: read_timestamp(&column_text(row, 11)?)?,
    })
}

/// One settled turn, read by id inside an open transaction.
fn read_turn(transaction: &Transaction<'_>, id: &str) -> RepositoryResult<Option<SettledTurn>> {
    let mut statement = transaction
        .prepare(
            "SELECT id, task_id, team_run_id, agent_run_id, role_slot_id, turn_ordinal,
                    artifacts, evidence_hash, binding_generation, settled_at
             FROM role_turns WHERE id = ?1",
        )
        .map_err(backend)?;
    let mut rows = statement.query(params![id]).map_err(backend)?;
    let Some(row) = rows.next().map_err(backend)? else {
        return Ok(None);
    };
    turn_from_row(row).map(Some)
}

/// One settled turn, from a row carrying the standard column order.
fn turn_from_row(row: &rusqlite::Row<'_>) -> RepositoryResult<SettledTurn> {
    let artifacts: Vec<String> = from_json(&column_text(row, 6)?)?;
    Ok(SettledTurn {
        id: RoleTurnId::parse(&column_text(row, 0)?)?,
        task_id: TaskId::parse(&column_text(row, 1)?)?,
        team_run_id: TeamRunId::parse(&column_text(row, 2)?)?,
        agent_run_id: AgentRunId::parse(&column_text(row, 3)?)?,
        role_slot_id: RoleSlotId::parse(&column_text(row, 4)?)?,
        turn_ordinal: u32::try_from(row.get::<_, i64>(5).map_err(backend)?).unwrap_or(u32::MAX),
        artifacts: artifacts
            .iter()
            .map(|key| ArtifactKey::parse(key))
            .collect::<Result<_, _>>()?,
        evidence_hash: ContentHash::parse(&column_text(row, 7)?)?,
        binding_generation: u64::try_from(row.get::<_, i64>(8).map_err(backend)?).unwrap_or(0),
        settled_at: read_timestamp(&column_text(row, 9)?)?,
    })
}
