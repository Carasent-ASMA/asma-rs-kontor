-- Schema v120. One Core Team route succession, owned exclusively and
-- recoverable across every interval its effects span.
--
-- The route correction retires a predecessor, launches a successor, replaces
-- the active occupancy, installs the launch intent, observes the SeatBinding
-- and only then records its command receipt. Three separate problems live in
-- that sequence, and this one table answers all three because they are the same
-- fact recorded at different moments.
--
-- **Exclusivity.** The launch is the duplicable effect: two callers arriving
-- with two fresh idempotency keys would each retire and each launch, and the
-- seat would end with two natives claiming one logical owner. A read lock over
-- native activity does not prove exclusivity — it serialises, then both
-- proceed — and a compare-and-swap after the launch is too late, because the
-- native already exists. So a caller claims the succession *before* the first
-- effect, and `ux_core_team_route_succession_owner` makes that claim unique per
-- (project, seat, predecessor occupancy). The loser refuses having launched
-- nothing.
--
-- **Recovery.** A process lost after the store transition and before the
-- receipt left durable generation-2 state that no key could find. The claim is
-- already there to find, so the replay completes rather than re-planning
-- against a seat that has moved underneath it.
--
-- **Honest completion.** A route commit alone is not the whole effect: the
-- launch intent must be installed and the SeatBinding observed. Recording those
-- two as columns is what stops a replay reporting a complete succession whose
-- pending effects never landed (ASMA-8187, acceptance 3 and 4).
CREATE TABLE core_team_route_successions (
    -- The apply key a replay arrives holding.
    idempotency_key                  TEXT NOT NULL PRIMARY KEY
        CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    -- The exact pre-effect intent the claim was admitted under. A replay whose
    -- request derives a different intent is a different command wearing a used
    -- key, and is refused rather than answered from this row.
    intent_hash                      TEXT NOT NULL CHECK (
        length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'
    ),
    project_id                       TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id                  TEXT NOT NULL REFERENCES mini_projects(id) ON DELETE RESTRICT,
    -- The logical seat the succession preserves. Deliberately not a foreign key
    -- to `hosted_topology_seats`: that row is the *current* occupant and moves
    -- under later successions, while this evidence must outlive every one.
    seat_binding_id                  TEXT NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,

    -- Claimed before any effect.
    predecessor_native_id            TEXT NOT NULL CHECK (length(predecessor_native_id) BETWEEN 1 AND 256),
    predecessor_generation           INTEGER NOT NULL CHECK (predecessor_generation >= 0),
    predecessor_occupancy_generation INTEGER NOT NULL CHECK (predecessor_occupancy_generation >= 1),
    successor_occupancy_generation   INTEGER NOT NULL CHECK (successor_occupancy_generation >= 1),
    -- The credential generation the successor will be launched under. An
    -- identity and a number: never a credential value, never a bearer, never an
    -- environment. A predecessor's grant is not copied here because it is not
    -- recorded here at all.
    successor_credential_generation  INTEGER NOT NULL CHECK (successor_credential_generation >= 1),
    claimed_at                       TEXT NOT NULL,

    -- Filled when the transition commits, in that same transaction.
    successor_native_id              TEXT NULL CHECK (
        successor_native_id IS NULL OR length(successor_native_id) BETWEEN 1 AND 256
    ),
    successor_generation             INTEGER NULL CHECK (
        successor_generation IS NULL OR successor_generation >= 0
    ),
    -- The complete final readback. Persisted rather than recomputed, because
    -- recomputation answers with the seat's *current* state, which is precisely
    -- the wrong answer once the seat has moved on.
    readback                         TEXT NULL CHECK (readback IS NULL OR json_valid(readback)),
    readback_hash                    TEXT NULL CHECK (
        readback_hash IS NULL OR (length(readback_hash) = 64 AND readback_hash NOT GLOB '*[^0-9a-f]*')
    ),
    route_committed_at               TEXT NULL,

    -- The two effects that follow the route commit. A succession is complete
    -- only when both have landed; until then a replay reconciles them rather
    -- than reporting success.
    launch_intent_installed          INTEGER NOT NULL DEFAULT 0 CHECK (launch_intent_installed IN (0, 1)),
    seat_binding_observed            INTEGER NOT NULL DEFAULT 0 CHECK (seat_binding_observed IN (0, 1)),

    receipt_id                       TEXT NULL REFERENCES command_receipts(id) ON DELETE RESTRICT,
    receipted_at                     TEXT NULL,

    -- A committed transition names its successor and its evidence together, or
    -- names neither. There is no half-committed shape to read.
    CHECK (
        (route_committed_at IS NULL
            AND successor_native_id IS NULL AND successor_generation IS NULL
            AND readback IS NULL AND readback_hash IS NULL)
        OR
        (route_committed_at IS NOT NULL
            AND successor_native_id IS NOT NULL AND successor_generation IS NOT NULL
            AND readback IS NOT NULL AND readback_hash IS NOT NULL)
    ),
    CHECK ((receipt_id IS NULL AND receipted_at IS NULL)
           OR (receipt_id IS NOT NULL AND receipted_at IS NOT NULL)),
    -- A receipt is the statement that this command finished. A row may only
    -- carry one once the route has committed *and* both trailing effects have
    -- landed, because a receipt on a half-landed succession is exactly the
    -- misleading complete result the pending columns exist to prevent. The
    -- storage rule stands beside the binder's own predicate: an application
    -- check that is the only guard is a guard that one caller can forget.
    CHECK (receipt_id IS NULL
           OR (route_committed_at IS NOT NULL
               AND launch_intent_installed = 1
               AND seat_binding_observed = 1)),
    -- Pending effects cannot be marked landed before the route they follow.
    CHECK (route_committed_at IS NOT NULL
           OR (launch_intent_installed = 0 AND seat_binding_observed = 0))
) STRICT;

