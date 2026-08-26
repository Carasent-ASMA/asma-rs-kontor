-- Schema v62. Crash-safe, historically exact profile-selection outcomes.
--
-- A local selection command used to persist its receipt before replacing the
-- active workflow. A crash between those writes left a receipt that could not
-- prove which workflow it authorized, while a later valid selection made an
-- old idempotency key project the newer active policy. Bind every new receipt
-- to the exact immutable workflow and specification revisions in the same
-- transaction as the effect. The row is a historical result, not a projection
-- of whichever workflow happens to be active later.
CREATE TABLE profile_selection_outcomes (
    project_id          TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    receipt_id          TEXT    NOT NULL,
    task_id             TEXT    NOT NULL,
    workflow_id         TEXT    NOT NULL,
    profile_key         TEXT    NOT NULL CHECK (length(profile_key) BETWEEN 1 AND 128),
    profile_version     INTEGER NOT NULL CHECK (profile_version >= 1),
    profile_hash        TEXT    NOT NULL
                                CHECK (length(profile_hash) = 64
                                       AND profile_hash NOT GLOB '*[^0-9a-f]*'),
    team_template_id    TEXT    NULL
                                CHECK (team_template_id IS NULL
                                       OR (length(team_template_id) = 36
                                           AND team_template_id NOT GLOB '*[^0-9a-f-]*')),
    team_template_version INTEGER NULL
                                CHECK (team_template_version IS NULL
                                       OR team_template_version >= 1),
    team_template_hash  TEXT    NULL
                                CHECK (team_template_hash IS NULL
                                       OR (length(team_template_hash) = 64
                                           AND team_template_hash NOT GLOB '*[^0-9a-f]*')),
    applied             TEXT    NOT NULL CHECK (applied IN ('created', 'unchanged')),
    recorded_at         TEXT    NOT NULL
                                CHECK (recorded_at GLOB
                                       '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, receipt_id),
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, workflow_id)
        REFERENCES task_workflows(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, profile_key, profile_version)
        REFERENCES work_profiles(project_id, profile_key, version) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_template_id, team_template_version)
        REFERENCES team_templates(project_id, template_id, version) ON DELETE RESTRICT,
    CHECK ((team_template_id IS NULL) = (team_template_version IS NULL)),
    CHECK ((team_template_id IS NULL) = (team_template_hash IS NULL))
) STRICT;

CREATE TRIGGER profile_selection_outcomes_are_immutable
BEFORE UPDATE ON profile_selection_outcomes
BEGIN SELECT RAISE(ABORT, 'a profile selection outcome is immutable'); END;

CREATE TRIGGER profile_selection_outcomes_are_not_deletable
BEFORE DELETE ON profile_selection_outcomes
BEGIN SELECT RAISE(ABORT, 'profile selection outcomes are not deletable'); END;

PRAGMA user_version = 62;
