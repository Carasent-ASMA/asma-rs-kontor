-- Schema v98. Make reconstructed local-command confirmations explicit,
-- injective and distinguishable from confirmations reported by the runtime.
--
-- Schema v97 repaired pre-hook receipts by correlating them with the durable
-- mutation they must have produced. It did not retain that reconstruction as
-- typed provenance, used the intent digest as though it were a result, and did
-- not refuse two receipts that both matched one mutation. This generation
-- corrects those three properties without rewriting the append-only history:
-- it records the reconstruction separately, appends a new typed transition,
-- and marks an ambiguous v97 reconstruction confirmation-unknown.

CREATE TABLE legacy_local_command_confirmation_provenance (
    project_id          TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    receipt_id          TEXT NOT NULL,
    receipt_kind        TEXT NOT NULL CHECK (receipt_kind IN (
                                'transition_task', 'record_gate_verdict')),
    source              TEXT NOT NULL CHECK (source IN (
                                'v97_reconstruction', 'v98_reconstruction')),
    disposition         TEXT NOT NULL CHECK (disposition IN (
                                'certified', 'ambiguous')),
    mutation_identity   TEXT NOT NULL CHECK (length(mutation_identity) BETWEEN 1 AND 512),
    mutation_recorded_at TEXT NULL,
    certificate_ref     TEXT NULL CHECK (
                                certificate_ref IS NULL OR
                                length(certificate_ref) BETWEEN 1 AND 256),
    certified_at        TEXT NOT NULL CHECK (
                                certified_at GLOB
                                '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, receipt_id),
    UNIQUE (project_id, certificate_ref),
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    CHECK ((disposition = 'certified') = (certificate_ref IS NOT NULL)),
    CHECK ((disposition = 'certified') = (mutation_recorded_at IS NOT NULL))
) STRICT;

CREATE TRIGGER legacy_local_command_confirmation_provenance_no_update
BEFORE UPDATE ON legacy_local_command_confirmation_provenance
BEGIN SELECT RAISE(ABORT, 'legacy local confirmation provenance is immutable'); END;

CREATE TRIGGER legacy_local_command_confirmation_provenance_no_delete
BEFORE DELETE ON legacy_local_command_confirmation_provenance
BEGIN SELECT RAISE(ABORT, 'legacy local confirmation provenance is permanent'); END;

CREATE TEMP TABLE _legacy_local_receipts AS
SELECT receipt.project_id,
       receipt.id AS receipt_id,
       receipt.kind AS receipt_kind,
       receipt.state AS receipt_state,
       CASE WHEN EXISTS (
           SELECT 1
             FROM command_receipt_transitions AS prior
            WHERE prior.project_id = receipt.project_id
              AND prior.receipt_id = receipt.id
              AND prior.state = 'confirmed'
              AND prior.recorded_at = '2026-09-16T12:00:00Z'
              AND prior.evidence_ref = receipt.intent_hash
       ) THEN 'v97_reconstruction' ELSE 'v98_reconstruction' END AS source
  FROM command_receipts AS receipt
 WHERE receipt.kind IN ('transition_task', 'record_gate_verdict')
   AND receipt.execution_mode = 'local'
   AND (
       receipt.state = 'intent_persisted'
       OR EXISTS (
           SELECT 1
             FROM command_receipt_transitions AS prior
            WHERE prior.project_id = receipt.project_id
              AND prior.receipt_id = receipt.id
              AND prior.state = 'confirmed'
              AND prior.recorded_at = '2026-09-16T12:00:00Z'
              AND prior.evidence_ref = receipt.intent_hash
       )
   );

CREATE TEMP TABLE _legacy_local_matches AS
SELECT DISTINCT candidate.project_id,
       candidate.receipt_id,
       candidate.receipt_kind,
       candidate.receipt_state,
       candidate.source,
       'task:' || task.id || ':' || task.revision AS mutation_identity,
       task.updated_at AS mutation_recorded_at
  FROM _legacy_local_receipts AS candidate
  JOIN command_receipts AS receipt
    ON receipt.project_id = candidate.project_id
   AND receipt.id = candidate.receipt_id
  JOIN command_targets AS target
    ON target.project_id = receipt.project_id
   AND target.receipt_id = receipt.id
  JOIN tasks AS task
    ON task.project_id = target.project_id
   AND task.id = target.target_task_id
 WHERE receipt.kind = 'transition_task'
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
SELECT DISTINCT candidate.project_id,
       candidate.receipt_id,
       candidate.receipt_kind,
       candidate.receipt_state,
       candidate.source,
       'gate:' || workflow.id || ':' || evaluation.gate_key || ':' || evaluation.sequence
           AS mutation_identity,
       evaluation.recorded_at AS mutation_recorded_at
  FROM _legacy_local_receipts AS candidate
  JOIN command_receipts AS receipt
    ON receipt.project_id = candidate.project_id
   AND receipt.id = candidate.receipt_id
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
   AND target.target_kind = 'task'
   AND json_extract(receipt.intent, '$.operation') = 'gate_record'
   AND json_extract(receipt.intent, '$.task_id') = target.target_task_id
   AND json_extract(receipt.intent, '$.gate') = evaluation.gate_key
   AND json_extract(receipt.intent, '$.verdict') = evaluation.verdict
   AND json_extract(receipt.intent, '$.evaluator_role') = evaluation.evaluator_role
   AND json_extract(receipt.intent, '$.evaluator_account') = evaluation.evaluator_account
   AND json_extract(receipt.intent, '$.evidence') = json(evaluation.evidence)
   AND abs((julianday(receipt.created_at) - julianday(evaluation.recorded_at)) * 86400.0) < 1.0;

CREATE TEMP TABLE _legacy_local_resolution AS
SELECT candidate.project_id,
       candidate.receipt_id,
       candidate.receipt_kind,
       candidate.receipt_state,
       candidate.source,
       COALESCE(min(matched.mutation_identity), 'unresolved:' || candidate.receipt_id)
           AS mutation_identity,
       min(matched.mutation_recorded_at) AS mutation_recorded_at,
       count(matched.mutation_identity) AS receipt_match_count,
       CASE WHEN count(matched.mutation_identity) = 1 THEN (
           SELECT count(DISTINCT peer.receipt_id)
             FROM _legacy_local_matches AS peer
            WHERE peer.project_id = candidate.project_id
              AND peer.mutation_identity = min(matched.mutation_identity)
       ) ELSE 0 END AS mutation_receipt_count
  FROM _legacy_local_receipts AS candidate
  LEFT JOIN _legacy_local_matches AS matched
    ON matched.project_id = candidate.project_id
   AND matched.receipt_id = candidate.receipt_id
 GROUP BY candidate.project_id, candidate.receipt_id,
          candidate.receipt_kind, candidate.receipt_state, candidate.source;

INSERT INTO legacy_local_command_confirmation_provenance
    (project_id, receipt_id, receipt_kind, source, disposition,
     mutation_identity, mutation_recorded_at, certificate_ref, certified_at)
SELECT resolution.project_id,
       resolution.receipt_id,
       resolution.receipt_kind,
       resolution.source,
       CASE WHEN ((resolution.source = 'v97_reconstruction'
                       AND resolution.receipt_state = 'confirmed')
                   OR (resolution.source = 'v98_reconstruction'
                       AND resolution.receipt_state = 'intent_persisted'))
                  AND resolution.receipt_match_count = 1
                  AND resolution.mutation_receipt_count = 1
            THEN 'certified' ELSE 'ambiguous' END,
       resolution.mutation_identity,
       CASE WHEN ((resolution.source = 'v97_reconstruction'
                       AND resolution.receipt_state = 'confirmed')
                   OR (resolution.source = 'v98_reconstruction'
                       AND resolution.receipt_state = 'intent_persisted'))
                  AND resolution.receipt_match_count = 1
                  AND resolution.mutation_receipt_count = 1
            THEN resolution.mutation_recorded_at ELSE NULL END,
       CASE WHEN ((resolution.source = 'v97_reconstruction'
                       AND resolution.receipt_state = 'confirmed')
                   OR (resolution.source = 'v98_reconstruction'
                       AND resolution.receipt_state = 'intent_persisted'))
                  AND resolution.receipt_match_count = 1
                  AND resolution.mutation_receipt_count = 1
            THEN 'legacy-local-confirmation-v98:' || resolution.receipt_id ELSE NULL END,
       strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
  FROM _legacy_local_resolution AS resolution
 WHERE resolution.source = 'v97_reconstruction'
    OR resolution.receipt_match_count > 0;

INSERT INTO command_receipt_transitions
    (project_id, receipt_id, sequence, state, correlation, native_identity,
     evidence_ref, recorded_at)
SELECT provenance.project_id,
       provenance.receipt_id,
       (SELECT COALESCE(max(existing.sequence), 0) + 1
          FROM command_receipt_transitions AS existing
         WHERE existing.project_id = provenance.project_id
           AND existing.receipt_id = provenance.receipt_id),
       CASE WHEN provenance.disposition = 'certified'
            THEN 'confirmed' ELSE 'confirmation_unknown' END,
       receipt.correlation,
       receipt.native_identity,
       provenance.certificate_ref,
       provenance.certified_at
  FROM legacy_local_command_confirmation_provenance AS provenance
  JOIN command_receipts AS receipt
    ON receipt.project_id = provenance.project_id
   AND receipt.id = provenance.receipt_id
 WHERE provenance.disposition = 'certified'
    OR receipt.state IN ('intent_persisted', 'confirmed');

DROP TRIGGER command_receipts_identity_immutable;

UPDATE command_receipts AS receipt
   SET state = 'confirmed',
       result_ref = (
           SELECT provenance.certificate_ref
             FROM legacy_local_command_confirmation_provenance AS provenance
            WHERE provenance.project_id = receipt.project_id
              AND provenance.receipt_id = receipt.id
       ),
       updated_at = (
           SELECT provenance.certified_at
             FROM legacy_local_command_confirmation_provenance AS provenance
            WHERE provenance.project_id = receipt.project_id
              AND provenance.receipt_id = receipt.id
       )
 WHERE EXISTS (
       SELECT 1
         FROM legacy_local_command_confirmation_provenance AS provenance
        WHERE provenance.project_id = receipt.project_id
          AND provenance.receipt_id = receipt.id
          AND provenance.disposition = 'certified'
   );

UPDATE command_receipts AS receipt
   SET state = 'confirmation_unknown',
       result_ref = NULL,
       updated_at = (
           SELECT provenance.certified_at
             FROM legacy_local_command_confirmation_provenance AS provenance
            WHERE provenance.project_id = receipt.project_id
              AND provenance.receipt_id = receipt.id
       )
 WHERE receipt.state IN ('intent_persisted', 'confirmed')
   AND EXISTS (
       SELECT 1
         FROM legacy_local_command_confirmation_provenance AS provenance
        WHERE provenance.project_id = receipt.project_id
          AND provenance.receipt_id = receipt.id
          AND provenance.disposition = 'ambiguous'
   );

CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key OR OLD.target <> NEW.target
  OR OLD.intent <> NEW.intent OR OLD.intent_hash <> NEW.intent_hash
  OR OLD.kind <> NEW.kind OR OLD.project_id <> NEW.project_id
  OR OLD.state IN ('confirmed','failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;

DROP TABLE _legacy_local_resolution;
DROP TABLE _legacy_local_matches;
DROP TABLE _legacy_local_receipts;

PRAGMA user_version = 98;
