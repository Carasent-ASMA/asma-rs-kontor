-- ASMA-8514 / KR-09: the typed operational-gap ledger.
--
-- Before this table a seat whose intended Kontor operation was missing had to
-- leave its containment evidence in a repository document. That made a
-- control-plane fact live outside the control plane and gave the gap collector
-- nothing durable to count. This is the smallest record the transition rule
-- requires: the intended operation, its failure class, the exact task/epic/run/
-- seat identities, the checkpoint, the bounded fallback and the owner.
--
-- The record is append-only and carries the state it was written in. Closure is
-- a later, separately authorized operation, so no update path exists here.
CREATE TABLE operational_gaps (
    idempotency_key    TEXT    NOT NULL UNIQUE
                               CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    gap_id             TEXT    NOT NULL
                               CHECK (length(gap_id) = 36 AND gap_id NOT GLOB '*[^0-9a-f-]*'),
    project_id         TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id    TEXT    NULL,
    task_id            TEXT    NOT NULL,
    agent_run_id       TEXT    NULL,
    seat_binding_id    TEXT    NULL REFERENCES seat_bindings (id) ON DELETE RESTRICT,
    failure_class      TEXT    NOT NULL CHECK (length(failure_class) BETWEEN 1 AND 128),
    intended_operation TEXT    NOT NULL CHECK (length(intended_operation) BETWEEN 1 AND 65536),
    checkpoint         TEXT    NOT NULL CHECK (length(checkpoint) BETWEEN 1 AND 65536),
    fallback_action    TEXT    NOT NULL CHECK (length(fallback_action) BETWEEN 1 AND 65536),
    owner              TEXT    NOT NULL CHECK (length(owner) BETWEEN 1 AND 65536),
    evidence_hash      TEXT    NULL
                               CHECK (evidence_hash IS NULL
                                      OR (length(evidence_hash) = 64
                                          AND evidence_hash NOT GLOB '*[^0-9a-f]*')),
    status             TEXT    NOT NULL CHECK (status IN ('open')),
    recorded_at        TEXT    NOT NULL
                               CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, gap_id),
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_operational_gaps_task ON operational_gaps (project_id, task_id, recorded_at);

CREATE TRIGGER operational_gaps_no_update BEFORE UPDATE ON operational_gaps
BEGIN SELECT RAISE(ABORT, 'operational gaps are append-only'); END;
CREATE TRIGGER operational_gaps_no_delete BEFORE DELETE ON operational_gaps
BEGIN SELECT RAISE(ABORT, 'operational gaps are append-only'); END;

PRAGMA user_version = 135;
