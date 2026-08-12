//! Durable intake: ingestion, decisions, terminal authority and lineage.
//!
//! Intake is split into two commits on purpose, and the split is the whole
//! reason a crash here is survivable.
//!
//! 1. **Ingestion** ([`ingest_source_event`]) writes the canonical event and
//!    nothing else. Its two uniqueness constraints — the source identity, and
//!    the canonical digest on the same connection — are the concurrency
//!    authority: whichever of two racing writers loses is *told* it lost, by
//!    SQLite, rather than by a decision some evaluator may not have reached yet.
//!
//! 2. **Evaluation** ([`record_intake_decision`]) writes the decision about an
//!    event that is already durable. A crash between the two leaves a stored
//!    event with no receipt, which is exactly the state
//!    [`SourceEventIngest::Unevaluated`] describes and a replay resumes from.
//!    Nothing infers "evaluated" from a column: `source_events` is immutable, so
//!    its `processing_state` records what the adapter observed and the existence
//!    of a receipt is what says a decision was reached.
//!
//! The terminal half ([`commit_intake_decision`]) is one transaction because it
//! has to be: the decision row, the goal and tasks it creates and one lineage
//! row per task either all exist or none do. Work without lineage would be work
//! nobody authorized; lineage without work would name tasks that were never
//! created. `intake_decisions`' `UNIQUE (project_id, intake_receipt_id)` and
//! `intake_created_work`'s `PRIMARY KEY (project_id, task_id)` are what make a
//! replay — or a race — attach exactly one graph.
//!
//! Nothing here launches a runtime. Intake creates work; the scheduler decides
//! whether that work may start, on exactly the same terms as every other task.

use kontor_core::DomainError;
use kontor_core::calendar::{ExecutionAuthorization, TimeRange, WorkScope};
use kontor_core::id::{
    ContentHash, ExecutionAuthorizationId, ExternalName, IntakeDecisionId, IntakeReceiptId,
    ProjectId, SourceEventId, SpecVersion, TaskId, TriggerKey,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    IntakeAuthority, IntakeCreatedWork, IntakeDecisionOutcome, IntakeDecisionRecord, IntakeOutcome,
    NewIntakeDecision, NewIntakeDecisionRecord, NewSourceEvent, RepositoryError, RepositoryResult,
    SourceEventIngest,
};
use kontor_core::spec::{
    AutoArmPolicy, AutoArmRequest, CanonicalSourceEvent, ExecutionCapability, IntakeReceipt,
    IntakeResult, SourceProcessingState, TriggerSpec,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};

use crate::SqliteStore;
use crate::repository::{
    backend, conflict, ensure_receipt_authorizes, from_json, read_budget, read_timestamp,
    read_version, stored_document, text, to_json, version_column,
};

// ---------------------------------------------------------------------------
// Ingestion
// ---------------------------------------------------------------------------

/// The decision recorded against one stored event, if it has one.
///
/// A `duplicate` receipt is skipped: it is a pointer at another decision, not a
/// decision about this event.
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

