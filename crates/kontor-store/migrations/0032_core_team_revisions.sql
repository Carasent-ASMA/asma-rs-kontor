-- Immutable Project Core Team revisions.
--
-- The Core Team is project *configuration*: which standard roles this project
-- staffs an epic with, under which presence policy, and which of them may open
-- a Quick session. It is not a TeamRun and not a set of live seats, so it is
-- stored as its own small aggregate rather than as rows in the seat tables —
-- a seat here would be a seat nothing is running.
--
-- Every revision is kept. Promotion freezes the exact revision an epic was
-- created under, and that snapshot has to stay readable for the whole life of
-- the epic even after the project has edited its defaults ten times. Storing
-- only the current composition would make "the roster this epic was promoted
-- with" unanswerable the moment the project moved on.
--
-- There is deliberately no `is_current` column. The current revision is the
-- highest version a project has published, which is one fact derived from the
-- rows rather than a second fact that can disagree with them. A stored flag
-- would need its own transaction to stay true, and the first partial write
-- would leave the project with two current rosters or none.
CREATE TABLE core_team_revisions (
    project_id   TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    version      INTEGER NOT NULL CHECK (version >= 1),
    -- The exact role catalog this revision resolved its codes against. Held so
    -- a later read can prove the titles it reports came from that revision
    -- rather than from whichever catalog happens to be current now.
    catalog_hash TEXT    NOT NULL
                         CHECK (length(catalog_hash) = 64 AND catalog_hash NOT GLOB '*[^0-9a-f]*'),
    -- The resolved seats in their declared order, as the domain canonicalizes
    -- them. Kept as one canonical document because the order is part of the
    -- published fact, and a child table would re-sort it on every read.
    seats        TEXT    NOT NULL CHECK (json_valid(seats)),
    created_at   TEXT    NOT NULL
                         CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, version)
) STRICT;

-- A published revision is published. Both guards exist for the same reason the
-- topology specification has them: an epic pins a version, so editing or
-- removing one would silently change what an already-promoted epic claims it
-- was staffed with.
CREATE TRIGGER core_team_revisions_are_immutable
BEFORE UPDATE ON core_team_revisions
BEGIN
    SELECT RAISE(ABORT, 'a published Core Team revision is immutable');
END;

CREATE TRIGGER core_team_revisions_are_permanent
BEFORE DELETE ON core_team_revisions
BEGIN
    SELECT RAISE(ABORT, 'a published Core Team revision cannot be withdrawn');
END;

-- Widen the closed command-kind list by the Core Team publication command.
--
-- Same rebuild shape as v24, v28 and v29, and for the same reason: `kind` is a
-- CHECK, so a new command is a migration rather than a code change.
CREATE TABLE command_receipts_v32 (
    id               TEXT    NOT NULL PRIMARY KEY
                             CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id       TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    idempotency_key  TEXT    NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    kind             TEXT    NOT NULL CHECK (kind IN (
                                 'launch_run', 'cancel_run', 'park_run', 'abandon_run',
                                 'resume_task', 'record_gate_verdict', 'approve_intake',
                                 'sync_ticket', 'assign_ticket', 'transition_ticket',
                                 'authorize_execution', 'approve_schedule_override',
                                 'revoke_schedule_override', 'resolve_status_conflict',
                                 'assign_work_calendar', 'revoke_execution_authorization',
                                 'ensure_project', 'ensure_account_profile',
                                 'apply_epic_graph', 'transition_epic',
                                 'start_scheduled_work', 'transition_task',
                                 'resolve_context', 'select_task_profile',
                                 'select_task_team', 'select_task_account',
                                 'reconcile_ticket', 'settle_runtime',
                                 'submit_intake', 'pull_ticket_comments',
                                 'claim_ticket', 'replace_seat',
                                 'refresh_capacity', 'override_availability',
                                 'observe_seat', 'retire_seat',
                                 'publish_topology_spec', 'upgrade_topology',
                                 'retitle_container',
                                 'apply_core_team')),
    target           TEXT    NOT NULL CHECK (json_valid(target)),
    target_revision  INTEGER NOT NULL CHECK (target_revision >= 1),
    intent           TEXT    NOT NULL CHECK (json_valid(intent)),
    intent_hash      TEXT    NOT NULL
                             CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    state            TEXT    NOT NULL CHECK (state IN (
                                 'intent_persisted', 'dispatch_pending', 'dispatched',
                                 'acknowledged', 'confirmation_unknown', 'confirmed', 'failed')),
    correlation      TEXT    NULL CHECK (correlation IS NULL OR length(correlation) BETWEEN 1 AND 256),
    native_identity  TEXT    NULL CHECK (native_identity IS NULL OR json_valid(native_identity)),
    result_ref       TEXT    NULL CHECK (result_ref IS NULL OR length(result_ref) BETWEEN 1 AND 256),
    attempts         INTEGER NOT NULL CHECK (attempts >= 0),
    created_at       TEXT    NOT NULL
                             CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    updated_at       TEXT    NOT NULL
                             CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id)
) STRICT;

INSERT INTO command_receipts_v32
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v32 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

PRAGMA user_version = 32;
