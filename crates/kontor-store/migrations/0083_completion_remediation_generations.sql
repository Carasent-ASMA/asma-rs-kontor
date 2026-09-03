-- Completion round numbers restart when ticket work reopens. Preserve the
-- immutable era-one rows and include the completion generation in both replay
-- identities so a later era cannot collide with, or borrow authority from, an
-- earlier one.
CREATE TABLE epic_completion_remediation_proposals_v83 (
    project_id               TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id          TEXT    NOT NULL CHECK (length(mini_project_id) = 36),
    completion_generation    INTEGER NOT NULL DEFAULT 1 CHECK (completion_generation >= 1),
    round                    INTEGER NOT NULL CHECK (round >= 1),
    failed_round_evidence    TEXT    NOT NULL CHECK (length(failed_round_evidence) = 64 AND failed_round_evidence NOT GLOB '*[^0-9a-f]*'),
    proposal                 TEXT    NOT NULL CHECK (length(proposal) = 64 AND proposal NOT GLOB '*[^0-9a-f]*'),
    lsa_seat_binding_id      TEXT    NOT NULL CHECK (length(lsa_seat_binding_id) = 36),
    proposed_at              TEXT    NOT NULL CHECK (proposed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    lsa_occupancy_generation INTEGER NOT NULL DEFAULT 1 CHECK (lsa_occupancy_generation >= 1),
    PRIMARY KEY (project_id, mini_project_id, completion_generation, round)
) STRICT;

INSERT INTO epic_completion_remediation_proposals_v83
    (project_id, mini_project_id, completion_generation, round,
     failed_round_evidence, proposal, lsa_seat_binding_id, proposed_at,
     lsa_occupancy_generation)
SELECT project_id, mini_project_id, 1, round, failed_round_evidence, proposal,
       lsa_seat_binding_id, proposed_at, lsa_occupancy_generation
FROM epic_completion_remediation_proposals;

DROP TABLE epic_completion_remediation_proposals;
ALTER TABLE epic_completion_remediation_proposals_v83
    RENAME TO epic_completion_remediation_proposals;

CREATE TABLE epic_completion_remediation_command_claims_v83 (
    project_id            TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id       TEXT    NOT NULL,
    completion_generation INTEGER NOT NULL DEFAULT 1 CHECK (completion_generation >= 1),
    round                 INTEGER NOT NULL CHECK (round >= 1),
    action                TEXT    NOT NULL CHECK (action IN ('lsa_proposal', 'tpm_route')),
    idempotency_key       TEXT    NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    intent_hash           TEXT    NOT NULL CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    effect_revision       INTEGER NULL CHECK (effect_revision IS NULL OR effect_revision >= 1),
    claimed_at            TEXT    NOT NULL,
    PRIMARY KEY (project_id, mini_project_id, completion_generation, round, action),
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects(project_id, id) ON DELETE RESTRICT
) STRICT;

INSERT INTO epic_completion_remediation_command_claims_v83
    (project_id, mini_project_id, completion_generation, round, action,
     idempotency_key, intent_hash, effect_revision, claimed_at)
SELECT project_id, mini_project_id, 1, round, action, idempotency_key,
       intent_hash, effect_revision, claimed_at
FROM epic_completion_remediation_command_claims;

DROP TABLE epic_completion_remediation_command_claims;
ALTER TABLE epic_completion_remediation_command_claims_v83
    RENAME TO epic_completion_remediation_command_claims;

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

PRAGMA user_version = 83;
