-- The receipt a confirmed Team Definition migration was commanded under.
--
-- Confirmation commits the epic pin and the terminal `confirmed` state in one
-- transaction; the command receipt is written after it. A crash in that window
-- leaves a migration that really did happen with nothing to point a retrying
-- caller at it, because the intent row is terminal and every apply operation
-- refuses a terminal intent.
--
-- Kept in its own table rather than as a column on the intent, because the
-- intent's update trigger deliberately freezes a settled row: a settled
-- migration must not become editable again just so that a late receipt can be
-- attached to it. This table is append-only and one row per migration, so
-- binding a receipt can never overwrite the receipt a migration already has.
CREATE TABLE team_definition_migration_receipts (
    intent_id  TEXT NOT NULL PRIMARY KEY
                    REFERENCES team_definition_migration_intents(id) ON DELETE RESTRICT,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    receipt_id TEXT NOT NULL
                    CHECK (length(receipt_id) = 36 AND receipt_id NOT GLOB '*[^0-9a-f-]*'),
    bound_at   TEXT NOT NULL,
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts(project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER team_definition_migration_receipts_are_immutable
BEFORE UPDATE ON team_definition_migration_receipts
BEGIN
    SELECT RAISE(ABORT, 'a migration is commanded once and keeps its receipt');
END;

CREATE TRIGGER team_definition_migration_receipts_are_permanent
BEFORE DELETE ON team_definition_migration_receipts
BEGIN
    SELECT RAISE(ABORT, 'migration receipts are evidence and are not deletable');
END;

PRAGMA user_version = 79;
