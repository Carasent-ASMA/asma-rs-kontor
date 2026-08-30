-- Schema v72. One durable Kontor-owned namespace per epic, independent of
-- execution-scope and Jira identity. Legacy native-name tokens remain intact;
-- valid unique values are adopted, while duplicates/invalid values stay
-- readable migration evidence and cannot participate in new materialization.
CREATE TABLE epic_backlog_codes (
    project_id      TEXT NOT NULL,
    mini_project_id TEXT NOT NULL,
    code            TEXT NOT NULL,
    provenance      TEXT NOT NULL CHECK (provenance IN (
                        'automatic', 'manual', 'legacy')),
    status          TEXT NOT NULL CHECK (status IN (
                        'active', 'legacy_duplicate', 'legacy_invalid')),
    assigned_at     TEXT NOT NULL,
    PRIMARY KEY (project_id, mini_project_id, status),
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    CHECK (status <> 'active' OR (
        length(code) BETWEEN 2 AND 32
        AND code NOT GLOB '*[^A-Z0-9]*'
        AND code GLOB '*[A-Z]*'))
) STRICT;

WITH legacy AS (
    SELECT project_id,
           mini_project_id,
           kontor_backlog_code AS code,
           declared_at,
           length(kontor_backlog_code) BETWEEN 2 AND 32
             AND kontor_backlog_code NOT GLOB '*[^A-Z0-9]*'
             AND kontor_backlog_code GLOB '*[A-Z]*' AS canonical,
           count(*) OVER (
               PARTITION BY project_id, lower(kontor_backlog_code)
           ) AS uses
    FROM epic_native_name_tokens
)
INSERT INTO epic_backlog_codes
    (project_id, mini_project_id, code, provenance, status, assigned_at)
SELECT project_id,
       mini_project_id,
       code,
       'legacy',
       CASE
           WHEN NOT canonical THEN 'legacy_invalid'
           WHEN uses > 1 THEN 'legacy_duplicate'
           ELSE 'active'
       END,
       declared_at
FROM legacy;

CREATE UNIQUE INDEX ux_epic_backlog_codes_project_code
ON epic_backlog_codes (project_id, code COLLATE NOCASE)
WHERE status = 'active';

CREATE TRIGGER epic_backlog_codes_are_immutable
BEFORE UPDATE ON epic_backlog_codes
BEGIN SELECT RAISE(ABORT, 'epic backlog codes are immutable'); END;

CREATE TRIGGER epic_backlog_codes_are_permanent
BEFORE DELETE ON epic_backlog_codes
BEGIN SELECT RAISE(ABORT, 'epic backlog codes are permanent'); END;

PRAGMA user_version = 72;
