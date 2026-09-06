-- Durable release intent is separate from the consultation verdict. Archiving
-- a session never changes a finding, seat binding, or settled run.
CREATE TABLE consultation_session_releases (
    project_id TEXT NOT NULL REFERENCES projects(id),
    run_id TEXT NOT NULL REFERENCES consultation_runs(run_id),
    seat_binding_id TEXT NOT NULL REFERENCES seat_bindings(id),
    native_id TEXT NOT NULL,
    native_identity TEXT NOT NULL CHECK (json_valid(native_identity)),
    requested_at TEXT NOT NULL,
    attempted_at TEXT,
    archived_at TEXT,
    PRIMARY KEY (project_id, run_id, seat_binding_id, native_id)
) STRICT;
CREATE TRIGGER consultation_release_identity_immutable BEFORE UPDATE ON consultation_session_releases
WHEN OLD.project_id <> NEW.project_id OR OLD.run_id <> NEW.run_id
  OR OLD.seat_binding_id <> NEW.seat_binding_id OR OLD.native_id <> NEW.native_id
  OR OLD.native_identity <> NEW.native_identity
  OR OLD.requested_at <> NEW.requested_at
  OR (OLD.archived_at IS NOT NULL AND OLD.archived_at IS NOT NEW.archived_at)
BEGIN SELECT RAISE(ABORT, 'consultation release identity and confirmation are immutable'); END;
CREATE TRIGGER consultation_release_not_deletable BEFORE DELETE ON consultation_session_releases
BEGIN SELECT RAISE(ABORT, 'consultation releases are not deletable'); END;
PRAGMA user_version = 93;
