-- ===========================================================================
-- Schema v90. A rejected gate verdict and the workflow route it caused become
-- one durable, append-only fact, and that fact is also the fence that stops
-- pre-rejection evidence from immediately undoing the route.
--
-- Before this generation the route was a bare `current_phase` update inside the
-- verdict transaction. That is enough to move the workflow and not enough to
-- prove it: nothing recorded which receipt caused the move, nothing stopped a
-- second command from routing the same rejection again, and nothing
-- distinguished the artifacts that existed *before* the rejection from the work
-- the rejection asked for, nor which TeamRun was the one being asked. A row per
-- rejected evaluation answers all four, and
-- answers them the same way whether the route was written when the verdict was
-- recorded or recovered afterwards for a verdict recorded before the fix.
-- ===========================================================================

-- Widen the closed command-kind list. The table shape is the v84 shape plus the
-- one recovery command introduced by this generation. It is deliberately not
-- `record_gate_verdict`: recovering a route must never be replayable as the
-- authority that recorded the verdict it routes.
CREATE TABLE command_receipts_v90 (
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
        'upgrade_team_definition','correct_epic_backlog_code','recover_topology_container',
        'recover_gate_rejection')),
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
INSERT INTO command_receipts_v90 SELECT * FROM command_receipts;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v90 RENAME TO command_receipts;
CREATE INDEX ix_command_receipts_state ON command_receipts(project_id, state);
CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key OR OLD.target <> NEW.target
  OR OLD.intent <> NEW.intent OR OLD.intent_hash <> NEW.intent_hash
  OR OLD.kind <> NEW.kind OR OLD.project_id <> NEW.project_id
  OR OLD.state IN ('confirmed','failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;
CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

-- One row per rejected gate evaluation that routed its workflow.
--
-- The primary key is the evaluation itself, so a second route for the same
-- `(workflow, gate, sequence)` cannot be written at all. `rejection_receipt_id`
-- is separately unique, so the *source* verdict is consumed exactly once even
-- if a caller reaches it by another identity. `route_receipt_id` is unique for
-- the mirror reason: one command records one route.
--
-- On the ordinary path both receipt columns hold the same `record_gate_verdict`
-- receipt — the command that recorded the verdict is the command that routed
-- it. On the recovery path `rejection_receipt_id` stays the original verdict
-- receipt and `route_receipt_id` is the new `recover_gate_rejection` receipt,
-- which is what makes the two origins distinguishable forever without either
-- rewriting the other's history.
CREATE TABLE task_gate_rejection_routes (
    project_id           TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id              TEXT    NOT NULL,
    workflow_id          TEXT    NOT NULL,
    gate_key             TEXT    NOT NULL CHECK (length(gate_key) BETWEEN 1 AND 128),
    gate_sequence        INTEGER NOT NULL CHECK (gate_sequence >= 1),
    rejection_receipt_id TEXT    NOT NULL UNIQUE
                                 CHECK (length(rejection_receipt_id) = 36
                                        AND rejection_receipt_id NOT GLOB '*[^0-9a-f-]*'),
    route_receipt_id     TEXT    NOT NULL UNIQUE
                                 CHECK (length(route_receipt_id) = 36
                                        AND route_receipt_id NOT GLOB '*[^0-9a-f-]*'),
    route_origin         TEXT    NOT NULL CHECK (route_origin IN ('recorded', 'recovered')),
    -- The TeamRun this task had when the route was written.
    --
    -- A snapshot, never a lookup hint. The fence asks whether the rework was
    -- authored by *this* run; recomputing a "current" run at evaluation time
    -- would let any TeamRun created later for the same task release a route it
    -- had nothing to do with. Lifecycle is deliberately not constrained: a team
    -- whose seats have all settled closes as `succeeded` while its seats stay
    -- reusable, which is exactly the state a recovered rejection is found in.
    team_run_id          TEXT    NOT NULL
                                 CHECK (length(team_run_id) = 36
                                        AND team_run_id NOT GLOB '*[^0-9a-f-]*'),
    from_phase           TEXT    NOT NULL CHECK (length(from_phase) BETWEEN 1 AND 128),
    rejection_target     TEXT    NOT NULL CHECK (length(rejection_target) BETWEEN 1 AND 128),
    from_revision        INTEGER NOT NULL CHECK (from_revision >= 1),
    to_revision          INTEGER NOT NULL CHECK (to_revision > from_revision),
    routed_at            TEXT    NOT NULL
                                 CHECK (routed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- A recovery proves the gate's phase *is* the current phase before it
    -- writes, and the target is a strict ancestor of that phase, so a recovered
    -- route that lands where it started is impossible by construction.
    --
    -- The recorded path is deliberately not held to it. A rejection may be
    -- recorded while the stored phase already sits at the target -- the ordinary
    -- verdict path does not require the gate's phase to be current for a
    -- rejection -- and refusing that here would turn an existing, permitted
    -- recording into a hard failure. Such a route is recorded honestly instead.
    CHECK (route_origin = 'recorded' OR from_phase <> rejection_target),
    -- On the ordinary path one receipt does both; on the recovery path they are
    -- necessarily different commands. Nothing else is a legal combination.
    CHECK ((route_origin = 'recorded' AND route_receipt_id = rejection_receipt_id)
        OR (route_origin = 'recovered' AND route_receipt_id <> rejection_receipt_id)),
    PRIMARY KEY (project_id, workflow_id, gate_key, gate_sequence),
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, workflow_id)
        REFERENCES task_workflows (project_id, id) ON DELETE RESTRICT,
    -- The route may only name an evaluation that durably exists. This is what
    -- stops a recovery from inventing a sequence the gate never had.
    FOREIGN KEY (project_id, workflow_id, gate_key, gate_sequence)
        REFERENCES task_gate_evaluations (project_id, workflow_id, gate_key, sequence)
        ON DELETE RESTRICT,
    -- The route may only name a TeamRun that durably exists for this project.
    FOREIGN KEY (project_id, team_run_id)
        REFERENCES team_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (rejection_receipt_id) REFERENCES command_receipts (id) ON DELETE RESTRICT,
    FOREIGN KEY (route_receipt_id) REFERENCES command_receipts (id) ON DELETE RESTRICT
) STRICT;

-- The fence is read by active workflow and current phase.
CREATE INDEX ix_gate_rejection_routes_workflow
    ON task_gate_rejection_routes (project_id, workflow_id, rejection_target);

-- A route is a fact about something that already happened. Editing one would
-- change what a later advancement was allowed to conclude, after it concluded
-- it; deleting one would silently release the fence it stands for.
CREATE TRIGGER task_gate_rejection_routes_immutable
BEFORE UPDATE ON task_gate_rejection_routes
BEGIN SELECT RAISE(ABORT, 'a gate rejection route is immutable'); END;

CREATE TRIGGER task_gate_rejection_routes_no_delete
BEFORE DELETE ON task_gate_rejection_routes
BEGIN SELECT RAISE(ABORT, 'gate rejection routes are not deletable'); END;

PRAGMA user_version = 90;
