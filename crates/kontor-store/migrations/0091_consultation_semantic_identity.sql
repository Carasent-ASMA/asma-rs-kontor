-- Schema v90. One server-derived semantic identity for every new Advisor or
-- Committee invocation. Legacy runs remain NULL until a supported correction
-- adopts an identity; a caller-controlled idempotency key is never the
-- uniqueness boundary for a native consultation container.

ALTER TABLE consultation_runs
    ADD COLUMN semantic_identity_hash TEXT NULL
        CHECK (
            semantic_identity_hash IS NULL
            OR (
                length(semantic_identity_hash) = 64
                AND semantic_identity_hash NOT GLOB '*[^0-9a-f]*'
            )
        );

CREATE UNIQUE INDEX consultation_runs_by_semantic_identity
    ON consultation_runs (project_id, semantic_identity_hash)
    WHERE semantic_identity_hash IS NOT NULL;

-- A malformed pre-enforcement topic may be corrected exactly once. The
-- before/after pair and command authority remain immutable while the run,
-- topology node, seats, findings and native binding keep their identities.
CREATE TABLE consultation_topic_corrections (
    receipt_id             TEXT NOT NULL PRIMARY KEY,
    project_id             TEXT NOT NULL,
    run_id                 TEXT NOT NULL,
    family                 TEXT NOT NULL CHECK (family IN ('advisor', 'committee')),
    prior_topic            TEXT NOT NULL,
    corrected_topic        TEXT NOT NULL,
    semantic_identity_hash TEXT NOT NULL CHECK (
                               length(semantic_identity_hash) = 64
                               AND semantic_identity_hash NOT GLOB '*[^0-9a-f]*'
                           ),
    reason                 TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 2048),
    corrected_at           TEXT NOT NULL,
    UNIQUE (project_id, run_id),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE RESTRICT,
    FOREIGN KEY (run_id) REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts(project_id, id) ON DELETE RESTRICT,
    CHECK (prior_topic <> corrected_topic)
) STRICT;
CREATE TRIGGER consultation_topic_corrections_are_immutable
BEFORE UPDATE ON consultation_topic_corrections
BEGIN SELECT RAISE(ABORT, 'consultation topic corrections are immutable evidence'); END;
CREATE TRIGGER consultation_topic_corrections_are_permanent
BEFORE DELETE ON consultation_topic_corrections
BEGIN SELECT RAISE(ABORT, 'consultation topic corrections are permanent evidence'); END;

PRAGMA user_version = 91;
