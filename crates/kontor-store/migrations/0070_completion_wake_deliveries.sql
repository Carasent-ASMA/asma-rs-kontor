-- Durable delivery attempts for Completion wakes addressed to persistent TPM
-- seats.  A wake is logical and survives native replacement; each exact native
-- occupancy gets its own stable message id and frozen body so acknowledgement
-- loss and daemon restart can reconcile the same effect rather than send again.
CREATE TABLE epic_completion_wake_deliveries (
    project_id            TEXT    NOT NULL,
    mini_project_id       TEXT    NOT NULL,
    completion_revision   INTEGER NOT NULL CHECK (completion_revision >= 1),
    reason                TEXT    NOT NULL,
    seat_binding_id       TEXT    NOT NULL,
    occupancy_generation  INTEGER NOT NULL CHECK (occupancy_generation >= 1),
    runtime_kind          TEXT    NOT NULL,
    host                  TEXT    NOT NULL,
    runtime_generation    INTEGER NOT NULL CHECK (runtime_generation >= 0),
    native_id             TEXT    NOT NULL,
    message_id            TEXT    NOT NULL UNIQUE
                                    CHECK (length(message_id) = 36 AND
                                           message_id NOT GLOB '*[^0-9a-f-]*' AND
                                           substr(message_id, 9, 1) = '-' AND
                                           substr(message_id, 14, 1) = '-' AND
                                           substr(message_id, 15, 1) = '7' AND
                                           substr(message_id, 19, 1) = '-' AND
                                           substr(message_id, 20, 1) GLOB '[89ab]' AND
                                           substr(message_id, 24, 1) = '-'),
    body                  TEXT    NOT NULL CHECK (length(body) BETWEEN 1 AND 65536),
    body_hash             TEXT    NOT NULL
                                    CHECK (length(body_hash) = 64 AND
                                           body_hash NOT GLOB '*[^0-9a-f]*'),
    created_at            TEXT    NOT NULL
                                    CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    acknowledged_at       TEXT    NULL
                                    CHECK (acknowledged_at IS NULL OR
                                           acknowledged_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    timeline_epoch        INTEGER NULL CHECK (timeline_epoch IS NULL OR timeline_epoch >= 1),
    timeline_sequence     INTEGER NULL CHECK (timeline_sequence IS NULL OR timeline_sequence >= 1),
    PRIMARY KEY (
        project_id, mini_project_id, completion_revision, reason,
        seat_binding_id, occupancy_generation, native_id
    ),
    FOREIGN KEY (
        project_id, mini_project_id, completion_revision, reason, seat_binding_id
    ) REFERENCES epic_completion_wakes (
        project_id, mini_project_id, completion_revision, reason, seat_binding_id
    ) ON DELETE RESTRICT,
    CHECK ((acknowledged_at IS NULL AND timeline_epoch IS NULL AND timeline_sequence IS NULL) OR
           (acknowledged_at IS NOT NULL AND timeline_epoch IS NOT NULL AND timeline_sequence IS NOT NULL))
) STRICT;

CREATE INDEX ix_completion_wake_deliveries_pending
    ON epic_completion_wake_deliveries (project_id, mini_project_id, completion_revision)
    WHERE acknowledged_at IS NULL;

CREATE TRIGGER epic_completion_wake_deliveries_are_permanent
BEFORE DELETE ON epic_completion_wake_deliveries
BEGIN
    SELECT RAISE(ABORT, 'completion wake delivery history is permanent');
END;

PRAGMA user_version = 70;
