-- Schema v132. Dated durable desk seat declarations, and the two receipt kinds
-- that declare and reconcile them (ASMA-8451).
--
-- Forward-only and additive. Every existing table, row, constraint, trigger
-- and index is kept. The one new table records which long-lived seats an
-- operator decided a durable desk (v131) should hold, who decided it and when,
-- and who may restart each seat. The receipt ledger is rebuilt exactly as v128
-- left it, with two more closed kinds.

-- 1. One immutable, permanent row per declared seat.
--
-- A daemon-owned seat plans its seat binding here, before anything is
-- launched, for the same reason a desk plans its nodes: the binding id is the
-- runtime correlation label, so every later attempt -- a replay, a retry after
-- a lost answer, a restarted daemon -- addresses the same logical seat instead
-- of creating a second one. The column is plain TEXT without a foreign key
-- because the binding is created later, at the first reconciliation that
-- launches the seat.
--
-- A Paseo-direct seat is run outside Kontor and plans no binding: there is
-- nothing for Kontor to correlate, adopt or restart.
CREATE TABLE durable_seat_declarations (
    project_id       TEXT    NOT NULL,
    desk_key         TEXT    NOT NULL,
    seat_key         TEXT    NOT NULL
                             CHECK (length(seat_key) BETWEEN 1 AND 128
                                    AND seat_key NOT GLOB '*[^a-z0-9._-]*'),
    title            TEXT    NOT NULL CHECK (length(title) BETWEEN 1 AND 512),
    role_code        TEXT    NOT NULL CHECK (length(role_code) BETWEEN 1 AND 64),
    seat_label       TEXT    NOT NULL CHECK (length(seat_label) BETWEEN 1 AND 512),
    model_rung       TEXT    NULL CHECK (model_rung IS NULL OR json_valid(model_rung)),
    procedure        TEXT    NULL CHECK (procedure IS NULL OR length(procedure) BETWEEN 1 AND 512),
    recovery_owner   TEXT    NOT NULL CHECK (recovery_owner IN ('daemon', 'paseo_direct')),
    decided_by       TEXT    NOT NULL CHECK (length(decided_by) BETWEEN 1 AND 512),
    decided_on       TEXT    NOT NULL
                             CHECK (decided_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    seat_binding_id  TEXT    NULL
                             CHECK (seat_binding_id IS NULL OR length(seat_binding_id) = 36),
    declared_at      TEXT    NOT NULL
                             CHECK (declared_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, desk_key, seat_key),
    -- One title names one seat in a desk, whoever owns it. A daemon-owned seat
    -- can therefore never be declared over a Paseo-direct seat's title.
    UNIQUE (project_id, desk_key, title),
    UNIQUE (project_id, seat_binding_id),
    -- The daemon restarts only what it can launch: a route and a planned
    -- binding. A Paseo-direct seat carries no binding at all.
    CHECK ((recovery_owner = 'daemon') = (seat_binding_id IS NOT NULL)),
    CHECK (recovery_owner <> 'daemon' OR model_rung IS NOT NULL),
    FOREIGN KEY (project_id, desk_key) REFERENCES desks (project_id, desk_key)
        ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER durable_seat_declarations_are_immutable
BEFORE UPDATE ON durable_seat_declarations
BEGIN
    SELECT RAISE(ABORT, 'a durable seat declaration is a dated decision and is immutable');
END;

CREATE TRIGGER durable_seat_declarations_are_permanent
BEFORE DELETE ON durable_seat_declarations
BEGIN
    SELECT RAISE(ABORT, 'a durable seat declaration is never withdrawn');
END;

-- 2. The v128 receipt shape with exactly two additional kinds.
CREATE TABLE command_receipts_v132 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    idempotency_key TEXT NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    kind TEXT NOT NULL CHECK (kind IN (
        'launch_run','cancel_run','park_run','abandon_run','resume_task','record_gate_verdict',
        'approve_intake','sync_ticket','assign_ticket','transition_ticket','authorize_execution',
        'approve_schedule_override','revoke_schedule_override','resolve_status_conflict',
        'assign_work_calendar','revoke_execution_authorization','ensure_project',
        'ensure_account_profile','apply_epic_graph','import_backlog','transition_epic',
        'start_scheduled_work','transition_task','resolve_context','select_task_profile',
        'select_task_team','select_task_account','correct_task_worktree','reconcile_ticket',
        'materialize_jira','activate_asma_epic','settle_runtime','submit_intake',
        'pull_ticket_comments','claim_ticket','replace_seat','refresh_capacity',
        'override_availability','observe_seat','retire_seat','publish_topology_spec',
        'select_project_topology','upgrade_topology','retitle_container',
        'reconcile_native_names','apply_core_team','ensure_quick_session',
        'promote_quick_session','materialize_core_team','correct_core_team_route',
        'claim_core_team_seat','upgrade_epic_roster','apply_advisor_profile',
        'apply_committee_template','apply_completion_profile','advance_completion',
        'remediate_completion','invoke_advisor_run','settle_advisor_run','invoke_committee_run',
        'record_committee_findings','settle_committee_run','recover_consultation_seat',
        'reroute_unmaterialized_consultation_seat','publish_trigger','install_workflow_spec',
        'withdraw_task','publish_team_definition','select_project_team_definition',
        'upgrade_team_definition','correct_epic_backlog_code','recover_topology_container',
        'recover_gate_rejection','publish_ticket_description','publish_epic_description',
        'attest_retired_evaluator_evidence',
        'apply_planning_pair_profile','invoke_planning_pair_run','record_planning_pair_finding',
        'request_planning_pair_clarification','record_planning_pair_answer',
        'record_planning_pair_disposition','recover_planning_pair_seat',
        'declare_durable_seat','reconcile_durable_seats')),
    target TEXT NOT NULL CHECK (json_valid(target)),
    target_revision INTEGER NOT NULL CHECK (target_revision >= 1),
    intent TEXT NOT NULL CHECK (json_valid(intent)),
    intent_hash TEXT NOT NULL CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    state TEXT NOT NULL CHECK (state IN ('intent_persisted','dispatch_pending','dispatched',
        'acknowledged','confirmation_unknown','confirmed','failed')),
    correlation TEXT NULL CHECK (correlation IS NULL OR length(correlation) BETWEEN 1 AND 256),
    native_identity TEXT NULL CHECK (native_identity IS NULL OR json_valid(native_identity)),
    result_ref TEXT NULL CHECK (result_ref IS NULL OR length(result_ref) BETWEEN 1 AND 256),
    attempts INTEGER NOT NULL CHECK (attempts >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    execution_mode TEXT NOT NULL DEFAULT 'dispatch' CHECK (execution_mode IN ('local','dispatch')),
    UNIQUE (project_id, id)
) STRICT;

INSERT INTO command_receipts_v132 SELECT * FROM command_receipts;
PRAGMA legacy_alter_table = ON;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v132 RENAME TO command_receipts;
PRAGMA legacy_alter_table = OFF;
CREATE INDEX ix_command_receipts_state ON command_receipts(project_id, state);
CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key OR OLD.target <> NEW.target
  OR OLD.intent <> NEW.intent OR OLD.intent_hash <> NEW.intent_hash
  OR OLD.kind <> NEW.kind OR OLD.project_id <> NEW.project_id
  OR OLD.state IN ('confirmed','failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;
CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

PRAGMA user_version = 132;
