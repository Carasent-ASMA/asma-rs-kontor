-- Schema v121. The content of an imported record, kept as inspectable evidence
-- and never as authority.
--
-- `imported_records` says *which* source record an import saw and what its bytes
-- hashed to. That is enough to prove nothing has changed since, and it is not
-- enough to read what the record said. For most kinds it does not need to be:
-- the destination either materialized the record or has no business reading it.
--
-- A Core Team route succession is the case where it does. The succession row is
-- the only durable account of what one route correction actually did — which
-- predecessor was retired, which successor replaced it, under which occupancy
-- and grant generation, and against which receipt. An investigator in another
-- Realm must be able to *read* that account. What they must never be able to do
-- is act on it: an imported succession restores no live succession or
-- idempotency authority, no credential or grant, no placement, and no
-- materialization. It is testimony, not a command.
--
-- This table is what keeps those two apart. The content lands here, beside the
-- lineage row, and nowhere else. Nothing reads it to decide anything: no
-- repository method resolves a destination effect from it, the source
-- idempotency key it names is not a key in this Realm, and it is excluded from
-- this Realm's own export so imported testimony is never forwarded as though it
-- were a local record (ASMA-8187).
CREATE TABLE imported_record_evidence (
    import_id       TEXT NOT NULL,
    record_kind     TEXT NOT NULL,
    source_identity TEXT NOT NULL,
    -- The exact canonical JSON the source record's digest was taken over. Not a
    -- re-encoding: a re-encoding would agree with the digest only by luck, and
    -- the digest is the whole reason this is evidence rather than a copy.
    content         TEXT NOT NULL CHECK (json_valid(content)),
    content_hash    TEXT NOT NULL CHECK (
        length(content_hash) = 64 AND content_hash NOT GLOB '*[^0-9a-f]*'
    ),
    recorded_at     TEXT NOT NULL
                         CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (import_id, record_kind, source_identity),
    -- Evidence exists only for a lineage row that was actually recorded. The
    -- composite reference is deliberate: it also pins the import receipt, which
    -- is what names the source Realm.
    FOREIGN KEY (import_id, record_kind, source_identity)
        REFERENCES imported_records (import_id, record_kind, source_identity)
        ON DELETE RESTRICT
) STRICT;

-- Content may accompany a `recorded` lineage row and no other disposition.
--
-- `materialized` and `already_present` describe records that became destination
-- state, and evidence beside those would be a second, unreconciled copy of a
-- live row. `refused` describes a record this build declined to take at all.
-- Only `recorded` means "kept, deliberately not executable here", which is
-- exactly the disposition this content is admissible under. The rule lives in a
-- trigger because a CHECK cannot ask another table.
CREATE TRIGGER imported_record_evidence_is_never_live
BEFORE INSERT ON imported_record_evidence
WHEN (SELECT disposition FROM imported_records
       WHERE import_id = NEW.import_id
         AND record_kind = NEW.record_kind
         AND source_identity = NEW.source_identity) IS NOT 'recorded'
BEGIN
    SELECT RAISE(ABORT,
        'imported record evidence may only accompany a recorded, non-live lineage row');
END;

-- Testimony that can be edited is not testimony. The digest is recorded beside
-- the bytes precisely so a later reader can check them, and a row that could be
-- rewritten would make that check prove only that someone had rewritten both.
CREATE TRIGGER imported_record_evidence_is_immutable
BEFORE UPDATE ON imported_record_evidence
BEGIN
    SELECT RAISE(ABORT, 'imported record evidence cannot be rewritten');
END;

CREATE TRIGGER imported_record_evidence_is_undeletable
BEFORE DELETE ON imported_record_evidence
BEGIN
    SELECT RAISE(ABORT, 'imported record evidence cannot be deleted');
END;

CREATE INDEX ix_imported_record_evidence_kind
    ON imported_record_evidence (record_kind, content_hash);

PRAGMA user_version = 121;
