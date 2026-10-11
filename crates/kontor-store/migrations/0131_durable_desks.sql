-- Durable desks: project-level native projects that no epic owns.
--
-- A desk is two unscoped topology nodes — the desk itself, directly below the
-- project root, and the one workspace placed inside it — plus this row, which is
-- the only place the desk's key and its Team Definition pin live.
--
-- The row is written before either node and carries the ids they will use, so a
-- retry that lost its answer reconciles the same nodes instead of placing a
-- second desk. That is the Quick-session precedent, and for the same reason the
-- node columns are plain TEXT without a foreign key: the row must be writable
-- while the nodes it names do not exist yet.
--
-- There is deliberately no epic column. Nothing that closes, archives or
-- migrates an epic can select a desk through this table, and the row records
-- membership only: which epic or pull request an occupant of the desk works
-- for is a separate binding, never a property of the desk.
CREATE TABLE desks (
    project_id               TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    desk_key                 TEXT    NOT NULL
                                     CHECK (length(desk_key) BETWEEN 1 AND 128
                                            AND desk_key NOT GLOB '*[^a-z0-9._-]*'),
    topology_node_id         TEXT    NOT NULL CHECK (length(topology_node_id) = 36),
    workspace_node_id        TEXT    NOT NULL CHECK (length(workspace_node_id) = 36),
    -- The exact definition revision the desk was created under. Its names and
    -- its kinds are read from these bytes for the desk's whole life: selecting
    -- another project default later never renames or moves an existing desk.
    team_definition_id       TEXT    NOT NULL,
    team_definition_version  INTEGER NOT NULL CHECK (team_definition_version > 0),
    team_definition_hash     TEXT    NOT NULL
                                     CHECK (length(team_definition_hash) = 64
                                            AND team_definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at               TEXT    NOT NULL
                                     CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, desk_key),
    UNIQUE (project_id, topology_node_id),
    UNIQUE (project_id, workspace_node_id),
    CHECK (topology_node_id <> workspace_node_id),
    FOREIGN KEY (project_id, team_definition_id, team_definition_version)
        REFERENCES team_definitions (project_id, definition_id, version) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER desks_are_immutable
BEFORE UPDATE ON desks
BEGIN
    SELECT RAISE(ABORT, 'a desk keeps its key, nodes and Team Definition pin');
END;

CREATE TRIGGER desks_are_permanent
BEFORE DELETE ON desks
BEGIN
    SELECT RAISE(ABORT, 'a desk is never deleted');
END;

PRAGMA user_version = 131;
