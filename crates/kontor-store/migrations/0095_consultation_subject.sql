-- The advised or debated subject of one consultation (ASMA-8117).
--
-- A consultation is asked either about one ticket or about the epic as a
-- whole, and its ASW/CSW name has to render that subject's confirmed Jira key.
-- Until now the request's `task_id` was used for authorization and folded into
-- the semantic identity hash, then discarded: `topology_nodes.task_id` cannot
-- carry it, because `ux_topology_node_task` reserves that column for the one
-- active delivery workspace per task and a consultation about a task would
-- collide with the task's own TSW. So the subject belongs on the run.
--
-- Two columns rather than one nullable task, because a NULL task has to mean
-- two different things and only one of them may render a name:
--
--   subject_kind = 'epic'  — the epic itself is the subject;
--   subject_kind = 'task'  — subject_task_id is the exact advised ticket;
--   subject_kind IS NULL   — a run invoked before this migration, whose
--                            subject was never recorded and cannot be
--                            recovered. Rendering a subject-bearing token for
--                            one of these fails closed instead of quietly
--                            substituting the containing epic.
--
-- No backfill is possible or attempted: the discarded value survives nowhere,
-- and inventing 'epic' for historical rows would assert something this realm
-- never observed.
ALTER TABLE consultation_runs ADD COLUMN subject_kind TEXT NULL
    CHECK (subject_kind IS NULL OR subject_kind IN ('epic', 'task'));

-- The pairing is exhaustive and null-safe on purpose. SQLite treats a CHECK
-- that evaluates to NULL as success, so any test written with `=` against a
-- nullable column silently admits the rows it looks like it forbids: with `=`,
-- `(NULL, <task>)` evaluates to NULL and is stored. `IS` is null-safe
-- comparison — `NULL IS 'task'` is false, not NULL — so this expression is
-- always true or false, and only the three legal pairings are true.
ALTER TABLE consultation_runs ADD COLUMN subject_task_id TEXT NULL
    REFERENCES tasks(id) ON DELETE RESTRICT
    CHECK ((subject_kind IS NULL AND subject_task_id IS NULL)
           OR (subject_kind IS 'epic' AND subject_task_id IS NULL)
           OR (subject_kind IS 'task' AND subject_task_id IS NOT NULL));

-- The subject is a frozen semantic input like every other invocation fact: it
-- is decided once, before the first native effect, and a later write may not
-- move it. Recording a subject onto a historical run is equally refused, so an
-- unrecoverable NULL stays visibly unrecoverable.
-- The subject is a frozen semantic input like every other invocation fact: it
-- is decided once, before the first native effect, and a later write may not
-- move it. Recording a subject onto a historical run is equally refused, so an
-- unrecoverable NULL stays visibly unrecoverable.
--
-- The rest of this trigger is the exact schema-v92 definition, reproduced
-- unchanged. v92 narrowed the settled-run rule so the v91 legacy-topic
-- correction may still move a settled run's topic, semantic identity, revision
-- and timestamp under the exact recorded correction command; recreating the
-- trigger from any earlier body would silently withdraw that recovery path.
-- Only the two subject predicates below are added, and `IS NOT` is used so a
-- NULL on either side compares as a difference rather than as unknown.
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
  OR OLD.subject_kind IS NOT NEW.subject_kind
  OR OLD.subject_task_id IS NOT NEW.subject_task_id
BEGIN
    SELECT RAISE(ABORT, 'a consultation run cannot rewrite frozen input or settled evidence');
END;

PRAGMA user_version = 95;
