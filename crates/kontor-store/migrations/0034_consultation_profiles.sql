-- Immutable Advisor profile and Committee template revisions.
--
-- A consultation is read-only advice, and what it may read, who may ask it and
-- how its output is aggregated is policy. A run pins the exact revision it was
-- invoked under, so every revision is kept: advice recorded a month ago has to
-- stay readable against the policy that produced it, even after the project has
-- edited that profile ten times.
--
-- Both families share one table because they are one storage shape — an
-- identity, a monotonic version within it, a frozen label and the digest of the
-- canonical definition. The wire contract already says so (`ProfileRevisionDto`
-- is shared), and two tables differing only by a discriminator would need every
-- read, every trigger and every later migration written twice.
--
-- The typed definition is held as the canonical document the domain produced,
-- not as columns this layer would re-validate. The store's obligation is that
-- what it returns is byte-identical to what was published; whether a template
-- could produce an independent conjunction is `CommitteeTemplateSpec`'s job and
-- is already pinned by `definition_hash`.
--
-- As with `core_team_revisions` there is deliberately no `is_current` column.
-- The current revision of one profile is the highest version published under
-- its id, and a family's catalog revision is how many revisions the project has
-- published into it — both derived from these rows rather than second facts that
-- could disagree with them.
CREATE TABLE consultation_profile_revisions (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    -- Which family the revision belongs to. Closed here rather than in Rust
    -- alone so a caller that reached the database directly still cannot invent
    -- a third consultation family.
    family          TEXT    NOT NULL CHECK (family IN ('advisor', 'committee')),
    profile_id      TEXT    NOT NULL
                            CHECK (length(profile_id) = 36 AND profile_id NOT GLOB '*[^0-9a-f-]*'),
    version         INTEGER NOT NULL CHECK (version >= 1),
    -- The label frozen at publish, so a catalog read reports the name this
    -- revision was published under rather than the newest one.
    name            TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 512),
    definition      TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64
                                   AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, family, profile_id, version)
) STRICT;

-- A published revision is published. A run has already pinned it, so editing or
-- withdrawing one would silently change what an already-recorded consultation
-- claims it was asked under.
CREATE TRIGGER consultation_profile_revisions_are_immutable
BEFORE UPDATE ON consultation_profile_revisions
BEGIN
    SELECT RAISE(ABORT, 'a published consultation profile revision is immutable');
END;

CREATE TRIGGER consultation_profile_revisions_are_permanent
BEFORE DELETE ON consultation_profile_revisions
BEGIN
    SELECT RAISE(ABORT, 'a published consultation profile revision cannot be withdrawn');
END;

-- Widen the closed command-kind list by the two publication commands.
--
-- Same rebuild shape as v24, v28, v29, v30 and v31, and for the same reason:
-- `kind` is a CHECK, so a new command is a migration rather than a code change.
--
-- Only the two Admin publications are added. Invoking, recording findings and
-- settling belong to the durable services OP-05 composes next, and a command
-- kind accepted here before the service that writes it exists would be a
-- promise the database keeps and the daemon does not.
CREATE TABLE command_receipts_v34 (
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
                                 'apply_committee_template')),
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

INSERT INTO command_receipts_v34
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v34 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

PRAGMA user_version = 34;
