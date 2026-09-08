-- Schema v94. Kontor observed a Jira issue's status and never its body.
--
-- The consequence was not a missing feature but a wrong answer: reconciliation
-- reported `converged` with an empty diff while the issue's description still
-- held only the marker Kontor stamped at creation,
-- `Kontor <kind> <uuid>: <title>`. Five epics reached readers in exactly that
-- state, each publishing an internal Kontor UUID as its reader-facing text.
--
-- Two facts are recorded here, and they answer different questions.
--
--   1. The observed body joins the immutable observation, so a content conflict
--      cites the body Kontor actually saw rather than a body re-fetched later
--      and possibly changed in between.
--   2. Every description Kontor publishes is appended to a ledger. Without it
--      Kontor cannot tell its own stale projection from somebody else's edit,
--      and a repair path that cannot tell those apart is a path that
--      overwrites human-authored content.
--
-- Both are additive in the sense migration 0002 fixed: nullable, no default and
-- no backfill. An observation written before this version genuinely carries no
-- body evidence, and inventing an empty one for it would manufacture the exact
-- fact the reconciliation reads.

-- ---------------------------------------------------------------------------
-- 1. Two new command kinds
-- ---------------------------------------------------------------------------

-- Widen the closed command-kind list. The table shape is the v90 shape plus the
-- two publication commands this generation introduces.
--
-- They are deliberately not `reconcile_ticket`: reconciliation moves a status
-- and writes no body, and a body publication moves no status. One kind for both
-- would leave an audit unable to answer which of the two a receipt authorized,
-- which matters most in the case this whole generation exists for — replacing
-- what a human reader sees.
CREATE TABLE command_receipts_v94 (
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
        'upgrade_team_definition','correct_epic_backlog_code','recover_topology_container',
        'recover_gate_rejection','publish_ticket_description','publish_epic_description')),
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
INSERT INTO command_receipts_v94 SELECT * FROM command_receipts;
-- Migration 0092 put a `command_receipts` subquery inside a trigger body on
-- another table. Modern `ALTER TABLE ... RENAME` reparses the whole schema, so
-- between the drop and the rename that trigger names a table which does not
-- exist yet and the reparse fails. Legacy rename semantics do not reparse, and
-- are correct here for exactly that reason: the trigger already spells the name
-- this rename is about to restore, so there is nothing to rewrite.
PRAGMA legacy_alter_table = ON;
DROP TABLE command_receipts;
ALTER TABLE command_receipts_v94 RENAME TO command_receipts;
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

-- ---------------------------------------------------------------------------
-- 2. Observed body evidence
-- ---------------------------------------------------------------------------

-- Whether the connector reported a body field at all.
--
-- Distinct from an empty `description_text` on purpose. Jira returning `null`
-- for `description` is evidence the issue's body is empty; the connector never
-- reporting the field is the absence of evidence. Collapsing the two is how an
-- empty body reads as agreement.
ALTER TABLE external_ticket_observations ADD COLUMN description_present INTEGER NULL
    CHECK (description_present IS NULL OR description_present IN (0, 1));

-- Digest of the exact external body document.
--
-- Divergence is decided on this, not on the rendered text: two different
-- documents can render to the same plain text, and adopting one for the other
-- would lose the reader's actual formatting.
ALTER TABLE external_ticket_observations ADD COLUMN description_hash TEXT NULL
    CHECK (description_hash IS NULL
           OR (length(description_hash) = 64 AND description_hash NOT GLOB '*[^0-9a-f]*'));

-- The body rendered to plain text, bounded as evidence.
--
-- Kept so a refusal is readable without a second fetch, and so emptiness is
-- judged on what a human would read rather than on document structure.
ALTER TABLE external_ticket_observations ADD COLUMN description_text TEXT NULL
    CHECK (description_text IS NULL OR length(description_text) <= 65536);

-- The three columns are one fact, so a half-written body is not a state this
-- schema can hold. `ALTER TABLE` cannot add a table-level CHECK, so the rule is
-- a trigger, exactly as v1 and v4 do wherever SQLite cannot express one.
CREATE TRIGGER observation_description_is_whole
BEFORE INSERT ON external_ticket_observations
WHEN (NEW.description_present IS NULL) <> (NEW.description_hash IS NULL)
  OR (NEW.description_present IS NULL) <> (NEW.description_text IS NULL)
BEGIN SELECT RAISE(ABORT, 'an observed description is present, hashed and rendered together'); END;

-- ---------------------------------------------------------------------------
-- 3. Published description ledger
-- ---------------------------------------------------------------------------

-- One row per description Kontor successfully wrote and read back.
--
-- Append-only, and deliberately not a desired-state table. Kontor does not hold
-- an epic's authored body as state it re-asserts: the author supplies it, Kontor
-- publishes it once and remembers the exact digest it published. A later
-- observation equal to that digest is Kontor's own projection going stale; an
-- observation equal to nothing Kontor ever published is a human's edit, and is
-- reported rather than overwritten.
CREATE TABLE ticket_description_publications (
    id                 TEXT NOT NULL PRIMARY KEY
                            CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id         TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    -- An epic and a task are separate aggregates with separate authority, and
    -- one column of ids for both would let an epic body be attributed to a task.
    subject_kind       TEXT NOT NULL CHECK (subject_kind IN ('epic', 'task')),
    subject_id         TEXT NOT NULL
                            CHECK (length(subject_id) = 36 AND subject_id NOT GLOB '*[^0-9a-f-]*'),
    external_issue_key TEXT NOT NULL CHECK (length(external_issue_key) BETWEEN 1 AND 256),
    -- Digest of the document Kontor wrote, computed exactly as an observed body
    -- is, so the two are comparable without re-deriving either.
    body_hash          TEXT NOT NULL
                            CHECK (length(body_hash) = 64 AND body_hash NOT GLOB '*[^0-9a-f]*'),
    body_text          TEXT NOT NULL CHECK (length(body_text) BETWEEN 1 AND 65536),
    -- The observed digest this publication replaced, when one was observed.
    -- This is what makes a repair auditable: it names what the reader had before.
    replaced_hash      TEXT NULL
                            CHECK (replaced_hash IS NULL
                                   OR (length(replaced_hash) = 64 AND replaced_hash NOT GLOB '*[^0-9a-f]*')),
    receipt_id         TEXT NOT NULL
                            CHECK (length(receipt_id) = 36 AND receipt_id NOT GLOB '*[^0-9a-f-]*'),
    published_at       TEXT NOT NULL
                            CHECK (published_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id)
) STRICT;

CREATE INDEX ix_description_publications_subject
    ON ticket_description_publications (project_id, subject_kind, subject_id, published_at);

CREATE TRIGGER description_publication_no_update
BEFORE UPDATE ON ticket_description_publications
BEGIN SELECT RAISE(ABORT, 'a published description is immutable evidence'); END;

CREATE TRIGGER description_publication_no_delete
BEFORE DELETE ON ticket_description_publications
BEGIN SELECT RAISE(ABORT, 'a published description is not deletable'); END;

PRAGMA user_version = 94;
