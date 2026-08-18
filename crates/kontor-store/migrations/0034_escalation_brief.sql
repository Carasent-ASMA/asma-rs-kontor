-- OP-REQ-036: an escalation reaches a human with a recommended resolution, its
-- author, and the deliberation path already walked.
--
-- The three columns are nullable because every episode that has not escalated
-- has nothing to say here, and because episodes written before this migration
-- escalated without a brief. A NULL brief on a `needs_human` row is therefore
-- readable as what it is — an escalation from before the rule — rather than
-- being backfilled with an invented recommendation nobody made.
--
-- New escalations cannot be NULL: `recovery_episodes_needs_human_carries_a_brief`
-- below refuses any row that reaches `needs_human` without one. It is written as
-- a trigger rather than a CHECK because a CHECK would also reject the historical
-- rows this migration deliberately preserves.

ALTER TABLE recovery_episodes ADD COLUMN escalation_recommendation TEXT NULL;
ALTER TABLE recovery_episodes ADD COLUMN escalation_recommended_by TEXT NULL;
ALTER TABLE recovery_episodes ADD COLUMN deliberation_path_json TEXT NULL;

-- A row that *arrives* at `needs_human` states its brief. Both the insert and
-- the update path are covered: an episode may be opened straight into any
-- status, and the update path is how every real escalation happens.
CREATE TRIGGER recovery_episodes_needs_human_carries_a_brief_on_insert
BEFORE INSERT ON recovery_episodes
WHEN NEW.status = 'needs_human'
     AND (NEW.escalation_recommendation IS NULL
          OR NEW.escalation_recommended_by IS NULL
          OR NEW.deliberation_path_json IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'an escalation to a human states a recommendation, its author and the path already tried');
END;

CREATE TRIGGER recovery_episodes_needs_human_carries_a_brief_on_update
BEFORE UPDATE ON recovery_episodes
WHEN NEW.status = 'needs_human'
     AND OLD.status <> 'needs_human'
     AND (NEW.escalation_recommendation IS NULL
          OR NEW.escalation_recommended_by IS NULL
          OR NEW.deliberation_path_json IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'an escalation to a human states a recommendation, its author and the path already tried');
END;

PRAGMA user_version = 34;
