-- Schema v68. Native-less Committee materialization recovery.
--
-- A consultation launch can fail before a native identity exists. This
-- lineage preserves the original admitted route and the exact Admin-authorized
-- replacement while the logical run, topology and SeatBinding stay frozen.
CREATE TABLE consultation_seat_materialization_reroutes (
    project_id             TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    run_id                 TEXT    NOT NULL,
    role_slot_id           TEXT    NOT NULL,
    seat_binding_id        TEXT    NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    predecessor_generation INTEGER NOT NULL CHECK (predecessor_generation >= 1),
    successor_generation   INTEGER NOT NULL CHECK (successor_generation = predecessor_generation + 1),
    predecessor_model_rung TEXT    NOT NULL CHECK (json_valid(predecessor_model_rung)),
    successor_model_rung   TEXT    NOT NULL CHECK (json_valid(successor_model_rung)),
    reason                 TEXT    NOT NULL CHECK (reason = 'permission_mode_unsupported'),
    recovery_profile       TEXT    NOT NULL CHECK (json_valid(recovery_profile)),
    recovery_profile_hash  TEXT    NOT NULL CHECK (
                                  length(recovery_profile_hash) = 64
                                  AND recovery_profile_hash NOT GLOB '*[^0-9a-f]*'),
    request_intent_hash    TEXT    NOT NULL CHECK (
                                  length(request_intent_hash) = 64
                                  AND request_intent_hash NOT GLOB '*[^0-9a-f]*'),
    idempotency_key        TEXT    NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    headroom_account_profile_id TEXT NOT NULL,
    headroom_observation_id TEXT NOT NULL REFERENCES provider_usage_observations(id) ON DELETE RESTRICT,
    headroom_evidence_hash TEXT NOT NULL CHECK (
                                  length(headroom_evidence_hash) = 64
                                  AND headroom_evidence_hash NOT GLOB '*[^0-9a-f]*'),
    predecessor_revision   INTEGER NOT NULL CHECK (predecessor_revision >= 1),
    successor_revision     INTEGER NOT NULL CHECK (successor_revision = predecessor_revision + 1),
    rerouted_at            TEXT    NOT NULL,
    PRIMARY KEY (project_id, run_id, role_slot_id, predecessor_generation),
    UNIQUE (project_id, run_id, role_slot_id, successor_generation),
    UNIQUE (project_id, request_intent_hash),
    FOREIGN KEY (project_id, run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT,
    FOREIGN KEY (run_id, role_slot_id)
        REFERENCES consultation_seats(run_id, role_slot_id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, headroom_account_profile_id)
        REFERENCES account_profiles(project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER consultation_seat_materialization_reroutes_are_immutable
BEFORE UPDATE ON consultation_seat_materialization_reroutes
BEGIN SELECT RAISE(ABORT, 'a materialization reroute is immutable'); END;

CREATE TRIGGER consultation_seat_materialization_reroutes_are_permanent
BEFORE DELETE ON consultation_seat_materialization_reroutes
BEGIN SELECT RAISE(ABORT, 'a materialization reroute cannot be withdrawn'); END;

-- Give the reroute its own receipt authority rather than laundering it through
-- native predecessor recovery.
CREATE TABLE command_receipts_v68 AS SELECT * FROM command_receipts WHERE 0;
DROP TABLE command_receipts_v68;
CREATE TABLE command_receipts_v68 (
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
        'select_task_team','select_task_account','reconcile_ticket','materialize_jira',
        'activate_asma_epic','settle_runtime','submit_intake','pull_ticket_comments',
        'claim_ticket','replace_seat','refresh_capacity','override_availability','observe_seat',
        'retire_seat','publish_topology_spec','select_project_topology','upgrade_topology',
        'retitle_container','reconcile_native_names','apply_core_team','ensure_quick_session',
        'promote_quick_session','materialize_core_team','correct_core_team_route',
        'claim_core_team_seat','upgrade_epic_roster','apply_advisor_profile',
        'apply_committee_template','apply_completion_profile','advance_completion',
        'remediate_completion','invoke_advisor_run','settle_advisor_run','invoke_committee_run',
        'record_committee_findings','settle_committee_run','recover_consultation_seat',
        'reroute_unmaterialized_consultation_seat','publish_trigger','install_workflow_spec',
        'withdraw_task')),
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
INSERT INTO command_receipts_v68 SELECT * FROM command_receipts;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v68 RENAME TO command_receipts;
CREATE INDEX ix_command_receipts_state ON command_receipts(project_id, state);
CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key OR OLD.target <> NEW.target
  OR OLD.intent <> NEW.intent OR OLD.intent_hash <> NEW.intent_hash
  OR OLD.kind <> NEW.kind OR OLD.project_id <> NEW.project_id
  OR OLD.state IN ('confirmed','failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;
CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

PRAGMA user_version = 68;
