-- ===========================================================================
-- Schema v123. Idempotency keys for fleet policy publication and activation.
--
-- A fleet policy is realm-wide state: its published artifact and activation
-- record live in the Realm state root, not in a project. Like the capacity
-- configuration apply of v28 it has no aggregate for a command receipt to name,
-- so its key is bound here. The list is a CHECK, so widening it is a table
-- rebuild (ASMA-8280).
--
-- The v28 rebuild dropped the two v15 triggers with the old table, which left
-- bindings silently rebindable and deletable. This rebuild restores them: a
-- binding that could move would let a second use of a key pass as the first.
-- ===========================================================================

CREATE TABLE realm_idempotency_bindings_v123 (
    idempotency_key TEXT NOT NULL PRIMARY KEY
                         CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    operation       TEXT NOT NULL CHECK (operation IN (
                             'register_profile_pack', 'apply_capacity_configuration',
                             'publish_fleet_policy', 'activate_fleet_policy')),
    fingerprint     TEXT NOT NULL
                         CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    bound_at        TEXT NOT NULL
                         CHECK (bound_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

INSERT INTO realm_idempotency_bindings_v123
SELECT idempotency_key, operation, fingerprint, bound_at FROM realm_idempotency_bindings;

DROP TABLE realm_idempotency_bindings;
ALTER TABLE realm_idempotency_bindings_v123 RENAME TO realm_idempotency_bindings;

CREATE TRIGGER realm_idempotency_bindings_are_immutable
BEFORE UPDATE ON realm_idempotency_bindings
BEGIN
    SELECT RAISE(ABORT, 'an idempotency binding is never rebound');
END;

CREATE TRIGGER realm_idempotency_bindings_are_permanent
BEFORE DELETE ON realm_idempotency_bindings
BEGIN
    SELECT RAISE(ABORT, 'an idempotency binding is never released');
END;

PRAGMA user_version = 123;
