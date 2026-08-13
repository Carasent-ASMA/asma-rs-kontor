-- ===========================================================================
-- Schema v16. Where a task's work actually happens.
--
-- Nothing in the model carried this. Seat admission needed a workspace root to
-- prepare, had no field to read one from, and synthesized `/w/<task_id>` — a
-- path that names a real task and no real directory. A runtime that verifies
-- placement refuses it, and one that does not would have run the work in a
-- directory nobody chose. Either way the control plane was deciding where code
-- gets edited by string formatting.
--
-- It is a row per task rather than a column on `tasks` for the reason
-- `task_account_selections` is: it is a *pre-run placement decision*, settable
-- and correctable before a run snapshots it, and `tasks` is the aggregate whose
-- revision guards the lifecycle rather than the placement. Keeping it separate
-- also leaves `NewTask` — a domain type two dozen call sites construct — alone,
-- so nothing that merely creates a task has to have an opinion about worktrees.
--
-- The path is stored as text and validated in Rust before it arrives:
-- `WorkspaceRoot::parse` refuses a relative path, `.`, `..` and repeated
-- separators, and SQL cannot state those rules. The `GLOB` here is the part SQL
-- *can* state — it must be absolute — so a row written by anything other than
-- the application still cannot name a relative place.
--
-- Replaceable, like the account selection and for the same reason: correcting
-- where a task will run is a decision a Lead may revisit until a run has
-- snapshotted it. What a run *did* use is the workspace binding the runtime
-- issued, which lives with the run and is not this row.
-- ===========================================================================

CREATE TABLE task_worktrees (
    project_id   TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id      TEXT NOT NULL,
    worktree     TEXT NOT NULL CHECK (
                     length(worktree) BETWEEN 1 AND 512
                     AND worktree GLOB '/*'
                 ),
    declared_at  TEXT NOT NULL
                     CHECK (declared_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, task_id),
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT
) STRICT;

PRAGMA user_version = 16;
