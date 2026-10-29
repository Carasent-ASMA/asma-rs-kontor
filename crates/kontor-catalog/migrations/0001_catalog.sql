-- Catalog schema v1. This file is the catalog's own database, separate from
-- `kontor.db`: an observation outage must never touch governed state, and a
-- governed migration must never rewrite an observation.
--
-- The schema separates the durable *work identity* from the *sweep that
-- observed it*. A page commit stages what a scan saw; only a complete sweep
-- promotes staged observations and tombstones what a complete sweep proved
-- absent. A partial sweep therefore cannot erase a fact, and a crash between
-- pages leaves the last complete generation untouched.

CREATE TABLE catalog_meta (
    key   TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

-- One row per catalog scope. Today the host is one scope; the column exists so
-- a later standalone host is a new row rather than a second table.
CREATE TABLE catalog_scope (
    scope             TEXT    NOT NULL PRIMARY KEY,
    catalog_id        TEXT    NOT NULL,
    owner_id          TEXT    NULL CHECK (owner_id IS NULL OR length(owner_id) BETWEEN 1 AND 128),
    lease_until       TEXT    NULL
                              CHECK (lease_until IS NULL
                                     OR lease_until GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- The most recent generation *attempted*, and the most recent generation
    -- that completed. Entries at generation zero have never been observed.
    generation        INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
    active_generation INTEGER NOT NULL DEFAULT 0 CHECK (active_generation >= 0),
    -- The effective registry projection from the last complete sweep.
    registry_json     TEXT    NULL,
    updated_at        TEXT    NOT NULL
                              CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

-- Imported configuration provenance. The operator-owned file is
-- `<state_root>/catalog.json`; this row records what was imported, from where
-- and when, so a migrated launchd environment is visible rather than guessed.
CREATE TABLE catalog_config (
    singleton      INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    origin         TEXT    NOT NULL
                           CHECK (origin IN ('catalog_file', 'migrated_plugin_env',
                                             'migrated_plugin_file', 'none')),
    revision       TEXT    NULL CHECK (revision IS NULL OR length(revision) = 64),
    imported_at    TEXT    NOT NULL
                           CHECK (imported_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    settings_json  TEXT    NOT NULL
) STRICT;

-- One attempted sweep. `complete` is the only state that may promote
-- observations or tombstone absence.
CREATE TABLE catalog_sweeps (
    scope           TEXT    NOT NULL,
    generation      INTEGER NOT NULL CHECK (generation >= 1),
    owner_id        TEXT    NOT NULL,
    state           TEXT    NOT NULL
                            CHECK (state IN ('running', 'complete', 'incomplete', 'failed')),
    started_at      TEXT    NOT NULL
                            CHECK (started_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    finished_at     TEXT    NULL
                            CHECK (finished_at IS NULL
                                   OR finished_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    pages_committed INTEGER NOT NULL DEFAULT 0 CHECK (pages_committed >= 0),
    files_seen      INTEGER NOT NULL DEFAULT 0 CHECK (files_seen >= 0),
    entries_seen    INTEGER NOT NULL DEFAULT 0 CHECK (entries_seen >= 0),
    error           TEXT    NULL CHECK (error IS NULL OR length(error) <= 4096),
    coverage_json   TEXT    NOT NULL DEFAULT '[]',
    diagnostics_json TEXT   NOT NULL DEFAULT '[]',
    PRIMARY KEY (scope, generation)
) STRICT;

-- A committed page is durable observation evidence even when the sweep never
-- completes. It is never read as scheduler or completion truth; it is the unit
-- a crash can leave behind without inventing deletion.
CREATE TABLE catalog_sweep_pages (
    scope      TEXT    NOT NULL,
    generation INTEGER NOT NULL,
    page_index INTEGER NOT NULL CHECK (page_index >= 0),
    observed_at TEXT   NOT NULL
                       CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    entries_json TEXT  NOT NULL,
    files_seen   INTEGER NOT NULL DEFAULT 0 CHECK (files_seen >= 0),
    entries_seen INTEGER NOT NULL DEFAULT 0 CHECK (entries_seen >= 0),
    PRIMARY KEY (scope, generation, page_index),
    FOREIGN KEY (scope, generation) REFERENCES catalog_sweeps (scope, generation)
        ON DELETE CASCADE
) STRICT;

CREATE TABLE catalog_logical_plans (
    logical_plan_id   TEXT    NOT NULL PRIMARY KEY,
    kind              TEXT    NOT NULL CHECK (kind IN ('plan', 'execution_unit')),
    title             TEXT    NOT NULL,
    state             TEXT    NOT NULL CHECK (state IN ('active', 'removed')),
    first_seen        TEXT    NOT NULL,
    last_seen         TEXT    NOT NULL,
    first_seen_generation INTEGER NOT NULL DEFAULT 0,
    last_seen_generation  INTEGER NOT NULL DEFAULT 0,
    -- NULL while an observation is staged by an incomplete sweep. Reads only
    -- see rows at the active generation, so a partial scan cannot replace a
    -- complete generation's answer.
    visible_generation INTEGER NULL,
    -- NULL while active. A removed plan keeps its row, its aliases and its
    -- associations: history is not deletion.
    removed_at        TEXT    NULL
) STRICT;

CREATE TABLE catalog_aliases (
    logical_plan_id TEXT NOT NULL REFERENCES catalog_logical_plans (logical_plan_id)
                        ON DELETE RESTRICT,
    alias_kind      TEXT NOT NULL CHECK (alias_kind IN ('renamed_from', 'source_id', 'explicit_id')),
    alias_value     TEXT NOT NULL,
    observed_at     TEXT NOT NULL,
    PRIMARY KEY (logical_plan_id, alias_kind, alias_value)
) STRICT;

CREATE TABLE catalog_content_revisions (
    revision_id   TEXT    NOT NULL PRIMARY KEY CHECK (length(revision_id) = 64),
    byte_length   INTEGER NOT NULL CHECK (byte_length >= 0),
    modified_at   TEXT    NOT NULL,
    title         TEXT    NOT NULL,
    epic          TEXT    NULL,
    workstream    TEXT    NULL,
    first_seen    TEXT    NOT NULL,
    last_seen     TEXT    NOT NULL
) STRICT;

CREATE TABLE catalog_occurrences (
    occurrence_id  TEXT    NOT NULL PRIMARY KEY,
    logical_plan_id TEXT   NOT NULL REFERENCES catalog_logical_plans (logical_plan_id)
                               ON DELETE RESTRICT,
    repository_id  TEXT    NOT NULL,
    repository_root TEXT   NOT NULL,
    relative_path  TEXT    NOT NULL,
    content_kind   TEXT    NOT NULL
                           CHECK (content_kind IN ('tracked', 'untracked_draft', 'external_source')),
    revision_kind  TEXT    NOT NULL
                           CHECK (revision_kind IN ('working_copy', 'saved_draft',
                                                    'approved_revision')),
    revision_id    TEXT    NOT NULL REFERENCES catalog_content_revisions (revision_id)
                               ON DELETE RESTRICT,
    expected_sha256 TEXT   NULL CHECK (expected_sha256 IS NULL OR length(expected_sha256) = 64),
    state          TEXT    NOT NULL CHECK (state IN ('active', 'removed')),
    first_seen     TEXT    NOT NULL,
    last_seen      TEXT    NOT NULL,
    first_seen_generation INTEGER NOT NULL DEFAULT 0,
    last_seen_generation  INTEGER NOT NULL DEFAULT 0,
    visible_generation INTEGER NULL,
    removed_at     TEXT    NULL,
    UNIQUE (repository_id, repository_root, relative_path, content_kind)
) STRICT;

CREATE INDEX catalog_occurrences_by_plan ON catalog_occurrences (logical_plan_id);

CREATE TABLE catalog_occurrence_sources (
    occurrence_id TEXT NOT NULL REFERENCES catalog_occurrences (occurrence_id)
                       ON DELETE RESTRICT,
    source_id     TEXT NOT NULL,
    PRIMARY KEY (occurrence_id, source_id)
) STRICT;

-- Explicit operator associations and plan declarations share one store but
-- never one authority. `verified` is always 0 in TASK-001: only a later,
-- independent Jira observation may set it.
CREATE TABLE catalog_associations (
    association_id  TEXT    NOT NULL PRIMARY KEY,
    logical_plan_id TEXT    REFERENCES catalog_logical_plans (logical_plan_id)
                            ON DELETE RESTRICT,
    relation        TEXT    NOT NULL CHECK (relation IN ('workstream', 'epic', 'project')),
    value           TEXT    NOT NULL,
    association_kind TEXT   NOT NULL
                            CHECK (association_kind IN ('explicit_operator', 'plan_declaration')),
    source_id       TEXT    NULL,
    verified        INTEGER NOT NULL DEFAULT 0 CHECK (verified IN (0, 1)),
    dangling        INTEGER NOT NULL DEFAULT 0 CHECK (dangling IN (0, 1)),
    note            TEXT    NULL CHECK (note IS NULL OR length(note) <= 512),
    first_seen      TEXT    NOT NULL,
    last_seen       TEXT    NOT NULL,
    first_seen_generation INTEGER NOT NULL DEFAULT 0,
    last_seen_generation  INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX catalog_associations_by_plan ON catalog_associations (logical_plan_id);

CREATE TABLE catalog_diagnostics (
    diagnostic_id TEXT NOT NULL PRIMARY KEY,
    scope         TEXT NOT NULL,
    generation    INTEGER NOT NULL,
    code          TEXT NOT NULL,
    severity      TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'error')),
    message       TEXT NOT NULL CHECK (length(message) <= 4096),
    subject       TEXT NULL CHECK (subject IS NULL OR length(subject) <= 1024),
    logical_plan_id TEXT NULL,
    observed_at   TEXT NOT NULL
) STRICT;

CREATE INDEX catalog_diagnostics_by_generation ON catalog_diagnostics (scope, generation);

CREATE TABLE catalog_conflicts (
    conflict_id   TEXT NOT NULL PRIMARY KEY,
    scope         TEXT NOT NULL,
    generation    INTEGER NOT NULL,
    kind          TEXT NOT NULL
                       CHECK (kind IN ('divergent_worktree', 'ambiguous_rename',
                                       'duplicate_explicit_id', 'duplicate_epic',
                                       'pin_mismatch', 'association_conflict')),
    logical_plan_id TEXT NULL,
    message       TEXT NOT NULL CHECK (length(message) <= 4096),
    observed_at   TEXT NOT NULL
) STRICT;

CREATE INDEX catalog_conflicts_by_generation ON catalog_conflicts (scope, generation);

-- The last complete collection's counters, kept separately from the sweep row
-- so a failed or incomplete attempt cannot make status look fresher.
CREATE TABLE catalog_collection (
    scope             TEXT    NOT NULL PRIMARY KEY,
    generation        INTEGER NOT NULL,
    attempt_generation INTEGER NOT NULL DEFAULT 0,
    state             TEXT    NOT NULL
                              CHECK (state IN ('never_collected', 'complete',
                                               'incomplete', 'failed')),
    started_at        TEXT    NULL,
    finished_at       TEXT    NULL,
    duration_ms       INTEGER NULL CHECK (duration_ms IS NULL OR duration_ms >= 0),
    repositories_configured INTEGER NOT NULL DEFAULT 0,
    repositories_scanned    INTEGER NOT NULL DEFAULT 0,
    files_seen        INTEGER NOT NULL DEFAULT 0,
    entries_observed  INTEGER NOT NULL DEFAULT 0,
    error             TEXT    NULL CHECK (error IS NULL OR length(error) <= 4096),
    coverage_json     TEXT    NOT NULL DEFAULT '[]',
    registry_json     TEXT    NULL
) STRICT;

PRAGMA user_version = 1;
