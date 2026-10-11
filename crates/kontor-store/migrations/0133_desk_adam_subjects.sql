-- Desk Adam subjects: which work a leadership session on a durable desk does.
--
-- A desk's workspace is shared. Every epic led from the desk, and every ad-hoc
-- topic opened there before an epic exists, is a subject: a topology node of
-- its own directly below the desk's workspace, with no native container. Its
-- seats live in the workspace's container, but they are bound to the subject,
-- so the epic a seat acts for is read from the subject node, never from the
-- shared workspace. Membership and work identity stay separate.
--
-- This row is written before the subject node and carries the id it will use,
-- as the desks row does, so a retry that lost its answer reconciles the same
-- node instead of opening a second subject. For the same reason the node
-- columns are plain TEXT without a foreign key to topology_nodes.
CREATE TABLE desk_adam_subjects (
    project_id               TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    topology_node_id         TEXT    NOT NULL CHECK (length(topology_node_id) = 36),
    desk_key                 TEXT    NOT NULL,
    subject_kind             TEXT    NOT NULL CHECK (subject_kind IN ('epic', 'ad_hoc')),
    -- The epic an epic subject leads; an ad-hoc subject has none.
    mini_project_id          TEXT    NULL,
    -- The short name or topic its seat titles render, frozen with the subject:
    -- a later rename of the epic never retitles the sessions that lead it.
    topic                    TEXT    NOT NULL
                                     CHECK (length(topic) BETWEEN 1 AND 512
                                            AND topic NOT GLOB '*•*'
                                            AND topic NOT GLOB '*·*'),
    -- The ad-hoc subject an epic subject was bound from, if any. The ad-hoc
    -- row and its seats' history are never rewritten by the binding.
    origin_topology_node_id  TEXT    NULL,
    -- The exact definition revision whose desk_leadership placed the subject
    -- and whose templates render its seat titles.
    team_definition_id       TEXT    NOT NULL,
    team_definition_version  INTEGER NOT NULL CHECK (team_definition_version > 0),
    team_definition_hash     TEXT    NOT NULL
                                     CHECK (length(team_definition_hash) = 64
                                            AND team_definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at               TEXT    NOT NULL
                                     CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, topology_node_id),
    CHECK ((subject_kind = 'epic') = (mini_project_id IS NOT NULL)),
    CHECK (origin_topology_node_id IS NULL OR subject_kind = 'epic'),
    CHECK (origin_topology_node_id IS NOT topology_node_id),
    FOREIGN KEY (project_id, desk_key)
        REFERENCES desks (project_id, desk_key) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, origin_topology_node_id)
        REFERENCES desk_adam_subjects (project_id, topology_node_id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_definition_id, team_definition_version)
        REFERENCES team_definitions (project_id, definition_id, version) ON DELETE RESTRICT
) STRICT;

-- One epic Adam per epic, across every desk of the project.
CREATE UNIQUE INDEX ux_desk_adam_subjects_epic
    ON desk_adam_subjects (project_id, mini_project_id)
    WHERE mini_project_id IS NOT NULL;

-- An ad-hoc subject binds to at most one epic, ever.
CREATE UNIQUE INDEX ux_desk_adam_subjects_origin
    ON desk_adam_subjects (project_id, origin_topology_node_id)
    WHERE origin_topology_node_id IS NOT NULL;

-- An ad-hoc topic names one subject per desk, so a retry finds the subject it
-- opened rather than a second tab with the same title.
CREATE UNIQUE INDEX ux_desk_adam_subjects_ad_hoc_topic
    ON desk_adam_subjects (project_id, desk_key, topic)
    WHERE subject_kind = 'ad_hoc';

-- A binding names an ad-hoc subject of the same desk. Binding an epic subject
-- to another would move one epic's leadership into another's history.
CREATE TRIGGER desk_adam_subjects_bind_ad_hoc_on_the_same_desk
BEFORE INSERT ON desk_adam_subjects
WHEN NEW.origin_topology_node_id IS NOT NULL
 AND NOT EXISTS (
        SELECT 1 FROM desk_adam_subjects AS origin
         WHERE origin.project_id = NEW.project_id
           AND origin.topology_node_id = NEW.origin_topology_node_id
           AND origin.subject_kind = 'ad_hoc'
           AND origin.desk_key = NEW.desk_key)
BEGIN
    SELECT RAISE(ABORT, 'a desk Adam subject binds only an ad-hoc subject of its own desk');
END;

