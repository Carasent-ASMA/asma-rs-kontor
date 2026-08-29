-- A terminal needs_human completion may be explicitly recovered into a clean
-- third Committee run. Schema v36 froze both the run and finding round checks
-- to the original two-round policy, so the validated recovery reached storage
-- and was refused before any native effect. The completion state machine owns
-- the actual round policy; persistence accepts the full positive u8 range it
-- serializes and retains every immutable historical row byte-for-byte.

-- SQLite reparses every trigger while a referenced table is dropped. Preserve
-- the cross-table Advisor attestation guard explicitly across the rebuild.
DROP TRIGGER advisor_advice_belongs_to_its_attested_seat;

CREATE TABLE consultation_runs_v69 (
    run_id                  TEXT    NOT NULL PRIMARY KEY
                                    CHECK (length(run_id) = 36
                                           AND run_id NOT GLOB '*[^0-9a-f-]*'),
    project_id              TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id         TEXT    NOT NULL REFERENCES mini_projects(id) ON DELETE RESTRICT,
    family                  TEXT    NOT NULL CHECK (family IN ('advisor', 'committee')),
    profile_id              TEXT    NOT NULL,
    profile_version         INTEGER NOT NULL CHECK (profile_version >= 1),
    definition_hash         TEXT    NOT NULL
                                    CHECK (length(definition_hash) = 64
                                           AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    question                TEXT    NOT NULL CHECK (length(question) BETWEEN 1 AND 32768),
    question_hash           TEXT    NOT NULL
                                    CHECK (length(question_hash) = 64
                                           AND question_hash NOT GLOB '*[^0-9a-f]*'),
    context                 TEXT    NOT NULL CHECK (json_valid(context)),
    context_hash            TEXT    NOT NULL
                                    CHECK (length(context_hash) = 64
                                           AND context_hash NOT GLOB '*[^0-9a-f]*'),
    caller_seat_binding_id  TEXT    NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    topology_node_id        TEXT    NOT NULL UNIQUE REFERENCES topology_nodes(id) ON DELETE RESTRICT,
    invoke_key              TEXT    NOT NULL UNIQUE CHECK (length(invoke_key) BETWEEN 1 AND 256),
    invoke_intent_hash      TEXT    NOT NULL
                                    CHECK (length(invoke_intent_hash) = 64
                                           AND invoke_intent_hash NOT GLOB '*[^0-9a-f]*'),
    state                   TEXT    NOT NULL CHECK (state IN (
                                    'materializing', 'running', 'awaiting_judge',
                                    'settled', 'needs_human')),
    round                   INTEGER NOT NULL CHECK (round BETWEEN 1 AND 255),
    result                  TEXT    NULL CHECK (result IS NULL OR json_valid(result)),
    result_hash             TEXT    NULL
                                    CHECK (result_hash IS NULL OR
                                           (length(result_hash) = 64
                                            AND result_hash NOT GLOB '*[^0-9a-f]*')),
    revision                INTEGER NOT NULL CHECK (revision >= 1),
    created_at              TEXT    NOT NULL,
    updated_at              TEXT    NOT NULL,
    settled_at              TEXT    NULL,
    FOREIGN KEY (project_id, family, profile_id, profile_version)
        REFERENCES consultation_profile_revisions(project_id, family, profile_id, version)
        ON DELETE RESTRICT,
    CHECK ((result IS NULL) = (result_hash IS NULL)),
    CHECK ((state = 'settled') = (settled_at IS NOT NULL)),
    UNIQUE (project_id, run_id)
) STRICT;

INSERT INTO consultation_runs_v69
    (run_id, project_id, mini_project_id, family, profile_id, profile_version,
     definition_hash, question, question_hash, context, context_hash,
     caller_seat_binding_id, topology_node_id, invoke_key, invoke_intent_hash,
     state, round, result, result_hash, revision, created_at, updated_at, settled_at)
SELECT run_id, project_id, mini_project_id, family, profile_id, profile_version,
       definition_hash, question, question_hash, context, context_hash,
       caller_seat_binding_id, topology_node_id, invoke_key, invoke_intent_hash,
       state, round, result, result_hash, revision, created_at, updated_at, settled_at
FROM consultation_runs;

DROP TABLE consultation_runs;
ALTER TABLE consultation_runs_v69 RENAME TO consultation_runs;

CREATE INDEX ix_consultation_runs_epic
    ON consultation_runs(project_id, mini_project_id, family, created_at, run_id);

CREATE TRIGGER consultation_run_inputs_are_frozen
BEFORE UPDATE ON consultation_runs
WHEN OLD.project_id <> NEW.project_id
  OR OLD.mini_project_id <> NEW.mini_project_id
  OR OLD.family <> NEW.family
  OR OLD.profile_id <> NEW.profile_id
  OR OLD.profile_version <> NEW.profile_version
  OR OLD.definition_hash <> NEW.definition_hash
  OR OLD.question <> NEW.question
  OR OLD.question_hash <> NEW.question_hash
  OR OLD.context <> NEW.context
  OR OLD.context_hash <> NEW.context_hash
  OR OLD.caller_seat_binding_id <> NEW.caller_seat_binding_id
  OR OLD.topology_node_id <> NEW.topology_node_id
  OR OLD.invoke_key <> NEW.invoke_key
  OR OLD.invoke_intent_hash <> NEW.invoke_intent_hash
  OR OLD.created_at <> NEW.created_at
  OR OLD.result IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a consultation run cannot rewrite frozen input or settled evidence');
END;

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

CREATE TABLE committee_findings_v69 (
    committee_run_id        TEXT    NOT NULL REFERENCES consultation_runs(run_id) ON DELETE RESTRICT,
    project_id              TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    round                   INTEGER NOT NULL CHECK (round BETWEEN 1 AND 255),
    role_slot_id            TEXT    NOT NULL,
    role                    TEXT    NOT NULL CHECK (role IN ('reviewer', 'judge')),
    verdict                 TEXT    NOT NULL CHECK (verdict IN ('compliant', 'non_compliant')),
    evidence_complete       INTEGER NOT NULL CHECK (evidence_complete IN (0, 1)),
    document                TEXT    NOT NULL CHECK (json_valid(document)),
    document_hash           TEXT    NOT NULL
                                    CHECK (length(document_hash) = 64
                                           AND document_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at             TEXT    NOT NULL,
    PRIMARY KEY (committee_run_id, round, role_slot_id),
    FOREIGN KEY (project_id, committee_run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT,
    FOREIGN KEY (committee_run_id, role_slot_id)
        REFERENCES consultation_seats(run_id, role_slot_id) ON DELETE RESTRICT
) STRICT;

INSERT INTO committee_findings_v69
    (committee_run_id, project_id, round, role_slot_id, role, verdict,
     evidence_complete, document, document_hash, recorded_at)
SELECT committee_run_id, project_id, round, role_slot_id, role, verdict,
       evidence_complete, document, document_hash, recorded_at
FROM committee_findings;

DROP TABLE committee_findings;
ALTER TABLE committee_findings_v69 RENAME TO committee_findings;

CREATE TRIGGER committee_findings_are_immutable
BEFORE UPDATE ON committee_findings
BEGIN
    SELECT RAISE(ABORT, 'a Committee finding is immutable');
END;

CREATE TRIGGER committee_findings_are_permanent
BEFORE DELETE ON committee_findings
BEGIN
    SELECT RAISE(ABORT, 'a Committee finding cannot be withdrawn');
END;

PRAGMA user_version = 69;
