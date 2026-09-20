-- Addressable operator recovery of an artifact claimed by an exact settled turn.
-- No labels are backfilled and no historical runtime proof is invented.
CREATE TABLE artifact_producer_submissions (
    evidence_id TEXT NOT NULL PRIMARY KEY REFERENCES artifact_evidence(id),
    project_id TEXT NOT NULL REFERENCES projects(id),
    task_id TEXT NOT NULL,
    role_turn_id TEXT NOT NULL REFERENCES role_turns(id),
    idempotency_key TEXT NOT NULL UNIQUE,
    request_hash TEXT NOT NULL CHECK(length(request_hash) = 64),
    authority_tier TEXT NOT NULL CHECK(authority_tier IN ('operator', 'admin')),
    provenance TEXT NOT NULL CHECK(provenance = 'operator_recovered_git_blob'),
    turn_proof_class TEXT NOT NULL CHECK(turn_proof_class IN ('runtime_proved', 'legacy_or_attested')),
    response TEXT NOT NULL CHECK(json_valid(response)),
    FOREIGN KEY(project_id, task_id) REFERENCES tasks(project_id, id)
) STRICT;
CREATE TRIGGER artifact_producer_submissions_no_update
BEFORE UPDATE ON artifact_producer_submissions BEGIN SELECT RAISE(ABORT, 'artifact submissions are immutable'); END;
CREATE TRIGGER artifact_producer_submissions_no_delete
BEFORE DELETE ON artifact_producer_submissions BEGIN SELECT RAISE(ABORT, 'artifact submissions are immutable'); END;
PRAGMA user_version = 111;
