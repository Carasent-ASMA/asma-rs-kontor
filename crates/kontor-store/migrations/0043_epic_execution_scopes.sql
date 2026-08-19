-- The durable runtime-facing identity of an epic.
--
-- A runtime plane may serve several epics in one Kontor project. The external
-- tracker key and short title therefore belong to the epic, not to a
-- process-wide runtime configuration document. They are immutable import facts:
-- a later correction must be an explicit migration rather than a silent
-- re-application under a different native identity.
CREATE TABLE epic_execution_scopes (
    project_id        TEXT NOT NULL,
    mini_project_id   TEXT NOT NULL,
    external_epic_key TEXT NOT NULL CHECK (length(trim(external_epic_key)) > 0),
    short_title       TEXT NOT NULL CHECK (length(trim(short_title)) > 0),
    created_at        TEXT NOT NULL
                           CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, mini_project_id),
    UNIQUE (project_id, external_epic_key),
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER epic_execution_scopes_are_immutable
BEFORE UPDATE ON epic_execution_scopes
BEGIN
    SELECT RAISE(ABORT, 'an epic execution scope is immutable');
END;

CREATE TRIGGER epic_execution_scopes_are_permanent
BEFORE DELETE ON epic_execution_scopes
BEGIN
    SELECT RAISE(ABORT, 'an epic execution scope cannot be deleted');
END;

PRAGMA user_version = 43;
