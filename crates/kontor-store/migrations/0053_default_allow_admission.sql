-- ===========================================================================
-- Schema v53. Default-allow admission.
--
-- An admission no longer has to name an authorization. Unarmed work is allowed
-- the same way an unconfigured calendar is unrestricted; a grant only *narrows*
-- (window, concurrency, selected tasks); a disarm is an explicit stop and is
-- not a return to unarmed.
--
-- SQLite cannot drop a CHECK, so the table is rebuilt. Foreign keys, the
-- one-admission-per-run unique index, the task index and the immutability
-- triggers are recreated with the table. The only rule that changes is the
-- pairing CHECK that required `authorization_id` on every `admitted` row.
-- ===========================================================================

CREATE TABLE scheduler_admission_events_v53 (
    id                TEXT NOT NULL PRIMARY KEY
                           CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id        TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id           TEXT NOT NULL,
    decision          TEXT NOT NULL CHECK (decision IN ('admitted', 'rejected')),
    rejection_code    TEXT NULL
                           CHECK (rejection_code IS NULL
                                  OR (length(rejection_code) BETWEEN 1 AND 128
                                      AND rejection_code NOT GLOB '*[^a-z0-9_]*')),
    team_run_id       TEXT NULL,
    agent_run_id      TEXT NULL,
    launch_receipt_id TEXT NULL,
    -- NULL is default-allow: nothing narrowed this run. A grant id is recorded
    -- only when an active authorization was attached.
    authorization_id  TEXT NULL,
    evidence          TEXT NOT NULL CHECK (json_valid(evidence)),
    evidence_hash     TEXT NOT NULL
                           CHECK (length(evidence_hash) = 64
                                  AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    decided_at        TEXT NOT NULL
                           CHECK (decided_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    CHECK ((decision = 'rejected') = (rejection_code IS NOT NULL)),
    CHECK ((decision = 'admitted') = (team_run_id IS NOT NULL)),
    CHECK ((decision = 'admitted') = (agent_run_id IS NOT NULL)),
    CHECK ((decision = 'admitted') = (launch_receipt_id IS NOT NULL)),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_run_id)
        REFERENCES team_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, launch_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, authorization_id)
        REFERENCES execution_authorizations (project_id, id) ON DELETE RESTRICT
) STRICT;

INSERT INTO scheduler_admission_events_v53
        (id, project_id, task_id, decision, rejection_code, team_run_id,
         agent_run_id, launch_receipt_id, authorization_id, evidence,
         evidence_hash, decided_at)
SELECT   id, project_id, task_id, decision, rejection_code, team_run_id,
         agent_run_id, launch_receipt_id, authorization_id, evidence,
         evidence_hash, decided_at
FROM     scheduler_admission_events;

-- This trigger lives on `resource_leases` and SELECTs the admission table.
-- Dropping the table while it still exists would abort the rebuild.
DROP TRIGGER resource_leases_admission_in_project;

DROP TABLE scheduler_admission_events;
ALTER TABLE scheduler_admission_events_v53 RENAME TO scheduler_admission_events;

CREATE UNIQUE INDEX ux_scheduler_admission_events_run
    ON scheduler_admission_events (project_id, agent_run_id)
    WHERE agent_run_id IS NOT NULL;

CREATE INDEX ix_scheduler_admission_events_task
    ON scheduler_admission_events (project_id, task_id, decided_at);

CREATE TRIGGER scheduler_admission_events_no_update
BEFORE UPDATE ON scheduler_admission_events
BEGIN SELECT RAISE(ABORT, 'an admission decision is immutable'); END;
CREATE TRIGGER scheduler_admission_events_no_delete
BEFORE DELETE ON scheduler_admission_events
BEGIN SELECT RAISE(ABORT, 'an admission decision is immutable'); END;

CREATE TRIGGER resource_leases_admission_in_project
BEFORE INSERT ON resource_leases
WHEN NEW.admission_event_id IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM scheduler_admission_events
                     WHERE project_id = NEW.project_id AND id = NEW.admission_event_id)
BEGIN SELECT RAISE(ABORT, 'a lease names an admission from another project'); END;

PRAGMA user_version = 53;
