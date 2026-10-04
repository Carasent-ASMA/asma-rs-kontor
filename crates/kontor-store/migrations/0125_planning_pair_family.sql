-- Schema v125. A third, closed consultation family: `planning_pair`
-- (ASMA-8282, architecture disposition D-1 and D-2 of 2026-10-02).
--
-- A planning pair is advice, never a Committee: two read-only members, one
-- sealed findings round, at most one clarification and the caller's
-- disposition. It reuses the one consultation run registry, the shared
-- semantic identity and its unique index, seat bindings and command receipts.
-- Its protocol payload lives in run-keyed detail tables below, which are not a
-- second identity or run registry.
--
-- Forward-only. Every existing row, constraint, trigger and index is kept. The
-- family CHECKs widen to exactly the three named families, so an unknown family
-- is still refused. Advisor and Committee keep their exact state vocabulary and
-- settlement rule; a planning pair may only be materializing, running,
-- needs_human or disposed, never awaiting_judge or settled, and never carries a
-- result. `consultation_topic_corrections` keeps its two families: it records
-- only the legacy topic correction, and a planning pair is never legacy because
-- every invocation carries its semantic identity from the start.

-- 1. Published consultation documents. Same table, three families.
CREATE TABLE consultation_profile_revisions_v125 (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    family          TEXT    NOT NULL
                            CHECK (family IN ('advisor', 'committee', 'planning_pair')),
    profile_id      TEXT    NOT NULL
                            CHECK (length(profile_id) = 36 AND profile_id NOT GLOB '*[^0-9a-f-]*'),
    version         INTEGER NOT NULL CHECK (version >= 1),
    name            TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 512),
    definition      TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64
                                   AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, family, profile_id, version)
) STRICT;

INSERT INTO consultation_profile_revisions_v125
    (project_id, family, profile_id, version, name, definition, definition_hash, created_at)
SELECT project_id, family, profile_id, version, name, definition, definition_hash, created_at
FROM consultation_profile_revisions;

-- Legacy rename semantics: another table's trigger body names this table, and a
-- modern RENAME would reparse the schema while the name is momentarily absent.
PRAGMA legacy_alter_table = ON;
DROP TABLE consultation_profile_revisions;
ALTER TABLE consultation_profile_revisions_v125 RENAME TO consultation_profile_revisions;
PRAGMA legacy_alter_table = OFF;

CREATE TRIGGER consultation_profile_revisions_are_immutable
BEFORE UPDATE ON consultation_profile_revisions
BEGIN
    SELECT RAISE(ABORT, 'a published consultation profile revision is immutable');
END;

CREATE TRIGGER consultation_profile_revisions_are_permanent
BEFORE DELETE ON consultation_profile_revisions
BEGIN
    SELECT RAISE(ABORT, 'a published consultation profile revision cannot be withdrawn');
END;

