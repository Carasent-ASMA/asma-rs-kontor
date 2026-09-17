-- A kickoff hold that says what would end it.
--
-- `execution_authorization_revocations` already records who held work back and
-- why, but `reason` is prose: it reads well and decides nothing. So the only
-- thing that ever lifted a hold was a human calling `execution-arm`, and an
-- epic whose stated condition had been true for days sat idle because nobody
-- was asked to look.
--
-- The condition is stored beside the revocation rather than on it because a
-- revocation is evidence and evidence is never updated. A hold recorded before
-- this table existed has no row here and is read as `manual`, which is exactly
-- what it meant: nothing about self-lifting was promised to it.
CREATE TABLE execution_hold_conditions (
    project_id       TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    authorization_id TEXT NOT NULL,
    -- The closed vocabulary of `HoldLiftCondition`. Constrained here as well as
    -- in the domain because a value this table cannot evaluate is a hold that
    -- never lifts, and that failure is silent.
    condition        TEXT NOT NULL CHECK (condition IN
                         ('manual', 'jira_graph_confirmed', 'leadership_staffed')),
    recorded_at      TEXT NOT NULL
                         CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, authorization_id),
    -- Only a revoked authorization is a hold. Pointing at the revocation rather
    -- than the grant makes that unrepresentable rather than merely discouraged.
    FOREIGN KEY (project_id, authorization_id)
        REFERENCES execution_authorization_revocations (project_id, authorization_id)
        ON DELETE RESTRICT
) STRICT;

-- Append-only, like every other piece of evidence in this schema. A condition
-- that could be edited after the fact is a hold whose terms move while it holds.
CREATE TRIGGER execution_hold_conditions_no_update
BEFORE UPDATE ON execution_hold_conditions
BEGIN SELECT RAISE(ABORT, 'a hold lift condition is evidence, not a draft'); END;

PRAGMA user_version = 100;