-- An ad-hoc subject binds to an epic only once its own sessions are fenced:
-- every seat on it retired and no native occupancy left on any of them. The
-- epic's LSA is a fresh session, so the two never run at once.
CREATE TRIGGER desk_adam_subjects_bind_after_the_ad_hoc_fence
BEFORE INSERT ON desk_adam_subjects
WHEN NEW.origin_topology_node_id IS NOT NULL
 AND EXISTS (
        SELECT 1 FROM seat_bindings AS seat
         WHERE seat.project_id = NEW.project_id
           AND seat.topology_node_id = NEW.origin_topology_node_id
           AND (seat.lifecycle = 'active'
                OR EXISTS (SELECT 1 FROM hosted_topology_seats AS live
                            WHERE live.seat_binding_id = seat.id)))
BEGIN
    SELECT RAISE(ABORT, 'an ad-hoc subject binds to an epic only after its sessions are fenced');
END;

-- Once bound, an ad-hoc subject's work belongs to the epic, so the ad-hoc
-- subject never seats or hosts anyone again. A later migration that rebuilds
-- seat_bindings or hosted_topology_seats drops its trigger and must recreate it.
CREATE TRIGGER desk_adam_subjects_bound_ad_hoc_seats_no_one
BEFORE INSERT ON seat_bindings
WHEN EXISTS (
        SELECT 1 FROM desk_adam_subjects AS bound
         WHERE bound.project_id = NEW.project_id
           AND bound.origin_topology_node_id = NEW.topology_node_id)
BEGIN
    SELECT RAISE(ABORT, 'a bound ad-hoc subject seats no one again');
END;

CREATE TRIGGER desk_adam_subjects_bound_ad_hoc_hosts_no_one
BEFORE INSERT ON hosted_topology_seats
WHEN EXISTS (
        SELECT 1
          FROM seat_bindings AS seat
          JOIN desk_adam_subjects AS bound
            ON bound.project_id = seat.project_id
           AND bound.origin_topology_node_id = seat.topology_node_id
         WHERE seat.id = NEW.seat_binding_id)
BEGIN
    SELECT RAISE(ABORT, 'a bound ad-hoc subject seats no one again');
END;

CREATE TRIGGER desk_adam_subjects_are_immutable
BEFORE UPDATE ON desk_adam_subjects
BEGIN
    SELECT RAISE(ABORT, 'a desk Adam subject keeps its node, desk, scope, topic, origin and pin');
END;

CREATE TRIGGER desk_adam_subjects_are_permanent
BEFORE DELETE ON desk_adam_subjects
BEGIN
    SELECT RAISE(ABORT, 'a desk Adam subject is never deleted');
END;

-- A subject has one phase owner at a time: its LSA while planning, its TPM
-- while coordinating, never both. Sharing the desk's container confers no
-- shared authority, so a second live session on one subject is refused here
-- whatever path asks for it. A route change for the same seat replaces its row
-- and is unaffected; a handoff archives the predecessor's row first, and a
-- retired seat whose last occupancy was never archived is no longer live.
--
-- A later migration that rebuilds hosted_topology_seats drops this trigger
-- with the old table and must recreate it.
CREATE TRIGGER desk_adam_subjects_host_one_session
BEFORE INSERT ON hosted_topology_seats
WHEN EXISTS (
        SELECT 1
          FROM seat_bindings AS joining
          JOIN desk_adam_subjects AS subject
            ON subject.project_id = joining.project_id
           AND subject.topology_node_id = joining.topology_node_id
          JOIN seat_bindings AS other
            ON other.project_id = joining.project_id
           AND other.topology_node_id = joining.topology_node_id
           AND other.id <> joining.id
           AND other.lifecycle = 'active'
          JOIN hosted_topology_seats AS live
            ON live.seat_binding_id = other.id
         WHERE joining.id = NEW.seat_binding_id)
BEGIN
    SELECT RAISE(ABORT, 'a desk Adam subject hosts one live session at a time');
END;

-- The v128 receipt shape with four more kinds: opening an ad-hoc desk Adam
-- and handing a desk Adam subject from one phase owner to the next, and the
-- two durable-seat kinds reserved for the sibling desk-seat generation, so
-- either generation can land first without narrowing the other's receipts.
CREATE TABLE command_receipts_v133 (
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
        'open_ad_hoc_desk_adam','hand_off_desk_adam')),
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

INSERT INTO command_receipts_v133 SELECT * FROM command_receipts;
PRAGMA legacy_alter_table = ON;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v133 RENAME TO command_receipts;
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

PRAGMA user_version = 133;
