-- ===========================================================================
-- Schema v20. A declared role slot that was never bound is accounted for by an
-- explicit, authorized waiver — and by nothing else.
--
-- v19 can close a team when every declared slot settled a turn. A slot that
-- never got a seat cannot settle one, and the two ways to "finish" such a team
-- without this table are both lies: invent an `agent_runs` row and cast it
-- terminal (a claim about a runtime nothing observed), or let the closure
-- silently skip the slot (a claim the template never authorized). A waiver is
-- the third answer: the frozen template said this seat *may* be excused, a role
-- the template authorized excused it, and the evidence it demanded was cited.
--
-- Two structural properties carry the whole design:
--
-- * `UNIQUE (project_id, team_run_id, role_slot_id)` — a slot is waived once or
--   not at all. A second waiver is not an append, it is a refusal.
-- * the cross-table triggers below — a waiver and a *bound seat* are mutually
--   exclusive in both directions and at every point in time. Checking that only
--   in the application would leave the invariant true of the code that happened
--   to be running rather than of the data.
--
-- `evidence_hash` is recomputed and compared at closure, so what it covers is
-- load-bearing: schema version, operation name, project/task/team-run/slot,
-- the team revision the waiver was taken against, the authorizing role and the
-- tier the credential proved, and the sorted, deduplicated evidence keys. It
-- deliberately excludes the waiver id, the idempotency key and the timestamp,
-- so an identical retry hashes identically and replay is recognisable.
-- ===========================================================================

-- ---------------------------------------------------------------------------
-- team_runs: a fourth terminal source.
--
-- Rebuilt rather than altered, because SQLite cannot change a `CHECK` in place.
-- Same procedure, and the same safety argument, as v19: `migrate` lifts
-- reference enforcement around the migration transaction and runs
-- `PRAGMA foreign_key_check` over the whole database before it commits.
-- ---------------------------------------------------------------------------

CREATE TABLE team_runs_v20 (
    id                      TEXT    NOT NULL PRIMARY KEY
                                    CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id              TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id                 TEXT    NOT NULL,
    template_id             TEXT    NOT NULL,
    template_version        INTEGER NOT NULL CHECK (template_version >= 1),
    snapshot                TEXT    NOT NULL CHECK (json_valid(snapshot)),
    snapshot_hash           TEXT    NOT NULL
                                    CHECK (length(snapshot_hash) = 64 AND snapshot_hash NOT GLOB '*[^0-9a-f]*'),
    lifecycle               TEXT    NOT NULL CHECK (lifecycle IN
                                    ('queued', 'launching', 'running', 'waiting_input', 'blocked',
                                     'succeeded', 'failed', 'cancelled', 'parked')),
    terminal_outcome        TEXT    NULL CHECK (terminal_outcome IS NULL OR terminal_outcome IN
                                    ('succeeded', 'failed', 'cancelled', 'parked', 'abandoned')),
    -- Evidence is a pointer into persisted rows: a child-evidence closure names
    -- the team whose children were counted, an operator closure names its
    -- receipt, a settled-turn closure names the team whose declared slots must
    -- each have an immutable `role_turns` row, and a role-slot-disposition
    -- closure names the team whose declared slots must each have *exactly one*
    -- of a settled turn or a waiver. The last two cite no receipt and expect
    -- their children to still be live.
    terminal_source_kind    TEXT    NULL CHECK (terminal_source_kind IS NULL OR terminal_source_kind IN
                                    ('child_evidence', 'operator_abandon', 'settled_turns',
                                     'role_slot_dispositions')),
    terminal_receipt_id     TEXT    NULL,
    terminal_evidence_hash  TEXT    NULL
                                    CHECK (terminal_evidence_hash IS NULL
                                           OR (length(terminal_evidence_hash) = 64
                                               AND terminal_evidence_hash NOT GLOB '*[^0-9a-f]*')),
    closed_at               TEXT    NULL
                                    CHECK (closed_at IS NULL OR closed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revision                INTEGER NOT NULL CHECK (revision >= 1),
    created_at              TEXT    NOT NULL
                                    CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    CHECK (lifecycle NOT IN ('succeeded', 'failed', 'cancelled', 'parked')
           OR (closed_at IS NOT NULL AND terminal_outcome IS NOT NULL
               AND terminal_source_kind IS NOT NULL AND terminal_evidence_hash IS NOT NULL)),
    CHECK ((terminal_outcome IS NULL) = (terminal_source_kind IS NULL)),
    CHECK ((terminal_source_kind IS NULL) = (terminal_evidence_hash IS NULL)),
    CHECK ((terminal_source_kind = 'operator_abandon') = (terminal_receipt_id IS NOT NULL)),
    CHECK (terminal_source_kind IS NOT 'operator_abandon' OR terminal_outcome = 'abandoned'),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, terminal_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, template_id, template_version)
        REFERENCES team_templates (project_id, template_id, version) ON DELETE RESTRICT
) STRICT;

