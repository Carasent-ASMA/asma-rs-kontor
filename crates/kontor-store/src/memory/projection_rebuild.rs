//! Append-only rebuild request/result receipts; activation and result are atomic.
use super::*;
use kontor_core::id::IdempotencyKey;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionRebuildInput {
    pub expected_generation: u64,
    pub expected_memory_cursor: i64,
    pub preview_digest: ContentHash,
}

#[derive(Debug)]
pub enum ProjectionRebuildStage {
    Staged(ProjectionPreview),
    Replay(ProjectionReadback),
}

impl ProjectionRebuildInput {
    fn canonical(&self, project: ProjectId) -> Result<CanonicalDocument, MemoryError> {
        Ok(CanonicalDocument::from_value(&serde_json::json!({
            "schema_version": 1, "operation": "rebuild_memory_projection", "project_id": project, "request": self
        }))?)
    }
}

/// Check the immutable request even while qualification is pending. A completed
/// result is hash-verified and replayed before consulting current projection state.
fn replay_in(
    connection: &rusqlite::Connection,
    project: ProjectId,
    key: &IdempotencyKey,
    input: &ProjectionRebuildInput,
) -> Result<(bool, Option<ProjectionReadback>), MemoryError> {
    let row = connection.query_row(
        "SELECT request,request_hash FROM memory_projection_rebuild_keys WHERE idempotency_key=?1",
        [key.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ).optional()?;
    let Some((json, hash)) = row else {
        return Ok((false, None));
    };
    let original = CanonicalDocument::from_stored(&json, &ContentHash::parse(&hash)?)?;
    if original != input.canonical(project)? {
        return Err(MemoryError::Refused(MemoryRefusal::BindingConflict));
    }
    let result = connection.query_row(
        "SELECT result,result_hash FROM memory_projection_rebuild_results WHERE idempotency_key=?1",
        [key.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    ).optional()?;
    let parsed = result
        .map(|(json, hash)| {
            let document = CanonicalDocument::from_stored(&json, &ContentHash::parse(&hash)?)?;
            let envelope: serde_json::Value = serde_json::from_str(document.json())?;
            Ok::<_, MemoryError>(serde_json::from_value(envelope["projection"].clone())?)
        })
        .transpose()?;
    Ok((true, parsed))
}

impl SqliteStore {
    pub fn replay_projection_rebuild(
        &self,
        project: ProjectId,
        key: &IdempotencyKey,
        input: &ProjectionRebuildInput,
    ) -> Result<Option<ProjectionReadback>, MemoryError> {
        let tx = self.connection.unchecked_transaction()?;
        Ok(replay_in(&tx, project, key, input)?.1)
    }

    /// Stage the exact current snapshot and bind optimistic inputs in one local
    /// transaction. No network await is allowed inside this store operation.
    pub fn stage_projection_rebuild(
        &self,
        project: ProjectId,
        key: &IdempotencyKey,
        input: &ProjectionRebuildInput,
    ) -> Result<ProjectionRebuildStage, MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let (bound, replay) = replay_in(&tx, project, key, input)?;
        if let Some(original) = replay {
            return Ok(ProjectionRebuildStage::Replay(original));
        }
        let preview = stage_projection_in(&tx, project)?;
        if preview.active_generation != input.expected_generation
            || preview.snapshot.memory_cursor != input.expected_memory_cursor
            || preview.snapshot.digest != input.preview_digest
        {
            return Err(MemoryError::Refused(MemoryRefusal::ProjectionConflict));
        }
        if !bound {
            let request = input.canonical(project)?;
            tx.execute(
                "INSERT INTO memory_projection_rebuild_keys(idempotency_key,project_id,request,request_hash,recorded_at) VALUES (?1,?2,?3,?4,?5)",
                params![key.as_str(), project.to_string(), request.json(), request.hash().as_str(), Timestamp::now().to_string()],
            )?;
        }
        tx.commit()?;
        Ok(ProjectionRebuildStage::Staged(preview))
    }

    /// Late CAS, canary guard, pointer and immutable original response commit
    /// together. Concurrent same-key completion replays the winner; distinct-key
    /// contenders must still satisfy the original generation/cursor/digest.
    pub fn activate_projection_rebuild(
        &self,
        project: ProjectId,
        key: &IdempotencyKey,
        input: &ProjectionRebuildInput,
        qualification: &ProjectionQualification,
    ) -> Result<ProjectionReadback, MemoryError> {
        let tx = Transaction::new_unchecked(&self.connection, TransactionBehavior::Immediate)?;
        let (bound, replay) = replay_in(&tx, project, key, input)?;
        if let Some(original) = replay {
            return Ok(original);
        }
        if !bound {
            return Err(MemoryError::Refused(MemoryRefusal::BindingConflict));
        }
        let result = activate_projection_in(
            &tx,
            project,
            input.expected_memory_cursor,
            input.expected_generation,
            qualification,
        )?;
        let document = CanonicalDocument::from_value(
            &serde_json::json!({"schema_version":1,"projection":result}),
        )?;
        tx.execute(
            "INSERT INTO memory_projection_rebuild_results(idempotency_key,result,result_hash,recorded_at) VALUES (?1,?2,?3,?4)",
            params![key.as_str(), document.json(), document.hash().as_str(), Timestamp::now().to_string()],
        )?;
        tx.commit()?;
        Ok(result)
    }
}
