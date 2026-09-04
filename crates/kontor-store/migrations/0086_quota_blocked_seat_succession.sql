-- Schema v86. Bind every new runtime quota provenance record to the exact
-- control-plane observation cursor, then persist a forward-only succession
-- attempt before any predecessor retirement or successor launch.

-- Historical v85 rows predate this link and remain readable as legacy evidence.
-- Every insert after this migration must provide the cursor, and the trigger
-- proves it is the matching blocked observation rather than a native content
-- sequence copied into the wrong namespace.
ALTER TABLE provider_quota_observation_provenance
ADD COLUMN runtime_observation_cursor INTEGER NULL REFERENCES runtime_events(cursor) ON DELETE RESTRICT;

CREATE UNIQUE INDEX runtime_bindings_succession_exact_tuple
ON runtime_bindings (
    project_id, id, agent_run_id, runtime_kind, host, generation, native_id
);

CREATE TRIGGER provider_quota_provenance_requires_control_cursor
BEFORE INSERT ON provider_quota_observation_provenance
WHEN NEW.runtime_observation_cursor IS NULL
BEGIN
    SELECT RAISE(ABORT, 'new runtime quota provenance requires its control observation cursor');
END;

CREATE TRIGGER provider_quota_provenance_cursor_matches_observation
BEFORE INSERT ON provider_quota_observation_provenance
BEGIN
    SELECT RAISE(ABORT, 'quota provenance cursor must name its exact blocked runtime observation')
    WHERE NOT EXISTS (
        SELECT 1 FROM runtime_events
         WHERE cursor = NEW.runtime_observation_cursor
           AND project_id = NEW.project_id
           AND event_kind = 'runtime_observation'
           AND agent_run_id = NEW.agent_run_id
           AND runtime_kind = (
               SELECT runtime_kind FROM runtime_bindings WHERE id = NEW.runtime_binding_id
           )
           AND host = (
               SELECT host FROM runtime_bindings WHERE id = NEW.runtime_binding_id
           )
           AND generation = NEW.binding_generation
           AND native_id = NEW.native_id
           AND observed_state = 'blocked'
    );
END;

