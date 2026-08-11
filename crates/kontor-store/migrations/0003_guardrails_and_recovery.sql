-- ===========================================================================
-- Schema v3 — guardrail evidence and bounded parked recovery (KON-MVP-10)
--
-- Six append-only tables and three added columns. Every rule schema v1 states
-- about evidence holds here unchanged: STRICT tables, composite project-scoped
-- foreign keys, canonical JSON with its digest beside it, and BEFORE
-- UPDATE/DELETE triggers so direct SQL cannot rewrite what happened.
--
-- Three decisions worth stating, because each one had a tempting alternative.
--
--  1. **`guardrail_evaluations` is untouched.** v1's table is a run-scoped
--     pass/warn/block record with a trust rung, and things already read it.
--     Widening its verdict domain and bolting a subject onto it would change
--     what every existing row means. `policy_evaluations` is therefore a new
--     table with the complete KON-MVP-10 contract, and the old one keeps its
--     historical contract exactly.
--
--  2. **There is no rejection counter.** A counter column is a second source of
--     truth for something `task_gate_evaluations` already records completely,
--     and every bug in this area is a counter that drifted, missed an increment
--     or was reset by the wrong event. `rejections_since_pass` is derived from
--     the append-only history on every read. The three columns added below
--     exist to make that derivation *possible* — a stable reviewer principal to
--     key on — not to cache its result.
--
--  3. **An approval is one action's approval.** `approval_receipts` is unique
--     on the canonical action digest, scoped to a project and optionally to a
--     task, and expires. There is no shape here that expresses "this actor may
--     delete things"; the only representable approval names one exact command.
-- ===========================================================================

-- ---------------------------------------------------------------------------
-- 1. Guardrail evaluations
-- ---------------------------------------------------------------------------

-- One immutable evaluation of one rule against one subject.
--
-- `inputs` is the canonical evaluation request the verdict was reached on, so a
-- reviewer can re-run the rule against the stored bytes and get the same answer
-- or find out that they cannot. That is the whole reason the inputs are stored
-- rather than summarized: a summary is not re-checkable.
CREATE TABLE policy_evaluations (
    id            TEXT    NOT NULL PRIMARY KEY
                          CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id    TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id       TEXT    NOT NULL,
    workflow_id   TEXT    NOT NULL,
    -- Nullable because an evaluation can precede any run at all: refusing a
    -- launch is exactly the case where there is no run to name yet.
    team_run_id   TEXT    NULL,
    agent_run_id  TEXT    NULL,
    rule_key      TEXT    NOT NULL CHECK (rule_key IN (
                              'worktree_sticky', 'module_collision', 'second_rejection_parks',
                              'degraded_verdict_denied', 'destructive_requires_approval',
                              'account_pin_required', 'terminal_evidence_required')),
    rule_version  INTEGER NOT NULL CHECK (rule_version >= 1),
    subject_kind  TEXT    NOT NULL CHECK (subject_kind IN (
                              'task', 'task_workflow', 'team_run', 'agent_run', 'gate', 'action')),
    subject_id    TEXT    NOT NULL CHECK (length(subject_id) BETWEEN 1 AND 256
                                          AND subject_id NOT GLOB '* *'),
    inputs        TEXT    NOT NULL CHECK (json_valid(inputs)),
    inputs_hash   TEXT    NOT NULL
                          CHECK (length(inputs_hash) = 64 AND inputs_hash NOT GLOB '*[^0-9a-f]*'),
    verdict       TEXT    NOT NULL CHECK (verdict IN
                              ('pass', 'warn', 'block', 'park', 'needs_human')),
    -- The Rust `ReasonCode` is the closed domain; SQL enforces the lexical shape
    -- rather than repeating ~35 spellings that a later rule revision would have
    -- to migrate. An unknown code is refused on the way back in by `parse`.
    reason_code   TEXT    NOT NULL CHECK (length(reason_code) BETWEEN 1 AND 128
                                          AND reason_code NOT GLOB '*[^a-z0-9_]*'),
    evidence_refs TEXT    NOT NULL CHECK (json_valid(evidence_refs)),
    recorded_at   TEXT    NOT NULL
                          CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, workflow_id)
        REFERENCES task_workflows (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_run_id) REFERENCES team_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_policy_evaluations_subject
    ON policy_evaluations (project_id, rule_key, subject_kind, subject_id);

