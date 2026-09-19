-- Schema v108. Exact, task-scoped repair of a stale worktree claim.
--
-- A legacy epic graph may hold a path that the supported ASMA worktree
-- materializer can never create. Re-applying the whole graph is too broad: it
-- can move unrelated task facts and cannot prove which old placement the
-- operator inspected. This generation adds one distinct local command and an
-- append-only before/after ledger. The task row is not updated, so lifecycle,
-- revision, Jira identity, workflow, gates and dependencies stay untouched.

-- SQLite cannot widen a CHECK in place. This is the v107 receipt shape with
-- exactly one additional kind: `correct_task_worktree`.
CREATE TABLE command_receipts_v108 (
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
        'attest_retired_evaluator_evidence')),
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

-- See v107: legacy rename semantics avoid reparsing a v92 trigger while the
-- referenced receipt table is momentarily between names.
INSERT INTO command_receipts_v108 SELECT * FROM command_receipts;
PRAGMA legacy_alter_table = ON;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v108 RENAME TO command_receipts;
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

CREATE TABLE task_worktree_corrections (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    receipt_id TEXT NOT NULL REFERENCES command_receipts(id) ON DELETE RESTRICT,
    task_id TEXT NOT NULL,
    task_revision INTEGER NOT NULL CHECK (task_revision >= 1),
    old_worktree TEXT NOT NULL CHECK (length(old_worktree) BETWEEN 1 AND 512 AND old_worktree GLOB '/*'),
    new_worktree TEXT NOT NULL CHECK (length(new_worktree) BETWEEN 1 AND 512 AND new_worktree GLOB '/*'),
    module_key TEXT NOT NULL CHECK (length(module_key) BETWEEN 1 AND 128),
    branch_name TEXT NOT NULL CHECK (length(branch_name) BETWEEN 1 AND 512),
    preview_hash TEXT NOT NULL CHECK (
        length(preview_hash) = 64 AND preview_hash NOT GLOB '*[^0-9a-f]*'
    ),
    corrected_at TEXT NOT NULL CHECK (
        corrected_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'
    ),
    PRIMARY KEY (project_id, receipt_id),
    UNIQUE (receipt_id),
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks(project_id, id) ON DELETE RESTRICT,
    CHECK (old_worktree <> new_worktree)
) STRICT;

CREATE INDEX ix_task_worktree_corrections_task
    ON task_worktree_corrections(project_id, task_id, corrected_at);
CREATE TRIGGER task_worktree_corrections_immutable
BEFORE UPDATE ON task_worktree_corrections
BEGIN SELECT RAISE(ABORT, 'task worktree corrections are immutable'); END;
CREATE TRIGGER task_worktree_corrections_no_delete
BEFORE DELETE ON task_worktree_corrections
BEGIN SELECT RAISE(ABORT, 'task worktree corrections are not deletable'); END;

PRAGMA user_version = 108;
