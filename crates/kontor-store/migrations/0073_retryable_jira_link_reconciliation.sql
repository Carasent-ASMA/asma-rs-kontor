-- Jira materialization is a durable attempt log. A failed create attempt may
-- be followed by an explicitly linked, externally read-back recovery attempt
-- for the same epic/task. The original table-level uniqueness silently
-- discarded those link items because they intentionally reuse the stable
-- marker and scope identity.
--
-- Keep create markers unique: they are the connector's duplicate-creation
-- fence. Link attempts are non-creating and are instead fenced by their batch
-- preview plus the final confirmed binding tables.

ALTER TABLE jira_materialization_items RENAME TO jira_materialization_items_v72;

CREATE TABLE jira_materialization_items (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    batch_id TEXT NOT NULL REFERENCES jira_materialization_batches (id) ON DELETE RESTRICT,
    project_id TEXT NOT NULL,
    epic_id TEXT NOT NULL,
    task_id TEXT NULL REFERENCES tasks (id) ON DELETE RESTRICT,
    link_id TEXT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    item_kind TEXT NOT NULL CHECK (item_kind IN ('epic', 'task')),
    intent_kind TEXT NOT NULL CHECK (intent_kind IN ('create', 'link')),
    requested_key TEXT NULL,
    marker TEXT NOT NULL CHECK (length(marker) BETWEEN 1 AND 255),
    status TEXT NOT NULL CHECK (status IN ('planned', 'confirmed', 'conflict')),
    confirmed_key TEXT NULL,
    readback_hash TEXT NULL CHECK (readback_hash IS NULL OR length(readback_hash) = 64),
    confirmed_at TEXT NULL,
    UNIQUE (batch_id, ordinal),
    CHECK ((item_kind = 'epic' AND task_id IS NULL AND link_id IS NULL)
        OR (item_kind = 'task' AND task_id IS NOT NULL AND link_id IS NOT NULL)),
    CHECK ((intent_kind = 'create' AND requested_key IS NULL) OR (intent_kind = 'link' AND requested_key IS NOT NULL)),
    CHECK ((status = 'confirmed' AND confirmed_key IS NOT NULL AND readback_hash IS NOT NULL AND confirmed_at IS NOT NULL)
        OR (status <> 'confirmed' AND confirmed_key IS NULL AND readback_hash IS NULL AND confirmed_at IS NULL)),
    FOREIGN KEY (project_id, epic_id) REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT
) STRICT;

INSERT INTO jira_materialization_items (
    id, batch_id, project_id, epic_id, task_id, link_id, ordinal,
    item_kind, intent_kind, requested_key, marker, status,
    confirmed_key, readback_hash, confirmed_at
)
SELECT
    id, batch_id, project_id, epic_id, task_id, link_id, ordinal,
    item_kind, intent_kind, requested_key, marker, status,
    confirmed_key, readback_hash, confirmed_at
FROM jira_materialization_items_v72;

DROP TABLE jira_materialization_items_v72;

CREATE UNIQUE INDEX jira_materialization_unique_create_marker
    ON jira_materialization_items (marker)
    WHERE intent_kind = 'create';

CREATE INDEX jira_materialization_items_scope
    ON jira_materialization_items (project_id, epic_id, task_id, status);

PRAGMA user_version = 73;
