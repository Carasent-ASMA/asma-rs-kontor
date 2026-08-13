-- ===========================================================================
-- Schema v14. Operator-registered profile packs.
--
-- The profile catalogue was compiled into the binary and had no other source, so
-- a deployment could not introduce a work profile or a team template of its own
-- without a rebuild. This is where a registered pack lives.
--
-- Realm-scoped, not project-scoped, and deliberately: a pack is a *catalogue*.
-- The bundled one is already realm-wide, `resolve_profile` resolves a category
-- against a whole pack, and scoping a second source per project would mean the
-- same category name resolving to two different phase DAGs in one database.
--
-- `(pack_id, version)` is the primary key and rows are never updated: a pack
-- revision is immutable, exactly as a work-profile or team-template revision is.
-- Registering the same bytes again is a replay; registering different bytes at
-- the same version is a conflict the application refuses. Publishing a change
-- means publishing the next version, which is what makes a frozen epic's pin
-- still mean what it meant.
--
-- The whole canonical document is stored rather than exploded into tables. The
-- pack's structure is already validated in Rust by `validate_pack`, which knows
-- rules SQL cannot state — that a slot's role is required at that exact
-- revision, that gate authority does not overlap between evaluating and waiving
-- — and a second, weaker copy of those rules in `CHECK` constraints would be a
-- place for the two to disagree. `document_hash` is what makes the stored bytes
-- self-verifying on read.
-- ===========================================================================

CREATE TABLE registered_profile_packs (
    pack_id        TEXT    NOT NULL CHECK (length(pack_id) BETWEEN 1 AND 128),
    version        INTEGER NOT NULL CHECK (version >= 1),
    document       TEXT    NOT NULL CHECK (json_valid(document)),
    document_hash  TEXT    NOT NULL
                           CHECK (length(document_hash) = 64
                                  AND document_hash NOT GLOB '*[^0-9a-f]*'),
    registered_at  TEXT    NOT NULL
                           CHECK (registered_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (pack_id, version)
) STRICT;

-- A pack revision is immutable once registered. Both triggers exist because the
-- application is not the only thing that can reach this table, and a catalogue
-- that could be edited underneath a frozen epic would make its pinned profile
-- mean something else than it did when the graph was applied.
CREATE TRIGGER registered_profile_packs_are_immutable
BEFORE UPDATE ON registered_profile_packs
BEGIN
    SELECT RAISE(ABORT, 'a registered profile pack revision is immutable');
END;

CREATE TRIGGER registered_profile_packs_are_permanent
BEFORE DELETE ON registered_profile_packs
BEGIN
    SELECT RAISE(ABORT, 'a registered profile pack revision is never removed');
END;

PRAGMA user_version = 14;