INSERT INTO team_runs_v20
SELECT id, project_id, task_id, template_id, template_version, snapshot, snapshot_hash,
       lifecycle, terminal_outcome, terminal_source_kind, terminal_receipt_id,
       terminal_evidence_hash, closed_at, revision, created_at
FROM team_runs;

DROP TABLE team_runs;
ALTER TABLE team_runs_v20 RENAME TO team_runs;

-- All four are recreated verbatim: the rebuild drops every trigger the old
-- table carried, and losing one would quietly remove a guard that direct-SQL
-- tests exist to prove.
CREATE TRIGGER team_runs_terminal_immutable BEFORE UPDATE ON team_runs
WHEN OLD.lifecycle IN ('succeeded', 'failed', 'cancelled', 'parked')
BEGIN SELECT RAISE(ABORT, 'a closed team run is immutable and cannot reopen'); END;

CREATE TRIGGER team_runs_evidence_immutable BEFORE UPDATE ON team_runs
WHEN OLD.terminal_source_kind IS NOT NULL
     AND (OLD.terminal_source_kind IS NOT NEW.terminal_source_kind
          OR OLD.terminal_receipt_id IS NOT NEW.terminal_receipt_id
          OR OLD.terminal_evidence_hash IS NOT NEW.terminal_evidence_hash
          OR OLD.closed_at IS NOT NEW.closed_at)
BEGIN SELECT RAISE(ABORT, 'team closure evidence is immutable'); END;

CREATE TRIGGER team_runs_no_delete BEFORE DELETE ON team_runs
BEGIN SELECT RAISE(ABORT, 'team runs are not deletable'); END;

CREATE TRIGGER team_runs_snapshot_immutable BEFORE UPDATE ON team_runs
WHEN OLD.snapshot <> NEW.snapshot
     OR OLD.snapshot_hash <> NEW.snapshot_hash
     OR OLD.template_id <> NEW.template_id
     OR OLD.template_version <> NEW.template_version
     OR OLD.project_id <> NEW.project_id
     OR OLD.task_id <> NEW.task_id
     OR OLD.created_at <> NEW.created_at
BEGIN SELECT RAISE(ABORT, 'a pinned team snapshot is immutable'); END;

-- ---------------------------------------------------------------------------
-- role_slot_waivers
-- ---------------------------------------------------------------------------