-- ---------------------------------------------------------------------------
-- 2. Artifact evidence
-- ---------------------------------------------------------------------------

-- What was produced, addressed by reference.
--
-- `locator` says *where* the artifact is; it is never the artifact. No
-- transcript, no diff body and no credential belongs in this table, and
-- `CanonicalDocument` refuses the last of those before the row is built.
CREATE TABLE artifact_evidence (
    id               TEXT NOT NULL PRIMARY KEY
                          CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id       TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id          TEXT NOT NULL,
    workflow_id      TEXT NOT NULL,
    agent_run_id     TEXT NULL,
    -- The artifact contract key the pinned profile declares. Open data: this
    -- schema never interprets it.
    artifact_key     TEXT NOT NULL CHECK (length(artifact_key) BETWEEN 1 AND 128),
    locator          TEXT NOT NULL CHECK (json_valid(locator)),
    locator_hash     TEXT NOT NULL
                          CHECK (length(locator_hash) = 64 AND locator_hash NOT GLOB '*[^0-9a-f]*'),
    producer_role    TEXT NOT NULL CHECK (length(producer_role) BETWEEN 1 AND 128),
    producer_account TEXT NOT NULL,
    recorded_at      TEXT NOT NULL
                          CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, workflow_id)
        REFERENCES task_workflows (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, producer_account)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_artifact_evidence_workflow
    ON artifact_evidence (project_id, workflow_id, artifact_key);

-- ---------------------------------------------------------------------------
-- 3. Gate waivers
-- ---------------------------------------------------------------------------

