-- ===========================================================================
-- Schema v4 — durable scheduler leases and admission evidence (KON-MVP-09)
--
-- Two append-only tables, seven added columns on `resource_leases`, and the
-- realm-wide uniqueness the v1 index could not express. Every rule schema v1
-- states still holds: STRICT tables, composite project-scoped foreign keys,
-- canonical JSON with its digest beside it, and BEFORE UPDATE/DELETE triggers so
-- direct SQL cannot rewrite what happened.
--
-- Five decisions worth stating, because each one had a tempting alternative.
--
--  1. **`resource_leases` is extended, not replaced.** v1 already models a lease
--     over a contended resource with a receipt-backed release, and things
--     already reference it. A second "scheduler lease" table would mean two
--     models of the same claim, and the first cross-project collision would be
--     the one nobody's index covered. So this migration adds the durability the
--     scheduler needs — an expiry, a fencing token, a holder and a kind — to the
--     table that already exists.
--
--  2. **Module contention is realm-wide.** v1's `ux_resource_leases_active` is
--     keyed on `(project_id, resource_key, …)`, so two projects in one Realm can
--     each hold `directory/asma-app-directory` and neither insert violates
--     anything. That is precisely the collision a lease exists to prevent: the
--     module is a place on disk, and disk does not know about project rows. The
--     v1 index is therefore dropped and replaced by two realm-wide partial
--     indexes. Dropping an index loses no data and no history; leaving it in
--     place would have left a narrower rule shadowing the correct one.
--
--  3. **Expiry is not release.** v1 binds `released_at` to a release receipt in
--     a CHECK, and rightly: a release is somebody's decision. An expiry is
--     nobody's — it is the absence of a renewal — and SQLite cannot widen a
--     v1 CHECK through `ALTER TABLE` anyway. So expiry gets its own column, an
--     active lease is one with neither set, and the two reasons a lease stopped
--     being active stay distinguishable forever. An expiry frees the *resource*
--     and says nothing whatever about the run that held it: nothing in the
--     expiry path touches `agent_runs`, and a lost lease is never a verdict.
--
--  4. **A renewal rotates in place.** The alternative — insert a successor lease
--     and retire its predecessor — needs a third terminal state ("superseded")
--     that is neither a release nor an expiry, and every uniqueness rule below
--     would have to learn about it. Rotating `fencing_token` and `expires_at` on
--     the row instead keeps one lease per claim for the claim's whole life,
--     which is also the only shape in which "the stale holder's token no longer
--     matches" is a checkable statement. `renewed_from_lease_id` therefore
--     records *reclaim lineage*: the expired lease a fresh acquisition took the
--     resource over from, which is the one link a renewal in place cannot show.
--
--  5. **There is no `lease_scope` column.** The scope of a lease is a function
--     of its kind — a module and a worktree are both realm-wide places on disk —
--     and a column that must always agree with another column is a column that
--     can disagree with it. The kind is stored; the scope is what the indexes
--     below enforce.
-- ===========================================================================

-- ---------------------------------------------------------------------------
-- 1. Durability on the existing lease
-- ---------------------------------------------------------------------------

-- What kind of place is being claimed.
--
-- `resource_key` is one realm-wide namespace, exactly as it was in v1 — the kind
-- says how to read a key, it does not partition it. A module key and a worktree
-- label are spelled differently, so they cannot collide by accident; and if a
-- deployment ever made them collide, refusing the overlap is the safe answer.
--
-- Nullable with no default and no backfill, like every column migration 0002
-- added: a v1 lease row was written before this distinction existed, and
-- inventing `'module'` for it would fabricate the fact the scheduler reads.
ALTER TABLE resource_leases ADD COLUMN lease_kind TEXT NULL
    CHECK (lease_kind IS NULL OR lease_kind IN ('module', 'worktree'));

