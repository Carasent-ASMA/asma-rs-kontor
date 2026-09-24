-- Schema v116. The role persona one launched occupancy was opened under.
--
-- ASMA-8196 delivers a role's system prompt at seat creation. Delivering it is
-- not the same as being able to prove it was delivered: Paseo's
-- `config.systemPrompt` is creation-only, so nothing can later ask a native
-- which persona it is running under. Without a durable record, "which persona
-- did this seat receive" can only be answered by re-reading today's
-- operational-domain pack -- which answers a different question, because an
-- edit to that pack after launch would silently rewrite the answer to a
-- question about the past.
--
-- This table is that record. It is written before the native call, beside the
-- launch intent, so a launch whose acknowledgement is lost still leaves behind
-- what it was going to deliver. It is keyed per occupancy rather than per seat,
-- so a replacement gets its own row: a successor launched with no persona
-- cannot be read as the predecessor that had one, which is exactly the
-- confusion a per-seat row would permit.
--
-- `delivery` is the honest half. It records what the runtime's acceptance
-- actually proves, and today there is exactly one answer: the text was written
-- into the create request and the runtime offers no readback. A runtime that
-- can one day attest its applied prompt earns a new value here rather than a
-- silent reinterpretation of this one.
--
-- Nothing here creates or mutates a seat, occupancy, native session, agent run
-- or topology node.
CREATE TABLE hosted_seat_role_personas (
    project_id           TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    seat_binding_id      TEXT NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    occupancy_generation INTEGER NOT NULL CHECK (occupancy_generation >= 1),
    -- Stored rather than derived, so a reader sees which role's persona was
    -- delivered without re-resolving the seat's role, which may itself have
    -- moved since.
    role_code            TEXT NOT NULL CHECK (length(role_code) BETWEEN 1 AND 128),
    -- The exact delivered text, kept beside its digest. The digest alone proves
    -- integrity but not content, and re-deriving content from the pack is the
    -- failure this table exists to prevent.
    prompt               TEXT NOT NULL CHECK (length(trim(prompt)) > 0),
    prompt_hash          TEXT NOT NULL
                              CHECK (length(prompt_hash) = 64
                                     AND prompt_hash NOT GLOB '*[^0-9a-f]*'),
    delivery             TEXT NOT NULL CHECK (delivery IN ('create_only_no_readback')),
    schema_version       INTEGER NOT NULL CHECK (schema_version >= 1),
    frozen_at            TEXT NOT NULL
                              CHECK (frozen_at GLOB
                                     '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- One persona per occupancy, so a replay converges on the row it already
    -- wrote instead of recording a second answer for the same launch.
    PRIMARY KEY (project_id, seat_binding_id, occupancy_generation)
) STRICT;

-- A persona that could be edited would record whatever was last resolved rather
-- than what the seat was actually created under, which is the one property this
-- table exists to hold.
CREATE TRIGGER hosted_seat_role_personas_immutable
BEFORE UPDATE ON hosted_seat_role_personas
BEGIN
    SELECT RAISE(ABORT,
        'a launched occupancy''s role persona is immutable');
END;

CREATE TRIGGER hosted_seat_role_personas_no_delete
BEFORE DELETE ON hosted_seat_role_personas
BEGIN
    SELECT RAISE(ABORT,
        'a launched occupancy''s role persona is not deletable');
END;

PRAGMA user_version = 116;
