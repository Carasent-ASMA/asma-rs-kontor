-- ===========================================================================
-- Schema v5. Disarming an execution authorization.
--
-- An `execution_authorizations` row is immutable — `0001_init.sql` puts a
-- no-update and a no-delete trigger on it — because an authorization is the
-- durable record of a capability that was granted at an instant, and rewriting
-- one would rewrite what was true when work was admitted under it.
--
-- Revoking is therefore an *append*, exactly as a schedule override's
-- revocation is evidence rather than an edit. The difference from
-- `schedule_overrides`, which carries its revocation in nullable columns, is
-- that those columns predate the immutability trigger on this table; a separate
-- child row is the only shape that can be written without weakening it.
--
-- One revocation per authorization: the primary key is the authorization, so a
-- second disarm of the same authorization is refused by the schema and not only
-- by the Rust layer.
-- ===========================================================================

CREATE TABLE execution_authorization_revocations (
    project_id            TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    authorization_id      TEXT NOT NULL,
    revoked_at            TEXT NOT NULL
                               CHECK (revoked_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revoked_by            TEXT NOT NULL,
    revocation_receipt_id TEXT NOT NULL,
    reason                TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 512),
    PRIMARY KEY (project_id, authorization_id),
    FOREIGN KEY (project_id, authorization_id)
        REFERENCES execution_authorizations (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, revoked_by)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, revocation_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

-- Append-only, like every other piece of evidence in this schema.
CREATE TRIGGER execution_authorization_revocations_no_update
BEFORE UPDATE ON execution_authorization_revocations
BEGIN SELECT RAISE(ABORT, 'an execution authorization revocation is evidence, not a draft'); END;
CREATE TRIGGER execution_authorization_revocations_no_delete
BEFORE DELETE ON execution_authorization_revocations
BEGIN SELECT RAISE(ABORT, 'an execution authorization revocation is evidence, not a draft'); END;

PRAGMA user_version = 9;
