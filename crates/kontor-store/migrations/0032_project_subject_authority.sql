-- Per-project, per-subject write authority, replacing the realm singleton.
--
-- v21 shipped one `memory_authority(singleton = 1)` row. That shape can only
-- answer one question — "has *the Realm* cut over?" — and a Realm holds
-- projects whose facts arrived by different routes: one created in Kontor, one
-- still being imported out of AgentsRoom. Flipping the singleton to serve a
-- fresh project would silently claim authority over every legacy project that
-- had not been imported yet, and flipping it for a legacy project would demand
-- a freeze and an export from a native one that never had a source to freeze.
--
-- Authority becomes a fact about `(project_id, subject)`. The singleton table is
-- left in place, unread and frozen by trigger: the route that wrote it now
-- refuses, and a migration that dropped it would take the evidence of what the
-- Realm used to claim with it.

-- Who may write one project's one subject, and the evidence that moved it.
--
-- `origin` is immutable and decides whether cutover is meaningful at all;
-- `authority` is who may write now. Splitting them is what lets a fresh native
-- subject be writable from its first instant while a pending one stays
-- read-only until it has actually been imported.
CREATE TABLE project_subject_authority (
    project_id        TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    subject           TEXT    NOT NULL CHECK (subject IN ('memory', 'backlog')),
    origin            TEXT    NOT NULL CHECK (origin IN ('kontor_native',
                                                         'legacy_pending')),
    authority         TEXT    NOT NULL CHECK (authority IN ('agentsroom', 'kontor')),
    revision          INTEGER NOT NULL CHECK (revision >= 1),
    -- The operator attestation that the legacy source is frozen. Set on its own,
    -- once, and required by the switch.
    source_frozen_at  TEXT    NULL
                              CHECK (source_frozen_at IS NULL OR
                                     source_frozen_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- The import this switch was granted against.
    final_import_hash TEXT    NULL
                              CHECK (final_import_hash IS NULL OR
                                     (length(final_import_hash) = 64 AND
                                      final_import_hash NOT GLOB '*[^0-9a-f]*')),
    -- Recomputed from stored Kontor state at switch time, never from the bytes
    -- the caller submitted.
    readback_hash     TEXT    NULL
                              CHECK (readback_hash IS NULL OR
                                     (length(readback_hash) = 64 AND
                                      readback_hash NOT GLOB '*[^0-9a-f]*')),
    switched_at       TEXT    NULL
                              CHECK (switched_at IS NULL OR
                                     switched_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, subject),
    -- A native subject is Kontor's from creation and carries no cutover
    -- evidence: there is no source it was frozen from and no import it came out
    -- of. This is the constraint that makes an empty-export ceremony
    -- unrepresentable rather than merely discouraged.
    CHECK (origin <> 'kontor_native' OR
           (authority = 'kontor' AND
            source_frozen_at IS NULL AND final_import_hash IS NULL AND
            readback_hash IS NULL AND switched_at IS NULL)),
    -- A pending subject is Kontor's only with the complete evidence set. Partial
    -- evidence is not a state: either all four facts stand behind the switch or
    -- authority has not moved.
    CHECK (origin <> 'legacy_pending' OR authority = 'agentsroom' OR
           (source_frozen_at IS NOT NULL AND final_import_hash IS NOT NULL AND
            readback_hash IS NOT NULL AND switched_at IS NOT NULL))
) STRICT;

-- Exactly two updates are legal, and both are guarded here rather than trusted
-- to the caller: the attestation that records a frozen source, and the one-way
-- switch that spends it. Everything else — a changed origin, a re-frozen
-- source, an authority moving back to AgentsRoom, a second switch, a revision
-- that does not advance — aborts.
CREATE TRIGGER project_subject_authority_guarded_update
BEFORE UPDATE ON project_subject_authority
WHEN NOT (
    -- Attest: the legacy source is frozen. Authority does not move here.
    (OLD.origin = 'legacy_pending' AND NEW.origin = OLD.origin AND
     OLD.authority = 'agentsroom' AND NEW.authority = 'agentsroom' AND
     OLD.source_frozen_at IS NULL AND NEW.source_frozen_at IS NOT NULL AND
     NEW.final_import_hash IS NULL AND NEW.readback_hash IS NULL AND
     NEW.switched_at IS NULL AND NEW.revision = OLD.revision + 1)
    OR
    -- Switch: authority moves once, on the complete evidence set.
    (OLD.origin = 'legacy_pending' AND NEW.origin = OLD.origin AND
     OLD.authority = 'agentsroom' AND NEW.authority = 'kontor' AND
     OLD.source_frozen_at IS NOT NULL AND NEW.source_frozen_at = OLD.source_frozen_at AND
     NEW.final_import_hash IS NOT NULL AND NEW.readback_hash IS NOT NULL AND
     NEW.switched_at IS NOT NULL AND NEW.revision = OLD.revision + 1)
)
BEGIN SELECT RAISE(ABORT, 'project subject authority permits only the guarded attestation and switch'); END;

CREATE TRIGGER project_subject_authority_no_delete
BEFORE DELETE ON project_subject_authority
BEGIN SELECT RAISE(ABORT, 'project subject authority rows are never deleted'); END;

-- What one subject's legacy import actually carried.
--
-- `readback_hash` is recomputed from stored Kontor state after the import
-- transaction, so the switch can compare what was asked for with what is now
-- durably here. Keyed by subject as well as source, because a project's memory
-- and backlog are imported separately.
CREATE TABLE subject_import_manifests (
    project_id         TEXT    NOT NULL,
    subject            TEXT    NOT NULL CHECK (subject IN ('memory', 'backlog')),
    source             TEXT    NOT NULL CHECK (length(source) BETWEEN 1 AND 128),
    import_hash        TEXT    NOT NULL
                               CHECK (length(import_hash) = 64 AND import_hash NOT GLOB '*[^0-9a-f]*'),
    canonical_manifest TEXT    NOT NULL CHECK (json_valid(canonical_manifest)),
    imported_count     INTEGER NOT NULL CHECK (imported_count >= 0),
    readback_hash      TEXT    NOT NULL
                               CHECK (length(readback_hash) = 64 AND readback_hash NOT GLOB '*[^0-9a-f]*'),
    imported_at        TEXT    NOT NULL
                               CHECK (imported_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, subject, source, import_hash),
    FOREIGN KEY (project_id, subject)
        REFERENCES project_subject_authority (project_id, subject) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER subject_import_manifests_no_update
BEFORE UPDATE ON subject_import_manifests
BEGIN SELECT RAISE(ABORT, 'subject import manifests are immutable'); END;
CREATE TRIGGER subject_import_manifests_no_delete
BEFORE DELETE ON subject_import_manifests
BEGIN SELECT RAISE(ABORT, 'subject import manifests are immutable'); END;

-- One receipt per authority operation, replayable rather than recomputed.
CREATE TABLE subject_authority_receipts (
    id          TEXT NOT NULL PRIMARY KEY
                     CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id  TEXT NOT NULL,
    subject     TEXT NOT NULL CHECK (subject IN ('memory', 'backlog')),
    -- Only the three operations that actually write. A preview earns no receipt
    -- because it changes nothing.
    operation   TEXT NOT NULL CHECK (operation IN ('import', 'attest', 'switch')),
    input_hash  TEXT NOT NULL
                     CHECK (length(input_hash) = 64 AND input_hash NOT GLOB '*[^0-9a-f]*'),
    result_hash TEXT NOT NULL
                     CHECK (length(result_hash) = 64 AND result_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at TEXT NOT NULL
                     CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    FOREIGN KEY (project_id, subject)
        REFERENCES project_subject_authority (project_id, subject) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_subject_authority_receipts_subject
    ON subject_authority_receipts (project_id, subject, recorded_at);

CREATE TRIGGER subject_authority_receipts_no_update
BEFORE UPDATE ON subject_authority_receipts
BEGIN SELECT RAISE(ABORT, 'subject authority receipts are immutable'); END;
CREATE TRIGGER subject_authority_receipts_no_delete
BEFORE DELETE ON subject_authority_receipts
BEGIN SELECT RAISE(ABORT, 'subject authority receipts are immutable'); END;

-- Seed every project that already exists.
--
-- Backlog is seeded native because these projects' graphs were created in
-- Kontor: their epics, tasks and dependencies have no AgentsRoom original to
-- import, and declaring them pending would invent a second writer for facts
-- Kontor already owns.
INSERT INTO project_subject_authority (project_id, subject, origin, authority, revision)
SELECT id, 'backlog', 'kontor_native', 'kontor', 1 FROM projects;

-- Memory is seeded from what the singleton actually claimed. A Realm that had
-- already switched holds imported memory Kontor owns, which is native from here
-- on; one that had not still has a legacy source to import per project.
INSERT INTO project_subject_authority (project_id, subject, origin, authority, revision)
SELECT p.id,
       'memory',
       CASE a.authority WHEN 'kontor' THEN 'kontor_native'
                        ELSE 'legacy_pending' END,
       a.authority,
       1
FROM projects p JOIN memory_authority a ON a.singleton = 1;

-- The singleton keeps its last claim as evidence and stops being writable. The
-- route that used to update it refuses in the API; this is the guarantee that a
-- missed caller cannot reach it either.
CREATE TRIGGER memory_authority_frozen_by_v32
BEFORE UPDATE ON memory_authority
BEGIN SELECT RAISE(ABORT, 'realm-wide memory authority was replaced by project_subject_authority'); END;

PRAGMA user_version = 32;
