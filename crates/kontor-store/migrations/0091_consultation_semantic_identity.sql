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

PRAGMA user_version = 91;
