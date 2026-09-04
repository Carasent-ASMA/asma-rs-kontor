-- Schema v84. Two deliberately narrow recovery authorities unblock legacy
-- epics without weakening the ordinary immutable identity contracts.

-- Widen the closed command-kind list. The table shape is the v77 shape plus
-- the two recovery commands introduced by this generation.
CREATE TABLE command_receipts_v84 (
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
        'withdraw_task','publish_team_definition','select_project_team_definition',
        'upgrade_team_definition','correct_epic_backlog_code','recover_topology_container')),
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
INSERT INTO command_receipts_v84 SELECT * FROM command_receipts;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v84 RENAME TO command_receipts;
CREATE INDEX ix_command_receipts_state ON command_receipts(project_id, state);
CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key OR OLD.target <> NEW.target
  OR OLD.intent <> NEW.intent OR OLD.intent_hash <> NEW.intent_hash
  OR OLD.kind <> NEW.kind OR OLD.project_id <> NEW.project_id
  OR OLD.state IN ('confirmed','failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;
CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

-- The original row remains immutable and reserved. This append-only row is the
-- only effective-code override, and only application preflight may create it.
CREATE TABLE epic_backlog_code_corrections (
    project_id      TEXT NOT NULL,
    mini_project_id TEXT NOT NULL,
    prior_code      TEXT NOT NULL,
    corrected_code  TEXT NOT NULL CHECK (
                        length(corrected_code) BETWEEN 2 AND 32
                        AND corrected_code NOT GLOB '*[^A-Z0-9]*'
                        AND corrected_code GLOB '*[A-Z]*'),
    reason          TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 2048),
    receipt_id      TEXT NOT NULL,
    corrected_at    TEXT NOT NULL,
    PRIMARY KEY (project_id, mini_project_id),
    UNIQUE (project_id, corrected_code COLLATE NOCASE),
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts(project_id, id) ON DELETE RESTRICT,
    CHECK (prior_code <> corrected_code)
) STRICT;
CREATE TRIGGER epic_backlog_code_corrections_are_immutable
BEFORE UPDATE ON epic_backlog_code_corrections
BEGIN SELECT RAISE(ABORT, 'epic backlog code corrections are immutable evidence'); END;
CREATE TRIGGER epic_backlog_code_corrections_are_permanent
BEFORE DELETE ON epic_backlog_code_corrections
BEGIN SELECT RAISE(ABORT, 'epic backlog code corrections are permanent evidence'); END;

-- One row per recovery command preserves the identity that disappeared and
-- the exact replacement proved by the runtime census. The live binding table
-- still carries exactly one current row per topology node.
CREATE TABLE topology_container_recoveries (
    receipt_id           TEXT NOT NULL PRIMARY KEY,
    project_id           TEXT NOT NULL,
    topology_node_id     TEXT NOT NULL,
    container_binding_id TEXT NOT NULL,
    prior_runtime_kind   TEXT NOT NULL,
    prior_host           TEXT NOT NULL,
    prior_generation     INTEGER NOT NULL CHECK (prior_generation >= 0),
    prior_native_id      TEXT NOT NULL,
    next_runtime_kind    TEXT NOT NULL,
    next_host            TEXT NOT NULL,
    next_generation      INTEGER NOT NULL CHECK (next_generation >= 0),
    next_native_id       TEXT NOT NULL,
    parent_native_id     TEXT NOT NULL,
    observed_kind        TEXT NOT NULL CHECK (observed_kind IN ('project','workspace')),
    canonical_cwd        TEXT NULL,
    observed_title       TEXT NOT NULL,
    recovered_at         TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE RESTRICT,
    FOREIGN KEY (topology_node_id) REFERENCES topology_nodes(id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts(project_id, id) ON DELETE RESTRICT,
    UNIQUE (project_id, topology_node_id, next_runtime_kind, next_host,
            next_generation, next_native_id),
    CHECK (prior_runtime_kind <> next_runtime_kind
        OR prior_host <> next_host
        OR prior_generation <> next_generation
        OR prior_native_id <> next_native_id)
) STRICT;
CREATE TRIGGER topology_container_recoveries_are_immutable
BEFORE UPDATE ON topology_container_recoveries
BEGIN SELECT RAISE(ABORT, 'topology container recoveries are immutable evidence'); END;
CREATE TRIGGER topology_container_recoveries_are_permanent
BEFORE DELETE ON topology_container_recoveries
BEGIN SELECT RAISE(ABORT, 'topology container recoveries are permanent evidence'); END;

PRAGMA user_version = 84;