/// Read one stored source event back through its own domain types.
fn load_source_event(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: &str,
) -> RepositoryResult<Option<CanonicalSourceEvent>> {
    transaction
        .query_row(
            "SELECT id, source_kind, source_connection, external_event_id, envelope,
                    envelope_hash, external_observed_at, ingested_at, processing_state
             FROM source_events WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?
        .map(read_source_event)
        .transpose()
}

fn read_source_event(
    row: (
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
    ),
) -> RepositoryResult<CanonicalSourceEvent> {
    let (
        id,
        source_kind,
        source_connection,
        external_event_id,
        envelope,
        envelope_hash,
        observed_at,
        ingested_at,
        processing_state,
    ) = row;
    let digest = ContentHash::parse(&envelope_hash)?;
    Ok(CanonicalSourceEvent {
        id: SourceEventId::parse(&id)?,
        identity: kontor_core::spec::SourceIdentity {
            source_kind: kontor_core::id::SourceKindKey::parse(&source_kind)?,
            source_connection: kontor_core::id::SourceConnectionKey::parse(&source_connection)?,
            external_event_id: kontor_core::id::ExternalId::parse(&external_event_id)?,
        },
        envelope: kontor_core::id::CanonicalDocument::from_stored(&envelope, &digest)?,
        external_observed_at: read_timestamp(&observed_at)?,
        ingested_at: read_timestamp(&ingested_at)?,
        processing_state: SourceProcessingState::parse(&processing_state)?,
    })
}

/// Commit the canonical identity of one source event.
///
/// See [`kontor_core::repository::IntakeRepository::ingest_source_event`].
pub(crate) fn ingest_source_event(
    store: &SqliteStore,
    project_id: ProjectId,
    event: &CanonicalSourceEvent,
) -> RepositoryResult<SourceEventIngest> {
    let transaction = store.begin()?;
    let identity = &event.identity;

    // A repeat of either the source identity or the canonical payload on the
    // same connection is the same event arriving twice. The stored digest is
    // read back so a *contradiction* can be told apart from a replay.
    let existing: Option<(String, String)> = transaction
        .query_row(
            "SELECT id, envelope_hash FROM source_events
             WHERE project_id = ?1
               AND ((source_kind = ?2 AND source_connection = ?3 AND external_event_id = ?4)
                    OR (source_connection = ?3 AND envelope_hash = ?5))
             LIMIT 1",
            params![
                project_id.to_string(),
                identity.source_kind.as_str(),
                identity.source_connection.as_str(),
                identity.external_event_id.as_str(),
                event.envelope.hash().as_str()
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;

    if let Some((stored_id, stored_hash)) = existing {
        // The same source identity carrying *different* canonical bytes is not
        // a replay at all: the upstream system changed what it said under an id
        // it had already used. Returning the old decision would silently
        // discard the new content, so this is a conflict a human has to see.
        if ContentHash::parse(&stored_hash)? != *event.envelope.hash() {
            return Err(conflict(
                "source event",
                "the same source identity already exists with different canonical bytes",
            ));
        }
        if let Some(receipt) = load_receipt_for_event(&transaction, project_id, &stored_id)? {
            return Ok(SourceEventIngest::Decided(Box::new(receipt)));
        }
        let stored = load_source_event(&transaction, project_id, &stored_id)?.ok_or(
            RepositoryError::NotFound {
                subject: "source event",
            },
        )?;
        return Ok(SourceEventIngest::Unevaluated(Box::new(stored)));
    }

    transaction
        .execute(
            "INSERT INTO source_events
                 (id, project_id, source_kind, source_connection, external_event_id,
                  envelope, envelope_hash, external_observed_at, ingested_at,
                  processing_state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                event.id.to_string(),
                project_id.to_string(),
                identity.source_kind.as_str(),
                identity.source_connection.as_str(),
                identity.external_event_id.as_str(),
                event.envelope.json(),
                event.envelope.hash().as_str(),
                text(event.external_observed_at),
                text(event.ingested_at),
                event.processing_state.as_str()
            ],
        )
        .map_err(backend)?;
    transaction.commit().map_err(backend)?;
    Ok(SourceEventIngest::Recorded(Box::new(event.clone())))
}

/// Record the deterministic decision about an already-durable event.
///
/// See [`kontor_core::repository::IntakeRepository::record_intake_decision`].
pub(crate) fn record_intake_decision(
    store: &SqliteStore,
    request: &NewIntakeDecision,
) -> RepositoryResult<IntakeOutcome> {
    request.receipt.validate()?;
    // The decision must be about the event it is filed against. Without this a
    // receipt could be stored against an event it never evaluated, and every
    // later lineage check would be reading a claim rather than evidence.
    request
        .receipt
        .ensure_decides(request.source_event_id, &request.source_event_hash)?;

    let transaction = store.begin()?;
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
            "IntakeDecision",
            "the source event no longer has the cited digest",
        )
        .into());
    }

    // One pinned trigger revision decides one stored event exactly once. A
    // replay is that same decision arriving again; a *different* verdict under
    // the same revision is a contradiction, because a trigger revision is
    // deterministic.
    let existing: Option<String> = transaction
        .query_row(
            "SELECT receipt FROM intake_receipts
             WHERE project_id = ?1 AND source_event_id = ?2 AND trigger_key = ?3
               AND trigger_version = ?4",
            params![
                request.project_id.to_string(),
                request.source_event_id.to_string(),
                request.receipt.trigger.as_str(),
                version_column(request.receipt.trigger_version)
            ],
            |row| row.get(0),
        )
        .optional()
        .map_err(backend)?;
    if let Some(json) = existing {
        let stored: IntakeReceipt = from_json(&json)?;
        if !request.receipt.decides_the_same_as(&stored) {
            return Err(conflict(
                "intake decision",
                "the same trigger revision already recorded a different decision",
            ));
        }
        return Ok(IntakeOutcome::Duplicate(Box::new(stored)));
    }

    insert_receipt(&transaction, request.project_id, &request.receipt)?;
    transaction.commit().map_err(backend)?;
    Ok(IntakeOutcome::Recorded(Box::new(request.receipt.clone())))
}

