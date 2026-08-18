-- Published epic Completion Profiles, one durable completion run per epic, and
-- the outbox that wakes an epic's existing TPM seat.
--
-- All three exist for the same reason: a completion fact that lived only in
-- process memory would, after a restart between a transition being committed and
-- its effects being complete, run the effect suffix a *second* time — a second
-- Team C run, a second Committee round, a second closeout. The state is written
-- before the effects, and the effects are found again from these rows.

-- One immutable published Completion Profile revision.
--
-- The definition is stored as its canonical bytes with its digest beside it,
-- rather than as exploded columns. Preview hashes the canonical document and
-- apply is only allowed to publish the document that hash was taken over, so
-- normalizing it into columns here would mean re-assembling the exact bytes on
-- every read and hoping the reassembly is byte-identical. It would not have to
-- be wrong often to matter: one differing byte silently invalidates every
-- preview hash a caller holds.
--
-- `(project_id, id, version)` is the key because a revision is immutable: a
-- second write of the same version is a conflict, never an update. That is what
-- makes an epic's pin meaningful — the bytes under a pinned version cannot move.
CREATE TABLE completion_profile_revisions (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    id              TEXT    NOT NULL CHECK (length(id) BETWEEN 1 AND 128),
    version         INTEGER NOT NULL CHECK (version >= 1),
    name            TEXT    NOT NULL CHECK (length(name) BETWEEN 1 AND 128),
    definition      TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64 AND
                                   definition_hash NOT GLOB '*[^0-9a-f]*'),
    published_at    TEXT    NOT NULL
                            CHECK (published_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, id, version)
) STRICT;

CREATE INDEX ix_completion_profiles_project ON completion_profile_revisions (project_id);

-- One epic's durable completion run.
--
-- The pinned profile is three columns beside the state document, not a field
-- inside it. A restore has to be able to refuse a state whose pin disagrees with
-- the profile it is handed, and it cannot make that comparison from data it
-- needed the profile to decode first.
--
-- `revision` mirrors the revision inside the state document. It is a column so
-- the optimistic-concurrency check is one indexed comparison rather than a
-- decode of every candidate row, and the pair is written together so they cannot
-- disagree.
--
-- No foreign key to `mini_projects`, matching `epic_rosters`: completion is
-- attached to the epic and addressed by its id, and the row's lifetime is not
-- the epic row's to end.
CREATE TABLE epic_completion (
    project_id      TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id TEXT    NOT NULL CHECK (length(mini_project_id) = 36),
    profile_id      TEXT    NOT NULL CHECK (length(profile_id) BETWEEN 1 AND 128),
    profile_version INTEGER NOT NULL CHECK (profile_version >= 1),
    definition_hash TEXT    NOT NULL
                            CHECK (length(definition_hash) = 64 AND
                                   definition_hash NOT GLOB '*[^0-9a-f]*'),
    state           TEXT    NOT NULL CHECK (json_valid(state)),
    revision        INTEGER NOT NULL CHECK (revision >= 1),
    updated_at      TEXT    NOT NULL
                            CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, mini_project_id)
) STRICT;

