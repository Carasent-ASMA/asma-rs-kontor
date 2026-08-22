-- Schema v50. A quota state may name the provider's own usage endpoint as its
-- authority.
--
-- v48 admitted exactly two authorities: a parsed runtime message and an
-- operator's assertion. Both only ever arrive *after* something went wrong —
-- the first needs a refusal to parse, the second needs a person to notice one.
-- So a Realm could record that a window had closed and could never record that
-- it had reopened, and every `available` row in the table was a human typing
-- one in.
--
-- `provider_report` is the third authority: a structured answer from the
-- account's own usage endpoint, about a window that has not necessarily refused
-- anything. It is the only source that can lower a block without a human, and
-- keeping it distinct from `runtime_observation` is what lets an operator tell
-- "we were turned away" from "we asked, and this is the number" — which matter
-- differently when the two disagree.
--
-- SQLite cannot widen a CHECK in place, so the table is rebuilt with the v48
-- definition plus the new value. Every other column, constraint and comment is
-- carried forward unchanged; see 0048 for why each one is the way it is.

CREATE TABLE provider_quota_states_v50 (
    project_id         TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    account_profile_id TEXT NOT NULL,
    provider           TEXT NOT NULL CHECK (length(provider) BETWEEN 1 AND 64
                                            AND provider NOT GLOB '*[^0-9a-z._-]*'),
    state              TEXT NOT NULL CHECK (state IN
                            ('available', 'exhausted', 'drained', 'unknown')),
    resets_at          TEXT
                            CHECK (resets_at IS NULL
                                   OR resets_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    evidence_hash      TEXT NOT NULL
                            CHECK (length(evidence_hash) = 64
                                   AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    source             TEXT NOT NULL CHECK (source IN
                            ('runtime_observation', 'provider_report', 'operator')),
    observed_at        TEXT NOT NULL
                            CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revision           INTEGER NOT NULL CHECK (revision > 0),
    updated_at         TEXT NOT NULL
                            CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, account_profile_id, provider),
    FOREIGN KEY (project_id, account_profile_id)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    CHECK ((state = 'exhausted' AND resets_at IS NOT NULL)
           OR (state <> 'exhausted' AND resets_at IS NULL))
) STRICT;

INSERT INTO provider_quota_states_v50
SELECT project_id, account_profile_id, provider, state, resets_at, evidence_hash,
       source, observed_at, revision, updated_at
FROM provider_quota_states;

DROP TABLE provider_quota_states;
ALTER TABLE provider_quota_states_v50 RENAME TO provider_quota_states;
CREATE INDEX ix_provider_quota_states_project
    ON provider_quota_states (project_id, provider);

PRAGMA user_version = 50;
