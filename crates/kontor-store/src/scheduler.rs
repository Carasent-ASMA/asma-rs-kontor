//! Durable leases and the one transaction an admission has to be.
//!
//! `kontor-scheduler` decides; this module makes one decision durable and applies
//! its consequences. The split is the same one [`crate::policy`] draws: every
//! function here writes, and none of them re-decides a policy question. What they
//! *do* re-check is the handful of facts a decision made against a snapshot must
//! not have been wrong about by the time it commits.
//!
//! ## The one transaction that has to be one transaction
//!
//! [`SqliteStore::admit_candidate`] runs inside a single `BEGIN IMMEDIATE` and
//! writes, in this order:
//!
//! 1. the team run that is the task's top-level envelope;
//! 2. its first agent run, `queued`;
//! 3. the `launch_run` intent — receipt, normalized target, outbox entry, desired
//!    state and intent event — through [`crate::commands::intent::insert_intent`],
//!    which is the only path an intent reaches storage by;
//! 4. the immutable admission decision;
//! 5. the module and worktree leases, each with its `acquired` history event.
//!
//! There is deliberately no window between those steps. A crash anywhere leaves
//! either all of it or none of it, so a lease can never exist for a run that was
//! never queued, and a queued run can never exist without the claim that entitles
//! it to the module it is about to edit.
//!
//! **No runtime is contacted here.** The outbox entry written in step 3 is
//! dispatched after the transaction commits, which is what keeps a native call
//! from happening inside a write lock — and what makes a lost acknowledgement a
//! question about a receipt rather than about a half-written admission.
//!
//! ## What is re-checked, and why each one
//!
//! The snapshot the decision was made against is a *read*, and reads go stale.
//! Five facts are therefore proved again with the write lock held:
//!
//! * **the task's revision and state** — a task that was resumed, parked or
//!   cancelled since the snapshot is not the task that was admitted;
//! * **its dependencies** — a dependency that has not reached `done` blocks, and
//!   a dependency this transaction cannot see is not treated as finished;
//! * **its serialization peers** — a peer with an open run is exactly the
//!   collision the pass could not see, because the peer was admitted by *another
//!   scheduler instance* between the snapshot and this commit;
//! * **capacity** — recounted from the rows rather than trusted from the
//!   snapshot, so two instances cannot each admit the last unit of headroom;
//! * **the leases themselves** — pre-checked here for a typed refusal, and made
//!   unrepresentable by the realm-wide indexes and the exclusion trigger
//!   migration 0004 installs. The second half is what holds against a caller
//!   that never came through this function.
//!
//! Runtime and account evidence is deliberately *not* re-derived. It is
//! snapshotted from the modules that own it, and re-deciding trust here would put
//! a second opinion about it in a second place.
//!
//! ## Replay
//!
//! An admission is idempotent on the launch command's idempotency key. A caller
//! whose acknowledgement was lost retries with the same key, and the probe at the
//! top of the transaction finds the original decision and returns it having
//! written nothing — no second run, no second lease, no second launch. The
//! `ux_scheduler_admission_events_run` index is the structural half of the same
//! guarantee.
//!
//! ## Leases
//!
//! A lease is a claim on a place on disk, and the three things that can happen to
//! one are kept strictly apart:
//!
//! * **renewal** rotates the fencing token and pushes the expiry out. It requires
//!   the token currently on the row, so a holder that was asleep while someone
//!   else renewed cannot extend a claim it no longer owns;
//! * **release** is receipt-backed and requires the current token too;
//! * **expiry** is nobody's decision — it is the absence of a renewal. It frees
//!   the *resource* and concludes **nothing whatever** about the run that held it.
//!   Nothing in the expiry path reads or writes `agent_runs`: an absence is not a
//!   completion and not a failure, and a reclaimed module says only that the
//!   module is reclaimable.

use std::collections::BTreeSet;

use kontor_core::DomainError;
use kontor_core::id::{
    AccountProfileId, AgentRunId, AggregateRevision, CanonicalDocument, CommandReceiptId,
    ExternalId, ExternalName, IdempotencyKey, MiniProjectId, ProjectId, ResourceLeaseId, TaskId,
    Timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind, CommandReceipt};
use kontor_core::repository::{
    CommandRepository as _, NewAgentRun, NewCommandIntent, NewTeamRun, RepositoryError,
    RepositoryResult,
};
use kontor_core::state::TaskState;
use kontor_core::{DomainResult, closed_enum};
use kontor_policy::ModuleClaim;
use kontor_scheduler::{
    AdmissionEventId, AdmittedCandidate, CapacityConfig, RejectionCode, RejectionEvidence,
};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::SqliteStore;
use crate::commands::intent::insert_intent;
use crate::repository::{backend, conflict, from_json, read_timestamp, revision_of, text};

/// The lifecycle values an agent run is no longer occupying capacity in.
///
/// Spelled out rather than derived from a `NOT IN` over the open ones so that
/// adding a lifecycle value to the schema is a compile-time visit to this list
/// rather than a silent change of what "in flight" counts.
const TERMINAL_LIFECYCLES: &str = "'succeeded', 'failed', 'cancelled', 'parked'";

closed_enum! {
    /// What kind of place a lease claims.
    ///
    /// `resource_key` is one Realm-wide namespace across both kinds, exactly as it
    /// was in schema v1: the kind says how to read a key, it does not partition
    /// it. A module key and a worktree label are spelled differently, and if a
    /// deployment ever made them collide, refusing the overlap is the safe answer.
    LeaseKind, "LeaseKind" {
        /// A module or code area two tasks could edit at once.
        Module => "module",
        /// A worktree that isolates work from other work.
        Worktree => "worktree",
    }
}

closed_enum! {
    /// One thing that happened to a lease.
    LeaseEventKind, "LeaseEventKind" {
        /// The claim was taken.
        Acquired => "acquired",
        /// The claim was extended and its token rotated.
        Renewed => "renewed",
        /// The holder gave the claim up, with a receipt.
        Released => "released",
        /// The claim lapsed. Says nothing about the run that held it.
        Expired => "expired",
    }
}

/// One durable claim on a contended place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLease {
    /// The lease.
    pub id: ResourceLeaseId,
    /// The project the claiming run belongs to. Contention is *not* scoped to it.
    pub project_id: ProjectId,
    /// What kind of place is claimed.
    pub kind: LeaseKind,
    /// The place. One Realm-wide namespace.
    pub resource_key: ExternalName,
    /// The worktree that isolates this claim, when one does.
    pub worktree_key: Option<ExternalName>,
    /// The run the claim is held for.
    pub agent_run_id: AgentRunId,
    /// The scheduler instance responsible for renewing it.
    pub holder_instance: Option<ExternalId>,
    /// The token a renewal or a release must present.
    pub fencing_token: u64,
    /// When it was taken.
    pub acquired_at: Timestamp,
    /// When it lapses unless renewed.
    pub expires_at: Timestamp,
    /// When it was released, with a receipt.
    pub released_at: Option<Timestamp>,
    /// When it was found lapsed.
    pub expired_at: Option<Timestamp>,
    /// The expired lease this one took the place over from.
    pub renewed_from_lease_id: Option<ResourceLeaseId>,
    /// The admission that acquired it.
    pub admission_event_id: Option<AdmissionEventId>,
}

