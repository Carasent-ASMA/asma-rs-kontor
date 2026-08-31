-- Schema v74. A non-creating link recovery adopts the exact pending create
-- batch it is repairing. The original create intent and marker remain
-- immutable incident evidence; the recovery keys are a separate append-only
-- intent persisted before the connector is contacted.
CREATE TABLE jira_materialization_recoveries (
    project_id          TEXT NOT NULL,
    batch_id            TEXT NOT NULL,
    item_id             TEXT NOT NULL,
    recovery_receipt_id TEXT NOT NULL,
    preview_hash        TEXT NOT NULL CHECK (length(preview_hash) = 64),
    ordinal             INTEGER NOT NULL CHECK (ordinal >= 0),
    requested_key       TEXT NOT NULL CHECK (length(requested_key) BETWEEN 1 AND 255),
    marker              TEXT NOT NULL CHECK (length(marker) BETWEEN 1 AND 255),
    recovered_at        TEXT NOT NULL,
    PRIMARY KEY (project_id, batch_id, item_id),
    UNIQUE (recovery_receipt_id, ordinal),
    FOREIGN KEY (project_id, recovery_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (batch_id)
        REFERENCES jira_materialization_batches (id) ON DELETE RESTRICT,
    FOREIGN KEY (item_id)
        REFERENCES jira_materialization_items (id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, batch_id)
        REFERENCES jira_materialization_batches (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX jira_materialization_recoveries_batch
    ON jira_materialization_recoveries (project_id, batch_id, ordinal);

CREATE TRIGGER jira_materialization_recovery_scope_exact
BEFORE INSERT ON jira_materialization_recoveries
WHEN NOT EXISTS (
    SELECT 1
    FROM jira_materialization_items AS item
    WHERE item.id = NEW.item_id
      AND item.project_id = NEW.project_id
      AND item.batch_id = NEW.batch_id
      AND item.ordinal = NEW.ordinal
      AND item.marker = NEW.marker
      AND item.intent_kind = 'create'
)
BEGIN SELECT RAISE(ABORT, 'Jira materialization recovery scope is not exact'); END;

CREATE TRIGGER jira_materialization_recoveries_are_immutable
BEFORE UPDATE ON jira_materialization_recoveries
BEGIN SELECT RAISE(ABORT, 'Jira materialization recoveries are immutable'); END;

CREATE TRIGGER jira_materialization_recoveries_are_permanent
BEFORE DELETE ON jira_materialization_recoveries
BEGIN SELECT RAISE(ABORT, 'Jira materialization recoveries are permanent'); END;

PRAGMA user_version = 74;
