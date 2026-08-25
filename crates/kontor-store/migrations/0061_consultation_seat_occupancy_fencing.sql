-- Schema v61. Generation-fenced consultation-seat occupancy recovery.
--
-- SeatBinding is the immutable logical subject.  A native filler holds one
-- occupancy generation only.  Recovery fences that generation and persists
-- the exact policy/route before contacting the runtime, so a retry can finish
-- the same replacement after a crash or lost acknowledgement.
ALTER TABLE consultation_seats
ADD COLUMN occupancy_generation INTEGER NOT NULL DEFAULT 1
    CHECK (occupancy_generation >= 1);

ALTER TABLE consultation_seat_recoveries
ADD COLUMN predecessor_occupancy_generation INTEGER NOT NULL DEFAULT 1
    CHECK (predecessor_occupancy_generation >= 1);

ALTER TABLE consultation_seat_recoveries
ADD COLUMN successor_occupancy_generation INTEGER NOT NULL DEFAULT 1
    CHECK (successor_occupancy_generation >= 1);

CREATE TABLE consultation_seat_recovery_attempts (
    project_id                       TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    run_id                           TEXT NOT NULL REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    role_slot_id                     TEXT NOT NULL,
    seat_binding_id                  TEXT NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    predecessor_native_id            TEXT NOT NULL,
    predecessor_occupancy_generation INTEGER NOT NULL CHECK (predecessor_occupancy_generation >= 1),
    successor_occupancy_generation   INTEGER NOT NULL CHECK (successor_occupancy_generation >= 2),
    predecessor_run_revision         INTEGER NOT NULL CHECK (predecessor_run_revision >= 1),
    prepared_run_revision            INTEGER NOT NULL CHECK (prepared_run_revision = predecessor_run_revision + 1),
    recovery_reason                  TEXT NOT NULL CHECK (recovery_reason IN (
                                              'credential_propagation', 'provider_unavailable')),
    request_intent_hash              TEXT NOT NULL
                                               CHECK (length(request_intent_hash) = 64
                                                      AND request_intent_hash NOT GLOB '*[^0-9a-f]*'),
    recovery_profile                 TEXT NOT NULL CHECK (json_valid(recovery_profile)),
    recovery_profile_hash            TEXT NOT NULL
                                               CHECK (length(recovery_profile_hash) = 64
                                                      AND recovery_profile_hash NOT GLOB '*[^0-9a-f]*'),
    selected_model_rung              TEXT NOT NULL CHECK (json_valid(selected_model_rung)),
    state                            TEXT NOT NULL CHECK (state IN (
                                              'prepared', 'predecessor_retired',
                                              'successor_observed', 'installed')),
    successor_runtime_kind           TEXT NULL,
    successor_host                   TEXT NULL,
    successor_generation             INTEGER NULL CHECK (successor_generation IS NULL OR successor_generation >= 0),
    successor_native_id              TEXT NULL,
    successor_provider_session       TEXT NULL,
    successor_observed_at            TEXT NULL,
    prepared_at                      TEXT NOT NULL,
    retired_at                       TEXT NULL,
    installed_at                     TEXT NULL,
    PRIMARY KEY (project_id, run_id, role_slot_id, predecessor_native_id),
    UNIQUE (project_id, seat_binding_id, successor_occupancy_generation),
    FOREIGN KEY (project_id, run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT,
    FOREIGN KEY (run_id, role_slot_id)
        REFERENCES consultation_seats(run_id, role_slot_id) ON DELETE RESTRICT,
    CHECK ((successor_runtime_kind IS NULL) = (successor_host IS NULL)),
    CHECK ((successor_runtime_kind IS NULL) = (successor_generation IS NULL)),
    CHECK ((successor_runtime_kind IS NULL) = (successor_native_id IS NULL)),
    CHECK ((successor_runtime_kind IS NULL) = (successor_observed_at IS NULL)),
    CHECK ((state IN ('successor_observed', 'installed')) = (successor_native_id IS NOT NULL)),
    CHECK ((state = 'installed') = (installed_at IS NOT NULL))
) STRICT;

CREATE TRIGGER consultation_seat_recovery_attempt_identity_immutable
BEFORE UPDATE ON consultation_seat_recovery_attempts
WHEN OLD.project_id <> NEW.project_id
  OR OLD.run_id <> NEW.run_id
  OR OLD.role_slot_id <> NEW.role_slot_id
  OR OLD.seat_binding_id <> NEW.seat_binding_id
  OR OLD.predecessor_native_id <> NEW.predecessor_native_id
  OR OLD.predecessor_occupancy_generation <> NEW.predecessor_occupancy_generation
  OR OLD.successor_occupancy_generation <> NEW.successor_occupancy_generation
  OR OLD.predecessor_run_revision <> NEW.predecessor_run_revision
  OR OLD.prepared_run_revision <> NEW.prepared_run_revision
  OR OLD.recovery_reason <> NEW.recovery_reason
  OR OLD.request_intent_hash <> NEW.request_intent_hash
  OR OLD.recovery_profile <> NEW.recovery_profile
  OR OLD.recovery_profile_hash <> NEW.recovery_profile_hash
  OR OLD.selected_model_rung <> NEW.selected_model_rung
  OR OLD.prepared_at <> NEW.prepared_at
BEGIN SELECT RAISE(ABORT, 'a consultation recovery attempt identity is immutable'); END;

CREATE TRIGGER consultation_seat_recovery_attempt_state_forward_only
BEFORE UPDATE OF state ON consultation_seat_recovery_attempts
WHEN CASE OLD.state
       WHEN 'prepared' THEN NEW.state NOT IN ('prepared', 'predecessor_retired', 'successor_observed', 'installed')
       WHEN 'predecessor_retired' THEN NEW.state NOT IN ('predecessor_retired', 'successor_observed', 'installed')
       WHEN 'successor_observed' THEN NEW.state NOT IN ('successor_observed', 'installed')
       WHEN 'installed' THEN NEW.state <> 'installed'
     END
BEGIN SELECT RAISE(ABORT, 'a consultation recovery attempt cannot move backward'); END;

CREATE TRIGGER consultation_seat_recovery_attempts_are_not_deletable
BEFORE DELETE ON consultation_seat_recovery_attempts
BEGIN SELECT RAISE(ABORT, 'consultation recovery attempts are not deletable'); END;

PRAGMA user_version = 61;