impl ResourceLease {
    /// Whether the claim still holds the place.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.released_at.is_none() && self.expired_at.is_none()
    }

    /// Whether the claim has lapsed at `now` without anyone having recorded it.
    #[must_use]
    pub fn is_lapsed(&self, now: Timestamp) -> bool {
        self.is_active() && self.expires_at <= now
    }
}

/// Everything one atomic admission needs.
///
/// The team run, the agent run and the launch intent arrive already assembled —
/// by `kontor-teams` and the caller — because this module records decisions and
/// does not make them. What it *does* own is that all three, the leases and the
/// decision become durable together.
#[derive(Debug, Clone)]
pub struct AdmissionCommit<'a> {
    /// The scheduler's decision about this task.
    pub admitted: &'a AdmittedCandidate,
    /// The peers this task may not run beside.
    ///
    /// Re-checked with the write lock held: this is the one collision the pass
    /// cannot see, because a peer admitted by another scheduler instance between
    /// the snapshot and this commit was not in the snapshot.
    pub serializes_with: &'a BTreeSet<TaskId>,
    /// The ceilings to recount against.
    pub capacity: CapacityConfig,
    /// The task's top-level envelope.
    pub team_run: NewTeamRun,
    /// Its first seat, which this transaction queues.
    pub agent_run: NewAgentRun,
    /// The `launch_run` intent for that seat, computed against
    /// [`AggregateRevision::INITIAL`] because the run is created here.
    pub launch: NewCommandIntent,
    /// The decision's id.
    pub admission_event_id: AdmissionEventId,
    /// The module lease's id. Present exactly when the task holds a module.
    pub module_lease_id: Option<ResourceLeaseId>,
    /// The worktree lease's id. Present exactly when the task holds a verified
    /// tree.
    pub worktree_lease_id: Option<ResourceLeaseId>,
    /// The scheduler instance that will renew the leases.
    pub holder_instance: ExternalId,
    /// When the leases lapse unless renewed. A duration from configuration,
    /// resolved against the caller's injected clock.
    pub lease_expires_at: Timestamp,
    /// The canonical decision evidence, stored byte-for-byte with its digest.
    pub evidence: CanonicalDocument,
    /// When the decision was made.
    pub decided_at: Timestamp,
}

impl AdmissionCommit<'_> {
    /// Prove the request's parts describe one admission of one task.
    ///
    /// Every check here is about *internal agreement*, so a mismatched request is
    /// refused before the transaction opens rather than producing rows that
    /// disagree with each other.
    ///
    /// # Errors
    /// Returns [`DomainError::Invalid`] for a launch that is not a `launch_run`
    /// against the run being created, a run that belongs to another project or
    /// another team, an account that is not the one admitted, a lease that is
    /// present without a place to claim (or absent with one), and an expiry that
    /// does not outlive the decision.
    fn validate(&self) -> DomainResult<()> {
        let invalid = |rule: &'static str| DomainError::invalid("AdmissionCommit", rule);
        let project = self.admitted.project_id;
        if self.team_run.project_id != project
            || self.agent_run.project_id != project
            || self.launch.project_id != project
        {
            return Err(invalid("every part of an admission belongs to one project"));
        }
        if self.team_run.task_id != self.admitted.task_id {
            return Err(invalid("the team run serves another task"));
        }
        if self.agent_run.team_run_id != self.team_run.id {
            return Err(invalid("the agent run belongs to another team run"));
        }
        if self.agent_run.account_profile_id != self.admitted.account_profile_id {
            return Err(invalid("the agent run is pinned to another account"));
        }
        if self.launch.kind != CommandKind::LaunchRun {
            return Err(invalid("an admission dispatches a launch, nothing else"));
        }
        if self.launch.target
            != (AggregateRef::AgentRun {
                agent_run_id: self.agent_run.id,
            })
        {
            return Err(invalid("the launch targets another run"));
        }
        if self.launch.target_revision != AggregateRevision::INITIAL {
            return Err(invalid(
                "the launch is computed against the revision of the run this transaction creates",
            ));
        }
        if self.module_lease_id.is_some() != self.admitted.module.is_some() {
            return Err(invalid(
                "a module lease is acquired exactly when the task holds a module",
            ));
        }
        if self.worktree_lease_id.is_some() != self.admitted.worktree.is_some() {
            return Err(invalid(
                "a worktree lease is acquired exactly when the task holds a verified tree",
            ));
        }
        if self.lease_expires_at <= self.decided_at {
            return Err(invalid("a lease must outlive the decision that took it"));
        }
        Ok(())
    }
}

/// What one admission produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionOutcome {
    /// The decision.
    pub admission_event_id: AdmissionEventId,
    /// The launch receipt the outbox will dispatch.
    pub receipt: CommandReceipt,
    /// The module lease acquired, if any.
    pub module_lease_id: Option<ResourceLeaseId>,
    /// The worktree lease acquired, if any.
    pub worktree_lease_id: Option<ResourceLeaseId>,
    /// Lapsed leases this admission recorded as expired to reclaim their places.
    pub reclaimed: Vec<ResourceLeaseId>,
    /// Whether this call found the admission already durable and wrote nothing.
    pub replayed: bool,
}

/// A renewal, presenting the token the holder believes is current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseRenewal {
    /// The project the lease belongs to.
    pub project_id: ProjectId,
    /// The lease.
    pub lease_id: ResourceLeaseId,
    /// The token the caller holds.
    pub presented_token: u64,
    /// The new expiry. Must move forward.
    pub expires_at: Timestamp,
    /// When the renewal happened.
    pub renewed_at: Timestamp,
}

/// A receipt-backed release, presenting the token the holder believes is current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeaseRelease {
    /// The project the lease belongs to.
    pub project_id: ProjectId,
    /// The lease.
    pub lease_id: ResourceLeaseId,
    /// The token the caller holds.
    pub presented_token: u64,
    /// The receipt that decided the release.
    pub receipt_id: CommandReceiptId,
    /// When it was released.
    pub released_at: Timestamp,
}

/// One refusal, as the audit trail records it.
#[derive(Debug, Clone)]
pub struct RecordedRejection {
    /// Owning project.
    pub project_id: ProjectId,
    /// The task that was refused.
    pub task_id: TaskId,
    /// The decision's id.
    pub admission_event_id: AdmissionEventId,
    /// Why it was refused.
    pub code: RejectionCode,
    /// The canonical evidence behind the refusal.
    pub evidence: CanonicalDocument,
}

