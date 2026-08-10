-- Kontor schema v1 (KON-MVP-03 / ASMA-7747).
--
-- This file is applied exactly once, inside one BEGIN IMMEDIATE transaction, by
-- `kontor_store::migrations`. Any error rolls back every object below and leaves
-- `user_version` at 0. The last statement sets `user_version = 1`.
--
-- Once this schema has shipped in a public release it is FROZEN: later changes
-- are new numbered migrations, never edits to this file.
--
-- Conventions
-- -----------
-- * Every table is STRICT; every column is NOT NULL unless absence is meaningful.
-- * Entity ids, timestamps and digests are canonical TEXT. Booleans, counters,
--   revisions and cursors are INTEGER. Versioned documents and evidence are
--   canonical JSON TEXT guarded by `json_valid`.
-- * Closed domain enums are stored as their stable TEXT spelling with a
--   `CHECK (... IN (...))` list. The spelling is the same one the Rust
--   `closed_enum!` types and the JSON wire format use, so a stored row is
--   readable, reviewable and impossible to reinterpret by reordering a Rust
--   enum. No floating-point column exists anywhere; money is integer minor
--   units plus an ISO-4217 code.
-- * Every project-scoped table carries `project_id`, and every parent reference
--   is a COMPOSITE foreign key on `(project_id, id)`. A globally valid UUID from
--   another project therefore cannot resolve: global uniqueness is not tenant
--   isolation.
-- * Immutability and append-only behaviour are enforced by BEFORE UPDATE/DELETE
--   triggers, not only by the Rust layer, so direct SQL cannot rewrite history.
-- * Uncertainty is never terminal. Terminal CHECKs are written as "a terminal
--   lifecycle requires evidence and a closure time", never as "no evidence means
--   closed". Nothing in this file maps a missing binding, a missing process or a
--   missing timestamp to a terminal state.

-- ===========================================================================
-- 0. Realm identity
--
-- One database file is one Realm. The singleton primary key makes a second row
-- impossible, and the triggers below make the one row immutable. Domain tables
-- deliberately do NOT repeat `realm_id`: the file is the isolation boundary, so
-- a Realm column on every table would be filtering, not isolation.
--
-- The row is inserted by `migrations.rs` inside the same IMMEDIATE transaction
-- that creates every object below, so a database can never exist with a schema
-- but no Realm, or with a Realm but no schema.
-- ===========================================================================

CREATE TABLE realm_metadata (
    singleton      INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    realm_id       TEXT    NOT NULL UNIQUE
                           CHECK (length(realm_id) = 36 AND realm_id NOT GLOB '*[^0-9a-f-]*'),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    created_at     TEXT    NOT NULL
                           CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]*Z'),
    display_label  TEXT    NULL CHECK (display_label IS NULL OR length(display_label) BETWEEN 1 AND 512)
) STRICT;

-- Realm identity is never updated, deleted or regenerated. Recovery from a
-- corrupt row is an explicit export/import into a *different* Realm, never an
-- in-place repair.
CREATE TRIGGER realm_metadata_no_update BEFORE UPDATE ON realm_metadata
BEGIN SELECT RAISE(ABORT, 'realm identity is immutable'); END;
CREATE TRIGGER realm_metadata_no_delete BEFORE DELETE ON realm_metadata
BEGIN SELECT RAISE(ABORT, 'realm identity is immutable'); END;

-- ===========================================================================
-- 1. Hierarchy
-- ===========================================================================

CREATE TABLE projects (
    id          TEXT    NOT NULL PRIMARY KEY
                        CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    name        TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 512),
    root_path   TEXT    NOT NULL UNIQUE CHECK (length(root_path) BETWEEN 1 AND 4096),
    revision    INTEGER NOT NULL CHECK (revision >= 1),
    created_at  TEXT    NOT NULL
                        CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9]*Z')
) STRICT;

CREATE TABLE mini_projects (
    id          TEXT    NOT NULL PRIMARY KEY
                        CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id  TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    name        TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 512),
    revision    INTEGER NOT NULL CHECK (revision >= 1),
    created_at  TEXT    NOT NULL
                        CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id)
) STRICT;

CREATE TABLE tasks (
    id              TEXT    NOT NULL PRIMARY KEY
                            CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id TEXT    NULL,
    title           TEXT    NOT NULL CHECK (length(title) BETWEEN 1 AND 512),
    module_key      TEXT    NULL CHECK (module_key IS NULL OR length(module_key) BETWEEN 1 AND 128),
    state           TEXT    NOT NULL CHECK (state IN (
                                'draft', 'todo', 'ready', 'in_progress', 'blocked',
                                'parked', 'needs_human', 'done', 'failed', 'cancelled')),
    revision        INTEGER NOT NULL CHECK (revision >= 1),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    updated_at      TEXT    NOT NULL
                            CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_tasks_project_state ON tasks (project_id, state);

CREATE TABLE task_dependencies (
    project_id          TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id             TEXT NOT NULL,
    depends_on_task_id  TEXT NOT NULL,
    created_at          TEXT NOT NULL
                             CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, task_id, depends_on_task_id),
    CHECK (task_id <> depends_on_task_id),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, depends_on_task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT
) STRICT;

-- ===========================================================================
-- 2. Specifications, workflows, gates, personas and intake
-- ===========================================================================

CREATE TABLE work_profiles (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    profile_key     TEXT    NOT NULL CHECK (length(profile_key) BETWEEN 1 AND 128),
    version         INTEGER NOT NULL CHECK (version >= 1),
    definition      TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64 AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, profile_key, version)
) STRICT;

CREATE TABLE team_templates (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    template_id     TEXT    NOT NULL
                            CHECK (length(template_id) = 36 AND template_id NOT GLOB '*[^0-9a-f-]*'),
    version         INTEGER NOT NULL CHECK (version >= 1),
    name            TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 512),
    definition      TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64 AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    role_authority  TEXT    NOT NULL CHECK (json_valid(role_authority)),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, template_id, version)
) STRICT;

CREATE TABLE task_workflows (
    id              TEXT    NOT NULL PRIMARY KEY
                            CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id         TEXT    NOT NULL,
    profile_key     TEXT    NOT NULL CHECK (length(profile_key) BETWEEN 1 AND 128),
    profile_version INTEGER NOT NULL CHECK (profile_version >= 1),
    snapshot        TEXT    NOT NULL CHECK (json_valid(snapshot)),
    snapshot_hash   TEXT    NOT NULL
                            CHECK (length(snapshot_hash) = 64 AND snapshot_hash NOT GLOB '*[^0-9a-f]*'),
    current_phase   TEXT    NOT NULL CHECK (length(current_phase) BETWEEN 1 AND 128),
    active          INTEGER NOT NULL CHECK (active IN (0, 1)),
    revision        INTEGER NOT NULL CHECK (revision >= 1),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    -- The pinned profile revision must exist in this project. A snapshot that is
    -- merely self-consistent proves nothing about what was actually stored.
    FOREIGN KEY (project_id, profile_key, profile_version)
        REFERENCES work_profiles (project_id, profile_key, version) ON DELETE RESTRICT
) STRICT;

-- At most one active workflow per task.
CREATE UNIQUE INDEX ux_task_workflows_active
    ON task_workflows (project_id, task_id) WHERE active = 1;

CREATE TABLE task_gate_evaluations (
    project_id        TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    workflow_id       TEXT    NOT NULL,
    gate_key          TEXT    NOT NULL CHECK (length(gate_key) BETWEEN 1 AND 128),
    sequence          INTEGER NOT NULL CHECK (sequence >= 1),
    verdict           TEXT    NOT NULL CHECK (verdict IN
                                  ('started', 'passed', 'rejected', 'waived', 'parked')),
    evaluator_role    TEXT    NOT NULL CHECK (length(evaluator_role) BETWEEN 1 AND 128),
    evaluator_account TEXT    NOT NULL,
    evidence          TEXT    NOT NULL CHECK (json_valid(evidence)),
    recorded_at       TEXT    NOT NULL
                              CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, workflow_id, gate_key, sequence),
    -- A pass or a waiver must cite evidence; SQL enforces it independently of
    -- the domain layer.
    CHECK (verdict NOT IN ('passed', 'waived') OR json_array_length(evidence) >= 1),
    FOREIGN KEY (project_id, workflow_id)
        REFERENCES task_workflows (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, evaluator_account)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE persona_scenarios (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    scenario_id     TEXT    NOT NULL
                            CHECK (length(scenario_id) = 36 AND scenario_id NOT GLOB '*[^0-9a-f-]*'),
    version         INTEGER NOT NULL CHECK (version >= 1),
    persona_key     TEXT    NOT NULL CHECK (length(persona_key) BETWEEN 1 AND 128),
    gate_key        TEXT    NOT NULL CHECK (length(gate_key) BETWEEN 1 AND 128),
    definition      TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64 AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, scenario_id, version)
) STRICT;