-- When the claim lapses unless it is renewed.
ALTER TABLE resource_leases ADD COLUMN expires_at TEXT NULL
    CHECK (expires_at IS NULL
           OR expires_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z');

-- When the claim was found lapsed, and by whom it was recorded as such.
--
-- Separate from `released_at` by decision 3. A lease with `expired_at` set is
-- not active, so its resource is reclaimable — and the reclaim is a new lease,
-- never a resurrection of this one.
ALTER TABLE resource_leases ADD COLUMN expired_at TEXT NULL
    CHECK (expired_at IS NULL
           OR expired_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z');

-- The monotonic token that makes a stale holder harmless.
--
-- It starts at 1 and every renewal increments it. A holder that comes back after
-- its lease was renewed by someone else — or reclaimed after expiry — presents
-- the token it remembers, and the token it remembers is no longer the one on the
-- row, so its renewal and its release both fail. Without this, a process that
-- was asleep for the whole lease duration could release a lease that now belongs
-- to different work.
ALTER TABLE resource_leases ADD COLUMN fencing_token INTEGER NULL
    CHECK (fencing_token IS NULL OR fencing_token >= 1);

-- The expired lease this one took the resource over from.
--
-- Reclaim lineage, not renewal history: a renewal rotates the token on the row
-- it renews (decision 4), so the only lease-to-lease link that needs recording
-- is the one across an expiry. An audit reading a chain of these sees exactly
-- how many times a resource was reclaimed without anyone releasing it.
ALTER TABLE resource_leases ADD COLUMN renewed_from_lease_id TEXT NULL
    CHECK (renewed_from_lease_id IS NULL
           OR (length(renewed_from_lease_id) = 36
               AND renewed_from_lease_id NOT GLOB '*[^0-9a-f-]*'));

-- Which scheduler instance holds the claim.
--
-- The run is already on the row and is what the work *is*; this is who is
-- responsible for renewing on its behalf. Two scheduler instances over one
-- database is the case durable leases exist for, and after a restart the
-- surviving instance needs to be able to tell "my lease" from "the one the
-- process that died was holding" without guessing from timestamps.
ALTER TABLE resource_leases ADD COLUMN holder_instance TEXT NULL
    CHECK (holder_instance IS NULL
           OR (length(holder_instance) BETWEEN 1 AND 256 AND holder_instance NOT GLOB '* *'));

-- The admission that acquired it, once one exists.
--
-- `ALTER TABLE` cannot add a composite foreign key, and a single-column one
-- would let a globally valid id from another project resolve. The project-scoped
-- binding is the trigger in section 5, the same mechanism v1 and v3 use wherever
-- SQLite cannot express a rule as a constraint.
ALTER TABLE resource_leases ADD COLUMN admission_event_id TEXT NULL
    CHECK (admission_event_id IS NULL
           OR (length(admission_event_id) = 36
               AND admission_event_id NOT GLOB '*[^0-9a-f-]*'));

-- ---------------------------------------------------------------------------
-- 2. Realm-wide contention
-- ---------------------------------------------------------------------------

-- v1's project-local index, replaced by decision 2. It could not prevent a
-- cross-project collision and it did not know about expiry, so it would have
-- gone on refusing a legitimate reclaim while admitting the overlap that
-- matters.
DROP INDEX ux_resource_leases_active;

-- One holder per resource, across the whole Realm, when nothing isolates it.
CREATE UNIQUE INDEX ux_resource_leases_unisolated
    ON resource_leases (resource_key)
    WHERE released_at IS NULL AND expired_at IS NULL AND worktree_key IS NULL;

-- One holder per (resource, worktree), across the whole Realm.
--
-- Two tasks may hold one module simultaneously *only* through distinct
-- worktrees, and then each pair is claimed exactly once. A duplicate worktree
-- identity collides here rather than being admitted as isolation.
CREATE UNIQUE INDEX ux_resource_leases_isolated
    ON resource_leases (resource_key, worktree_key)
    WHERE released_at IS NULL AND expired_at IS NULL AND worktree_key IS NOT NULL;

-- Isolation is all-or-nothing, and no index can say so.
--
-- The two indexes above are each internally complete but blind to each other: an
-- unisolated claim on `m` and an isolated claim on `(m, tree-a)` violate neither,
-- and yet they are two tasks editing one module with only one of them in a
-- separate tree. So the exclusion is a trigger. An active unisolated holder
-- excludes every contender, and an isolated holder excludes an unisolated
-- contender and any contender naming the same tree.
--
-- This is the rule `kontor_policy::module_isolated_by_worktree` states for a
-- guardrail evaluation, restated where a caller that never consulted a guardrail
-- still cannot get past it.
CREATE TRIGGER resource_leases_isolation_exclusive BEFORE INSERT ON resource_leases
WHEN EXISTS (SELECT 1 FROM resource_leases
             WHERE released_at IS NULL
               AND expired_at IS NULL
               AND resource_key = NEW.resource_key
               AND (worktree_key IS NULL
                    OR NEW.worktree_key IS NULL
                    OR worktree_key = NEW.worktree_key))
BEGIN SELECT RAISE(ABORT, 'this resource is already claimed by an active lease'); END;

-- ---------------------------------------------------------------------------
-- 3. Lease history
-- ---------------------------------------------------------------------------

-- Everything that ever happened to one lease, in order.
--
-- Append-only, including the events that freed the resource. The fencing token
-- in force at each event is recorded with it, so a reader can reconstruct which
-- holder was authoritative at any point without inferring it from timestamps.
--
-- A release names the receipt that decided it. An expiry names none, and the
-- CHECK below makes that structural rather than conventional: no operator
-- decided an expiry, so a row claiming one had a receipt is refused.
CREATE TABLE lease_events (
    project_id    TEXT    NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    lease_id      TEXT    NOT NULL,
    sequence      INTEGER NOT NULL CHECK (sequence >= 1),
    event         TEXT    NOT NULL CHECK (event IN
                              ('acquired', 'renewed', 'released', 'expired')),
    fencing_token INTEGER NOT NULL CHECK (fencing_token >= 1),
    receipt_id    TEXT    NULL,
    occurred_at   TEXT    NOT NULL
                          CHECK (occurred_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, lease_id, sequence),
    CHECK ((event = 'released') = (receipt_id IS NOT NULL)),
    FOREIGN KEY (project_id, lease_id)
        REFERENCES resource_leases (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT
) STRICT;

-- A lease is acquired once. A second `acquired` row would mean two acquisitions
-- of one lease id, which is how a replayed admission stops being detectable.
CREATE UNIQUE INDEX ux_lease_events_acquired
    ON lease_events (project_id, lease_id) WHERE event = 'acquired';

-- A lease stops being active once. Both terminal events are covered by one
-- index because they are alternatives, not a sequence: a released lease never
-- expires and an expired one is never released.
CREATE UNIQUE INDEX ux_lease_events_terminal
    ON lease_events (project_id, lease_id) WHERE event IN ('released', 'expired');

-- ---------------------------------------------------------------------------
-- 4. Admission evidence
-- ---------------------------------------------------------------------------

-- One immutable scheduling decision about one task.
--
-- This is the canonical record of *why* work started, or why it did not:
-- `evidence` is the canonical decision document — the ordering inputs the
-- candidate was sorted on, the capacity snapshot it fitted into, and the
-- authorization, calendar, runtime, account and intake evidence each gate was
-- decided against — stored byte-for-byte with its digest, so a reviewer can
-- re-run the decision against the stored bytes rather than re-reading a summary
-- of it.
--
-- The relational columns beside it are not a second copy of that document.
-- They are the references that need composite foreign keys, because a decision
-- that names a run or a receipt from another project is not a decision about
-- anything: `evidence` is checkable, and these are enforceable.
--
-- **The leases are deliberately not among them.** A lease names the admission
-- that acquired it (`resource_leases.admission_event_id`), and this table does
-- not name the lease back. Both directions would be a foreign-key cycle:
-- whichever row is inserted first would reference one that does not exist yet,
-- and the only ways out are deferring foreign keys for the whole transaction or
-- inserting a lease and then updating it — one turns a guard off, the other makes
-- an admission a two-step write that a crash can land between. One enforced
-- direction says the same thing: the leases an admission took are
-- `SELECT id FROM resource_leases WHERE admission_event_id = …`, and the
-- immutability triggers mean that answer never changes.
--
-- An admission and a rejection are one table because they are one decision with
-- two outcomes, and the CHECKs below make each outcome's shape complete:
-- a rejection names a code and nothing it did not do, an admission names the
-- run, the launch receipt and the team it created and no code.
CREATE TABLE scheduler_admission_events (
    id                TEXT NOT NULL PRIMARY KEY
                           CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id        TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    task_id           TEXT NOT NULL,
    decision          TEXT NOT NULL CHECK (decision IN ('admitted', 'rejected')),
    -- The Rust `RejectionCode` is the closed domain; SQL enforces the lexical
    -- shape rather than repeating every spelling a later revision would have to
    -- migrate. An unknown code is refused on the way back in by `parse`.
    rejection_code    TEXT NULL
                           CHECK (rejection_code IS NULL
                                  OR (length(rejection_code) BETWEEN 1 AND 128
                                      AND rejection_code NOT GLOB '*[^a-z0-9_]*')),
    team_run_id       TEXT NULL,
    agent_run_id      TEXT NULL,
    launch_receipt_id TEXT NULL,
    -- The authorization that armed the work. An admission always has one:
    -- "unrestricted" is a calendar answer, never an authorization answer.
    authorization_id  TEXT NULL,
    evidence          TEXT NOT NULL CHECK (json_valid(evidence)),
    evidence_hash     TEXT NOT NULL
                           CHECK (length(evidence_hash) = 64
                                  AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    decided_at        TEXT NOT NULL
                           CHECK (decided_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    UNIQUE (project_id, id),
    CHECK ((decision = 'rejected') = (rejection_code IS NOT NULL)),
    CHECK ((decision = 'admitted') = (team_run_id IS NOT NULL)),
    CHECK ((decision = 'admitted') = (agent_run_id IS NOT NULL)),
    CHECK ((decision = 'admitted') = (launch_receipt_id IS NOT NULL)),
    CHECK ((decision = 'admitted') = (authorization_id IS NOT NULL)),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, team_run_id)
        REFERENCES team_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, agent_run_id)
        REFERENCES agent_runs (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, launch_receipt_id)
        REFERENCES command_receipts (project_id, id) ON DELETE RESTRICT,
    FOREIGN KEY (project_id, authorization_id)
        REFERENCES execution_authorizations (project_id, id) ON DELETE RESTRICT
) STRICT;

-- One admission per run. This is the structural half of "a lost acknowledgement
-- produces one durable admission, not two": a replay that reached this table
-- again would have to name the same run, and it cannot insert a second row for
-- it.
CREATE UNIQUE INDEX ux_scheduler_admission_events_run
    ON scheduler_admission_events (project_id, agent_run_id)
    WHERE agent_run_id IS NOT NULL;

CREATE INDEX ix_scheduler_admission_events_task
    ON scheduler_admission_events (project_id, task_id, decided_at);

-- The other half of the one enforced direction: which places an admission took.
CREATE INDEX ix_resource_leases_admission
    ON resource_leases (project_id, admission_event_id)
    WHERE admission_event_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- 5. Project-scoped bindings `ALTER TABLE` cannot express
-- ---------------------------------------------------------------------------

CREATE TRIGGER resource_leases_admission_in_project
BEFORE INSERT ON resource_leases
WHEN NEW.admission_event_id IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM scheduler_admission_events
                     WHERE project_id = NEW.project_id AND id = NEW.admission_event_id)
BEGIN SELECT RAISE(ABORT, 'a lease names an admission from another project'); END;

-- Reclaim lineage is Realm-wide, and this is the one reference in this schema
-- that deliberately is not project-scoped.
--
-- It follows from decision 2. Contention is Realm-wide, so the lapsed lease
-- blocking a place is very often another project's — that is the whole case
-- durable leases exist for. A project-scoped check here would refuse exactly the
-- reclaim it is supposed to record, and the alternative (recording no lineage
-- when the predecessor is another project's) would drop the link in precisely the
-- case an audit most wants it.
--
-- `resource_leases.id` is the primary key, so the lookup is still exact: a lease
-- id this database does not have resolves to nothing, and there is no
-- cross-database attach for one to come from.
CREATE TRIGGER resource_leases_reclaim_exists
BEFORE INSERT ON resource_leases
WHEN NEW.renewed_from_lease_id IS NOT NULL
     AND NOT EXISTS (SELECT 1 FROM resource_leases WHERE id = NEW.renewed_from_lease_id)
BEGIN SELECT RAISE(ABORT, 'a lease reclaims a lease this Realm does not have'); END;

-- A lease never reclaims itself.
CREATE TRIGGER resource_leases_reclaim_not_self
BEFORE INSERT ON resource_leases
WHEN NEW.renewed_from_lease_id IS NOT NULL AND NEW.renewed_from_lease_id = NEW.id
BEGIN SELECT RAISE(ABORT, 'a lease cannot reclaim itself'); END;

-- ---------------------------------------------------------------------------
-- 6. Immutability and the renewal rules
-- ---------------------------------------------------------------------------

CREATE TRIGGER lease_events_no_update BEFORE UPDATE ON lease_events
BEGIN SELECT RAISE(ABORT, 'a lease event is immutable'); END;
CREATE TRIGGER lease_events_no_delete BEFORE DELETE ON lease_events
BEGIN SELECT RAISE(ABORT, 'a lease event is immutable'); END;

CREATE TRIGGER scheduler_admission_events_no_update
BEFORE UPDATE ON scheduler_admission_events
BEGIN SELECT RAISE(ABORT, 'an admission decision is immutable'); END;
CREATE TRIGGER scheduler_admission_events_no_delete
BEFORE DELETE ON scheduler_admission_events
BEGIN SELECT RAISE(ABORT, 'an admission decision is immutable'); END;

-- A lease advances only the way a lease is allowed to advance.
--
-- v1's `resource_leases_release_immutable` already freezes a released lease and
-- pins `resource_key` and `agent_run_id`. What it cannot know about is expiry
-- and the fencing token, so this trigger covers the rest:
--
-- * an expired lease is as final as a released one — the resource is reclaimed
--   by a *new* lease, never by reviving this row;
-- * a lease that stays active is being renewed, and a renewal advances the
--   token by exactly one and moves the expiry forward. Both halves matter: a
--   renewal that did not rotate the token would leave a stale holder
--   authoritative, and one that did not extend the expiry would be a no-op that
--   spent a token;
-- * the columns that say *which claim this is* never change, in any update.
--
-- Ending a lease is held to the opposite rule: a release and an expiry each
-- *freeze* the token and the expiry rather than moving them. Without that clause
-- one statement could end a lease and rotate its token at the same time, and the
-- token it ended on — the one every later reader judges a stale holder against —
-- would be a value nothing ever recorded.
CREATE TRIGGER resource_leases_advance_rules BEFORE UPDATE ON resource_leases
WHEN OLD.expired_at IS NOT NULL
     OR OLD.lease_kind IS NOT NEW.lease_kind
     OR OLD.worktree_key IS NOT NEW.worktree_key
     OR OLD.holder_instance IS NOT NEW.holder_instance
     OR OLD.acquired_at IS NOT NEW.acquired_at
     OR OLD.renewed_from_lease_id IS NOT NEW.renewed_from_lease_id
     OR OLD.admission_event_id IS NOT NEW.admission_event_id
     OR NEW.fencing_token < OLD.fencing_token
     OR (NEW.released_at IS NULL AND NEW.expired_at IS NULL
         AND (NEW.fencing_token IS NOT OLD.fencing_token + 1
              OR NEW.expires_at <= OLD.expires_at))
     OR ((NEW.released_at IS NOT NULL OR NEW.expired_at IS NOT NULL)
         AND (NEW.fencing_token IS NOT OLD.fencing_token
              OR NEW.expires_at IS NOT OLD.expires_at))
BEGIN SELECT RAISE(ABORT, 'a lease advances only by renewal, release or expiry'); END;

-- A lease changes only *because its history says so*.
--
-- The rules above constrain the **shape** of a change. They cannot say that the
-- change was recorded, and that is the half that matters for evidence: without
-- this trigger a direct
-- `UPDATE resource_leases SET released_at = …, release_receipt_id = …` leaves a
-- lease that says it was given up with nothing in `lease_events` accounting for
-- it — and `lease_events` is exactly where an audit reads which holder was
-- authoritative when.
--
-- A blanket no-update trigger is not available here and would be the wrong shape
-- if it were: unlike the evidence tables, a lease legitimately advances — that is
-- what renewal, release and expiry are. Freezing it would make a claim
-- unreleasable.
--
-- So the update is bound to the history instead of forbidden, exactly as
-- migration 0003's `recovery_episodes_require_step` binds an episode to its
-- steps. Each of the three transitions must find its own event already appended:
--
-- * a **release** needs a `released` row at the token the lease is ending on and
--   at the instant it records;
-- * an **expiry** needs an `expired` row, likewise;
-- * anything else is a **renewal**, and needs a `renewed` row at the token the
--   update is moving *to*. Matching on the new token is what makes it exact:
--   every renewal has a token no other renewal of this lease has, so an appended
--   event can neither be reused by a second rotation nor satisfied by the
--   `acquired` row.
--
-- Together with the shape rules, the token on a lease is therefore always the
-- token of that lease's newest logged event — for the store service and for raw
-- SQL alike.
--
-- The store service consequently appends the event *first* and updates second,
-- which is the same order every other consequence in this schema is written in:
-- evidence, then the thing derived from it.
CREATE TRIGGER resource_leases_require_lease_event BEFORE UPDATE ON resource_leases
WHEN (NEW.released_at IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM lease_events
                      WHERE project_id = NEW.project_id
                        AND lease_id = NEW.id
                        AND event = 'released'
                        AND fencing_token = NEW.fencing_token
                        AND occurred_at = NEW.released_at))
     OR (NEW.expired_at IS NOT NULL
         AND NOT EXISTS (SELECT 1 FROM lease_events
                         WHERE project_id = NEW.project_id
                           AND lease_id = NEW.id
                           AND event = 'expired'
                           AND fencing_token = NEW.fencing_token
                           AND occurred_at = NEW.expired_at))
     OR (NEW.released_at IS NULL AND NEW.expired_at IS NULL
         AND NOT EXISTS (SELECT 1 FROM lease_events
                         WHERE project_id = NEW.project_id
                           AND lease_id = NEW.id
                           AND event = 'renewed'
                           AND fencing_token = NEW.fencing_token))
BEGIN SELECT RAISE(ABORT, 'a lease changes only by appending the event that records it'); END;

CREATE TRIGGER resource_leases_no_delete BEFORE DELETE ON resource_leases
BEGIN SELECT RAISE(ABORT, 'leases are not deletable'); END;

PRAGMA user_version = 4;
