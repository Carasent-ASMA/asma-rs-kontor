-- ===========================================================================
-- Schema v7. The provider account a task is pinned to, before a run exists.
--
-- `agent_runs.account_profile_id` records what a run *did* execute as, and it is
-- written when the scheduler admits the task. That is the right place for it and
-- the wrong place for a decision made before any run exists: a Lead correcting
-- the account a task will run under has nowhere to put the answer, and the
-- scheduler consequently has nothing to read.
--
-- So the selection is its own row, one per task, and it carries the profile's
-- revision alongside its id. The revision is what makes the pin *checkable*: a
-- profile whose mutable fields moved after the selection was made is a stale pin,
-- and the scheduler is entitled to notice rather than to launch against a profile
-- nobody looked at.
--
-- The row is replaceable, unlike the run's copy, precisely because it is a
-- pre-run decision. Once a run snapshots it the run's own column is the record,
-- and the application layer refuses to touch this one.
-- ===========================================================================

CREATE TABLE task_account_selections (
    project_id         TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id            TEXT    NOT NULL,
    account_profile_id TEXT    NOT NULL,
    account_revision   INTEGER NOT NULL CHECK (account_revision >= 1),
    selected_at        TEXT    NOT NULL
                               CHECK (selected_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, task_id),
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, account_profile_id)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;

PRAGMA user_version = 7;
