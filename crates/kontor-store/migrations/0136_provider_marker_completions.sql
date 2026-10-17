-- ===========================================================================
-- Schema v136. Durable completion state for the provider-marker correction.
--
-- A realm idempotency binding records the *claim*, not whether the effect it
-- names finished. For a file effect the document is the only other witness, and
-- an operator may restore the marker for a later outage; without this row a
-- replay of the completed key would remove that restored marker a second time.
-- The row is written only after a successful apply (or a proven already-cleared)
-- and names the revision the correction left behind, so every later use of the
-- key is a replay that never writes.
--
-- Completion is permanent: a completed act is never reopened, and a key that
-- has completed can never turn back into an unfinished claim (ASMA-7968).
-- ===========================================================================

CREATE TABLE provider_marker_completions (
    idempotency_key  TEXT NOT NULL PRIMARY KEY
                          CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    written_revision TEXT NOT NULL
                          CHECK (length(written_revision) = 64
                                 AND written_revision NOT GLOB '*[^0-9a-f]*'),
    completed_at     TEXT NOT NULL
                          CHECK (completed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

CREATE TRIGGER provider_marker_completions_are_immutable
BEFORE UPDATE ON provider_marker_completions
BEGIN
    SELECT RAISE(ABORT, 'a provider-marker completion is never rewritten');
END;

CREATE TRIGGER provider_marker_completions_are_permanent
BEFORE DELETE ON provider_marker_completions
BEGIN
    SELECT RAISE(ABORT, 'a provider-marker completion is never released');
END;

PRAGMA user_version = 136;
