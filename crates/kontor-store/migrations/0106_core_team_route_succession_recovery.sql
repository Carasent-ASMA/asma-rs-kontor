-- Schema v106. Make one Core Team route succession recoverable across the
-- interval between its committed store transition and its command receipt.
--
-- The succession retires the predecessor, launches the successor, replaces the
-- active occupancy row and only then records the receipt its idempotency key is
-- found by. A process loss inside that interval left durable generation-2 state
-- with no receipt, and replay had nothing to find: it re-planned against the
-- moved SeatBinding and the spent preview, so it refused its own recorded
-- effect instead of converging on it (ASMA-8187 F-8187-V1).
--
-- This table is that missing evidence. It is written *inside the same
-- transaction* as the history append and the active-row replacement, so the
-- transition and the means to reconstruct its receipt commit together or not at
-- all. There is no window left in which the seat has moved and this row does
-- not exist.
--
-- It is not a second receipt. `command_receipts` remains the idempotency
-- authority; this row is the durable outcome the receipt is reconstructed
-- *from* when the key finds no receipt, and the durable readback that an exact
-- replay answers with once one exists.
CREATE TABLE core_team_route_successions (
    -- The apply key, which is what a replay arrives holding. Unique because one
    -- key names one succession: a second row under the same key would be the
    -- duplicate effect this table exists to make impossible.
    idempotency_key                 TEXT NOT NULL PRIMARY KEY
        CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    -- The exact pre-effect intent this succession was admitted under. A replay
    -- that derives a different intent from its request is a different command
    -- wearing a used key, and is refused rather than answered from this row.
    intent_hash                     TEXT NOT NULL CHECK (
        length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'
    ),
    project_id                      TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id                 TEXT NOT NULL REFERENCES mini_projects(id) ON DELETE RESTRICT,
    -- The logical seat this succession preserved. Deliberately not a foreign key
    -- to `hosted_topology_seats`: that row is the *current* occupant and moves
    -- under later successions, while this evidence must outlive every one of
    -- them.
    seat_binding_id                 TEXT NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    predecessor_native_id           TEXT NOT NULL CHECK (length(predecessor_native_id) BETWEEN 1 AND 256),
    predecessor_generation          INTEGER NOT NULL CHECK (predecessor_generation >= 0),
    successor_native_id             TEXT NOT NULL CHECK (length(successor_native_id) BETWEEN 1 AND 256),
    successor_generation            INTEGER NOT NULL CHECK (successor_generation >= 0),
    -- Both occupancy generations, so a replay can answer "which generation did
    -- *this* command produce" without recounting a history that later
    -- successions have since extended.
    predecessor_occupancy_generation INTEGER NOT NULL CHECK (predecessor_occupancy_generation >= 1),
    successor_occupancy_generation  INTEGER NOT NULL CHECK (successor_occupancy_generation >= 1),
    -- The complete final readback this command produced: both native
    -- identities and provider sessions, both occupancy generations, the exact
    -- ECP placement and canonical cwd, and the unchanged route and pins.
    -- Persisted rather than recomputed because recomputation answers with the
    -- seat's *current* state, which is precisely the wrong answer once the seat
    -- has moved on (F-8187-V3).
    -- The exact native project the ECP container hung under when the
    -- succession committed — Paseo's own `prj_*`, not this realm's project
    -- UUID. Held as its own column so a replay can prove the persisted
    -- readback names the parent the transition actually ran in, rather than
    -- re-asserting whatever the readback happens to say. NULL only for a
    -- native root, which has no parent project.
    native_parent_project_id        TEXT NULL CHECK (
        native_parent_project_id IS NULL
        OR length(native_parent_project_id) BETWEEN 1 AND 256
    ),
    readback                        TEXT NOT NULL CHECK (json_valid(readback)),
    readback_hash                   TEXT NOT NULL CHECK (
        length(readback_hash) = 64 AND readback_hash NOT GLOB '*[^0-9a-f]*'
    ),
    -- NULL exactly for the interval this table exists to survive: the store
    -- transition has committed and the receipt has not been recorded yet. The
    -- resume path fills it in, once.
    receipt_id                      TEXT NULL REFERENCES command_receipts(id) ON DELETE RESTRICT,
    recorded_at                     TEXT NOT NULL,
    receipted_at                    TEXT NULL,
    CHECK ((receipt_id IS NULL AND receipted_at IS NULL)
           OR (receipt_id IS NOT NULL AND receipted_at IS NOT NULL))
) STRICT;

