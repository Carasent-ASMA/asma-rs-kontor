-- ===========================================================================
-- Schema v8. Four command kinds the intake, conflict, comment and ownership
-- surfaces need.
--
-- Same reasoning and same mechanism as v6: `command_receipts.kind` is a closed
-- `CHECK` list, SQLite cannot alter a `CHECK` in place, and `migrate` lifts
-- reference enforcement around the transaction and runs `PRAGMA
-- foreign_key_check` over the whole database before committing.
--
-- The four kinds, and why none of them borrows an existing one:
--
-- * `submit_intake` — deciding an inbound event under a pinned trigger. Sharing
--   `approve_intake` would let a receipt that merely *recorded a decision* be
--   cited as the human approval an approved intake requires.
-- * `resolve_status_conflict` already exists and is reused deliberately: closing
--   a reconciliation conflict is exactly the decision that kind names.
-- * `pull_ticket_comments` — mirroring inbound comment revisions. It reads the
--   external system and writes only the mirror; `sync_ticket` is the kind that
--   writes *to* the ticket, and the two must not be interchangeable.
-- * `claim_ticket` — taking ownership of a ticket for the principal Kontor
--   authenticates as. `assign_ticket` can name any assignee the connector
--   accepts; a claim can name only the principal, so a claim receipt must not be
--   replayable as the authority for an arbitrary assignment.
-- ===========================================================================

CREATE TABLE command_receipts_v8 (
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
                                 'claim_ticket')),
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

INSERT INTO command_receipts_v8
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v8 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

PRAGMA user_version = 8;
