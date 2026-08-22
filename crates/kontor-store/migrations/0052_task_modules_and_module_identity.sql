-- Schema v52. Every module a task changes is a lock, and a live dotted
-- holdout of the same module is still that lock.
--
-- Numbered 0052 because OP-13 PR 84 owns
-- `0051_provider_quota_headroom.sql`. This script stays v52.
--
-- Two facts this generation records, and why they are one migration:
--
--  1. `tasks.module_key` is still the primary module. A task that also changes
--     `editor/asma-app-editor` had nowhere to say so, so admission took one
--     lease and left the rest of the checkout unlocked. `task_modules` holds
--     those additional keys. The primary is not copied here: one name in two
--     tables would be two facts that can disagree. The set is immutable once
--     written — the same promise `tasks.module_key` already makes — and there
--     is no backfill. Live dotted QNR holdouts stay on their current rows.
--
--  2. Exact `resource_key` equality is not module identity. OP-15 made `/` the
--     canonical spelling and left four ACTIVE leases on the pre-OP-15 dotted
--     surrogate (`shared.asma-core-helpers`, `editor.asma-bunjs-editor` twice,
--     `editor.asma-app-editor`). A slash admission of the same module would
--     insert a second lease because the unique indexes and
--     `resource_leases_isolation_exclusive` compare bytes. The new trigger
--     fires only for `lease_kind` module (or the NULL v1 module rows) and only
--     when the keys *differ* but `replace(resource_key, '/', '.')` matches —
--     the same identity `ModuleKey::contention_identity` uses. Worktree leases
--     are not this problem even when a filesystem path contains `.`.
--
-- The abort message is the one the existing trigger already uses, so a caller
-- that never came through `admit_candidate` still cannot tell the two rules
-- apart, and a test that asserts the message does not have to learn a third
-- spelling.

CREATE TABLE task_modules (
    project_id  TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id     TEXT NOT NULL,
    module_key  TEXT NOT NULL CHECK (length(module_key) BETWEEN 1 AND 128),
    declared_at TEXT NOT NULL
                     CHECK (declared_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, task_id, module_key),
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER task_modules_no_update BEFORE UPDATE ON task_modules
BEGIN SELECT RAISE(ABORT, 'task_modules is immutable'); END;

CREATE TRIGGER task_modules_no_delete BEFORE DELETE ON task_modules
BEGIN SELECT RAISE(ABORT, 'task_modules is immutable'); END;

CREATE TRIGGER resource_leases_module_identity_exclusive BEFORE INSERT ON resource_leases
WHEN (NEW.lease_kind IS NULL OR NEW.lease_kind = 'module')
 AND EXISTS (
    SELECT 1 FROM resource_leases
    WHERE released_at IS NULL
      AND expired_at IS NULL
      AND (lease_kind IS NULL OR lease_kind = 'module')
      AND resource_key <> NEW.resource_key
      AND replace(resource_key, '/', '.') = replace(NEW.resource_key, '/', '.')
      AND (worktree_key IS NULL
           OR NEW.worktree_key IS NULL
           OR worktree_key = NEW.worktree_key)
 )
BEGIN SELECT RAISE(ABORT, 'this resource is already claimed by an active lease'); END;

PRAGMA user_version = 52;