CREATE TABLE task_persona_snapshots (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id         TEXT    NOT NULL,
    scenario_id     TEXT    NOT NULL,
    version         INTEGER NOT NULL CHECK (version >= 1),
    -- The workflow whose pinned profile authorized this persona's evaluators,
    -- and the gate it was checked against. Without these the authority claim
    -- would be unverifiable after the fact.
    workflow_id     TEXT    NOT NULL,
    gate_key        TEXT    NOT NULL CHECK (length(gate_key) BETWEEN 1 AND 128),
    snapshot        TEXT    NOT NULL CHECK (json_valid(snapshot)),
    snapshot_hash   TEXT    NOT NULL
                            CHECK (length(snapshot_hash) = 64 AND snapshot_hash NOT GLOB '*[^0-9a-f]*'),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, task_id, scenario_id, version),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, scenario_id, version)
        REFERENCES persona_scenarios (project_id, scenario_id, version) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, workflow_id)
        REFERENCES task_workflows (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE trigger_specs (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    trigger_key     TEXT    NOT NULL CHECK (length(trigger_key) BETWEEN 1 AND 128),
    version         INTEGER NOT NULL CHECK (version >= 1),
    source_kind     TEXT    NOT NULL CHECK (length(source_kind) BETWEEN 1 AND 128),
    source_connection TEXT  NOT NULL CHECK (length(source_connection) BETWEEN 1 AND 128),
    -- The relational half of the trigger's pins. The canonical definition stays
    -- authoritative, but a logical id with a same-database target does not get
    -- to live in JSON alone: the repository writes both and re-checks that they
    -- agree byte-for-byte on insert and on read.
    work_profile_key      TEXT    NOT NULL CHECK (length(work_profile_key) BETWEEN 1 AND 128),
    work_profile_version  INTEGER NOT NULL CHECK (work_profile_version >= 1),
    team_template_id      TEXT    NOT NULL
                            CHECK (length(team_template_id) = 36 AND team_template_id NOT GLOB '*[^0-9a-f-]*'),
    team_template_version INTEGER NOT NULL CHECK (team_template_version >= 1),
    -- Context templates have no table in schema v1, so these two columns are
    -- normalized for the agreement check but cannot carry a foreign key yet.
    context_template      TEXT    NOT NULL CHECK (length(context_template) BETWEEN 1 AND 128),
    context_version       INTEGER NOT NULL CHECK (context_version >= 1),
    -- Calendar policy is optional; the pair is all-null or all-present.
    calendar_profile_id   TEXT    NULL
                            CHECK (calendar_profile_id IS NULL
                                   OR (length(calendar_profile_id) = 36
                                       AND calendar_profile_id NOT GLOB '*[^0-9a-f-]*')),
    calendar_version      INTEGER NULL CHECK (calendar_version IS NULL OR calendar_version >= 1),
    definition      TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64 AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, trigger_key, version),
    CHECK ((calendar_profile_id IS NULL) = (calendar_version IS NULL)),
    FOREIGN KEY (project_id, work_profile_key, work_profile_version)
        REFERENCES work_profiles (project_id, profile_key, version) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_template_id, team_template_version)
        REFERENCES team_templates (project_id, template_id, version) ON DELETE RESTRICT,
    -- Calendar profiles are workspace-level, so this pin is not project-scoped.
    FOREIGN KEY (calendar_profile_id, calendar_version)
        REFERENCES calendar_profiles (profile_id, version) ON DELETE RESTRICT
) STRICT;

