-- Schema v92. A settled consultation's verdict and frozen inputs remain
-- immutable, while the v91 legacy-topic correction may update only its topic,
-- server-derived semantic identity, revision and timestamp under the exact
-- synchronous correction command recorded in the same transaction.

DROP TRIGGER consultation_run_inputs_are_frozen;

CREATE TRIGGER consultation_run_inputs_are_frozen
BEFORE UPDATE ON consultation_runs
WHEN OLD.project_id <> NEW.project_id
  OR OLD.mini_project_id <> NEW.mini_project_id
  OR OLD.family <> NEW.family
  OR OLD.profile_id <> NEW.profile_id
  OR OLD.profile_version <> NEW.profile_version
  OR OLD.definition_hash <> NEW.definition_hash
  OR OLD.question <> NEW.question
  OR OLD.question_hash <> NEW.question_hash
  OR OLD.context <> NEW.context
  OR OLD.context_hash <> NEW.context_hash
  OR OLD.caller_seat_binding_id <> NEW.caller_seat_binding_id
  OR OLD.topology_node_id <> NEW.topology_node_id
  OR OLD.invoke_key <> NEW.invoke_key
  OR OLD.invoke_intent_hash <> NEW.invoke_intent_hash
  OR OLD.created_at <> NEW.created_at
  OR (
      OLD.result IS NOT NULL
      AND NOT (
          OLD.state IS NEW.state
          AND OLD.round = NEW.round
          AND OLD.result IS NEW.result
          AND OLD.result_hash IS NEW.result_hash
          AND OLD.settled_at IS NEW.settled_at
          AND OLD.topic IS NOT NEW.topic
          AND OLD.semantic_identity_hash IS NULL
          AND NEW.semantic_identity_hash IS NOT NULL
          AND NEW.revision = OLD.revision + 1
          AND OLD.updated_at <> NEW.updated_at
          AND EXISTS (
              SELECT 1
                FROM command_receipts AS receipt
               WHERE receipt.project_id = OLD.project_id
                 AND receipt.kind = 'reconcile_native_names'
                 AND receipt.execution_mode = 'local'
                 AND receipt.state = 'intent_persisted'
                 AND json_extract(receipt.intent, '$.operation') =
                     'correct_consultation_topic'
                 AND json_extract(receipt.intent, '$.committee_run_id') = OLD.run_id
                 AND json_extract(receipt.intent, '$.expected_prior_topic') = OLD.topic
                 AND json_extract(receipt.intent, '$.corrected_topic') = NEW.topic
          )
      )
  )
BEGIN
    SELECT RAISE(ABORT, 'a consultation run cannot rewrite frozen input or settled evidence');
END;

PRAGMA user_version = 92;
