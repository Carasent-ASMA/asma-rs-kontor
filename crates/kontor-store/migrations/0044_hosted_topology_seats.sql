-- Native identity for persistent topology seats that are not Delivery
-- AgentRuns (notably the LSA and TPM hosted in an epic control plane).
--
-- SeatBinding remains the logical identity and uniqueness key. This table only
-- records the exact runtime readback and frozen route that fills it, allowing
-- launch recovery and messages without inventing a TeamRun.
CREATE TABLE hosted_topology_seats (
    seat_binding_id     TEXT NOT NULL PRIMARY KEY
                             REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    project_id          TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    model_rung          TEXT NOT NULL CHECK (json_valid(model_rung)),
    runtime_kind        TEXT NOT NULL,
    host                TEXT NOT NULL,
    generation          INTEGER NOT NULL CHECK (generation >= 0),
    native_id           TEXT NOT NULL,
    provider_session_id TEXT NULL,
    observed_at         TEXT NOT NULL,
    UNIQUE (runtime_kind, host, generation, native_id)
) STRICT;

CREATE INDEX ix_hosted_topology_seats_project
    ON hosted_topology_seats(project_id, seat_binding_id);

PRAGMA user_version = 44;