impl RecordedRejection {
    /// Build a refusal record from a scheduler decision's evidence.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the evidence cannot be canonicalized, which is
    /// also where an oversized or secret-bearing document is refused before it can
    /// be stored.
    pub fn new(
        project_id: ProjectId,
        task_id: TaskId,
        code: RejectionCode,
        evidence: &[RejectionEvidence],
    ) -> DomainResult<Self> {
        let document = CanonicalDocument::from_serializable(&RejectionDocument {
            schema_version: kontor_core::id::SCHEMA_VERSION,
            code,
            evidence: evidence.to_vec(),
        })?;
        Ok(Self {
            project_id,
            task_id,
            admission_event_id: AdmissionEventId::generate(),
            code,
            evidence: document,
        })
    }
}

/// The exact shape a refusal's evidence is stored as.
#[derive(Debug, serde::Serialize)]
struct RejectionDocument {
    schema_version: kontor_core::id::SchemaVersion,
    code: RejectionCode,
    evidence: Vec<RejectionEvidence>,
}

// ---------------------------------------------------------------------------
// Reads for the snapshot
// ---------------------------------------------------------------------------

impl SqliteStore {
    /// Read the immutable scheduler decision behind one launch command.
    ///
    /// This is the recovery half of admission: the scheduler start may have
    /// committed the run before the runtime accepted its workspace. Retrying the
    /// same command must use the original decision rather than re-plan a task
    /// that is now correctly reported as already in flight.
    pub fn admitted_candidate_by_launch_key(
        &self,
        project_id: ProjectId,
        launch_key: &IdempotencyKey,
    ) -> RepositoryResult<Option<AdmittedCandidate>> {
        #[derive(serde::Deserialize)]
        struct StoredAdmission {
            admitted: AdmittedCandidate,
        }

        let evidence: Option<String> = self
            .connection
            .query_row(
                "SELECT event.evidence
                 FROM scheduler_admission_events AS event
                 JOIN command_receipts AS receipt
                   ON receipt.project_id = event.project_id
                  AND receipt.id = event.launch_receipt_id
                 WHERE event.project_id = ?1
                   AND event.decision = 'admitted'
                   AND receipt.idempotency_key = ?2",
                params![project_id.to_string(), launch_key.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(backend)?;
        evidence
            .map(|value| from_json::<StoredAdmission>(&value).map(|stored| stored.admitted))
            .transpose()
    }

    /// Every module currently claimed by an unlapsed lease, across the Realm.
    ///
    /// Deliberately not project-scoped. Every other read on this store is, because
    /// a globally unique id is not tenant isolation — but a module is a place on
    /// disk, and a project-scoped read of module contention cannot answer the
    /// question the scheduler is asking. The rows it returns carry the project
    /// each claim belongs to only through the task that holds it.
    ///
    /// A lease whose expiry has passed is *not* reported as held: the place is
    /// reclaimable, and the admission that reclaims it is what records the expiry.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn active_module_claims(&self, now: Timestamp) -> RepositoryResult<Vec<ModuleClaim>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT lease.resource_key, lease.worktree_key, team.task_id
                 FROM resource_leases AS lease
                 JOIN agent_runs AS run
                   ON run.project_id = lease.project_id AND run.id = lease.agent_run_id
                 JOIN team_runs AS team
                   ON team.project_id = run.project_id AND team.id = run.team_run_id
                 WHERE lease.lease_kind = 'module'
                   AND lease.released_at IS NULL
                   AND lease.expired_at IS NULL
                   AND lease.expires_at > ?1
                 ORDER BY lease.resource_key, team.task_id",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![text(now)]).map_err(backend)?;
        let mut claims = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let module: String = row.get(0).map_err(backend)?;
            let worktree: Option<String> = row.get(1).map_err(backend)?;
            let task_id: String = row.get(2).map_err(backend)?;
            claims.push(ModuleClaim {
                module: kontor_core::id::ModuleKey::parse(&module)?,
                task_id: TaskId::parse(&task_id)?,
                worktree: worktree.as_deref().map(ExternalName::parse).transpose()?,
                in_flight: true,
            });
        }
        Ok(claims)
    }

    /// Every worktree currently claimed by an unlapsed lease, across the Realm.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn active_worktree_leases(
        &self,
        now: Timestamp,
    ) -> RepositoryResult<BTreeSet<ExternalName>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT resource_key FROM resource_leases
                 WHERE lease_kind = 'worktree'
                   AND released_at IS NULL AND expired_at IS NULL AND expires_at > ?1
                 ORDER BY resource_key",
            )
            .map_err(backend)?;
        let mut rows = statement.query(params![text(now)]).map_err(backend)?;
        let mut trees = BTreeSet::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let tree: String = row.get(0).map_err(backend)?;
            trees.insert(ExternalName::parse(&tree)?);
        }
        Ok(trees)
    }

    /// Every task with a non-terminal run, across the Realm.
    ///
    /// This is what a caller assembling a [`kontor_scheduler::SchedulingSnapshot`]
    /// puts in `in_flight_tasks`: it answers both "is this task already running"
    /// and "is a serialization peer already running" from the same read, so the
    /// two cannot be built from views taken at different instants.
    ///
    /// Realm-wide for the same reason the lease reads are: a serialization peer or
    /// a module contender may live in another project, and a project-scoped read
    /// cannot see it.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn tasks_with_open_runs(&self) -> RepositoryResult<BTreeSet<TaskId>> {
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT DISTINCT team.task_id FROM agent_runs AS run
                 JOIN team_runs AS team
                   ON team.project_id = run.project_id AND team.id = run.team_run_id
                 WHERE run.lifecycle NOT IN ({TERMINAL_LIFECYCLES})
                 ORDER BY team.task_id"
            ))
            .map_err(backend)?;
        let mut rows = statement.query([]).map_err(backend)?;
        let mut tasks = BTreeSet::new();
        while let Some(row) = rows.next().map_err(backend)? {
            tasks.insert(TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?);
        }
        Ok(tasks)
    }

    /// Read one lease inside a project.
    ///
    /// # Errors
    /// Backend failures only; a lease from another project is `Ok(None)`.
    pub fn get_lease(
        &self,
        project_id: ProjectId,
        id: ResourceLeaseId,
    ) -> RepositoryResult<Option<ResourceLease>> {
        let transaction = self.begin()?;
        read_lease(&transaction, project_id, id)
    }

    /// Every lease one run still holds: neither released, expired nor lapsed.
    ///
    /// A run that ends still holds what it claimed, because a lease is given up
    /// deliberately and not by the row it belonged to changing state. Whoever
    /// ends the run has to hand them back, or the next admission of the same
    /// task waits out the expiry for no reason.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn live_leases_of_run(
        &self,
        project_id: ProjectId,
        agent_run_id: AgentRunId,
        now: Timestamp,
    ) -> RepositoryResult<Vec<ResourceLease>> {
        let transaction = self.begin()?;
        let mut statement = transaction
            .prepare(
                "SELECT id, project_id, lease_kind, resource_key, worktree_key, agent_run_id,
                        holder_instance, fencing_token, acquired_at, expires_at, released_at,
                        expired_at, renewed_from_lease_id, admission_event_id
                 FROM resource_leases
                 WHERE project_id = ?1 AND agent_run_id = ?2
                   AND released_at IS NULL AND expired_at IS NULL AND expires_at > ?3
                 ORDER BY acquired_at",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![
                project_id.to_string(),
                agent_run_id.to_string(),
                text(now)
            ])
            .map_err(backend)?;
        let mut leases = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            leases.push(read_lease_row(row)?);
        }
        Ok(leases)
    }

    /// Read one lease's append-only history, in order.
    ///
    /// # Errors
    /// Backend failures only.
    pub fn lease_history(
        &self,
        project_id: ProjectId,
        id: ResourceLeaseId,
    ) -> RepositoryResult<Vec<(u32, LeaseEventKind, u64)>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT sequence, event, fencing_token FROM lease_events
                 WHERE project_id = ?1 AND lease_id = ?2 ORDER BY sequence",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![project_id.to_string(), id.to_string()])
            .map_err(backend)?;
        let mut history = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            let sequence: i64 = row.get(0).map_err(backend)?;
            let event: String = row.get(1).map_err(backend)?;
            let token: i64 = row.get(2).map_err(backend)?;
            history.push((
                u32::try_from(sequence).unwrap_or(u32::MAX),
                LeaseEventKind::parse(&event)?,
                u64::try_from(token).unwrap_or(0),
            ));
        }
        Ok(history)
    }
}