-- One owner per (seat, predecessor occupancy). This is the exclusivity itself:
-- a second caller with a second fresh key cannot insert, so it never reaches
-- the retire or the launch. It is a uniqueness constraint rather than an
-- application check because two concurrent transactions both pass a check and
-- only one passes an index.
CREATE UNIQUE INDEX ux_core_team_route_succession_owner
ON core_team_route_successions (project_id, seat_binding_id, predecessor_occupancy_generation);

-- A succession advances exactly one occupancy generation, and its credential
-- generation is the successor's own. Recording anything else would make the
-- pair unusable as the "which occupancy did this command produce" answer.
CREATE TRIGGER core_team_route_succession_advances_one_generation
BEFORE INSERT ON core_team_route_successions
WHEN NEW.successor_occupancy_generation <> NEW.predecessor_occupancy_generation + 1
  OR NEW.successor_credential_generation <> NEW.successor_occupancy_generation
BEGIN
    SELECT RAISE(ABORT,
        'a Core Team route succession advances exactly one occupancy and credential generation');
END;

-- The claim is frozen from the moment it commits; only the outcome columns move,
-- and each of them only once, forward. A claim that could be re-pointed at
-- another predecessor would be an exclusivity token that does not name what it
-- excludes.
--
-- `IS NOT` throughout rather than `<>`: SQLite treats a WHEN that evaluates to
-- NULL as false, so a nullable column compared with `<>` silently admits the
-- rows the rule looks like it forbids.
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
  -- The receipt binds once and never moves.
  OR (OLD.receipt_id IS NOT NULL AND OLD.receipt_id IS NOT NEW.receipt_id)
  -- Pending effects latch forward only.
  OR (OLD.launch_intent_installed = 1 AND NEW.launch_intent_installed = 0)
  OR (OLD.seat_binding_observed = 1 AND NEW.seat_binding_observed = 0)
BEGIN
    SELECT RAISE(ABORT,
        'a Core Team route succession cannot rewrite its claim or its committed evidence');
END;

-- Deleting one reopens every window this table closes: the seat would hold a
-- successor with no owner, no reconstruction path and no exclusivity token, and
-- the next caller would launch a second native. Command receipts have been
-- undeletable by trigger since `0001_init` for the same reason.
CREATE TRIGGER core_team_route_successions_are_undeletable
BEFORE DELETE ON core_team_route_successions
BEGIN
    SELECT RAISE(ABORT, 'a recorded Core Team route succession cannot be deleted');
END;

PRAGMA user_version = 120;
