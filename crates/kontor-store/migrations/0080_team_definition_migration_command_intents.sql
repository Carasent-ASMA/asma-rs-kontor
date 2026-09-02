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
    intent_hash   TEXT NOT NULL
                       CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at   TEXT NOT NULL
) STRICT;

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
