-- Restore the command-receipt immutability and no-delete triggers.
--
-- `0001_init.sql:1723` created `command_receipts_identity_immutable` and
-- `command_receipts_no_delete`. Every migration that widens the closed
-- command-kind CHECK rebuilds the table, and `DROP TABLE` takes a table's
-- triggers with it: v10, v12, v24, v28, v29, v30, v31 and v32 each re-created
-- `ix_command_receipts_state` and neither trigger. So a database has been
-- carrying no receipt triggers since schema v10, and v32 was the eighth rebuild
-- to drop them.
--
-- Nothing in the build deletes or rewrites a receipt, and `ensure_replay`
-- enforces the same rule in application code, so this was lost defence in depth
-- rather than live corruption. It is restored here because it is exactly the
-- guarantee the consultation publications lean on when they promise that one
-- key plus one intent returns the original projection — and because the
-- precedent those rebuilds "followed exactly" was itself the bug.
--
-- This migration deliberately does not rebuild the table. There is nothing to
-- widen, and rebuilding is what has been dropping these triggers all along.
-- `schema_v1.rs` now asserts both by name, so a ninth rebuild that drops them
-- fails the suite instead of shipping.
CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key
     OR OLD.target <> NEW.target
     OR OLD.intent <> NEW.intent
     OR OLD.intent_hash <> NEW.intent_hash
     OR OLD.kind <> NEW.kind
     OR OLD.project_id <> NEW.project_id
     OR OLD.state IN ('confirmed', 'failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;

CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

PRAGMA user_version = 33;
