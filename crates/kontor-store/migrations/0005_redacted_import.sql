-- ===========================================================================
-- Schema v5 — destination receipts and lineage for a redacted import (KON-MVP-19)
--
-- A redacted import is not a restore, and this schema is where the difference
-- is made durable. A restore reinstates a Realm's own bytes: the receipts,
-- bindings and identities that come back are the ones this Realm minted. An
-- import takes *another* Realm's export, and every id in it is a reference to
-- something that happened elsewhere. Two tables express exactly that.
--
--  1. **`import_receipts` is the destination's own receipt.** It is minted here,
--     with a destination id and a destination instant, and it names the source
--     by realm id, export generation and records digest. It is deliberately not
--     a `command_receipts` row: a command receipt is a *dispatchable* thing with
--     an outbox, an attempt count and a state machine that ends in a runtime
--     effect. An import has already happened by the time it is recorded, and
--     giving it a shape that a dispatcher recognizes is how a source Realm's
--     work would eventually be re-executed here.
--
--  2. **`imported_records` is lineage, not state.** One row per source record,
--     carrying the source's identity and content digest and what this import
--     did about it — nothing else. A source command, status-transition or
--     dispatch receipt is recorded here as evidence that it existed, and is
--     never written into the destination's own receipt tables, where a
--     scheduler or a reconciler would read it as this Realm's own history.
--
-- The `UNIQUE (project_id, source_realm_id, records_hash)` is the idempotency
-- rule: the same export cannot be imported into the same project twice, because
-- the second import would double every materialized specification's provenance
-- while claiming to be a separate event.
--
-- Both tables are append-only through BEFORE UPDATE/DELETE triggers, like every
-- other evidence table in this schema, and a trigger refuses an import that
-- claims to come from this Realm — that is a restore, and it takes a different
-- route.
-- ===========================================================================

-- One import of one export document into this Realm.
CREATE TABLE import_receipts (
    id                     TEXT    NOT NULL PRIMARY KEY
                                   CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id             TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    -- The Realm the records came from. A reference: nothing in this database
    -- may be resolved against it, and no foreign key points at it.
    source_realm_id        TEXT    NOT NULL
                                   CHECK (length(source_realm_id) = 36
                                          AND source_realm_id NOT GLOB '*[^0-9a-f-]*'),
    -- The export document's own generation, so a later reader knows which
    -- contract these records were written under.
    export_schema_version  INTEGER NOT NULL CHECK (export_schema_version >= 1),
    -- The database generation the source read its rows from.
    source_schema_version  INTEGER NOT NULL CHECK (source_schema_version >= 1),
    -- The digest the source computed over its records, re-verified here before
    -- anything was applied.
    records_hash           TEXT    NOT NULL
                                   CHECK (length(records_hash) = 64
                                          AND records_hash NOT GLOB '*[^0-9a-f]*'),
    -- The source's instant, and this Realm's. Both are kept: an import's own
    -- time is destination authority, the export's time is source provenance,
    -- and collapsing them would lose which is which.
    exported_at            TEXT    NOT NULL
                                   CHECK (exported_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    imported_at            TEXT    NOT NULL
                                   CHECK (imported_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    record_count           INTEGER NOT NULL CHECK (record_count >= 0),
    materialized_count     INTEGER NOT NULL CHECK (materialized_count >= 0
                                                   AND materialized_count <= record_count),
    UNIQUE (project_id, id),
    UNIQUE (project_id, source_realm_id, records_hash)
) STRICT;

CREATE INDEX ix_import_receipts_source
    ON import_receipts (source_realm_id, records_hash);

-- What one import did about one source record.
--
-- `disposition` is the whole vocabulary, and it is closed on purpose:
--
--   * `materialized` — a versioned specification the destination re-validated
--     through its own domain types and inserted under its own project;
--   * `already_present` — the destination already had that specification at
--     that version, and its own revision was left alone;
--   * `recorded`      — evidence about the source that is kept as lineage and
--     is deliberately not executable here: receipts, transitions, dispatch
--     state, observations, gaps, leases and bindings;
--   * `refused`       — a record this build will not take, with the reason
--     recorded as a stable code rather than as prose.
CREATE TABLE imported_records (
    import_id       TEXT NOT NULL REFERENCES import_receipts (id) ON DELETE RESTRICT,
    -- The source table the record came from. An open key: a later export
    -- generation may carry kinds this one never saw, and refusing them by
    -- lexical shape is honest where an enum would need a migration.
    record_kind     TEXT NOT NULL CHECK (length(record_kind) BETWEEN 1 AND 128
                                         AND record_kind NOT GLOB '*[^a-z0-9_]*'),
    -- The source record's primary key, rendered as text. It is never resolved
    -- against anything in this database.
    source_identity TEXT NOT NULL CHECK (length(source_identity) BETWEEN 1 AND 512),
    source_hash     TEXT NOT NULL CHECK (length(source_hash) = 64
                                         AND source_hash NOT GLOB '*[^0-9a-f]*'),
    disposition     TEXT NOT NULL CHECK (disposition IN
                        ('materialized', 'already_present', 'recorded', 'refused')),
    -- Why, for a refusal. A stable code, never prose and never a stored value.
    reason_code     TEXT NULL CHECK (reason_code IS NULL
                                     OR (length(reason_code) BETWEEN 1 AND 128
                                         AND reason_code NOT GLOB '*[^a-z0-9_]*')),
    recorded_at     TEXT NOT NULL
                         CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (import_id, record_kind, source_identity)
) STRICT;

CREATE INDEX ix_imported_records_kind ON imported_records (record_kind, source_hash);

-- An export of this Realm is not imported into it. That operation exists and is
-- called restore; letting it in here would mint a second, source-referenced
-- copy of this Realm's own lineage and make every id ambiguous.
CREATE TRIGGER import_receipts_reject_own_realm BEFORE INSERT ON import_receipts
WHEN NEW.source_realm_id = (SELECT realm_id FROM realm_metadata WHERE singleton = 1)
BEGIN SELECT RAISE(ABORT, 'an export of this realm is restored, never imported'); END;

CREATE TRIGGER import_receipts_no_update BEFORE UPDATE ON import_receipts
BEGIN SELECT RAISE(ABORT, 'an import receipt is immutable'); END;

CREATE TRIGGER import_receipts_no_delete BEFORE DELETE ON import_receipts
BEGIN SELECT RAISE(ABORT, 'an import receipt is not deletable'); END;

CREATE TRIGGER imported_records_no_update BEFORE UPDATE ON imported_records
BEGIN SELECT RAISE(ABORT, 'imported-record lineage is immutable'); END;

CREATE TRIGGER imported_records_no_delete BEFORE DELETE ON imported_records
BEGIN SELECT RAISE(ABORT, 'imported-record lineage is not deletable'); END;

PRAGMA user_version = 5;