pub(crate) fn insert_receipt(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    receipt: &IntakeReceipt,
) -> RepositoryResult<()> {
    transaction
        .execute(
            "INSERT INTO intake_receipts
                 (id, project_id, source_event_id, source_event_hash, trigger_key,
                  trigger_version, result, receipt, idempotency_key, dedup_key,
                  duplicate_of, predecessor_receipt_id, decided_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                receipt.id.to_string(),
                project_id.to_string(),
                receipt.source_event_id.to_string(),
                receipt.source_event_hash.as_str(),
                receipt.trigger.as_str(),
                version_column(receipt.trigger_version),
                receipt.result.as_str(),
                to_json(receipt)?,
                receipt.idempotency_key.as_str(),
                receipt.dedup_key.as_str(),
                receipt.duplicate_of.map(|id| id.to_string()),
                receipt.predecessor_receipt_id.map(|id| id.to_string()),
                text(receipt.decided_at)
            ],
        )
        .map_err(backend)?;
    Ok(())
}

/// Ingest an event and then decide it, in that order.
///
/// See [`kontor_core::repository::IntakeRepository::record_source_event`].
pub(crate) fn record_source_event(
    store: &SqliteStore,
    request: &NewSourceEvent,
) -> RepositoryResult<IntakeOutcome> {
    // Both checks are pure, and both run before the event is committed: a
    // decision that contradicts itself, or that is about some other event,
    // persists nothing at all.
    request.receipt.validate()?;
    request
        .receipt
        .ensure_decides(request.event.id, request.event.envelope.hash())?;

    let stored = match ingest_source_event(store, request.project_id, &request.event)? {
        SourceEventIngest::Decided(receipt) => return Ok(IntakeOutcome::Duplicate(receipt)),
        SourceEventIngest::Recorded(event) | SourceEventIngest::Unevaluated(event) => event,
    };
    // A resumed event is the one already in the database, which may carry a
    // different id from the one this caller minted for it. The decision is
    // filed against the stored event, never against the copy that lost the race.
    let receipt = IntakeReceipt {
        source_event_id: stored.id,
        source_event_hash: stored.envelope.hash().clone(),
        ..request.receipt.clone()
    };
    record_intake_decision(
        store,
        &NewIntakeDecision {
            project_id: request.project_id,
            source_event_id: stored.id,
            source_event_hash: stored.envelope.hash().clone(),
            receipt,
        },
    )
}

// ---------------------------------------------------------------------------
// Terminal decisions and lineage
// ---------------------------------------------------------------------------

