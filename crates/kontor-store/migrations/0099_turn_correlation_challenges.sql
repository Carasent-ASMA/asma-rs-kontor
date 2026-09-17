-- Schema v97. A server-owned correlation challenge for one already-bound
-- persistent seat whose historical native transcript omitted message identity.
--
-- Historical positions are deliberately absent. A challenge starts at a tail
-- boundary observed before dispatch, freezes an unpredictable server MessageId
-- and exact body, and may advance only after that body is found once after the
-- boundary. This cannot bless either of two indistinguishable old prompts.

CREATE TABLE turn_correlation_challenges (
    message_id            TEXT NOT NULL PRIMARY KEY
                               CHECK (length(message_id) = 36
                                      AND message_id NOT GLOB '*[^0-9a-f-]*'
                                      AND substr(message_id, 15, 1) = '7'),
    project_id            TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    task_id               TEXT NOT NULL,
    team_run_id           TEXT NOT NULL,
    agent_run_id          TEXT NOT NULL,
    role_slot_id          TEXT NOT NULL CHECK (length(role_slot_id) BETWEEN 1 AND 128),
    seat_binding_id       TEXT NOT NULL REFERENCES seat_bindings(id) ON DELETE RESTRICT,
    runtime_binding_id    TEXT NOT NULL,
    runtime_kind          TEXT NOT NULL CHECK (length(runtime_kind) BETWEEN 1 AND 128),
    runtime_host          TEXT NOT NULL CHECK (length(runtime_host) BETWEEN 1 AND 512),
    runtime_generation    INTEGER NOT NULL CHECK (runtime_generation >= 0),
    native_id             TEXT NOT NULL CHECK (length(native_id) BETWEEN 1 AND 256),
    task_revision         INTEGER NOT NULL CHECK (task_revision >= 1),
    agent_run_revision    INTEGER NOT NULL CHECK (agent_run_revision >= 1),
    artifact_key          TEXT NOT NULL CHECK (length(artifact_key) BETWEEN 1 AND 128),
    evidence_revision_id  TEXT NOT NULL,
    evidence_content_hash TEXT NOT NULL
                               CHECK (length(evidence_content_hash) = 64
                                      AND evidence_content_hash NOT GLOB '*[^0-9a-f]*'),
    report_checksum       TEXT NOT NULL
                               CHECK (length(report_checksum) = 64
                                      AND report_checksum NOT GLOB '*[^0-9a-f]*'),
    boundary_epoch        INTEGER NOT NULL CHECK (boundary_epoch >= 1),
    boundary_sequence     INTEGER NOT NULL CHECK (boundary_sequence >= 0),
    native_epoch          TEXT NOT NULL CHECK (length(native_epoch) BETWEEN 1 AND 256),
    body                  TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 65536),
    body_hash             TEXT NOT NULL
                               CHECK (length(body_hash) = 64
                                      AND body_hash NOT GLOB '*[^0-9a-f]*'),
    expected_response     TEXT NOT NULL CHECK (length(expected_response) BETWEEN 1 AND 65536),
    preview_hash          TEXT NOT NULL
                               CHECK (length(preview_hash) = 64
                                      AND preview_hash NOT GLOB '*[^0-9a-f]*'),
    request_hash          TEXT NOT NULL
                               CHECK (length(request_hash) = 64
                                      AND request_hash NOT GLOB '*[^0-9a-f]*'),
    idempotency_key       TEXT NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    state                 TEXT NOT NULL
                               CHECK (state IN ('prepared','dispatching','acknowledged','settled')),
    message_epoch         INTEGER NULL CHECK (message_epoch IS NULL OR message_epoch >= 1),
    message_sequence      INTEGER NULL CHECK (message_sequence IS NULL OR message_sequence >= 1),
    acknowledged_at       TEXT NULL,
    settled_turn_id       TEXT NULL REFERENCES role_turns(id) ON DELETE RESTRICT,
    settled_at            TEXT NULL,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL,
    UNIQUE (project_id, message_id),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_run_id) REFERENCES team_runs(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, agent_run_id) REFERENCES agent_runs(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, runtime_binding_id)
        REFERENCES runtime_bindings(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, evidence_revision_id)
        REFERENCES memory_revisions(project_id, id) ON DELETE RESTRICT,
    CHECK ((message_epoch IS NULL) = (message_sequence IS NULL)),
    CHECK ((message_epoch IS NULL) = (acknowledged_at IS NULL)),
    CHECK ((state IN ('prepared','dispatching')) = (message_epoch IS NULL)),
    CHECK ((state = 'settled') = (settled_turn_id IS NOT NULL)),
    CHECK ((state = 'settled') = (settled_at IS NOT NULL))
) STRICT;

CREATE INDEX ix_turn_correlation_challenges_run
    ON turn_correlation_challenges(project_id, agent_run_id, state);

-- A transport-uncertain first dispatch cannot be bypassed by presenting a new
-- idempotency key. The existing challenge must reconcile or settle before this
-- AgentRun can own another server-generated prompt.
CREATE UNIQUE INDEX uq_turn_correlation_challenges_active_run
    ON turn_correlation_challenges(project_id, agent_run_id)
    WHERE state <> 'settled';

