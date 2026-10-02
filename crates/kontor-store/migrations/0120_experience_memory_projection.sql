-- ASMA-8156: additive typed eligibility cache, immutable projection identities
-- and recall metadata. No duplicate experience payload survives explicit purge.
CREATE TABLE memory_experience_eligibility (
    project_id TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    confidence TEXT NOT NULL CHECK(confidence IN ('observed','inferred')),
    projection_policy TEXT NOT NULL CHECK(projection_policy IN ('local_only','provider_eligible')),
    PRIMARY KEY(project_id,revision_id),
    FOREIGN KEY(project_id,revision_id) REFERENCES memory_revisions(project_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE memory_projection_snapshots (
    project_id TEXT NOT NULL REFERENCES projects(id),
    memory_cursor INTEGER NOT NULL CHECK(memory_cursor >= 0),
    dataset TEXT NOT NULL,
    digest TEXT NOT NULL,
    identities TEXT NOT NULL CHECK(json_valid(identities)),
    created_at TEXT NOT NULL,
    PRIMARY KEY(project_id,memory_cursor)
) STRICT;
CREATE TABLE memory_projection_active (
    project_id TEXT PRIMARY KEY REFERENCES projects(id),
    memory_cursor INTEGER NOT NULL,
    generation INTEGER NOT NULL CHECK(generation > 0),
    FOREIGN KEY(project_id,memory_cursor) REFERENCES memory_projection_snapshots(project_id,memory_cursor)
) STRICT;
CREATE TABLE memory_recall_metadata (
    project_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    metadata TEXT NOT NULL CHECK(json_valid(metadata)),
    PRIMARY KEY(project_id,run_id),
    FOREIGN KEY(project_id,run_id) REFERENCES memory_context_bindings(project_id,run_id)
) STRICT;
CREATE TRIGGER memory_projection_snapshots_no_update BEFORE UPDATE ON memory_projection_snapshots
BEGIN SELECT RAISE(ABORT,'memory projection snapshots are immutable'); END;
CREATE TRIGGER memory_projection_snapshots_no_delete BEFORE DELETE ON memory_projection_snapshots
BEGIN SELECT RAISE(ABORT,'memory projection snapshots are immutable'); END;
CREATE TRIGGER memory_recall_metadata_no_update BEFORE UPDATE ON memory_recall_metadata
BEGIN SELECT RAISE(ABORT,'memory recall metadata is immutable'); END;
CREATE TRIGGER memory_recall_metadata_no_delete BEFORE DELETE ON memory_recall_metadata
BEGIN SELECT RAISE(ABORT,'memory recall metadata is immutable'); END;
CREATE TABLE memory_experience_proposals (
    project_id TEXT NOT NULL REFERENCES projects(id),
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    revision_id TEXT NOT NULL,
    receipt TEXT NOT NULL CHECK(json_valid(receipt)),
    PRIMARY KEY(project_id,idempotency_key)
) STRICT;
CREATE TRIGGER memory_experience_proposals_no_update BEFORE UPDATE ON memory_experience_proposals
BEGIN SELECT RAISE(ABORT,'experience proposal keys are immutable'); END;
CREATE TRIGGER memory_experience_proposals_no_delete BEFORE DELETE ON memory_experience_proposals
BEGIN SELECT RAISE(ABORT,'experience proposal keys are immutable'); END;
CREATE TABLE memory_recall_keys (
    project_id TEXT NOT NULL REFERENCES projects(id),
    idempotency_key TEXT NOT NULL,
    run_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    PRIMARY KEY(project_id,idempotency_key),
    FOREIGN KEY(project_id,run_id) REFERENCES memory_context_bindings(project_id,run_id)
) STRICT;
CREATE TRIGGER memory_recall_keys_no_update BEFORE UPDATE ON memory_recall_keys
BEGIN SELECT RAISE(ABORT,'memory recall keys are immutable'); END;
CREATE TRIGGER memory_recall_keys_no_delete BEFORE DELETE ON memory_recall_keys
BEGIN SELECT RAISE(ABORT,'memory recall keys are immutable'); END;
PRAGMA user_version = 120;
