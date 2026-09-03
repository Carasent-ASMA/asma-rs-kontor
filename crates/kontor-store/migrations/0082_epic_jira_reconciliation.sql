-- Schema v82. Epic Jira convergence is a first-class aggregate operation.
--
-- An epic has no task link, so its conflicts and authorized transition
-- attempts must not be hidden behind a fabricated jira_links row. These two
-- ledgers retain the exact binding, policy revision, observation digest and
-- confirmed readback needed to recover safely after a daemon restart.

CREATE TABLE epic_status_conflicts (
    id                    TEXT NOT NULL PRIMARY KEY
                               CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id            TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    epic_id               TEXT NOT NULL,
    kind                  TEXT NOT NULL CHECK (kind IN (
                              'stale_observation', 'no_live_transition',
                              'multiple_live_transitions', 'incompatible_human_move',
                              'external_terminal_before_internal_evidence',
                              'unknown_status_class', 'unknown_transition_path',
                              'ownership_unresolved', 'ownership_mismatch',
                              'terminal_ownership_violation')),
    external_issue_key    TEXT NOT NULL CHECK (length(external_issue_key) BETWEEN 1 AND 256),
    observed_status_id    TEXT NOT NULL CHECK (length(observed_status_id) BETWEEN 1 AND 256),
    observed_status_name  TEXT NOT NULL CHECK (length(observed_status_name) BETWEEN 1 AND 512),
    observed_at           TEXT NOT NULL
                               CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    payload_hash          TEXT NOT NULL CHECK (
                              length(payload_hash) = 64
                              AND payload_hash NOT GLOB '*[^0-9a-f]*'
                          ),
    epic_revision         INTEGER NOT NULL CHECK (epic_revision >= 1),
    spec_version          INTEGER NOT NULL CHECK (spec_version >= 1),
    milestone             TEXT NULL CHECK (milestone IS NULL OR length(milestone) BETWEEN 1 AND 128),
    detected_at           TEXT NOT NULL
                               CHECK (detected_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    resolved_at           TEXT NULL
                               CHECK (resolved_at IS NULL OR resolved_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    resolution_receipt_id TEXT NULL,
    FOREIGN KEY (project_id, epic_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, resolution_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    CHECK ((resolved_at IS NULL) = (resolution_receipt_id IS NULL))
) STRICT;

CREATE UNIQUE INDEX ux_epic_status_conflicts_one_open_kind
    ON epic_status_conflicts (project_id, epic_id, kind)
    WHERE resolved_at IS NULL;

CREATE TABLE epic_jira_transition_intents (
    id                        TEXT NOT NULL PRIMARY KEY
                                   CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id                TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    epic_id                   TEXT NOT NULL,
    external_issue_key        TEXT NOT NULL CHECK (length(external_issue_key) BETWEEN 1 AND 256),
    idempotency_key           TEXT NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    intent_hash               TEXT NOT NULL CHECK (
                                  length(intent_hash) = 64
                                  AND intent_hash NOT GLOB '*[^0-9a-f]*'
                              ),
    epic_revision             INTEGER NOT NULL CHECK (epic_revision >= 1),
    spec_version              INTEGER NOT NULL CHECK (spec_version >= 1),
    milestone                 TEXT NOT NULL CHECK (length(milestone) BETWEEN 1 AND 128),
    target_status_id          TEXT NOT NULL CHECK (length(target_status_id) BETWEEN 1 AND 256),
    target_status_name        TEXT NOT NULL CHECK (length(target_status_name) BETWEEN 1 AND 512),
    destination_status_id     TEXT NOT NULL CHECK (length(destination_status_id) BETWEEN 1 AND 256),
    destination_status_name   TEXT NOT NULL CHECK (length(destination_status_name) BETWEEN 1 AND 512),
    prior_payload_hash        TEXT NOT NULL CHECK (
                                  length(prior_payload_hash) = 64
                                  AND prior_payload_hash NOT GLOB '*[^0-9a-f]*'
                              ),
    planned_at                TEXT NOT NULL
                                   CHECK (planned_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    confirmed_at              TEXT NULL
                                   CHECK (confirmed_at IS NULL OR confirmed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    confirmation_payload_hash TEXT NULL CHECK (
                                  confirmation_payload_hash IS NULL OR
                                  (length(confirmation_payload_hash) = 64
                                   AND confirmation_payload_hash NOT GLOB '*[^0-9a-f]*')
                              ),
    FOREIGN KEY (project_id, epic_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    UNIQUE (project_id, idempotency_key),
    CHECK ((confirmed_at IS NULL) = (confirmation_payload_hash IS NULL))
) STRICT;

CREATE TRIGGER epic_status_conflicts_keep_detection_evidence
BEFORE UPDATE ON epic_status_conflicts
WHEN OLD.resolved_at IS NOT NULL
     OR OLD.project_id <> NEW.project_id
     OR OLD.epic_id <> NEW.epic_id
     OR OLD.kind <> NEW.kind
     OR OLD.external_issue_key <> NEW.external_issue_key
     OR OLD.observed_status_id <> NEW.observed_status_id
     OR OLD.observed_status_name <> NEW.observed_status_name
     OR OLD.observed_at <> NEW.observed_at
     OR OLD.payload_hash <> NEW.payload_hash
     OR OLD.epic_revision <> NEW.epic_revision
     OR OLD.spec_version <> NEW.spec_version
     OR OLD.milestone IS NOT NEW.milestone
     OR OLD.detected_at <> NEW.detected_at
BEGIN
    SELECT RAISE(ABORT, 'epic Jira conflict detection evidence is immutable');
END;

CREATE TRIGGER epic_status_conflicts_are_permanent
BEFORE DELETE ON epic_status_conflicts
BEGIN
    SELECT RAISE(ABORT, 'epic Jira conflicts are evidence and are not deletable');
END;

CREATE TRIGGER epic_jira_transition_intents_keep_authority
BEFORE UPDATE OF id, project_id, epic_id, external_issue_key, idempotency_key,
                 intent_hash, epic_revision, spec_version, milestone,
                 target_status_id, target_status_name,
                 destination_status_id, destination_status_name,
                 prior_payload_hash,
                 planned_at
ON epic_jira_transition_intents
BEGIN
    SELECT RAISE(ABORT, 'epic Jira transition authority is immutable');
END;

CREATE TRIGGER epic_jira_transition_intents_are_permanent
BEFORE DELETE ON epic_jira_transition_intents
BEGIN
    SELECT RAISE(ABORT, 'epic Jira transition intents are evidence and are not deletable');
END;

PRAGMA user_version = 82;
