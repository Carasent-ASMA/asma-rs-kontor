-- Schema v46. Durable task short codes and auditable hosted-seat route history.
--
-- A task title is a backlog description, not a runtime display identity.  Keep
-- the compact code separately so imports, container retitles and seat labels all
-- consume the same operator-declared fact.  The only automatic backfill is the
-- already-explicit KON-OP spelling used by this Realm's original backlog; no
-- description, Jira key, UUID or worktree slug is treated as a code.
CREATE TABLE task_short_codes (
    project_id   TEXT NOT NULL,
    task_id      TEXT NOT NULL,
    short_code   TEXT NOT NULL CHECK (
                     length(short_code) BETWEEN 1 AND 32
                     AND short_code NOT GLOB '*[^A-Za-z0-9._-]*'),
    source       TEXT NOT NULL CHECK (source IN ('import', 'legacy_kon_op')),
    declared_at  TEXT NOT NULL
                     CHECK (declared_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, task_id),
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT
) STRICT;

INSERT INTO task_short_codes (project_id, task_id, short_code, source, declared_at)
SELECT project_id,
       id,
       'OP-' || substr(title, 8, instr(title, ':') - 8),
       'legacy_kon_op',
       updated_at
FROM tasks
WHERE title LIKE 'KON-OP-%:%'
  AND instr(title, ':') > 8
  AND substr(title, 8, instr(title, ':') - 8) <> ''
  AND substr(title, 8, instr(title, ':') - 8) NOT GLOB '*[^0-9]*';

-- Native route replacement keeps the logical SeatBinding.  When a later
-- command changes the provider/model filling that seat, the predecessor row is
-- moved here before the active row is replaced.  This table is evidence only;
-- it grants no launch or retirement authority by itself.
CREATE TABLE hosted_topology_seat_history (
    seat_binding_id     TEXT NOT NULL
                             REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    project_id          TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    generation          INTEGER NOT NULL CHECK (generation >= 0),
    model_rung          TEXT NOT NULL CHECK (json_valid(model_rung)),
    runtime_kind        TEXT NOT NULL,
    host                TEXT NOT NULL,
    native_id           TEXT NOT NULL,
    provider_session_id TEXT NULL,
    observed_at         TEXT NOT NULL,
    retired_at          TEXT NOT NULL,
    retirement_reason   TEXT NOT NULL CHECK (length(retirement_reason) BETWEEN 1 AND 512),
    PRIMARY KEY (project_id, seat_binding_id, native_id),
    UNIQUE (runtime_kind, host, generation, native_id)
) STRICT;

-- Route correction is authority distinct from initial Core Team
-- materialization. Widen the receipt vocabulary without rewriting any row.
CREATE TABLE command_receipts_v46 (
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
                                 'publish_topology_spec', 'upgrade_topology',
                                 'retitle_container',
                                 'apply_core_team', 'ensure_quick_session',
                                 'promote_quick_session', 'materialize_core_team',
                                 'correct_core_team_route',
                                 'upgrade_epic_roster', 'apply_advisor_profile',
                                 'apply_committee_template', 'apply_completion_profile',
                                 'advance_completion', 'remediate_completion',
                                 'invoke_advisor_run', 'settle_advisor_run',
                                 'invoke_committee_run', 'record_committee_findings',
                                 'settle_committee_run', 'publish_trigger',
                                 'install_workflow_spec', 'withdraw_task')),
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

INSERT INTO command_receipts_v46
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v46 RENAME TO command_receipts;
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

CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

PRAGMA user_version = 46;
