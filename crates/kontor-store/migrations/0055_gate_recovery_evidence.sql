-- ===========================================================================
-- Schema v55. Gate recovery session evidence.
--
-- A gate verdict on the recovery path is recorded on behalf of an evaluator
-- seat whose runtime is closed or unreachable. The citation names the
-- evaluator's own session record (the agent run) and the digest of the verdict
-- content that session rendered, and both halves are persisted on the
-- evaluation row so the citation is durable, append-only evidence.
--
-- The two columns are nullable with no default and no backfill: a v1..v54 row
-- was never recorded under a session citation, and inventing one would
-- fabricate the very evidence the recovery path exists to carry. The daemon
-- writes them together or not at all; `session_evidence_agent_run` is also
-- covered by the v3 trigger that proves an `agent_run_id` belongs to the
-- project, because the store writes it into that column as well.
-- ===========================================================================

ALTER TABLE task_gate_evaluations ADD COLUMN session_evidence_agent_run TEXT NULL
    CHECK (session_evidence_agent_run IS NULL
           OR (length(session_evidence_agent_run) = 36
               AND session_evidence_agent_run NOT GLOB '*[^0-9a-f-]*'));

ALTER TABLE task_gate_evaluations ADD COLUMN session_evidence_digest TEXT NULL
    CHECK (session_evidence_digest IS NULL
           OR (length(session_evidence_digest) = 64
               AND session_evidence_digest NOT GLOB '*[^0-9a-f]*'));

PRAGMA user_version = 55;
