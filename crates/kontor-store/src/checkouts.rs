//! Append-only task checkout intents, written before Git and confirmed by readback.

use kontor_core::id::{
    AggregateRevision, CanonicalDocument, ContentHash, ExternalName, ModuleKey, ProjectId, TaskId,
    Timestamp,
};
use kontor_core::repository::{RepositoryError, RepositoryResult};
use rusqlite::{Connection, OptionalExtension, params};

use crate::SqliteStore;
use crate::repository::{backend, text};

/// A source/branch/base snapshot tied to one exact durable worktree claim.
#[derive(Debug, Clone)]
pub struct TaskCheckoutPlacement {
    /// Owning project.
    pub project_id: ProjectId,
    /// Owning task.
    pub task_id: TaskId,
    /// Claim which must still be current before dispatch.
    pub worktree: ExternalName,
    /// Filesystem-canonical destination used for collision checks.
    pub destination: ExternalName,
    /// Revision witnessed before the first Git effect.
    pub task_revision: AggregateRevision,
    /// Exact durable module, absent for an explicit root-only placement.
    pub module: Option<ModuleKey>,
    /// Immutable resolved host input, common directory, branch and base commit.
    pub intent: CanonicalDocument,
    /// First successful Git readback; historical head evidence never changes.
    pub readback: Option<CanonicalDocument>,
}

fn conflict(rule: &'static str) -> RepositoryError {
    RepositoryError::Conflict {
        subject: "TaskCheckoutPlacement",
        rule,
    }
}

