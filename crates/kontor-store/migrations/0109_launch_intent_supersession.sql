-- Schema v109. A hosted TPM seat wedged by a launch that never happened.
--
-- `prepare_hosted_seat_launch_intent` refuses a second route for one occupancy
-- generation. That is right while a launch is in flight and wrong forever
-- afterwards: an intent prepared before a launch that never occurred pins the
-- seat to a route nothing can start, and every later materialization of that
-- generation refuses against it.
--
-- The repair replaces only the route and the prepared instant of such an inert
-- intent. The row keeps its identity, generation and `prepared` state, so the
-- launch that follows is the *first* launch of that occupancy rather than a
-- second one. No binding is created, retired or replaced, and nothing is
-- launched here.
--
-- Provenance and reconciliation. The semantics are re-expressed from commit
-- 8c3a60c1, which carried them in a migration numbered 0106. That number is
-- taken on the deployed line by `0106_container_native_readback`, 0107 is
-- `0107_retired_evaluator_attestations`, and 0108 was taken by
-- `0108_task_worktree_corrections` (ASMA-8120, #241) while this branch was in
-- review, so this is allocated 0109 from the current tree. Deployed history is untouched: nothing is renumbered,
-- rewritten or replayed. Only the launch-intent supersession half of that
-- source migration is re-expressed; its `core_team_route_successions` half is
-- a separate concern and `supersede_hosted_seat_launch_intent` does not read
-- it.
--
-- The immutability trigger this narrows is the one deployed at 107, whose
-- condition this carve-out reproduces verbatim before appending its exception,
-- so the rewrite preserves every refusal the deployed trigger already made.

-- replacement auditable and exactly-once. It records what was superseded as
-- well as what replaced it, so the inert route is never silently lost, and its
-- unique idempotency key is what admits exactly one supersession per attempt.
--
-- It is deliberately not a soft delete of the intent: the intent row keeps its
-- identity, generation and `prepared` state, so the launch that follows is the
-- first launch of that occupancy rather than a second one.
CREATE TABLE hosted_seat_launch_intent_supersessions (
    idempotency_key           TEXT NOT NULL PRIMARY KEY
        CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    intent_hash               TEXT NOT NULL CHECK (
        length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'
    ),
    project_id                TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    seat_binding_id           TEXT NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    occupancy_generation      INTEGER NOT NULL CHECK (occupancy_generation >= 1),
    -- The exact binding revision the compare-and-swap was taken against.
    seat_binding_revision     INTEGER NOT NULL CHECK (seat_binding_revision >= 1),
    -- What was replaced, kept verbatim. An inert route that is overwritten
    -- without being recorded cannot be audited afterwards.
    superseded_model_rung     TEXT NOT NULL CHECK (json_valid(superseded_model_rung)),
    superseded_prepared_at    TEXT NOT NULL,
    -- What replaced it.
    replacement_model_rung    TEXT NOT NULL CHECK (json_valid(replacement_model_rung)),
    receipt_id                TEXT NULL REFERENCES command_receipts(id) ON DELETE RESTRICT,
    recorded_at               TEXT NOT NULL
) STRICT;

-- One supersession per (seat, occupancy generation). A second would mean the
-- same inert intent was replaced twice, which no replay may produce and which
-- would make "exactly one launch" unprovable.
CREATE UNIQUE INDEX ux_launch_intent_supersession_occupancy
ON hosted_seat_launch_intent_supersessions (project_id, seat_binding_id, occupancy_generation);

-- Frozen on commit except the receipt binding, once, exactly as the succession
-- ledger above. `IS NOT` throughout: a NULL compared with `<>` evaluates to
-- NULL, which SQLite accepts as success.
CREATE TRIGGER launch_intent_supersession_is_frozen
BEFORE UPDATE ON hosted_seat_launch_intent_supersessions
WHEN OLD.idempotency_key IS NOT NEW.idempotency_key
  OR OLD.intent_hash IS NOT NEW.intent_hash
  OR OLD.project_id IS NOT NEW.project_id
  OR OLD.seat_binding_id IS NOT NEW.seat_binding_id
  OR OLD.occupancy_generation IS NOT NEW.occupancy_generation
  OR OLD.seat_binding_revision IS NOT NEW.seat_binding_revision
  OR OLD.superseded_model_rung IS NOT NEW.superseded_model_rung
  OR OLD.superseded_prepared_at IS NOT NEW.superseded_prepared_at
  OR OLD.replacement_model_rung IS NOT NEW.replacement_model_rung
  OR OLD.recorded_at IS NOT NEW.recorded_at
  OR OLD.receipt_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT,
        'a recorded launch-intent supersession cannot rewrite its durable evidence');
