-- Durable Jira materialization and confirmed bindings. Planned item identities
-- exist before transport, and activation is a separate confirmed local fact.
CREATE TABLE command_receipts_v57 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    idempotency_key TEXT NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    kind TEXT NOT NULL CHECK (kind IN (
        'launch_run', 'cancel_run', 'park_run', 'abandon_run', 'resume_task',
        'record_gate_verdict', 'approve_intake', 'sync_ticket', 'assign_ticket',
        'transition_ticket', 'authorize_execution', 'approve_schedule_override',
        'revoke_schedule_override', 'resolve_status_conflict', 'assign_work_calendar',
        'revoke_execution_authorization', 'ensure_project', 'ensure_account_profile',
        'apply_epic_graph', 'transition_epic', 'start_scheduled_work', 'transition_task',
        'resolve_context', 'select_task_profile', 'select_task_team', 'select_task_account',
        'reconcile_ticket', 'materialize_jira', 'activate_asma_epic', 'settle_runtime',
        'submit_intake', 'pull_ticket_comments', 'claim_ticket', 'replace_seat',
        'refresh_capacity', 'override_availability', 'observe_seat', 'retire_seat',
        'publish_topology_spec', 'select_project_topology', 'upgrade_topology',
        'retitle_container', 'reconcile_native_names', 'apply_core_team',
        'ensure_quick_session', 'promote_quick_session', 'materialize_core_team',
        'correct_core_team_route', 'upgrade_epic_roster', 'apply_advisor_profile',
        'apply_committee_template', 'apply_completion_profile', 'advance_completion',
        'remediate_completion', 'invoke_advisor_run', 'settle_advisor_run',
        'invoke_committee_run', 'record_committee_findings', 'settle_committee_run',
        'publish_trigger', 'install_workflow_spec', 'withdraw_task')),
    target TEXT NOT NULL CHECK (json_valid(target)),
    target_revision INTEGER NOT NULL CHECK (target_revision >= 1),
    intent TEXT NOT NULL CHECK (json_valid(intent)),
    intent_hash TEXT NOT NULL CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    state TEXT NOT NULL CHECK (state IN (
        'intent_persisted', 'dispatch_pending', 'dispatched', 'acknowledged',
        'confirmation_unknown', 'confirmed', 'failed')),
    correlation TEXT NULL CHECK (correlation IS NULL OR length(correlation) BETWEEN 1 AND 256),
    native_identity TEXT NULL CHECK (native_identity IS NULL OR json_valid(native_identity)),
    result_ref TEXT NULL CHECK (result_ref IS NULL OR length(result_ref) BETWEEN 1 AND 256),
    attempts INTEGER NOT NULL CHECK (attempts >= 0),
    created_at TEXT NOT NULL CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    updated_at TEXT NOT NULL CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    execution_mode TEXT NOT NULL DEFAULT 'dispatch' CHECK (execution_mode IN ('local', 'dispatch')),
    UNIQUE (project_id, id)
) STRICT;

INSERT INTO command_receipts_v57
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at, execution_mode
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v57 RENAME TO command_receipts;
CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key
     OR OLD.target <> NEW.target
     OR OLD.intent <> NEW.intent
     OR OLD.intent_hash <> NEW.intent_hash
     OR OLD.kind <> NEW.kind
     OR OLD.project_id <> NEW.project_id
     OR OLD.state IN ('confirmed', 'failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;

CREATE TABLE jira_materialization_batches (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    project_id TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    epic_id TEXT NOT NULL REFERENCES mini_projects (id) ON DELETE RESTRICT,
    idempotency_key TEXT NOT NULL UNIQUE,
    preview_hash TEXT NOT NULL CHECK (length(preview_hash) = 64),
    expected_revision INTEGER NOT NULL CHECK (expected_revision >= 1),
    status TEXT NOT NULL CHECK (status IN ('planned', 'confirmed', 'conflict')),
    created_at TEXT NOT NULL,
    confirmed_at TEXT NULL,
    UNIQUE (project_id, id),
    UNIQUE (project_id, epic_id, preview_hash)
) STRICT;

CREATE TABLE jira_materialization_items (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36),
    batch_id TEXT NOT NULL REFERENCES jira_materialization_batches (id) ON DELETE RESTRICT,
    project_id TEXT NOT NULL,
    epic_id TEXT NOT NULL,
    task_id TEXT NULL REFERENCES tasks (id) ON DELETE RESTRICT,
    link_id TEXT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    item_kind TEXT NOT NULL CHECK (item_kind IN ('epic', 'task')),
    intent_kind TEXT NOT NULL CHECK (intent_kind IN ('create', 'link')),
    requested_key TEXT NULL,
    marker TEXT NOT NULL UNIQUE CHECK (length(marker) BETWEEN 1 AND 255),
    status TEXT NOT NULL CHECK (status IN ('planned', 'confirmed', 'conflict')),
    confirmed_key TEXT NULL,
    readback_hash TEXT NULL CHECK (readback_hash IS NULL OR length(readback_hash) = 64),
    confirmed_at TEXT NULL,
    UNIQUE (batch_id, ordinal),
    UNIQUE (project_id, epic_id, task_id),
    CHECK ((item_kind = 'epic' AND task_id IS NULL AND link_id IS NULL)
        OR (item_kind = 'task' AND task_id IS NOT NULL AND link_id IS NOT NULL)),
    CHECK ((intent_kind = 'create' AND requested_key IS NULL) OR (intent_kind = 'link' AND requested_key IS NOT NULL)),
    CHECK ((status = 'confirmed' AND confirmed_key IS NOT NULL AND readback_hash IS NOT NULL AND confirmed_at IS NOT NULL)
        OR (status <> 'confirmed' AND confirmed_key IS NULL AND readback_hash IS NULL AND confirmed_at IS NULL)),
    FOREIGN KEY (project_id, epic_id) REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE jira_epic_bindings (
    project_id TEXT NOT NULL,
    epic_id TEXT NOT NULL,
    external_issue_key TEXT NOT NULL,
    readback_hash TEXT NOT NULL CHECK (length(readback_hash) = 64),
    confirmed_at TEXT NOT NULL,
    PRIMARY KEY (project_id, epic_id),
    UNIQUE (project_id, external_issue_key),
    FOREIGN KEY (project_id, epic_id) REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE jira_task_binding_confirmations (
    project_id TEXT NOT NULL,
    link_id TEXT NOT NULL,
    readback_hash TEXT NOT NULL CHECK (length(readback_hash) = 64),
    confirmed_at TEXT NOT NULL,
    PRIMARY KEY (project_id, link_id),
    FOREIGN KEY (project_id, link_id) REFERENCES jira_links (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE asma_epic_activations (
    project_id TEXT NOT NULL,
    epic_id TEXT NOT NULL,
    receipt_id TEXT NOT NULL REFERENCES command_receipts (id) ON DELETE RESTRICT,
    activated_at TEXT NOT NULL,
    PRIMARY KEY (project_id, epic_id),
    UNIQUE (receipt_id),
    FOREIGN KEY (project_id, epic_id) REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT
) STRICT;

PRAGMA user_version = 57;
