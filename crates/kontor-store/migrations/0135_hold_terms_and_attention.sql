-- A hold states who owns it, why it is held, and when it must be reviewed.
--
-- v101 recorded only the lift condition. `manual` was still reachable by
-- omission, so a hold could name no end at all, and an owner could park work by
-- revoking an authorization without recording any terms beside it. REQ-008
-- closes both: every hold has an owner (the revocation's `revoked_by`), a reason
-- code, a lift condition and a review date at most 30 days ahead.
--
-- The review date is NOT NULL here. A legacy hold that predates this table is
-- backfilled below; a hold written after this migration always carries one. The
-- reason code stays nullable because the closed vocabulary did not exist when
-- earlier holds were recorded, and inventing a reason would fabricate evidence.
--
-- SQLite cannot widen the v101 CHECK for the two new conditions, so the table is
-- rebuilt in the same shape v134 uses for its receipt rebuild. The old table is
-- dropped only after every row has been copied into the successor.
CREATE TABLE execution_hold_conditions_v135 (
    project_id       TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    authorization_id TEXT NOT NULL,
    condition        TEXT NOT NULL
                         CHECK (condition IN ('manual', 'kickoff_ready',
                                              'dependency_done', 'review_date')),
    -- Nullable only for holds recorded before the vocabulary existed. Every
    -- write after this migration states a code.
    reason_code      TEXT NULL
                         CHECK (reason_code IS NULL
                                OR reason_code IN ('waiting_on_dependency',
                                                   'waiting_on_capacity',
                                                   'waiting_on_decision',
                                                   'waiting_on_external_party',
                                                   'deprioritised')),
    review_by        TEXT NOT NULL
                         CHECK (review_by GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    dependency_task_id TEXT NULL
                         CHECK (dependency_task_id IS NULL
                                OR (length(dependency_task_id) = 36
                                    AND dependency_task_id NOT GLOB '*[^0-9a-f-]*')),
    recorded_at      TEXT NOT NULL
                         CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, authorization_id),
    -- A dependency condition without a dependency is a hold nothing can
    -- evaluate; a dependency on any other condition is a term the condition
    -- never reads.
    CHECK ((condition = 'dependency_done') = (dependency_task_id IS NOT NULL)),
    FOREIGN KEY (project_id, authorization_id)
        REFERENCES execution_authorization_revocations (project_id, authorization_id)
        ON DELETE RESTRICT
) STRICT;

-- A recorded condition keeps its terms. The default review date is fourteen
-- days after this migration, which runs as part of the KR-14 deploy: the deploy
-- is the boundary REQ-008 names, and a hold that was already standing when the
-- release landed gets its first review from that instant rather than an
-- invented past date.
INSERT INTO execution_hold_conditions_v135
    (project_id, authorization_id, condition, reason_code, review_by,
     dependency_task_id, recorded_at)
SELECT project_id,
       authorization_id,
       condition,
       NULL,
       strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+14 days'),
       NULL,
       recorded_at
FROM execution_hold_conditions;

-- Before v101 a hold had no row at all: every revocation *was* the hold and the
-- read path treated the missing row as `manual`. Those are exactly the
-- indefinite holds REQ-008 names, so they get a typed row and the same review
-- date rather than staying invisible to the sweep.
INSERT INTO execution_hold_conditions_v135
    (project_id, authorization_id, condition, reason_code, review_by,
     dependency_task_id, recorded_at)
SELECT r.project_id,
       r.authorization_id,
       'manual',
       NULL,
       strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+14 days'),
       NULL,
       r.revoked_at
FROM execution_authorization_revocations r
WHERE NOT EXISTS (
    SELECT 1
    FROM execution_hold_conditions c
    WHERE c.project_id = r.project_id
      AND c.authorization_id = r.authorization_id
);

PRAGMA legacy_alter_table = ON;
DROP TABLE execution_hold_conditions;
ALTER TABLE execution_hold_conditions_v135 RENAME TO execution_hold_conditions;
PRAGMA legacy_alter_table = OFF;

-- Append-only, exactly as in v101. A condition that could be edited after the
-- fact is a hold whose terms move while it holds; a condition that could be
-- deleted is worse, because the read path would see a hole and a hold with no
-- review date.
CREATE TRIGGER execution_hold_conditions_no_update
BEFORE UPDATE ON execution_hold_conditions
BEGIN SELECT RAISE(ABORT, 'a hold lift condition is evidence, not a draft'); END;

CREATE TRIGGER execution_hold_conditions_no_delete
BEFORE DELETE ON execution_hold_conditions
BEGIN SELECT RAISE(ABORT, 'a hold lift condition is evidence, and evidence is not withdrawn'); END;

-- One typed attention record per hold and attention code.
--
-- Raising the same attention twice is not a second fact: the hold passed its
-- review date once. The primary key makes the sweep idempotent at the SQL
-- boundary, so a retry after a crash cannot produce two alerts for one expiry.
CREATE TABLE execution_hold_attention (
    project_id       TEXT NOT NULL,
    authorization_id TEXT NOT NULL,
    attention_code   TEXT NOT NULL CHECK (attention_code IN ('hold_review_overdue')),
    owner            TEXT NOT NULL,
    reason_code      TEXT NULL
                         CHECK (reason_code IS NULL
                                OR reason_code IN ('waiting_on_dependency',
                                                   'waiting_on_capacity',
                                                   'waiting_on_decision',
                                                   'waiting_on_external_party',
                                                   'deprioritised')),
    review_by        TEXT NOT NULL
                         CHECK (review_by GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    raised_at        TEXT NOT NULL
                         CHECK (raised_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, authorization_id, attention_code),
    FOREIGN KEY (project_id, authorization_id)
        REFERENCES execution_hold_conditions (project_id, authorization_id)
        ON DELETE RESTRICT,
    FOREIGN KEY (project_id, owner)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT
) STRICT;

-- Attention is evidence. It is not withdrawn when the hold lifts, because "the
-- owner was told this hold was overdue" remains true after the fact.
CREATE TRIGGER execution_hold_attention_no_update
BEFORE UPDATE ON execution_hold_attention
BEGIN SELECT RAISE(ABORT, 'a hold attention record is evidence, not a draft'); END;

CREATE TRIGGER execution_hold_attention_no_delete
BEFORE DELETE ON execution_hold_attention
BEGIN SELECT RAISE(ABORT, 'a hold attention record is evidence, and evidence is not withdrawn'); END;

PRAGMA user_version = 135;
