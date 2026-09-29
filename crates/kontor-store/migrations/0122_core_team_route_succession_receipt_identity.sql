-- Schema v122. One receipt, one succession, and a binding instant that is as
-- frozen as the binding itself.
--
-- `0120` gave `receipt_id` a foreign key, which proves the receipt exists. It
-- does not prove the receipt is *this* succession's, and it does not stop two
-- successions naming the same one. A receipt is the statement that one command
-- finished; two rows pointing at it would make it the statement that two did,
-- and nothing in the schema said otherwise.
--
-- The trigger gap is narrower and the same shape. `receipt_id` was frozen once
-- bound; `receipted_at` was not, so the instant a succession says it completed
-- could be rewritten afterwards while the receipt it names stayed put
-- (ASMA-8187 P2).

-- One receipt binds at most one succession. Partial, because an unbound row is
-- not a claim on any receipt and any number of them may be pending at once.
CREATE UNIQUE INDEX ux_core_team_route_succession_receipt
ON core_team_route_successions (receipt_id)
WHERE receipt_id IS NOT NULL;

-- Recreated rather than altered: SQLite has no `ALTER TRIGGER`, and the rule
-- this adds belongs beside the ones it already carries rather than in a second
-- trigger that a reader could miss.
DROP TRIGGER core_team_route_succession_claim_is_frozen;

CREATE TRIGGER core_team_route_succession_claim_is_frozen
BEFORE UPDATE ON core_team_route_successions
WHEN OLD.idempotency_key IS NOT NEW.idempotency_key
  OR OLD.intent_hash IS NOT NEW.intent_hash
  OR OLD.project_id IS NOT NEW.project_id
  OR OLD.mini_project_id IS NOT NEW.mini_project_id
  OR OLD.seat_binding_id IS NOT NEW.seat_binding_id
  OR OLD.predecessor_native_id IS NOT NEW.predecessor_native_id
  OR OLD.predecessor_generation IS NOT NEW.predecessor_generation
  OR OLD.predecessor_occupancy_generation IS NOT NEW.predecessor_occupancy_generation
  OR OLD.successor_occupancy_generation IS NOT NEW.successor_occupancy_generation
  OR OLD.successor_credential_generation IS NOT NEW.successor_credential_generation
  OR OLD.claimed_at IS NOT NEW.claimed_at
  -- A committed transition is immutable evidence.
  OR (OLD.route_committed_at IS NOT NULL AND (
         OLD.successor_native_id IS NOT NEW.successor_native_id
      OR OLD.successor_generation IS NOT NEW.successor_generation
      OR OLD.readback IS NOT NEW.readback
      OR OLD.readback_hash IS NOT NEW.readback_hash
      OR OLD.route_committed_at IS NOT NEW.route_committed_at))
  -- The receipt binds once and never moves, and neither does the instant it
  -- bound at: a completion time that can be rewritten is not evidence of when
  -- anything completed.
  OR (OLD.receipt_id IS NOT NULL AND OLD.receipt_id IS NOT NEW.receipt_id)
  OR (OLD.receipted_at IS NOT NULL AND OLD.receipted_at IS NOT NEW.receipted_at)
  -- Pending effects latch forward only.
  OR (OLD.launch_intent_installed = 1 AND NEW.launch_intent_installed = 0)
  OR (OLD.seat_binding_observed = 1 AND NEW.seat_binding_observed = 0)
BEGIN
    SELECT RAISE(ABORT,
        'a Core Team route succession cannot rewrite its claim or its committed evidence');
END;

PRAGMA user_version = 122;
