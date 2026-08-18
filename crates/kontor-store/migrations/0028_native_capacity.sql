-- Account-owned capacity evidence, and the operator judgement that stands
-- beside it.
--
-- Two tables rather than one column, because they are two different facts with
-- two different authorities. `capacity_observations` is what a collector read
-- from a runtime family; `availability_overrides` is what an operator asserts
-- in spite of it. Folding the override into the observation would leave no
-- record of the disagreement, and the disagreement is the only reason an
-- override is worth having.
--
-- Nothing here is derived from `~/.asma/fleet` or any other program's store:
-- the reading column holds what Kontor's own collector observed.

-- One raw reading, immutable once written.
--
-- `reading` is the collector's evidence verbatim, as canonical JSON. It is the
-- record an override must not be able to touch, so the table refuses UPDATE
-- and DELETE outright below rather than trusting every future call site to
-- only insert.
CREATE TABLE capacity_observations (
    id                 TEXT NOT NULL PRIMARY KEY
                            CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id         TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    account_profile_id TEXT NOT NULL,
    observed_at        TEXT NOT NULL
                            CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    reading            TEXT NOT NULL CHECK (json_valid(reading)),
    -- Re-admitted through its own digest on read, like every other canonical
    -- document here: a reading edited underneath the store fails to load
    -- rather than being believed.
    reading_hash       TEXT NOT NULL
                            CHECK (length(reading_hash) = 64 AND reading_hash NOT GLOB '*[^0-9a-f]*'),
    -- Derived in the same transaction as the insert, never later: a row whose
    -- conclusion was written by a second pass could disagree with its own
    -- evidence.
    available          INTEGER NOT NULL CHECK (available IN (0, 1)),
    pressure           INTEGER NOT NULL CHECK (pressure IN (0, 1)),
    cooling_until      TEXT
                            CHECK (cooling_until IS NULL
                                   OR cooling_until GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    FOREIGN KEY (project_id, account_profile_id)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_capacity_observations_account
    ON capacity_observations (project_id, account_profile_id, observed_at DESC);

CREATE TRIGGER trg_capacity_observations_immutable_update
BEFORE UPDATE ON capacity_observations
BEGIN
    SELECT RAISE(ABORT, 'capacity observations are immutable raw evidence');
END;

CREATE TRIGGER trg_capacity_observations_immutable_delete
BEFORE DELETE ON capacity_observations
BEGIN
    SELECT RAISE(ABORT, 'capacity observations are immutable raw evidence');
END;

-- One standing operator judgement per account, replaced under compare-and-swap.
--
-- `expires_at` is how a judgement stops being permanent by accident: an
-- operator who marks an account usable during an incident does not have to
-- remember to undo it.
CREATE TABLE availability_overrides (
    project_id         TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    account_profile_id TEXT NOT NULL,
    available          INTEGER NOT NULL CHECK (available IN (0, 1)),
    reason             TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 512),
    expires_at         TEXT
                            CHECK (expires_at IS NULL
                                   OR expires_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revision           INTEGER NOT NULL CHECK (revision > 0),
    updated_at         TEXT NOT NULL
                            CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, account_profile_id),
    FOREIGN KEY (project_id, account_profile_id)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;

-- The Realm's capacity ceilings, as an operator has set them.
--
-- One row, enforced by the primary key: ceilings constrain one another, so
-- there is one document to read and one revision to present, never a per-field
-- history that could be half-applied.
--
-- This record is what an operator *asked for*. The ceilings a running daemon
-- admits under are the ones it was composed with, and they stay that way for
-- the life of the process on purpose: a Realm that re-read its ceilings between
-- planning a batch and committing it could refuse a candidate its own plan had
-- already admitted.
CREATE TABLE capacity_configuration (
    id         INTEGER NOT NULL PRIMARY KEY CHECK (id = 1),
    ceilings   TEXT NOT NULL CHECK (json_valid(ceilings)),
    ceilings_hash TEXT NOT NULL
                    CHECK (length(ceilings_hash) = 64 AND ceilings_hash NOT GLOB '*[^0-9a-f]*'),
    revision   INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL
                    CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

-- Widen the closed command-kind list by the four project-scoped capacity and
-- exact-seat commands.
--
-- Same rebuild shape as v24, and for the same reason: `kind` is a CHECK, so a
-- new command is a migration rather than a code change, and a value SQL would
-- accept but the code cannot interpret never reaches a row.
CREATE TABLE command_receipts_v28 (
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
                                 'observe_seat', 'retire_seat')),
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

INSERT INTO command_receipts_v28
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v28 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

-- Widen the realm-scoped idempotency operations by exactly one.
--
-- A capacity-configuration apply is realm-wide, so it has no aggregate for a
-- command receipt to name; the binding table is the mechanism v15 built for
-- precisely that case. The list is a CHECK, so widening it is a table rebuild
-- rather than a code change — which is the point: a value SQL accepts but the
-- code cannot interpret is an unreadable row.
CREATE TABLE realm_idempotency_bindings_v28 (
    idempotency_key TEXT NOT NULL PRIMARY KEY
                         CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    operation       TEXT NOT NULL CHECK (operation IN (
                             'register_profile_pack', 'apply_capacity_configuration')),
    fingerprint     TEXT NOT NULL
                         CHECK (length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    bound_at        TEXT NOT NULL
                         CHECK (bound_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

INSERT INTO realm_idempotency_bindings_v28
SELECT idempotency_key, operation, fingerprint, bound_at FROM realm_idempotency_bindings;

DROP TABLE realm_idempotency_bindings;
ALTER TABLE realm_idempotency_bindings_v28 RENAME TO realm_idempotency_bindings;

PRAGMA user_version = 28;
