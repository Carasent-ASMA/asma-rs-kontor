-- Schema v126. A planning pair member's known native session, and the frozen
-- caller's same-native member recovery (ASMA-8282 frontier A, LSA decision of
-- 2026-10-02 under D-1 to D-3 / ADR-0008).
--
-- Forward-only and additive. Every existing table, row, constraint, trigger
-- and index is kept. The one new table is a run-keyed planning pair detail, not
-- an identity or run registry, and not a qualification: a member is qualified
-- only by its bound consultation seat. The receipt ledger is rebuilt exactly as
-- v125 left it, with one more closed kind.

-- 1. The known native claim: what one trusted runtime outcome proved about one
-- member's native session at one occupancy generation. Immutable and
-- permanent, so a replay, a restart or a recovery meets that same session and
-- never discovers or creates another.
CREATE TABLE planning_pair_member_natives (
    run_id               TEXT    NOT NULL,
    project_id           TEXT    NOT NULL,
    seat_binding_id      TEXT    NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    occupancy_generation INTEGER NOT NULL CHECK (occupancy_generation >= 1),
    runtime_kind         TEXT    NOT NULL CHECK (length(runtime_kind) BETWEEN 1 AND 256),
    host                 TEXT    NOT NULL CHECK (length(host) BETWEEN 1 AND 256),
    runtime_generation   INTEGER NOT NULL CHECK (runtime_generation >= 0),
    native_id            TEXT    NOT NULL CHECK (length(native_id) BETWEEN 1 AND 256),
    provider_session_id  TEXT    NULL
                                 CHECK (provider_session_id IS NULL
                                        OR length(provider_session_id) BETWEEN 1 AND 256),
    context_hash         TEXT    NOT NULL
                                 CHECK (length(context_hash) = 64
                                        AND context_hash NOT GLOB '*[^0-9a-f]*'),
    placement_hash       TEXT    NOT NULL
                                 CHECK (length(placement_hash) = 64
                                        AND placement_hash NOT GLOB '*[^0-9a-f]*'),
    readback_refusal     TEXT    NULL CHECK (readback_refusal IS NULL OR readback_refusal IN (
                                     'no_member_surface', 'correlation_unobserved',
                                     'route_unobserved', 'tool_restriction_unobserved',
                                     'provenance_unconfirmed')),
    observed_at          TEXT    NOT NULL
                                 CHECK (observed_at GLOB
                                        '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (run_id, seat_binding_id, occupancy_generation),
    FOREIGN KEY (run_id) REFERENCES planning_pair_placements(run_id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, run_id) REFERENCES consultation_runs(project_id, run_id)
        ON DELETE RESTRICT
) STRICT;

-- A claim belongs only to one of its own planning pair's member seats, at that
-- seat's current occupancy generation, on the run's own frozen placement, and
-- never after the pair is disposed.
CREATE TRIGGER planning_pair_member_natives_belong_to_a_current_member
BEFORE INSERT ON planning_pair_member_natives
WHEN NOT EXISTS (
    SELECT 1
      FROM consultation_seats AS seat
      JOIN consultation_runs AS run
        ON run.project_id = seat.project_id AND run.run_id = seat.run_id
      JOIN planning_pair_placements AS placement
        ON placement.run_id = run.run_id
     WHERE seat.run_id = NEW.run_id
       AND seat.project_id = NEW.project_id
       AND seat.seat_binding_id = NEW.seat_binding_id
       AND seat.occupancy_generation = NEW.occupancy_generation
       AND run.family = 'planning_pair'
       AND run.state <> 'disposed'
       AND placement.placement_hash = NEW.placement_hash
)
BEGIN
    SELECT RAISE(ABORT,
        'a known planning pair native belongs only to a current member seat of an open pair');
END;

CREATE TRIGGER planning_pair_member_natives_are_immutable
BEFORE UPDATE ON planning_pair_member_natives
BEGIN
    SELECT RAISE(ABORT, 'a known planning pair member native is immutable');
END;

CREATE TRIGGER planning_pair_member_natives_are_permanent
BEFORE DELETE ON planning_pair_member_natives
BEGIN
    SELECT RAISE(ABORT, 'a known planning pair member native cannot be withdrawn');
END;

-- 2. The v125 receipt shape with exactly one additional kind.
CREATE TABLE command_receipts_v126 (
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
        'record_planning_pair_disposition','recover_planning_pair_seat')),
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

INSERT INTO command_receipts_v126 SELECT * FROM command_receipts;
PRAGMA legacy_alter_table = ON;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v126 RENAME TO command_receipts;
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

PRAGMA user_version = 126;