CREATE TRIGGER turn_correlation_challenge_identity_is_frozen
BEFORE UPDATE ON turn_correlation_challenges
WHEN OLD.message_id <> NEW.message_id
  OR OLD.project_id <> NEW.project_id
  OR OLD.task_id <> NEW.task_id
  OR OLD.team_run_id <> NEW.team_run_id
  OR OLD.agent_run_id <> NEW.agent_run_id
  OR OLD.role_slot_id <> NEW.role_slot_id
  OR OLD.seat_binding_id <> NEW.seat_binding_id
  OR OLD.runtime_binding_id <> NEW.runtime_binding_id
  OR OLD.runtime_kind <> NEW.runtime_kind
  OR OLD.runtime_host <> NEW.runtime_host
  OR OLD.runtime_generation <> NEW.runtime_generation
  OR OLD.native_id <> NEW.native_id
  OR OLD.task_revision <> NEW.task_revision
  OR OLD.agent_run_revision <> NEW.agent_run_revision
  OR OLD.artifact_key <> NEW.artifact_key
  OR OLD.evidence_revision_id <> NEW.evidence_revision_id
  OR OLD.evidence_content_hash <> NEW.evidence_content_hash
  OR OLD.report_checksum <> NEW.report_checksum
  OR OLD.boundary_epoch <> NEW.boundary_epoch
  OR OLD.boundary_sequence <> NEW.boundary_sequence
  OR OLD.native_epoch <> NEW.native_epoch
  OR OLD.body <> NEW.body
  OR OLD.body_hash <> NEW.body_hash
  OR OLD.expected_response <> NEW.expected_response
  OR OLD.preview_hash <> NEW.preview_hash
  OR OLD.request_hash <> NEW.request_hash
  OR OLD.idempotency_key <> NEW.idempotency_key
  OR OLD.created_at <> NEW.created_at
BEGIN
    SELECT RAISE(ABORT, 'a turn correlation challenge cannot rewrite its identity or proof');
END;

CREATE TRIGGER turn_correlation_challenge_moves_forward
BEFORE UPDATE ON turn_correlation_challenges
WHEN NOT ((OLD.state = 'prepared' AND NEW.state = 'dispatching'
           AND NEW.message_epoch IS NULL)
       OR (OLD.state = 'dispatching' AND NEW.state = 'acknowledged'
           AND NEW.message_epoch IS NOT NULL
           AND NEW.settled_turn_id IS NULL)
       OR (OLD.state = 'acknowledged' AND NEW.state = 'settled'
           AND OLD.message_epoch = NEW.message_epoch
           AND OLD.message_sequence = NEW.message_sequence
           AND OLD.acknowledged_at = NEW.acknowledged_at))
BEGIN
    SELECT RAISE(ABORT, 'a turn correlation challenge only moves forward');
END;

CREATE TRIGGER turn_correlation_challenges_are_permanent
BEFORE DELETE ON turn_correlation_challenges
BEGIN
    SELECT RAISE(ABORT, 'a turn correlation challenge is never removed');
END;

-- If a role turn consumes a challenge MessageId, storage independently checks
-- the exact task/run/binding generation and the challenged artifact. The API's
-- runtime proof selects the response; this trigger makes consuming the same
-- server challenge a one-way atomic part of inserting that role turn.
CREATE TRIGGER challenged_role_turn_must_match
BEFORE INSERT ON role_turns
WHEN EXISTS (
    SELECT 1 FROM turn_correlation_challenges challenge
     WHERE challenge.project_id = NEW.project_id
       AND challenge.message_id = NEW.runtime_message_id
)
AND NOT EXISTS (
    SELECT 1 FROM turn_correlation_challenges challenge
     WHERE challenge.project_id = NEW.project_id
       AND challenge.message_id = NEW.runtime_message_id
       AND challenge.state = 'acknowledged'
       AND challenge.task_id = NEW.task_id
       AND challenge.task_revision = NEW.task_revision
       AND challenge.team_run_id = NEW.team_run_id
       AND challenge.agent_run_id = NEW.agent_run_id
       AND challenge.role_slot_id = NEW.role_slot_id
       AND 1 = (SELECT COUNT(*) FROM seat_bindings seat
                 WHERE seat.project_id = NEW.project_id
                   AND seat.task_id = NEW.task_id
                   AND seat.team_run_id = NEW.team_run_id
                   AND seat.role_slot_id = NEW.role_slot_id
                   AND seat.lifecycle = 'active')
       AND EXISTS (SELECT 1 FROM seat_bindings seat
                    WHERE seat.project_id = NEW.project_id
                      AND seat.id = challenge.seat_binding_id
                      AND seat.task_id = NEW.task_id
                      AND seat.team_run_id = NEW.team_run_id
                      AND seat.role_slot_id = NEW.role_slot_id
                      AND seat.lifecycle = 'active')
       AND challenge.runtime_generation = NEW.binding_generation
       AND challenge.message_epoch = NEW.message_timeline_epoch
       AND challenge.message_sequence = NEW.message_timeline_sequence
       AND EXISTS (SELECT 1 FROM json_each(NEW.artifacts)
                    WHERE value = challenge.artifact_key)
)
BEGIN
    SELECT RAISE(ABORT, 'a challenged role turn must consume its exact acknowledged challenge');
END;

CREATE TRIGGER challenged_role_turn_settles_challenge
AFTER INSERT ON role_turns
WHEN NEW.runtime_message_id IS NOT NULL
BEGIN
    UPDATE turn_correlation_challenges
       SET state = 'settled', settled_turn_id = NEW.id,
           settled_at = NEW.settled_at, updated_at = NEW.settled_at
     WHERE project_id = NEW.project_id
       AND message_id = NEW.runtime_message_id
       AND state = 'acknowledged';
END;

PRAGMA user_version = 99;
