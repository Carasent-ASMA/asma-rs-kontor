-- The durable native container binding per topology node, and the OP-REQ-039
-- attachment evidence a logical seat is concluded from.

-- One *current* native container per topology node. History is not kept here:
-- a node that is rebound has one binding, and the old native id is evidence in
-- the event stream rather than a second row that could be mistaken for a live
-- placement.
CREATE TABLE topology_node_containers (
    topology_node_id TEXT PRIMARY KEY NOT NULL
        REFERENCES topology_nodes(id) ON DELETE RESTRICT,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    container_binding_id TEXT NOT NULL,
    -- The exact native identity, kept in the four parts that make it one:
    -- a native id is only meaningful inside one generation on one host of one
    -- runtime family, and comparing any subset of these silently matches a
    -- container that a restart has already replaced.
    runtime_kind TEXT NOT NULL,
    host TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation >= 0),
    native_id TEXT NOT NULL,
    -- What the runtime said the container *is*, read back rather than assumed.
    -- A workspace where a project was expected is a placement error, and it can
    -- only be caught if the observed shape is stored beside the desired one.
    observed_kind TEXT NOT NULL CHECK (observed_kind IN ('project', 'workspace')),
    canonical_cwd TEXT,
    bound_at TEXT NOT NULL,
    -- When the binding was last confirmed against the runtime. Distinct from
    -- `bound_at` so a stale binding is visible as stale instead of looking as
    -- fresh as the day it was made.
    last_readback_at TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0)
) STRICT;

-- Two nodes claiming one native container is the collision that must be
-- refused at the moment of binding. Discovering it later means both nodes have
-- already been treated as placed.
CREATE UNIQUE INDEX ux_topology_node_container_native
    ON topology_node_containers(runtime_kind, host, generation, native_id);

CREATE INDEX ix_topology_node_containers_project
    ON topology_node_containers(project_id, topology_node_id);

-- `seat_bindings` is rebuilt rather than altered: `attach_deadline` has to be
-- NOT NULL, and SQLite can only add a NOT NULL column with a constant default.
-- A constant default would be a fabricated deadline, which is the exact failure
-- this column exists to prevent.
--
-- Nothing references `seat_bindings`, so the rebuild needs no foreign-key
-- suspension — which is just as well, since `PRAGMA foreign_keys` is a no-op
-- inside the transaction every migration runs in.
CREATE TABLE seat_bindings_v2 (
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
    -- OP-REQ-039a. Fixed when the seat is created and never recomputed: a
    -- deadline derived at read time moves whenever the grace constant changes,
    -- which retroactively forgives every seat that already failed to attach.
    attach_deadline TEXT NOT NULL,
    -- OP-REQ-039b. When the seat was last observed attached to its session.
    last_attached_at TEXT,
    -- OP-REQ-039c. When the seat last produced *observed activity*. A readback
    -- may confirm attachment; only an observed runtime event or turn position
    -- may write this, which is why it is a separate column from any generic
    -- confirmation instant.
    last_activity_at TEXT,
    -- The exact owning epic seat. Orphanhood is derived from this seat's Kontor
    -- lifecycle, never from a runtime's own parent field.
    parent_seat_binding_id TEXT REFERENCES seat_bindings_v2(id) ON DELETE RESTRICT,
    released_at TEXT,
    replaced_by_seat_binding_id TEXT REFERENCES seat_bindings_v2(id) ON DELETE RESTRICT,
    -- The runtime's self-report, stored so an escalation can quote it. It is
    -- never consulted when concluding attachment.
    runtime_reported TEXT CHECK (runtime_reported IS NULL OR runtime_reported IN
        ('unknown', 'queued', 'launching', 'running', 'waiting_input',
         'blocked', 'succeeded', 'failed', 'cancelled')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (role_catalog_id, role_catalog_version)
        REFERENCES role_catalog_revisions(catalog_id, version) ON DELETE RESTRICT,
    CHECK (id <> parent_seat_binding_id),
    CHECK (id <> replaced_by_seat_binding_id)
) STRICT;

-- Existing rows keep the deadline the old reader would have derived for them,
-- so the rebuild changes no seat's conclusion. Ten minutes is OP-REQ-039b's
-- grace, spelled here once for the backfill only.
INSERT INTO seat_bindings_v2
    (id, project_id, topology_node_id, role_slot_id, role_catalog_id,
     role_catalog_version, role_code, standard_title, custom_display_name,
     task_id, team_run_id, lifecycle, attach_deadline, revision,
     created_at, updated_at)
SELECT
    id, project_id, topology_node_id, role_slot_id, role_catalog_id,
    role_catalog_version, role_code, standard_title, custom_display_name,
    task_id, team_run_id, lifecycle,
    strftime('%Y-%m-%dT%H:%M:%SZ', created_at, '+600 seconds'),
    revision, created_at, updated_at
FROM seat_bindings;

DROP TABLE seat_bindings;

ALTER TABLE seat_bindings_v2 RENAME TO seat_bindings;

CREATE UNIQUE INDEX ux_active_seat_binding_key
    ON seat_bindings(topology_node_id, role_slot_id)
    WHERE lifecycle = 'active';

CREATE INDEX ix_seat_bindings_project_node
    ON seat_bindings(project_id, topology_node_id, created_at, id);

CREATE INDEX ix_seat_bindings_team_run
    ON seat_bindings(project_id, team_run_id);

PRAGMA user_version = 26;
