-- Local gate/lifecycle effects and their original public results now commit
-- with a confirmed receipt. They are never obligations for a native dispatcher.
CREATE TABLE local_command_results (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    receipt_id TEXT NOT NULL,
    payload TEXT NOT NULL CHECK (json_valid(payload)),
    payload_hash TEXT NOT NULL CHECK (length(payload_hash) = 64),
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (project_id, receipt_id),
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts(project_id, id) ON DELETE RESTRICT
) STRICT;
CREATE TRIGGER local_command_results_no_update BEFORE UPDATE ON local_command_results
BEGIN SELECT RAISE(ABORT, 'local command results are immutable'); END;
CREATE TRIGGER local_command_results_no_delete BEFORE DELETE ON local_command_results
BEGIN SELECT RAISE(ABORT, 'local command results are permanent'); END;
CREATE TRIGGER local_command_results_exact_intent BEFORE INSERT ON local_command_results
WHEN NOT EXISTS (
    SELECT 1 FROM command_receipts AS receipt
    WHERE receipt.project_id = NEW.project_id AND receipt.id = NEW.receipt_id
      AND receipt.execution_mode = 'local' AND receipt.state = 'intent_persisted'
      AND receipt.kind IN ('transition_task', 'withdraw_task', 'resume_task', 'record_gate_verdict')
      AND json_extract(NEW.payload, '$.result.intent_hash') = receipt.intent_hash
      AND json_remove(NEW.payload, '$.result') = json(receipt.intent)
)
BEGIN SELECT RAISE(ABORT, 'local command result requires its exact pending local intent'); END;

-- v49 classified only receipts that existed at migration time. The atomic
-- writers introduced later still used NewCommandIntent, leaving successful
-- local changes falsely marked dispatch/intent_persisted. v97/v98 deliberately
-- considered only LOCAL receipts. Preserve that history and record this repair
-- as a separate authority class, never as an observed native result.
CREATE TABLE legacy_dispatch_local_confirmation_provenance (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    receipt_id TEXT NOT NULL,
    receipt_kind TEXT NOT NULL CHECK (receipt_kind IN ('transition_task', 'record_gate_verdict')),
    source TEXT NOT NULL CHECK (source = 'v115_atomic_local_reconstruction'),
    mutation_identity TEXT NOT NULL CHECK (length(mutation_identity) BETWEEN 1 AND 512),
    mutation_recorded_at TEXT NOT NULL,
    certificate_ref TEXT NOT NULL CHECK (length(certificate_ref) BETWEEN 1 AND 256),
    certified_at TEXT NOT NULL,
    PRIMARY KEY (project_id, receipt_id),
    UNIQUE (project_id, mutation_identity),
    UNIQUE (project_id, certificate_ref),
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts(project_id, id) ON DELETE RESTRICT
) STRICT;
CREATE TRIGGER legacy_dispatch_local_confirmation_no_update
BEFORE UPDATE ON legacy_dispatch_local_confirmation_provenance
BEGIN SELECT RAISE(ABORT, 'legacy dispatch local confirmation is immutable'); END;
CREATE TRIGGER legacy_dispatch_local_confirmation_no_delete
BEFORE DELETE ON legacy_dispatch_local_confirmation_provenance
BEGIN SELECT RAISE(ABORT, 'legacy dispatch local confirmation is permanent'); END;

-- Match the full receipt population, not merely repair-eligible rows: an
-- already confirmed, claimed or ambiguous peer must still prevent two commands
-- from claiming the same mutation. Result envelopes, when present, must agree.
CREATE TEMP TABLE _v115_local_matches AS
SELECT DISTINCT receipt.project_id, receipt.id AS receipt_id, receipt.kind AS receipt_kind,
       'task:' || task.id || ':' || task.revision AS mutation_identity,
       task.updated_at AS mutation_recorded_at
FROM command_receipts AS receipt
JOIN command_targets AS target ON target.project_id = receipt.project_id AND target.receipt_id = receipt.id
JOIN tasks AS task ON task.project_id = target.project_id AND task.id = target.target_task_id
LEFT JOIN command_outbox AS outbox ON outbox.project_id = receipt.project_id AND outbox.receipt_id = receipt.id
WHERE receipt.kind = 'transition_task' AND target.target_kind = 'task'
  AND task.state = 'done' AND task.imported_state IS NULL
  AND receipt.target_revision = task.revision - 1
  AND json_extract(receipt.intent, '$.operation') = 'lifecycle'
  AND json_extract(receipt.intent, '$.action') = 'complete_task'
  AND json_extract(receipt.intent, '$.task_id') = task.id
  AND json_extract(receipt.intent, '$.expected_revision') = receipt.target_revision
  AND json_type(receipt.intent, '$.evidence') = 'array'
  AND abs((julianday(receipt.created_at) - julianday(task.updated_at)) * 86400.0) < 1.0
  AND (json_type(outbox.payload, '$.result') IS NULL OR (
      json_extract(outbox.payload, '$.result.intent_hash') = receipt.intent_hash
      AND json_extract(outbox.payload, '$.result.task_id') = task.id
      AND json_extract(outbox.payload, '$.result.state') = 'done'
      AND json_extract(outbox.payload, '$.result.resulting_revision') = task.revision
  ))
