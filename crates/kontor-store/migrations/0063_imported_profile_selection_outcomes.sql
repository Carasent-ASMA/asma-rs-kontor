-- Schema v63. Exact, non-executable lineage for imported profile selections.
--
-- Export generation 3 carries the immutable result bound to each local
-- profile-selection receipt. An import must preserve that result without
-- turning the source receipt into authority in this Realm. These rows are
-- therefore owned by the destination's import receipt, refer to every source
-- identity only as text, and have no foreign key into live command, task,
-- workflow, profile, or team tables.
CREATE TABLE imported_profile_selection_outcomes (
    project_id             TEXT    NOT NULL,
    import_id              TEXT    NOT NULL,
    source_project_id      TEXT    NOT NULL
                                   CHECK (length(source_project_id) = 36
                                          AND source_project_id NOT GLOB '*[^0-9a-f-]*'),
    source_receipt_id      TEXT    NOT NULL
                                   CHECK (length(source_receipt_id) = 36
                                          AND source_receipt_id NOT GLOB '*[^0-9a-f-]*'),
    source_task_id         TEXT    NOT NULL
                                   CHECK (length(source_task_id) = 36
                                          AND source_task_id NOT GLOB '*[^0-9a-f-]*'),
    source_workflow_id     TEXT    NOT NULL
                                   CHECK (length(source_workflow_id) = 36
                                          AND source_workflow_id NOT GLOB '*[^0-9a-f-]*'),
    profile_key            TEXT    NOT NULL CHECK (length(profile_key) BETWEEN 1 AND 128),
    profile_version        INTEGER NOT NULL CHECK (profile_version >= 1),
    profile_hash           TEXT    NOT NULL
                                   CHECK (length(profile_hash) = 64
                                          AND profile_hash NOT GLOB '*[^0-9a-f]*'),
    team_template_id       TEXT    NULL
                                   CHECK (team_template_id IS NULL
                                          OR (length(team_template_id) = 36
                                              AND team_template_id NOT GLOB '*[^0-9a-f-]*')),
    team_template_version  INTEGER NULL
                                   CHECK (team_template_version IS NULL
                                          OR team_template_version >= 1),
    team_template_hash     TEXT    NULL
                                   CHECK (team_template_hash IS NULL
                                          OR (length(team_template_hash) = 64
                                              AND team_template_hash NOT GLOB '*[^0-9a-f]*')),
    applied                TEXT    NOT NULL CHECK (applied IN ('created', 'unchanged')),
    source_recorded_at     TEXT    NOT NULL
                                   CHECK (source_recorded_at GLOB
                                          '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    source_record_hash     TEXT    NOT NULL
                                   CHECK (length(source_record_hash) = 64
                                          AND source_record_hash NOT GLOB '*[^0-9a-f]*'),
    PRIMARY KEY (project_id, import_id, source_project_id, source_receipt_id),
    FOREIGN KEY (project_id, import_id)
        REFERENCES import_receipts(project_id, id) ON DELETE RESTRICT,
    CHECK ((team_template_id IS NULL) = (team_template_version IS NULL)),
    CHECK ((team_template_id IS NULL) = (team_template_hash IS NULL))
) STRICT;

CREATE INDEX ix_imported_profile_selection_outcomes_source
    ON imported_profile_selection_outcomes
       (project_id, source_project_id, source_receipt_id, source_record_hash);

CREATE TRIGGER imported_profile_selection_outcomes_are_immutable
BEFORE UPDATE ON imported_profile_selection_outcomes
BEGIN SELECT RAISE(ABORT, 'imported profile selection lineage is immutable'); END;

CREATE TRIGGER imported_profile_selection_outcomes_are_not_deletable
BEFORE DELETE ON imported_profile_selection_outcomes
BEGIN SELECT RAISE(ABORT, 'imported profile selection lineage is not deletable'); END;

PRAGMA user_version = 63;
