-- Split Advisor evidence from the requester's disposition.
--
-- The Advisor seat is the only authority that may append its output. The
-- requester or owning LSA can disposition that already-frozen artifact later,
-- through the same public settlement route, without being able to author or
-- rewrite the advice itself.
CREATE TABLE advisor_advice_artifacts (
    advisor_run_id    TEXT NOT NULL PRIMARY KEY
                           REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    project_id        TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    seat_binding_id   TEXT NOT NULL UNIQUE
                           REFERENCES consultation_seats(seat_binding_id) ON DELETE RESTRICT,
    document          TEXT NOT NULL CHECK (json_valid(document)),
    document_hash     TEXT NOT NULL
                           CHECK (length(document_hash) = 64
                                  AND document_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at       TEXT NOT NULL,
    FOREIGN KEY (project_id, advisor_run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT
) STRICT;

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

PRAGMA user_version = 40;
