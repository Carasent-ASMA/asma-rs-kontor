-- ===========================================================================
-- Schema v15. Idempotency keys for realm-scoped operations.
--
-- `command_receipts.idempotency_key` is already UNIQUE and is the mechanism
-- every project-scoped mutation uses. It cannot serve a *realm-scoped* one:
-- `command_receipts.project_id` is a NOT NULL foreign key into `projects`, and
-- an operation that creates no project has nothing to point it at.
--
-- This is the same rule for the operations that live outside a project. A key is
-- bound, once and permanently, to the *logical operation* it was first used for
-- — named by a fingerprint over the operation's own identifying content — and
-- reusing it for anything else is refused. That closes the gap a content-only
-- check leaves open: without this table, one key registering two different packs
-- would succeed twice, because each registration is independently valid and
-- nothing was comparing them to each other.
--
-- The fingerprint is a digest of a canonical JSON document, the same way a
-- command intent is digested, so what a key is bound to is derived by exactly
-- the convention receipts and events already use rather than by a second
-- serialization that could disagree with them.
--
-- The realm *file* is the boundary, as everywhere else in this schema: there is
-- no realm_id column, because one database is one realm.
--
-- `operation` is a closed `CHECK` list for the same reason `command_receipts.kind`
-- is: a value SQL accepts but the code cannot interpret is an unreadable row, and
-- widening the set is therefore a migration and not a code change.
-- ===========================================================================

CREATE TABLE realm_idempotency_bindings (
    idempotency_key TEXT NOT NULL PRIMARY KEY
                         CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    operation       TEXT NOT NULL CHECK (operation IN ('register_profile_pack')),
    fingerprint     TEXT NOT NULL
                         CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    bound_at        TEXT NOT NULL
                         CHECK (bound_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

-- A binding is permanent. A key that could be rebound would let the second use
-- of it look like the first, which is the whole thing this table exists to stop.
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

PRAGMA user_version = 15;