UNION ALL
SELECT DISTINCT receipt.project_id, receipt.id, receipt.kind,
       'gate:' || workflow.id || ':' || evaluation.gate_key || ':' || evaluation.sequence,
       evaluation.recorded_at
FROM command_receipts AS receipt
JOIN command_targets AS target ON target.project_id = receipt.project_id AND target.receipt_id = receipt.id
JOIN task_workflows AS workflow ON workflow.project_id = target.project_id AND workflow.task_id = target.target_task_id
JOIN task_gate_evaluations AS evaluation ON evaluation.project_id = workflow.project_id AND evaluation.workflow_id = workflow.id
LEFT JOIN command_outbox AS outbox ON outbox.project_id = receipt.project_id AND outbox.receipt_id = receipt.id
WHERE receipt.kind = 'record_gate_verdict' AND target.target_kind = 'task'
  AND evaluation.verdict = 'passed'
  AND json_extract(receipt.intent, '$.operation') = 'gate_record'
  AND json_extract(receipt.intent, '$.task_id') = target.target_task_id
  AND json_extract(receipt.intent, '$.gate') = evaluation.gate_key
  AND json_extract(receipt.intent, '$.verdict') = evaluation.verdict
  AND json_extract(receipt.intent, '$.evaluator_role') = evaluation.evaluator_role
  AND json_extract(receipt.intent, '$.evaluator_account') = evaluation.evaluator_account
  AND json_extract(receipt.intent, '$.evidence') = json(evaluation.evidence)
  AND abs((julianday(receipt.created_at) - julianday(evaluation.recorded_at)) * 86400.0) < 1.0
  AND (json_type(outbox.payload, '$.result') IS NULL OR (
      json_extract(outbox.payload, '$.result.intent_hash') = receipt.intent_hash
      AND json_extract(outbox.payload, '$.result.workflow_id') = workflow.id
      AND json_extract(outbox.payload, '$.result.gate_sequence') = evaluation.sequence
  ));

INSERT INTO legacy_dispatch_local_confirmation_provenance
    (project_id, receipt_id, receipt_kind, source, mutation_identity,
     mutation_recorded_at, certificate_ref, certified_at)
SELECT matched.project_id, matched.receipt_id, matched.receipt_kind,
       'v115_atomic_local_reconstruction', matched.mutation_identity,
       matched.mutation_recorded_at, 'legacy-local-confirmation-v115:' || matched.receipt_id,
       strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
FROM _v115_local_matches AS matched
JOIN command_receipts AS receipt ON receipt.project_id = matched.project_id AND receipt.id = matched.receipt_id
JOIN command_outbox AS outbox ON outbox.project_id = receipt.project_id AND outbox.receipt_id = receipt.id
WHERE receipt.execution_mode = 'dispatch' AND receipt.state = 'intent_persisted'
  AND receipt.attempts = 0 AND receipt.correlation IS NULL AND receipt.native_identity IS NULL
  AND receipt.result_ref IS NULL
  AND outbox.claim_token IS NULL AND outbox.claimed_at IS NULL
  AND outbox.dispatched_at IS NULL AND outbox.attempts = 0
  AND json_remove(outbox.payload, '$.result') = json(receipt.intent)
  AND EXISTS (SELECT 1 FROM command_receipt_transitions AS initial
              WHERE initial.project_id = receipt.project_id AND initial.receipt_id = receipt.id
                AND initial.sequence = 1 AND initial.state = 'intent_persisted'
                AND initial.correlation IS NULL AND initial.native_identity IS NULL
                AND initial.evidence_ref IS NULL)
  AND NOT EXISTS (SELECT 1 FROM command_receipt_transitions AS later
                  WHERE later.project_id = receipt.project_id AND later.receipt_id = receipt.id
                    AND later.sequence <> 1)
  AND (SELECT count(*) FROM _v115_local_matches AS peer
       WHERE peer.project_id = matched.project_id AND peer.receipt_id = matched.receipt_id) = 1
  AND (SELECT count(*) FROM _v115_local_matches AS peer
       WHERE peer.project_id = matched.project_id AND peer.mutation_identity = matched.mutation_identity) = 1
  AND NOT EXISTS (SELECT 1 FROM legacy_local_command_confirmation_provenance AS prior
                  WHERE prior.project_id = matched.project_id
                    AND prior.mutation_identity = matched.mutation_identity);

INSERT INTO command_receipt_transitions
    (project_id, receipt_id, sequence, state, correlation, native_identity, evidence_ref, recorded_at)
SELECT project_id, receipt_id, 2, 'confirmed', NULL, NULL, certificate_ref, certified_at
FROM legacy_dispatch_local_confirmation_provenance;

UPDATE command_receipts AS receipt
SET execution_mode = 'local', state = 'confirmed',
    result_ref = (SELECT certificate_ref FROM legacy_dispatch_local_confirmation_provenance AS p
                  WHERE p.project_id = receipt.project_id AND p.receipt_id = receipt.id),
    updated_at = (SELECT certified_at FROM legacy_dispatch_local_confirmation_provenance AS p
                  WHERE p.project_id = receipt.project_id AND p.receipt_id = receipt.id)
WHERE EXISTS (SELECT 1 FROM legacy_dispatch_local_confirmation_provenance AS p
              WHERE p.project_id = receipt.project_id AND p.receipt_id = receipt.id);

DROP TABLE _v115_local_matches;
PRAGMA user_version = 115;
