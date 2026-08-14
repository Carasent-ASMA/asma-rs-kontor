-- ===========================================================================
-- Schema v18. Bounded Kontor role turns, and the follow-ups they derive.
--
-- # Why a turn is not a run
--
-- A Paseo seat is *persistent*: one native session serves a role slot across
-- many pieces of work. A Kontor role turn is one bounded piece of that work.
-- Conflating them is what left the control plane with nothing honest to close:
-- the turn was finished and the only closure available was the run's, whose
-- terminal evidence must come from the runtime — and the runtime had not ended
-- anything, because the seat is still sitting there ready for the next turn.
--
-- So a turn gets its own receipt, and settling one closes the *turn*. The agent
-- run stays non-terminal, its binding stays live, and the next turn on that slot
-- resumes the same native session. Nothing here writes `agent_runs.terminal`,
-- and nothing here is admissible as terminal evidence.
--
-- # What the receipt attests, and what it must never claim
--
-- It attests that **Kontor's** bounded turn completed, under a named actor's
-- authority, against a named task revision and native binding generation, citing
-- the artifacts it produced.
--
-- It does **not** claim the runtime emitted a verdict. Archive, an
-- `attentionReason` of finished, idle, a closed stream and a lost process remain
-- insufficient terminal evidence exactly as before (CON-003 / CON-011): this
-- table is a different axis of authority, not a cheaper route to the same
-- conclusion. A turn receipt says "Kontor is done asking for this"; only the
-- runtime can say "this session ended".
--
-- `(project_id, agent_run_id, role_slot_id, turn_ordinal)` is unique, so turns
-- on one seat are a dense ordered sequence and a replay cannot open a second
-- turn at the same position. `idempotency_key` is unique realm-wide, on the same
-- rule every other mutation keeps.
-- ===========================================================================

CREATE TABLE role_turns (
    id                 TEXT    NOT NULL PRIMARY KEY
                               CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id         TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id            TEXT    NOT NULL,
    team_run_id        TEXT    NOT NULL,
    agent_run_id       TEXT    NOT NULL,
    role_slot_id       TEXT    NOT NULL CHECK (length(role_slot_id) BETWEEN 1 AND 128),
    turn_ordinal       INTEGER NOT NULL CHECK (turn_ordinal >= 1),
    idempotency_key    TEXT    NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    task_revision      INTEGER NOT NULL CHECK (task_revision >= 1),
    binding_generation INTEGER NOT NULL CHECK (binding_generation >= 1),
    -- The authority that settled it, as the *authenticated caller* presented it.
    --
    -- Deliberately not a caller-supplied account id. Checking that a named
    -- account exists and is enabled proves nothing about who is asking, so
    -- persisting one as attribution would record a claim the control plane never
    -- verified. What it can verify is the tier the bearer authenticated at, so
    -- that is what is kept, and it is kept under a name that says so.
    authority_tier     TEXT    NOT NULL CHECK (authority_tier IN
                                   ('observer', 'operator', 'admin')),
    -- The provider account the *seat* runs as, derived from the bound run rather
    -- than supplied. It is operational context, never caller identity.
    account_profile    TEXT    NULL
                               CHECK (account_profile IS NULL
                                      OR (length(account_profile) = 36
                                          AND account_profile NOT GLOB '*[^0-9a-f-]*')),
    artifacts          TEXT    NOT NULL CHECK (json_valid(artifacts)),
    evidence_hash      TEXT    NOT NULL
                               CHECK (length(evidence_hash) = 64
                                      AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    settled_at         TEXT    NOT NULL
                               CHECK (settled_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, agent_run_id, role_slot_id, turn_ordinal),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_role_turns_task ON role_turns (project_id, task_id);

-- A settled turn is a fact. Editing one would change what a follow-up was
-- derived from after the follow-up already happened.
CREATE TRIGGER role_turns_are_immutable
BEFORE UPDATE ON role_turns
BEGIN
    SELECT RAISE(ABORT, 'a settled role turn is immutable');
END;

CREATE TRIGGER role_turns_are_permanent
BEFORE DELETE ON role_turns
BEGIN
    SELECT RAISE(ABORT, 'a settled role turn is never removed');
END;

-- ===========================================================================
-- The follow-ups a settled turn derived.
--
-- This is what makes successor activation *at most once*. The primary key is the
-- settling turn plus the slot it hands to, so re-deriving the same follow-up —
-- on a replayed settlement, or on the next startup reconciliation re-reading the
-- same persisted facts — inserts nothing and dispatches nothing. The derivation
-- is therefore free to be re-run as often as the seam likes, which is what lets
-- it live in reconciliation rather than behind a timer.
--
-- `dispatched` records whether the effect actually reached the seat. A row with
-- `dispatched = 0` is a follow-up that was decided and not delivered; the next
-- reconciliation retries exactly that one rather than deriving a second.
-- ===========================================================================

CREATE TABLE turn_dispatches (
    settled_turn_id  TEXT    NOT NULL REFERENCES role_turns (id) ON DELETE RESTRICT,
    to_role_slot_id  TEXT    NOT NULL CHECK (length(to_role_slot_id) BETWEEN 1 AND 128),
    project_id       TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    team_run_id      TEXT    NOT NULL,
    -- The message the follow-up is delivered as, fixed when the row is derived.
    --
    -- A retry of an undelivered follow-up must be the *same* message, or an
    -- effect the runtime already committed but could not acknowledge would be
    -- delivered a second time. Generating an id per attempt is exactly that bug,
    -- so the id belongs to the dispatch and not to the attempt.
    message_id       TEXT    NOT NULL
                             CHECK (length(message_id) = 36
                                    AND message_id NOT GLOB '*[^0-9a-f-]*'),
    target_agent_run TEXT    NULL
                             CHECK (target_agent_run IS NULL
                                    OR (length(target_agent_run) = 36
                                        AND target_agent_run NOT GLOB '*[^0-9a-f-]*')),
    dispatched       INTEGER NOT NULL CHECK (dispatched IN (0, 1)),
    derived_at       TEXT    NOT NULL
                             CHECK (derived_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (settled_turn_id, to_role_slot_id)
) STRICT;

CREATE INDEX ix_turn_dispatches_undelivered
    ON turn_dispatches (project_id, dispatched);

PRAGMA user_version = 18;
