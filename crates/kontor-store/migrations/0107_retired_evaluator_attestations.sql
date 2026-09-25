-- Schema v107. A retired evaluator seat's verdict has no supported proof path.
--
-- When rejection settlement retires the exact audit SeatBinding, the gate it
-- rendered can no longer be recorded: the forward-only live-seat challenge
-- refuses because there is no live seat, and widening that surface would
-- weaken it for every caller that legitimately needs a *new* answer from a
-- seat that can still give one.
--
-- This version adds the durable half of the other path. The judgement itself
-- is a pure function in `kontor_core::retired_evaluator` and is unchanged
-- here; what is added is the command kind that records one, and an append-only
-- ledger of the proofs it produced, so that a closed-evaluator recovery can
-- cite a proof instead of an operator retyping a verdict.
--
-- Nothing here evaluates a gate, mutates topology, or dispatches to a native.
-- A proof is evidence that an already-durable verdict existed; it is not the
-- verdict, and recording one advances no workflow by itself.

-- ---------------------------------------------------------------------------
-- 1. One new command kind
-- ---------------------------------------------------------------------------

-- The closed kind list has to be widened by rebuilding the table: SQLite
-- cannot alter a CHECK in place. The shape below is the v106 shape verbatim
-- plus `attest_retired_evaluator_evidence`.

CREATE TABLE command_receipts_v107 (
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

-- Migration 0092 put a `command_receipts` subquery inside a trigger body on
-- another table (`consultation_run_inputs_are_frozen`). Modern
-- `ALTER TABLE ... RENAME` reparses the whole schema, so between the drop and
-- the rename that trigger names a table which does not exist yet and the
-- reparse fails. Legacy rename semantics do not reparse, and are correct here
-- for exactly that reason: the trigger already spells the name this rename is
-- about to restore, so there is nothing to rewrite.
INSERT INTO command_receipts_v107 SELECT * FROM command_receipts;
PRAGMA legacy_alter_table = ON;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v107 RENAME TO command_receipts;
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

-- ---------------------------------------------------------------------------
-- 2. The append-only proof ledger
-- ---------------------------------------------------------------------------

-- One row per attested retired evaluator. Every fenced fact is stored, not
-- just the digest, so a later reader can see *what* was proved without having
-- to re-derive it, and so a mismatch names the field that disagreed.
--
-- `proof_digest` is the deterministic digest of exactly those facts. It is
-- UNIQUE per project: an identical claim replays onto the same row, which is
-- what makes a lost acknowledgement safe, while a claim that changed any fact
-- digests differently and is a different row rather than a silent overwrite.
CREATE TABLE retired_evaluator_attestations (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    receipt_id TEXT NOT NULL REFERENCES command_receipts(id) ON DELETE RESTRICT,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE RESTRICT,
    workflow_revision INTEGER NOT NULL CHECK (workflow_revision >= 1),
    gate_key TEXT NOT NULL CHECK (length(gate_key) BETWEEN 1 AND 128),
    team_run_id TEXT NOT NULL,
    evaluator_role TEXT NOT NULL CHECK (length(evaluator_role) BETWEEN 1 AND 128),
    role_slot_id TEXT NOT NULL CHECK (length(role_slot_id) BETWEEN 1 AND 128),
    agent_run_id TEXT NOT NULL,
    seat_binding_id TEXT NOT NULL,
    seat_revision INTEGER NOT NULL CHECK (seat_revision >= 1),
    runtime_binding_id TEXT NOT NULL,
    runtime_generation INTEGER NOT NULL CHECK (runtime_generation >= 0),
    native_id TEXT NOT NULL,
    artifact_key TEXT NOT NULL CHECK (length(artifact_key) BETWEEN 1 AND 128),
    artifact_checksum TEXT NOT NULL
        CHECK (length(artifact_checksum) = 64 AND artifact_checksum NOT GLOB '*[^0-9a-f]*'),
    evidence_digest TEXT NOT NULL
        CHECK (length(evidence_digest) = 64 AND evidence_digest NOT GLOB '*[^0-9a-f]*'),
    proof_digest TEXT NOT NULL
        CHECK (length(proof_digest) = 64 AND proof_digest NOT GLOB '*[^0-9a-f]*'),
    attested_at TEXT NOT NULL
        CHECK (attested_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    UNIQUE (project_id, proof_digest),
    UNIQUE (receipt_id)
) STRICT;

CREATE INDEX ix_retired_evaluator_attestations_gate
    ON retired_evaluator_attestations(project_id, task_id, gate_key);

-- Append-only, like every other evidence ledger in this schema: a proof that
-- can be edited or removed is not evidence of anything.
CREATE TRIGGER retired_evaluator_attestations_immutable BEFORE UPDATE
ON retired_evaluator_attestations
BEGIN SELECT RAISE(ABORT, 'a retired-evaluator attestation is immutable'); END;
CREATE TRIGGER retired_evaluator_attestations_no_delete BEFORE DELETE
ON retired_evaluator_attestations
BEGIN SELECT RAISE(ABORT, 'retired-evaluator attestations are not deletable'); END;

PRAGMA user_version = 107;