CREATE TABLE role_slot_waivers (
    id                TEXT    NOT NULL PRIMARY KEY
                              CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id        TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id           TEXT    NOT NULL,
    team_run_id       TEXT    NOT NULL,
    role_slot_id      TEXT    NOT NULL CHECK (length(role_slot_id) BETWEEN 1 AND 128),
    idempotency_key   TEXT    NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    team_run_revision INTEGER NOT NULL CHECK (team_run_revision >= 1),
    authorized_role   TEXT    NOT NULL CHECK (length(authorized_role) BETWEEN 1 AND 128),
    -- One tier, spelled out rather than implied. A waiver is an admin act, and
    -- a column that could hold anything else would invite one.
    authority_tier    TEXT    NOT NULL CHECK (authority_tier = 'admin'),
    evidence          TEXT    NOT NULL CHECK (json_valid(evidence)
                                              AND json_type(evidence) = 'array'
                                              AND json_array_length(evidence) >= 1),
    evidence_hash     TEXT    NOT NULL
                              CHECK (length(evidence_hash) = 64 AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at       TEXT    NOT NULL
                              CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- The slot is waived once or not at all.
    UNIQUE (project_id, team_run_id, role_slot_id),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_run_id) REFERENCES team_runs (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_role_slot_waivers_team ON role_slot_waivers (project_id, team_run_id);

-- A waiver is permanent. Correcting one would rewrite the reason a team closed.
CREATE TRIGGER role_slot_waivers_are_immutable BEFORE UPDATE ON role_slot_waivers
BEGIN SELECT RAISE(ABORT, 'a role slot waiver is immutable'); END;

CREATE TRIGGER role_slot_waivers_are_permanent BEFORE DELETE ON role_slot_waivers
BEGIN SELECT RAISE(ABORT, 'a role slot waiver is never removed'); END;

-- --- waiver ← seat: a slot that was ever bound cannot be waived --------------
--
-- The binding *history* is what is consulted, not a live session: a lost
-- process or an unreachable runtime is not an unbound slot, and this is the
-- fact that says so.
CREATE TRIGGER role_slot_waivers_refuse_ever_bound BEFORE INSERT ON role_slot_waivers
WHEN EXISTS (
    SELECT 1
    FROM runtime_bindings AS binding
    JOIN agent_runs AS run
      ON run.id = binding.agent_run_id AND run.project_id = binding.project_id
    WHERE run.project_id = NEW.project_id
      AND run.team_run_id = NEW.team_run_id
      AND run.role_key = NEW.role_slot_id)
BEGIN SELECT RAISE(ABORT, 'a role slot that was ever bound cannot be waived'); END;

CREATE TRIGGER role_slot_waivers_refuse_settled BEFORE INSERT ON role_slot_waivers
WHEN EXISTS (
    SELECT 1 FROM role_turns AS turn
    WHERE turn.project_id = NEW.project_id
      AND turn.team_run_id = NEW.team_run_id
      AND turn.role_slot_id = NEW.role_slot_id)
BEGIN SELECT RAISE(ABORT, 'a role slot that settled a turn cannot be waived'); END;

CREATE TRIGGER role_slot_waivers_refuse_after_terminal BEFORE INSERT ON role_slot_waivers
WHEN EXISTS (
    SELECT 1 FROM team_runs AS team
    WHERE team.project_id = NEW.project_id
      AND team.id = NEW.team_run_id
      AND team.terminal_source_kind IS NOT NULL)
BEGIN SELECT RAISE(ABORT, 'a closed team run cannot waive a role slot'); END;

-- --- seat ← waiver: the same exclusion, enforced from the other side ---------

CREATE TRIGGER role_turns_refuse_waived_slot BEFORE INSERT ON role_turns
WHEN EXISTS (
    SELECT 1 FROM role_slot_waivers AS waiver
    WHERE waiver.project_id = NEW.project_id
      AND waiver.team_run_id = NEW.team_run_id
      AND waiver.role_slot_id = NEW.role_slot_id)
BEGIN SELECT RAISE(ABORT, 'a waived role slot cannot settle a turn'); END;

CREATE TRIGGER runtime_bindings_refuse_waived_slot BEFORE INSERT ON runtime_bindings
WHEN EXISTS (
    SELECT 1
    FROM agent_runs AS run
    JOIN role_slot_waivers AS waiver
      ON waiver.project_id = run.project_id
     AND waiver.team_run_id = run.team_run_id
     AND waiver.role_slot_id = run.role_key
    WHERE run.id = NEW.agent_run_id AND run.project_id = NEW.project_id)
BEGIN SELECT RAISE(ABORT, 'a waived role slot cannot be bound to a session'); END;

CREATE TRIGGER agent_runs_refuse_waived_slot BEFORE INSERT ON agent_runs
WHEN EXISTS (
    SELECT 1 FROM role_slot_waivers AS waiver
    WHERE waiver.project_id = NEW.project_id
      AND waiver.team_run_id = NEW.team_run_id
      AND waiver.role_slot_id = NEW.role_key)
BEGIN SELECT RAISE(ABORT, 'a waived role slot cannot be seated'); END;

-- The placeholder run a waived slot may already own stays exactly as it was.
-- Its lifecycle, its states and its terminal fields are all frozen: waiving a
-- slot must never become a back door to declaring a run finished.
CREATE TRIGGER agent_runs_refuse_waived_slot_update BEFORE UPDATE ON agent_runs
WHEN (OLD.lifecycle IS NOT NEW.lifecycle
      OR OLD.desired_state IS NOT NEW.desired_state
      OR OLD.observed_state IS NOT NEW.observed_state
      OR OLD.derived_state IS NOT NEW.derived_state
      OR OLD.terminal_outcome IS NOT NEW.terminal_outcome)
     AND EXISTS (
         SELECT 1 FROM role_slot_waivers AS waiver
         WHERE waiver.project_id = OLD.project_id
           AND waiver.team_run_id = OLD.team_run_id
           AND waiver.role_slot_id = OLD.role_key)
BEGIN SELECT RAISE(ABORT, 'a waived role slot has no operable run'); END;

PRAGMA user_version = 20;
