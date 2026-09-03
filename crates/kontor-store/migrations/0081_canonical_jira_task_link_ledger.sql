-- Schema v81. Jira has one canonical task-to-issue identity even when an old
-- database used both the historical `jira` alias and `connector.jira`.
--
-- The original `jira_links` rows remain intact. Their ids are embedded in
-- immutable observations, receipts and command intent, so deleting one of an
-- otherwise exact alias/canonical pair would erase or rewrite evidence. This
-- ledger selects the one live identity while every historical row and foreign
-- key remains available for audit and restore.

CREATE TABLE migration_0081_jira_link_guard (
    violation INTEGER NOT NULL
) STRICT;

CREATE TRIGGER migration_0081_jira_link_guard_refuses
BEFORE INSERT ON migration_0081_jira_link_guard
BEGIN
    SELECT RAISE(ABORT, 'irreconcilable historical Jira task-link identities');
END;

-- One task with two issue keys, or one issue key attached to two tasks, is not
-- an alias cleanup. Neither side is authoritative enough to choose silently.
INSERT INTO migration_0081_jira_link_guard (violation)
SELECT 1
WHERE EXISTS (
    SELECT 1
    FROM jira_links
    WHERE connector IN ('jira', 'connector.jira')
    GROUP BY project_id, task_id
    HAVING count(DISTINCT external_issue_key) > 1
)
OR EXISTS (
    SELECT 1
    FROM jira_links
    WHERE connector IN ('jira', 'connector.jira')
    GROUP BY project_id, external_issue_key
    HAVING count(DISTINCT task_id) > 1
);

DROP TRIGGER migration_0081_jira_link_guard_refuses;
DROP TABLE migration_0081_jira_link_guard;

CREATE TABLE migration_0081_open_conflict_guard (
    violation INTEGER NOT NULL
) STRICT;

CREATE TRIGGER migration_0081_open_conflict_guard_refuses
BEFORE INSERT ON migration_0081_open_conflict_guard
BEGIN
    SELECT RAISE(ABORT, 'irreconcilable historical duplicate open status conflicts');
END;

INSERT INTO migration_0081_open_conflict_guard (violation)
SELECT 1
WHERE EXISTS (
    SELECT 1
    FROM status_conflicts
    WHERE resolved_at IS NULL
    GROUP BY project_id, link_id, kind
    HAVING count(*) > 1
);

DROP TRIGGER migration_0081_open_conflict_guard_refuses;
DROP TABLE migration_0081_open_conflict_guard;

CREATE TABLE canonical_jira_task_links (
    project_id         TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id            TEXT NOT NULL,
    external_issue_key TEXT NOT NULL
                            CHECK (length(external_issue_key) BETWEEN 1 AND 256),
    link_id            TEXT NOT NULL,
    PRIMARY KEY (project_id, task_id),
    UNIQUE (project_id, external_issue_key),
    UNIQUE (project_id, link_id),
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, link_id)
        REFERENCES jira_links (project_id, id) ON DELETE RESTRICT
) STRICT;

-- Prefer the already canonical row when both spellings describe the exact same
-- task and key. If only the legacy row exists, its stable id remains canonical;
-- the repository normalizes the connector spelling when it reads that row.
INSERT INTO canonical_jira_task_links
    (project_id, task_id, external_issue_key, link_id)
SELECT link.project_id, link.task_id, link.external_issue_key, link.id
FROM jira_links AS link
WHERE link.connector = 'connector.jira'
   OR (
       link.connector = 'jira'
       AND NOT EXISTS (
           SELECT 1
           FROM jira_links AS canonical
           WHERE canonical.project_id = link.project_id
             AND canonical.task_id = link.task_id
             AND canonical.external_issue_key = link.external_issue_key
             AND canonical.connector = 'connector.jira'
       )
   );

-- New raw writes cannot recreate the legacy spelling around the repository
-- boundary. Existing rows are historical evidence and deliberately remain.
CREATE TRIGGER jira_links_require_canonical_jira_insert
BEFORE INSERT ON jira_links
WHEN NEW.connector = 'jira'
BEGIN
    SELECT RAISE(ABORT, 'new Jira links use connector.jira');
END;

CREATE TRIGGER jira_links_require_canonical_jira_update
BEFORE UPDATE OF connector ON jira_links
WHEN NEW.connector = 'jira'
BEGIN
    SELECT RAISE(ABORT, 'new Jira links use connector.jira');
END;

CREATE TRIGGER canonical_jira_task_links_immutable
BEFORE UPDATE ON canonical_jira_task_links
BEGIN
    SELECT RAISE(ABORT, 'canonical Jira task links are immutable');
END;

CREATE TRIGGER canonical_jira_task_links_permanent
BEFORE DELETE ON canonical_jira_task_links
BEGIN
    SELECT RAISE(ABORT, 'canonical Jira task links are permanent');
END;

CREATE UNIQUE INDEX ux_status_conflicts_one_open_kind
    ON status_conflicts (project_id, link_id, kind)
    WHERE resolved_at IS NULL;

PRAGMA user_version = 81;