-- The wake intents completion has appended for an epic's existing TPM seat.
--
-- The primary key *is* the idempotency rule: one wake per
-- `(epic, completion revision, reason, seat)`. A duplicate observation or a
-- replayed runtime callback collides with the row already standing and reuses
-- its receipt, so neither can open a second turn for one completion revision.
-- Without this key that de-duplication would have to be a lookup followed by an
-- insert, and two concurrent callbacks would both pass the lookup.
--
-- `acknowledged_at` is what separates "appended" from "the runtime took the
-- turn". A resumed dispatch continues only the unacknowledged suffix; a wake
-- that reported success on an unacknowledged intent would be claiming the seat
-- had been woken because a row existed saying it should be.
CREATE TABLE epic_completion_wakes (
    project_id          TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id     TEXT    NOT NULL CHECK (length(mini_project_id) = 36),
    completion_revision INTEGER NOT NULL CHECK (completion_revision >= 1),
    reason              TEXT    NOT NULL CHECK (length(reason) BETWEEN 1 AND 128),
    seat_binding_id     TEXT    NOT NULL CHECK (length(seat_binding_id) = 36),
    receipt             TEXT    NOT NULL
                                CHECK (length(receipt) = 64 AND receipt NOT GLOB '*[^0-9a-f]*'),
    appended_at         TEXT    NOT NULL
                                CHECK (appended_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    acknowledged_at     TEXT    NULL
                                CHECK (acknowledged_at IS NULL OR
                                       acknowledged_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, mini_project_id, completion_revision, reason, seat_binding_id)
) STRICT;

CREATE INDEX ix_completion_wakes_pending
    ON epic_completion_wakes (project_id, mini_project_id)
    WHERE acknowledged_at IS NULL;

-- One epic LSA remediation proposal, waiting for its TPM route.
--
-- Remediation takes two authorities acting in order and no round may launch
-- until both receipts are durable. The first half therefore needs somewhere to
-- live that is not the completion state: the state records an authorization only
-- once it is *complete*, and a half-filled one stored there would be
-- indistinguishable from an approved one to every reader of it.
--
-- Keyed by round, so a proposal is per failed round and a second proposal for
-- the same round is a conflict rather than a silent replacement of the bounded
-- correction the TPM is about to route.
CREATE TABLE epic_completion_remediation_proposals (
    project_id            TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id       TEXT    NOT NULL CHECK (length(mini_project_id) = 36),
    round                 INTEGER NOT NULL CHECK (round >= 1),
    -- The failed round's evidence as the proposer read it. Stored so the route
    -- that follows can be checked against the same round the LSA answered.
    failed_round_evidence TEXT    NOT NULL
                                  CHECK (length(failed_round_evidence) = 64 AND
                                         failed_round_evidence NOT GLOB '*[^0-9a-f]*'),
    proposal              TEXT    NOT NULL
                                  CHECK (length(proposal) = 64 AND
                                         proposal NOT GLOB '*[^0-9a-f]*'),
    -- The exact seat that proposed. A later route is checked against the epic's
    -- current LSA seat, so a proposal from a replaced seat cannot be routed.
    lsa_seat_binding_id   TEXT    NOT NULL CHECK (length(lsa_seat_binding_id) = 36),
    proposed_at           TEXT    NOT NULL
                                  CHECK (proposed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, mini_project_id, round)
) STRICT;

-- Widen the closed command-kind list by the three OP-06 commands.
--
-- Same rebuild shape as v24, v28, v29, v30 and v31: `kind` is a CHECK, so a new
-- command is a migration rather than a code change.
CREATE TABLE command_receipts_v35 (
    id               TEXT    NOT NULL PRIMARY KEY
                             CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id       TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    idempotency_key  TEXT    NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    kind             TEXT    NOT NULL CHECK (kind IN (
                                 'launch_run', 'cancel_run', 'park_run', 'abandon_run',
                                 'resume_task', 'record_gate_verdict', 'approve_intake',
                                 'sync_ticket', 'assign_ticket', 'transition_ticket',
                                 'authorize_execution', 'approve_schedule_override',
                                 'revoke_schedule_override', 'resolve_status_conflict',
                                 'assign_work_calendar', 'revoke_execution_authorization',
                                 'ensure_project', 'ensure_account_profile',
                                 'apply_epic_graph', 'transition_epic',
                                 'start_scheduled_work', 'transition_task',
                                 'resolve_context', 'select_task_profile',
                                 'select_task_team', 'select_task_account',
                                 'reconcile_ticket', 'settle_runtime',
                                 'submit_intake', 'pull_ticket_comments',
                                 'claim_ticket', 'replace_seat',
                                 'refresh_capacity', 'override_availability',
                                 'observe_seat', 'retire_seat',
                                 'publish_topology_spec', 'upgrade_topology',
                                 'retitle_container',
                                 'apply_core_team', 'ensure_quick_session',
                                 'promote_quick_session', 'materialize_core_team',
                                 'upgrade_epic_roster', 'apply_advisor_profile',
                                 'apply_committee_template', 'apply_completion_profile',
                                 'advance_completion', 'remediate_completion',
                                 'publish_trigger')),
    target           TEXT    NOT NULL CHECK (json_valid(target)),
    target_revision  INTEGER NOT NULL CHECK (target_revision >= 1),
    intent           TEXT    NOT NULL CHECK (json_valid(intent)),
    intent_hash      TEXT    NOT NULL
                             CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    state            TEXT    NOT NULL CHECK (state IN (
                                 'intent_persisted', 'dispatch_pending', 'dispatched',
                                 'acknowledged', 'confirmation_unknown', 'confirmed', 'failed')),
    correlation      TEXT    NULL CHECK (correlation IS NULL OR length(correlation) BETWEEN 1 AND 256),
    native_identity  TEXT    NULL CHECK (native_identity IS NULL OR json_valid(native_identity)),
    result_ref       TEXT    NULL CHECK (result_ref IS NULL OR length(result_ref) BETWEEN 1 AND 256),
    attempts         INTEGER NOT NULL CHECK (attempts >= 0),
    created_at       TEXT    NOT NULL
                             CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    updated_at       TEXT    NOT NULL
                             CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id)
) STRICT;

INSERT INTO command_receipts_v35
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v35 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

PRAGMA user_version = 35;