CREATE TABLE succession_attempts (
    id                              TEXT NOT NULL PRIMARY KEY
        CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id                      TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    task_id                         TEXT NOT NULL,
    team_run_id                     TEXT NOT NULL,
    role_key                        TEXT NOT NULL CHECK (length(role_key) BETWEEN 1 AND 128),
    predecessor_agent_run_id        TEXT NOT NULL,
    predecessor_runtime_binding_id  TEXT NOT NULL,
    predecessor_runtime_kind        TEXT NOT NULL CHECK (length(predecessor_runtime_kind) BETWEEN 1 AND 128),
    predecessor_host                TEXT NOT NULL CHECK (length(predecessor_host) BETWEEN 1 AND 512),
    predecessor_native_id           TEXT NOT NULL CHECK (length(predecessor_native_id) BETWEEN 1 AND 256),
    predecessor_generation          INTEGER NOT NULL CHECK (predecessor_generation >= 0),
    expected_task_revision          INTEGER NOT NULL CHECK (expected_task_revision >= 1),
    expected_team_revision          INTEGER NOT NULL CHECK (expected_team_revision >= 1),
    expected_predecessor_revision   INTEGER NOT NULL CHECK (expected_predecessor_revision >= 1),
    runtime_observation_cursor      INTEGER NOT NULL,
    quota_provenance_id             TEXT NOT NULL,
    quota_state_revision            INTEGER NOT NULL CHECK (quota_state_revision >= 1),
    quota_evidence_hash             TEXT NOT NULL CHECK (
        length(quota_evidence_hash) = 64 AND quota_evidence_hash NOT GLOB '*[^0-9a-f]*'
    ),
    quota_provider                  TEXT NOT NULL CHECK (length(quota_provider) BETWEEN 1 AND 128),
    successor_model_rung            TEXT NULL CHECK (
        successor_model_rung IS NULL OR json_valid(successor_model_rung)
    ),
    successor_model_rung_hash       TEXT NULL CHECK (
        successor_model_rung_hash IS NULL OR (
            length(successor_model_rung_hash) = 64
            AND successor_model_rung_hash NOT GLOB '*[^0-9a-f]*'
        )
    ),
    successor_account_profile_id    TEXT NULL,
    idempotency_key                 TEXT NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    intent_hash                     TEXT NOT NULL CHECK (
        length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'
    ),
    state                           TEXT NOT NULL CHECK (state IN (
        'planned', 'deferred', 'predecessor_retired', 'successor_observed', 'confirmed', 'refused'
    )),
    deferred_until                  TEXT NULL CHECK (
        deferred_until IS NULL OR deferred_until GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'
    ),
    handoff                         TEXT NULL CHECK (handoff IS NULL OR json_valid(handoff)),
    handoff_hash                    TEXT NULL CHECK (
        handoff_hash IS NULL OR (length(handoff_hash) = 64 AND handoff_hash NOT GLOB '*[^0-9a-f]*')
    ),
    successor_agent_run_id          TEXT NULL,
    successor_runtime_binding_id    TEXT NULL,
    successor_runtime_kind          TEXT NULL,
    successor_host                  TEXT NULL,
    successor_native_id             TEXT NULL,
    successor_generation            INTEGER NULL CHECK (successor_generation IS NULL OR successor_generation >= 0),
    successor_observation_cursor    INTEGER NULL,
    successor_observed_at           TEXT NULL,
    refusal_reason                  TEXT NULL CHECK (refusal_reason IS NULL OR refusal_reason IN (
        'evidence_stale', 'quota_no_longer_blocking', 'retirement_refused',
        'launch_refused', 'confirmation_refused'
    )),
    revision                        INTEGER NOT NULL CHECK (revision >= 1),
    created_at                      TEXT NOT NULL CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    updated_at                      TEXT NOT NULL CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    predecessor_retired_at          TEXT NULL,
    confirmed_at                    TEXT NULL,
    refused_at                      TEXT NULL,
    successor_planned_at            TEXT NULL CHECK (
        successor_planned_at IS NULL
        OR successor_planned_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'
    ),

    UNIQUE (project_id, id),
    CHECK (state <> 'deferred' OR deferred_until IS NOT NULL),
    CHECK ((successor_model_rung IS NULL)
           = (successor_model_rung_hash IS NULL
              AND successor_account_profile_id IS NULL
              AND successor_planned_at IS NULL)),
    CHECK (state <> 'deferred' OR successor_model_rung IS NULL),
    CHECK (state NOT IN ('planned', 'predecessor_retired', 'successor_observed', 'confirmed')
           OR successor_model_rung IS NOT NULL),
    CHECK ((handoff IS NULL) = (handoff_hash IS NULL)),
    CHECK ((successor_agent_run_id IS NULL)
           = (successor_runtime_binding_id IS NULL
              AND successor_runtime_kind IS NULL
              AND successor_host IS NULL
              AND successor_native_id IS NULL
              AND successor_generation IS NULL
              AND successor_observation_cursor IS NULL
              AND successor_observed_at IS NULL)),
    CHECK ((state IN ('successor_observed', 'confirmed')) = (successor_agent_run_id IS NOT NULL)),
    CHECK (state NOT IN ('predecessor_retired', 'successor_observed', 'confirmed')
           OR (predecessor_retired_at IS NOT NULL AND handoff_hash IS NOT NULL)),
    CHECK ((state = 'confirmed') = (confirmed_at IS NOT NULL)),
    CHECK ((state = 'refused') = (refusal_reason IS NOT NULL AND refused_at IS NOT NULL)),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_run_id) REFERENCES team_runs(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, predecessor_agent_run_id)
        REFERENCES agent_runs(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, predecessor_runtime_binding_id, predecessor_agent_run_id,
                 predecessor_runtime_kind, predecessor_host, predecessor_generation,
                 predecessor_native_id)
        REFERENCES runtime_bindings(project_id, id, agent_run_id, runtime_kind, host,
                                    generation, native_id) ON DELETE RESTRICT,
    FOREIGN KEY (runtime_observation_cursor) REFERENCES runtime_events(cursor) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, quota_provenance_id)
        REFERENCES provider_quota_observation_provenance(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, successor_account_profile_id)
        REFERENCES account_profiles(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, successor_agent_run_id)
        REFERENCES agent_runs(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, successor_runtime_binding_id, successor_agent_run_id,
                 successor_runtime_kind, successor_host, successor_generation,
                 successor_native_id)
        REFERENCES runtime_bindings(project_id, id, agent_run_id, runtime_kind, host,
                                    generation, native_id) ON DELETE RESTRICT,
    FOREIGN KEY (successor_observation_cursor) REFERENCES runtime_events(cursor) ON DELETE RESTRICT
) STRICT;