fn load_receipt(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: IntakeReceiptId,
) -> RepositoryResult<Option<IntakeReceipt>> {
    let found: Option<String> = transaction
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

fn load_trigger(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    trigger: &TriggerKey,
    version: SpecVersion,
) -> RepositoryResult<Option<TriggerSpec>> {
    let found: Option<(String, String)> = transaction
        .query_row(
            "SELECT definition, definition_hash FROM trigger_specs
             WHERE project_id = ?1 AND trigger_key = ?2 AND version = ?3",
            params![
                project_id.to_string(),
                trigger.as_str(),
                version_column(version)
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(backend)?;
    found
        .map(|(json, hash)| stored_document::<TriggerSpec>(&json, &hash))
        .transpose()
}

/// Read one execution authorization back in full, budget included.
///
/// [`crate::query`]'s listing carries only what the scheduler refuses on; the
/// auto-arm rule compares budgets, so it needs the whole grant.
fn load_authorization(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: ExecutionAuthorizationId,
) -> RepositoryResult<Option<ExecutionAuthorization>> {
    type AuthorizationRow = (
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        i64,
        i64,
        i64,
        i64,
        i64,
        String,
        String,
        String,
        String,
    );
    let found: Option<AuthorizationRow> = transaction
        .query_row(
            "SELECT scope_kind, scope_mini_project_id, scope_task_id, allowed_start, allowed_end,
                    max_concurrency, max_tokens, max_commands, max_duration_seconds,
                    max_cost_minor_units, cost_currency, created_by, capability_receipt_id,
                    created_at
             FROM execution_authorizations WHERE project_id = ?1 AND id = ?2",
            params![project_id.to_string(), id.to_string()],
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
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some(row) = found else {
        return Ok(None);
    };
    let scope = match (row.0.as_str(), &row.1, &row.2) {
        ("project", _, _) => WorkScope::Project,
        ("mini_project", Some(goal), _) => WorkScope::MiniProject {
            mini_project_id: kontor_core::id::MiniProjectId::parse(goal)?,
        },
        ("task", _, Some(task)) => WorkScope::Task {
            task_id: TaskId::parse(task)?,
        },
        // The column is constrained to three spellings and paired with its own
        // id by a CHECK, so anything else is a row this build did not write.
        // Reading it as the widest scope would widen a grant, so it is refused.
        _ => {
            return Err(DomainError::invalid(
                "ExecutionAuthorization",
                "records a scope kind this build does not understand",
            )
            .into());
        }
    };
    Ok(Some(ExecutionAuthorization {
        id,
        project_id,
        scope,
        // Filled from the child table, which is the single source of that set.
        selected_tasks: Vec::new(),
        allowed_start: TimeRange {
            start: read_timestamp(&row.3)?,
            end: read_timestamp(&row.4)?,
        },
        max_concurrency: u32::try_from(row.5).unwrap_or(u32::MAX),
        budget: read_budget(row.6, row.7, row.8, row.9, &row.10)?,
        created_by: kontor_core::id::AccountProfileId::parse(&row.11)?,
        capability_receipt: kontor_core::id::CommandReceiptId::parse(&row.12)?,
        created_at: read_timestamp(&row.13)?,
    }))
}

/// The tasks one authorization was narrowed to, read separately so the child
/// table stays the single source of that set.
fn authorization_tasks(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    id: ExecutionAuthorizationId,
) -> RepositoryResult<Vec<TaskId>> {
    let mut statement = transaction
        .prepare(
            "SELECT task_id FROM execution_authorization_tasks
             WHERE project_id = ?1 AND authorization_id = ?2 ORDER BY task_id",
        )
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), id.to_string()])
        .map_err(backend)?;
    let mut selected = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        selected.push(TaskId::parse(&row.get::<_, String>(0).map_err(backend)?)?);
    }
    Ok(selected)
}

/// Prove a bounded auto-arm may create exactly the work it names.
///
/// The rule itself lives in [`TriggerSpec::authorize_auto_arm`] and is called
/// from here rather than restated, so a caller that goes straight to the store
/// is refused by the same bounds as one that goes through `kontor-intake`.
fn authorize_auto_arm(
    transaction: &Transaction<'_>,
    request: &NewIntakeDecisionRecord,
    receipt: &IntakeReceipt,
    caller: kontor_core::id::AccountProfileId,
) -> RepositoryResult<ExecutionCapability> {
    let trigger = load_trigger(
        transaction,
        request.project_id,
        &receipt.trigger,
        receipt.trigger_version,
    )?
    .ok_or(DomainError::MissingEvidence {
        subject: "bounded auto-arm",
        rule: "the trigger revision the proposal pinned is not stored in this project",
    })?;
    let AutoArmPolicy::BoundedAutoArm { capability, .. } = trigger.approval else {
        return Err(DomainError::MissingAuthority {
            subject: "bounded auto-arm",
            rule: kontor_core::spec::AutoArmRefusal::PolicyRequiresApproval.as_str(),
        }
        .into());
    };
    let mut authorization = load_authorization(
        transaction,
        request.project_id,
        capability.execution_authorization,
    )?
    .ok_or(DomainError::MissingEvidence {
        subject: "bounded auto-arm",
        rule: "the execution authorization the policy pins is not stored in this project",
    })?;
    authorization.selected_tasks = authorization_tasks(
        transaction,
        request.project_id,
        capability.execution_authorization,
    )?;

    let work = request.work.as_ref().ok_or(DomainError::MissingEvidence {
        subject: "bounded auto-arm",
        rule: kontor_core::spec::AutoArmRefusal::NoWorkProposed.as_str(),
    })?;
    let task_ids: Vec<TaskId> = work.tasks.iter().map(|task| task.id).collect();
    trigger
        .authorize_auto_arm(&AutoArmRequest {
            caller,
            authorization: &authorization,
            at: request.decided_at,
            mini_project_id: work.mini_project.as_ref().map(|goal| goal.id),
            task_ids: &task_ids,
        })
        .map_err(|refusal| DomainError::MissingAuthority {
            subject: "bounded auto-arm",
            rule: refusal.as_str(),
        })?;

    // The receipt this decision cites must be the very one that granted the
    // capability. Any other receipt in the project would prove that *something*
    // was authorized, which is not the same as this.
    if authorization.capability_receipt != request.authority.command_receipt() {
        return Err(DomainError::invalid(
            "IntakeDecision",
            "a bounded auto-arm cites the receipt that granted its authorization",
        )
        .into());
    }
    Ok(capability)
}

