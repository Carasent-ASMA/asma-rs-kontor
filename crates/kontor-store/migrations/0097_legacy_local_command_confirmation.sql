-- Schema v97. Confirm successful legacy synchronous task and gate commands.
--
-- Before `complete_local_command` landed in 0a3a6e0d on 2026-08-22, a local
-- command persisted its intent and then mutated the aggregate, but a successful
-- response did not append the final `confirmed` transition. The durable Realm
-- therefore contains successful task closures and gate passes whose receipts
-- still say `intent_persisted`.
--
-- This migration does not infer success from age. It confirms only the two
-- legacy command shapes needed by ticket completion, and only when the exact
-- mutation-side row agrees with the receipt's target, revision, typed intent,
-- authority/evidence (for a gate), and sub-second occurrence time. The cutoff
-- is the commit instant that introduced the confirmation hook. Receipts at or
-- after that boundary remain governed by the normal application lifecycle.

INSERT INTO command_receipt_transitions
    (project_id, receipt_id, sequence, state, correlation, native_identity,
     evidence_ref, recorded_at)
SELECT DISTINCT receipt.project_id,
       receipt.id,
       (SELECT COALESCE(MAX(existing.sequence), 0) + 1
          FROM command_receipt_transitions AS existing
         WHERE existing.project_id = receipt.project_id
           AND existing.receipt_id = receipt.id),
       'confirmed',
       receipt.correlation,
       receipt.native_identity,
       receipt.intent_hash,
       '2026-09-16T12:00:00Z'
  FROM command_receipts AS receipt
  JOIN command_targets AS target
    ON target.project_id = receipt.project_id
   AND target.receipt_id = receipt.id
  JOIN tasks AS task
    ON task.project_id = target.project_id
   AND task.id = target.target_task_id
 WHERE receipt.kind = 'transition_task'
   AND receipt.execution_mode = 'local'
   AND receipt.state = 'intent_persisted'
   AND receipt.created_at < '2026-08-22T06:31:17Z'
   AND target.target_kind = 'task'
   AND task.state = 'done'
   AND task.imported_state IS NULL
   AND receipt.target_revision = task.revision - 1
   AND json_extract(receipt.intent, '$.operation') = 'lifecycle'
   AND json_extract(receipt.intent, '$.action') = 'complete_task'
   AND json_extract(receipt.intent, '$.task_id') = task.id
   AND json_extract(receipt.intent, '$.expected_revision') = receipt.target_revision
   AND json_type(receipt.intent, '$.evidence') = 'array'
   AND abs((julianday(receipt.created_at) - julianday(task.updated_at)) * 86400.0) < 1.0
UNION ALL
SELECT DISTINCT receipt.project_id,
       receipt.id,
       (SELECT COALESCE(MAX(existing.sequence), 0) + 1
          FROM command_receipt_transitions AS existing
         WHERE existing.project_id = receipt.project_id
           AND existing.receipt_id = receipt.id),
       'confirmed',
       receipt.correlation,
       receipt.native_identity,
       receipt.intent_hash,
       '2026-09-16T12:00:00Z'
  FROM command_receipts AS receipt
  JOIN command_targets AS target
    ON target.project_id = receipt.project_id
   AND target.receipt_id = receipt.id
  JOIN task_workflows AS workflow
    ON workflow.project_id = target.project_id
   AND workflow.task_id = target.target_task_id
  JOIN task_gate_evaluations AS evaluation
    ON evaluation.project_id = workflow.project_id
   AND evaluation.workflow_id = workflow.id
 WHERE receipt.kind = 'record_gate_verdict'
   AND receipt.execution_mode = 'local'
   AND receipt.state = 'intent_persisted'
   AND receipt.created_at < '2026-08-22T06:31:17Z'
   AND target.target_kind = 'task'
   AND json_extract(receipt.intent, '$.operation') = 'gate_record'
   AND json_extract(receipt.intent, '$.task_id') = target.target_task_id
   AND json_extract(receipt.intent, '$.gate') = evaluation.gate_key
   AND json_extract(receipt.intent, '$.verdict') = evaluation.verdict
   AND json_extract(receipt.intent, '$.evaluator_role') = evaluation.evaluator_role
   AND json_extract(receipt.intent, '$.evaluator_account') = evaluation.evaluator_account
   AND json_extract(receipt.intent, '$.evidence') = json(evaluation.evidence)
   AND abs((julianday(receipt.created_at) - julianday(evaluation.recorded_at)) * 86400.0) < 1.0;

UPDATE command_receipts AS receipt
   SET state = 'confirmed',
       result_ref = receipt.intent_hash,
       updated_at = '2026-09-16T12:00:00Z'
 WHERE receipt.state = 'intent_persisted'
   AND EXISTS (
       SELECT 1
         FROM command_receipt_transitions AS transition
        WHERE transition.project_id = receipt.project_id
          AND transition.receipt_id = receipt.id
          AND transition.state = 'confirmed'
          AND transition.recorded_at = '2026-09-16T12:00:00Z'
   );

PRAGMA user_version = 97;
