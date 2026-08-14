CREATE TABLE team_drafts (
    team_id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    slots_json TEXT NOT NULL
) STRICT;

CREATE TABLE team_revisions (
    team_id TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    name TEXT NOT NULL,
    slots_json TEXT NOT NULL,
    PRIMARY KEY (team_id, version)
) STRICT;

CREATE TRIGGER team_revisions_are_immutable
BEFORE UPDATE ON team_revisions
BEGIN
    SELECT RAISE(ABORT, 'team revisions are immutable');
END;

CREATE TRIGGER team_revisions_are_permanent
BEFORE DELETE ON team_revisions
BEGIN
    SELECT RAISE(ABORT, 'team revisions are permanent');
END;

CREATE TABLE teams_projection (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    cursor INTEGER NOT NULL CHECK (cursor >= 0)
) STRICT;
INSERT INTO teams_projection(singleton, cursor) VALUES (1, 0);

CREATE TABLE team_command_replays (
    idempotency_key TEXT PRIMARY KEY NOT NULL,
    fingerprint TEXT NOT NULL,
    response_json TEXT NOT NULL
) STRICT;

PRAGMA user_version = 22;
