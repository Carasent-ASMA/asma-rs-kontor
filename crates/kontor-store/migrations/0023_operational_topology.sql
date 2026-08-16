-- Generic Operational topology and catalog state. Runtime placement remains in
-- kontor-runtime and is deliberately absent from this migration.

CREATE TABLE topology_specs (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    spec_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    name TEXT NOT NULL,
    root_kind TEXT NOT NULL,
    definition TEXT NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT NOT NULL
        CHECK (length(definition_hash) = 64 AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    published_at TEXT NOT NULL,
    PRIMARY KEY (project_id, spec_id, version)
) STRICT;

CREATE TRIGGER topology_specs_are_immutable
BEFORE UPDATE ON topology_specs
BEGIN
    SELECT RAISE(ABORT, 'topology specification revisions are immutable');
END;

CREATE TRIGGER topology_specs_are_permanent
BEFORE DELETE ON topology_specs
BEGIN
    SELECT RAISE(ABORT, 'published topology specification revisions are permanent');
END;

CREATE TABLE project_topology_defaults (
    project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    spec_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    canonical_hash TEXT NOT NULL
        CHECK (length(canonical_hash) = 64 AND canonical_hash NOT GLOB '*[^0-9a-f]*'),
    selected_at TEXT NOT NULL,
    FOREIGN KEY (project_id, spec_id, version)
        REFERENCES topology_specs(project_id, spec_id, version) ON DELETE RESTRICT
) STRICT;

CREATE TABLE mini_project_topology_snapshots (
    mini_project_id TEXT PRIMARY KEY NOT NULL REFERENCES mini_projects(id) ON DELETE RESTRICT,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    spec_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    canonical_hash TEXT NOT NULL
        CHECK (length(canonical_hash) = 64 AND canonical_hash NOT GLOB '*[^0-9a-f]*'),
    pinned_at TEXT NOT NULL,
    FOREIGN KEY (project_id, spec_id, version)
        REFERENCES topology_specs(project_id, spec_id, version) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER mini_project_topology_snapshots_are_immutable
BEFORE UPDATE ON mini_project_topology_snapshots
BEGIN
    SELECT RAISE(ABORT, 'mini-project topology snapshots are immutable');
END;

CREATE TRIGGER mini_project_topology_snapshots_are_permanent
BEFORE DELETE ON mini_project_topology_snapshots
BEGIN
    SELECT RAISE(ABORT, 'mini-project topology snapshots are permanent');
END;

CREATE TABLE role_catalog_revisions (
    catalog_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    name TEXT NOT NULL,
    definition TEXT NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT NOT NULL
        CHECK (length(definition_hash) = 64 AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    published_at TEXT NOT NULL,
    PRIMARY KEY (catalog_id, version)
) STRICT;

CREATE TRIGGER role_catalog_revisions_are_immutable
BEFORE UPDATE ON role_catalog_revisions
BEGIN
    SELECT RAISE(ABORT, 'role catalog revisions are immutable');
END;

CREATE TRIGGER role_catalog_revisions_are_permanent
BEFORE DELETE ON role_catalog_revisions
BEGIN
    SELECT RAISE(ABORT, 'published role catalog revisions are permanent');
END;

CREATE TABLE topology_nodes (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id TEXT REFERENCES mini_projects(id) ON DELETE RESTRICT,
    spec_id TEXT NOT NULL,
    spec_version INTEGER NOT NULL CHECK (spec_version > 0),
    spec_hash TEXT NOT NULL
        CHECK (length(spec_hash) = 64 AND spec_hash NOT GLOB '*[^0-9a-f]*'),
    kind TEXT NOT NULL,
    parent_id TEXT REFERENCES topology_nodes(id) ON DELETE RESTRICT,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('active', 'retired', 'archived')),
    placement TEXT NOT NULL CHECK (placement IN ('unbound', 'bound', 'drifted', 'placement_blocked')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (project_id, spec_id, spec_version)
        REFERENCES topology_specs(project_id, spec_id, version) ON DELETE RESTRICT,
    CHECK (id <> parent_id)
) STRICT;

CREATE INDEX ix_topology_nodes_scope
    ON topology_nodes(project_id, mini_project_id, parent_id, kind);

CREATE TABLE seat_bindings (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    topology_node_id TEXT NOT NULL REFERENCES topology_nodes(id) ON DELETE RESTRICT,
    role_slot_id TEXT NOT NULL,
    role_catalog_id TEXT NOT NULL,
    role_catalog_version INTEGER NOT NULL CHECK (role_catalog_version > 0),
    role_code TEXT NOT NULL,
    standard_title TEXT NOT NULL,
    custom_display_name TEXT,
    task_id TEXT REFERENCES tasks(id) ON DELETE RESTRICT,
    team_run_id TEXT REFERENCES team_runs(id) ON DELETE RESTRICT,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('active', 'retired', 'archived')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (role_catalog_id, role_catalog_version)
        REFERENCES role_catalog_revisions(catalog_id, version) ON DELETE RESTRICT
) STRICT;

CREATE UNIQUE INDEX ux_active_seat_binding_key
    ON seat_bindings(topology_node_id, role_slot_id)
    WHERE lifecycle = 'active';

CREATE INDEX ix_seat_bindings_project_node
    ON seat_bindings(project_id, topology_node_id, created_at, id);

CREATE TABLE adaptive_admission_state (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id TEXT PRIMARY KEY NOT NULL REFERENCES mini_projects(id) ON DELETE RESTRICT,
    current_window INTEGER NOT NULL CHECK (current_window > 0),
    clean_observation_streak INTEGER NOT NULL CHECK (clean_observation_streak BETWEEN 0 AND 1),
    last_observation_id TEXT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL
) STRICT;

PRAGMA user_version = 23;