/// Read one lease row inside a transaction the caller owns.
fn read_lease(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: ResourceLeaseId,
) -> RepositoryResult<Option<ResourceLease>> {
    let row: Option<RepositoryResult<ResourceLease>> = transaction
        .query_row(
            "SELECT id, project_id, lease_kind, resource_key, worktree_key, agent_run_id,
                    holder_instance, fencing_token, acquired_at, expires_at, released_at,
                    expired_at, renewed_from_lease_id, admission_event_id
             FROM resource_leases WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), id.to_string()],
            |row| Ok(read_lease_row(row)),
        )
        .optional()
        .map_err(backend)?;
    row.transpose()
}

fn read_lease_row(row: &rusqlite::Row<'_>) -> RepositoryResult<ResourceLease> {
    let column =
        |index: usize| -> RepositoryResult<Option<String>> { row.get(index).map_err(backend) };
    let required = |index: usize| -> RepositoryResult<String> { row.get(index).map_err(backend) };
    let kind: Option<String> = column(2)?;
    let kind = kind.ok_or(RepositoryError::Conflict {
        subject: "resource lease",
        rule: "a lease written before schema v4 declares no kind",
    })?;
    let token: Option<i64> = row.get(7).map_err(backend)?;
    let token = token.ok_or(RepositoryError::Conflict {
        subject: "resource lease",
        rule: "a lease written before schema v4 carries no fencing token",
    })?;
    let expires_at: Option<String> = column(9)?;
    let expires_at = expires_at.ok_or(RepositoryError::Conflict {
        subject: "resource lease",
        rule: "a lease written before schema v4 carries no expiry",
    })?;
    Ok(ResourceLease {
        id: ResourceLeaseId::parse(&required(0)?)?,
        project_id: ProjectId::parse(&required(1)?)?,
        kind: LeaseKind::parse(&kind)?,
        resource_key: ExternalName::parse(&required(3)?)?,
        worktree_key: column(4)?.as_deref().map(ExternalName::parse).transpose()?,
        agent_run_id: AgentRunId::parse(&required(5)?)?,
        holder_instance: column(6)?.as_deref().map(ExternalId::parse).transpose()?,
        fencing_token: u64::try_from(token).map_err(|_| RepositoryError::Backend {
            detail: "a stored fencing token is out of range".to_owned(),
        })?,
        acquired_at: read_timestamp(&required(8)?)?,
        expires_at: read_timestamp(&expires_at)?,
        released_at: column(10)?.as_deref().map(read_timestamp).transpose()?,
        expired_at: column(11)?.as_deref().map(read_timestamp).transpose()?,
        renewed_from_lease_id: column(12)?
            .as_deref()
            .map(ResourceLeaseId::parse)
            .transpose()?,
        admission_event_id: column(13)?
            .as_deref()
            .map(AdmissionEventId::parse)
            .transpose()?,
    })
}

// ---------------------------------------------------------------------------
// Admission
// ---------------------------------------------------------------------------

impl SqliteStore {
    /// Make one admission durable, or find that it already is.
    ///
    /// # Errors
    /// * [`RepositoryError::Domain`] when the request's parts do not describe one
    ///   admission of one task, or when the launch key names a different command.
    /// * [`RepositoryError::NotFound`] for a task that is not in this project.
    /// * [`RepositoryError::Conflict`] when the task's revision or state moved, a
    ///   dependency is unfinished, a serialization peer has an open run, a ceiling
    ///   is already spent, or the place is claimed by an active lease.
    /// * [`RepositoryError::Backend`] on backend failure.
    ///
    /// On any refusal nothing is written: no run, no lease, no receipt, no
    /// decision.
    pub fn admit_candidate(
        &self,
        request: &AdmissionCommit<'_>,
    ) -> RepositoryResult<AdmissionOutcome> {
        request.validate()?;
        let admitted = request.admitted;
        let project = admitted.project_id;
        let transaction = self.begin()?;

        // A replay writes nothing. The probe runs before every other read so that
        // a retried admission cannot even look like a new one.
        if let Some(outcome) = replayed_admission(&transaction, request)? {
            return Ok(outcome);
        }

        ensure_task_admissible(&transaction, project, admitted)?;
        // A task is implicitly serialized against itself, and this is the check
        // that makes "two schedulers never admit one task twice" hold in general.
        // Every other exclusion has a gap for it: the task's own module lease does
        // not contend with the task that holds it, the launch idempotency key is
        // the caller's to vary, and a task's lifecycle may still read `ready` while
        // an envelope of it is running.
        ensure_no_open_run(
            &transaction,
            project,
            admitted.task_id,
            "the task already has an open run",
        )?;
        ensure_dependencies_done(&transaction, project, admitted.task_id)?;
        ensure_no_open_serialization_peer(&transaction, project, request.serializes_with)?;
        ensure_capacity(&transaction, request)?;

        // Reclaim first, then check: a place whose lease lapsed is free, and the
        // expiry is what makes it free *to the indexes* as well as to the reader.
        //
        // The two lineages are kept apart. A pooled list would let the worktree
        // lease cite the lease the *module* was reclaimed from, which is a link to
        // a claim on a different place — the one thing reclaim lineage must not
        // say.
        let module_key = admitted
            .module
            .as_ref()
            .map(|module| ExternalName::parse(module.as_str()))
            .transpose()?;
        let module_reclaimed = match module_key.as_ref() {
            Some(module) => expire_lapsed(&transaction, module, request.decided_at)?,
            None => Vec::new(),
        };
        let worktree_reclaimed = match admitted.worktree.as_ref() {
            Some(worktree) => expire_lapsed(&transaction, worktree, request.decided_at)?,
            None => Vec::new(),
        };
        if let Some(module) = module_key.as_ref() {
            ensure_place_free(&transaction, module, admitted.worktree.as_ref())?;
        }
        if let Some(worktree) = admitted.worktree.as_ref() {
            ensure_place_free(&transaction, worktree, None)?;
        }

        insert_team_run(&transaction, &request.team_run)?;
        insert_agent_run(&transaction, &request.agent_run)?;

        // The intent goes through the one path an intent reaches storage by, which
        // also compare-and-swaps the run's desired state to `run_requested`. A
        // replay here would mean the probe above missed one, so it is a conflict
        // rather than a silent success.
        if insert_intent(&transaction, &request.launch)?.is_some() {
            return Err(conflict(
                "admission",
                "the launch command was already recorded outside this admission",
            ));
        }

        insert_admission_event(&transaction, request)?;

        if let (Some(lease_id), Some(module)) = (request.module_lease_id, module_key.as_ref()) {
            insert_lease(
                &transaction,
                request,
                lease_id,
                LeaseKind::Module,
                module,
                admitted.worktree.as_ref(),
                &module_reclaimed,
            )?;
        }
        if let (Some(lease_id), Some(worktree)) =
            (request.worktree_lease_id, admitted.worktree.as_ref())
        {
            insert_lease(
                &transaction,
                request,
                lease_id,
                LeaseKind::Worktree,
                worktree,
                None,
                &worktree_reclaimed,
            )?;
        }

        transaction.commit().map_err(backend)?;

        let receipt = self
            .get_receipt_by_key(&request.launch.idempotency_key)?
            .ok_or(RepositoryError::NotFound {
                subject: "command receipt",
            })?;
        let mut reclaimed = module_reclaimed;
        reclaimed.extend(worktree_reclaimed);
        Ok(AdmissionOutcome {
            admission_event_id: request.admission_event_id,
            receipt,
            module_lease_id: request.module_lease_id,
            worktree_lease_id: request.worktree_lease_id,
            reclaimed,
            replayed: false,
        })
    }

