-- The bounded task reopen.
--
-- v1 made every terminal task permanently unwritable, which was right while
-- nothing could reopen one. The lifecycle surface advertises `reopen_task` and
-- the domain now has a rule for it — one source state, one target state, and a
-- durable command receipt — so the row has to be able to move that far, and no
-- further.
--
-- What is given up is exactly `done -> ready`. A `failed` or `cancelled` task
-- stays immutable: those are outcomes rather than claims, and the honest answer to
-- one is a successor task. Nothing else about a reopened task may change either,
-- which the second trigger enforces: a reopen that renamed the task or moved it to
-- another epic would be a rewrite wearing a smaller word.
--
-- Nothing about the *history* is given up. Gate evaluations stay append-only,
-- workflow snapshots stay immutable, closed runs stay closed, and the reopen is
-- recorded in `command_receipts` under `resume_task` with its canonical intent —
-- which is where every other decision in this control plane is audited.
DROP TRIGGER tasks_terminal_immutable;

CREATE TRIGGER tasks_terminal_immutable BEFORE UPDATE ON tasks
WHEN OLD.state IN ('done', 'failed', 'cancelled')
 AND NOT (OLD.state = 'done' AND NEW.state = 'ready')
BEGIN SELECT RAISE(ABORT, 'a terminal task is immutable'); END;

CREATE TRIGGER tasks_reopen_changes_only_the_state BEFORE UPDATE ON tasks
WHEN OLD.state = 'done' AND NEW.state = 'ready'
 AND (OLD.project_id <> NEW.project_id
   OR IFNULL(OLD.mini_project_id, '') <> IFNULL(NEW.mini_project_id, '')
   OR OLD.title <> NEW.title
   OR IFNULL(OLD.module_key, '') <> IFNULL(NEW.module_key, '')
   OR OLD.created_at <> NEW.created_at)
BEGIN SELECT RAISE(ABORT, 'reopening a task changes its state and nothing else'); END;

PRAGMA user_version = 31;
