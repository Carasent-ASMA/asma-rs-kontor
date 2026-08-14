-- ===========================================================================
-- Schema v19. A team may close because its declared slots finished their turns.
--
-- `team_runs.terminal_source_kind` was a closed `CHECK` list of two: a team
-- closed because its child *runs* ended, or because an operator abandoned it.
-- Neither can express the case a persistent seat creates — every declared role
-- slot has settled its final bounded Kontor turn, and the native sessions are
-- deliberately still live.
--
-- Without a third kind the only way to close such a team is to cast a live
-- `AgentRun` terminal, which would be a claim about the runtime that nothing
-- observed. So the list is widened rather than the rule bent.
--
-- SQLite cannot alter a `CHECK` in place, so the table is rebuilt: new shape,
-- copy, drop, rename. `agent_runs`, `handoffs`, `guardrail_evaluations`,
-- `recovery_episodes` and `scheduler_admission_events` all reference it, so for
-- the two statements between the `DROP` and the `RENAME` their rows point at a
-- table that does not exist. That is safe here and nowhere else: `migrate` lifts
-- reference enforcement around the whole migration transaction and runs
-- `PRAGMA foreign_key_check` over the entire database before committing.
--
-- The two adjacent constraints need no change and are reproduced exactly:
-- `(terminal_source_kind = 'operator_abandon') = (terminal_receipt_id IS NOT NULL)`
-- holds for a receipt-free third kind (false = false), and
-- `terminal_source_kind IS NOT 'operator_abandon' OR terminal_outcome = 'abandoned'`
-- is vacuously true for it.
-- ===========================================================================

CREATE TABLE team_runs_v19 (
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
    -- receipt, and a settled-turn closure names the team whose declared slots
    -- must each have an immutable `role_turns` row. The last one cites no
    -- receipt and expects its children to still be live.
    terminal_source_kind    TEXT    NULL CHECK (terminal_source_kind IS NULL OR terminal_source_kind IN
                                    ('child_evidence', 'operator_abandon', 'settled_turns')),
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

INSERT INTO team_runs_v19
SELECT id, project_id, task_id, template_id, template_version, snapshot, snapshot_hash,
       lifecycle, terminal_outcome, terminal_source_kind, terminal_receipt_id,
       terminal_evidence_hash, closed_at, revision, created_at
FROM team_runs;

DROP TABLE team_runs;
ALTER TABLE team_runs_v19 RENAME TO team_runs;

-- The rebuild drops every trigger the old table carried, so all four are
-- recreated verbatim. Losing one would quietly remove a guard that direct-SQL
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

PRAGMA user_version = 19;
