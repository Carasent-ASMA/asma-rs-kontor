-- Schema v110. Adopting an already-created AgentRun into a declared TeamRun
-- slot.
--
-- A run can exist, correctly, before anything has claimed it for the slot it
-- belongs to: the scheduler admitted nothing for it, no turn was dispatched to
-- it, and no runtime ever bound it. Seat fill cannot act on such a run today,
-- because the only authority it recognises is an owed dispatch, and there is
-- none. The alternative — minting a second AgentRun for the slot — would throw
-- away the identity the first one already has.
--
-- This version adds the durable half of the other path: an append-only ledger
-- of which run was adopted into which slot, on whose authority, and at which
-- revision of that run. The adoption is evidence that a specific existing run
-- was claimed for a specific declared slot; it is not the seating, it creates
-- no run, and recording one dispatches nothing by itself.
--
-- Nothing here admits a candidate, creates or mutates a TeamRun, AgentRun,
-- task, binding, native session or topology node.

-- One row per adopted run. Every identity the adoption is *about* is stored
-- rather than derived later, so a reader can see what was claimed without
-- re-deriving it, and so a mismatch names the field that disagreed.
--
-- The three uniqueness rules are the contract, and each refuses a different
-- mistake:
--
--   * `(project_id, team_run_id, role_slot_id)` — one slot is adopted once.
--     A second adoption of the same slot is a different claim about the same
--     place and must refuse rather than overwrite.
--   * `(project_id, agent_run_id)` — one run is adopted once. The same run
--     cannot be claimed by two slots, which is what would let a single run owe
--     two seats.
--   * `receipt_id` — one command records one adoption. An exact-key replay
--     therefore reads the row the first call wrote instead of writing a second,
--     and a receipt that already recorded a different claim is refused.
--
-- The foreign keys are project-scoped composites against the `(project_id, id)`
-- uniqueness every parent already declares, so a row cannot name a task, team
-- run, agent run or receipt belonging to another project even if the bare id
-- happened to exist there.
CREATE TABLE team_run_admission_adoptions (
    id TEXT NOT NULL PRIMARY KEY
        CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id TEXT NOT NULL,
    team_run_id TEXT NOT NULL,
    role_slot_id TEXT NOT NULL CHECK (length(role_slot_id) BETWEEN 1 AND 128),
    agent_run_id TEXT NOT NULL,
    -- The revision the adopting caller proved it had read. A later seat fill
    -- revalidates against this, so a run that moved after the adoption cannot
    -- be seated on the strength of a stale claim.
    adopted_agent_run_revision INTEGER NOT NULL
        CHECK (adopted_agent_run_revision >= 1),
    receipt_id TEXT NOT NULL,
    adopted_at TEXT NOT NULL
        CHECK (adopted_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    UNIQUE (project_id, team_run_id, role_slot_id),
    UNIQUE (project_id, agent_run_id),
    UNIQUE (receipt_id),
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_run_id)
        REFERENCES team_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

-- Seat fill asks "is this slot adopted?" and recovery asks "what did this team
-- adopt?", so the team is the leading column of the lookup.
CREATE INDEX ix_team_run_admission_adoptions_team
    ON team_run_admission_adoptions (project_id, team_run_id, role_slot_id);

-- Append-only, like every other evidence ledger in this schema: an adoption
-- that can be edited or removed is not evidence that anything was adopted.
CREATE TRIGGER team_run_admission_adoptions_immutable BEFORE UPDATE
ON team_run_admission_adoptions
BEGIN SELECT RAISE(ABORT, 'a team-run admission adoption is immutable'); END;

CREATE TRIGGER team_run_admission_adoptions_no_delete BEFORE DELETE
ON team_run_admission_adoptions
BEGIN SELECT RAISE(ABORT, 'team-run admission adoptions are not deletable'); END;

PRAGMA user_version = 110;
