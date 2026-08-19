-- The open-question ledger (OP-REQ-038).
--
-- An unresolved ambiguity is a durable record, not a note in a transcript, and
-- an epic carrying an undispositioned one cannot reach `done`. That gate is only
-- worth having if the history behind it cannot be quietly edited, so the shape
-- here is one mutable head row and three append-only child tables.
--
-- What may change: `open_questions.revision`, and nothing else. Every other
-- column of the head is written once, and all three child tables refuse UPDATE
-- and DELETE outright. A correction is a new round or a new disposition that
-- names the one it supersedes; the predecessor keeps its exact bytes. This is
-- the whole point — a ledger that could be rewritten would record that we were
-- always right, which is not information.
--
-- Three domain rules are enforced here rather than only in the aggregate,
-- because a rule that lives in one layer is a rule that direct SQL can walk
-- around:
--
--   * a `deferred` disposition must name the trigger that reopens it, and only
--     a `deferred` one may;
--   * a firing must match the exact trigger its deferral named;
--   * one deferral reopens at most once.
--
-- Tenant isolation does not rest on UUIDs being globally unique: every child
-- key carries `project_id` and every foreign key into the head is composite, so
-- a valid id from another project cannot resolve.

CREATE TABLE open_questions (
    question_id       TEXT    NOT NULL PRIMARY KEY
                              CHECK (length(question_id) = 36
                                     AND question_id NOT GLOB '*[^0-9a-f-]*'),
    project_id        TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id   TEXT    NOT NULL,
    subject           TEXT    NOT NULL CHECK (length(trim(subject)) > 0),
    scope             TEXT    NOT NULL CHECK (scope IN (
                                  'architecture', 'product', 'process', 'routing')),
    attachment        TEXT    NOT NULL CHECK (json_valid(attachment)),
    author_seat_id    TEXT    NOT NULL REFERENCES seat_bindings (id) ON DELETE RESTRICT,
    shareability_class TEXT   NOT NULL
                              CHECK (shareability_class IN ('project_shared', 'kontor_local')),
    shareability_classifier TEXT,
    shareability_provenance TEXT NOT NULL
                              CHECK (shareability_provenance IN ('type_default', 'human_override')),
    created_at        TEXT    NOT NULL
                              CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revision          INTEGER NOT NULL CHECK (revision >= 1),
    UNIQUE (project_id, question_id),
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX open_questions_by_epic
    ON open_questions (project_id, mini_project_id, created_at);

-- The same pairing rule 0025 applies to every other classified record: an
-- override is attributable and a default rule is not.
CREATE TRIGGER open_questions_shareability_is_attributable
BEFORE INSERT ON open_questions
WHEN (NEW.shareability_provenance = 'human_override')
     <> (NEW.shareability_classifier IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'shareability classifier identity and provenance disagree');
END;

-- A question is raised by a seat of its own project.
CREATE TRIGGER open_questions_author_belongs_to_the_project
BEFORE INSERT ON open_questions
WHEN NOT EXISTS (
    SELECT 1 FROM seat_bindings
     WHERE seat_bindings.id = NEW.author_seat_id
       AND seat_bindings.project_id = NEW.project_id
)
BEGIN
    SELECT RAISE(ABORT, 'an open question is raised by a seat of its own project');
END;

CREATE TRIGGER open_questions_only_the_revision_moves
BEFORE UPDATE ON open_questions
WHEN OLD.question_id <> NEW.question_id
  OR OLD.project_id <> NEW.project_id
  OR OLD.mini_project_id <> NEW.mini_project_id
  OR OLD.subject <> NEW.subject
  OR OLD.scope <> NEW.scope
  OR OLD.attachment <> NEW.attachment
  OR OLD.author_seat_id <> NEW.author_seat_id
  OR OLD.shareability_class <> NEW.shareability_class
  OR IFNULL(OLD.shareability_classifier, '') <> IFNULL(NEW.shareability_classifier, '')
  OR OLD.shareability_provenance <> NEW.shareability_provenance
  OR OLD.created_at <> NEW.created_at
BEGIN
    SELECT RAISE(ABORT, 'an open question header is immutable except for its revision');
END;

CREATE TRIGGER open_questions_are_permanent
BEFORE DELETE ON open_questions
BEGIN
    SELECT RAISE(ABORT, 'an open question cannot be deleted');
END;

-- ---------------------------------------------------------------------------
-- Rounds
-- ---------------------------------------------------------------------------

CREATE TABLE open_question_rounds (
    project_id    TEXT    NOT NULL,
    question_id   TEXT    NOT NULL,
    ordinal       INTEGER NOT NULL CHECK (ordinal >= 1),
    author_seat_id TEXT   NOT NULL REFERENCES seat_bindings (id) ON DELETE RESTRICT,
    why_ambiguous TEXT    NOT NULL CHECK (length(trim(why_ambiguous)) > 0),
    options       TEXT    NOT NULL CHECK (json_valid(options)
                                          AND json_array_length(options) >= 1),
    supersedes    INTEGER CHECK (supersedes IS NULL OR supersedes >= 1),
    recorded_at   TEXT    NOT NULL,
    PRIMARY KEY (project_id, question_id, ordinal),
    FOREIGN KEY (project_id, question_id)
        REFERENCES open_questions (project_id, question_id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER open_question_rounds_are_immutable
BEFORE UPDATE ON open_question_rounds
BEGIN
    SELECT RAISE(ABORT, 'an open-question round is immutable; a correction appends');
END;

CREATE TRIGGER open_question_rounds_are_permanent
BEFORE DELETE ON open_question_rounds
BEGIN
    SELECT RAISE(ABORT, 'an open-question round cannot be withdrawn');
END;

CREATE TRIGGER open_question_rounds_supersede_an_existing_round
BEFORE INSERT ON open_question_rounds
WHEN NEW.supersedes IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM open_question_rounds
     WHERE project_id = NEW.project_id
       AND question_id = NEW.question_id
       AND ordinal = NEW.supersedes
)
BEGIN
    SELECT RAISE(ABORT, 'a correction must supersede a round that exists');
END;

-- ---------------------------------------------------------------------------
-- Dispositions
-- ---------------------------------------------------------------------------

CREATE TABLE open_question_dispositions (
    project_id     TEXT    NOT NULL,
    question_id    TEXT    NOT NULL,
    ordinal        INTEGER NOT NULL CHECK (ordinal >= 1),
    author_seat_id TEXT    NOT NULL REFERENCES seat_bindings (id) ON DELETE RESTRICT,
    kind           TEXT    NOT NULL
                           CHECK (kind IN ('resolved', 'deferred', 'not_relevant')),
    -- Exactly the deferrals name a trigger. Storing it as its own column rather
    -- than only inside the payload is what lets the firing rule below be a
    -- schema constraint instead of a convention.
    trigger_key    TEXT    CHECK (trigger_key IS NULL OR length(trim(trigger_key)) > 0),
    payload        TEXT    NOT NULL CHECK (json_valid(payload)),
    supersedes     INTEGER CHECK (supersedes IS NULL OR supersedes >= 1),
    recorded_at    TEXT    NOT NULL,
    PRIMARY KEY (project_id, question_id, ordinal),
    CHECK ((kind = 'deferred') = (trigger_key IS NOT NULL)),
    FOREIGN KEY (project_id, question_id)
        REFERENCES open_questions (project_id, question_id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER open_question_dispositions_are_immutable
BEFORE UPDATE ON open_question_dispositions
BEGIN
    SELECT RAISE(ABORT, 'a disposition is immutable; a correction appends and supersedes');
END;

CREATE TRIGGER open_question_dispositions_are_permanent
BEFORE DELETE ON open_question_dispositions
BEGIN
    SELECT RAISE(ABORT, 'a disposition cannot be withdrawn');
END;

CREATE TRIGGER open_question_dispositions_supersede_an_existing_one
BEFORE INSERT ON open_question_dispositions
WHEN NEW.supersedes IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM open_question_dispositions
     WHERE project_id = NEW.project_id
       AND question_id = NEW.question_id
       AND ordinal = NEW.supersedes
)
BEGIN
    SELECT RAISE(ABORT, 'a correction must supersede a disposition that exists');
END;

-- ---------------------------------------------------------------------------
-- Trigger firings
-- ---------------------------------------------------------------------------

CREATE TABLE open_question_trigger_firings (
    project_id          TEXT    NOT NULL,
    question_id         TEXT    NOT NULL,
    ordinal             INTEGER NOT NULL CHECK (ordinal >= 1),
    disposition_ordinal INTEGER NOT NULL CHECK (disposition_ordinal >= 1),
    trigger_key         TEXT    NOT NULL CHECK (length(trim(trigger_key)) > 0),
    observed_by_seat_id TEXT    NOT NULL REFERENCES seat_bindings (id) ON DELETE RESTRICT,
    recorded_at         TEXT    NOT NULL,
    PRIMARY KEY (project_id, question_id, ordinal),
    -- One deferral reopens at most once.
    UNIQUE (project_id, question_id, disposition_ordinal),
    FOREIGN KEY (project_id, question_id)
        REFERENCES open_questions (project_id, question_id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, question_id, disposition_ordinal)
        REFERENCES open_question_dispositions (project_id, question_id, ordinal)
        ON DELETE RESTRICT
) STRICT;

-- Only the exact trigger the deferral named reopens it. Without this, a firing
-- could reopen a question on a trigger nobody deferred it on.
CREATE TRIGGER open_question_firing_matches_its_deferral
BEFORE INSERT ON open_question_trigger_firings
WHEN NOT EXISTS (
    SELECT 1 FROM open_question_dispositions
     WHERE project_id = NEW.project_id
       AND question_id = NEW.question_id
       AND ordinal = NEW.disposition_ordinal
       AND kind = 'deferred'
       AND trigger_key = NEW.trigger_key
)
BEGIN
    SELECT RAISE(ABORT, 'a firing must name the exact trigger its deferral deferred on');
END;

CREATE TRIGGER open_question_firings_are_immutable
BEFORE UPDATE ON open_question_trigger_firings
BEGIN
    SELECT RAISE(ABORT, 'a trigger firing is immutable');
END;

CREATE TRIGGER open_question_firings_are_permanent
BEFORE DELETE ON open_question_trigger_firings
BEGIN
    SELECT RAISE(ABORT, 'a trigger firing cannot be withdrawn');
END;

PRAGMA user_version = 41;
