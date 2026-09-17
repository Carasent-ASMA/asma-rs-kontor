-- Schema v100. The authority a hosted-seat launch resolved, recorded before the
-- native call rather than after it.
--
-- v99 persisted autonomy beside the occupancy, which is where it belongs, but
-- the occupancy row is only written once the native answer comes back. That
-- leaves a window with a durable effect and no durable intent: the native is
-- created, the acknowledgement is lost or the process exits, and nothing says
-- what authority that native was asked for.
--
-- A replay inside that window resolved `permission_posture` afresh. If an
-- operator had changed it, the replay asked for one mode, found the native
-- already running in the other, and refused on the readback mismatch. That is
-- not a safe failure: the same mismatch blocks retire, so the seat can be
-- neither used nor replaced. It is the original ASMA-8193 wedge, reached
-- through a crash instead of a configuration change.
--
-- The intent is therefore written first and consumed when the occupancy binds.
-- It grants nothing by itself: it is a record of what was already decided, and
-- the occupancy row remains the authority every later control operation reads.
CREATE TABLE hosted_topology_seat_launch_intents (
    project_id           TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    seat_binding_id      TEXT NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    -- One intent per generation, so a replay converges on the row it already
    -- wrote instead of opening a second authority for the same occupancy.
    occupancy_generation INTEGER NOT NULL CHECK (occupancy_generation >= 1),
    autonomy             TEXT NOT NULL CHECK (autonomy IN (
                              'supervised', 'bounded', 'advisory')),
    model_rung           TEXT NOT NULL CHECK (json_valid(model_rung)),
    state                TEXT NOT NULL CHECK (state IN ('prepared', 'installed')),
    observed_native_id   TEXT NULL,
    prepared_at          TEXT NOT NULL,
    installed_at         TEXT NULL,
    PRIMARY KEY (project_id, seat_binding_id, occupancy_generation),
    CHECK ((state = 'installed') = (installed_at IS NOT NULL)),
    CHECK ((state = 'installed') = (observed_native_id IS NOT NULL))
) STRICT;

CREATE INDEX ix_hosted_seat_launch_intents_prepared
    ON hosted_topology_seat_launch_intents(project_id, seat_binding_id, state);

-- What a launch asked for is evidence, not a setting. Reconciliation may fill
-- the observed native and move `prepared` to `installed`; it may never restate
-- the authority or the route, because a row that could be rewritten would
-- record whatever the last replay resolved rather than what the native was
-- actually created under — which is the property this table exists to hold.
CREATE TRIGGER hosted_seat_launch_intent_decision_immutable
BEFORE UPDATE ON hosted_topology_seat_launch_intents
WHEN OLD.autonomy <> NEW.autonomy
  OR OLD.model_rung <> NEW.model_rung
  OR OLD.occupancy_generation <> NEW.occupancy_generation
  OR OLD.project_id <> NEW.project_id
  OR OLD.seat_binding_id <> NEW.seat_binding_id
  OR OLD.prepared_at <> NEW.prepared_at
  OR OLD.state = 'installed'
BEGIN
    SELECT RAISE(ABORT,
        'a hosted-seat launch intent records what was resolved before the native call and never changes it');
END;

PRAGMA user_version = 100;
