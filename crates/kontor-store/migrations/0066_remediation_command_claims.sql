-- Schema v64. A remediation effect and its command receipt share one durable
-- replay authority. The claim is deliberately a separate immutable row: it
-- lets a retry prove the exact key and intent at a fault boundary without
-- treating the mutable completion projection as an idempotency ledger.
CREATE TABLE epic_completion_remediation_command_claims (
    project_id      TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id TEXT    NOT NULL,
    round           INTEGER NOT NULL CHECK (round BETWEEN 1 AND 2),
    action          TEXT    NOT NULL CHECK (action IN ('lsa_proposal', 'tpm_route')),
    idempotency_key TEXT    NOT NULL UNIQUE
                            CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    intent_hash     TEXT    NOT NULL
                            CHECK (length(intent_hash) = 64
                                   AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    effect_revision INTEGER NULL CHECK (effect_revision IS NULL OR effect_revision >= 1),
    claimed_at      TEXT    NOT NULL,
    PRIMARY KEY (project_id, mini_project_id, round, action),
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects(project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER epic_completion_remediation_command_claims_are_immutable
BEFORE UPDATE ON epic_completion_remediation_command_claims
BEGIN
    SELECT RAISE(ABORT, 'a completion remediation command claim is immutable');
END;

CREATE TRIGGER epic_completion_remediation_command_claims_are_permanent
BEFORE DELETE ON epic_completion_remediation_command_claims
BEGIN
    SELECT RAISE(ABORT, 'a completion remediation command claim cannot be withdrawn');
END;

-- A clean re-review is unique by the canonical provenance bytes, not by its
-- caller-chosen invoke key. This row is inserted inside create_consultation_run's
-- existing run/topology/seat transaction, so a concurrent loser rolls all of
-- its placement state back before any native effect is attempted.
CREATE TABLE committee_re_review_claims (
    project_id       TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id  TEXT NOT NULL,
    provenance       TEXT NOT NULL CHECK (json_valid(provenance)),
    provenance_hash  TEXT NOT NULL
                          CHECK (length(provenance_hash) = 64
                                 AND provenance_hash NOT GLOB '*[^0-9a-f]*'),
    committee_run_id TEXT NOT NULL UNIQUE,
    claimed_at       TEXT NOT NULL,
    PRIMARY KEY (project_id, mini_project_id, provenance_hash),
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, committee_run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER committee_re_review_claims_are_immutable
BEFORE UPDATE ON committee_re_review_claims
BEGIN
    SELECT RAISE(ABORT, 'a Committee re-review provenance claim is immutable');
END;

CREATE TRIGGER committee_re_review_claims_are_permanent
BEFORE DELETE ON committee_re_review_claims
BEGIN
    SELECT RAISE(ABORT, 'a Committee re-review provenance claim cannot be withdrawn');
END;

PRAGMA user_version = 66;