fn guard(connection: &Connection, pin: &TaskCheckoutPlacement) -> RepositoryResult<()> {
    let row: Option<(i64, Option<String>, String)> = connection.query_row(
        "SELECT task.revision, task.module_key, claim.worktree
         FROM tasks task JOIN task_worktrees claim ON claim.project_id = task.project_id AND claim.task_id = task.id
         WHERE task.project_id = ?1 AND task.id = ?2",
        params![pin.project_id.to_string(), pin.task_id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional().map_err(backend)?;
    let (revision, module, worktree) = row.ok_or_else(|| conflict("checkout.claim_missing"))?;
    if worktree != pin.worktree.as_str()
        || module.as_deref() != pin.module.as_ref().map(ModuleKey::as_str)
        || (pin.readback.is_none() && u64::try_from(revision).ok() != Some(pin.task_revision.get()))
    {
        return Err(conflict("checkout.stale_claim_or_intent"));
    }
    let collision: bool = connection.query_row(
        "SELECT EXISTS (SELECT 1 FROM task_worktrees WHERE (worktree = ?1 OR worktree = ?2)
          AND (project_id <> ?3 OR task_id <> ?4)) OR EXISTS (
          SELECT 1 FROM topology_node_containers binding JOIN topology_nodes node ON node.id = binding.topology_node_id
          WHERE binding.canonical_cwd = ?2 AND (binding.project_id <> ?3 OR node.task_id IS NOT ?4))",
        params![pin.worktree.as_str(), pin.destination.as_str(), pin.project_id.to_string(), pin.task_id.to_string()],
        |row| row.get(0),
    ).map_err(backend)?;
    if collision {
        return Err(conflict("checkout.destination_ownership_conflict"));
    }
    Ok(())
}

pub(crate) fn insert(
    connection: &Connection,
    pin: &TaskCheckoutPlacement,
    at: Timestamp,
) -> RepositoryResult<()> {
    let value: serde_json::Value =
        serde_json::from_str(pin.intent.json()).map_err(|_| conflict("checkout.intent_invalid"))?;
    if value["claim"].as_str() != Some(pin.worktree.as_str())
        || value["destination"].as_str() != Some(pin.destination.as_str())
        || value["module"].as_str() != pin.module.as_ref().map(ModuleKey::as_str)
    {
        return Err(conflict("checkout.intent_claim_mismatch"));
    }
    // Only a module task may acquire its first derived claim here. A root-only
    // task needs its explicit claim; an existing or historical pin is never moved.
    if let Some(module) = &pin.module {
        connection.execute(
            "INSERT INTO task_worktrees (project_id, task_id, worktree, declared_at)
             SELECT project_id, id, ?3, ?4 FROM tasks
             WHERE project_id = ?1 AND id = ?2 AND revision = ?5 AND module_key = ?6
              AND NOT EXISTS (SELECT 1 FROM task_worktrees WHERE project_id = ?1 AND task_id = ?2)
              AND NOT EXISTS (SELECT 1 FROM task_checkout_placements WHERE project_id = ?1 AND task_id = ?2)",
            params![pin.project_id.to_string(), pin.task_id.to_string(), pin.worktree.as_str(), text(at),
                i64::try_from(pin.task_revision.get()).map_err(|_| conflict("checkout.intent_invalid"))?, module.as_str()],
        ).map_err(backend)?;
    }
    guard(connection, pin)?;
    let existing: Option<String> = connection.query_row(
        "SELECT intent_hash FROM task_checkout_placements WHERE project_id = ?1 AND task_id = ?2 AND worktree = ?3",
        params![pin.project_id.to_string(), pin.task_id.to_string(), pin.worktree.as_str()], |row| row.get(0),
    ).optional().map_err(backend)?;
    if let Some(hash) = existing {
        return if hash == pin.intent.hash().as_str() {
            Ok(())
        } else {
            Err(conflict("checkout.source_pin_conflict"))
        };
    }
    if pin.readback.is_some() {
        return Err(conflict("checkout.readback_before_intent"));
    }
    connection.execute(
        "INSERT INTO task_checkout_placements
         (project_id, task_id, worktree, destination, task_revision, module_key, intent, intent_hash, created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![pin.project_id.to_string(), pin.task_id.to_string(), pin.worktree.as_str(), pin.destination.as_str(),
            i64::try_from(pin.task_revision.get()).map_err(|_| conflict("checkout.intent_invalid"))?,
            pin.module.as_ref().map(ModuleKey::as_str), pin.intent.json(), pin.intent.hash().as_str(), text(at)],
    ).map_err(backend)?;
    Ok(())
}

impl SqliteStore {
    /// Read an exact claim's frozen checkout, checking its canonical document hash.
    ///
    /// # Errors
    /// Refuses malformed or hash-mismatched stored evidence.
    pub fn task_checkout_placement(
        &self,
        project: ProjectId,
        task: TaskId,
        worktree: &ExternalName,
    ) -> RepositoryResult<Option<TaskCheckoutPlacement>> {
        type Row = (String, i64, Option<String>, String, String, Option<String>);
        let row: Option<Row> = self.connection.query_row(
            "SELECT destination, task_revision, module_key, intent, intent_hash, readback
             FROM task_checkout_placements WHERE project_id = ?1 AND task_id = ?2 AND worktree = ?3",
            params![project.to_string(), task.to_string(), worktree.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        ).optional().map_err(backend)?;
        let Some((destination, revision, module, intent, hash, readback)) = row else {
            return Ok(None);
        };
        let parse = |json: &str| -> RepositoryResult<CanonicalDocument> {
            let value =
                serde_json::from_str(json).map_err(|_| conflict("checkout.intent_invalid"))?;
            Ok(CanonicalDocument::from_value(&value)?)
        };
        let intent = parse(&intent)?;
        if intent.hash() != &ContentHash::parse(&hash)? {
            return Err(conflict("checkout.intent_hash_mismatch"));
        }
        Ok(Some(TaskCheckoutPlacement {
            project_id: project,
            task_id: task,
            worktree: worktree.clone(),
            destination: ExternalName::parse(&destination)?,
            task_revision: AggregateRevision::parse(
                u64::try_from(revision).map_err(|_| conflict("checkout.intent_invalid"))?,
            )?,
            module: module.as_deref().map(ModuleKey::parse).transpose()?,
            intent,
            readback: readback.as_deref().map(parse).transpose()?,
        }))
    }

    /// Freeze a validated placement with claim/revision/collision checks.
    /// A module task's first derived claim is committed in the same transaction.
    ///
    /// # Errors
    /// An existing pin may be replayed exactly, never replaced by a host edit.
    pub fn freeze_task_checkout(
        &self,
        pin: &TaskCheckoutPlacement,
        at: Timestamp,
    ) -> RepositoryResult<TaskCheckoutPlacement> {
        let transaction = self.begin()?;
        insert(&transaction, pin, at)?;
        transaction.commit().map_err(backend)?;
        self.task_checkout_placement(pin.project_id, pin.task_id, &pin.worktree)?
            .ok_or_else(|| conflict("checkout.intent_missing"))
    }

    /// Recheck the same durable claim immediately before its Git effect.
    ///
    /// # Errors
    /// Refuses a stale or replaced intent and conflicting task/native ownership.
    pub fn guard_task_checkout(&self, pin: &TaskCheckoutPlacement) -> RepositoryResult<()> {
        let stored = self
            .task_checkout_placement(pin.project_id, pin.task_id, &pin.worktree)?
            .ok_or_else(|| conflict("checkout.intent_missing"))?;
        if stored.intent != pin.intent {
            return Err(conflict("checkout.source_pin_conflict"));
        }
        guard(&self.connection, &stored)
    }

    /// Append the first correlated readback, retaining earlier successful evidence.
    ///
    /// # Errors
    /// Refuses changed claims, collisions and missing/mismatched placement intents.
    pub fn confirm_task_checkout(
        &self,
        pin: &TaskCheckoutPlacement,
        readback: &CanonicalDocument,
    ) -> RepositoryResult<()> {
        let transaction = self.begin()?;
        self.guard_task_checkout(pin)?;
        let value: serde_json::Value = serde_json::from_str(readback.json())
            .map_err(|_| conflict("checkout.readback_invalid"))?;
        if value["intent_hash"].as_str() != Some(pin.intent.hash().as_str()) {
            return Err(conflict("checkout.readback_intent_mismatch"));
        }
        transaction
            .execute(
                "UPDATE task_checkout_placements SET readback = ?4
             WHERE project_id = ?1 AND task_id = ?2 AND worktree = ?3 AND readback IS NULL",
                params![
                    pin.project_id.to_string(),
                    pin.task_id.to_string(),
                    pin.worktree.as_str(),
                    readback.json()
                ],
            )
            .map_err(backend)?;
        transaction.commit().map_err(backend)?;
        Ok(())
    }

    /// Claims across the realm for filesystem-canonical collision checks.
    ///
    /// # Errors
    /// Refuses unreadable or malformed durable task identities.
    pub fn checkout_claims(&self) -> RepositoryResult<Vec<(ProjectId, TaskId, ExternalName)>> {
        let mut statement = self
            .connection
            .prepare("SELECT project_id, task_id, worktree FROM task_worktrees")
            .map_err(backend)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(backend)?;
        rows.map(|row| {
            let (project, task, worktree) = row.map_err(backend)?;
            Ok((
                ProjectId::parse(&project)?,
                TaskId::parse(&task)?,
                ExternalName::parse(&worktree)?,
            ))
        })
        .collect()
    }
}
