-- Jira issue identity is two-part. The canonical issue key is mutable — Jira
-- rewrites it whenever an issue moves project — while the REST top-level `id`
-- is immutable for the life of the issue. Retaining the id is the only thing
-- that lets a confirmed key change on the same issue be told apart from an
-- attempted rebind onto a different issue; the readback hash cannot, because
-- the key is itself part of the hashed document.
--
-- The column is nullable on purpose. Rows confirmed before this migration were
-- observed without an id and none is invented for them: a synthesized id would
-- be indistinguishable from proof. Such a row stays fail-closed for key changes
-- until a supported Jira readback establishes its id.
ALTER TABLE jira_epic_bindings
    ADD COLUMN external_issue_id TEXT
    CHECK (external_issue_id IS NULL OR length(external_issue_id) BETWEEN 1 AND 64);

ALTER TABLE jira_task_binding_confirmations
    ADD COLUMN external_issue_id TEXT
    CHECK (external_issue_id IS NULL OR length(external_issue_id) BETWEEN 1 AND 64);

-- One immutable Jira issue names at most one subject per project in each
-- ledger. SQLite holds NULLs distinct in a unique index, so pre-migration rows
-- coexist without claiming identity with one another. The cross-ledger case
-- (an issue confirmed for both an epic and a task) has no single-table
-- expression and stays an application guard inside the confirming transaction.
CREATE UNIQUE INDEX jira_epic_bindings_immutable_issue
    ON jira_epic_bindings (project_id, external_issue_id);

CREATE UNIQUE INDEX jira_task_binding_confirmations_immutable_issue
    ON jira_task_binding_confirmations (project_id, external_issue_id);

-- Once observed, an immutable id never changes. Establishing the id of a
-- legacy NULL row is permitted; overwriting an established one is not.
CREATE TRIGGER jira_epic_binding_issue_id_immutable BEFORE UPDATE ON jira_epic_bindings
WHEN OLD.external_issue_id IS NOT NULL AND NEW.external_issue_id IS NOT OLD.external_issue_id
BEGIN SELECT RAISE(ABORT, 'the immutable Jira issue id of a confirmed epic binding never changes'); END;

CREATE TRIGGER jira_task_binding_issue_id_immutable BEFORE UPDATE ON jira_task_binding_confirmations
WHEN OLD.external_issue_id IS NOT NULL AND NEW.external_issue_id IS NOT OLD.external_issue_id
BEGIN SELECT RAISE(ABORT, 'the immutable Jira issue id of a confirmed task binding never changes'); END;

-- v81 froze the whole canonical task-link row because, at that time, the Jira
-- key *was* the task's external identity, so "the key never changes" and "the
-- identity never changes" were the same sentence. This ticket separates them:
-- identity is now the immutable issue id, and the key is an attribute of it
-- that Jira itself rewrites when an issue moves project. The invariant v81
-- meant to protect is kept in full; only the conflation is removed.
DROP TRIGGER canonical_jira_task_links_immutable;

-- What a canonical link names never changes: not its project, not its task,
-- not its link. Deletes stay refused by canonical_jira_task_links_permanent.
CREATE TRIGGER canonical_jira_task_links_identity_immutable
BEFORE UPDATE ON canonical_jira_task_links
WHEN OLD.project_id <> NEW.project_id
  OR OLD.task_id <> NEW.task_id
  OR OLD.link_id <> NEW.link_id
BEGIN
    SELECT RAISE(ABORT, 'canonical Jira task link identity is immutable');
END;

-- A key change is admitted only as the tail of a proven same-issue rename. It
-- requires the confirmation ledger to already hold an immutable issue id for
-- this link — a legacy row without one stays fail-closed — and requires the
-- link ledger to already name the new key, which it does only inside the
-- confirming transaction. A direct SQL edit of this table alone satisfies
-- neither and is refused.
CREATE TRIGGER canonical_jira_task_links_key_change_requires_proof
BEFORE UPDATE OF external_issue_key ON canonical_jira_task_links
WHEN NEW.external_issue_key <> OLD.external_issue_key
 AND (
     NOT EXISTS (
         SELECT 1 FROM jira_task_binding_confirmations AS confirmation
         WHERE confirmation.project_id = NEW.project_id
           AND confirmation.link_id = NEW.link_id
           AND confirmation.external_issue_id IS NOT NULL
     )
     OR NOT EXISTS (
         SELECT 1 FROM jira_links AS link
         WHERE link.project_id = NEW.project_id
           AND link.id = NEW.link_id
           AND link.external_issue_key = NEW.external_issue_key
     )
 )
BEGIN
    SELECT RAISE(ABORT, 'a canonical Jira task link key changes only on proven immutable issue identity');
END;

PRAGMA user_version = 95;
