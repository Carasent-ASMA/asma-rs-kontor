-- Schema v99. A hosted leadership seat's authority is state of its occupancy
-- generation, not a value recomputed from live configuration.
--
-- ASMA-8193 gave leadership seats the same three-source resolution delivery
-- seats already had, but resolved it again on every later control operation.
-- `permission_posture` is a mutable plane default, so a deployment that flipped
-- it made `inspect` and `retire` compare a freshly computed value against a
-- native launched under the previous one. That mismatch refuses the retire
-- *before* the predecessor is archived, wedging the one governed path that
-- could install a correctly-routed successor.
--
-- Autonomy is therefore persisted beside the occupancy, exactly as `model_rung`
-- already is, and read back afterwards. A configuration change reaches a seat
-- only as a new generation through the audited retire/replace path.
--
-- Backfill. Every row written before this generation was launched by a build
-- whose hosted create passed a hardcoded `SeatAutonomy::Supervised`, and no
-- table in any prior schema recorded an autonomy at all -- so `supervised` is
-- not a guess, it is the only launch mode any existing row can have had.
--
-- Fail closed. The backfill is to the *least* authority the domain has, so a
-- row this migration cannot evidence is narrowed rather than widened; and the
-- CHECK below admits only the three spellings `SeatAutonomy` serializes, so a
-- row carrying anything else aborts the whole migration transaction instead of
-- being loaded under an authority no evidence supports.

CREATE TABLE hosted_topology_seats_v99 (
    seat_binding_id     TEXT NOT NULL PRIMARY KEY
                             REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    project_id          TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    model_rung          TEXT NOT NULL CHECK (json_valid(model_rung)),
    runtime_kind        TEXT NOT NULL,
    host                TEXT NOT NULL,
    generation          INTEGER NOT NULL CHECK (generation >= 0),
    native_id           TEXT NOT NULL,
    autonomy            TEXT NOT NULL CHECK (autonomy IN (
                             'supervised', 'bounded', 'advisory')),
    provider_session_id TEXT NULL,
    observed_at         TEXT NOT NULL,
    UNIQUE (runtime_kind, host, generation, native_id)
) STRICT;

INSERT INTO hosted_topology_seats_v99
    (seat_binding_id, project_id, model_rung, runtime_kind, host,
     generation, native_id, autonomy, provider_session_id, observed_at)
SELECT seat_binding_id, project_id, model_rung, runtime_kind, host,
       generation, native_id, 'supervised', provider_session_id, observed_at
FROM hosted_topology_seats;

-- Modern `ALTER TABLE ... RENAME` reparses the whole schema, so the rename is
-- only valid where every other object it re-reads already resolves. Both tables
-- here carry foreign keys, and the rebuild deliberately leaves them naming a
-- table that exists only side by side with its replacement for the space of two
-- statements. Legacy rename semantics do not reparse, and are correct for the
-- same reason migration 0094 needed them: this rename restores a name the rest
-- of the schema already spells, so there is nothing to rewrite.
PRAGMA legacy_alter_table = ON;
DROP TABLE hosted_topology_seats;
ALTER TABLE hosted_topology_seats_v99 RENAME TO hosted_topology_seats;
PRAGMA legacy_alter_table = OFF;

CREATE INDEX ix_hosted_topology_seats_project
    ON hosted_topology_seats(project_id, seat_binding_id);

-- History carries the same value, so a retired generation still says what it
-- was allowed to do. An audit that has to infer a predecessor's authority from
-- whatever configuration happens to be live has no evidence at all.
CREATE TABLE hosted_topology_seat_history_v99 (
    seat_binding_id     TEXT NOT NULL
                             REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    project_id          TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    generation          INTEGER NOT NULL CHECK (generation >= 0),
    model_rung          TEXT NOT NULL CHECK (json_valid(model_rung)),
    runtime_kind        TEXT NOT NULL,
    host                TEXT NOT NULL,
    native_id           TEXT NOT NULL,
    autonomy            TEXT NOT NULL CHECK (autonomy IN (
                             'supervised', 'bounded', 'advisory')),
    provider_session_id TEXT NULL,
    observed_at         TEXT NOT NULL,
    retired_at          TEXT NOT NULL,
    retirement_reason   TEXT NOT NULL CHECK (length(retirement_reason) BETWEEN 1 AND 512),
    PRIMARY KEY (project_id, seat_binding_id, native_id),
    UNIQUE (runtime_kind, host, generation, native_id)
) STRICT;

INSERT INTO hosted_topology_seat_history_v99
    (seat_binding_id, project_id, generation, model_rung, runtime_kind, host,
     native_id, autonomy, provider_session_id, observed_at, retired_at,
     retirement_reason)
SELECT seat_binding_id, project_id, generation, model_rung, runtime_kind, host,
       native_id, 'supervised', provider_session_id, observed_at, retired_at,
       retirement_reason
FROM hosted_topology_seat_history;

PRAGMA legacy_alter_table = ON;
DROP TABLE hosted_topology_seat_history;
ALTER TABLE hosted_topology_seat_history_v99 RENAME TO hosted_topology_seat_history;
PRAGMA legacy_alter_table = OFF;

PRAGMA user_version = 102;
