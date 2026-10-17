-- ===========================================================================
-- Schema v135. Widen the realm-scoped idempotency operations by exactly one.
--
-- The provider-marker correction writes `<state-root>/runtimes.json`, a realm
-- file with no aggregate a command receipt could name — the same class v15
-- built `realm_idempotency_bindings` for and v28, v125 and v126 widened. Its
-- key is judged before the revision CAS and claimed before the correction is
-- written, so the write always happens under its own operation's key, a replay
-- of a completed correction is answered from the binding instead of writing the
-- file a second time, and an interrupted attempt can finish on retry.
--
-- The list is a CHECK, so widening it is a table rebuild rather than a code
-- change: a value SQL accepts but the code cannot interpret is an unreadable
-- row. The permanence triggers v125 restored are recreated with the table, as
-- v126 did (ASMA-7968).
-- ===========================================================================

CREATE TABLE realm_idempotency_bindings_v135 (
    idempotency_key TEXT NOT NULL PRIMARY KEY
                         CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    operation       TEXT NOT NULL CHECK (operation IN (
                             'register_profile_pack', 'apply_capacity_configuration',
                             'publish_fleet_policy', 'activate_fleet_policy',
                             'publish_fleet_bundle', 'activate_fleet_bundle',
                             'clear_provider_marker')),
    fingerprint     TEXT NOT NULL
                         CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    bound_at        TEXT NOT NULL
                         CHECK (bound_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

INSERT INTO realm_idempotency_bindings_v135
SELECT idempotency_key, operation, fingerprint, bound_at FROM realm_idempotency_bindings;

DROP TABLE realm_idempotency_bindings;
ALTER TABLE realm_idempotency_bindings_v135 RENAME TO realm_idempotency_bindings;

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

PRAGMA user_version = 135;
