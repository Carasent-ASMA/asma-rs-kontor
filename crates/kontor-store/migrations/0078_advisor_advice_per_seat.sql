-- Advisor advice is keyed by the seat that gave it.
--
-- Split from v77 deliberately: this is one self-contained correction to one
-- table, so it can be applied to a seeded pre-v78 fixture and proved to carry
-- every existing row across unchanged.

-- One Advisor Session Workspace holds one *or more* independently reporting
-- advisor seats, so advice is keyed by the seat that gave it.
--
-- v40 made `advisor_run_id` the primary key, which encoded "one advisor, one
-- answer" into the schema. That is the assumption the approved contract
-- retires: an ASW represents one advised subject and may contain several seats,
-- each reporting on its own. The composite key is therefore the correction of a
-- cardinality mistake, not a new feature.
--
-- Every existing row is carried over unchanged. A single-seat run keeps exactly
-- the artifact it had, under the same identity and bytes, and the immutability
-- and permanence guards are recreated with the table: advice that was given
-- cannot be edited or withdrawn, before or after this generation.
CREATE TABLE advisor_advice_artifacts_v78 (
    advisor_run_id  TEXT NOT NULL
                         REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    -- Still globally unique: a seat belongs to exactly one run and advises
    -- exactly once, so one seat can never appear under two runs or twice here.
    seat_binding_id TEXT NOT NULL UNIQUE
                         REFERENCES consultation_seats(seat_binding_id) ON DELETE RESTRICT,
    document        TEXT NOT NULL CHECK (json_valid(document)),
    document_hash   TEXT NOT NULL
                         CHECK (length(document_hash) = 64
                                AND document_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at     TEXT NOT NULL,
    PRIMARY KEY (advisor_run_id, seat_binding_id),
    FOREIGN KEY (project_id, advisor_run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT
) STRICT;

INSERT INTO advisor_advice_artifacts_v78
    (advisor_run_id, project_id, seat_binding_id, document, document_hash, recorded_at)
SELECT advisor_run_id, project_id, seat_binding_id, document, document_hash, recorded_at
FROM advisor_advice_artifacts;

DROP TABLE advisor_advice_artifacts;
ALTER TABLE advisor_advice_artifacts_v78 RENAME TO advisor_advice_artifacts;

CREATE TRIGGER advisor_advice_belongs_to_its_attested_seat
BEFORE INSERT ON advisor_advice_artifacts
WHEN NOT EXISTS (
    SELECT 1
      FROM consultation_runs AS run
      JOIN consultation_seats AS seat ON seat.run_id = run.run_id
     WHERE run.project_id = NEW.project_id
       AND run.run_id = NEW.advisor_run_id
       AND run.family = 'advisor'
       AND seat.seat_binding_id = NEW.seat_binding_id
       AND seat.native_id IS NOT NULL
)
BEGIN
    SELECT RAISE(ABORT, 'Advisor advice requires its own attested seat');
END;

CREATE TRIGGER advisor_advice_is_immutable
BEFORE UPDATE ON advisor_advice_artifacts
BEGIN
    SELECT RAISE(ABORT, 'Advisor advice is immutable');
END;

CREATE TRIGGER advisor_advice_is_permanent
BEFORE DELETE ON advisor_advice_artifacts
BEGIN
    SELECT RAISE(ABORT, 'Advisor advice cannot be withdrawn');
END;

PRAGMA user_version = 78;
