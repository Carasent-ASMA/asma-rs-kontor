-- PR review subjects: which pull request one Adam tab and one review room on a
-- PR desk serve (ASMA-8453).
--
-- A pull request is identified by its repository and its number, and by
-- nothing else. Two pull requests of one Jira ticket are two rows, and the
-- same number in two repositories is two rows; the confirmed Jira key a review
-- was opened under is a fact about the pull request, never a second identity.
-- Each row names the two nodes its placement uses: the Adam subject directly
-- below the desk's workspace, which owns no native container, and the review
-- room directly below the desk, a native workspace of the desk's project.
--
-- The row is written before either node and carries their ids, as the desks
-- and desk_adam_subjects rows do, so a retry that lost its answer reconciles
-- the same nodes instead of placing a second Adam or room. For the same reason
-- the node columns are plain TEXT without a foreign key to topology_nodes.
--
-- Closing a review releases its seats and never this row: reopening the same
-- pull request reuses its nodes, and its history stays readable.
CREATE TABLE pr_review_subjects (
    project_id               TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    desk_key                 TEXT    NOT NULL,
    -- The forge repository as owner/name, exactly as authorized.
    repository               TEXT    NOT NULL
                                     CHECK (length(repository) BETWEEN 3 AND 140
                                            AND repository GLOB '?*/?*'
                                            AND repository NOT GLOB '*/*/*'),
    pull_request_number      INTEGER NOT NULL CHECK (pull_request_number >= 1),
    -- The exact confirmed Jira key, and the epic (and task) it is bound to.
    jira_key                 TEXT    NOT NULL
                                     CHECK (length(jira_key) BETWEEN 3 AND 256
                                            AND jira_key GLOB '[A-Z]*-[1-9]*'),
    mini_project_id          TEXT    NOT NULL,
    task_id                  TEXT    NULL,
    adam_node_id             TEXT    NOT NULL CHECK (length(adam_node_id) = 36),
    room_node_id             TEXT    NOT NULL CHECK (length(room_node_id) = 36),
    -- The exact definition revision whose pr_review placed both nodes and
    -- whose templates render their titles.
    team_definition_id       TEXT    NOT NULL,
    team_definition_version  INTEGER NOT NULL CHECK (team_definition_version > 0),
    team_definition_hash     TEXT    NOT NULL
                                     CHECK (length(team_definition_hash) = 64
                                            AND team_definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at               TEXT    NOT NULL
                                     CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, desk_key, repository, pull_request_number),
    CHECK (adam_node_id <> room_node_id),
    FOREIGN KEY (project_id, desk_key)
        REFERENCES desks (project_id, desk_key) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_definition_id, team_definition_version)
        REFERENCES team_definitions (project_id, definition_id, version) ON DELETE RESTRICT
) STRICT;

-- Each node serves one pull request.
CREATE UNIQUE INDEX ux_pr_review_subjects_adam
    ON pr_review_subjects (project_id, adam_node_id);
CREATE UNIQUE INDEX ux_pr_review_subjects_room
    ON pr_review_subjects (project_id, room_node_id);

-- No node serves two pull requests, in either role: an Adam id is never
-- another row's room, and the reverse. The two unique indexes cover each
-- column alone.
CREATE TRIGGER pr_review_subjects_nodes_serve_one
BEFORE INSERT ON pr_review_subjects
WHEN EXISTS (
        SELECT 1 FROM pr_review_subjects AS other
         WHERE other.project_id = NEW.project_id
           AND (other.adam_node_id IN (NEW.adam_node_id, NEW.room_node_id)
                OR other.room_node_id IN (NEW.adam_node_id, NEW.room_node_id)))
BEGIN
    SELECT RAISE(ABORT, 'a PR review node serves one pull request');
END;

-- A task key names a task of the epic the row records.
CREATE TRIGGER pr_review_subjects_task_of_its_epic
BEFORE INSERT ON pr_review_subjects
WHEN NEW.task_id IS NOT NULL
 AND NOT EXISTS (
        SELECT 1 FROM tasks AS task
         WHERE task.project_id = NEW.project_id
           AND task.id = NEW.task_id
           AND task.mini_project_id = NEW.mini_project_id)
BEGIN
    SELECT RAISE(ABORT, 'a PR review task key names a task of its own epic');
END;

CREATE TRIGGER pr_review_subjects_are_immutable
BEFORE UPDATE ON pr_review_subjects
BEGIN
    SELECT RAISE(ABORT, 'a PR review subject keeps its pull request, key, nodes and pin');
END;

CREATE TRIGGER pr_review_subjects_are_permanent
BEFORE DELETE ON pr_review_subjects
BEGIN
    SELECT RAISE(ABORT, 'a PR review subject is never deleted');
END;

-- The v133 receipt shape, desk Adam kinds included, with two more kinds:
-- opening and closing one pull request's review.
CREATE TABLE command_receipts_v134 (
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
        'declare_durable_seat','reconcile_durable_seats',
        'open_ad_hoc_desk_adam','hand_off_desk_adam',
        'open_pr_review','close_pr_review')),
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

INSERT INTO command_receipts_v134 SELECT * FROM command_receipts;
PRAGMA legacy_alter_table = ON;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v134 RENAME TO command_receipts;
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

PRAGMA user_version = 134;
