-- Topology specification publication and the explicit epic upgrade.
--
-- Two changes, both forced by operations that did not exist when v23 was
-- written: publishing a specification revision through `/v1`, and moving one
-- epic's pin to a different published revision.

-- An epic's pin is its *current position*, not evidence.
--
-- v23 made the row permanently unwritable, which was right while nothing could
-- move it: a pin that drifted silently would relabel every node already placed
-- under it. The Operational contract now has an explicit upgrade — preview the
-- effects, then apply the exact preview under the epic's expected revision — so
-- the pin has to be able to move, once, deliberately, through that operation.
--
-- Nothing about immutability is given up. The *specification revision* the pin
-- points at is still immutable and still permanent, which is the thing that
-- actually has to be frozen; and every move is recorded in `command_receipts`
-- with its canonical intent, which is where every other decision in this
-- control plane is audited. A second history table here would be a second
-- answer to a question the receipt ledger already answers.
--
-- The DELETE guard stays exactly as it was: an epic that has been pinned has
-- been pinned, and unpinning it would leave its nodes citing a revision the
-- epic no longer claims.
DROP TRIGGER mini_project_topology_snapshots_are_immutable;

CREATE TRIGGER mini_project_topology_snapshots_keep_their_epic
BEFORE UPDATE ON mini_project_topology_snapshots
BEGIN
    SELECT RAISE(ABORT, 'a topology pin belongs to its epic and its project')
    WHERE OLD.mini_project_id <> NEW.mini_project_id
       OR OLD.project_id <> NEW.project_id;
END;

-- Widen the closed command-kind list by the two publication commands.
--
-- Same rebuild shape as v24 and v28, and for the same reason: `kind` is a
-- CHECK, so a new command is a migration rather than a code change.
CREATE TABLE command_receipts_v29 (
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
                                 'publish_topology_spec', 'upgrade_topology')),
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

INSERT INTO command_receipts_v29
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v29 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

PRAGMA user_version = 29;
