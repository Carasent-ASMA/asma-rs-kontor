-- ===========================================================================
-- Schema v89. Publication attestations (ASMA-8101).
--
-- One row per judged publication: the branch, commit and pull request a
-- producer observed, the epic/task binding Kontor resolved for the branch key,
-- and the typed decision with its stable reason codes. Accepted and refused
-- publications are both recorded — a refusal is evidence that the rail held.
--
-- The caller's idempotency key is unique per project: replaying it returns the
-- original row, and a different publication under a used key is a conflict.
-- The binding columns are nullable because a refused publication may name a
-- key Kontor never confirmed, and that refusal is exactly what must be kept.
-- ===========================================================================

CREATE TABLE publication_attestations (
    id               TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    project_id       TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id  TEXT NULL CHECK (mini_project_id IS NULL OR length(mini_project_id) = 36),
    task_id          TEXT NULL CHECK (task_id IS NULL OR length(task_id) = 36),
    repository       TEXT NOT NULL CHECK (length(repository) BETWEEN 3 AND 512),
    base_branch      TEXT NOT NULL CHECK (length(base_branch) BETWEEN 1 AND 512),
    head_branch      TEXT NOT NULL CHECK (length(head_branch) BETWEEN 1 AND 512),
    head_sha         TEXT NOT NULL CHECK (length(head_sha) = 40 AND head_sha NOT GLOB '*[^0-9a-f]*'),
    pull_request     INTEGER NULL CHECK (pull_request IS NULL OR pull_request >= 1),
    title            TEXT NULL CHECK (title IS NULL OR length(title) BETWEEN 1 AND 512),
    accepted         INTEGER NOT NULL CHECK (accepted IN (0, 1)),
    reasons          TEXT NOT NULL,
    policy_revision  INTEGER NOT NULL CHECK (policy_revision >= 1),
    idempotency_key  TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    recorded_at      TEXT NOT NULL
                         CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, idempotency_key)
) STRICT;

CREATE INDEX publication_attestations_by_head
    ON publication_attestations (project_id, repository, head_sha);

PRAGMA user_version = 89;
