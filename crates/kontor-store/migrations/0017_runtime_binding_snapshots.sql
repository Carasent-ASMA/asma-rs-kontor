-- ===========================================================================
-- Schema v17. The frozen capability snapshot a binding was issued under.
--
-- `runtime_bindings` records *which* native session a run is bound to. It does
-- not record the evidence quality the binding was created at — the capability
-- set the runtime could prove at that moment, and the correlation that proves
-- the session belongs to the run — and those lived only in process memory.
--
-- So a daemon restart orphaned every live session: the binding survived, the
-- native session survived, and nothing could operate it, because the snapshot
-- needed to address it at the quality it was bound at was gone. Rebuilding one
-- from a fresh discovery is not an option — a session bound at a degraded grade
-- would come back promoted because the runtime happens to answer better today —
-- so the snapshot has to be *kept*, not recomputed.
--
-- The document is the snapshot as its own types serialize it, stored whole with
-- its digest and re-proved on read. Exploding it into columns would put a second
-- spelling of the capability model in SQL, and the two would drift.
--
-- This row is a *claim*, not authority. Restoring it hands it back to the
-- issuing runtime, which re-attests it against what it actually issued; a row
-- edited underneath the daemon fails that comparison exactly as a forged
-- in-memory snapshot does. What the row buys is that the claim still exists to
-- be re-attested after a restart.
--
-- Deleted when the binding is released, because a snapshot for a closed run is
-- not evidence of anything and would be one more thing a census has to explain.
-- ===========================================================================

CREATE TABLE runtime_binding_snapshots (
    binding_id     TEXT NOT NULL PRIMARY KEY
                        CHECK (length(binding_id) = 36 AND binding_id NOT GLOB '*[^0-9a-f-]*'),
    agent_run_id   TEXT NOT NULL
                        CHECK (length(agent_run_id) = 36 AND agent_run_id NOT GLOB '*[^0-9a-f-]*'),
    document       TEXT NOT NULL CHECK (json_valid(document)),
    document_hash  TEXT NOT NULL
                        CHECK (length(document_hash) = 64 AND document_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at    TEXT NOT NULL
                        CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

CREATE INDEX ix_runtime_binding_snapshots_run
    ON runtime_binding_snapshots (agent_run_id);

PRAGMA user_version = 17;