    /// Append refusals to the audit trail.
    ///
    /// A refusal started nothing, so it has nothing to be atomic *with* — but the
    /// whole batch of them is one transaction anyway, so a pass's record is never
    /// half written.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] for a task that is not in this project.
    /// * [`RepositoryError::Backend`] on backend failure.
    pub fn record_admission_rejections(
        &self,
        decided_at: Timestamp,
        rejections: &[RecordedRejection],
    ) -> RepositoryResult<()> {
        if rejections.is_empty() {
            return Ok(());
        }
        let transaction = self.begin()?;
        for rejection in rejections {
            transaction
                .execute(
                    "INSERT INTO scheduler_admission_events
                         (id, project_id, task_id, decision, rejection_code, evidence,
                          evidence_hash, decided_at)
                     VALUES (?1, ?2, ?3, 'rejected', ?4, ?5, ?6, ?7)",
                    params![
                        rejection.admission_event_id.to_string(),
                        rejection.project_id.to_string(),
                        rejection.task_id.to_string(),
                        rejection.code.as_str(),
                        rejection.evidence.json(),
                        rejection.evidence.hash().as_str(),
                        text(decided_at)
                    ],
                )
                .map_err(backend)?;
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Extend a lease, rotating its fencing token.
    ///
    /// # Errors
    /// * [`RepositoryError::NotFound`] for a lease that is not in this project.
    /// * [`RepositoryError::Conflict`] when the lease is no longer active, when the
    ///   presented token is not the one on the row, when the owning run has closed,
    ///   or when the new expiry does not move forward.
    pub fn renew_lease(&self, request: &LeaseRenewal) -> RepositoryResult<ResourceLease> {
        let transaction = self.begin()?;
        let lease = live_lease(
            &transaction,
            request.project_id,
            request.lease_id,
            request.presented_token,
        )?;
        if request.expires_at <= lease.expires_at {
            return Err(conflict(
                "resource lease",
                "a renewal must move the expiry forward",
            ));
        }
        // A lease exists to protect work. Renewing one whose run has closed would
        // keep a place claimed for work that no longer exists.
        ensure_run_open(&transaction, lease.project_id, lease.agent_run_id)?;

        let next = lease.fencing_token.saturating_add(1);
        // Evidence first. `resource_leases_require_lease_event` will not let the
        // row move to a token its history does not account for, so appending is
        // not a courtesy the update depends on the caller remembering.
        append_lease_event(
            &transaction,
            request.project_id,
            request.lease_id,
            LeaseEventKind::Renewed,
            next,
            None,
            request.renewed_at,
        )?;
        let changed = transaction
            .execute(
                "UPDATE resource_leases SET expires_at = ?1, fencing_token = ?2
                 WHERE project_id = ?3 AND id = ?4 AND fencing_token = ?5
                   AND released_at IS NULL AND expired_at IS NULL",
                params![
                    text(request.expires_at),
                    token_column(next)?,
                    request.project_id.to_string(),
                    request.lease_id.to_string(),
                    token_column(lease.fencing_token)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "resource lease",
                "the lease changed during the renewal",
            ));
        }
        let renewed = read_lease(&transaction, request.project_id, request.lease_id)?.ok_or(
            RepositoryError::NotFound {
                subject: "resource lease",
            },
        )?;
        transaction.commit().map_err(backend)?;
        Ok(renewed)
    }

    /// Give a lease up, with the receipt that decided it.
    ///
    /// # Errors
    /// As [`SqliteStore::renew_lease`], minus the expiry rule: a release does not
    /// move an expiry and does not rotate a token.
    pub fn release_lease(&self, request: &LeaseRelease) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        let lease = live_lease(
            &transaction,
            request.project_id,
            request.lease_id,
            request.presented_token,
        )?;
        // Evidence first, as for a renewal: the trigger holds the update to the
        // event, so the release a reader finds is always one the history records.
        append_lease_event(
            &transaction,
            request.project_id,
            request.lease_id,
            LeaseEventKind::Released,
            lease.fencing_token,
            Some(request.receipt_id),
            request.released_at,
        )?;
        let changed = transaction
            .execute(
                "UPDATE resource_leases SET released_at = ?1, release_receipt_id = ?2
                 WHERE project_id = ?3 AND id = ?4 AND fencing_token = ?5
                   AND released_at IS NULL AND expired_at IS NULL",
                params![
                    text(request.released_at),
                    request.receipt_id.to_string(),
                    request.project_id.to_string(),
                    request.lease_id.to_string(),
                    token_column(lease.fencing_token)?
                ],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "resource lease",
                "the lease changed during the release",
            ));
        }
        transaction.commit().map_err(backend)?;
        Ok(())
    }
}

