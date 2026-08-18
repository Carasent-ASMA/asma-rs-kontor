-- Quick sessions, their promotion to an epic, and the roster an epic froze.
--
-- Three facts OP-04 owns, and each is here because something has to survive a
-- restart between a command being authorized and its effects being complete.
-- A promotion that lived in process memory would, after a crash halfway
-- through, build a *second* MiniProject on retry — which is the one outcome
-- promotion is not allowed to have.

-- One ad-hoc session under the project's session base.
--
-- The node and seat ids are columns rather than something rediscovered by
-- searching the topology for a node of the right kind. Two Quick sessions in
-- one project are both QSW nodes below the same base, so a search by kind
-- cannot tell them apart, and reconciling the wrong one would hand a second
-- session's work to the first one's seat.
CREATE TABLE quick_sessions (
    id                   TEXT    NOT NULL PRIMARY KEY
                                 CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id           TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    -- The exact catalog role snapshot the seat fills, resolved once at open.
    role                 TEXT    NOT NULL CHECK (json_valid(role)),
    role_slot_id         TEXT    NOT NULL CHECK (length(role_slot_id) BETWEEN 1 AND 128),
    topology_node_id     TEXT    NOT NULL
                                 CHECK (length(topology_node_id) = 36),
    seat_binding_id      TEXT    NOT NULL CHECK (length(seat_binding_id) = 36),
    -- The base this session was placed under, as it was bound at open. Kept so
    -- a later readback can be compared with what the placement actually used.
    psw_topology_node_id TEXT    NOT NULL CHECK (length(psw_topology_node_id) = 36),
    -- The native project observed for that base at placement, when one had
    -- been observed. Absent means nothing had been read back yet, which is not
    -- the same as a disagreement: a base nothing has been placed under has no
    -- observation to disagree with. A stored value that later stops matching is
    -- drift, and refuses.
    psw_native_id        TEXT    NULL CHECK (psw_native_id IS NULL OR
                                             length(psw_native_id) BETWEEN 1 AND 256),
    purpose              TEXT    NOT NULL CHECK (length(purpose) BETWEEN 1 AND 512),
    -- The canonical intent of the command that opened this session. It is what
    -- a retry after a lost acknowledgement finds the session by: the id was
    -- minted here, so the caller cannot name it, and the key alone lives in the
    -- receipt ledger which does not know what it produced.
    intent_hash          TEXT    NOT NULL
                                 CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    -- `idle` by default and forever unless an explicit archive is authorized.
    disposition          TEXT    NOT NULL CHECK (disposition IN ('idle', 'archive')),
    revision             INTEGER NOT NULL CHECK (revision >= 1),
    created_at           TEXT    NOT NULL
                                 CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    -- One command opens one session. Without this the retry that lost its
    -- answer would be free to open a second workspace for the same request.
    UNIQUE (project_id, intent_hash)
) STRICT;

CREATE INDEX ix_quick_sessions_project ON quick_sessions (project_id);

-- One promotion of one Quick session.
--
-- The row is written *before* the first effect, carrying the ids the effects
-- will use. That ordering is the whole mechanism: a retry after a lost
-- response, a partial failure or a restart reads these ids back and reconciles
-- the same MiniProject, the same nodes and the same seats instead of minting
-- new ones. `completed_at` is what separates "authorized and in progress" from
-- "delivered", so a resumed apply knows which suffix is still missing.
--
-- A Quick session promotes once: the primary key is the source, not the
-- command. A second promotion of the same source would be a second epic
-- claiming the same provenance.
CREATE TABLE quick_session_promotions (
    quick_session_id    TEXT    NOT NULL PRIMARY KEY
                                REFERENCES quick_sessions (id) ON DELETE RESTRICT,
    project_id          TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id     TEXT    NOT NULL CHECK (length(mini_project_id) = 36),
    -- The digest of the plan this apply was authorized against.
    preview_hash        TEXT    NOT NULL
                                CHECK (length(preview_hash) = 64 AND preview_hash NOT GLOB '*[^0-9a-f]*'),
    source_disposition  TEXT    NOT NULL CHECK (source_disposition IN ('idle', 'archive')),
    -- The exact bytes delivered to the LSA, and the seat that received them.
    -- Null until delivery has actually happened: a promotion may not report
    -- success on a handoff it has not placed.
    handoff             TEXT    NULL CHECK (handoff IS NULL OR json_valid(handoff)),
    handoff_hash        TEXT    NULL
                                CHECK (handoff_hash IS NULL OR
                                       (length(handoff_hash) = 64 AND handoff_hash NOT GLOB '*[^0-9a-f]*')),
    lsa_seat_binding_id TEXT    NULL CHECK (lsa_seat_binding_id IS NULL OR length(lsa_seat_binding_id) = 36),
    completed_at        TEXT    NULL
                                CHECK (completed_at IS NULL OR
                                       completed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    created_at          TEXT    NOT NULL
                                CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, mini_project_id)
) STRICT;

-- The roster one epic is staffed from, frozen at promotion.
--
-- Addressed by the epic, because that is what the contract addresses: the
-- roster routes are `/epics/{epic_id}/...`, and an epic that was never promoted
-- from a Quick session still has a roster. Keying this by its source session
-- would leave every other epic unable to answer for its own seats.
--
-- The seats are the resolved document, not a pointer to the project's current
-- Core Team revision. That is the point of freezing: a later project edit
-- publishes a new revision and this row does not move, so the epic keeps
-- reporting the roster it was actually staffed with.
CREATE TABLE epic_rosters (
    project_id       TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    mini_project_id  TEXT    NOT NULL CHECK (length(mini_project_id) = 36),
    core_team_version INTEGER NOT NULL CHECK (core_team_version >= 1),
    catalog_hash     TEXT    NOT NULL
                             CHECK (length(catalog_hash) = 64 AND catalog_hash NOT GLOB '*[^0-9a-f]*'),
    seats            TEXT    NOT NULL CHECK (json_valid(seats)),
    -- The session this epic was promoted from, when it was. Provenance, not
    -- identity: the epic is addressed by its own id everywhere else.
    quick_session_id TEXT    NULL CHECK (quick_session_id IS NULL OR length(quick_session_id) = 36),
    revision         INTEGER NOT NULL CHECK (revision >= 1),
    pinned_at        TEXT    NOT NULL
                             CHECK (pinned_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, mini_project_id)
) STRICT;

-- Widen the closed command-kind list by the four OP-04 commands.
--
-- Same rebuild shape as v24, v28, v29 and v30: `kind` is a CHECK, so a new
-- command is a migration rather than a code change.
CREATE TABLE command_receipts_v31 (
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
                                 'upgrade_epic_roster')),
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

INSERT INTO command_receipts_v31
SELECT id, project_id, idempotency_key, kind, target, target_revision, intent,
       intent_hash, state, correlation, native_identity, result_ref, attempts,
       created_at, updated_at
FROM command_receipts;

DROP TABLE command_receipts;
ALTER TABLE command_receipts_v31 RENAME TO command_receipts;

CREATE INDEX ix_command_receipts_state ON command_receipts (project_id, state);

PRAGMA user_version = 31;
