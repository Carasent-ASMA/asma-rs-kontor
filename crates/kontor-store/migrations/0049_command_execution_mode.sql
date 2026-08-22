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
--
-- `command_receipts_identity_immutable` (v47) aborts *any* update whose OLD row
-- is `confirmed` or `failed`, not merely one that moves an identity column — so
-- the backfill below cannot run while it is installed. It is dropped for the
-- statement and recreated verbatim underneath, inside the same transaction the
-- whole migration runs in, so no window exists in which a receipt is mutable.
--
-- This is not a relaxation of the invariant. What the trigger protects is a
-- *running* control plane, where a confirmed receipt is settled evidence; a
-- migration is the one place allowed to add a column to rows that already
-- exist, and the alternative -- rebuilding the table so the value arrives in an
-- INSERT -- would hand-copy a CHECK that has accumulated more than fifty
-- command kinds across v29-v49, for one backfill.
DROP TRIGGER IF EXISTS command_receipts_identity_immutable;

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

CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key
     OR OLD.target <> NEW.target
     OR OLD.intent <> NEW.intent
     OR OLD.intent_hash <> NEW.intent_hash
     OR OLD.kind <> NEW.kind
     OR OLD.project_id <> NEW.project_id
     OR OLD.state IN ('confirmed', 'failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;

PRAGMA user_version = 49;
