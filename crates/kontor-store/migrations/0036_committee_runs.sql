-- Durable Committee consultations: the run, its frozen slots, and the immutable
-- findings each slot records per round.
--
-- A Committee's verdict means "independent readers agreed". Everything that makes
-- that true is frozen here before the first native effect: the template revision,
-- the question, the resolved context, and the exact slot-to-seat assignment that
-- will answer. The conjunction is recomputed from these rows, never stored as a
-- decision somebody made.
--
-- Same row-first discipline as `advisor_runs`, for the same reason: two Committees
-- of one epic are both CSW nodes under the same ESW, so a search cannot tell them
-- apart, and a row written after its effects would leave a failure in between with
-- an orphaned workspace and unattached seats nothing can attribute.
CREATE TABLE committee_runs (
    id                          TEXT    NOT NULL PRIMARY KEY
                                        CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id                  TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id             TEXT    NOT NULL CHECK (length(mini_project_id) = 36),
    task_id                     TEXT    NULL CHECK (task_id IS NULL OR length(task_id) = 36),
    template_id                 TEXT    NOT NULL CHECK (length(template_id) = 36),
    template_version            INTEGER NOT NULL CHECK (template_version >= 1),
    template_hash               TEXT    NOT NULL
                                        CHECK (length(template_hash) = 64
                                               AND template_hash NOT GLOB '*[^0-9a-f]*'),
    question                    TEXT    NOT NULL CHECK (length(question) BETWEEN 1 AND 65536),
    question_hash               TEXT    NOT NULL
                                        CHECK (length(question_hash) = 64
                                               AND question_hash NOT GLOB '*[^0-9a-f]*'),
    -- The epic-owner authority the Committee was convened under. As with an
    -- Advisor consultation it does not identify the caller: the realm has one
    -- bearer secret per authority tier.
    owner_authority_seat_binding_id TEXT NOT NULL
                                        CHECK (length(owner_authority_seat_binding_id) = 36),
    context                     TEXT    NOT NULL CHECK (json_valid(context)),
    context_hash                TEXT    NOT NULL
                                        CHECK (length(context_hash) = 64
                                               AND context_hash NOT GLOB '*[^0-9a-f]*'),
    provenance                  TEXT    NOT NULL CHECK (json_valid(provenance)),
    topology_node_id            TEXT    NOT NULL CHECK (length(topology_node_id) = 36),
    esw_topology_node_id        TEXT    NOT NULL CHECK (length(esw_topology_node_id) = 36),
    esw_native_id               TEXT    NULL CHECK (esw_native_id IS NULL OR
                                                    length(esw_native_id) BETWEEN 1 AND 256),
    -- The round currently open. Round one decides; round two is the single
    -- authorized re-review. The template's own ceiling is checked against this.
    current_round               INTEGER NOT NULL CHECK (current_round BETWEEN 1 AND 2),
    -- `deliberating` while findings are awaited, `judging` once every required
    -- first finding is durable, `settled` once a round's outcome is frozen, and
    -- `needs_human` for a deliberation that cannot produce a typed aggregate.
    state                       TEXT    NOT NULL CHECK (state IN (
                                            'deliberating', 'judging', 'settled', 'needs_human')),
    intent_hash                 TEXT    NOT NULL
                                        CHECK (length(intent_hash) = 64
                                               AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    revision                    INTEGER NOT NULL CHECK (revision >= 1),
    created_at                  TEXT    NOT NULL
                                        CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    -- One invocation convenes one Committee.
    UNIQUE (project_id, intent_hash)
) STRICT;

CREATE INDEX ix_committee_runs_epic ON committee_runs (project_id, mini_project_id);

-- The frozen slot-to-seat assignment. One row per declared slot, written with the
-- run and never rewritten.
--
-- This is what makes a finding attributable: the ratified rule derives a
-- submitting slot from the seat its finding arrived through, and that mapping is
-- exactly this table. A slot whose seat is not bound and observed cannot submit,
-- so the round waits rather than accepting evidence nobody can attribute.
CREATE TABLE committee_slots (
    committee_run_id TEXT    NOT NULL REFERENCES committee_runs (id) ON DELETE RESTRICT,
    role_slot_id     TEXT    NOT NULL CHECK (length(role_slot_id) BETWEEN 1 AND 128),
    -- `reviewer` produces one independent finding per round; `judge` reads them
    -- and explains the outcome the rule already produced.
    slot_role        TEXT    NOT NULL CHECK (slot_role IN ('reviewer', 'judge')),
    seat_binding_id  TEXT    NOT NULL UNIQUE CHECK (length(seat_binding_id) = 36),
    role             TEXT    NOT NULL CHECK (json_valid(role)),
    -- The provider this slot's chain resolves to, frozen at invoke. Diversity is
    -- decided before any effect, and holding the resolved value is what lets a
    -- later reader prove the reviewers really were contrasting.
    provider         TEXT    NOT NULL CHECK (length(provider) BETWEEN 1 AND 128),
    created_at       TEXT    NOT NULL
                             CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (committee_run_id, role_slot_id)
) STRICT;

CREATE TRIGGER committee_slots_are_immutable
BEFORE UPDATE ON committee_slots
BEGIN SELECT RAISE(ABORT, 'a frozen Committee slot is immutable'); END;

CREATE TRIGGER committee_slots_are_permanent
BEFORE DELETE ON committee_slots
BEGIN SELECT RAISE(ABORT, 'a frozen Committee slot cannot be withdrawn'); END;

-- One immutable finding per slot per round, plus the Judge's aggregate.
--
-- Keyed by run, round and slot, so round two appends beside round one rather than
-- over it, and an exact resubmission replays while a different value for the same
-- key conflicts. `evidence_complete` stays in the denominator: a finding that
-- cited less than the template required counts against the gate rather than being
-- dropped from it.
CREATE TABLE committee_findings (
    committee_run_id  TEXT    NOT NULL REFERENCES committee_runs (id) ON DELETE RESTRICT,
    round             INTEGER NOT NULL CHECK (round BETWEEN 1 AND 2),
    role_slot_id      TEXT    NOT NULL CHECK (length(role_slot_id) BETWEEN 1 AND 128),
    verdict           TEXT    NOT NULL CHECK (verdict IN ('compliant', 'non_compliant')),
    evidence_complete INTEGER NOT NULL CHECK (evidence_complete IN (0, 1)),
    rationale         TEXT    NOT NULL CHECK (length(rationale) BETWEEN 1 AND 65536),
    -- References to already-authoritative evidence. A finding cites; it never
    -- uploads.
    evidence          TEXT    NOT NULL CHECK (json_valid(evidence)),
    -- Digest of the canonical finding, so an exact resubmission is recognisable as
    -- one rather than as a conflicting rewrite.
    finding_hash      TEXT    NOT NULL
                              CHECK (length(finding_hash) = 64
                                     AND finding_hash NOT GLOB '*[^0-9a-f]*'),
    created_at        TEXT    NOT NULL
                              CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (committee_run_id, round, role_slot_id)
) STRICT;

CREATE TRIGGER committee_findings_are_immutable
BEFORE UPDATE ON committee_findings
BEGIN SELECT RAISE(ABORT, 'a recorded finding is immutable'); END;

CREATE TRIGGER committee_findings_are_permanent
BEFORE DELETE ON committee_findings
BEGIN SELECT RAISE(ABORT, 'a recorded finding cannot be withdrawn'); END;

-- One settled outcome per round, recomputed by the server from the findings above.
--
-- The Judge explains this; it does not decide it. Storing the recomputed value is
-- not storing a decision: it is freezing what the rule produced from evidence that
-- is itself immutable, so a later reader gets the same answer without re-deriving
-- it, and a Judge cannot turn a failing conjunction into a passing one by writing
-- here.
CREATE TABLE committee_outcomes (
    committee_run_id TEXT    NOT NULL REFERENCES committee_runs (id) ON DELETE RESTRICT,
    round            INTEGER NOT NULL CHECK (round BETWEEN 1 AND 2),
    verdict          TEXT    NOT NULL CHECK (verdict IN ('compliant', 'non_compliant')),
    -- The Judge's bounded explanation, when the template froze a Judge slot.
    explanation      TEXT    NULL CHECK (explanation IS NULL OR
                                         length(explanation) BETWEEN 1 AND 65536),
    created_at       TEXT    NOT NULL
                             CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (committee_run_id, round)
) STRICT;

CREATE TRIGGER committee_outcomes_are_immutable
BEFORE UPDATE ON committee_outcomes
BEGIN SELECT RAISE(ABORT, 'a settled round is immutable'); END;

CREATE TRIGGER committee_outcomes_are_permanent
BEFORE DELETE ON committee_outcomes
BEGIN SELECT RAISE(ABORT, 'a settled round cannot be withdrawn'); END;

PRAGMA user_version = 36;