/// Load a lease that is still active and whose token the caller actually holds.
///
/// This is the whole fencing rule in one place: a stale holder — one that was
/// asleep while its lease was renewed by someone else, or reclaimed after expiry —
/// presents a token that is no longer on the row, and gets a conflict instead of
/// authority over a place that has moved on.
fn live_lease(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    lease_id: ResourceLeaseId,
    presented_token: u64,
) -> RepositoryResult<ResourceLease> {
    let lease =
        read_lease(transaction, project_id, lease_id)?.ok_or(RepositoryError::NotFound {
            subject: "resource lease",
        })?;
    if !lease.is_active() {
        return Err(conflict(
            "resource lease",
            "the lease has already been released or has expired",
        ));
    }
    if lease.fencing_token != presented_token {
        return Err(conflict(
            "resource lease",
            "the presented fencing token is not the one the lease carries",
        ));
    }
    Ok(lease)
}

/// Refuse when the run a lease protects has closed.
fn ensure_run_open(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    agent_run_id: AgentRunId,
) -> RepositoryResult<()> {
    let lifecycle: Option<String> = transaction
        .query_row(
            "SELECT lifecycle FROM agent_runs WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), agent_run_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    let lifecycle = lifecycle.ok_or(RepositoryError::NotFound {
        subject: "agent run",
    })?;
    if kontor_core::state::RunLifecycle::parse(&lifecycle)?.is_terminal() {
        return Err(conflict(
            "resource lease",
            "the run the lease protects has closed",
        ));
    }
    Ok(())
}

