-- Durable Advisor consultations: the run, its frozen context, its one immutable
-- advice artifact and the append-only dispositions recorded about it.
--
-- A consultation is read-only advice. What makes it *evidence* rather than chat
-- is that everything it was asked under is frozen before the first native
-- effect: the profile revision, the question bytes, the resolved context and its
-- provenance, and the exact ASW node and seat that will answer. A later edit to
-- the profile, the files or the epic cannot reach an already-invoked
-- consultation, because none of it is read again.
--
-- The row is written before its effects and carries the ids those effects will
-- use, exactly as `quick_sessions` does and for the same reason: the node cannot
-- be found by searching, since two consultations of one epic are both ASW nodes
-- under the same parent and a search cannot tell them apart. Written after its
-- effects, any failure in between would leave an orphaned workspace and an
-- unattached seat binding that nothing can attribute — which is what the
-- OP-REQ-039 phantom was made of. The id columns are plain `TEXT` with no
-- foreign keys precisely so the row can be written while the things it names do
-- not exist yet.
CREATE TABLE advisor_runs (
    id                          TEXT    NOT NULL PRIMARY KEY
                                        CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id                  TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    -- The epic this consultation advises. An ASW is a child of the epic's ESW,
    -- so a consultation with no epic has nowhere to be placed and nothing to
    -- advise.
    mini_project_id             TEXT    NOT NULL CHECK (length(mini_project_id) = 36),
    -- The ticket, when the question was asked about one. A ticket-scoped
    -- consultation changes the scope key, never the node kind.
    task_id                     TEXT    NULL CHECK (task_id IS NULL OR length(task_id) = 36),
    -- The frozen profile revision. Held as id + version + canonical hash so the
    -- advice stays readable against the exact policy that produced it after the
    -- project has published ten more revisions.
    profile_id                  TEXT    NOT NULL CHECK (length(profile_id) = 36),
    profile_version             INTEGER NOT NULL CHECK (profile_version >= 1),
    profile_hash                TEXT    NOT NULL
                                        CHECK (length(profile_hash) = 64
                                               AND profile_hash NOT GLOB '*[^0-9a-f]*'),
    question                    TEXT    NOT NULL CHECK (length(question) BETWEEN 1 AND 65536),
    question_hash               TEXT    NOT NULL
                                        CHECK (length(question_hash) = 64
                                               AND question_hash NOT GLOB '*[^0-9a-f]*'),
    -- The epic-owner authority this consultation was requested under: the exact
    -- ECP LSA seat, resolved server-side from the epic and never accepted from the
    -- request. It does not identify the caller -- the realm has one bearer secret
    -- per authority tier, so there is no principal at the boundary to record --
    -- and it is preserved per run, so replacing the LSA affects only later runs.
    owner_authority_seat_binding_id   TEXT    NOT NULL CHECK (length(owner_authority_seat_binding_id) = 36),
    -- The canonical resolved context document and its digest, byte-for-byte as
    -- delivered to the seat, plus the provenance of every source it was built
    -- from and every redaction applied.
    context                     TEXT    NOT NULL CHECK (json_valid(context)),
    context_hash                TEXT    NOT NULL
                                        CHECK (length(context_hash) = 64
                                               AND context_hash NOT GLOB '*[^0-9a-f]*'),
    provenance                  TEXT    NOT NULL CHECK (json_valid(provenance)),
    -- The ASW and the one Advisor seat inside it.
    topology_node_id            TEXT    NOT NULL CHECK (length(topology_node_id) = 36),
    seat_binding_id             TEXT    NOT NULL CHECK (length(seat_binding_id) = 36),
    role_slot_id                TEXT    NOT NULL CHECK (length(role_slot_id) BETWEEN 1 AND 128),
    role                        TEXT    NOT NULL CHECK (json_valid(role)),
    -- The epic ESW this ASW was placed inside, and the native project observed
    -- for it at placement. A stored value that later stops matching is drift and
    -- refuses rather than placing a sibling somewhere else.
    esw_topology_node_id        TEXT    NOT NULL CHECK (length(esw_topology_node_id) = 36),
    esw_native_id               TEXT    NULL CHECK (esw_native_id IS NULL OR
                                                    length(esw_native_id) BETWEEN 1 AND 256),
    -- `placed` once the ASW and seat exist; `advised` once the immutable advice
    -- artifact is durable; `disposed` once a disposition has been recorded;
    -- `needs_human` for a path that cannot produce an answer and must say so
    -- with a recommendation rather than stall in `placed` forever.
    state                       TEXT    NOT NULL CHECK (state IN (
                                            'placed', 'advised', 'disposed', 'needs_human')),
    intent_hash                 TEXT    NOT NULL
                                        CHECK (length(intent_hash) = 64
                                               AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    revision                    INTEGER NOT NULL CHECK (revision >= 1),
    created_at                  TEXT    NOT NULL
                                        CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    -- One invocation opens one consultation. Without this, the retry that lost
    -- its answer would be free to place a second ASW and spend a second
    -- consultation against the profile's limit.
    UNIQUE (project_id, intent_hash)
) STRICT;

CREATE INDEX ix_advisor_runs_epic ON advisor_runs (project_id, mini_project_id);

-- The Advisor's own bounded output. One per run, immutable once written.
--
-- This is the whole of the Advisor's write authority: it may submit its own
-- advice as evidence and nothing else. A disposition recorded later cannot
-- rewrite it, which is what makes "the advice was considered and not adopted" a
-- statement about a durable document rather than about whatever it says now.
CREATE TABLE advisor_advice (
    advisor_run_id TEXT    NOT NULL PRIMARY KEY REFERENCES advisor_runs (id) ON DELETE RESTRICT,
    advice         TEXT    NOT NULL CHECK (length(advice) BETWEEN 1 AND 65536),
    advice_hash    TEXT    NOT NULL
                           CHECK (length(advice_hash) = 64 AND advice_hash NOT GLOB '*[^0-9a-f]*'),
    created_at     TEXT    NOT NULL
                           CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

CREATE TRIGGER advisor_advice_is_immutable
BEFORE UPDATE ON advisor_advice
BEGIN SELECT RAISE(ABORT, 'recorded advice is immutable'); END;

CREATE TRIGGER advisor_advice_is_permanent
BEFORE DELETE ON advisor_advice
BEGIN SELECT RAISE(ABORT, 'recorded advice cannot be withdrawn'); END;

-- What the requester or the owning LSA decided about that advice, append-only.
--
-- `superseded` is how a later decision replaces an earlier one: the earlier row
-- stays, because the fact that the advice was once rejected is part of the
-- record. A disposition may cite the receipts of commands that were separately
-- authorized; it never asserts that one ran.
CREATE TABLE advisor_dispositions (
    advisor_run_id TEXT    NOT NULL REFERENCES advisor_runs (id) ON DELETE RESTRICT,
    sequence       INTEGER NOT NULL CHECK (sequence >= 1),
    disposition    TEXT    NOT NULL CHECK (disposition IN (
                               'accepted', 'partially_accepted', 'rejected', 'superseded')),
    rationale      TEXT    NOT NULL CHECK (length(rationale) BETWEEN 1 AND 65536),
    -- Receipt ids of separately authorized commands this decision refers to.
    cited_receipts TEXT    NOT NULL CHECK (json_valid(cited_receipts)),
    recorded_by    TEXT    NOT NULL CHECK (length(recorded_by) = 36),
    created_at     TEXT    NOT NULL
                           CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (advisor_run_id, sequence)
) STRICT;

CREATE TRIGGER advisor_dispositions_are_immutable
BEFORE UPDATE ON advisor_dispositions
BEGIN SELECT RAISE(ABORT, 'a recorded disposition is immutable'); END;

CREATE TRIGGER advisor_dispositions_are_permanent
BEFORE DELETE ON advisor_dispositions
BEGIN SELECT RAISE(ABORT, 'a recorded disposition cannot be withdrawn'); END;

-- Widen the closed command-kind list by the two Advisor run commands.
--
-- The two Committee run commands are deliberately absent: no service writes them
-- yet, and a kind the database accepts before the daemon can produce it is a
-- promise only one half of the system keeps.
--
-- This is the ninth rebuild of `command_receipts`, and the first that does not
-- drop its triggers: v33 restored them and `schema_v1.rs` now pins them by name,
-- so they are re-created below beside the index.
CREATE TABLE command_receipts_v34 (
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
                                 'apply_core_team', 'ensure_quick_session',
                                 'promote_quick_session', 'materialize_core_team',
                                 'upgrade_epic_roster', 'apply_advisor_profile',
                                 'apply_committee_template', 'invoke_advisor_run',
                                 'settle_advisor_run')),
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

INSERT INTO command_receipts_v34
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v34 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key
     OR OLD.target <> NEW.target
     OR OLD.intent <> NEW.intent
     OR OLD.intent_hash <> NEW.intent_hash
     OR OLD.kind <> NEW.kind
     OR OLD.project_id <> NEW.project_id
     OR OLD.state IN ('confirmed', 'failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;

CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

PRAGMA user_version = 34;
