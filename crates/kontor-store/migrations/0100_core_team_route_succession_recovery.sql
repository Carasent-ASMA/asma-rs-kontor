-- Schema v100. Make one Core Team route succession recoverable across the
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

PRAGMA user_version = 100;
