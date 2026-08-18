-- Durable Advisor and Committee invocations.
--
-- A run freezes every semantic input before a runtime effect: exact policy
-- revision, question, context digest, caller seat and dedicated topology node.
-- Native session identities are readback evidence on the declared seats; they
-- are never accepted as invocation input.
CREATE TABLE consultation_runs (
    run_id                  TEXT    NOT NULL PRIMARY KEY
                                    CHECK (length(run_id) = 36
                                           AND run_id NOT GLOB '*[^0-9a-f-]*'),
    project_id              TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id         TEXT    NOT NULL REFERENCES mini_projects(id) ON DELETE RESTRICT,
    family                  TEXT    NOT NULL CHECK (family IN ('advisor', 'committee')),
    profile_id              TEXT    NOT NULL,
    profile_version         INTEGER NOT NULL CHECK (profile_version >= 1),
    definition_hash         TEXT    NOT NULL
                                    CHECK (length(definition_hash) = 64
                                           AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    question                TEXT    NOT NULL CHECK (length(question) BETWEEN 1 AND 32768),
    question_hash           TEXT    NOT NULL
                                    CHECK (length(question_hash) = 64
                                           AND question_hash NOT GLOB '*[^0-9a-f]*'),
    context                 TEXT    NOT NULL CHECK (json_valid(context)),
    context_hash            TEXT    NOT NULL
                                    CHECK (length(context_hash) = 64
                                           AND context_hash NOT GLOB '*[^0-9a-f]*'),
    caller_seat_binding_id  TEXT    NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    topology_node_id        TEXT    NOT NULL UNIQUE REFERENCES topology_nodes(id) ON DELETE RESTRICT,
    invoke_key              TEXT    NOT NULL UNIQUE CHECK (length(invoke_key) BETWEEN 1 AND 256),
    invoke_intent_hash      TEXT    NOT NULL
                                    CHECK (length(invoke_intent_hash) = 64
                                           AND invoke_intent_hash NOT GLOB '*[^0-9a-f]*'),
    state                   TEXT    NOT NULL CHECK (state IN (
                                    'materializing', 'running', 'awaiting_judge',
                                    'settled', 'needs_human')),
    round                   INTEGER NOT NULL CHECK (round BETWEEN 1 AND 2),
    result                  TEXT    NULL CHECK (result IS NULL OR json_valid(result)),
    result_hash             TEXT    NULL
                                    CHECK (result_hash IS NULL OR
                                           (length(result_hash) = 64
                                            AND result_hash NOT GLOB '*[^0-9a-f]*')),
    revision                INTEGER NOT NULL CHECK (revision >= 1),
    created_at              TEXT    NOT NULL,
    updated_at              TEXT    NOT NULL,
    settled_at              TEXT    NULL,
    FOREIGN KEY (project_id, family, profile_id, profile_version)
        REFERENCES consultation_profile_revisions(project_id, family, profile_id, version)
        ON DELETE RESTRICT,
    CHECK ((result IS NULL) = (result_hash IS NULL)),
    CHECK ((state = 'settled') = (settled_at IS NOT NULL)),
    UNIQUE (project_id, run_id)
) STRICT;

CREATE INDEX ix_consultation_runs_epic
    ON consultation_runs(project_id, mini_project_id, family, created_at, run_id);

-- Semantic inputs never move after invocation. Lifecycle/result/revision are
-- the only mutable facts, and result can only move from absent to present.
CREATE TRIGGER consultation_run_inputs_are_frozen
BEFORE UPDATE ON consultation_runs
WHEN OLD.project_id <> NEW.project_id
  OR OLD.mini_project_id <> NEW.mini_project_id
  OR OLD.family <> NEW.family
  OR OLD.profile_id <> NEW.profile_id
  OR OLD.profile_version <> NEW.profile_version
  OR OLD.definition_hash <> NEW.definition_hash
  OR OLD.question <> NEW.question
  OR OLD.question_hash <> NEW.question_hash
  OR OLD.context <> NEW.context
  OR OLD.context_hash <> NEW.context_hash
  OR OLD.caller_seat_binding_id <> NEW.caller_seat_binding_id
  OR OLD.topology_node_id <> NEW.topology_node_id
  OR OLD.invoke_key <> NEW.invoke_key
  OR OLD.invoke_intent_hash <> NEW.invoke_intent_hash
  OR OLD.created_at <> NEW.created_at
  OR OLD.result IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a consultation run cannot rewrite frozen input or settled evidence');
END;

CREATE TABLE consultation_seats (
    run_id                  TEXT    NOT NULL REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    project_id              TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    role_slot_id            TEXT    NOT NULL,
    committee_role          TEXT    NULL CHECK (committee_role IS NULL OR
                                                committee_role IN ('reviewer', 'judge')),
    logical_role            TEXT    NOT NULL,
    seat_binding_id         TEXT    NOT NULL UNIQUE REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    model_rung              TEXT    NOT NULL CHECK (json_valid(model_rung)),
    runtime_kind            TEXT    NULL,
    host                    TEXT    NULL,
    generation              INTEGER NULL CHECK (generation IS NULL OR generation >= 0),
    native_id               TEXT    NULL,
    provider_session_id     TEXT    NULL,
    observed_at             TEXT    NULL,
    PRIMARY KEY (run_id, role_slot_id),
    FOREIGN KEY (project_id, run_id) REFERENCES consultation_runs(project_id, run_id)
        ON DELETE RESTRICT,
    CHECK ((runtime_kind IS NULL) = (host IS NULL)),
    CHECK ((runtime_kind IS NULL) = (generation IS NULL)),
    CHECK ((runtime_kind IS NULL) = (native_id IS NULL)),
    CHECK ((runtime_kind IS NULL) = (observed_at IS NULL))
) STRICT;

CREATE INDEX ix_consultation_seats_project
    ON consultation_seats(project_id, run_id, role_slot_id);

CREATE TABLE committee_findings (
    committee_run_id        TEXT    NOT NULL REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    project_id              TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    round                   INTEGER NOT NULL CHECK (round BETWEEN 1 AND 2),
    role_slot_id            TEXT    NOT NULL,
    role                    TEXT    NOT NULL CHECK (role IN ('reviewer', 'judge')),
    verdict                 TEXT    NOT NULL CHECK (verdict IN ('compliant', 'non_compliant')),
    evidence_complete       INTEGER NOT NULL CHECK (evidence_complete IN (0, 1)),
    document                TEXT    NOT NULL CHECK (json_valid(document)),
    document_hash           TEXT    NOT NULL
                                    CHECK (length(document_hash) = 64
                                           AND document_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at             TEXT    NOT NULL,
    PRIMARY KEY (committee_run_id, round, role_slot_id),
    FOREIGN KEY (project_id, committee_run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT,
    FOREIGN KEY (committee_run_id, role_slot_id)
        REFERENCES consultation_seats(run_id, role_slot_id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER committee_findings_are_immutable
BEFORE UPDATE ON committee_findings
BEGIN
    SELECT RAISE(ABORT, 'a Committee finding is immutable');
END;

CREATE TRIGGER committee_findings_are_permanent
BEFORE DELETE ON committee_findings
BEGIN
    SELECT RAISE(ABORT, 'a Committee finding cannot be withdrawn');
END;

-- Five consultation commands now have durable services behind them.
CREATE TABLE command_receipts_v36 (
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
                                 'upgrade_epic_roster', 'apply_advisor_profile',
                                 'apply_committee_template', 'apply_completion_profile',
                                 'advance_completion', 'remediate_completion',
                                 'invoke_advisor_run', 'settle_advisor_run',
                                 'invoke_committee_run', 'record_committee_findings',
                                 'settle_committee_run')),
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

INSERT INTO command_receipts_v36
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v36 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

PRAGMA user_version = 36;
