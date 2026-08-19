-- Preserve the source lifecycle supplied by an epic import without turning a
-- historical terminal fact into evidence of native Kontor closure.
ALTER TABLE tasks ADD COLUMN imported_state TEXT NULL
    CHECK (imported_state IS NULL OR imported_state IN ('ready', 'completed'));

PRAGMA user_version = 42;
