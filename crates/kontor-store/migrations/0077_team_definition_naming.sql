-- The Team Definition: one immutable revision that owns native hierarchy and
-- naming, the project selection and epic pin that address it, and the durable
-- intent an identity-preserving retitle resumes from.
--
-- `topology_specs` is deliberately still here and deliberately still referenced.
-- A Team Definition names the exact topology revision that *validates* it, so a
-- definition can never ask for a kind, parent or capability the project never
-- legalized; the topology remains a validator and never renders a second name.
-- The reference is by (project, spec, version) rather than by hash so the
-- database enforces the revision exists, while the canonical hash inside the
-- definition document keeps proving it is these exact bytes.

CREATE TABLE team_definitions (
    project_id       TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    definition_id    TEXT    NOT NULL,
    version          INTEGER NOT NULL CHECK (version > 0),
    name             TEXT    NOT NULL,
    topology_spec_id TEXT    NOT NULL,
    topology_version INTEGER NOT NULL CHECK (topology_version > 0),
    definition       TEXT    NOT NULL CHECK (json_valid(definition)),
    definition_hash  TEXT    NOT NULL
                             CHECK (length(definition_hash) = 64
                                    AND definition_hash NOT GLOB '*[^0-9a-f]*'),
    published_at     TEXT    NOT NULL,
    PRIMARY KEY (project_id, definition_id, version),
    FOREIGN KEY (project_id, topology_spec_id, topology_version)
        REFERENCES topology_specs(project_id, spec_id, version) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER team_definitions_are_immutable
BEFORE UPDATE ON team_definitions
BEGIN
    SELECT RAISE(ABORT, 'Team Definition revisions are immutable');
END;

CREATE TRIGGER team_definitions_are_permanent
BEFORE DELETE ON team_definitions
BEGIN
    SELECT RAISE(ABORT, 'published Team Definition revisions are permanent');
END;

-- What *future* epic scopes inherit. Selecting a new default never moves an
-- epic that has already frozen one: that is the separate upgrade authority.
CREATE TABLE project_team_definition_defaults (
    project_id     TEXT    PRIMARY KEY NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    definition_id  TEXT    NOT NULL,
    version        INTEGER NOT NULL CHECK (version > 0),
    canonical_hash TEXT    NOT NULL
                           CHECK (length(canonical_hash) = 64
                                  AND canonical_hash NOT GLOB '*[^0-9a-f]*'),
    selected_at    TEXT    NOT NULL,
    FOREIGN KEY (project_id, definition_id, version)
        REFERENCES team_definitions(project_id, definition_id, version) ON DELETE RESTRICT
) STRICT;

-- One epic's frozen definition. The row can move, exactly once per confirmed
-- upgrade, and only through the migration intent below; it can never move to
-- another epic or project, and it can never be removed, because every container
-- already placed under it cites the revision the epic claims.
CREATE TABLE mini_project_team_definition_snapshots (
    mini_project_id TEXT    PRIMARY KEY NOT NULL,
    project_id      TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    definition_id   TEXT    NOT NULL,
    version         INTEGER NOT NULL CHECK (version > 0),
    canonical_hash  TEXT    NOT NULL
                            CHECK (length(canonical_hash) = 64
                                   AND canonical_hash NOT GLOB '*[^0-9a-f]*'),
    pinned_at       TEXT    NOT NULL,
    -- The epic must belong to the project whose definition it pins. Two
    -- separate references would have allowed one project's revision to be
    -- pinned onto another project's epic.
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, definition_id, version)
        REFERENCES team_definitions(project_id, definition_id, version) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER mini_project_team_definition_snapshots_keep_their_epic
BEFORE UPDATE ON mini_project_team_definition_snapshots
BEGIN
    SELECT RAISE(ABORT, 'a Team Definition pin belongs to its epic and its project')
    WHERE OLD.mini_project_id <> NEW.mini_project_id
       OR OLD.project_id <> NEW.project_id;
END;

CREATE TRIGGER mini_project_team_definition_snapshots_are_permanent
BEFORE DELETE ON mini_project_team_definition_snapshots
BEGIN
    SELECT RAISE(ABORT, 'Team Definition pins are permanent');
END;

-- Project ownership of a topology node, addressable as a foreign key.
--
-- `topology_nodes` has only a single-column primary key, so nothing could
-- previously reference "this node, in this project" as one fact. A migration
-- target has to, or a caller could enumerate a node belonging to someone else.
CREATE UNIQUE INDEX ux_topology_nodes_project_identity
ON topology_nodes (project_id, id);

-- The durable half of an identity-preserving migration.
--
-- SQLite cannot transact atomically with a sequence of external runtime
-- renames, so the intent is recorded *before* the first effect and the epic
-- keeps its old pin for the whole apply. The pin moves only once every target
-- has read back its desired title under an unchanged native identity. A crash
-- between any two renames therefore resumes from this row under the same
-- idempotency key rather than recreating anything, and a partial apply leaves
-- the epic pointing at the definition its natives still render.
CREATE TABLE team_definition_migration_intents (
    id                  TEXT    NOT NULL PRIMARY KEY
                                CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id          TEXT    NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    mini_project_id     TEXT    NOT NULL,
    idempotency_key     TEXT    NOT NULL UNIQUE
                                CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    -- Digest of everything that makes this request the request it is: the epic,
    -- both pins and the exact enumerated target set. Reusing a key with a
    -- different fingerprint is a conflict, not a replay.
    fingerprint         TEXT    NOT NULL
                                CHECK (length(fingerprint) = 64
                                       AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    from_definition_id  TEXT    NULL,
    from_version        INTEGER NULL CHECK (from_version IS NULL OR from_version > 0),
    from_canonical_hash TEXT    NULL
                                CHECK (from_canonical_hash IS NULL
                                       OR (length(from_canonical_hash) = 64
                                           AND from_canonical_hash NOT GLOB '*[^0-9a-f]*')),
    to_definition_id    TEXT    NOT NULL,
    to_version          INTEGER NOT NULL CHECK (to_version > 0),
    to_canonical_hash   TEXT    NOT NULL
                                CHECK (length(to_canonical_hash) = 64
                                       AND to_canonical_hash NOT GLOB '*[^0-9a-f]*'),
    state               TEXT    NOT NULL
                                CHECK (state IN ('recorded', 'applying', 'confirmed', 'failed')),
    recorded_at         TEXT    NOT NULL,
    updated_at          TEXT    NOT NULL,
    -- The epic must belong to the project the migration claims. Referencing
    -- both tables separately would have let a migration pair one project's
    -- authority with another project's epic.
    FOREIGN KEY (project_id, mini_project_id)
        REFERENCES mini_projects(project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, to_definition_id, to_version)
        REFERENCES team_definitions(project_id, definition_id, version) ON DELETE RESTRICT,
    -- The prior pin is recorded whole or not at all: an epic pinned for the
    -- first time has no `from`, and one being upgraded has all three columns.
    CHECK ((from_definition_id IS NULL) = (from_version IS NULL)
           AND (from_definition_id IS NULL) = (from_canonical_hash IS NULL))
) STRICT;

-- The fence. One epic may have at most one migration in flight, so a second
-- apply cannot interleave renames with the first and materialization has one
-- unambiguous answer to "is this epic mid-migration".
CREATE UNIQUE INDEX ux_team_definition_migration_intents_in_flight
ON team_definition_migration_intents (mini_project_id)
WHERE state IN ('recorded', 'applying');

CREATE TRIGGER team_definition_migration_intents_keep_their_scope
BEFORE UPDATE ON team_definition_migration_intents
BEGIN
    SELECT RAISE(ABORT, 'a migration intent keeps its scope, target and key')
    WHERE OLD.project_id <> NEW.project_id
       OR OLD.mini_project_id <> NEW.mini_project_id
       OR OLD.idempotency_key <> NEW.idempotency_key
       OR OLD.fingerprint <> NEW.fingerprint
       OR OLD.to_definition_id <> NEW.to_definition_id
       OR OLD.to_version <> NEW.to_version
       OR OLD.to_canonical_hash <> NEW.to_canonical_hash
       OR OLD.state IN ('confirmed', 'failed');
END;

CREATE TRIGGER team_definition_migration_intents_are_permanent
BEFORE DELETE ON team_definition_migration_intents
BEGIN
    SELECT RAISE(ABORT, 'migration intents are evidence and are not deletable');
END;

-- One native object the intent must retitle, and what was actually observed.
--
-- The key is a stable `target_key`, not the topology node. A node is not one
-- native object: an ECP node carries its own container *and* the LSA and TPM
-- seats inside it, and a CSW node carries its container plus SEAT A, SEAT B and
-- JUDGE. Keying by node would have silently collapsed every seat on a node into
-- one row and dropped the rest from the migration.
--
-- Identity is the four-part native identity, not a bare id: a native id means
-- nothing on its own, and matching on a subset of it matches a container a
-- restart has already replaced. The desired parent, kind and cwd are recorded
-- at preview and re-proved at readback, so "the title changed" can never be
-- confused with "the object was replaced by one that happens to have the title".
CREATE TABLE team_definition_migration_targets (
    intent_id                 TEXT    NOT NULL
                                      REFERENCES team_definition_migration_intents(id)
                                      ON DELETE RESTRICT,
    project_id                TEXT    NOT NULL,
    target_key                TEXT    NOT NULL CHECK (length(target_key) BETWEEN 1 AND 128),
    subject_kind              TEXT    NOT NULL CHECK (subject_kind IN ('container', 'seat')),
    topology_node_id          TEXT    NOT NULL,
    seat_binding_id           TEXT    NULL,
    runtime_kind              TEXT    NOT NULL,
    native_host               TEXT    NOT NULL,
    native_generation         INTEGER NOT NULL CHECK (native_generation >= 0),
    native_id                 TEXT    NOT NULL,
    desired_title             TEXT    NOT NULL,
    desired_parent_native_id  TEXT    NULL,
    desired_kind              TEXT    NOT NULL
                                      CHECK (desired_kind IN ('project_container',
                                                              'workspace_container', 'seat')),
    desired_cwd               TEXT    NULL,
    observed_title            TEXT    NULL,
    observed_parent_native_id TEXT    NULL,
    observed_kind             TEXT    NULL
                                      CHECK (observed_kind IS NULL
                                             OR observed_kind IN ('project_container',
                                                                  'workspace_container', 'seat')),
    observed_cwd              TEXT    NULL,
    state                     TEXT    NOT NULL CHECK (state IN (
                                      'pending', 'unchanged', 'renamed',
                                      'rename_pending', 'failed')),
    updated_at                TEXT    NOT NULL,
    PRIMARY KEY (intent_id, target_key),
    -- A seat target names its seat; a container target has none to name.
    CHECK ((subject_kind = 'seat') = (seat_binding_id IS NOT NULL)),
    -- And the object kind has to agree with the subject: a seat recorded as a
    -- container, or the reverse, would make the readback prove the wrong thing.
    CHECK ((subject_kind = 'seat') = (desired_kind = 'seat')),
    -- A native root has no parent; a workspace or seat sits inside one, and a
    -- seat's container id is part of proving the seat is still that seat.
    CHECK ((desired_kind = 'project_container') = (desired_parent_native_id IS NULL)),
    -- One native object is one target, however it was addressed.
    UNIQUE (intent_id, runtime_kind, native_host, native_generation, native_id),
    -- A success state is a readback, not a label. `renamed` and `unchanged`
    -- require the exact desired title and unchanged placement; anything else
    -- stays pending or failed and cannot confirm the migration.
    CHECK (state NOT IN ('renamed', 'unchanged')
           OR (observed_title = desired_title
               AND observed_kind IS desired_kind
               AND observed_parent_native_id IS desired_parent_native_id
               AND observed_cwd IS desired_cwd)),
    -- The node has to be one this project owns.
    FOREIGN KEY (project_id, topology_node_id)
        REFERENCES topology_nodes(project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX ix_team_definition_migration_targets_node
ON team_definition_migration_targets (intent_id, topology_node_id);

CREATE TRIGGER team_definition_migration_targets_keep_their_identity
BEFORE UPDATE ON team_definition_migration_targets
BEGIN
    SELECT RAISE(ABORT, 'a migration target keeps its subject, native identity and desired state')
    WHERE OLD.native_id <> NEW.native_id
       OR OLD.runtime_kind <> NEW.runtime_kind
       OR OLD.native_host <> NEW.native_host
       OR OLD.native_generation <> NEW.native_generation
       OR OLD.desired_title <> NEW.desired_title
       OR OLD.desired_kind <> NEW.desired_kind
       OR OLD.desired_parent_native_id IS NOT NEW.desired_parent_native_id
       OR OLD.desired_cwd IS NOT NEW.desired_cwd
       OR OLD.subject_kind <> NEW.subject_kind
       OR OLD.topology_node_id <> NEW.topology_node_id
       OR OLD.project_id <> NEW.project_id
       OR OLD.seat_binding_id IS NOT NEW.seat_binding_id;
END;

CREATE TRIGGER team_definition_migration_targets_are_permanent
BEFORE DELETE ON team_definition_migration_targets
BEGIN
    SELECT RAISE(ABORT, 'migration targets are evidence and are not deletable');
END;

-- The consultation topic the ASW/CSW name templates render.
--
-- Nullable, and deliberately so. Every consultation recorded before this
-- generation has only its full question, and the naming contract forbids
-- deriving a topic from a question, profile, title, UUID or AI label. A NULL is
-- therefore an honest "no authoritative topic exists", not a defect to be
-- repaired by inference: those rows stay readable, keep their historical names,
-- and a migration that needs to render one of them fails closed until an
-- operator supplies the mapping. New invocations carry a topic from the start.
ALTER TABLE consultation_runs ADD COLUMN topic TEXT NULL
    CHECK (topic IS NULL OR length(topic) BETWEEN 1 AND 512);

-- Where a legacy consultation's topic came from.
--
-- The naming contract forbids deriving a topic from a question, profile, title,
-- UUID or AI label, so the only lawful source for a consultation recorded
-- before topics existed is an operator who states it. That makes the supplied
-- value a decision rather than data, and a decision needs provenance: which
-- migration carried it, and when.
--
-- Separate from `consultation_runs` on purpose. The topic belongs to the
-- consultation and is read on every render; the provenance belongs to the
-- migration that supplied it and is evidence. Keeping them in one row would
-- have made the evidence editable by anything that updates a consultation.
CREATE TABLE consultation_topic_migration_provenance (
    project_id  TEXT NOT NULL,
    run_id      TEXT NOT NULL,
    intent_id   TEXT NOT NULL
                     REFERENCES team_definition_migration_intents(id) ON DELETE RESTRICT,
    topic       TEXT NOT NULL CHECK (length(topic) BETWEEN 1 AND 512),
    supplied_at TEXT NOT NULL,
    PRIMARY KEY (project_id, run_id),
    FOREIGN KEY (project_id, run_id)
        REFERENCES consultation_runs(project_id, run_id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER consultation_topic_provenance_is_immutable
BEFORE UPDATE ON consultation_topic_migration_provenance
BEGIN
    SELECT RAISE(ABORT, 'a supplied consultation topic keeps the provenance it was supplied under');
END;

CREATE TRIGGER consultation_topic_provenance_is_permanent
BEFORE DELETE ON consultation_topic_migration_provenance
BEGIN
    SELECT RAISE(ABORT, 'consultation topic provenance is evidence and is not deletable');
END;

-- Widen the closed command-kind list by the three Team Definition commands.
-- Same rebuild shape as v29, v56 and v68, and for the same reason: `kind` is a
-- CHECK, so a new command is a migration rather than a code change.
CREATE TABLE command_receipts_v77 (
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
        'select_task_team','select_task_account','reconcile_ticket','materialize_jira',
        'activate_asma_epic','settle_runtime','submit_intake','pull_ticket_comments',
        'claim_ticket','replace_seat','refresh_capacity','override_availability','observe_seat',
        'retire_seat','publish_topology_spec','select_project_topology','upgrade_topology',
        'retitle_container','reconcile_native_names','apply_core_team','ensure_quick_session',
        'promote_quick_session','materialize_core_team','correct_core_team_route',
        'claim_core_team_seat','upgrade_epic_roster','apply_advisor_profile',
        'apply_committee_template','apply_completion_profile','advance_completion',
        'remediate_completion','invoke_advisor_run','settle_advisor_run','invoke_committee_run',
        'record_committee_findings','settle_committee_run','recover_consultation_seat',
        'reroute_unmaterialized_consultation_seat','publish_trigger','install_workflow_spec',
        'withdraw_task','publish_team_definition','select_project_team_definition',
        'upgrade_team_definition')),
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
INSERT INTO command_receipts_v77 SELECT * FROM command_receipts;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v77 RENAME TO command_receipts;
CREATE INDEX ix_command_receipts_state ON command_receipts(project_id, state);
CREATE TRIGGER command_receipts_identity_immutable BEFORE UPDATE ON command_receipts
WHEN OLD.idempotency_key <> NEW.idempotency_key OR OLD.target <> NEW.target
  OR OLD.intent <> NEW.intent OR OLD.intent_hash <> NEW.intent_hash
  OR OLD.kind <> NEW.kind OR OLD.project_id <> NEW.project_id
  OR OLD.state IN ('confirmed','failed')
BEGIN SELECT RAISE(ABORT, 'a command receipt identity is immutable'); END;
CREATE TRIGGER command_receipts_no_delete BEFORE DELETE ON command_receipts
BEGIN SELECT RAISE(ABORT, 'command receipts are not deletable'); END;

PRAGMA user_version = 77;
