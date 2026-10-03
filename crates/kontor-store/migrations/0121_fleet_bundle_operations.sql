-- ===========================================================================
-- Schema v121. Idempotency keys for fleet bundle publication and activation.
--
-- An orchestration bundle — its policy, its canonical Core Team revision and
-- its manifest — is realm-wide state in the Realm state root, like the single
-- policy of v120, so its publish and activate keys are bound here. The list is
-- a CHECK, so widening it is a table rebuild; the permanence triggers v120
-- restored are recreated with the table (ASMA-8280 S-1).
-- ===========================================================================

CREATE TABLE realm_idempotency_bindings_v121 (
    idempotency_key TEXT NOT NULL PRIMARY KEY
                         CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    operation       TEXT NOT NULL CHECK (operation IN (
                             'register_profile_pack', 'apply_capacity_configuration',
                             'publish_fleet_policy', 'activate_fleet_policy',
                             'publish_fleet_bundle', 'activate_fleet_bundle')),
    fingerprint     TEXT NOT NULL
                         CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    bound_at        TEXT NOT NULL
                         CHECK (bound_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

INSERT INTO realm_idempotency_bindings_v121
SELECT idempotency_key, operation, fingerprint, bound_at FROM realm_idempotency_bindings;

DROP TABLE realm_idempotency_bindings;
ALTER TABLE realm_idempotency_bindings_v121 RENAME TO realm_idempotency_bindings;

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

PRAGMA user_version = 121;