-- 2. The one consultation run registry. The v70 columns, then the four columns
-- later generations added, in the order they were added.
CREATE TABLE consultation_runs_v125 (
    run_id                  TEXT    NOT NULL PRIMARY KEY
                                    CHECK (length(run_id) = 36
                                           AND run_id NOT GLOB '*[^0-9a-f-]*'),
    project_id              TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id         TEXT    NOT NULL REFERENCES mini_projects(id) ON DELETE RESTRICT,
    family                  TEXT    NOT NULL
                                    CHECK (family IN ('advisor', 'committee', 'planning_pair')),
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
    -- Family-conditioned. The first arm is exactly the v70 vocabulary, so an
    -- Advisor or Committee row can neither reach nor be stored in `disposed`.
    state                   TEXT    NOT NULL CHECK (
                                    (family IN ('advisor', 'committee')
                                     AND state IN ('materializing', 'running', 'awaiting_judge',
                                                   'settled', 'needs_human'))
                                    OR (family = 'planning_pair'
                                        AND state IN ('materializing', 'running', 'needs_human',
                                                      'disposed'))),
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
    topic                   TEXT    NULL
                                    CHECK (topic IS NULL OR length(topic) BETWEEN 1 AND 512),
    semantic_identity_hash  TEXT    NULL
                                    CHECK (semantic_identity_hash IS NULL
                                           OR (length(semantic_identity_hash) = 64
                                               AND semantic_identity_hash NOT GLOB '*[^0-9a-f]*')),
    subject_kind            TEXT    NULL CHECK (subject_kind IS NULL OR subject_kind IN ('epic', 'task')),
    subject_task_id         TEXT    NULL REFERENCES tasks(id) ON DELETE RESTRICT
                                    CHECK ((subject_kind IS NULL AND subject_task_id IS NULL)
                                           OR (subject_kind IS 'epic' AND subject_task_id IS NULL)
                                           OR (subject_kind IS 'task' AND subject_task_id IS NOT NULL)),
    FOREIGN KEY (project_id, family, profile_id, profile_version)
        REFERENCES consultation_profile_revisions(project_id, family, profile_id, version)
        ON DELETE RESTRICT,
    CHECK ((result IS NULL) = (result_hash IS NULL)),
    CHECK ((state = 'settled') = (settled_at IS NOT NULL)),
    -- A planning pair has no settlement and no result field: its decision is
    -- the caller's disposition, kept in its own record.
    CHECK (family <> 'planning_pair' OR (result IS NULL AND settled_at IS NULL)),
    -- And no legacy shortcut: every planning pair carries its semantic
    -- identity, topic and subject from invocation.
    CHECK (family <> 'planning_pair'
           OR (semantic_identity_hash IS NOT NULL AND topic IS NOT NULL
               AND subject_kind IS NOT NULL)),
    -- One findings round and one clarification are the protocol's own bounds,
    -- not the Committee round counter.
    CHECK (family <> 'planning_pair' OR round = 1),
    UNIQUE (project_id, run_id)
) STRICT;

INSERT INTO consultation_runs_v125
    (run_id, project_id, mini_project_id, family, profile_id, profile_version,
     definition_hash, question, question_hash, context, context_hash,
     caller_seat_binding_id, topology_node_id, invoke_key, invoke_intent_hash,
     state, round, result, result_hash, revision, created_at, updated_at, settled_at,
     topic, semantic_identity_hash, subject_kind, subject_task_id)
SELECT run_id, project_id, mini_project_id, family, profile_id, profile_version,
       definition_hash, question, question_hash, context, context_hash,
       caller_seat_binding_id, topology_node_id, invoke_key, invoke_intent_hash,
       state, round, result, result_hash, revision, created_at, updated_at, settled_at,
       topic, semantic_identity_hash, subject_kind, subject_task_id
FROM consultation_runs;

PRAGMA legacy_alter_table = ON;
DROP TABLE consultation_runs;
ALTER TABLE consultation_runs_v125 RENAME TO consultation_runs;
PRAGMA legacy_alter_table = OFF;

CREATE INDEX ix_consultation_runs_epic
    ON consultation_runs(project_id, mini_project_id, family, created_at, run_id);

CREATE UNIQUE INDEX consultation_runs_by_semantic_identity
    ON consultation_runs (project_id, semantic_identity_hash)
    WHERE semantic_identity_hash IS NOT NULL;