CREATE UNIQUE INDEX one_active_succession_per_team_role
ON succession_attempts(project_id, team_run_id, role_key)
WHERE state IN ('planned', 'deferred', 'predecessor_retired', 'successor_observed');

CREATE INDEX succession_attempts_startup
ON succession_attempts(state, deferred_until, created_at, id);

CREATE TRIGGER succession_attempts_move_forward_only
BEFORE UPDATE OF state ON succession_attempts
WHEN NOT (
    (OLD.state = 'deferred' AND NEW.state IN ('planned', 'refused'))
    OR (OLD.state = 'planned' AND NEW.state IN ('predecessor_retired', 'refused'))
    OR (OLD.state = 'predecessor_retired' AND NEW.state IN ('successor_observed', 'refused'))
    OR (OLD.state = 'successor_observed' AND NEW.state IN ('confirmed', 'refused'))
)
BEGIN
    SELECT RAISE(ABORT, 'a succession attempt may only move forward');
END;

CREATE TRIGGER succession_attempts_immutable_decision
BEFORE UPDATE ON succession_attempts
WHEN OLD.project_id <> NEW.project_id
  OR OLD.task_id <> NEW.task_id
  OR OLD.team_run_id <> NEW.team_run_id
  OR OLD.role_key <> NEW.role_key
  OR OLD.predecessor_agent_run_id <> NEW.predecessor_agent_run_id
  OR OLD.predecessor_runtime_binding_id <> NEW.predecessor_runtime_binding_id
  OR OLD.predecessor_native_id <> NEW.predecessor_native_id
  OR OLD.predecessor_generation <> NEW.predecessor_generation
  OR OLD.runtime_observation_cursor <> NEW.runtime_observation_cursor
  OR OLD.quota_provenance_id <> NEW.quota_provenance_id
  OR OLD.quota_state_revision <> NEW.quota_state_revision
  OR OLD.quota_evidence_hash <> NEW.quota_evidence_hash
  OR OLD.deferred_until IS NOT NEW.deferred_until
  OR OLD.idempotency_key <> NEW.idempotency_key
  OR OLD.intent_hash <> NEW.intent_hash
BEGIN
    SELECT RAISE(ABORT, 'a succession decision is immutable');
END;

CREATE TRIGGER succession_attempts_freeze_successor_once
BEFORE UPDATE OF successor_model_rung, successor_model_rung_hash,
                 successor_account_profile_id, successor_planned_at
ON succession_attempts
WHEN NOT (
    OLD.state = 'deferred'
    AND NEW.state = 'planned'
    AND OLD.successor_model_rung IS NULL
    AND OLD.successor_model_rung_hash IS NULL
    AND OLD.successor_account_profile_id IS NULL
    AND OLD.successor_planned_at IS NULL
    AND NEW.successor_model_rung IS NOT NULL
    AND NEW.successor_model_rung_hash IS NOT NULL
    AND NEW.successor_account_profile_id IS NOT NULL
    AND NEW.successor_planned_at IS NOT NULL
    AND OLD.deferred_until <= NEW.successor_planned_at
)
BEGIN
    SELECT RAISE(ABORT, 'a succession successor route may only be frozen once when due');
END;

CREATE TRIGGER succession_attempts_are_permanent
BEFORE DELETE ON succession_attempts
BEGIN
    SELECT RAISE(ABORT, 'a succession attempt cannot be withdrawn');
END;

CREATE TABLE succession_receipts (
    id             TEXT NOT NULL PRIMARY KEY
        CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    attempt_id     TEXT NOT NULL UNIQUE,
    receipt        TEXT NOT NULL CHECK (json_valid(receipt)),
    receipt_hash   TEXT NOT NULL CHECK (
        length(receipt_hash) = 64 AND receipt_hash NOT GLOB '*[^0-9a-f]*'
    ),
    confirmed_at   TEXT NOT NULL CHECK (confirmed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, attempt_id)
        REFERENCES succession_attempts(project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER succession_receipts_are_immutable
BEFORE UPDATE ON succession_receipts
BEGIN
    SELECT RAISE(ABORT, 'a succession receipt is immutable');
END;

CREATE TRIGGER succession_receipts_are_permanent
BEFORE DELETE ON succession_receipts
BEGIN
    SELECT RAISE(ABORT, 'a succession receipt cannot be withdrawn');
END;

PRAGMA user_version = 86;
