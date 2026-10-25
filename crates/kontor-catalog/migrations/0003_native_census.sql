-- Catalog schema v3: the headless native runtime census.
--
-- The census is observational and separate from plan discovery. It shares the
-- catalog scope, the collector lease and the one SQLite file, but it has its
-- own attempt generation, pages, promoted tables and collection row. A Paseo
-- outage must never make local plan discovery look incomplete, and a plan
-- sweep must never advance native freshness.
--
-- Native identity is explicit: every row is keyed by the runtime instance and
-- the native id. A title is display data and is never a key, so two sessions
-- with one title remain two sessions.

ALTER TABLE catalog_scope
    ADD COLUMN native_generation INTEGER NOT NULL DEFAULT 0
        CHECK (native_generation >= 0);
ALTER TABLE catalog_scope
    ADD COLUMN native_active_generation INTEGER NOT NULL DEFAULT 0
        CHECK (native_active_generation >= 0);

-- One native census attempt. Only a complete attempt may promote observations
-- or prove absence. An incomplete attempt can insert a previously unknown row
-- hidden from readers, exactly as the plan protocol does.
CREATE TABLE catalog_native_sweeps (
    scope                 TEXT    NOT NULL,
    generation            INTEGER NOT NULL CHECK (generation >= 1),
    owner_id              TEXT    NOT NULL,
    state                 TEXT    NOT NULL
                                  CHECK (state IN ('running', 'complete', 'incomplete', 'failed')),
    started_at            TEXT    NOT NULL
                                  CHECK (started_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    finished_at           TEXT    NULL
                                  CHECK (finished_at IS NULL
                                         OR finished_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    pages_committed       INTEGER NOT NULL DEFAULT 0 CHECK (pages_committed >= 0),
    projects_seen         INTEGER NOT NULL DEFAULT 0 CHECK (projects_seen >= 0),
    workspaces_seen       INTEGER NOT NULL DEFAULT 0 CHECK (workspaces_seen >= 0),
    sessions_seen         INTEGER NOT NULL DEFAULT 0 CHECK (sessions_seen >= 0),
    archived_sessions_seen INTEGER NOT NULL DEFAULT 0 CHECK (archived_sessions_seen >= 0),
    runtimes_json         TEXT    NOT NULL DEFAULT '[]',
    diagnostics_json      TEXT    NOT NULL DEFAULT '[]',
    error                 TEXT    NULL CHECK (error IS NULL OR length(error) <= 4096),
    PRIMARY KEY (scope, generation)
) STRICT;

-- A committed page is durable census evidence, including per-runtime coverage
-- and the exact page boundary. It is never read as authority; promotion is.
CREATE TABLE catalog_native_sweep_pages (
    scope        TEXT    NOT NULL,
    generation   INTEGER NOT NULL,
    page_index   INTEGER NOT NULL CHECK (page_index >= 0),
    kind         TEXT    NOT NULL CHECK (kind IN ('projects', 'workspaces', 'sessions')),
    observed_at  TEXT    NOT NULL
                         CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    page_json    TEXT    NOT NULL,
    items_seen   INTEGER NOT NULL DEFAULT 0 CHECK (items_seen >= 0),
    PRIMARY KEY (scope, generation, page_index),
    FOREIGN KEY (scope, generation) REFERENCES catalog_native_sweeps (scope, generation)
        ON DELETE CASCADE
) STRICT;

CREATE TABLE catalog_native_projects (
    runtime_instance      TEXT    NOT NULL,
    project_id            TEXT    NOT NULL,
    display_name          TEXT    NOT NULL,
    custom_name           TEXT    NULL,
    project_key           TEXT    NULL,
    root_path             TEXT    NOT NULL,
    project_kind          TEXT    NULL,
    state                 TEXT    NOT NULL CHECK (state IN ('active', 'removed')),
    first_seen            TEXT    NOT NULL,
    last_seen             TEXT    NOT NULL,
    first_seen_generation INTEGER NOT NULL DEFAULT 0,
    last_seen_generation  INTEGER NOT NULL DEFAULT 0,
    visible_generation    INTEGER NULL,
    removed_at            TEXT    NULL,
    PRIMARY KEY (runtime_instance, project_id)
) STRICT;

CREATE TABLE catalog_native_workspaces (
    runtime_instance      TEXT    NOT NULL,
    workspace_id          TEXT    NOT NULL,
    project_id            TEXT    NOT NULL,
    name                  TEXT    NOT NULL,
    title                 TEXT    NULL,
    directory             TEXT    NOT NULL,
    kind                  TEXT    NOT NULL,
    archiving_at          TEXT    NULL,
    worktree_slug         TEXT    NULL,
    project_display_name  TEXT    NULL,
    project_custom_name   TEXT    NULL,
    project_root_path     TEXT    NULL,
    project_kind          TEXT    NULL,
    state                 TEXT    NOT NULL CHECK (state IN ('active', 'removed')),
    first_seen            TEXT    NOT NULL,
    last_seen             TEXT    NOT NULL,
    first_seen_generation INTEGER NOT NULL DEFAULT 0,
    last_seen_generation  INTEGER NOT NULL DEFAULT 0,
    visible_generation    INTEGER NULL,
    removed_at            TEXT    NULL,
    PRIMARY KEY (runtime_instance, workspace_id)
) STRICT;

CREATE INDEX catalog_native_workspaces_by_project
    ON catalog_native_workspaces (runtime_instance, project_id);

CREATE TABLE catalog_native_sessions (
    runtime_instance      TEXT    NOT NULL,
    session_id            TEXT    NOT NULL,
    workspace_id          TEXT    NULL,
    project_id            TEXT    NULL,
    project_name          TEXT    NULL,
    workspace_name        TEXT    NULL,
    title                 TEXT    NULL,
    provider              TEXT    NOT NULL,
    model                 TEXT    NULL,
    status                TEXT    NOT NULL,
    archived_at           TEXT    NULL,
    attention_reason      TEXT    NULL,
    cwd                   TEXT    NOT NULL,
    created_at            TEXT    NULL,
    updated_at            TEXT    NULL,
    last_user_message_at  TEXT    NULL,
    parent_session_id     TEXT    NULL,
    checkout_cwd          TEXT    NULL,
    checkout_branch       TEXT    NULL,
    worktree_root         TEXT    NULL,
    checkout_is_git       INTEGER NULL,
    checkout_is_paseo_owned_worktree INTEGER NULL,
    state                 TEXT    NOT NULL CHECK (state IN ('active', 'removed')),
    first_seen            TEXT    NOT NULL,
    last_seen             TEXT    NOT NULL,
    first_seen_generation INTEGER NOT NULL DEFAULT 0,
    last_seen_generation  INTEGER NOT NULL DEFAULT 0,
    visible_generation    INTEGER NULL,
    removed_at            TEXT    NULL,
    PRIMARY KEY (runtime_instance, session_id)
) STRICT;

CREATE INDEX catalog_native_sessions_by_workspace
    ON catalog_native_sessions (runtime_instance, workspace_id);
CREATE INDEX catalog_native_sessions_by_project
    ON catalog_native_sessions (runtime_instance, project_id);

-- The latest native collection, kept separately from the plan collection so a
-- failed census cannot make plan freshness look newer or vice versa.
CREATE TABLE catalog_native_collection (
    scope                TEXT    NOT NULL PRIMARY KEY,
    generation           INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
    attempt_generation   INTEGER NOT NULL DEFAULT 0 CHECK (attempt_generation >= 0),
    state                TEXT    NOT NULL
                                 CHECK (state IN ('never_collected', 'complete',
                                                  'incomplete', 'failed')),
    started_at           TEXT    NULL,
    finished_at          TEXT    NULL,
    duration_ms          INTEGER NULL CHECK (duration_ms IS NULL OR duration_ms >= 0),
    retry_at             TEXT    NULL,
    runtimes_configured  INTEGER NOT NULL DEFAULT 0 CHECK (runtimes_configured >= 0),
    runtimes_complete    INTEGER NOT NULL DEFAULT 0 CHECK (runtimes_complete >= 0),
    projects_observed    INTEGER NOT NULL DEFAULT 0 CHECK (projects_observed >= 0),
    workspaces_observed  INTEGER NOT NULL DEFAULT 0 CHECK (workspaces_observed >= 0),
    sessions_observed    INTEGER NOT NULL DEFAULT 0 CHECK (sessions_observed >= 0),
    archived_sessions    INTEGER NOT NULL DEFAULT 0 CHECK (archived_sessions >= 0),
    include_archived     INTEGER NOT NULL DEFAULT 0 CHECK (include_archived IN (0, 1)),
    runtimes_json        TEXT    NOT NULL DEFAULT '[]',
    diagnostics_json     TEXT    NOT NULL DEFAULT '[]',
    error                TEXT    NULL CHECK (error IS NULL OR length(error) <= 4096)
) STRICT;

PRAGMA user_version = 3;
