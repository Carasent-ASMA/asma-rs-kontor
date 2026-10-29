-- Catalog schema v2: read-only Jira observation evidence.
--
-- Jira evidence has its own generation, page checkpoints and coverage. It
-- never shares the local discovery generation: a local sweep failure must not
-- move Jira coverage, and a Jira outage must not hide a local fact. The
-- `catalog_*` tables from v1 are untouched by this migration.
--
-- Removal is only ever written by a complete relevant census. A 404, a failed
-- page, a rate limit or permission loss is stored as uncertainty, never as
-- deletion, and a tombstone is a retained row rather than a physical delete.

CREATE TABLE catalog_jira_scope (
    scope             TEXT    NOT NULL PRIMARY KEY,
    generation        INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
    active_generation INTEGER NOT NULL DEFAULT 0 CHECK (active_generation >= 0),
    last_order        INTEGER NOT NULL DEFAULT 0 CHECK (last_order >= 0),
    updated_at        TEXT    NOT NULL
                              CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

CREATE TABLE catalog_jira_sweeps (
    scope            TEXT    NOT NULL,
    generation       INTEGER NOT NULL CHECK (generation >= 1),
    mode             TEXT    NOT NULL CHECK (mode IN ('full', 'incremental')),
    state            TEXT    NOT NULL
                             CHECK (state IN ('running', 'complete', 'incomplete', 'failed')),
    started_at       TEXT    NOT NULL
                             CHECK (started_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    finished_at      TEXT    NULL
                             CHECK (finished_at IS NULL
                                    OR finished_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    window_start     TEXT    NULL
                             CHECK (window_start IS NULL
                                    OR window_start GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    window_end       TEXT    NULL
                             CHECK (window_end IS NULL
                                    OR window_end GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    order_value      INTEGER NOT NULL CHECK (order_value >= 1),
    pages_committed  INTEGER NOT NULL DEFAULT 0 CHECK (pages_committed >= 0),
    issues_seen      INTEGER NOT NULL DEFAULT 0 CHECK (issues_seen >= 0),
    requests         INTEGER NOT NULL DEFAULT 0 CHECK (requests >= 0),
    retries          INTEGER NOT NULL DEFAULT 0 CHECK (retries >= 0),
    error_kind       TEXT    NULL
                             CHECK (error_kind IS NULL
                                    OR error_kind IN ('credential', 'permission', 'not_found',
                                                      'rate_limited', 'timeout', 'transport',
                                                      'oversized', 'malformed', 'rejected',
                                                      'bounded', 'configuration')),
    error            TEXT    NULL CHECK (error IS NULL OR length(error) <= 4096),
    coverage_json    TEXT    NOT NULL DEFAULT '{}',
    diagnostics_json TEXT    NOT NULL DEFAULT '[]',
    PRIMARY KEY (scope, generation)
) STRICT;

CREATE TABLE catalog_jira_sweep_pages (
    scope             TEXT    NOT NULL,
    generation        INTEGER NOT NULL,
    page_index        INTEGER NOT NULL CHECK (page_index >= 0),
    observed_at       TEXT    NOT NULL
                              CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    observations_json TEXT    NOT NULL,
    PRIMARY KEY (scope, generation, page_index),
    FOREIGN KEY (scope, generation) REFERENCES catalog_jira_sweeps (scope, generation)
        ON DELETE CASCADE
) STRICT;

CREATE TABLE catalog_jira_observations (
    scope                 TEXT    NOT NULL,
    issue_id              TEXT    NOT NULL
                                  CHECK (issue_id NOT GLOB '*[^0-9]*'
                                         AND length(issue_id) BETWEEN 1 AND 20),
    current_key           TEXT    NOT NULL CHECK (length(current_key) BETWEEN 1 AND 64),
    issue_type            TEXT    NOT NULL CHECK (length(issue_type) BETWEEN 1 AND 80),
    hierarchy_level       INTEGER NOT NULL CHECK (hierarchy_level BETWEEN -1 AND 9),
    parent_key            TEXT    NULL CHECK (parent_key IS NULL OR length(parent_key) BETWEEN 1 AND 64),
    parent_id             TEXT    NULL
                                  CHECK (parent_id IS NULL
                                         OR (parent_id NOT GLOB '*[^0-9]*'
                                             AND length(parent_id) BETWEEN 1 AND 20)),
    labels_json           TEXT    NOT NULL,
    status                TEXT    NOT NULL CHECK (length(status) BETWEEN 1 AND 160),
    status_category       TEXT    NOT NULL CHECK (status_category IN ('new', 'indeterminate', 'done')),
    updated_at            TEXT    NOT NULL,
    summary               TEXT    NOT NULL CHECK (length(summary) <= 1000),
    observed_at           TEXT    NOT NULL,
    via                   TEXT    NOT NULL CHECK (via IN ('scope', 'declared', 'parent')),
    first_seen            TEXT    NOT NULL,
    last_seen             TEXT    NOT NULL,
    first_seen_generation INTEGER NOT NULL DEFAULT 0 CHECK (first_seen_generation >= 0),
    last_seen_generation  INTEGER NOT NULL DEFAULT 0 CHECK (last_seen_generation >= 0),
    -- NULL while an observation is staged by an incomplete sweep. Reads only
    -- see rows at the active generation, so an incomplete sweep cannot replace
    -- or erase a complete generation's answer.
    visible_generation    INTEGER NULL,
    state                 TEXT    NOT NULL CHECK (state IN ('active', 'removed')),
    removed_at            TEXT    NULL
                                  CHECK (removed_at IS NULL
                                         OR removed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    removed_reason        TEXT    NULL
                                  CHECK (removed_reason IS NULL
                                         OR removed_reason IN ('out_of_scope', 'not_relevant')),
    PRIMARY KEY (scope, issue_id)
) STRICT;

CREATE INDEX catalog_jira_observations_by_key
    ON catalog_jira_observations (scope, current_key);

CREATE TABLE catalog_jira_aliases (
    scope              TEXT    NOT NULL,
    alias_key          TEXT    NOT NULL CHECK (length(alias_key) BETWEEN 1 AND 64),
    issue_id           TEXT    NOT NULL,
    current_key        TEXT    NOT NULL CHECK (length(current_key) BETWEEN 1 AND 64),
    first_seen         TEXT    NOT NULL,
    last_seen          TEXT    NOT NULL,
    visible_generation INTEGER NULL,
    PRIMARY KEY (scope, alias_key),
    FOREIGN KEY (scope, issue_id)
        REFERENCES catalog_jira_observations (scope, issue_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE catalog_jira_collection (
    scope                TEXT    NOT NULL PRIMARY KEY,
    generation           INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
    attempt_generation   INTEGER NOT NULL DEFAULT 0 CHECK (attempt_generation >= 0),
    state                TEXT    NOT NULL
                                 CHECK (state IN ('never_collected', 'complete',
                                                  'incomplete', 'failed')),
    mode                 TEXT    NOT NULL CHECK (mode IN ('full', 'incremental')),
    started_at           TEXT    NULL,
    finished_at          TEXT    NULL,
    last_complete_at     TEXT    NULL,
    last_full_sweep_at   TEXT    NULL,
    next_full_sweep_at   TEXT    NULL,
    order_value          INTEGER NOT NULL DEFAULT 0 CHECK (order_value >= 0),
    pages                INTEGER NOT NULL DEFAULT 0 CHECK (pages >= 0),
    issues_seen          INTEGER NOT NULL DEFAULT 0 CHECK (issues_seen >= 0),
    requests             INTEGER NOT NULL DEFAULT 0 CHECK (requests >= 0),
    retries              INTEGER NOT NULL DEFAULT 0 CHECK (retries >= 0),
    permission_loss      INTEGER NOT NULL DEFAULT 0 CHECK (permission_loss IN (0, 1)),
    credential_failure   INTEGER NOT NULL DEFAULT 0 CHECK (credential_failure IN (0, 1)),
    backoff_until        TEXT    NULL,
    overdue              INTEGER NOT NULL DEFAULT 0 CHECK (overdue IN (0, 1)),
    coverage_json        TEXT    NOT NULL DEFAULT '{}',
    diagnostics_json     TEXT    NOT NULL DEFAULT '[]',
    error_kind           TEXT    NULL
                                 CHECK (error_kind IS NULL
                                        OR error_kind IN ('credential', 'permission', 'not_found',
                                                          'rate_limited', 'timeout', 'transport',
                                                          'oversized', 'malformed', 'rejected',
                                                          'bounded', 'configuration')),
    error                TEXT    NULL CHECK (error IS NULL OR length(error) <= 4096)
) STRICT;

PRAGMA user_version = 2;
