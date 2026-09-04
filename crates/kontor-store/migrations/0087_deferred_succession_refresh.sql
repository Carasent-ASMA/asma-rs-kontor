-- Schema v87. A deferred succession keeps its identity and initial intent, but
-- its authorizing runtime/quota observation must be refreshed when the exact
-- Wait instant arrives. This is the only state in which those evidence fields
-- may move, and the move is the same CAS that either records another Wait or
-- freezes a real successor route.

DROP TRIGGER succession_attempts_move_forward_only;
CREATE TRIGGER succession_attempts_move_forward_only
BEFORE UPDATE OF state ON succession_attempts
WHEN NOT (
    (OLD.state = 'deferred' AND NEW.state IN ('deferred', 'planned', 'refused'))
    OR (OLD.state = 'planned' AND NEW.state IN ('predecessor_retired', 'refused'))
    OR (OLD.state = 'predecessor_retired' AND NEW.state IN ('successor_observed', 'refused'))
    OR (OLD.state = 'successor_observed' AND NEW.state IN ('confirmed', 'refused'))
)
BEGIN
    SELECT RAISE(ABORT, 'a succession attempt may only move forward');
END;

DROP TRIGGER succession_attempts_immutable_decision;
CREATE TRIGGER succession_attempts_immutable_decision
BEFORE UPDATE ON succession_attempts
WHEN OLD.project_id <> NEW.project_id
  OR OLD.task_id <> NEW.task_id
  OR OLD.team_run_id <> NEW.team_run_id
  OR OLD.role_key <> NEW.role_key
  OR OLD.predecessor_agent_run_id <> NEW.predecessor_agent_run_id
  OR OLD.predecessor_runtime_binding_id <> NEW.predecessor_runtime_binding_id
  OR OLD.predecessor_runtime_kind <> NEW.predecessor_runtime_kind
  OR OLD.predecessor_host <> NEW.predecessor_host
  OR OLD.predecessor_native_id <> NEW.predecessor_native_id
  OR OLD.predecessor_generation <> NEW.predecessor_generation
  OR OLD.expected_task_revision <> NEW.expected_task_revision
  OR OLD.expected_team_revision <> NEW.expected_team_revision
  OR OLD.idempotency_key <> NEW.idempotency_key
  OR OLD.intent_hash <> NEW.intent_hash
  OR OLD.created_at <> NEW.created_at
BEGIN
    SELECT RAISE(ABORT, 'a succession decision identity is immutable');
END;

CREATE TRIGGER succession_attempts_deferred_authority_refresh_only
BEFORE UPDATE OF expected_predecessor_revision, runtime_observation_cursor,
                 quota_provenance_id, quota_state_revision, quota_evidence_hash,
                 quota_provider, deferred_until, successor_model_rung,
                 successor_model_rung_hash, successor_account_profile_id,
                 successor_planned_at, state, revision, updated_at
ON succession_attempts
WHEN (
    OLD.expected_predecessor_revision <> NEW.expected_predecessor_revision
    OR OLD.runtime_observation_cursor <> NEW.runtime_observation_cursor
    OR OLD.quota_provenance_id <> NEW.quota_provenance_id
    OR OLD.quota_state_revision <> NEW.quota_state_revision
    OR OLD.quota_evidence_hash <> NEW.quota_evidence_hash
    OR OLD.quota_provider <> NEW.quota_provider
    OR OLD.deferred_until IS NOT NEW.deferred_until
    OR OLD.successor_model_rung IS NOT NEW.successor_model_rung
    OR OLD.successor_model_rung_hash IS NOT NEW.successor_model_rung_hash
    OR OLD.successor_account_profile_id IS NOT NEW.successor_account_profile_id
    OR OLD.successor_planned_at IS NOT NEW.successor_planned_at
    OR (OLD.state = 'deferred' AND NEW.state IN ('deferred', 'planned'))
)
AND NOT (
    OLD.state = 'deferred'
    AND NEW.state IN ('deferred', 'planned')
    AND NEW.revision = OLD.revision + 1
    AND NEW.updated_at >= OLD.deferred_until
    AND NEW.runtime_observation_cursor > OLD.runtime_observation_cursor
    AND NEW.quota_provenance_id <> OLD.quota_provenance_id
    AND (
        (NEW.state = 'deferred'
         AND NEW.deferred_until > NEW.updated_at
         AND NEW.successor_model_rung IS NULL
         AND NEW.successor_model_rung_hash IS NULL
         AND NEW.successor_account_profile_id IS NULL
         AND NEW.successor_planned_at IS NULL)
        OR
        (NEW.state = 'planned'
         AND NEW.deferred_until IS NULL
         AND NEW.successor_model_rung IS NOT NULL
         AND NEW.successor_model_rung_hash IS NOT NULL
         AND NEW.successor_account_profile_id IS NOT NULL
         AND NEW.successor_planned_at = NEW.updated_at)
    )
)
BEGIN
    SELECT RAISE(ABORT, 'deferred succession authority may refresh only when due');
END;

PRAGMA user_version = 87;