-- One succession per (seat, predecessor occupancy). Two rows naming the same
-- seat and the same generation would mean one occupancy was retired twice,
-- which the append-only history cannot represent and no replay could
-- disambiguate.
CREATE UNIQUE INDEX ux_core_team_route_succession_occupancy
ON core_team_route_successions (project_id, seat_binding_id, predecessor_occupancy_generation);

-- A succession advances exactly one generation. Recording anything else would
-- make the pair unusable as the "which occupancy did this command produce"
-- answer the replay path reads it for.
CREATE TRIGGER core_team_route_succession_advances_one_generation
BEFORE INSERT ON core_team_route_successions
WHEN NEW.successor_occupancy_generation <> NEW.predecessor_occupancy_generation + 1
BEGIN
    SELECT RAISE(ABORT,
        'a Core Team route succession advances exactly one occupancy generation');
END;

-- The evidence is frozen the moment it commits. Only the receipt binding may
-- move, only from absent to present, and only once: everything else is the
-- durable answer a replay is entitled to receive unchanged however many
-- successions have happened since.
--
-- `IS NOT` throughout rather than `<>`, because SQLite treats a CHECK or WHEN
-- that evaluates to NULL as success — a nullable column compared with `<>`
-- silently admits the rows the rule looks like it forbids.
CREATE TRIGGER core_team_route_succession_evidence_is_frozen
BEFORE UPDATE ON core_team_route_successions
WHEN OLD.idempotency_key IS NOT NEW.idempotency_key
  OR OLD.intent_hash IS NOT NEW.intent_hash
  OR OLD.project_id IS NOT NEW.project_id
  OR OLD.mini_project_id IS NOT NEW.mini_project_id
  OR OLD.seat_binding_id IS NOT NEW.seat_binding_id
  OR OLD.predecessor_native_id IS NOT NEW.predecessor_native_id
  OR OLD.predecessor_generation IS NOT NEW.predecessor_generation
  OR OLD.successor_native_id IS NOT NEW.successor_native_id
  OR OLD.successor_generation IS NOT NEW.successor_generation
  OR OLD.predecessor_occupancy_generation IS NOT NEW.predecessor_occupancy_generation
  OR OLD.successor_occupancy_generation IS NOT NEW.successor_occupancy_generation
  OR OLD.native_parent_project_id IS NOT NEW.native_parent_project_id
  OR OLD.readback IS NOT NEW.readback
  OR OLD.readback_hash IS NOT NEW.readback_hash
  OR OLD.recorded_at IS NOT NEW.recorded_at
  OR OLD.receipt_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT,
        'a recorded Core Team route succession cannot rewrite its durable evidence');
END;

-- Deleting one would reopen exactly the window this table closes: the seat
-- would hold generation-2 state with neither a receipt nor a reconstruction
-- path, and the next replay would re-plan and refuse its own effect. Receipts
-- have been undeletable by trigger since `0001_init` for the same reason.
CREATE TRIGGER core_team_route_successions_are_undeletable
BEFORE DELETE ON core_team_route_successions
BEGIN
    SELECT RAISE(ABORT, 'a recorded Core Team route succession cannot be deleted');
END;

-- ASMA-7869. Certify the supersession of a never-bound prepared launch intent.
--
-- `prepare_hosted_seat_launch_intent` refuses a second route for one occupancy
-- generation, which is right: an occupancy is launched under one authority or
-- none. But an intent prepared before an effect that never happened is inert,
-- and the refusal then outlives the thing it was protecting — the seat holds a
-- route nobody can launch and nobody can change, and every later materialization
-- of that generation refuses forever.
--
-- The repair replaces only such an intent, and this row is what makes that
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
-- The rest of the trigger is the exact v103 body, reproduced unchanged.
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

PRAGMA user_version = 106;
