-- Schema v67. A mutable quota projection cannot prove that an unchanged
-- provider answer was observed recently. Keep every successful exact-account
-- poll as a small immutable heartbeat beside that projection.
CREATE TABLE provider_usage_observations (
    id                 TEXT NOT NULL PRIMARY KEY,
    project_id         TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    account_profile_id TEXT NOT NULL,
    provider           TEXT NOT NULL CHECK (
        length(provider) BETWEEN 1 AND 128
        AND provider NOT GLOB '*[^a-z0-9._-]*'
    ),
    evidence_hash      TEXT NOT NULL CHECK (
        length(evidence_hash) = 64
        AND evidence_hash NOT GLOB '*[^0-9a-f]*'
    ),
    state              TEXT NOT NULL CHECK (
        state IN ('available', 'exhausted', 'drained', 'unknown', 'cannot_report')
    ),
    resets_at          TEXT NULL,
    windows            TEXT NOT NULL CHECK (json_valid(windows) AND json_type(windows) = 'array'),
    observed_at        TEXT NOT NULL,
    idempotency_key    TEXT NULL UNIQUE CHECK (
        idempotency_key IS NULL OR length(idempotency_key) BETWEEN 1 AND 256
    ),
    intent_hash        TEXT NULL CHECK (
        intent_hash IS NULL OR (
            length(intent_hash) = 64
            AND intent_hash NOT GLOB '*[^0-9a-f]*'
        )
    ),
    CHECK ((idempotency_key IS NULL) = (intent_hash IS NULL)),
    CHECK (
        (state = 'exhausted' AND resets_at IS NOT NULL)
        OR (state <> 'exhausted' AND resets_at IS NULL)
    ),
    FOREIGN KEY (project_id, account_profile_id)
        REFERENCES account_profiles(project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX provider_usage_observations_latest
ON provider_usage_observations (
    project_id, account_profile_id, provider, observed_at DESC, id DESC
);

CREATE TRIGGER provider_usage_observations_are_immutable
BEFORE UPDATE ON provider_usage_observations
BEGIN
    SELECT RAISE(ABORT, 'a provider usage observation is immutable');
END;

CREATE TRIGGER provider_usage_observations_are_permanent
BEFORE DELETE ON provider_usage_observations
BEGIN
    SELECT RAISE(ABORT, 'a provider usage observation cannot be withdrawn');
END;

PRAGMA user_version = 67;