/// Commit a terminal decision and everything it creates, atomically.
///
/// See [`kontor_core::repository::IntakeRepository::commit_intake_decision`].
pub(crate) fn commit_intake_decision(
    store: &SqliteStore,
    request: &NewIntakeDecisionRecord,
) -> RepositoryResult<IntakeDecisionRecord> {
    request.validate()?;
    let transaction = store.begin()?;

    let receipt = load_receipt(&transaction, request.project_id, request.receipt_id)?.ok_or(
        RepositoryError::NotFound {
            subject: "intake receipt",
        },
    )?;
    if receipt.result != IntakeResult::Proposed {
        return Err(DomainError::invalid(
            "IntakeDecision",
            "only a proposed intake receipt can be decided",
        )
        .into());
    }

    // A proposal has exactly one terminal state. A replay of the decision that
    // produced it reads back that decision and attaches nothing; anything else
    // is a second decision about a proposal that already has one.
    if let Some(stored) = read_decision(&transaction, request.project_id, request.receipt_id)? {
        if stored.outcome == request.authority.outcome()
            && stored.command_receipt == request.authority.command_receipt()
            && stored.actor == request.authority.actor()
        {
            return Ok(stored);
        }
        return Err(conflict(
            "intake decision",
            "this proposal already has a terminal decision",
        ));
    }

    let capability = match &request.authority {
        IntakeAuthority::Approval {
            command_receipt, ..
        }
        | IntakeAuthority::Rejection {
            command_receipt, ..
        } => {
            // An approval or a rejection is a human decision about a proposal,
            // and the proposal is not an aggregate: the work it would create
            // does not exist yet. The receipt therefore targets the project the
            // proposal belongs to, and it has to be an `ApproveIntake` receipt
            // in that project — existing is not consent.
            ensure_receipt_authorizes(
                &transaction,
                "IntakeDecision",
                request.project_id,
                *command_receipt,
                CommandKind::ApproveIntake,
                AggregateRef::Project {
                    project_id: request.project_id,
                },
            )?;
            None
        }
        IntakeAuthority::BoundedAutoArm { caller, .. } => Some(authorize_auto_arm(
            &transaction,
            request,
            &receipt,
            *caller,
        )?),
    };

    let outcome = request.authority.outcome();
    let reason = match &request.authority {
        IntakeAuthority::Rejection { reason, .. } => Some(reason.clone()),
        _ => None,
    };
    transaction
        .execute(
            "INSERT INTO intake_decisions
                 (id, project_id, intake_receipt_id, outcome, actor, command_receipt_id,
                  reason, capability_granted_to, capability_execution_auth_id, decided_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                request.id.to_string(),
                request.project_id.to_string(),
                request.receipt_id.to_string(),
                outcome.as_str(),
                request.authority.actor().to_string(),
                request.authority.command_receipt().to_string(),
                reason.as_ref().map(ExternalName::as_str),
                capability.map(|value| value.granted_to.to_string()),
                capability.map(|value| value.execution_authorization.to_string()),
                text(request.decided_at)
            ],
        )
        .map_err(backend)?;

    let mut created_work = Vec::new();
    if let Some(work) = &request.work {
        if let Some(goal) = &work.mini_project {
            transaction
                .execute(
                    "INSERT INTO mini_projects (id, project_id, name, revision, created_at)
                     VALUES (?1, ?2, ?3, 1, ?4)",
                    params![
                        goal.id.to_string(),
                        goal.project_id.to_string(),
                        goal.name.as_str(),
                        text(goal.created_at)
                    ],
                )
                .map_err(backend)?;
        }
        for task in &work.tasks {
            transaction
                .execute(
                    "INSERT INTO tasks
                         (id, project_id, mini_project_id, title, module_key, state,
                          revision, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
                    params![
                        task.id.to_string(),
                        task.project_id.to_string(),
                        task.mini_project_id.map(|id| id.to_string()),
                        task.title.as_str(),
                        task.module.as_ref().map(kontor_core::id::ModuleKey::as_str),
                        task.state.as_str(),
                        text(task.created_at)
                    ],
                )
                .map_err(backend)?;
            let lineage = IntakeCreatedWork {
                project_id: request.project_id,
                receipt_id: request.receipt_id,
                decision_id: request.id,
                mini_project_id: task.mini_project_id,
                task_id: task.id,
                source_event_id: receipt.source_event_id,
                source_event_hash: receipt.source_event_hash.clone(),
                trigger: receipt.trigger.clone(),
                trigger_version: receipt.trigger_version,
                authority: outcome,
                execution_authorization: capability.map(|value| value.execution_authorization),
                created_at: request.decided_at,
            };
            insert_lineage(&transaction, &lineage)?;
            created_work.push(lineage);
        }
    }

    transaction.commit().map_err(backend)?;
    Ok(IntakeDecisionRecord {
        id: request.id,
        project_id: request.project_id,
        receipt_id: request.receipt_id,
        outcome,
        actor: request.authority.actor(),
        command_receipt: request.authority.command_receipt(),
        reason,
        capability,
        created_work,
        decided_at: request.decided_at,
    })
}

fn insert_lineage(
    transaction: &Transaction<'_>,
    lineage: &IntakeCreatedWork,
) -> RepositoryResult<()> {
    transaction
        .execute(
            "INSERT INTO intake_created_work
                 (project_id, task_id, intake_receipt_id, intake_decision_id, mini_project_id,
                  source_event_id, source_event_hash, trigger_key, trigger_version, authority,
                  execution_auth_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                lineage.project_id.to_string(),
                lineage.task_id.to_string(),
                lineage.receipt_id.to_string(),
                lineage.decision_id.to_string(),
                lineage.mini_project_id.map(|id| id.to_string()),
                lineage.source_event_id.to_string(),
                lineage.source_event_hash.as_str(),
                lineage.trigger.as_str(),
                version_column(lineage.trigger_version),
                lineage.authority.as_str(),
                lineage.execution_authorization.map(|id| id.to_string()),
                text(lineage.created_at)
            ],
        )
        .map_err(backend)?;
    Ok(())
}

const LINEAGE_COLUMNS: &str = "project_id, task_id, intake_receipt_id, intake_decision_id, \
     mini_project_id, source_event_id, source_event_hash, trigger_key, trigger_version, \
     authority, execution_auth_id, created_at";

fn read_lineage(row: &Row<'_>) -> RepositoryResult<IntakeCreatedWork> {
    Ok(IntakeCreatedWork {
        project_id: ProjectId::parse(&row.get::<_, String>(0).map_err(backend)?)?,
        task_id: TaskId::parse(&row.get::<_, String>(1).map_err(backend)?)?,
        receipt_id: IntakeReceiptId::parse(&row.get::<_, String>(2).map_err(backend)?)?,
        decision_id: IntakeDecisionId::parse(&row.get::<_, String>(3).map_err(backend)?)?,
        mini_project_id: row
            .get::<_, Option<String>>(4)
            .map_err(backend)?
            .as_deref()
            .map(kontor_core::id::MiniProjectId::parse)
            .transpose()?,
        source_event_id: SourceEventId::parse(&row.get::<_, String>(5).map_err(backend)?)?,
        source_event_hash: ContentHash::parse(&row.get::<_, String>(6).map_err(backend)?)?,
        trigger: TriggerKey::parse(&row.get::<_, String>(7).map_err(backend)?)?,
        trigger_version: read_version(row.get::<_, i64>(8).map_err(backend)?)?,
        authority: IntakeDecisionOutcome::parse(&row.get::<_, String>(9).map_err(backend)?)?,
        execution_authorization: row
            .get::<_, Option<String>>(10)
            .map_err(backend)?
            .as_deref()
            .map(ExecutionAuthorizationId::parse)
            .transpose()?,
        created_at: read_timestamp(&row.get::<_, String>(11).map_err(backend)?)?,
    })
}

fn read_decision(
    transaction: &Transaction<'_>,
    project_id: ProjectId,
    receipt_id: IntakeReceiptId,
) -> RepositoryResult<Option<IntakeDecisionRecord>> {
    /// `(id, outcome, actor, command receipt, reason, granted to, authorization,
    /// decided at)`.
    type DecisionRow = (
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    );
    let found: Option<DecisionRow> = transaction
        .query_row(
            "SELECT id, outcome, actor, command_receipt_id, reason, capability_granted_to,
                    capability_execution_auth_id, decided_at
             FROM intake_decisions WHERE project_id = ?1 AND intake_receipt_id = ?2",
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
                ))
            },
        )
        .optional()
        .map_err(backend)?;
    let Some((id, outcome, actor, command_receipt, reason, granted_to, auth_id, decided_at)) =
        found
    else {
        return Ok(None);
    };

    let mut statement = transaction
        .prepare(&format!(
            "SELECT {LINEAGE_COLUMNS} FROM intake_created_work
             WHERE project_id = ?1 AND intake_receipt_id = ?2 ORDER BY task_id"
        ))
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string(), receipt_id.to_string()])
        .map_err(backend)?;
    let mut created_work = Vec::new();
    while let Some(row) = rows.next().map_err(backend)? {
        created_work.push(read_lineage(row)?);
    }

    Ok(Some(IntakeDecisionRecord {
        id: IntakeDecisionId::parse(&id)?,
        project_id,
        receipt_id,
        outcome: IntakeDecisionOutcome::parse(&outcome)?,
        actor: kontor_core::id::AccountProfileId::parse(&actor)?,
        command_receipt: kontor_core::id::CommandReceiptId::parse(&command_receipt)?,
        reason: reason.as_deref().map(ExternalName::parse).transpose()?,
        capability: match (granted_to, auth_id) {
            (Some(granted_to), Some(auth_id)) => Some(ExecutionCapability {
                granted_to: kontor_core::id::AccountProfileId::parse(&granted_to)?,
                execution_authorization: ExecutionAuthorizationId::parse(&auth_id)?,
            }),
            _ => None,
        },
        created_work,
        decided_at: read_timestamp(&decided_at)?,
    }))
}

