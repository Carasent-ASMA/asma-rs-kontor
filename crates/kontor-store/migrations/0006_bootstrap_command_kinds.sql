-- ===========================================================================
-- Schema v6. Three command kinds the bootstrap and disarm paths need.
--
-- `command_receipts.kind` is a closed `CHECK` list, deliberately: a kind SQL
-- accepts but the domain cannot parse would be an unreadable row, and a kind
-- the domain knows and SQL does not would fail only at runtime, on the
-- authority paths that consume these receipts. Widening it is therefore a
-- migration and not a code change.
--
-- SQLite cannot alter a `CHECK` constraint in place, so the table is rebuilt:
-- new shape, copy, drop, rename. `command_receipt_transitions`,
-- `command_targets`, `command_outbox`, `execution_authorizations`,
-- `schedule_overrides` and `execution_authorization_revocations` all reference
-- it, so for the two statements between the `DROP` and the `RENAME` every one of
-- those rows points at a table that does not exist.
--
-- That is safe here and nowhere else: `migrate` lifts reference enforcement
-- around the whole migration transaction and runs `PRAGMA foreign_key_check`
-- over the entire database before committing, so a rebuild that genuinely
-- stranded a row rolls back. The pragma cannot be written in this script — it is
-- silently ignored inside a transaction — which is exactly why it lives in
-- `migrate` instead.
--
-- The three kinds:
--
-- * `revoke_execution_authorization` — disarming is its own grant. Reusing
--   `revoke_schedule_override` would let a receipt that closed a calendar
--   window be replayed as the authority that disarmed the work.
-- * `ensure_project` / `ensure_account_profile` — the two bootstrap mutations,
--   so they record durable receipts on the same path as every other mutation
--   rather than on a second idempotency mechanism that could disagree with it.
-- ===========================================================================

CREATE TABLE command_receipts_v6 (
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
                                 'reconcile_ticket', 'settle_runtime')),
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

INSERT INTO command_receipts_v6
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v6 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

PRAGMA user_version = 6;
