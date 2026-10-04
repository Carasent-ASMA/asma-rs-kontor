-- Preserve unknown producer accounts as NULL, never a synthetic account.
-- Existing evidence, references, identities and immutable protection survive.
CREATE TABLE artifact_evidence_v113 (
    id               TEXT NOT NULL PRIMARY KEY
                          CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id       TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id          TEXT NOT NULL,
    workflow_id      TEXT NOT NULL,
    agent_run_id     TEXT NULL,
    -- The artifact contract key the pinned profile declares. Open data: this
    -- schema never interprets it.
    artifact_key     TEXT NOT NULL CHECK (length(artifact_key) BETWEEN 1 AND 128),
    locator          TEXT NOT NULL CHECK (json_valid(locator)),
    locator_hash     TEXT NOT NULL
                          CHECK (length(locator_hash) = 64 AND locator_hash NOT GLOB '*[^0-9a-f]*'),
    producer_role    TEXT NOT NULL CHECK (length(producer_role) BETWEEN 1 AND 128),
    producer_account TEXT NULL,
    recorded_at      TEXT NOT NULL
                          CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, workflow_id)
        REFERENCES task_workflows (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, producer_account)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;
INSERT INTO artifact_evidence_v113 SELECT * FROM artifact_evidence;
DROP TABLE artifact_evidence;
ALTER TABLE artifact_evidence_v113 RENAME TO artifact_evidence;
CREATE INDEX ix_artifact_evidence_workflow ON artifact_evidence(project_id, workflow_id, artifact_key);

CREATE TRIGGER artifact_evidence_no_update BEFORE UPDATE ON artifact_evidence
BEGIN SELECT RAISE(ABORT, 'artifact evidence is immutable'); END;
CREATE TRIGGER artifact_evidence_no_delete BEFORE DELETE ON artifact_evidence
BEGIN SELECT RAISE(ABORT, 'artifact evidence is immutable'); END;

-- Unknown is allowed only when the exact immutable source turn carries native
-- proof and its original binding agrees. Neither an operator label nor a
-- historical proofless claim can invent a producer identity.
CREATE TRIGGER artifact_evidence_unknown_account_requires_native_proof
BEFORE INSERT ON artifact_evidence WHEN NEW.producer_account IS NULL
BEGIN
    SELECT CASE WHEN NOT EXISTS (
        SELECT 1 FROM role_turns turn
        JOIN agent_runs run ON run.project_id = turn.project_id AND run.id = turn.agent_run_id AND run.team_run_id = turn.team_run_id
        JOIN runtime_bindings binding ON binding.project_id = run.project_id AND binding.agent_run_id = run.id AND binding.generation = turn.binding_generation
        JOIN task_workflows workflow ON workflow.project_id = turn.project_id AND workflow.task_id = turn.task_id AND workflow.id = NEW.workflow_id AND workflow.active = 1
        WHERE turn.id = json_extract(NEW.locator, '$.role_turn_id')
          AND turn.project_id = NEW.project_id AND turn.task_id = NEW.task_id
          AND turn.agent_run_id = NEW.agent_run_id
          AND turn.role_slot_id = NEW.producer_role AND run.role_key = NEW.producer_role
          AND turn.account_profile IS NULL AND run.account_profile_id IS NULL
          AND turn.settlement_kind = 'current_runtime'
          AND turn.runtime_message_id IS NOT NULL
          AND turn.message_timeline_epoch IS NOT NULL AND turn.message_timeline_sequence IS NOT NULL
          AND turn.response_timeline_epoch = turn.message_timeline_epoch
          AND turn.response_timeline_sequence > turn.message_timeline_sequence
          AND turn.runtime_observation_cursor IS NOT NULL
          AND binding.bound_at <= turn.settled_at AND turn.settled_at >= workflow.created_at
          AND json_extract(NEW.locator, '$.producer_account_attribution') = 'native_proved_unknown'
          AND json_extract(NEW.locator, '$.turn_proof_class') = 'runtime_proved'
          AND json_extract(NEW.locator, '$.source_binding.id') = binding.id
          AND json_extract(NEW.locator, '$.source_binding.native_id') = binding.native_id
          AND json_extract(NEW.locator, '$.source_binding.generation') = binding.generation
          AND EXISTS (SELECT 1 FROM json_each(turn.artifacts) claim WHERE claim.type = 'text' AND claim.value = NEW.artifact_key)
    ) THEN RAISE(ABORT, 'an unknown producer account requires the exact native-proved source turn and binding') END;
END;
PRAGMA user_version = 113;