/// Read the terminal decision about one proposal.
///
/// See [`kontor_core::repository::IntakeRepository::get_intake_decision`].
pub(crate) fn get_intake_decision(
    store: &SqliteStore,
    project_id: ProjectId,
    receipt_id: IntakeReceiptId,
) -> RepositoryResult<Option<IntakeDecisionRecord>> {
    let transaction = store.begin()?;
    read_decision(&transaction, project_id, receipt_id)
}

/// The intake lineage of one task.
///
/// See [`kontor_core::repository::IntakeRepository::intake_lineage_of_task`].
pub(crate) fn intake_lineage_of_task(
    store: &SqliteStore,
    project_id: ProjectId,
    task_id: TaskId,
) -> RepositoryResult<Option<IntakeCreatedWork>> {
    store
        .connection
        .query_row(
            &format!(
                "SELECT {LINEAGE_COLUMNS} FROM intake_created_work
                 WHERE project_id = ?1 AND task_id = ?2"
            ),
            params![project_id.to_string(), task_id.to_string()],
            |row| Ok(read_lineage(row)),
        )
        .optional()
        .map_err(backend)?
        .transpose()
}

/// Every task in one project that intake created, with its authority.
pub(crate) fn lineage_by_task(
    store: &SqliteStore,
    project_id: ProjectId,
) -> RepositoryResult<std::collections::BTreeMap<TaskId, IntakeCreatedWork>> {
    let connection = &store.connection;
    let mut statement = connection
        .prepare(&format!(
            "SELECT {LINEAGE_COLUMNS} FROM intake_created_work WHERE project_id = ?1"
        ))
        .map_err(backend)?;
    let mut rows = statement
        .query(params![project_id.to_string()])
        .map_err(backend)?;
    let mut lineage = std::collections::BTreeMap::new();
    while let Some(row) = rows.next().map_err(backend)? {
        let record = read_lineage(row)?;
        lineage.insert(record.task_id, record);
    }
    Ok(lineage)
}