-- The v96 body, unchanged.
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
  OR (
      OLD.result IS NOT NULL
      AND NOT (
          OLD.state IS NEW.state
          AND OLD.round = NEW.round
          AND OLD.result IS NEW.result
          AND OLD.result_hash IS NEW.result_hash
          AND OLD.settled_at IS NEW.settled_at
          AND OLD.topic IS NOT NEW.topic
          AND OLD.semantic_identity_hash IS NULL
          AND NEW.semantic_identity_hash IS NOT NULL
          AND NEW.revision = OLD.revision + 1
          AND OLD.updated_at <> NEW.updated_at
          AND EXISTS (
              SELECT 1
                FROM command_receipts AS receipt
               WHERE receipt.project_id = OLD.project_id
                 AND receipt.kind = 'reconcile_native_names'
                 AND receipt.execution_mode = 'local'
                 AND receipt.state = 'intent_persisted'
                 AND json_extract(receipt.intent, '$.operation') =
                     'correct_consultation_topic'
                 AND json_extract(receipt.intent, '$.committee_run_id') = OLD.run_id
                 AND json_extract(receipt.intent, '$.expected_prior_topic') = OLD.topic
                 AND json_extract(receipt.intent, '$.corrected_topic') = NEW.topic
          )
      )
  )
  OR OLD.subject_kind IS NOT NEW.subject_kind
  OR OLD.subject_task_id IS NOT NEW.subject_task_id
BEGIN
    SELECT RAISE(ABORT, 'a consultation run cannot rewrite frozen input or settled evidence');
END;

-- The v96 bodies, unchanged.
CREATE TRIGGER consultation_subject_is_contained
BEFORE INSERT ON consultation_runs
WHEN NEW.subject_task_id IS NOT NULL
  AND NOT EXISTS (
      SELECT 1
        FROM tasks
       WHERE tasks.id = NEW.subject_task_id
         AND tasks.project_id = NEW.project_id
         AND tasks.mini_project_id IS NEW.mini_project_id
  )
BEGIN
    SELECT RAISE(ABORT,
        'a consultation subject must be a task of the same project and epic');
END;

CREATE TRIGGER consultation_subject_stays_contained
BEFORE UPDATE OF subject_task_id ON consultation_runs
WHEN NEW.subject_task_id IS NOT NULL
  AND NOT EXISTS (
      SELECT 1
        FROM tasks
       WHERE tasks.id = NEW.subject_task_id
         AND tasks.project_id = NEW.project_id
         AND tasks.mini_project_id IS NEW.mini_project_id
  )
BEGIN
    SELECT RAISE(ABORT,
        'a consultation subject must be a task of the same project and epic');
END;

-- A disposed planning pair is terminal: nothing about its run moves again.
CREATE TRIGGER planning_pair_disposed_is_terminal
BEFORE UPDATE ON consultation_runs
WHEN OLD.family = 'planning_pair' AND OLD.state = 'disposed'
BEGIN
    SELECT RAISE(ABORT, 'a disposed planning pair is terminal');
END;

-- 3. Protocol payload, keyed by run. Not an identity or a run registry.