END;

CREATE TRIGGER launch_intent_supersessions_are_undeletable
BEFORE DELETE ON hosted_seat_launch_intent_supersessions
BEGIN
    SELECT RAISE(ABORT, 'a recorded launch-intent supersession cannot be deleted');
END;

-- The v103 immutability rule stands, with exactly one carve-out.
--
-- "A launch intent records what was resolved before the native call and never
-- changes it" is right while a launch is in flight: the native out there was
-- created under that decision and the record must not drift away from it. It is
-- wrong for a decision no native ever consumed. The rule as written cannot tell
-- those apart, so an intent prepared before a launch that died pins its seat
-- forever.
--
-- The carve-out is narrow and evidence-bearing. Only the route and its prepared
-- instant may move; autonomy, generation, project and seat may not. The row must
-- be `prepared` on both sides with no installed instant and no observed native,
-- so an intent any native ever answered to is still immutable. And a matching
-- supersession must already be recorded, naming this exact seat, generation,
-- superseded route and prepared instant — so the change cannot happen without
-- the durable statement of what was replaced and why.
--

-- ---------------------------------------------------------------------------
-- The carve-out
-- ---------------------------------------------------------------------------

-- The v103 immutability trigger is narrowed rather than dropped. Route and
-- prepared instant may move only while the row is prepared with no installed
-- instant and no observed native, with autonomy, generation, project and seat
-- unchanged, and only when a supersession row already records exactly what is
-- being replaced. The evidence is therefore written *before* the swap.

DROP TRIGGER hosted_seat_launch_intent_decision_immutable;

CREATE TRIGGER hosted_seat_launch_intent_decision_immutable
BEFORE UPDATE ON hosted_topology_seat_launch_intents
WHEN (OLD.autonomy <> NEW.autonomy
   OR OLD.model_rung <> NEW.model_rung
   OR OLD.occupancy_generation <> NEW.occupancy_generation
   OR OLD.project_id <> NEW.project_id
   OR OLD.seat_binding_id <> NEW.seat_binding_id
   OR OLD.prepared_at <> NEW.prepared_at
   OR OLD.state = 'installed')
  AND NOT (
      OLD.autonomy = NEW.autonomy
      AND OLD.occupancy_generation = NEW.occupancy_generation
      AND OLD.project_id = NEW.project_id
      AND OLD.seat_binding_id = NEW.seat_binding_id
      AND OLD.state = 'prepared'
      AND NEW.state = 'prepared'
      AND OLD.installed_at IS NULL
      AND NEW.installed_at IS NULL
      AND OLD.observed_native_id IS NULL
      AND NEW.observed_native_id IS NULL
      AND EXISTS (
          SELECT 1
            FROM hosted_seat_launch_intent_supersessions AS s
           WHERE s.project_id = OLD.project_id
             AND s.seat_binding_id = OLD.seat_binding_id
             AND s.occupancy_generation = OLD.occupancy_generation
             AND s.superseded_model_rung = OLD.model_rung
             AND s.superseded_prepared_at = OLD.prepared_at
             AND s.replacement_model_rung = NEW.model_rung
      )
  )
BEGIN
    SELECT RAISE(ABORT,
        'a hosted-seat launch intent records what was resolved before the native call and never changes it');
END;

PRAGMA user_version = 109;