/// Find the admission this launch key already produced, if any.
fn replayed_admission(
    transaction: &Transaction<'_>,
    request: &AdmissionCommit<'_>,
) -> RepositoryResult<Option<AdmissionOutcome>> {
    let existing: Option<(String, String)> = transaction
        .query_row(
            "SELECT id, project_id FROM command_receipts WHERE idempotency_key = ?1",
            params![request.launch.idempotency_key.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let Some((receipt_id, receipt_project)) = existing else {
        return Ok(None);
    };
    if ProjectId::parse(&receipt_project)? != request.admitted.project_id {
        return Err(RepositoryError::CrossProject {
            subject: "command receipt",
        });
    }
    let receipt_id = CommandReceiptId::parse(&receipt_id)?;

    let admission: Option<String> = transaction
        .query_row(
            "SELECT id FROM scheduler_admission_events
             WHERE project_id = ?1 AND launch_receipt_id = ?2",
            params![
                request.admitted.project_id.to_string(),
                receipt_id.to_string()
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    let Some(admission_id) = admission else {
        // The key names a command that is not an admission at all. Reusing it here
        // would attach a launch to somebody else's receipt.
        return Err(conflict(
            "admission",
            "the launch idempotency key already names a command that is not this admission",
        ));
    };

    // The key is the caller's promise that two requests are one admission. Prove
    // the promise before honouring it: a key replayed against a different task or
    // a different intent is a different decision wearing a used key.
    let receipt = read_receipt(transaction, receipt_id)?;
    receipt.ensure_replay(&request.launch.target, &request.launch.intent)?;

    let admission_event_id = AdmissionEventId::parse(&admission_id)?;
    let (module_lease_id, worktree_lease_id) =
        leases_of(transaction, request.admitted.project_id, admission_event_id)?;
    Ok(Some(AdmissionOutcome {
        admission_event_id,
        receipt,
        module_lease_id,
        worktree_lease_id,
        reclaimed: Vec::new(),
        replayed: true,
    }))
}

fn read_receipt(
    transaction: &Transaction<'_>,
    receipt_id: CommandReceiptId,
) -> RepositoryResult<CommandReceipt> {
    let receipt: Option<RepositoryResult<CommandReceipt>> = transaction
        .query_row(
            &format!(
                "SELECT {} FROM command_receipts WHERE id = ?1",
                crate::repository::RECEIPT_COLUMNS
            ),
            params![receipt_id.to_string()],
            |row| Ok(crate::commands::receipts::read_receipt_row(row)),
        )
        .optional()
        .map_err(backend)?;
    receipt.transpose()?.ok_or(RepositoryError::NotFound {
        subject: "command receipt",
    })
}

/// Prove the task is still the task that was admitted.
fn ensure_task_admissible(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    admitted: &AdmittedCandidate,
) -> RepositoryResult<()> {
    let row: Option<(String, i64)> = transaction
        .query_row(
            "SELECT state, revision FROM tasks WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), admitted.task_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    let Some((state, revision)) = row else {
        return Err(RepositoryError::NotFound { subject: "task" });
    };
    if TaskState::parse(&state)? != TaskState::Ready {
        return Err(conflict(
            "admission",
            "the task left `ready` after the snapshot was taken",
        ));
    }
    revision_of(revision)?.expect("task", admitted.revision)?;
    Ok(())
}

/// Prove every declared dependency has reached `done`.
///
/// The count is compared rather than the rows inspected one by one: a dependency
/// row whose task is missing would otherwise be silently skipped, and a missing
/// dependency is not a finished one.
fn ensure_dependencies_done(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
) -> RepositoryResult<()> {
    let unfinished: i64 = transaction
        .query_row(
            "SELECT count(*) FROM task_dependencies AS edge
             LEFT JOIN tasks AS dependency
               ON dependency.project_id = edge.project_id
              AND dependency.id = edge.depends_on_task_id
             WHERE edge.project_id = ?1 AND edge.task_id = ?2
               AND (dependency.state IS NULL OR dependency.state <> 'done')",
            params![project_id.to_string(), task_id.to_string()],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if unfinished > 0 {
        return Err(conflict("admission", "a dependency has not reached `done`"));
    }
    Ok(())
}

/// Prove no serialization peer has an open run.
///
/// This is the check that makes two scheduler instances safe against each other
/// for serialized work: the peer the pass saw as idle may have been admitted by
/// another instance a millisecond later, and only a read under the write lock can
/// see that.
fn ensure_no_open_serialization_peer(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    peers: &BTreeSet<TaskId>,
) -> RepositoryResult<()> {
    for peer in peers {
        ensure_no_open_run(
            transaction,
            project_id,
            *peer,
            "a task this work serializes against has an open run",
        )?;
    }
    Ok(())
}

/// Refuse when `task_id` has any non-terminal run.
fn ensure_no_open_run(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
    rule: &'static str,
) -> RepositoryResult<()> {
    let open: i64 = transaction
        .query_row(
            &format!(
                "SELECT count(*) FROM agent_runs AS run
                 JOIN team_runs AS team
                   ON team.project_id = run.project_id AND team.id = run.team_run_id
                 WHERE team.project_id = ?1 AND team.task_id = ?2
                   AND run.lifecycle NOT IN ({TERMINAL_LIFECYCLES})"
            ),
            params![project_id.to_string(), task_id.to_string()],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if open > 0 {
        return Err(conflict("admission", rule));
    }
    Ok(())
}

/// Recount the ceilings that can be recounted from rows.
///
/// Global, project, mission and account concurrency are all countable from
/// `agent_runs` and the graph above it, so they are proved again here rather than
/// trusted from the snapshot — which is what stops two instances from each
/// admitting the last unit of headroom.
///
/// The runtime and provider ceilings deliberately are not. A queued run has no
/// runtime binding and no provider row yet — the binding is created when the
/// launch is dispatched — so there is nothing to count. Those two are decided by
/// the pass, and the runtime's own `max_concurrent_sessions` is enforced again by
/// `kontor_runtime::preflight` before the session starts.
fn ensure_capacity(
    transaction: &Transaction<'_>,
    request: &AdmissionCommit<'_>,
) -> RepositoryResult<()> {
    let config = &request.capacity;
    let project = request.admitted.project_id;

    // A spent ceiling is not a conflict: nothing about the presented state was
    // stale, and no re-read makes room. It gets its own variant so the boundary
    // can say "come back later" instead of "you are behind".
    let refuse = |scope| Err(RepositoryError::CapacityExhausted { scope });

    if count_in_flight(transaction, InFlightScope::Global)?
        >= i64::from(config.global_max_in_flight)
    {
        return refuse("global");
    }
    if count_in_flight(transaction, InFlightScope::Project(project))?
        >= i64::from(config.project_max_in_flight)
    {
        return refuse("project");
    }
    if let Some(mission) = mission_of(transaction, project, request.admitted.task_id)?
        && count_in_flight(transaction, InFlightScope::Mission(project, mission))?
            >= i64::from(config.mission_max_in_flight)
    {
        return refuse("goal");
    }
    if let Some(account) = request.admitted.account_profile_id
        && count_in_flight(transaction, InFlightScope::Account(project, account))?
            >= i64::from(config.account_max_in_flight)
    {
        return refuse("account");
    }
    Ok(())
}

/// Which population of open runs a count covers.
enum InFlightScope {
    /// Every open run in the Realm.
    Global,
    /// Every open run in one project.
    Project(ProjectId),
    /// Every open run under one goal.
    Mission(ProjectId, MiniProjectId),
    /// Every open run pinned to one account.
    Account(ProjectId, AccountProfileId),
}

fn count_in_flight(transaction: &Transaction<'_>, scope: InFlightScope) -> RepositoryResult<i64> {
    // A run is in flight when its own lifecycle is open *and* the team it serves
    // has not closed on settled turns.
    //
    // The second half exists because a seat is persistent: a team whose declared
    // slots have all finished their bounded turns is done, and its native
    // sessions are deliberately still live. Counting those runs would let a
    // finished task hold capacity for as long as its seat sits there — and the
    // seat is meant to sit there. The run's own lifecycle stays open, because
    // nothing observed the session end; it simply stops being *work in flight*.
    let open = format!(
        "run.lifecycle NOT IN ({TERMINAL_LIFECYCLES})
         AND NOT EXISTS (
             SELECT 1 FROM team_runs AS settled
             WHERE settled.project_id = run.project_id
               AND settled.id = run.team_run_id
               AND settled.terminal_source_kind = 'settled_turns')"
    );
    let (sql, bindings): (String, Vec<String>) = match scope {
        InFlightScope::Global => (
            format!("SELECT count(*) FROM agent_runs AS run WHERE {open}"),
            Vec::new(),
        ),
        InFlightScope::Project(project) => (
            format!("SELECT count(*) FROM agent_runs AS run WHERE run.project_id = ?1 AND {open}"),
            vec![project.to_string()],
        ),
        InFlightScope::Mission(project, mission) => (
            format!(
                "SELECT count(*) FROM agent_runs AS run
                 JOIN team_runs AS team
                   ON team.project_id = run.project_id AND team.id = run.team_run_id
                 JOIN tasks AS task
                   ON task.project_id = team.project_id AND task.id = team.task_id
                 WHERE run.project_id = ?1 AND task.mini_project_id = ?2 AND {open}"
            ),
            vec![project.to_string(), mission.to_string()],
        ),
        InFlightScope::Account(project, account) => (
            format!(
                "SELECT count(*) FROM agent_runs AS run
                 WHERE run.project_id = ?1 AND run.account_profile_id = ?2 AND {open}"
            ),
            vec![project.to_string(), account.to_string()],
        ),
    };
    let parameters: Vec<&dyn rusqlite::ToSql> = bindings
        .iter()
        .map(|value| value as &dyn rusqlite::ToSql)
        .collect();
    transaction
        .query_row(&sql, parameters.as_slice(), |row| row.get(0))
        .map_err(backend)
}

fn mission_of(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    task_id: TaskId,
) -> RepositoryResult<Option<MiniProjectId>> {
    let mission: Option<Option<String>> = transaction
        .query_row(
            "SELECT mini_project_id FROM tasks WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), task_id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    mission
        .flatten()
        .as_deref()
        .map(MiniProjectId::parse)
        .transpose()
        .map_err(Into::into)
}

/// Record every lapsed claim on one place as expired, and return their ids.
///
/// Realm-wide on purpose: the lapsed lease blocking a place may belong to another
/// project, and contention is not project-scoped.
///
/// Nothing here reads or writes `agent_runs`. An expiry frees a place; it does not
/// decide anything about the run that held it.
fn expire_lapsed(
    transaction: &Transaction<'_>,
    place: &ExternalName,
    now: Timestamp,
) -> RepositoryResult<Vec<ResourceLeaseId>> {
    let lapsed: Vec<(String, String, i64)> = {
        let mut statement = transaction
            .prepare(
                "SELECT project_id, id, fencing_token FROM resource_leases
                 WHERE resource_key = ?1 AND released_at IS NULL AND expired_at IS NULL
                   AND expires_at <= ?2
                 ORDER BY id",
            )
            .map_err(backend)?;
        let mut rows = statement
            .query(params![place.as_str(), text(now)])
            .map_err(backend)?;
        let mut lapsed = Vec::new();
        while let Some(row) = rows.next().map_err(backend)? {
            lapsed.push((
                row.get(0).map_err(backend)?,
                row.get(1).map_err(backend)?,
                row.get(2).map_err(backend)?,
            ));
        }
        lapsed
    };

    let mut expired = Vec::with_capacity(lapsed.len());
    for (project, lease, token) in lapsed {
        let project = ProjectId::parse(&project)?;
        let lease_id = ResourceLeaseId::parse(&lease)?;
        // The token the lease is ending on, read from the row rather than assumed.
        // It is not defaulted on an out-of-range value: the trigger matches the
        // appended event against this exact token, so a guess would either be
        // refused or — worse, if the trigger were ever loosened — record an expiry
        // at a token no holder ever held.
        let token = u64::try_from(token).map_err(|_| RepositoryError::Backend {
            detail: "a stored fencing token is out of range".to_owned(),
        })?;
        // Evidence first: `resource_leases_require_lease_event` holds the expiry to
        // the `expired` row that accounts for it.
        append_lease_event(
            transaction,
            project,
            lease_id,
            LeaseEventKind::Expired,
            token,
            None,
            now,
        )?;
        let changed = transaction
            .execute(
                "UPDATE resource_leases SET expired_at = ?1
                 WHERE project_id = ?2 AND id = ?3 AND released_at IS NULL AND expired_at IS NULL",
                params![text(now), project.to_string(), lease_id.to_string()],
            )
            .map_err(backend)?;
        if changed != 1 {
            return Err(conflict(
                "resource lease",
                "the lapsed lease changed while it was being reclaimed",
            ));
        }
        expired.push(lease_id);
    }
    Ok(expired)
}

/// Refuse when an active lease already holds `place` against this contender.
///
/// The predicate is the one the exclusion trigger enforces: an unisolated holder
/// excludes every contender, and an isolated holder excludes an unisolated
/// contender and any contender naming the same tree. It is checked here so the
/// caller gets a typed refusal instead of a raw constraint failure; the trigger is
/// what makes it true for a caller that never came this way.
fn ensure_place_free(
    transaction: &Transaction<'_>,
    place: &ExternalName,
    worktree: Option<&ExternalName>,
) -> RepositoryResult<()> {
    let held: i64 = transaction
        .query_row(
            "SELECT count(*) FROM resource_leases
             WHERE resource_key = ?1 AND released_at IS NULL AND expired_at IS NULL
               AND (worktree_key IS NULL OR ?2 IS NULL OR worktree_key = ?2)",
            params![place.as_str(), worktree.map(ExternalName::as_str)],
            |row| row.get(0),
        )
        .map_err(backend)?;
    if held > 0 {
        return Err(conflict(
            "resource lease",
            "an active lease already claims this place",
        ));
    }
    Ok(())
}

fn insert_team_run(transaction: &Transaction<'_>, request: &NewTeamRun) -> RepositoryResult<()> {
    let document = CanonicalDocument::from_serializable(&request.snapshot)?;
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
                i64::from(request.snapshot.template_version.get()),
                document.json(),
                document.hash().as_str(),
                text(request.created_at)
            ],
        )
        .map_err(backend)?;
    Ok(())
}

fn insert_agent_run(transaction: &Transaction<'_>, request: &NewAgentRun) -> RepositoryResult<()> {
    // No binding is written. An admitted run is `queued`: it has not been
    // dispatched, so there is no native session to bind to, and inventing one
    // would make a queued run look launched.
    if request.binding.is_some() {
        return Err(DomainError::invalid(
            "AdmissionCommit",
            "an admitted run is queued and has no runtime binding yet",
        )
        .into());
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
    Ok(())
}

fn insert_admission_event(
    transaction: &Transaction<'_>,
    request: &AdmissionCommit<'_>,
) -> RepositoryResult<()> {
    let admitted = request.admitted;
    transaction
        .execute(
            "INSERT INTO scheduler_admission_events
                 (id, project_id, task_id, decision, team_run_id, agent_run_id,
                  launch_receipt_id, authorization_id, evidence, evidence_hash, decided_at)
             VALUES (?1, ?2, ?3, 'admitted', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                request.admission_event_id.to_string(),
                admitted.project_id.to_string(),
                admitted.task_id.to_string(),
                request.team_run.id.to_string(),
                request.agent_run.id.to_string(),
                request.launch.receipt_id.to_string(),
                admitted.authorization_id.to_string(),
                request.evidence.json(),
                request.evidence.hash().as_str(),
                text(request.decided_at)
            ],
        )
        .map_err(backend)?;
    Ok(())
}

/// The leases one admission acquired, by kind.
///
/// The lease names the admission and not the other way round — a cycle of foreign
/// keys has no insertion order that satisfies both — so this is how a replay
/// recovers what the original admission took.
fn leases_of(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    admission: AdmissionEventId,
) -> RepositoryResult<(Option<ResourceLeaseId>, Option<ResourceLeaseId>)> {
    let mut statement = transaction
        .prepare(
            "SELECT lease_kind, id FROM resource_leases
             WHERE project_id = ?1 AND admission_event_id = ?2 ORDER BY lease_kind",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), admission.to_string()])
        .map_err(backend)?;
    let mut module = None;
    let mut worktree = None;
    while let Some(row) = rows.next().map_err(backend)? {
        let kind: String = row.get(0).map_err(backend)?;
        let id = ResourceLeaseId::parse(&row.get::<_, String>(1).map_err(backend)?)?;
        match LeaseKind::parse(&kind)? {
            LeaseKind::Module => module = Some(id),
            LeaseKind::Worktree => worktree = Some(id),
        }
    }
    Ok((module, worktree))
}

/// The token every lease starts at.
///
/// One, not zero: a token is a positive claim about which holder is current, and
/// the schema refuses zero so that "no token" and "the first token" cannot be
/// confused.
const FIRST_FENCING_TOKEN: u64 = 1;

fn insert_lease(
    transaction: &Transaction<'_>,
    request: &AdmissionCommit<'_>,
    lease_id: ResourceLeaseId,
    kind: LeaseKind,
    place: &ExternalName,
    worktree: Option<&ExternalName>,
    reclaimed: &[ResourceLeaseId],
) -> RepositoryResult<()> {
    // Reclaim lineage: the lease this one took the place over from, when this
    // admission is the one that found the previous holder lapsed. At most one
    // lapsed lease per place can have been active, so the first is the only one.
    let reclaimed_from = reclaimed.first().map(ToString::to_string);
    transaction
        .execute(
            "INSERT INTO resource_leases
                 (id, project_id, resource_key, worktree_key, agent_run_id, acquired_at,
                  lease_kind, expires_at, fencing_token, holder_instance,
                  renewed_from_lease_id, admission_event_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                lease_id.to_string(),
                request.admitted.project_id.to_string(),
                place.as_str(),
                worktree.map(ExternalName::as_str),
                request.agent_run.id.to_string(),
                text(request.decided_at),
                kind.as_str(),
                text(request.lease_expires_at),
                token_column(FIRST_FENCING_TOKEN)?,
                request.holder_instance.as_str(),
                reclaimed_from,
                request.admission_event_id.to_string()
            ],
        )
        .map_err(backend)?;
    append_lease_event(
        transaction,
        request.admitted.project_id,
        lease_id,
        LeaseEventKind::Acquired,
        FIRST_FENCING_TOKEN,
        None,
        request.decided_at,
    )?;
    Ok(())
}

fn append_lease_event(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    lease_id: ResourceLeaseId,
    event: LeaseEventKind,
    fencing_token: u64,
    receipt_id: Option<CommandReceiptId>,
    occurred_at: Timestamp,
) -> RepositoryResult<()> {
    let next: i64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM lease_events
             WHERE project_id = ?1 AND lease_id = ?2",
            params![project_id.to_string(), lease_id.to_string()],
            |row| row.get(0),
        )
        .map_err(backend)?;
    transaction
        .execute(
            "INSERT INTO lease_events
                 (project_id, lease_id, sequence, event, fencing_token, receipt_id, occurred_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                project_id.to_string(),
                lease_id.to_string(),
                next,
                event.as_str(),
                token_column(fencing_token)?,
                receipt_id.map(|id| id.to_string()),
                text(occurred_at)
            ],
        )
        .map_err(backend)?;
    Ok(())
}

fn token_column(token: u64) -> RepositoryResult<i64> {
    i64::try_from(token).map_err(|_| RepositoryError::Backend {
        detail: "a fencing token exceeds the storable range".to_owned(),
    })
}
