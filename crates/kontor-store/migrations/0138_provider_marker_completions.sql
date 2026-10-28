-- ===========================================================================
-- Schema v138. Durable completion state for the provider-marker correction.
--
-- A realm idempotency binding records the *claim*, not whether the effect it
-- names finished. For a file effect the document is the only other witness, and
-- an operator may restore the marker for a later outage; without this row a
-- replay of the completed key would remove that restored marker a second time.
-- The row is written only after a successful apply (or a proven already-cleared)
-- and names the revision the correction left behind, so every later use of the
-- key is a replay that never writes. It also carries the host key and the
-- before/after sets the successful apply reported, so a replay is answered from
-- durable state even after the addressed setting or the whole document has been
-- removed.
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
    host_key         TEXT NOT NULL
                          CHECK (length(host_key) BETWEEN 1 AND 256),
    before_providers TEXT NOT NULL,
    after_providers  TEXT NOT NULL,
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

-- The phase between the durable claim and durable completion. A key whose
-- write started but never completed is ambiguous from the document alone: the
-- marker can be present because the write never landed, or because an operator
-- restored it for a new outage after the write landed. Refusing to repeat the
-- write while the marker is present keeps the restored marker, and a fresh
-- preview and key are the supported way forward. A marker already absent still
-- recovers, because that is the durable effect the key names.
CREATE TABLE provider_marker_write_starts (
    idempotency_key TEXT NOT NULL PRIMARY KEY
                          CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    started_at      TEXT NOT NULL
                          CHECK (started_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

CREATE TRIGGER provider_marker_write_starts_are_immutable
BEFORE UPDATE ON provider_marker_write_starts
BEGIN
    SELECT RAISE(ABORT, 'a provider-marker write start is never rewritten');
END;

CREATE TRIGGER provider_marker_write_starts_are_permanent
BEFORE DELETE ON provider_marker_write_starts
BEGIN
    SELECT RAISE(ABORT, 'a provider-marker write start is never released');
END;

PRAGMA user_version = 138;
