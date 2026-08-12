-- Schema v8 — immutable child-scope calendar window revisions (KON-MVP-21).
--
-- A project calendar remains the authority. These rows only narrow it for one
-- mini-project or task; the resolver proves that relationship and requires a
-- scoped approved override for a widening. Current state is the unsuperseded
-- leaf, so history never needs an UPDATE.

CREATE TABLE child_calendar_windows (
    project_id       TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    work_calendar_id TEXT    NOT NULL,
    scope_kind       TEXT    NOT NULL CHECK (scope_kind IN ('mini_project', 'task')),
    mini_project_id  TEXT    NULL,
    task_id          TEXT    NULL,
    version          INTEGER NOT NULL CHECK (version >= 1),
    windows          TEXT    NOT NULL CHECK (json_valid(windows) AND json_type(windows) = 'array'),
    supersedes       INTEGER NULL CHECK (supersedes IS NULL OR supersedes >= 1),
    created_at       TEXT    NOT NULL
                              CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    CHECK ((scope_kind = 'mini_project') = (mini_project_id IS NOT NULL)),
    CHECK ((scope_kind = 'task') = (task_id IS NOT NULL)),
    CHECK ((supersedes IS NULL AND version = 1) OR supersedes + 1 = version),
    UNIQUE (project_id, work_calendar_id, scope_kind, mini_project_id, task_id, version),
    FOREIGN KEY (project_id, work_calendar_id)
        REFERENCES work_calendars (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_child_calendar_windows_scope
    ON child_calendar_windows
       (project_id, work_calendar_id, scope_kind, mini_project_id, task_id, version);

CREATE UNIQUE INDEX ux_child_calendar_windows_goal_version
    ON child_calendar_windows (project_id, work_calendar_id, mini_project_id, version)
    WHERE scope_kind = 'mini_project';

CREATE UNIQUE INDEX ux_child_calendar_windows_task_version
    ON child_calendar_windows (project_id, work_calendar_id, task_id, version)
    WHERE scope_kind = 'task';

CREATE TRIGGER child_calendar_windows_supersede_current BEFORE INSERT ON child_calendar_windows
WHEN NEW.supersedes IS NOT (
    SELECT current.version FROM child_calendar_windows AS current
     WHERE current.project_id = NEW.project_id
       AND current.work_calendar_id = NEW.work_calendar_id
       AND current.scope_kind = NEW.scope_kind
       AND current.mini_project_id IS NEW.mini_project_id
       AND current.task_id IS NEW.task_id
       AND NOT EXISTS (SELECT 1 FROM child_calendar_windows AS later
                        WHERE later.project_id = current.project_id
                          AND later.work_calendar_id = current.work_calendar_id
                          AND later.scope_kind = current.scope_kind
                          AND later.mini_project_id IS current.mini_project_id
                          AND later.task_id IS current.task_id
                          AND later.supersedes = current.version)
)
BEGIN SELECT RAISE(ABORT, 'child windows must supersede the current revision'); END;

CREATE TRIGGER child_calendar_windows_no_update BEFORE UPDATE ON child_calendar_windows
BEGIN SELECT RAISE(ABORT, 'child calendar windows are immutable'); END;

CREATE TRIGGER child_calendar_windows_no_delete BEFORE DELETE ON child_calendar_windows
BEGIN SELECT RAISE(ABORT, 'child calendar windows are not deletable'); END;

PRAGMA user_version = 8;
