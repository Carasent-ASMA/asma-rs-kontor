-- Native project memory: immutable revision facts, derived FTS, frozen bindings,
-- import lineage and the one-way AgentsRoom authority cutover.

CREATE TABLE memory_items (
    project_id       TEXT NOT NULL,
    id               TEXT NOT NULL,
    aggregate_revision INTEGER NOT NULL DEFAULT 0 CHECK (aggregate_revision >= 0),
    current_revision_id TEXT NULL,
    PRIMARY KEY (project_id, id),
    FOREIGN KEY (project_id) REFERENCES projects(id)
) STRICT;

CREATE TABLE memory_revisions (
    project_id       TEXT NOT NULL,
    item_id          TEXT NOT NULL,
    id               TEXT NOT NULL,
    revision         INTEGER NOT NULL CHECK (revision > 0),
    document         TEXT NOT NULL CHECK (json_valid(document)),
    content_hash     TEXT NOT NULL,
    provenance       TEXT NOT NULL CHECK (json_valid(provenance)),
    proposed_by      TEXT NOT NULL,
    proposed_at      TEXT NOT NULL,
    supersedes_id    TEXT NULL,
    history_unavailable INTEGER NOT NULL DEFAULT 0 CHECK (history_unavailable IN (0, 1)),
    PRIMARY KEY (project_id, id),
    UNIQUE (project_id, item_id, revision),
    FOREIGN KEY (project_id, item_id) REFERENCES memory_items(project_id, id)
) STRICT;

CREATE TABLE memory_approvals (
    project_id       TEXT NOT NULL,
    revision_id      TEXT NOT NULL,
    approved_by      TEXT NOT NULL,
    approved_at      TEXT NOT NULL,
    PRIMARY KEY (project_id, revision_id),
    FOREIGN KEY (project_id, revision_id) REFERENCES memory_revisions(project_id, id)
) STRICT;

CREATE TABLE memory_tombstones (
    project_id       TEXT NOT NULL,
    item_id          TEXT NOT NULL,
    aggregate_revision INTEGER NOT NULL,
    reason           TEXT NOT NULL,
    tombstoned_by    TEXT NOT NULL,
    tombstoned_at    TEXT NOT NULL,
    PRIMARY KEY (project_id, item_id),
    FOREIGN KEY (project_id, item_id) REFERENCES memory_items(project_id, id)
) STRICT;

CREATE TABLE memory_purges (
    project_id       TEXT NOT NULL,
    item_id          TEXT NOT NULL,
    manifest_hash    TEXT NOT NULL,
    purged_by        TEXT NOT NULL,
    purged_at        TEXT NOT NULL,
    PRIMARY KEY (project_id, item_id)
) STRICT;

CREATE TABLE memory_receipts (
    id               TEXT NOT NULL PRIMARY KEY,
    project_id       TEXT NOT NULL,
    operation        TEXT NOT NULL,
    item_id          TEXT NULL,
    revision_id      TEXT NULL,
    aggregate_revision INTEGER NULL,
    result_hash      TEXT NOT NULL,
    recorded_at      TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id)
) STRICT;

CREATE TABLE memory_context_bindings (
    project_id       TEXT NOT NULL,
    run_id           TEXT NOT NULL,
    selection_cursor INTEGER NOT NULL,
    selection_spec   TEXT NOT NULL CHECK (json_valid(selection_spec)),
    ordered_revisions TEXT NOT NULL CHECK (json_valid(ordered_revisions)),
    result_hash      TEXT NOT NULL,
    bound_at         TEXT NOT NULL,
    PRIMARY KEY (project_id, run_id),
    FOREIGN KEY (project_id) REFERENCES projects(id)
) STRICT;

CREATE TABLE memory_import_manifests (
    project_id       TEXT NOT NULL,
    source           TEXT NOT NULL,
    export_hash      TEXT NOT NULL,
    manifest         TEXT NOT NULL CHECK (json_valid(manifest)),
    imported_count   INTEGER NOT NULL,
    imported_at      TEXT NOT NULL,
    PRIMARY KEY (project_id, source, export_hash),
    FOREIGN KEY (project_id) REFERENCES projects(id)
) STRICT;

CREATE TABLE memory_authority (
    singleton        INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    authority        TEXT NOT NULL CHECK (authority IN ('agentsroom', 'kontor')),
    agentsroom_writes_frozen_at TEXT NULL,
    final_export_hash TEXT NULL,
    switched_at      TEXT NULL,
    CHECK (authority = 'agentsroom' OR (agentsroom_writes_frozen_at IS NOT NULL AND final_export_hash IS NOT NULL AND switched_at IS NOT NULL))
) STRICT;
INSERT INTO memory_authority(singleton, authority) VALUES (1, 'agentsroom');

CREATE VIRTUAL TABLE memory_fts USING fts5(project_id UNINDEXED, item_id UNINDEXED, revision_id UNINDEXED, document);

CREATE TRIGGER memory_revisions_no_update BEFORE UPDATE ON memory_revisions
BEGIN SELECT RAISE(ABORT, 'memory revisions are immutable'); END;
CREATE TRIGGER memory_revisions_no_delete BEFORE DELETE ON memory_revisions
WHEN NOT EXISTS (SELECT 1 FROM memory_purges p WHERE p.project_id = OLD.project_id AND p.item_id = OLD.item_id)
BEGIN SELECT RAISE(ABORT, 'memory revisions are immutable outside explicit purge'); END;
CREATE TRIGGER memory_approvals_no_update BEFORE UPDATE ON memory_approvals
BEGIN SELECT RAISE(ABORT, 'memory approvals are immutable'); END;
CREATE TRIGGER memory_approvals_no_delete BEFORE DELETE ON memory_approvals
WHEN NOT EXISTS (SELECT 1 FROM memory_purges p JOIN memory_revisions r ON r.project_id=p.project_id AND r.item_id=p.item_id WHERE r.project_id=OLD.project_id AND r.id=OLD.revision_id)
BEGIN SELECT RAISE(ABORT, 'memory approvals are immutable outside explicit purge'); END;

PRAGMA user_version = 21;