CREATE TABLE source_events (
    id                   TEXT NOT NULL PRIMARY KEY
                              CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id           TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    source_kind          TEXT NOT NULL CHECK (length(source_kind) BETWEEN 1 AND 128),
    source_connection    TEXT NOT NULL CHECK (length(source_connection) BETWEEN 1 AND 128),
    external_event_id    TEXT NOT NULL CHECK (length(external_event_id) BETWEEN 1 AND 256),
    envelope             TEXT NOT NULL CHECK (json_valid(envelope)),
    envelope_hash        TEXT NOT NULL
                              CHECK (length(envelope_hash) = 64 AND envelope_hash NOT GLOB '*[^0-9a-f]*'),
    external_observed_at TEXT NOT NULL
                              CHECK (external_observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    ingested_at          TEXT NOT NULL
                              CHECK (ingested_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    processing_state     TEXT NOT NULL CHECK (processing_state IN
                              ('received', 'evaluated', 'ignored', 'duplicate', 'failed')),
    UNIQUE (project_id, id),
    -- One event per source identity, and one event per canonical payload on the
    -- same connection. A repeat of either returns the original intake receipt.
    UNIQUE (project_id, source_kind, source_connection, external_event_id),
    UNIQUE (project_id, source_connection, envelope_hash)
) STRICT;

CREATE TABLE intake_receipts (
    id                TEXT    NOT NULL PRIMARY KEY
                              CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id        TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    source_event_id   TEXT    NOT NULL,
    source_event_hash TEXT    NOT NULL
                              CHECK (length(source_event_hash) = 64 AND source_event_hash NOT GLOB '*[^0-9a-f]*'),
    trigger_key       TEXT    NOT NULL CHECK (length(trigger_key) BETWEEN 1 AND 128),
    trigger_version   INTEGER NOT NULL CHECK (trigger_version >= 1),
    result            TEXT    NOT NULL CHECK (result IN
                              ('proposed', 'approved', 'rejected', 'ignored', 'duplicate')),
    receipt           TEXT    NOT NULL CHECK (json_valid(receipt)),
    idempotency_key   TEXT    NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    dedup_key         TEXT    NOT NULL
                              CHECK (length(dedup_key) = 64 AND dedup_key NOT GLOB '*[^0-9a-f]*'),
    duplicate_of      TEXT    NULL,
    -- Distinct from `duplicate_of`: this is the receipt a *newer trigger
    -- revision* supersedes, not a repeat of the same decision.
    predecessor_receipt_id TEXT NULL,
    decided_at        TEXT    NOT NULL
                              CHECK (decided_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    UNIQUE (project_id, idempotency_key),
    -- One decision per (event, trigger revision). A replay under the same
    -- revision therefore cannot insert a second receipt.
    UNIQUE (project_id, source_event_id, trigger_key, trigger_version),
    -- A duplicate always points at the original; nothing else may.
    CHECK ((result = 'duplicate') = (duplicate_of IS NOT NULL)),
    CHECK (predecessor_receipt_id IS NULL OR predecessor_receipt_id <> id),
    FOREIGN KEY (project_id, source_event_id)
        REFERENCES source_events (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, duplicate_of)
        REFERENCES intake_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, predecessor_receipt_id)
        REFERENCES intake_receipts (project_id, id) ON DELETE RESTRICT,
    -- The decision is pinned to the exact trigger revision that produced it.
    FOREIGN KEY (project_id, trigger_key, trigger_version)
        REFERENCES trigger_specs (project_id, trigger_key, version) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_intake_receipts_event ON intake_receipts (project_id, source_event_id);

-- ===========================================================================
-- 3. Calendars, authorization and overrides
-- ===========================================================================

-- Calendar profiles are workspace-level: they are shared by projects and
-- therefore carry no project_id.
CREATE TABLE calendar_profiles (
    profile_id      TEXT    NOT NULL
                            CHECK (length(profile_id) = 36 AND profile_id NOT GLOB '*[^0-9a-f-]*'),
    version         INTEGER NOT NULL CHECK (version >= 1),
    name            TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 512),
    definition      TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64 AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (profile_id, version)
) STRICT;

CREATE TABLE work_calendars (
    id              TEXT    NOT NULL PRIMARY KEY
                            CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    profile_id      TEXT    NOT NULL,
    profile_version INTEGER NOT NULL CHECK (profile_version >= 1),
    timezone        TEXT    NOT NULL CHECK (length(timezone) BETWEEN 1 AND 128),
    window_override TEXT    NULL CHECK (window_override IS NULL OR json_valid(window_override)),
    active          INTEGER NOT NULL CHECK (active IN (0, 1)),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    retired_at      TEXT    NULL
                            CHECK (retired_at IS NULL OR retired_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    CHECK (active = 0 OR retired_at IS NULL),
    FOREIGN KEY (profile_id, profile_version)
        REFERENCES calendar_profiles (profile_id, version) ON DELETE RESTRICT
) STRICT;

-- At most one active assignment per project. Zero rows is legal and means the
-- project is unrestricted.
CREATE UNIQUE INDEX ux_work_calendars_active
    ON work_calendars (project_id) WHERE active = 1;

CREATE TABLE holiday_sources (
    id              TEXT    NOT NULL PRIMARY KEY
                            CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    profile_id      TEXT    NOT NULL,
    profile_version INTEGER NOT NULL CHECK (profile_version >= 1),
    provider        TEXT    NOT NULL CHECK (provider IN ('ical', 'manual', 'bundled')),
    country         TEXT    NOT NULL CHECK (length(country) = 2),
    subdivision     TEXT    NULL CHECK (subdivision IS NULL OR length(subdivision) BETWEEN 1 AND 128),
    reference       TEXT    NOT NULL CHECK (length(reference) BETWEEN 1 AND 512),
    range_start     TEXT    NOT NULL CHECK (range_start GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    range_end       TEXT    NOT NULL CHECK (range_end GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    retrieved_at    TEXT    NOT NULL
                            CHECK (retrieved_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    raw_hash        TEXT    NOT NULL
                            CHECK (length(raw_hash) = 64 AND raw_hash NOT GLOB '*[^0-9a-f]*'),
    normalized_hash TEXT    NOT NULL
                            CHECK (length(normalized_hash) = 64 AND normalized_hash NOT GLOB '*[^0-9a-f]*'),
    CHECK (range_start <= range_end),
    FOREIGN KEY (profile_id, profile_version)
        REFERENCES calendar_profiles (profile_id, version) ON DELETE RESTRICT
) STRICT;

CREATE TABLE calendar_exceptions (
    id               TEXT NOT NULL PRIMARY KEY
                          CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id       TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    work_calendar_id TEXT NOT NULL,
    start_date       TEXT NOT NULL CHECK (start_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    end_date         TEXT NOT NULL CHECK (end_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    kind             TEXT NOT NULL CHECK (kind IN ('open', 'closed')),
    label            TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 512),
    provenance       TEXT NOT NULL CHECK (json_valid(provenance)),
    supersedes       TEXT NULL,
    created_at       TEXT NOT NULL
                          CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    CHECK (start_date <= end_date),
    FOREIGN KEY (project_id, work_calendar_id)
        REFERENCES work_calendars (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, supersedes)
        REFERENCES calendar_exceptions (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_calendar_exceptions_calendar
    ON calendar_exceptions (project_id, work_calendar_id, start_date);

CREATE TABLE execution_authorizations (
    id                    TEXT    NOT NULL PRIMARY KEY
                                  CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id            TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    scope_kind            TEXT    NOT NULL CHECK (scope_kind IN ('project', 'mini_project', 'task')),
    scope_mini_project_id TEXT    NULL,
    scope_task_id         TEXT    NULL,
    selected_tasks        TEXT    NOT NULL CHECK (json_valid(selected_tasks)),
    allowed_start         TEXT    NOT NULL
                                  CHECK (allowed_start GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    allowed_end           TEXT    NOT NULL
                                  CHECK (allowed_end GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    max_concurrency       INTEGER NOT NULL CHECK (max_concurrency >= 1),
    max_tokens            INTEGER NOT NULL CHECK (max_tokens >= 1),
    max_commands          INTEGER NOT NULL CHECK (max_commands >= 1),
    max_duration_seconds  INTEGER NOT NULL CHECK (max_duration_seconds >= 1),
    max_cost_minor_units  INTEGER NOT NULL CHECK (max_cost_minor_units >= 1),
    cost_currency         TEXT    NOT NULL CHECK (length(cost_currency) = 3),
    created_by            TEXT    NOT NULL,
    capability_receipt_id TEXT    NOT NULL,
    created_at            TEXT    NOT NULL
                                  CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    CHECK (allowed_start <= allowed_end),
    CHECK ((scope_kind = 'mini_project') = (scope_mini_project_id IS NOT NULL)),
    CHECK ((scope_kind = 'task') = (scope_task_id IS NOT NULL)),
    FOREIGN KEY (project_id, scope_mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, scope_task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, created_by)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, capability_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE schedule_overrides (
    id                     TEXT    NOT NULL PRIMARY KEY
                                   CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id             TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    scope_kind             TEXT    NOT NULL CHECK (scope_kind IN ('project', 'mini_project', 'task')),
    scope_mini_project_id  TEXT    NULL,
    scope_task_id          TEXT    NULL,
    reason                 TEXT    NOT NULL CHECK (length(reason) BETWEEN 1 AND 512),
    start_at               TEXT    NOT NULL
                                   CHECK (start_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    expiry_kind            TEXT    NOT NULL CHECK (expiry_kind IN ('fixed_at', 'goal_bound')),
    expiry_at              TEXT    NULL
                                   CHECK (expiry_at IS NULL OR expiry_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    expiry_mini_project_id TEXT    NULL,
    -- Mandatory. A goal-bound override still cannot outlive this instant.
    hard_ceiling           TEXT    NOT NULL
                                   CHECK (hard_ceiling GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    max_concurrency        INTEGER NOT NULL CHECK (max_concurrency >= 1),
    max_tokens             INTEGER NOT NULL CHECK (max_tokens >= 1),
    max_commands           INTEGER NOT NULL CHECK (max_commands >= 1),
    max_duration_seconds   INTEGER NOT NULL CHECK (max_duration_seconds >= 1),
    max_cost_minor_units   INTEGER NOT NULL CHECK (max_cost_minor_units >= 1),
    cost_currency          TEXT    NOT NULL CHECK (length(cost_currency) = 3),
    approved_by            TEXT    NOT NULL,
    approval_receipt_id    TEXT    NOT NULL,
    created_at             TEXT    NOT NULL
                                   CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revoked_at             TEXT    NULL
                                   CHECK (revoked_at IS NULL OR revoked_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revoked_by             TEXT    NULL,
    revocation_receipt_id  TEXT    NULL,
    UNIQUE (project_id, id),
    CHECK (start_at < hard_ceiling),
    CHECK ((expiry_kind = 'fixed_at') = (expiry_at IS NOT NULL)),
    CHECK ((expiry_kind = 'goal_bound') = (expiry_mini_project_id IS NOT NULL)),
    CHECK (expiry_at IS NULL OR expiry_at <= hard_ceiling),
    CHECK ((scope_kind = 'mini_project') = (scope_mini_project_id IS NOT NULL)),
    CHECK ((scope_kind = 'task') = (scope_task_id IS NOT NULL)),
    -- Revocation is all-or-nothing evidence.
    CHECK ((revoked_at IS NULL) = (revoked_by IS NULL)),
    CHECK ((revoked_at IS NULL) = (revocation_receipt_id IS NULL)),
    FOREIGN KEY (project_id, scope_mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, scope_task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, expiry_mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, approved_by)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, approval_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, revoked_by)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, revocation_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

-- ===========================================================================
-- 4. Accounts, context and handoffs
-- ===========================================================================

CREATE TABLE account_profiles (
    id                  TEXT NOT NULL PRIMARY KEY
                             CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id          TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    label               TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 512),
    external_account_id TEXT NULL
                             CHECK (external_account_id IS NULL OR length(external_account_id) BETWEEN 1 AND 256),
    created_at          TEXT NOT NULL
                             CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id)
) STRICT;

CREATE TABLE context_packs (
    id         TEXT NOT NULL PRIMARY KEY
                    CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id    TEXT NOT NULL,
    content    TEXT NOT NULL CHECK (json_valid(content)),
    content_hash TEXT NOT NULL
                    CHECK (length(content_hash) = 64 AND content_hash NOT GLOB '*[^0-9a-f]*'),
    created_at TEXT NOT NULL
                    CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE handoffs (
    id              TEXT NOT NULL PRIMARY KEY
                         CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id      TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    workflow_id     TEXT NOT NULL,
    from_phase      TEXT NOT NULL CHECK (length(from_phase) BETWEEN 1 AND 128),
    to_phase        TEXT NOT NULL CHECK (length(to_phase) BETWEEN 1 AND 128),
    context_pack_id TEXT NULL,
    payload         TEXT NOT NULL CHECK (json_valid(payload)),
    created_at      TEXT NOT NULL
                         CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, workflow_id)
        REFERENCES task_workflows (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, context_pack_id)
        REFERENCES context_packs (project_id, id) ON DELETE RESTRICT
) STRICT;

-- ===========================================================================
-- 5. Runs, runtime bindings, events and reconciliation
-- ===========================================================================

CREATE TABLE team_runs (
    id                      TEXT    NOT NULL PRIMARY KEY
                                    CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id              TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id                 TEXT    NOT NULL,
    template_id             TEXT    NOT NULL,
    template_version        INTEGER NOT NULL CHECK (template_version >= 1),
    snapshot                TEXT    NOT NULL CHECK (json_valid(snapshot)),
    snapshot_hash           TEXT    NOT NULL
                                    CHECK (length(snapshot_hash) = 64 AND snapshot_hash NOT GLOB '*[^0-9a-f]*'),
    lifecycle               TEXT    NOT NULL CHECK (lifecycle IN
                                    ('queued', 'launching', 'running', 'waiting_input', 'blocked',
                                     'succeeded', 'failed', 'cancelled', 'parked')),
    terminal_outcome        TEXT    NULL CHECK (terminal_outcome IS NULL OR terminal_outcome IN
                                    ('succeeded', 'failed', 'cancelled', 'parked', 'abandoned')),
    -- Evidence is a pointer into persisted rows, exactly as for agent runs: a
    -- child-evidence closure names the team whose children were counted, an
    -- operator closure names its receipt.
    terminal_source_kind    TEXT    NULL CHECK (terminal_source_kind IS NULL OR terminal_source_kind IN
                                    ('child_evidence', 'operator_abandon')),
    terminal_receipt_id     TEXT    NULL,
    terminal_evidence_hash  TEXT    NULL
                                    CHECK (terminal_evidence_hash IS NULL
                                           OR (length(terminal_evidence_hash) = 64
                                               AND terminal_evidence_hash NOT GLOB '*[^0-9a-f]*')),
    closed_at               TEXT    NULL
                                    CHECK (closed_at IS NULL OR closed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revision                INTEGER NOT NULL CHECK (revision >= 1),
    created_at              TEXT    NOT NULL
                                    CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    -- A terminal lifecycle requires a closure time and evidence. The implication
    -- runs one way only: a missing timestamp never implies a terminal lifecycle.
    CHECK (lifecycle NOT IN ('succeeded', 'failed', 'cancelled', 'parked')
           OR (closed_at IS NOT NULL AND terminal_outcome IS NOT NULL
               AND terminal_source_kind IS NOT NULL AND terminal_evidence_hash IS NOT NULL)),
    CHECK ((terminal_outcome IS NULL) = (terminal_source_kind IS NULL)),
    CHECK ((terminal_source_kind IS NULL) = (terminal_evidence_hash IS NULL)),
    CHECK ((terminal_source_kind = 'operator_abandon') = (terminal_receipt_id IS NOT NULL)),
    -- An operator can only ever abandon a team; every other outcome must be
    -- computed from immutable child evidence.
    CHECK (terminal_source_kind IS NOT 'operator_abandon' OR terminal_outcome = 'abandoned'),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, terminal_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, template_id, template_version)
        REFERENCES team_templates (project_id, template_id, version) ON DELETE RESTRICT
) STRICT;

CREATE TABLE agent_runs (
    id                    TEXT    NOT NULL PRIMARY KEY
                                  CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id            TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    team_run_id           TEXT    NOT NULL,
    parent_agent_run_id   TEXT    NULL,
    role_key              TEXT    NOT NULL CHECK (length(role_key) BETWEEN 1 AND 128),
    account_profile_id    TEXT    NULL,
    lifecycle             TEXT    NOT NULL CHECK (lifecycle IN
                                  ('queued', 'launching', 'running', 'waiting_input', 'blocked',
                                   'succeeded', 'failed', 'cancelled', 'parked')),
    desired_state         TEXT    NOT NULL CHECK (desired_state IN
                                  ('no_intent', 'run_requested', 'cancel_requested',
                                   'park_requested', 'abandon_requested')),
    observed_state        TEXT    NOT NULL CHECK (observed_state IN
                                  ('unknown', 'queued', 'launching', 'running', 'waiting_input',
                                   'blocked', 'succeeded', 'failed', 'cancelled')),
    -- The uncertainty values below are first-class and never terminal.
    derived_state         TEXT    NOT NULL CHECK (derived_state IN
                                  ('pending_confirmation', 'confirmed', 'stale', 'diverged',
                                   'runtime_unavailable', 'orphaned', 'lost_contact', 'terminal')),
    last_confirmed_at     TEXT    NULL
                                  CHECK (last_confirmed_at IS NULL OR last_confirmed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    last_cursor           INTEGER NULL CHECK (last_cursor IS NULL OR last_cursor >= 1),
    -- The runtime's own ordering for the last observation that actually reduced
    -- state. A replayed or older sequence is ignored rather than applied.
    last_native_sequence  INTEGER NULL CHECK (last_native_sequence IS NULL OR last_native_sequence >= 0),
    terminal_outcome      TEXT    NULL CHECK (terminal_outcome IS NULL OR terminal_outcome IN
                                  ('succeeded', 'failed', 'cancelled', 'parked', 'abandoned')),
    -- Evidence is a pointer into persisted rows, not a blob: JSON alone cannot
    -- be foreign-key bound, so it cannot prove the evidence belongs to this run.
    terminal_source_kind  TEXT    NULL CHECK (terminal_source_kind IS NULL OR terminal_source_kind IN
                                  ('runtime_observation', 'operator_abandon')),
    terminal_event_cursor INTEGER NULL CHECK (terminal_event_cursor IS NULL OR terminal_event_cursor >= 1),
    terminal_receipt_id   TEXT    NULL,
    terminal_evidence_hash TEXT   NULL
                                  CHECK (terminal_evidence_hash IS NULL
                                         OR (length(terminal_evidence_hash) = 64
                                             AND terminal_evidence_hash NOT GLOB '*[^0-9a-f]*')),
    closed_at             TEXT    NULL
                                  CHECK (closed_at IS NULL OR closed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revision              INTEGER NOT NULL CHECK (revision >= 1),
    created_at            TEXT    NOT NULL
                                  CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    CHECK (lifecycle NOT IN ('succeeded', 'failed', 'cancelled', 'parked')
           OR (closed_at IS NOT NULL AND terminal_outcome IS NOT NULL
               AND terminal_source_kind IS NOT NULL AND terminal_evidence_hash IS NOT NULL)),
    -- `terminal` is the only derived value that may carry an outcome, and it is
    -- reachable only together with evidence.
    CHECK ((derived_state = 'terminal') = (terminal_outcome IS NOT NULL)),
    -- Exactly one evidence shape, and all of it or none of it.
    CHECK ((terminal_outcome IS NULL) = (terminal_source_kind IS NULL)),
    CHECK ((terminal_source_kind IS NULL) = (terminal_evidence_hash IS NULL)),
    CHECK ((terminal_source_kind = 'runtime_observation') = (terminal_event_cursor IS NOT NULL)),
    CHECK ((terminal_source_kind = 'operator_abandon') = (terminal_receipt_id IS NOT NULL)),
    -- An operator can only ever abandon; a runtime verdict is the only route to
    -- succeeded/failed/cancelled.
    CHECK (terminal_source_kind IS NOT 'operator_abandon' OR terminal_outcome = 'abandoned'),
    CHECK (parent_agent_run_id IS NULL OR parent_agent_run_id <> id),
    FOREIGN KEY (project_id, terminal_event_cursor)
        REFERENCES runtime_events (project_id, cursor) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, terminal_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_run_id) REFERENCES team_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, parent_agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, account_profile_id)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_agent_runs_team ON agent_runs (project_id, team_run_id);

CREATE TABLE runtime_bindings (
    id            TEXT    NOT NULL PRIMARY KEY
                          CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id    TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    agent_run_id  TEXT    NOT NULL,
    runtime_kind  TEXT    NOT NULL CHECK (length(runtime_kind) BETWEEN 1 AND 128),
    host          TEXT    NOT NULL CHECK (length(host) BETWEEN 1 AND 512),
    generation    INTEGER NOT NULL CHECK (generation >= 0),
    native_id     TEXT    NOT NULL CHECK (length(native_id) BETWEEN 1 AND 256),
    bound_at      TEXT    NOT NULL
                          CHECK (bound_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    -- One binding per run: recovery creates a successor run, never a rebind.
    UNIQUE (project_id, agent_run_id),
    -- Native identity is unique inside a runtime generation, not globally.
    UNIQUE (runtime_kind, host, generation, native_id),
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT
) STRICT;

-- The single persisted event-cursor space. It keeps its architecture v1 name
-- while carrying two shapes: trusted runtime observations, and the command
-- intents that must commit atomically with their receipt and outbox entry.
CREATE TABLE runtime_events (
    -- AUTOINCREMENT guarantees a monotonic, never-reused cursor even after
    -- deletes, so a subscriber can always resume strictly after what it saw.
    cursor             INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    project_id         TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    event_kind         TEXT    NOT NULL CHECK (event_kind IN
                               ('runtime_observation', 'command_intent')),
    agent_run_id       TEXT    NULL,
    runtime_kind       TEXT    NULL CHECK (runtime_kind IS NULL OR length(runtime_kind) BETWEEN 1 AND 128),
    host               TEXT    NULL CHECK (host IS NULL OR length(host) BETWEEN 1 AND 512),
    generation         INTEGER NULL CHECK (generation IS NULL OR generation >= 0),
    native_id          TEXT    NULL CHECK (native_id IS NULL OR length(native_id) BETWEEN 1 AND 256),
    native_event_id    TEXT    NULL CHECK (native_event_id IS NULL OR length(native_event_id) BETWEEN 1 AND 256),
    -- The runtime's own ordering. A state-reducing observation must carry one.
    native_sequence    INTEGER NULL CHECK (native_sequence IS NULL OR native_sequence >= 0),
    observed_state     TEXT    NULL CHECK (observed_state IS NULL OR observed_state IN
                               ('unknown', 'queued', 'launching', 'running', 'waiting_input',
                                'blocked', 'succeeded', 'failed', 'cancelled')),
    command_receipt_id TEXT    NULL,
    payload            TEXT    NOT NULL CHECK (json_valid(payload)),
    payload_hash       TEXT    NOT NULL
                               CHECK (length(payload_hash) = 64 AND payload_hash NOT GLOB '*[^0-9a-f]*'),
    observed_at        TEXT    NOT NULL
                               CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    recorded_at        TEXT    NOT NULL
                               CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- Each shape is complete or absent; neither can borrow the other's columns.
    CHECK (event_kind <> 'runtime_observation'
           OR (agent_run_id IS NOT NULL AND runtime_kind IS NOT NULL AND host IS NOT NULL
               AND generation IS NOT NULL AND native_id IS NOT NULL
               AND native_sequence IS NOT NULL AND command_receipt_id IS NULL)),
    CHECK (event_kind <> 'command_intent'
           OR (command_receipt_id IS NOT NULL AND agent_run_id IS NULL
               AND runtime_kind IS NULL AND host IS NULL AND generation IS NULL
               AND native_id IS NULL AND native_event_id IS NULL
               AND native_sequence IS NULL AND observed_state IS NULL)),
    UNIQUE (project_id, cursor),
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, command_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

-- Reserve cursor 1 as the "nothing has happened yet" origin, so the first real
-- event is 2.
--
-- A snapshot taken against an empty ledger has to report *some* position, and a
-- subscriber resumes strictly after it. If the first event could also be 1, that
-- subscriber would silently skip it. Cursors stay positive (the domain type
-- refuses 0), and cursor 1 simply never names a row.
INSERT INTO sqlite_sequence (name, seq) VALUES ('runtime_events', 1);

CREATE INDEX ix_runtime_events_run ON runtime_events (project_id, agent_run_id, cursor);

-- Deduplicate observations by native event id inside a runtime generation when
-- the runtime provides one, and by canonical payload digest per run when it
-- does not. Neither applies to command intents.
CREATE UNIQUE INDEX ux_runtime_events_native
    ON runtime_events (runtime_kind, generation, native_event_id)
    WHERE event_kind = 'runtime_observation' AND native_event_id IS NOT NULL;
CREATE UNIQUE INDEX ux_runtime_events_hash
    ON runtime_events (agent_run_id, payload_hash)
    WHERE event_kind = 'runtime_observation' AND native_event_id IS NULL;

-- Exactly one intent event per command receipt.
CREATE UNIQUE INDEX ux_runtime_events_intent
    ON runtime_events (command_receipt_id)
    WHERE event_kind = 'command_intent';

CREATE TABLE runtime_reconciliation_epochs (
    project_id   TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    runtime_kind TEXT    NOT NULL CHECK (length(runtime_kind) BETWEEN 1 AND 128),
    host         TEXT    NOT NULL CHECK (length(host) BETWEEN 1 AND 512),
    generation   INTEGER NOT NULL CHECK (generation >= 0),
    started_at   TEXT    NOT NULL
                         CHECK (started_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    completed_at TEXT    NULL
                         CHECK (completed_at IS NULL OR completed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    status       TEXT    NOT NULL CHECK (status IN ('in_progress', 'completed', 'failed')),
    PRIMARY KEY (project_id, runtime_kind, host, generation, started_at),
    CHECK (status = 'in_progress' OR completed_at IS NOT NULL)
) STRICT;

CREATE TABLE guardrail_evaluations (
    id           TEXT NOT NULL PRIMARY KEY
                      CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id   TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    agent_run_id TEXT NOT NULL,
    rung         INTEGER NOT NULL CHECK (rung >= 1),
    verdict      TEXT NOT NULL CHECK (verdict IN ('pass', 'warn', 'block')),
    evidence     TEXT NOT NULL CHECK (json_valid(evidence)),
    evidence_hash TEXT NOT NULL
                      CHECK (length(evidence_hash) = 64 AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at  TEXT NOT NULL
                      CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE resource_leases (
    id            TEXT NOT NULL PRIMARY KEY
                       CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id    TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    resource_key  TEXT NOT NULL CHECK (length(resource_key) BETWEEN 1 AND 256),
    -- NULL means "no worktree isolation": such leases contend with each other.
    worktree_key  TEXT NULL CHECK (worktree_key IS NULL OR length(worktree_key) BETWEEN 1 AND 256),
    agent_run_id  TEXT NOT NULL,
    acquired_at   TEXT NOT NULL
                       CHECK (acquired_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- Release is a receipt-backed transaction, never a wall-clock expression.
    released_at   TEXT NULL
                       CHECK (released_at IS NULL OR released_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    release_receipt_id TEXT NULL,
    UNIQUE (project_id, id),
    CHECK ((released_at IS NULL) = (release_receipt_id IS NULL)),
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, release_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE UNIQUE INDEX ux_resource_leases_active
    ON resource_leases (project_id, resource_key, COALESCE(worktree_key, ''))
    WHERE released_at IS NULL;

-- ===========================================================================
-- 6. External ticket synchronization
-- ===========================================================================

-- `jira_links` keeps the architecture's original table name; the connector is a
-- column, and nothing in this schema is Jira-specific.
CREATE TABLE jira_links (
    id                 TEXT    NOT NULL PRIMARY KEY
                               CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id         TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id            TEXT    NOT NULL,
    connector          TEXT    NOT NULL CHECK (length(connector) BETWEEN 1 AND 128),
    external_issue_key TEXT    NOT NULL CHECK (length(external_issue_key) BETWEEN 1 AND 256),
    revision           INTEGER NOT NULL CHECK (revision >= 1),
    created_at         TEXT    NOT NULL
                               CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    UNIQUE (project_id, connector, external_issue_key),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE ticket_field_specs (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    connector       TEXT    NOT NULL CHECK (length(connector) BETWEEN 1 AND 128),
    external_project TEXT   NOT NULL CHECK (length(external_project) BETWEEN 1 AND 128),
    issue_type      TEXT    NOT NULL CHECK (length(issue_type) BETWEEN 1 AND 128),
    version         INTEGER NOT NULL CHECK (version >= 1),
    definition      TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64 AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, connector, external_project, issue_type, version)
) STRICT;

CREATE TABLE external_workflow_specs (
    project_id       TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    connector        TEXT    NOT NULL CHECK (length(connector) BETWEEN 1 AND 128),
    external_project TEXT    NOT NULL CHECK (length(external_project) BETWEEN 1 AND 128),
    issue_type       TEXT    NOT NULL CHECK (length(issue_type) BETWEEN 1 AND 128),
    version          INTEGER NOT NULL CHECK (version >= 1),
    -- A profile-specific mapping pins the exact profile revision it was written
    -- for. All-null or fully present, and foreign-key backed in this project.
    work_profile_key TEXT    NULL CHECK (work_profile_key IS NULL OR length(work_profile_key) BETWEEN 1 AND 128),
    work_profile_version INTEGER NULL CHECK (work_profile_version IS NULL OR work_profile_version >= 1),
    definition       TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash  TEXT    NOT NULL
                             CHECK (length(definition_hash) = 64 AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at       TEXT    NOT NULL
                             CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, connector, external_project, issue_type, version),
    CHECK ((work_profile_key IS NULL) = (work_profile_version IS NULL)),
    FOREIGN KEY (project_id, work_profile_key, work_profile_version)
        REFERENCES work_profiles (project_id, profile_key, version) ON DELETE RESTRICT
) STRICT;

CREATE TABLE ticket_sync_projections (
    id             TEXT    NOT NULL PRIMARY KEY
                           CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id     TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    link_id        TEXT    NOT NULL,
    link_revision  INTEGER NOT NULL CHECK (link_revision >= 1),
    connector      TEXT    NOT NULL CHECK (length(connector) BETWEEN 1 AND 128),
    -- The exact ticket-field specification this projection was computed against.
    -- Without it, a stored projection cannot be re-checked against the mapping
    -- that produced it.
    field_spec_project    TEXT    NOT NULL CHECK (length(field_spec_project) BETWEEN 1 AND 128),
    field_spec_issue_type TEXT    NOT NULL CHECK (length(field_spec_issue_type) BETWEEN 1 AND 128),
    field_spec_version    INTEGER NOT NULL CHECK (field_spec_version >= 1),
    external_issue_key TEXT NOT NULL CHECK (length(external_issue_key) BETWEEN 1 AND 256),
    fields         TEXT    NOT NULL CHECK (json_valid(fields)),
    -- Schema v1 has exactly one comment policy and no outbound comment column.
    comment_policy TEXT    NOT NULL CHECK (comment_policy = 'inbound_only'),
    external_comment_cursor TEXT NULL
                           CHECK (external_comment_cursor IS NULL OR length(external_comment_cursor) BETWEEN 1 AND 256),
    projection_hash TEXT   NOT NULL
                           CHECK (length(projection_hash) = 64 AND projection_hash NOT GLOB '*[^0-9a-f]*'),
    computed_at    TEXT    NOT NULL
                           CHECK (computed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    UNIQUE (project_id, link_id, link_revision),
    FOREIGN KEY (project_id, link_id) REFERENCES jira_links (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, connector, field_spec_project, field_spec_issue_type,
                 field_spec_version)
        REFERENCES ticket_field_specs (project_id, connector, external_project, issue_type,
                                       version) ON DELETE RESTRICT
) STRICT;

-- Inbound only. There is deliberately no outbound comment table or column
-- anywhere in this schema: adding one is a migration, not a configuration.
CREATE TABLE external_comments (
    project_id          TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    link_id             TEXT NOT NULL,
    external_comment_id TEXT NOT NULL CHECK (length(external_comment_id) BETWEEN 1 AND 256),
    body_hash           TEXT NOT NULL
                             CHECK (length(body_hash) = 64 AND body_hash NOT GLOB '*[^0-9a-f]*'),
    author_account_id   TEXT NOT NULL CHECK (length(author_account_id) BETWEEN 1 AND 256),
    author_display      TEXT NULL CHECK (author_display IS NULL OR length(author_display) BETWEEN 1 AND 512),
    external_created_at TEXT NOT NULL
                             CHECK (external_created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    external_updated_at TEXT NOT NULL
                             CHECK (external_updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    body                TEXT NOT NULL,
    observed_at         TEXT NOT NULL
                             CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    supersedes_hash     TEXT NULL
                             CHECK (supersedes_hash IS NULL OR (length(supersedes_hash) = 64 AND supersedes_hash NOT GLOB '*[^0-9a-f]*')),
    -- Identity is (link, external id, body digest): a replay collides, an edit
    -- inserts a new revision and both keep their provenance.
    PRIMARY KEY (project_id, link_id, external_comment_id, body_hash),
    FOREIGN KEY (project_id, link_id) REFERENCES jira_links (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE external_ticket_observations (
    id                  TEXT NOT NULL PRIMARY KEY
                             CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id          TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    link_id             TEXT NOT NULL,
    status_id           TEXT NOT NULL CHECK (length(status_id) BETWEEN 1 AND 256),
    status_name         TEXT NOT NULL CHECK (length(status_name) BETWEEN 1 AND 512),
    status_category     TEXT NOT NULL CHECK (length(status_category) BETWEEN 1 AND 512),
    issue_type          TEXT NOT NULL CHECK (length(issue_type) BETWEEN 1 AND 128),
    assignee_account_id TEXT NULL CHECK (assignee_account_id IS NULL OR length(assignee_account_id) BETWEEN 1 AND 256),
    assignee_display    TEXT NULL CHECK (assignee_display IS NULL OR length(assignee_display) BETWEEN 1 AND 512),
    external_version    TEXT NULL CHECK (external_version IS NULL OR length(external_version) BETWEEN 1 AND 256),
    observed_at         TEXT NOT NULL
                             CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    payload_hash        TEXT NOT NULL
                             CHECK (length(payload_hash) = 64 AND payload_hash NOT GLOB '*[^0-9a-f]*'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, link_id) REFERENCES jira_links (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_observations_link
    ON external_ticket_observations (project_id, link_id, observed_at);

CREATE TABLE status_transition_receipts (
    id                   TEXT    NOT NULL PRIMARY KEY
                                 CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id           TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    link_id              TEXT    NOT NULL,
    task_id              TEXT    NOT NULL,
    task_revision        INTEGER NOT NULL CHECK (task_revision >= 1),
    workflow_revision    INTEGER NOT NULL CHECK (workflow_revision >= 1),
    projection_revision  INTEGER NOT NULL CHECK (projection_revision >= 1),
    spec_version         INTEGER NOT NULL CHECK (spec_version >= 1),
    prior_observation_id TEXT    NOT NULL,
    milestone            TEXT    NOT NULL CHECK (length(milestone) BETWEEN 1 AND 128),
    target_status_id     TEXT    NOT NULL CHECK (length(target_status_id) BETWEEN 1 AND 256),
    -- NULL only for assignee-only convergence, which is exactly why an
    -- already-applied transition is never retried.
    transition_id        TEXT    NULL CHECK (transition_id IS NULL OR length(transition_id) BETWEEN 1 AND 256),
    principal_account_id TEXT    NOT NULL CHECK (length(principal_account_id) BETWEEN 1 AND 256),
    assignment_prerequisite INTEGER NOT NULL CHECK (assignment_prerequisite IN (0, 1)),
    assignment_result    TEXT    NULL CHECK (assignment_result IS NULL OR json_valid(assignment_result)),
    plan                 TEXT    NOT NULL CHECK (json_valid(plan)),
    idempotency_key      TEXT    NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    dispatched_at        TEXT    NOT NULL
                                 CHECK (dispatched_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    acknowledged_at      TEXT    NULL
                                 CHECK (acknowledged_at IS NULL OR acknowledged_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    confirmed_at         TEXT    NULL
                                 CHECK (confirmed_at IS NULL OR confirmed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    refetched_observation_id TEXT NULL,
    UNIQUE (project_id, id),
    UNIQUE (project_id, idempotency_key),
    -- Confirmation is never an assumption: it requires a refetched observation.
    CHECK (confirmed_at IS NULL OR refetched_observation_id IS NOT NULL),
    CHECK (transition_id IS NOT NULL OR assignment_result IS NOT NULL OR assignment_prerequisite = 1),
    FOREIGN KEY (project_id, link_id) REFERENCES jira_links (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, prior_observation_id)
        REFERENCES external_ticket_observations (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, refetched_observation_id)
        REFERENCES external_ticket_observations (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE status_conflicts (
    id                 TEXT    NOT NULL PRIMARY KEY
                               CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id         TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    link_id            TEXT    NOT NULL,
    kind               TEXT    NOT NULL CHECK (kind IN (
                                   'stale_observation', 'no_live_transition',
                                   'multiple_live_transitions', 'incompatible_human_move',
                                   'external_terminal_before_internal_evidence',
                                   'unknown_status_class', 'unknown_transition_path',
                                   'ownership_unresolved', 'ownership_mismatch',
                                   'terminal_ownership_violation')),
    observation_id     TEXT    NOT NULL,
    task_revision      INTEGER NOT NULL CHECK (task_revision >= 1),
    spec_version       INTEGER NOT NULL CHECK (spec_version >= 1),
    milestone          TEXT    NULL CHECK (milestone IS NULL OR length(milestone) BETWEEN 1 AND 128),
    detected_at        TEXT    NOT NULL
                               CHECK (detected_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    resolved_at        TEXT    NULL
                               CHECK (resolved_at IS NULL OR resolved_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    resolution_receipt_id TEXT NULL,
    UNIQUE (project_id, id),
    CHECK ((resolved_at IS NULL) = (resolution_receipt_id IS NULL)),
    FOREIGN KEY (project_id, link_id) REFERENCES jira_links (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, observation_id)
        REFERENCES external_ticket_observations (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, resolution_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

-- ===========================================================================
-- 7. Commands and outbox
-- ===========================================================================

CREATE TABLE command_receipts (
    id               TEXT    NOT NULL PRIMARY KEY
                             CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id       TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    idempotency_key  TEXT    NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    kind             TEXT    NOT NULL CHECK (kind IN (
                                 'launch_run', 'cancel_run', 'park_run', 'abandon_run',
                                 'resume_task', 'record_gate_verdict', 'approve_intake',
                                 'sync_ticket', 'assign_ticket', 'transition_ticket',
                                 'authorize_execution', 'approve_schedule_override',
                                 'revoke_schedule_override', 'resolve_status_conflict',
                                 'assign_work_calendar')),
    target           TEXT    NOT NULL CHECK (json_valid(target)),
    target_revision  INTEGER NOT NULL CHECK (target_revision >= 1),
    intent           TEXT    NOT NULL CHECK (json_valid(intent)),
    intent_hash      TEXT    NOT NULL
                             CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    state            TEXT    NOT NULL CHECK (state IN (
                                 'intent_persisted', 'dispatch_pending', 'dispatched',
                                 'acknowledged', 'confirmation_unknown', 'confirmed', 'failed')),
    correlation      TEXT    NULL CHECK (correlation IS NULL OR length(correlation) BETWEEN 1 AND 256),
    native_identity  TEXT    NULL CHECK (native_identity IS NULL OR json_valid(native_identity)),
    result_ref       TEXT    NULL CHECK (result_ref IS NULL OR length(result_ref) BETWEEN 1 AND 256),
    attempts         INTEGER NOT NULL CHECK (attempts >= 0),
    created_at       TEXT    NOT NULL
                             CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    updated_at       TEXT    NOT NULL
                             CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id)
) STRICT;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

CREATE TABLE command_outbox (
    -- Project-scoped: a globally unique receipt id is not a substitute for
    -- proving the receipt belongs to this project.
    receipt_id    TEXT    NOT NULL PRIMARY KEY,
    project_id    TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    payload       TEXT    NOT NULL CHECK (json_valid(payload)),
    payload_hash  TEXT    NOT NULL
                          CHECK (length(payload_hash) = 64 AND payload_hash NOT GLOB '*[^0-9a-f]*'),
    not_before    TEXT    NOT NULL
                          CHECK (not_before GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    dispatched_at TEXT    NULL
                          CHECK (dispatched_at IS NULL OR dispatched_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    attempts      INTEGER NOT NULL CHECK (attempts >= 0),
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_command_outbox_due ON command_outbox (project_id, not_before)
    WHERE dispatched_at IS NULL;

-- One row per receipt naming exactly one typed target, each with its own
-- composite foreign key. The receipt also stores canonical target JSON, but the
-- JSON is only trusted because this row proves the same thing relationally: a
-- logical reference that has a same-database FK target must not live in JSON
-- alone.
CREATE TABLE command_targets (
    project_id            TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    receipt_id            TEXT NOT NULL,
    target_kind           TEXT NOT NULL CHECK (target_kind IN
                               ('project', 'mini_project', 'task', 'team_run', 'agent_run',
                                'ticket_link', 'work_calendar')),
    target_project_id     TEXT NULL,
    target_mini_project_id TEXT NULL,
    target_task_id        TEXT NULL,
    target_team_run_id    TEXT NULL,
    target_agent_run_id   TEXT NULL,
    target_ticket_link_id TEXT NULL,
    target_work_calendar_id TEXT NULL,
    PRIMARY KEY (project_id, receipt_id),
    -- Exactly one typed id, and it must be the one the kind names.
    CHECK ((target_kind = 'project') = (target_project_id IS NOT NULL)),
    CHECK ((target_kind = 'mini_project') = (target_mini_project_id IS NOT NULL)),
    CHECK ((target_kind = 'task') = (target_task_id IS NOT NULL)),
    CHECK ((target_kind = 'team_run') = (target_team_run_id IS NOT NULL)),
    CHECK ((target_kind = 'agent_run') = (target_agent_run_id IS NOT NULL)),
    CHECK ((target_kind = 'ticket_link') = (target_ticket_link_id IS NOT NULL)),
    CHECK ((target_kind = 'work_calendar') = (target_work_calendar_id IS NOT NULL)),
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (target_project_id) REFERENCES projects (id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, target_mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, target_task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, target_team_run_id)
        REFERENCES team_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, target_agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, target_ticket_link_id)
        REFERENCES jira_links (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, target_work_calendar_id)
        REFERENCES work_calendars (project_id, id) ON DELETE RESTRICT
) STRICT;

-- The tasks an authorization actually arms. The child set must equal the
-- canonical domain value exactly; leaving it in JSON would let it name a task
-- from another project.
CREATE TABLE execution_authorization_tasks (
    project_id       TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    authorization_id TEXT NOT NULL,
    task_id          TEXT NOT NULL,
    PRIMARY KEY (project_id, authorization_id, task_id),
    FOREIGN KEY (project_id, authorization_id)
        REFERENCES execution_authorizations (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT
) STRICT;

-- ===========================================================================
-- 8. Immutability and append-only triggers
--
-- Everything below exists so that direct SQL cannot do what the Rust layer
-- refuses. The two layers are deliberately redundant.
-- ===========================================================================

-- Immutable specification revisions: insert only.
CREATE TRIGGER work_profiles_no_update BEFORE UPDATE ON work_profiles
BEGIN SELECT RAISE(ABORT, 'work_profiles revisions are immutable'); END;
CREATE TRIGGER work_profiles_no_delete BEFORE DELETE ON work_profiles
BEGIN SELECT RAISE(ABORT, 'work_profiles revisions are immutable'); END;

CREATE TRIGGER team_templates_no_update BEFORE UPDATE ON team_templates
BEGIN SELECT RAISE(ABORT, 'team_templates revisions are immutable'); END;
CREATE TRIGGER team_templates_no_delete BEFORE DELETE ON team_templates
BEGIN SELECT RAISE(ABORT, 'team_templates revisions are immutable'); END;

CREATE TRIGGER persona_scenarios_no_update BEFORE UPDATE ON persona_scenarios
BEGIN SELECT RAISE(ABORT, 'persona_scenarios revisions are immutable'); END;
CREATE TRIGGER persona_scenarios_no_delete BEFORE DELETE ON persona_scenarios
BEGIN SELECT RAISE(ABORT, 'persona_scenarios revisions are immutable'); END;

CREATE TRIGGER task_persona_snapshots_no_update BEFORE UPDATE ON task_persona_snapshots
BEGIN SELECT RAISE(ABORT, 'persona snapshots are immutable'); END;
CREATE TRIGGER task_persona_snapshots_no_delete BEFORE DELETE ON task_persona_snapshots
BEGIN SELECT RAISE(ABORT, 'persona snapshots are immutable'); END;

CREATE TRIGGER trigger_specs_no_update BEFORE UPDATE ON trigger_specs
BEGIN SELECT RAISE(ABORT, 'trigger_specs revisions are immutable'); END;
CREATE TRIGGER trigger_specs_no_delete BEFORE DELETE ON trigger_specs
BEGIN SELECT RAISE(ABORT, 'trigger_specs revisions are immutable'); END;

CREATE TRIGGER calendar_profiles_no_update BEFORE UPDATE ON calendar_profiles
BEGIN SELECT RAISE(ABORT, 'calendar_profiles revisions are immutable'); END;
CREATE TRIGGER calendar_profiles_no_delete BEFORE DELETE ON calendar_profiles
BEGIN SELECT RAISE(ABORT, 'calendar_profiles revisions are immutable'); END;

CREATE TRIGGER ticket_field_specs_no_update BEFORE UPDATE ON ticket_field_specs
BEGIN SELECT RAISE(ABORT, 'ticket_field_specs revisions are immutable'); END;
CREATE TRIGGER ticket_field_specs_no_delete BEFORE DELETE ON ticket_field_specs
BEGIN SELECT RAISE(ABORT, 'ticket_field_specs revisions are immutable'); END;

CREATE TRIGGER external_workflow_specs_no_update BEFORE UPDATE ON external_workflow_specs
BEGIN SELECT RAISE(ABORT, 'external_workflow_specs revisions are immutable'); END;
CREATE TRIGGER external_workflow_specs_no_delete BEFORE DELETE ON external_workflow_specs
BEGIN SELECT RAISE(ABORT, 'external_workflow_specs revisions are immutable'); END;

CREATE TRIGGER holiday_sources_no_update BEFORE UPDATE ON holiday_sources
BEGIN SELECT RAISE(ABORT, 'holiday_sources revisions are immutable'); END;
CREATE TRIGGER holiday_sources_no_delete BEFORE DELETE ON holiday_sources
BEGIN SELECT RAISE(ABORT, 'holiday_sources revisions are immutable'); END;

-- Append-only histories.
CREATE TRIGGER task_gate_evaluations_no_update BEFORE UPDATE ON task_gate_evaluations
BEGIN SELECT RAISE(ABORT, 'gate evaluations are append-only'); END;
CREATE TRIGGER task_gate_evaluations_no_delete BEFORE DELETE ON task_gate_evaluations
BEGIN SELECT RAISE(ABORT, 'gate evaluations are append-only'); END;

CREATE TRIGGER source_events_no_update BEFORE UPDATE ON source_events
BEGIN SELECT RAISE(ABORT, 'source events are append-only'); END;
CREATE TRIGGER source_events_no_delete BEFORE DELETE ON source_events
BEGIN SELECT RAISE(ABORT, 'source events are append-only'); END;

CREATE TRIGGER intake_receipts_no_update BEFORE UPDATE ON intake_receipts
BEGIN SELECT RAISE(ABORT, 'intake receipts are append-only'); END;
CREATE TRIGGER intake_receipts_no_delete BEFORE DELETE ON intake_receipts
BEGIN SELECT RAISE(ABORT, 'intake receipts are append-only'); END;

CREATE TRIGGER runtime_events_no_update BEFORE UPDATE ON runtime_events
BEGIN SELECT RAISE(ABORT, 'runtime events are append-only'); END;
CREATE TRIGGER runtime_events_no_delete BEFORE DELETE ON runtime_events
BEGIN SELECT RAISE(ABORT, 'runtime events are append-only'); END;

CREATE TRIGGER guardrail_evaluations_no_update BEFORE UPDATE ON guardrail_evaluations
BEGIN SELECT RAISE(ABORT, 'guardrail evaluations are append-only'); END;
CREATE TRIGGER guardrail_evaluations_no_delete BEFORE DELETE ON guardrail_evaluations
BEGIN SELECT RAISE(ABORT, 'guardrail evaluations are append-only'); END;

CREATE TRIGGER external_comments_no_update BEFORE UPDATE ON external_comments
BEGIN SELECT RAISE(ABORT, 'external comment revisions are append-only'); END;
CREATE TRIGGER external_comments_no_delete BEFORE DELETE ON external_comments
BEGIN SELECT RAISE(ABORT, 'external comment revisions are append-only'); END;

CREATE TRIGGER external_ticket_observations_no_update BEFORE UPDATE ON external_ticket_observations
BEGIN SELECT RAISE(ABORT, 'external ticket observations are append-only'); END;
CREATE TRIGGER external_ticket_observations_no_delete BEFORE DELETE ON external_ticket_observations
BEGIN SELECT RAISE(ABORT, 'external ticket observations are append-only'); END;

CREATE TRIGGER ticket_sync_projections_no_update BEFORE UPDATE ON ticket_sync_projections
BEGIN SELECT RAISE(ABORT, 'ticket projection revisions are immutable'); END;
CREATE TRIGGER ticket_sync_projections_no_delete BEFORE DELETE ON ticket_sync_projections
BEGIN SELECT RAISE(ABORT, 'ticket projection revisions are immutable'); END;

CREATE TRIGGER calendar_exceptions_no_update BEFORE UPDATE ON calendar_exceptions
BEGIN SELECT RAISE(ABORT, 'calendar exception revisions are append-only'); END;
CREATE TRIGGER calendar_exceptions_no_delete BEFORE DELETE ON calendar_exceptions
BEGIN SELECT RAISE(ABORT, 'calendar exception revisions are append-only'); END;

CREATE TRIGGER execution_authorizations_no_update BEFORE UPDATE ON execution_authorizations
BEGIN SELECT RAISE(ABORT, 'execution authorizations are immutable'); END;
CREATE TRIGGER execution_authorizations_no_delete BEFORE DELETE ON execution_authorizations
BEGIN SELECT RAISE(ABORT, 'execution authorizations are immutable'); END;

-- A pinned work-profile snapshot never changes. Phase and revision may advance.
CREATE TRIGGER task_workflows_snapshot_immutable BEFORE UPDATE ON task_workflows
WHEN OLD.snapshot <> NEW.snapshot
     OR OLD.snapshot_hash <> NEW.snapshot_hash
     OR OLD.profile_key <> NEW.profile_key
     OR OLD.profile_version <> NEW.profile_version
     OR OLD.task_id <> NEW.task_id
     OR OLD.project_id <> NEW.project_id
BEGIN SELECT RAISE(ABORT, 'a pinned work profile snapshot is immutable'); END;

CREATE TRIGGER task_workflows_no_delete BEFORE DELETE ON task_workflows
BEGIN SELECT RAISE(ABORT, 'task workflows are not deletable'); END;

-- A terminal task is immutable.
CREATE TRIGGER tasks_terminal_immutable BEFORE UPDATE ON tasks
WHEN OLD.state IN ('done', 'failed', 'cancelled')
BEGIN SELECT RAISE(ABORT, 'a terminal task is immutable'); END;

CREATE TRIGGER tasks_no_delete BEFORE DELETE ON tasks
BEGIN SELECT RAISE(ABORT, 'tasks are not deletable'); END;

-- A closed run never reopens, and its evidence, parent and closure time never
-- change.
CREATE TRIGGER team_runs_terminal_immutable BEFORE UPDATE ON team_runs
WHEN OLD.lifecycle IN ('succeeded', 'failed', 'cancelled', 'parked')
BEGIN SELECT RAISE(ABORT, 'a closed team run is immutable and cannot reopen'); END;

CREATE TRIGGER team_runs_evidence_immutable BEFORE UPDATE ON team_runs
WHEN OLD.terminal_source_kind IS NOT NULL
     AND (OLD.terminal_source_kind IS NOT NEW.terminal_source_kind
          OR OLD.terminal_receipt_id IS NOT NEW.terminal_receipt_id
          OR OLD.terminal_evidence_hash IS NOT NEW.terminal_evidence_hash
          OR OLD.closed_at IS NOT NEW.closed_at)
BEGIN SELECT RAISE(ABORT, 'team closure evidence is immutable'); END;

CREATE TRIGGER team_runs_no_delete BEFORE DELETE ON team_runs
BEGIN SELECT RAISE(ABORT, 'team runs are not deletable'); END;

-- The team definition a run started with is frozen for the life of the run,
-- exactly like the work-profile snapshot on task_workflows. Later edits to the
-- template create a new revision; they never reach a run already using it.
CREATE TRIGGER team_runs_snapshot_immutable BEFORE UPDATE ON team_runs
WHEN OLD.snapshot <> NEW.snapshot
     OR OLD.snapshot_hash <> NEW.snapshot_hash
     OR OLD.template_id <> NEW.template_id
     OR OLD.template_version <> NEW.template_version
     OR OLD.project_id <> NEW.project_id
     OR OLD.task_id <> NEW.task_id
     OR OLD.created_at <> NEW.created_at
BEGIN SELECT RAISE(ABORT, 'a pinned team snapshot is immutable'); END;

CREATE TRIGGER agent_runs_terminal_immutable BEFORE UPDATE ON agent_runs
WHEN OLD.lifecycle IN ('succeeded', 'failed', 'cancelled', 'parked')
BEGIN SELECT RAISE(ABORT, 'a closed agent run is immutable and cannot reopen'); END;

CREATE TRIGGER agent_runs_lineage_immutable BEFORE UPDATE ON agent_runs
WHEN OLD.parent_agent_run_id IS NOT NEW.parent_agent_run_id
     OR OLD.team_run_id <> NEW.team_run_id
     OR OLD.project_id <> NEW.project_id
     OR OLD.created_at <> NEW.created_at
BEGIN SELECT RAISE(ABORT, 'agent run lineage is immutable'); END;

CREATE TRIGGER agent_runs_no_delete BEFORE DELETE ON agent_runs
BEGIN SELECT RAISE(ABORT, 'agent runs are not deletable'); END;

-- A binding names one native session for the life of its run.
CREATE TRIGGER runtime_bindings_no_update BEFORE UPDATE ON runtime_bindings
BEGIN SELECT RAISE(ABORT, 'a runtime binding is immutable'); END;
CREATE TRIGGER runtime_bindings_no_delete BEFORE DELETE ON runtime_bindings
BEGIN SELECT RAISE(ABORT, 'a runtime binding is immutable'); END;

-- A conflict keeps the inputs that produced it; only the resolution is appended,
-- and only once.
CREATE TRIGGER status_conflicts_inputs_immutable BEFORE UPDATE ON status_conflicts
WHEN OLD.resolved_at IS NOT NULL
     OR OLD.kind <> NEW.kind
     OR OLD.observation_id <> NEW.observation_id
     OR OLD.task_revision <> NEW.task_revision
     OR OLD.spec_version <> NEW.spec_version
     OR OLD.link_id <> NEW.link_id
     OR OLD.detected_at <> NEW.detected_at
BEGIN SELECT RAISE(ABORT, 'a status conflict keeps its original inputs'); END;

CREATE TRIGGER status_conflicts_no_delete BEFORE DELETE ON status_conflicts
BEGIN SELECT RAISE(ABORT, 'status conflicts are not deletable'); END;

-- A transition receipt records what was dispatched; only acknowledgement and
-- confirmation are appended.
CREATE TRIGGER status_transition_receipts_immutable BEFORE UPDATE ON status_transition_receipts
WHEN OLD.link_id <> NEW.link_id
     OR OLD.task_id <> NEW.task_id
     OR OLD.plan <> NEW.plan
     OR OLD.target_status_id <> NEW.target_status_id
     OR OLD.transition_id IS NOT NEW.transition_id
     OR OLD.idempotency_key <> NEW.idempotency_key
     OR OLD.dispatched_at <> NEW.dispatched_at
     OR OLD.prior_observation_id <> NEW.prior_observation_id
BEGIN SELECT RAISE(ABORT, 'a dispatched transition receipt is immutable'); END;

CREATE TRIGGER status_transition_receipts_no_delete BEFORE DELETE ON status_transition_receipts
BEGIN SELECT RAISE(ABORT, 'transition receipts are not deletable'); END;

-- A command receipt's identity and intent are fixed at creation; reusing the key
-- for a different command is impossible rather than merely discouraged.
CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key
     OR OLD.target <> NEW.target
     OR OLD.intent <> NEW.intent
     OR OLD.intent_hash <> NEW.intent_hash
     OR OLD.kind <> NEW.kind
     OR OLD.project_id <> NEW.project_id
     OR OLD.state IN ('confirmed', 'failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;

CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

CREATE TRIGGER command_targets_no_update BEFORE UPDATE ON command_targets
BEGIN SELECT RAISE(ABORT, 'a command target is fixed at intent'); END;
CREATE TRIGGER command_targets_no_delete BEFORE DELETE ON command_targets
BEGIN SELECT RAISE(ABORT, 'a command target is fixed at intent'); END;

CREATE TRIGGER execution_authorization_tasks_no_update
BEFORE UPDATE ON execution_authorization_tasks
BEGIN SELECT RAISE(ABORT, 'an authorization task set is immutable'); END;
CREATE TRIGGER execution_authorization_tasks_no_delete
BEFORE DELETE ON execution_authorization_tasks
BEGIN SELECT RAISE(ABORT, 'an authorization task set is immutable'); END;

CREATE TRIGGER command_outbox_payload_immutable BEFORE UPDATE ON command_outbox
WHEN OLD.payload <> NEW.payload
     OR OLD.payload_hash <> NEW.payload_hash
     OR OLD.receipt_id <> NEW.receipt_id
BEGIN SELECT RAISE(ABORT, 'an outbox payload is immutable'); END;

-- A revocation is recorded once and never edited away.
CREATE TRIGGER schedule_overrides_revocation_immutable BEFORE UPDATE ON schedule_overrides
WHEN OLD.revoked_at IS NOT NULL
     OR OLD.hard_ceiling <> NEW.hard_ceiling
     OR OLD.scope_kind <> NEW.scope_kind
     OR OLD.start_at <> NEW.start_at
     OR OLD.approval_receipt_id <> NEW.approval_receipt_id
BEGIN SELECT RAISE(ABORT, 'a schedule override scope, ceiling and revocation are immutable'); END;

CREATE TRIGGER schedule_overrides_no_delete BEFORE DELETE ON schedule_overrides
BEGIN SELECT RAISE(ABORT, 'schedule overrides are not deletable'); END;

-- A retired calendar assignment stays retired; a pinned profile revision is
-- never silently upgraded in place.
CREATE TRIGGER work_calendars_pin_immutable BEFORE UPDATE ON work_calendars
WHEN OLD.profile_id <> NEW.profile_id
     OR OLD.profile_version <> NEW.profile_version
     OR OLD.project_id <> NEW.project_id
     OR (OLD.active = 0 AND NEW.active = 1)
BEGIN SELECT RAISE(ABORT, 'a pinned calendar profile revision is immutable'); END;

CREATE TRIGGER work_calendars_no_delete BEFORE DELETE ON work_calendars
BEGIN SELECT RAISE(ABORT, 'calendar assignments are retired, never deleted'); END;

-- A released lease stays released.
CREATE TRIGGER resource_leases_release_immutable BEFORE UPDATE ON resource_leases
WHEN OLD.released_at IS NOT NULL
     OR OLD.resource_key <> NEW.resource_key
     OR OLD.agent_run_id <> NEW.agent_run_id
BEGIN SELECT RAISE(ABORT, 'a released lease is immutable'); END;

PRAGMA user_version = 1;
