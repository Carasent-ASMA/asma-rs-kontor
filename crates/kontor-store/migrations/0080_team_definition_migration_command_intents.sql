-- The exact canonical command intent one migration was recorded under.
--
-- Recovery after a crash between the pin commit and the receipt write has to
-- answer a question the fingerprint cannot: is the request being retried the
-- same *command* that was originally issued? The fingerprint covers the epic,
-- both pins and the target set, but a retry can carry a different preview hash
-- or a different legacy-topic map and still fingerprint identically, because
-- those are inputs to the command rather than parts of the enumerated plan.
--
-- Written in the same transaction that records the intent, before any external
-- effect, so a retry can be compared against it and refused as an idempotency
-- conflict *before* a receipt is produced for a command nobody issued.
--
-- Append-only and one row per migration: a migration is issued once, and the
-- intent it was issued under is evidence rather than state.
CREATE TABLE team_definition_migration_command_intents (
    intent_id     TEXT NOT NULL PRIMARY KEY
                       REFERENCES team_definition_migration_intents(id) ON DELETE RESTRICT,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    intent_hash   TEXT NULL
                       CHECK (intent_hash IS NULL OR
                              (length(intent_hash) = 64
                               AND intent_hash NOT GLOB '*[^0-9a-f]*')),
    -- `issued` is written atomically with every v80+ migration. A v79 row is
    -- provable only when its separately bound UpgradeTeamDefinition command
    -- receipt carries the exact canonical intent hash. Everything else is
    -- retained as an explicit recovery fence: no fingerprint or target set is
    -- allowed to masquerade as the command request that was never recorded.
    source        TEXT NOT NULL
                       CHECK (source IN ('issued', 'legacy_receipt',
                                         'legacy_unrecoverable')),
    recorded_at   TEXT NOT NULL,
    CHECK ((source = 'legacy_unrecoverable') = (intent_hash IS NULL))
) STRICT;

INSERT INTO team_definition_migration_command_intents
    (intent_id, project_id, intent_hash, source, recorded_at)
SELECT migration.id,
       migration.project_id,
       CASE WHEN receipt.kind = 'upgrade_team_definition'
            THEN receipt.intent_hash ELSE NULL END,
       CASE WHEN receipt.kind = 'upgrade_team_definition'
            THEN 'legacy_receipt' ELSE 'legacy_unrecoverable' END,
       COALESCE(receipt.created_at, migration.recorded_at)
FROM team_definition_migration_intents AS migration
LEFT JOIN team_definition_migration_receipts AS binding
       ON binding.intent_id = migration.id
      AND binding.project_id = migration.project_id
LEFT JOIN command_receipts AS receipt
       ON receipt.id = binding.receipt_id
      AND receipt.project_id = binding.project_id;

CREATE TRIGGER team_definition_migration_command_intents_are_immutable
BEFORE UPDATE ON team_definition_migration_command_intents
BEGIN
    SELECT RAISE(ABORT, 'a migration keeps the command intent it was issued under');
END;

CREATE TRIGGER team_definition_migration_command_intents_are_permanent
BEFORE DELETE ON team_definition_migration_command_intents
BEGIN
    SELECT RAISE(ABORT, 'migration command intents are evidence and are not deletable');
END;

PRAGMA user_version = 80;
