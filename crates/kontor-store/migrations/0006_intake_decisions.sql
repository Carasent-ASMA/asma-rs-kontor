-- ===========================================================================
-- Schema v6 — terminal intake decisions and created-work lineage (KON-MVP-22)
--
-- Schema v1 already stores what intake *observed* (`source_events`) and what it
-- *decided* (`intake_receipts`), and both tables are immutable. That is exactly
-- why approval cannot live in them: a proposal that later becomes approved
-- would have to be rewritten, and rewriting the evidence is how the record of
-- what was proposed — and of who was asked — disappears. So the terminal half
-- is appended beside it, in two tables.
--
--  1. **`intake_decisions` is the terminal state of one proposal.** Approval,
--     rejection and bounded auto-arm are the same shape of fact — an actor, a
--     command receipt, an instant, and for a rejection a reason — so they are
--     one table with a closed `outcome`, not three. `UNIQUE (project_id,
--     intake_receipt_id)` is the whole concurrency rule: a proposal reaches a
--     terminal state exactly once, whatever raced to give it one, so two
--     concurrent approvals cannot produce two work graphs.
--
--  2. **`intake_created_work` is lineage, one row per created task.** It
--     carries what the scheduler is allowed to know about provenance — the
--     receipt, the source event and its digest, the pinned trigger revision,
--     whether an operator approved or the trigger armed, and the execution
--     authorization a bounded auto-arm acted under. `UNIQUE (project_id,
--     task_id)` says a task has at most one originating receipt: a replayed
--     decision cannot attach a second graph, and a task cannot claim two
--     provenances.
--
-- Neither table is a `command_receipts` row. A command receipt is dispatchable
-- — it has an outbox, an attempt count and a state machine that ends in a
-- runtime effect — and an intake decision has already happened by the time it
-- is recorded. It *references* the command receipt that authorized it instead,
-- which is what makes the authority checkable rather than asserted.
--
-- Every reference is a real foreign key, both tables are append-only through
-- BEFORE UPDATE/DELETE triggers, and the work a decision created is reachable
-- from the receipt and from the task with one index each.
-- ===========================================================================

-- The terminal decision about one proposed intake receipt.
CREATE TABLE intake_decisions (
    id                 TEXT    NOT NULL PRIMARY KEY
                               CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id         TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    -- The proposal. Never mutated by this row's existence.
    intake_receipt_id  TEXT    NOT NULL,
    outcome            TEXT    NOT NULL CHECK (outcome IN
                               ('approved', 'rejected', 'auto_armed')),
    -- Who acted. For a bounded auto-arm this is the account whose capability
    -- was exercised, which is a profile in this project like any other actor.
    actor              TEXT    NOT NULL,
    -- The receipt that recorded the command. Authority is receipt-backed or it
    -- is not authority.
    command_receipt_id TEXT    NOT NULL,
    -- Why, on a rejection. Present exactly when the outcome is a rejection: a
    -- reason on an approval would be prose nobody reads, and its absence on a
    -- rejection is the one case where it is evidence.
    reason             TEXT    NULL CHECK (reason IS NULL OR length(reason) BETWEEN 1 AND 512),
    -- The bounded auto-arm capability, present exactly on `auto_armed`.
    capability_granted_to        TEXT NULL,
    capability_execution_auth_id TEXT NULL,
    decided_at         TEXT    NOT NULL
                               CHECK (decided_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    -- One terminal decision per proposal. This is the constraint two racing
    -- approvals collide on, and therefore the reason exactly one graph exists.
    UNIQUE (project_id, intake_receipt_id),
    CHECK ((outcome = 'rejected') = (reason IS NOT NULL)),
    CHECK ((outcome = 'auto_armed') = (capability_granted_to IS NOT NULL)),
    CHECK ((capability_granted_to IS NULL) = (capability_execution_auth_id IS NULL)),
    FOREIGN KEY (project_id, intake_receipt_id)
        REFERENCES intake_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, actor)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, command_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, capability_granted_to)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, capability_execution_auth_id)
        REFERENCES execution_authorizations (project_id, id) ON DELETE RESTRICT
) STRICT;

-- Pending intake: the proposals nobody has decided yet, read by receipt.
CREATE INDEX ix_intake_decisions_receipt
    ON intake_decisions (project_id, intake_receipt_id, outcome);

-- One task created by one intake decision, with the authority behind it.
CREATE TABLE intake_created_work (
    project_id            TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id               TEXT    NOT NULL,
    intake_receipt_id     TEXT    NOT NULL,
    intake_decision_id    TEXT    NOT NULL,
    mini_project_id       TEXT    NULL,
    source_event_id       TEXT    NOT NULL,
    -- The digest the decision cited, kept beside the reference: a lineage that
    -- named an event without pinning its bytes would still resolve after the
    -- event's meaning changed underneath it.
    source_event_hash     TEXT    NOT NULL
                                  CHECK (length(source_event_hash) = 64
                                         AND source_event_hash NOT GLOB '*[^0-9a-f]*'),
    trigger_key           TEXT    NOT NULL CHECK (length(trigger_key) BETWEEN 1 AND 128),
    trigger_version       INTEGER NOT NULL CHECK (trigger_version >= 1),
    -- Approval or bounded auto-arm: which of the two armed this task is the
    -- difference between a human's authority and a policy's, and the scheduler
    -- admits the second one only with an authorization.
    authority             TEXT    NOT NULL CHECK (authority IN ('approved', 'auto_armed')),
    execution_auth_id     TEXT    NULL,
    created_at            TEXT    NOT NULL
                                  CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- One originating receipt per created task. A replay cannot attach a second
    -- graph, and a task cannot be claimed by two decisions.
    PRIMARY KEY (project_id, task_id),
    -- A bounded auto-arm always names its authorization; an approval never does.
    CHECK ((authority = 'auto_armed') = (execution_auth_id IS NOT NULL)),
    FOREIGN KEY (project_id, task_id)
        REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, intake_receipt_id)
        REFERENCES intake_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, intake_decision_id)
        REFERENCES intake_decisions (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, source_event_id)
        REFERENCES source_events (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, trigger_key, trigger_version)
        REFERENCES trigger_specs (project_id, trigger_key, version) ON DELETE RESTRICT
) STRICT;

-- The receipt-to-work direction: what did this decision create?
CREATE INDEX ix_intake_created_work_receipt
    ON intake_created_work (project_id, intake_receipt_id);

-- The source-event history direction: what has this event caused, ever?
CREATE INDEX ix_intake_created_work_event
    ON intake_created_work (project_id, source_event_id, created_at);

CREATE TRIGGER intake_decisions_no_update BEFORE UPDATE ON intake_decisions
BEGIN SELECT RAISE(ABORT, 'an intake decision is immutable'); END;

CREATE TRIGGER intake_decisions_no_delete BEFORE DELETE ON intake_decisions
BEGIN SELECT RAISE(ABORT, 'an intake decision is not deletable'); END;

CREATE TRIGGER intake_created_work_no_update BEFORE UPDATE ON intake_created_work
BEGIN SELECT RAISE(ABORT, 'intake work lineage is immutable'); END;

CREATE TRIGGER intake_created_work_no_delete BEFORE DELETE ON intake_created_work
BEGIN SELECT RAISE(ABORT, 'intake work lineage is not deletable'); END;

PRAGMA user_version = 6;
