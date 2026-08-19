-- One bounded Committee remediation between round one and round two.
--
-- The recommendation and tried path are evidence, not mutable run inputs. They
-- therefore live in their own append-only row and survive the round transition
-- without making the eventual terminal result rewritable.
CREATE TABLE committee_remediations (
    committee_run_id TEXT    NOT NULL PRIMARY KEY
                             REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    project_id       TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    from_round       INTEGER NOT NULL CHECK (from_round = 1),
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

PRAGMA user_version = 39;
