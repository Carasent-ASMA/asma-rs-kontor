-- ASMA-8159: durable request binding and original activation readback, following
-- the typed memory proposal/recall receipt conventions. Tentative, unshipped
-- numbering; reconcile with external succession migrations at serialized join.
CREATE TABLE memory_projection_rebuild_keys (
    idempotency_key TEXT PRIMARY KEY CHECK(length(idempotency_key) BETWEEN 1 AND 256),
    project_id TEXT NOT NULL REFERENCES projects(id),
    request TEXT NOT NULL CHECK(json_valid(request)),
    request_hash TEXT NOT NULL CHECK(length(request_hash) = 64),
    recorded_at TEXT NOT NULL
) STRICT;
CREATE TABLE memory_projection_rebuild_results (
    idempotency_key TEXT PRIMARY KEY REFERENCES memory_projection_rebuild_keys(idempotency_key),
    result TEXT NOT NULL CHECK(json_valid(result)),
    result_hash TEXT NOT NULL CHECK(length(result_hash) = 64),
    recorded_at TEXT NOT NULL
) STRICT;
CREATE TRIGGER memory_projection_rebuild_keys_no_update BEFORE UPDATE ON memory_projection_rebuild_keys
BEGIN SELECT RAISE(ABORT,'projection rebuild keys are immutable'); END;
CREATE TRIGGER memory_projection_rebuild_keys_no_delete BEFORE DELETE ON memory_projection_rebuild_keys
BEGIN SELECT RAISE(ABORT,'projection rebuild keys are permanent'); END;
CREATE TRIGGER memory_projection_rebuild_results_no_update BEFORE UPDATE ON memory_projection_rebuild_results
BEGIN SELECT RAISE(ABORT,'projection rebuild results are immutable'); END;
CREATE TRIGGER memory_projection_rebuild_results_no_delete BEFORE DELETE ON memory_projection_rebuild_results
BEGIN SELECT RAISE(ABORT,'projection rebuild results are permanent'); END;
PRAGMA user_version = 121;