-- The frozen placement: the shared allocator's receipt from one activated
-- snapshot, and its canonical hash, which every contribution is bound to.
CREATE TABLE planning_pair_placements (
    run_id          TEXT    NOT NULL PRIMARY KEY,
    project_id      TEXT    NOT NULL,
    protocol        TEXT    NOT NULL CHECK (protocol = 'planning_pair@1'),
    placement       TEXT    NOT NULL CHECK (json_valid(placement)),
    placement_hash  TEXT    NOT NULL
                            CHECK (length(placement_hash) = 64
                                   AND placement_hash NOT GLOB '*[^0-9a-f]*'),
    created_at      TEXT    NOT NULL
                            CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    FOREIGN KEY (project_id, run_id) REFERENCES consultation_runs(project_id, run_id)
        ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER planning_pair_placements_belong_to_a_planning_pair
BEFORE INSERT ON planning_pair_placements
WHEN NOT EXISTS (
    SELECT 1 FROM consultation_runs
     WHERE run_id = NEW.run_id AND project_id = NEW.project_id AND family = 'planning_pair'
)
BEGIN
    SELECT RAISE(ABORT, 'a planning pair placement belongs only to a planning_pair run');
END;

CREATE TRIGGER planning_pair_placements_are_immutable
BEFORE UPDATE ON planning_pair_placements
BEGIN
    SELECT RAISE(ABORT, 'a frozen planning pair placement is immutable');
END;

CREATE TRIGGER planning_pair_placements_are_permanent
BEFORE DELETE ON planning_pair_placements
BEGIN
    SELECT RAISE(ABORT, 'a frozen planning pair placement cannot be withdrawn');
END;

-- The canonical record, one immutable revision per accepted transition, in
-- lockstep with the run's own revision. Restore replays the latest through the
-- domain transitions; nothing here interprets it.
CREATE TABLE planning_pair_record_revisions (
    run_id      TEXT    NOT NULL,
    project_id  TEXT    NOT NULL,
    revision    INTEGER NOT NULL CHECK (revision >= 1),
    phase       TEXT    NOT NULL CHECK (phase IN (
                        'awaiting_findings', 'findings_released', 'awaiting_answers',
                        'answers_released', 'disposed')),
    record      TEXT    NOT NULL CHECK (json_valid(record)),
    record_hash TEXT    NOT NULL
                        CHECK (length(record_hash) = 64 AND record_hash NOT GLOB '*[^0-9a-f]*'),
    created_at  TEXT    NOT NULL
                        CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (run_id, revision),
    FOREIGN KEY (run_id) REFERENCES planning_pair_placements(run_id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, run_id) REFERENCES consultation_runs(project_id, run_id)
        ON DELETE RESTRICT
) STRICT;

-- A record revision is written only beside the run revision it describes, and
-- nothing follows a disposed record: the disposition is the last revision.
CREATE TRIGGER planning_pair_record_revisions_follow_their_run
BEFORE INSERT ON planning_pair_record_revisions
WHEN NOT EXISTS (
    SELECT 1 FROM consultation_runs
     WHERE run_id = NEW.run_id
       AND project_id = NEW.project_id
       AND family = 'planning_pair'
       AND revision = NEW.revision
)
  OR EXISTS (
    SELECT 1 FROM planning_pair_record_revisions
     WHERE run_id = NEW.run_id AND phase = 'disposed'
)
BEGIN
    SELECT RAISE(ABORT,
        'a planning pair record revision follows its run revision exactly and never a disposition');
END;

CREATE TRIGGER planning_pair_record_revisions_are_immutable
BEFORE UPDATE ON planning_pair_record_revisions
BEGIN
    SELECT RAISE(ABORT, 'a planning pair record revision is immutable');
END;

CREATE TRIGGER planning_pair_record_revisions_are_permanent
BEFORE DELETE ON planning_pair_record_revisions
BEGIN
    SELECT RAISE(ABORT, 'a planning pair record revision cannot be withdrawn');
END;

-- Each finding and answer, independently durable: one per round and slot, with
-- the exact authenticated member and occupancy generation that recorded it. The
-- advice itself is in the record; this row proves who gave it and that it was
-- never rewritten.
CREATE TABLE planning_pair_contributions (
    run_id               TEXT    NOT NULL,
    project_id           TEXT    NOT NULL,
    round                TEXT    NOT NULL CHECK (round IN ('findings', 'clarification')),
    slot                 TEXT    NOT NULL CHECK (slot IN ('seat-a', 'seat-b')),
    document_hash        TEXT    NOT NULL
                                 CHECK (length(document_hash) = 64
                                        AND document_hash NOT GLOB '*[^0-9a-f]*'),
    seat_binding_id      TEXT    NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    occupancy_generation INTEGER NOT NULL CHECK (occupancy_generation >= 1),
    record_revision      INTEGER NOT NULL CHECK (record_revision >= 1),
    created_at           TEXT    NOT NULL
                                 CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (run_id, round, slot),
    FOREIGN KEY (run_id, record_revision)
        REFERENCES planning_pair_record_revisions(run_id, revision) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, run_id) REFERENCES consultation_runs(project_id, run_id)
        ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER planning_pair_contributions_are_immutable
BEFORE UPDATE ON planning_pair_contributions
BEGIN
    SELECT RAISE(ABORT, 'a planning pair contribution is immutable');
END;

CREATE TRIGGER planning_pair_contributions_are_permanent
BEFORE DELETE ON planning_pair_contributions
BEGIN
    SELECT RAISE(ABORT, 'a planning pair contribution cannot be withdrawn');
END;

-- 4. The v108 receipt shape with exactly six additional kinds.
CREATE TABLE command_receipts_v125 (
    id TEXT NOT NULL PRIMARY KEY CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    idempotency_key TEXT NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    kind TEXT NOT NULL CHECK (kind IN (
        'launch_run','cancel_run','park_run','abandon_run','resume_task','record_gate_verdict',
        'approve_intake','sync_ticket','assign_ticket','transition_ticket','authorize_execution',
        'approve_schedule_override','revoke_schedule_override','resolve_status_conflict',
        'assign_work_calendar','revoke_execution_authorization','ensure_project',
        'ensure_account_profile','apply_epic_graph','import_backlog','transition_epic',
        'start_scheduled_work','transition_task','resolve_context','select_task_profile',
        'select_task_team','select_task_account','correct_task_worktree','reconcile_ticket',
        'materialize_jira','activate_asma_epic','settle_runtime','submit_intake',
        'pull_ticket_comments','claim_ticket','replace_seat','refresh_capacity',
        'override_availability','observe_seat','retire_seat','publish_topology_spec',
        'select_project_topology','upgrade_topology','retitle_container',
        'reconcile_native_names','apply_core_team','ensure_quick_session',
        'promote_quick_session','materialize_core_team','correct_core_team_route',
        'claim_core_team_seat','upgrade_epic_roster','apply_advisor_profile',
        'apply_committee_template','apply_completion_profile','advance_completion',
        'remediate_completion','invoke_advisor_run','settle_advisor_run','invoke_committee_run',
        'record_committee_findings','settle_committee_run','recover_consultation_seat',
        'reroute_unmaterialized_consultation_seat','publish_trigger','install_workflow_spec',
        'withdraw_task','publish_team_definition','select_project_team_definition',
        'upgrade_team_definition','correct_epic_backlog_code','recover_topology_container',
        'recover_gate_rejection','publish_ticket_description','publish_epic_description',
        'attest_retired_evaluator_evidence',
        'apply_planning_pair_profile','invoke_planning_pair_run','record_planning_pair_finding',
        'request_planning_pair_clarification','record_planning_pair_answer',
        'record_planning_pair_disposition')),
    target TEXT NOT NULL CHECK (json_valid(target)),
    target_revision INTEGER NOT NULL CHECK (target_revision >= 1),
    intent TEXT NOT NULL CHECK (json_valid(intent)),
    intent_hash TEXT NOT NULL CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    state TEXT NOT NULL CHECK (state IN ('intent_persisted','dispatch_pending','dispatched',
        'acknowledged','confirmation_unknown','confirmed','failed')),
    correlation TEXT NULL CHECK (correlation IS NULL OR length(correlation) BETWEEN 1 AND 256),
    native_identity TEXT NULL CHECK (native_identity IS NULL OR json_valid(native_identity)),
    result_ref TEXT NULL CHECK (result_ref IS NULL OR length(result_ref) BETWEEN 1 AND 256),
    attempts INTEGER NOT NULL CHECK (attempts >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    execution_mode TEXT NOT NULL DEFAULT 'dispatch' CHECK (execution_mode IN ('local','dispatch')),
    UNIQUE (project_id, id)
) STRICT;

INSERT INTO command_receipts_v125 SELECT * FROM command_receipts;
PRAGMA legacy_alter_table = ON;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v125 RENAME TO command_receipts;
PRAGMA legacy_alter_table = OFF;
CREATE INDEX ix_command_receipts_state ON command_receipts(project_id, state);
CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key OR OLD.target <> NEW.target
  OR OLD.intent <> NEW.intent OR OLD.intent_hash <> NEW.intent_hash
  OR OLD.kind <> NEW.kind OR OLD.project_id <> NEW.project_id
  OR OLD.state IN ('confirmed','failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;
CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

PRAGMA user_version = 125;
