-- ===========================================================================
-- Schema v7 — holiday import provenance (KON-MVP-21)
--
-- One table, two triggers on it and one trigger on `calendar_exceptions`. The
-- calendar model itself is schema v1's and stays exactly as it is: profiles,
-- assignments, holiday sources, exceptions and overrides are already the right
-- shape, and this migration adds only what an *import* knows that a stored
-- source revision cannot say.
--
-- Four decisions, each with a tempting alternative.
--
--  1. **The importer is named in a new column, not in `provider`.** v1's
--     `holiday_sources.provider` is `CHECK (provider IN ('ical','manual',
--     'bundled'))`, written before any importer existed, and SQLite cannot widen
--     a v1 CHECK — the same wall migration 0004 hit with `released_at`. So the
--     coarse v1 value keeps meaning what it meant (a retrieved feed, a human, a
--     shipped set) and `holiday_import_batches.import_kind` records which
--     importer actually read the bytes: `nager_v4`, `gov_uk_json` or `ical`. A
--     reader who needs the distinction has it in one join, and no v1 row had to
--     be reinterpreted to get it.
--
--  2. **One row per source revision, not a second copy of it.** The batch table
--     is keyed *by* `holiday_sources.id`. What it adds is the request — the
--     range asked for, the categories selected — and the outcome — the warnings,
--     how many exceptions were written. Those are facts about the import, not
--     about the holidays, and putting them on the revision would make a
--     revision's identity depend on how it was asked for.
--
--  3. **"Currently applied" is derived, never a flag.** An `active` column would
--     have to be flipped on the previous row, and every evidence table in this
--     schema is append-only through BEFORE UPDATE/DELETE triggers. So a batch
--     names the revision it supersedes and the current one is the batch nothing
--     supersedes. The unique index on `supersedes` forbids a fork, and the
--     insert trigger forbids a second chain: a new batch for a calendar that
--     already has a current import must supersede exactly that import. What
--     falls out is one applied import per calendar, enforced without a single
--     UPDATE.
--
--  4. **A holiday-source provenance must resolve.** `calendar_exceptions`
--     carries its provenance as JSON, so v1 could not give it a foreign key. The
--     trigger below is that foreign key: an exception claiming to come from a
--     holiday source is refused unless that source revision exists. Without it,
--     an import's exceptions could outlive — or precede — the revision that is
--     supposed to explain them, and resolution would close a day for a reason
--     nobody could look up.
--
-- The two indexes resolution needs already exist and are not restated here:
-- v1's `ux_work_calendars_active` (the active assignment per project) and
-- `ix_calendar_exceptions_calendar` (exceptions by calendar and start date).
-- ===========================================================================

CREATE TABLE holiday_import_batches (
    -- The revision this provenance belongs to. One-to-one by construction:
    -- the source revision's own id is this table's primary key.
    source_id          TEXT    NOT NULL PRIMARY KEY
                               REFERENCES holiday_sources (id) ON DELETE RESTRICT,
    project_id         TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    work_calendar_id   TEXT    NOT NULL,
    -- Decision 1. The precise importer, beside v1's coarse `provider`.
    import_kind        TEXT    NOT NULL CHECK (import_kind IN
                               ('nager_v4', 'gov_uk_json', 'ical')),
    -- What the request asked for, which is not what the revision covers: an
    -- import that found nothing still has a range, and only these two columns
    -- can tell "the source listed no holidays" from "nobody asked for any".
    requested_start    TEXT    NOT NULL
                               CHECK (requested_start GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    requested_end      TEXT    NOT NULL
                               CHECK (requested_end GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    -- The selected categories, as a JSON array of the domain spellings. Strict
    -- JSON like every other document column in this schema.
    categories         TEXT    NOT NULL CHECK (json_valid(categories)
                                               AND json_type(categories) = 'array'
                                               AND json_array_length(categories) >= 1),
    -- What the importer refused or dropped: an array of `{code, entry}`. Codes
    -- and positions only — an import warning never carries the source value it
    -- refused.
    warnings           TEXT    NOT NULL CHECK (json_valid(warnings)
                                               AND json_type(warnings) = 'array'),
    applied_exceptions INTEGER NOT NULL CHECK (applied_exceptions >= 0),
    -- Decision 3. NULL for the first import of a calendar.
    supersedes         TEXT    NULL REFERENCES holiday_sources (id) ON DELETE RESTRICT,
    -- The caller's replay key. A repeat returns the original apply rather than
    -- writing a second one.
    idempotency_key    TEXT    NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    applied_at         TEXT    NOT NULL
                               CHECK (applied_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    CHECK (requested_start <= requested_end),
    CHECK (supersedes IS NULL OR supersedes <> source_id),
    UNIQUE (project_id, work_calendar_id, idempotency_key),
    FOREIGN KEY (project_id, work_calendar_id)
        REFERENCES work_calendars (project_id, id) ON DELETE RESTRICT
) STRICT;

-- The chain has one head and no forks: two batches cannot both replace the same
-- revision, which is what would produce two "current" imports for one calendar.
CREATE UNIQUE INDEX ux_holiday_import_batches_supersedes
    ON holiday_import_batches (supersedes) WHERE supersedes IS NOT NULL;

CREATE INDEX ix_holiday_import_batches_calendar
    ON holiday_import_batches (project_id, work_calendar_id, applied_at);

-- Decision 3. A calendar's first import supersedes nothing; every later one
-- supersedes exactly the import that is current at the time it is written.
-- `IS NOT` is the null-safe comparison, so both halves of that rule are this one
-- predicate: no current import and a non-NULL `supersedes` is refused just as a
-- current import and the wrong (or a NULL) `supersedes` is.
CREATE TRIGGER holiday_import_batches_supersede_current BEFORE INSERT ON holiday_import_batches
WHEN NEW.supersedes IS NOT (
    SELECT current.source_id FROM holiday_import_batches AS current
     WHERE current.project_id = NEW.project_id
       AND current.work_calendar_id = NEW.work_calendar_id
       AND NOT EXISTS (SELECT 1 FROM holiday_import_batches AS later
                        WHERE later.supersedes = current.source_id)
)
BEGIN SELECT RAISE(ABORT, 'an import must supersede the calendar''s current import'); END;

CREATE TRIGGER holiday_import_batches_no_update BEFORE UPDATE ON holiday_import_batches
BEGIN SELECT RAISE(ABORT, 'an applied import is immutable'); END;

CREATE TRIGGER holiday_import_batches_no_delete BEFORE DELETE ON holiday_import_batches
BEGIN SELECT RAISE(ABORT, 'an applied import is not deletable'); END;

-- Decision 4. The foreign key v1 could not write, because the reference lives
-- inside a JSON document.
CREATE TRIGGER calendar_exceptions_holiday_source_exists BEFORE INSERT ON calendar_exceptions
WHEN json_extract(NEW.provenance, '$.kind') = 'holiday_source'
 AND NOT EXISTS (SELECT 1 FROM holiday_sources
                  WHERE id = json_extract(NEW.provenance, '$.source_id'))
BEGIN SELECT RAISE(ABORT, 'an imported exception must name a stored holiday source'); END;

PRAGMA user_version = 7;
