-- A command receipt records either work for an external dispatcher or a
-- synchronous control-plane operation. Before this column existed both were
-- written with an outbox row, so every successful application operation looked
-- like an undispatched command forever.
ALTER TABLE command_receipts
ADD COLUMN execution_mode TEXT NOT NULL DEFAULT 'dispatch'
    CHECK (execution_mode IN ('local', 'dispatch'));

-- The old application service was the only non-launch producer of zero-attempt
-- outbox entries in released builds. Reclassifying these rows is deliberately
-- conservative: anything claimed, attempted, or marked dispatched remains a
-- dispatch command and therefore remains visible to recovery.
UPDATE command_receipts
SET execution_mode = 'local'
WHERE state = 'intent_persisted'
  AND kind <> 'launch_run'
  AND EXISTS (
      SELECT 1
      FROM command_outbox AS outbox
      WHERE outbox.project_id = command_receipts.project_id
        AND outbox.receipt_id = command_receipts.id
        AND outbox.claim_token IS NULL
        AND outbox.claimed_at IS NULL
        AND outbox.dispatched_at IS NULL
        AND outbox.attempts = 0
  );

-- Operator abandonment is a local closure born confirmed and has never owned
-- an outbox entry.
UPDATE command_receipts
SET execution_mode = 'local'
WHERE kind = 'abandon_run'
  AND state = 'confirmed'
  AND NOT EXISTS (
      SELECT 1
      FROM command_outbox AS outbox
      WHERE outbox.project_id = command_receipts.project_id
        AND outbox.receipt_id = command_receipts.id
  );

PRAGMA user_version = 36;
