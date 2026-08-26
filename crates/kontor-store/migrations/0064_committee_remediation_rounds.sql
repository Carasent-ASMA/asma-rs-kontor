-- Committee remediation is not a round transition. It freezes the failed
-- result on the source run; a governed re-review is a separate run. The
-- original v39 check incorrectly limited the source round to one, which made
-- later failed rounds unsafe and prevented compatibility reads of old rows.
ALTER TABLE committee_remediations RENAME TO committee_remediations_v39;

CREATE TABLE committee_remediations (
    committee_run_id TEXT    NOT NULL PRIMARY KEY
                             REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    project_id       TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    from_round       INTEGER NOT NULL CHECK (from_round BETWEEN 1 AND 2),
    recommendation   TEXT    NOT NULL CHECK (length(recommendation) BETWEEN 1 AND 32768),
    tried_path       TEXT    NOT NULL CHECK (length(tried_path) BETWEEN 1 AND 32768),
    document         TEXT    NOT NULL CHECK (json_valid(document)),
    document_hash    TEXT    NOT NULL
                             CHECK (length(document_hash) = 64
                                    AND document_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at      TEXT    NOT NULL,
    FOREIGN KEY (project_id, committee_run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT
) STRICT;

INSERT INTO committee_remediations
    (committee_run_id, project_id, from_round, recommendation, tried_path,
     document, document_hash, recorded_at)
SELECT committee_run_id, project_id, from_round, recommendation, tried_path,
       document, document_hash, recorded_at
FROM committee_remediations_v39;

DROP TABLE committee_remediations_v39;

CREATE TRIGGER committee_remediations_are_immutable
BEFORE UPDATE ON committee_remediations
BEGIN
    SELECT RAISE(ABORT, 'a Committee remediation is immutable');
END;

CREATE TRIGGER committee_remediations_are_permanent
BEFORE DELETE ON committee_remediations
BEGIN
    SELECT RAISE(ABORT, 'a Committee remediation cannot be withdrawn');
END;

PRAGMA user_version = 64;