-- The explicit authority receipt behind a waived gate.
--
-- The waiver itself stays where it always was: a `waived` row in
-- `task_gate_evaluations`, which is what `gate_states` and `certify_closure`
-- read. This table does not duplicate that verdict — it records *who was allowed
-- to forgive it and on what grounds*, bound by composite foreign key to the
-- exact evaluation it explains. One receipt per waiver, so a single reason
-- cannot quietly cover a second one.
CREATE TABLE gate_waivers (
    id                   TEXT    NOT NULL PRIMARY KEY
                                 CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id           TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    workflow_id          TEXT    NOT NULL,
    gate_key             TEXT    NOT NULL CHECK (length(gate_key) BETWEEN 1 AND 128),
    sequence             INTEGER NOT NULL CHECK (sequence >= 1),
    authorizing_role     TEXT    NOT NULL CHECK (length(authorizing_role) BETWEEN 1 AND 128),
    authorizing_account  TEXT    NOT NULL,
    reason               TEXT    NOT NULL CHECK (length(reason) BETWEEN 1 AND 512),
    evidence             TEXT    NOT NULL CHECK (json_valid(evidence)),
    evidence_hash        TEXT    NOT NULL
                                 CHECK (length(evidence_hash) = 64
                                        AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at          TEXT    NOT NULL
                                 CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    UNIQUE (project_id, workflow_id, gate_key, sequence),
    FOREIGN KEY (project_id, workflow_id, gate_key, sequence)
        REFERENCES task_gate_evaluations (project_id, workflow_id, gate_key, sequence)
        ON DELETE RESTRICT,
    FOREIGN KEY (project_id, authorizing_account)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;

-- ---------------------------------------------------------------------------
-- 4. Approval receipts
-- ---------------------------------------------------------------------------

-- One approval, bound to one exact destructive action.
--
-- `action_digest` is the canonical digest of the concrete command, arguments
-- included, and it is unique per project: an approval for one deletion cannot be
-- replayed against another. `consumed_at` is the spend, and the trigger below
-- makes it a one-way door.
--
-- `authority_source` may not be `recovery_advice`. The value exists in the
-- domain enum so an advisor's recommendation can be recognized and refused; a
-- row carrying it is refused outright, so the refusal does not depend on any
-- caller remembering to check.
CREATE TABLE approval_receipts (
    id                 TEXT NOT NULL PRIMARY KEY
                            CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id         TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    scope_kind         TEXT NOT NULL CHECK (scope_kind IN ('project', 'task')),
    task_id            TEXT NULL,
    action_domain      TEXT NOT NULL CHECK (action_domain IN
                            ('filesystem', 'runtime', 'external_ticket', 'control_plane')),
    action_intent      TEXT NOT NULL CHECK (action_intent IN
                            ('inspect', 'produce_artifact', 'complete_phase', 'record_gate_verdict',
                             'record_gate_rejection', 'close_run', 'mutate')),
    action_effect      TEXT NOT NULL CHECK (action_effect IN ('read', 'mutate', 'destroy')),
    action_digest      TEXT NOT NULL
                            CHECK (length(action_digest) = 64
                                   AND action_digest NOT GLOB '*[^0-9a-f]*'),
    approver_principal TEXT NOT NULL CHECK (length(approver_principal) BETWEEN 1 AND 256
                                            AND approver_principal NOT GLOB '* *'),
    approver_role      TEXT NOT NULL CHECK (length(approver_role) BETWEEN 1 AND 128),
    approver_account   TEXT NOT NULL,
    authority_source   TEXT NOT NULL CHECK (authority_source IN
                            ('operator', 'execution_authorization')),
    evidence           TEXT NOT NULL CHECK (json_valid(evidence)),
    evidence_hash      TEXT NOT NULL
                            CHECK (length(evidence_hash) = 64
                                   AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    issued_at          TEXT NOT NULL
                            CHECK (issued_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    expires_at         TEXT NOT NULL
                            CHECK (expires_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    consumed_at        TEXT NULL
                            CHECK (consumed_at IS NULL
                                   OR consumed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    -- The binding that makes replay impossible.
    UNIQUE (project_id, action_digest),
    -- A task-scoped approval must name its task; a project-scoped one must not
    -- carry a task it does not actually cover.
    CHECK ((scope_kind = 'task') = (task_id IS NOT NULL)),
    -- An approval that never expires is a standing permission by another name.
    CHECK (expires_at > issued_at),
    CHECK (consumed_at IS NULL OR consumed_at >= issued_at),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, approver_account)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;

-- ---------------------------------------------------------------------------
-- 5. Recovery episodes and steps
-- ---------------------------------------------------------------------------

-- One bounded recovery for one parked run.
--
-- The budgets are in the schema, not only in Rust: two follow-ups, one advisor,
-- one committee. A caller that bypassed `kontor-policy` entirely still cannot
-- store a third dispatched follow-up.
CREATE TABLE recovery_episodes (
    id                     TEXT    NOT NULL PRIMARY KEY
                                   CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id             TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id                TEXT    NOT NULL,
    workflow_id            TEXT    NOT NULL,
    parked_agent_run_id    TEXT    NOT NULL,
    status                 TEXT    NOT NULL CHECK (status IN (
                                       'open', 'deterministic_repair', 'advisor', 'committee',
                                       'followup', 'recovered', 'needs_human')),
    cause_evaluation_id    TEXT    NOT NULL,
    advisor_used           INTEGER NOT NULL CHECK (advisor_used IN (0, 1)),
    committee_used         INTEGER NOT NULL CHECK (committee_used IN (0, 1)),
    effective_followups    INTEGER NOT NULL CHECK (effective_followups BETWEEN 0 AND 2),
    successor_agent_run_id TEXT    NULL,
    escalation_cause       TEXT    NULL CHECK (escalation_cause IS NULL OR escalation_cause IN (
                                       'unsafe_state', 'missing_authority', 'committee_disagreement',
                                       'incomplete_evidence', 'budget_exhausted')),
    revision               INTEGER NOT NULL CHECK (revision >= 1),
    created_at             TEXT    NOT NULL
                                   CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    closed_at              TEXT    NULL
                                   CHECK (closed_at IS NULL
                                          OR closed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    -- Closed exactly when terminal, in both directions.
    CHECK ((status IN ('recovered', 'needs_human')) = (closed_at IS NOT NULL)),
    -- `needs_human` is unreachable without one of the five declared causes, and
    -- a cause is meaningless without it.
    CHECK ((status = 'needs_human') = (escalation_cause IS NOT NULL)),
    -- Recovery runs as a successor. The parked run is never its own rescue.
    CHECK (successor_agent_run_id IS NULL OR successor_agent_run_id <> parked_agent_run_id),
    -- A successor exists only because a follow-up was dispatched.
    CHECK (successor_agent_run_id IS NULL OR effective_followups >= 1),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, workflow_id)
        REFERENCES task_workflows (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, parked_agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, successor_agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, cause_evaluation_id)
        REFERENCES policy_evaluations (project_id, id) ON DELETE RESTRICT
) STRICT;

-- One open episode per parked run. A restart that replays the park finds the
-- episode already there instead of opening a second one against the same run.
CREATE UNIQUE INDEX ux_recovery_episodes_open
    ON recovery_episodes (project_id, parked_agent_run_id) WHERE closed_at IS NULL;

-- Everything an episode actually did, in order.
--
-- Append-only, including the steps that achieved nothing: a follow-up whose
-- preflight refused it is recorded so an audit can see it was tried, and it is
-- distinguishable from one that ran because only the latter moved
-- `effective_followups`.
CREATE TABLE recovery_steps (
    project_id           TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    episode_id           TEXT    NOT NULL,
    sequence             INTEGER NOT NULL CHECK (sequence >= 1),
    kind                 TEXT    NOT NULL CHECK (kind IN (
                                     'deterministic_repair', 'advisor', 'committee',
                                     'followup_execution', 'escalation')),
    input_hash           TEXT    NOT NULL
                                 CHECK (length(input_hash) = 64 AND input_hash NOT GLOB '*[^0-9a-f]*'),
    output_hash          TEXT    NULL
                                 CHECK (output_hash IS NULL
                                        OR (length(output_hash) = 64
                                            AND output_hash NOT GLOB '*[^0-9a-f]*')),
    agent_run_id         TEXT    NULL,
    policy_evaluation_id TEXT    NULL,
    artifact_evidence_id TEXT    NULL,
    recorded_at          TEXT    NOT NULL
                                 CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, episode_id, sequence),
    -- A read-only consultation cannot name a run: the one step that starts work
    -- is unrepresentable from an advisor or a committee row.
    CHECK (kind NOT IN ('advisor', 'committee') OR agent_run_id IS NULL),
    FOREIGN KEY (project_id, episode_id)
        REFERENCES recovery_episodes (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, policy_evaluation_id)
        REFERENCES policy_evaluations (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, artifact_evidence_id)
        REFERENCES artifact_evidence (project_id, id) ON DELETE RESTRICT
) STRICT;

-- ---------------------------------------------------------------------------
-- 6. Park closures
-- ---------------------------------------------------------------------------

-- Why a closed run was closed: a guardrail parked it, not an operator.
--
-- The run itself closes with `lifecycle = 'parked'` and
-- `terminal_outcome = 'abandoned'`, which is v1's own encoding of "closed
-- without a runtime verdict" — no runtime ever reports `parked`, so
-- `terminal_outcome = 'parked'` has no admissible evidence and the v1 CHECKs
-- cannot be widened by `ALTER TABLE` to invent one.
--
-- What that encoding cannot say is *who* closed it, and a guardrail park is not
-- a human decision. This table is what keeps the two apart: it names the
-- evaluation that caused the park and the episode that owns the recovery, both
-- by composite foreign key, so an audit can separate an automatic park from an
-- operator abandon without reading a receipt payload.
CREATE TABLE run_park_closures (
    project_id           TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    agent_run_id         TEXT NOT NULL,
    team_run_id          TEXT NULL,
    policy_evaluation_id TEXT NOT NULL,
    recovery_episode_id  TEXT NOT NULL,
    closure_receipt_id   TEXT NOT NULL,
    reason_code          TEXT NOT NULL CHECK (length(reason_code) BETWEEN 1 AND 128
                                              AND reason_code NOT GLOB '*[^a-z0-9_]*'),
    evidence_hash        TEXT NOT NULL
                              CHECK (length(evidence_hash) = 64
                                     AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at          TEXT NOT NULL
                              CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, agent_run_id),
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_run_id) REFERENCES team_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, policy_evaluation_id)
        REFERENCES policy_evaluations (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, recovery_episode_id)
        REFERENCES recovery_episodes (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, closure_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

-- ---------------------------------------------------------------------------
-- 7. Reviewer identity on gate evaluations
-- ---------------------------------------------------------------------------

-- A rejection counter needs to know *who* rejected, and neither column v1 offers
-- is that. `evaluator_account` is the profile a verdict was written through, and
-- a run id is reissued on every relaunch — keying on either would reset the
-- count exactly when the same reviewer comes back, which is the one moment the
-- count has to survive.
--
-- All three are nullable with no default and no backfill. A v1 or v2 row is
-- attributable to nobody, so it neither advances nor resets any reviewer's
-- stream; inventing a principal for it would fabricate the very fact the counter
-- is derived from.
ALTER TABLE task_gate_evaluations ADD COLUMN agent_run_id TEXT NULL;

ALTER TABLE task_gate_evaluations ADD COLUMN reviewer_principal TEXT NULL
    CHECK (reviewer_principal IS NULL
           OR (length(reviewer_principal) BETWEEN 1 AND 256
               AND reviewer_principal NOT GLOB '* *'));

ALTER TABLE task_gate_evaluations ADD COLUMN policy_evaluation_id TEXT NULL
    CHECK (policy_evaluation_id IS NULL
           OR (length(policy_evaluation_id) = 36
               AND policy_evaluation_id NOT GLOB '*[^0-9a-f-]*'));

-- `ALTER TABLE` cannot add a composite foreign key, and a single-column one
-- would let a globally valid id from another project resolve — which is exactly
-- the isolation this schema does not accept. So the project-scoped binding for
-- both added references is a trigger, the same mechanism v1 uses everywhere
-- SQLite cannot express a rule as a constraint.
CREATE TRIGGER task_gate_evaluations_run_in_project
BEFORE INSERT ON task_gate_evaluations
WHEN NEW.agent_run_id IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM agent_runs
                     WHERE project_id = NEW.project_id AND id = NEW.agent_run_id)
BEGIN SELECT RAISE(ABORT, 'a gate evaluation names an agent run from another project'); END;

CREATE TRIGGER task_gate_evaluations_evaluation_in_project
BEFORE INSERT ON task_gate_evaluations
WHEN NEW.policy_evaluation_id IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM policy_evaluations
                     WHERE project_id = NEW.project_id AND id = NEW.policy_evaluation_id)
BEGIN SELECT RAISE(ABORT, 'a gate evaluation names a policy evaluation from another project'); END;

-- ---------------------------------------------------------------------------
-- 8. Immutability
-- ---------------------------------------------------------------------------

CREATE TRIGGER policy_evaluations_no_update BEFORE UPDATE ON policy_evaluations
BEGIN SELECT RAISE(ABORT, 'a guardrail evaluation is immutable'); END;
CREATE TRIGGER policy_evaluations_no_delete BEFORE DELETE ON policy_evaluations
BEGIN SELECT RAISE(ABORT, 'a guardrail evaluation is immutable'); END;

CREATE TRIGGER artifact_evidence_no_update BEFORE UPDATE ON artifact_evidence
BEGIN SELECT RAISE(ABORT, 'artifact evidence is immutable'); END;
CREATE TRIGGER artifact_evidence_no_delete BEFORE DELETE ON artifact_evidence
BEGIN SELECT RAISE(ABORT, 'artifact evidence is immutable'); END;

CREATE TRIGGER gate_waivers_no_update BEFORE UPDATE ON gate_waivers
BEGIN SELECT RAISE(ABORT, 'a waiver receipt is immutable'); END;
CREATE TRIGGER gate_waivers_no_delete BEFORE DELETE ON gate_waivers
BEGIN SELECT RAISE(ABORT, 'a waiver receipt is immutable'); END;

CREATE TRIGGER recovery_steps_no_update BEFORE UPDATE ON recovery_steps
BEGIN SELECT RAISE(ABORT, 'a recovery step is immutable'); END;
CREATE TRIGGER recovery_steps_no_delete BEFORE DELETE ON recovery_steps
BEGIN SELECT RAISE(ABORT, 'a recovery step is immutable'); END;

CREATE TRIGGER run_park_closures_no_update BEFORE UPDATE ON run_park_closures
BEGIN SELECT RAISE(ABORT, 'a park closure is immutable'); END;
CREATE TRIGGER run_park_closures_no_delete BEFORE DELETE ON run_park_closures
BEGIN SELECT RAISE(ABORT, 'a park closure is immutable'); END;

-- An approval is written once and spent once. The only column that may ever
-- change is `consumed_at`, and only from unspent to spent: without the second
-- half of that rule a spent receipt could be un-spent and replayed.
CREATE TRIGGER approval_receipts_spend_only BEFORE UPDATE ON approval_receipts
WHEN OLD.consumed_at IS NOT NULL
     OR NEW.consumed_at IS NULL
     OR OLD.project_id IS NOT NEW.project_id
     OR OLD.scope_kind IS NOT NEW.scope_kind
     OR OLD.task_id IS NOT NEW.task_id
     OR OLD.action_domain IS NOT NEW.action_domain
     OR OLD.action_intent IS NOT NEW.action_intent
     OR OLD.action_effect IS NOT NEW.action_effect
     OR OLD.action_digest IS NOT NEW.action_digest
     OR OLD.approver_principal IS NOT NEW.approver_principal
     OR OLD.approver_role IS NOT NEW.approver_role
     OR OLD.approver_account IS NOT NEW.approver_account
     OR OLD.authority_source IS NOT NEW.authority_source
     OR OLD.evidence_hash IS NOT NEW.evidence_hash
     OR OLD.issued_at IS NOT NEW.issued_at
     OR OLD.expires_at IS NOT NEW.expires_at
BEGIN SELECT RAISE(ABORT, 'an approval receipt is immutable and may only be consumed once'); END;

CREATE TRIGGER approval_receipts_no_delete BEFORE DELETE ON approval_receipts
BEGIN SELECT RAISE(ABORT, 'an approval receipt is immutable'); END;

-- A closed episode never reopens, its identity never moves, and every change
-- advances the revision by exactly one — so a replayed transition cannot be
-- applied twice even through raw SQL.
CREATE TRIGGER recovery_episodes_closed_immutable BEFORE UPDATE ON recovery_episodes
WHEN OLD.closed_at IS NOT NULL
BEGIN SELECT RAISE(ABORT, 'a closed recovery episode is immutable'); END;

CREATE TRIGGER recovery_episodes_identity_immutable BEFORE UPDATE ON recovery_episodes
WHEN OLD.project_id IS NOT NEW.project_id
     OR OLD.task_id IS NOT NEW.task_id
     OR OLD.workflow_id IS NOT NEW.workflow_id
     OR OLD.parked_agent_run_id IS NOT NEW.parked_agent_run_id
     OR OLD.cause_evaluation_id IS NOT NEW.cause_evaluation_id
     OR OLD.created_at IS NOT NEW.created_at
     OR NEW.revision IS NOT OLD.revision + 1
     -- Budgets are spent, never returned.
     OR NEW.advisor_used < OLD.advisor_used
     OR NEW.committee_used < OLD.committee_used
     OR NEW.effective_followups < OLD.effective_followups
BEGIN SELECT RAISE(ABORT, 'a recovery episode identity and its spent budget are immutable'); END;

-- An episode state only ever moves *because a step was appended*.
--
-- A blanket no-update trigger is not available here and would be the wrong
-- shape if it were: unlike the evidence tables, an episode is an aggregate that
-- legitimately advances — that is what `status`, the budgets and `revision` are
-- for. Freezing it would make recovery unrepresentable.
--
-- What must be impossible is an advance with *no record of what caused it*: a
-- direct `UPDATE ... SET status = 'recovered'` that skips the append path and
-- leaves an episode claiming an outcome nothing accounts for.
--
-- So the update is bound to the history instead of forbidden. Each transition
-- appends exactly one step, so after N steps the revision is N + 1; requiring
-- that equation at every update means an advance is only reachable once its step
-- exists. Together with `recovery_episodes_identity_immutable` — which already
-- forces `NEW.revision = OLD.revision + 1` — the two are exact: one update, one
-- step, no gap in either direction, whether the writer is the store service or
-- raw SQL.
--
-- The store service therefore appends the step *first* and updates second,
-- which is the same order every other consequence in this schema is written in:
-- evidence, then the thing derived from it.
CREATE TRIGGER recovery_episodes_require_step BEFORE UPDATE ON recovery_episodes
WHEN NEW.revision IS NOT (SELECT count(*) FROM recovery_steps
                          WHERE project_id = NEW.project_id AND episode_id = NEW.id) + 1
BEGIN SELECT RAISE(ABORT, 'a recovery episode advances only by appending a step'); END;

CREATE TRIGGER recovery_episodes_no_delete BEFORE DELETE ON recovery_episodes
BEGIN SELECT RAISE(ABORT, 'recovery episodes are not deletable'); END;

-- One successor run belongs to at most one step of one episode.
--
-- The budget counts dispatches, so a caller that hands the same run id to both
-- follow-ups would spend two of them on one session — two entries in the ledger,
-- one thing that actually ran, and a "linked successor" that is really the
-- previous attempt being resumed under a new number. The index makes that
-- unrepresentable rather than merely refused in Rust.
--
-- Partial, because the read-only steps deliberately name no run at all and would
-- otherwise all collide on NULL.
CREATE UNIQUE INDEX ux_recovery_steps_successor
    ON recovery_steps (project_id, episode_id, agent_run_id)
    WHERE agent_run_id IS NOT NULL;

PRAGMA user_version = 3;
