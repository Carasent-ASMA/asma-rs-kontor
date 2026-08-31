-- Schema v75. Committee permission answers are control-plane effects. Their
-- exact seat/native/request/decision identity is durable before dispatch, a
-- single caller claims dispatch, and only a runtime acknowledgement confirms.
CREATE TABLE consultation_permission_responses (
    project_id          TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    committee_run_id    TEXT NOT NULL REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    seat_binding_id     TEXT NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    occupancy_generation INTEGER NOT NULL CHECK (occupancy_generation >= 1),
    native_id           TEXT NOT NULL CHECK (length(native_id) BETWEEN 1 AND 255),
    permission_id       TEXT NOT NULL CHECK (length(permission_id) BETWEEN 1 AND 255),
    response_id         TEXT NOT NULL UNIQUE CHECK (length(response_id) = 36),
    decision            TEXT NOT NULL CHECK (decision IN ('allow', 'deny')),
    status              TEXT NOT NULL CHECK (status IN ('planned', 'dispatching', 'confirmed')),
    planned_at          TEXT NOT NULL,
    accepted_at         TEXT NULL,
    PRIMARY KEY (project_id, committee_run_id, seat_binding_id, occupancy_generation, permission_id),
    FOREIGN KEY (project_id, committee_run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT,
    CHECK ((status = 'confirmed') = (accepted_at IS NOT NULL))
) STRICT;

CREATE TRIGGER consultation_permission_response_scope_exact
BEFORE INSERT ON consultation_permission_responses
WHEN NOT EXISTS (
    SELECT 1
      FROM consultation_runs AS run
      JOIN consultation_seats AS seat ON seat.run_id = run.run_id
     WHERE run.project_id = NEW.project_id
       AND run.run_id = NEW.committee_run_id
       AND run.family = 'committee'
       AND seat.seat_binding_id = NEW.seat_binding_id
       AND seat.occupancy_generation = NEW.occupancy_generation
       AND seat.native_id = NEW.native_id
)
BEGIN SELECT RAISE(ABORT, 'Committee permission response scope is not exact'); END;

CREATE TRIGGER consultation_permission_response_identity_immutable
BEFORE UPDATE ON consultation_permission_responses
WHEN OLD.project_id <> NEW.project_id
  OR OLD.committee_run_id <> NEW.committee_run_id
  OR OLD.seat_binding_id <> NEW.seat_binding_id
  OR OLD.occupancy_generation <> NEW.occupancy_generation
  OR OLD.native_id <> NEW.native_id
  OR OLD.permission_id <> NEW.permission_id
  OR OLD.response_id <> NEW.response_id
  OR OLD.decision <> NEW.decision
  OR OLD.planned_at <> NEW.planned_at
  OR OLD.status = 'confirmed'
  OR (OLD.status = 'planned'
      AND (NEW.status <> 'dispatching' OR NEW.accepted_at IS NOT NULL))
  OR (OLD.status = 'dispatching'
      AND (NEW.status <> 'confirmed' OR NEW.accepted_at IS NULL))
BEGIN SELECT RAISE(ABORT, 'Committee permission response identity is immutable'); END;

CREATE TRIGGER consultation_permission_responses_are_permanent
BEFORE DELETE ON consultation_permission_responses
BEGIN SELECT RAISE(ABORT, 'Committee permission responses are permanent'); END;

PRAGMA user_version = 75;
