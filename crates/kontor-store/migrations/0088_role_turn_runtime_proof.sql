-- ===========================================================================
-- Schema v88. Bind every new bounded role-turn settlement to the exact current
-- Kontor message and terminal canonical runtime position it was proved from.
--
-- Existing receipts remain historical facts and therefore keep NULL here.
-- New ordinary settlements are complete-or-refused; the separately-authorized
-- late-handoff path is explicitly marked and remains proofless. In particular, a delayed native
-- "finished" notification cannot be recorded as the current turn merely
-- because it names the same persistent session after a restart.
-- ===========================================================================

ALTER TABLE role_turns ADD COLUMN settlement_kind TEXT NOT NULL DEFAULT 'historical'
    CHECK (settlement_kind IN ('historical', 'current_runtime', 'late_handoff'));

ALTER TABLE role_turns ADD COLUMN runtime_message_id TEXT NULL
    CHECK (runtime_message_id IS NULL
           OR (length(runtime_message_id) = 36
               AND runtime_message_id NOT GLOB '*[^0-9a-f-]*'
               AND substr(runtime_message_id, 15, 1) = '7'));
ALTER TABLE role_turns ADD COLUMN message_timeline_epoch INTEGER NULL
    CHECK (message_timeline_epoch IS NULL OR message_timeline_epoch >= 1);
ALTER TABLE role_turns ADD COLUMN message_timeline_sequence INTEGER NULL
    CHECK (message_timeline_sequence IS NULL OR message_timeline_sequence >= 1);
ALTER TABLE role_turns ADD COLUMN response_timeline_epoch INTEGER NULL
    CHECK (response_timeline_epoch IS NULL OR response_timeline_epoch >= 1);
ALTER TABLE role_turns ADD COLUMN response_timeline_sequence INTEGER NULL
    CHECK (response_timeline_sequence IS NULL OR response_timeline_sequence >= 1);
ALTER TABLE role_turns ADD COLUMN runtime_observation_cursor INTEGER NULL
    CHECK (runtime_observation_cursor IS NULL OR runtime_observation_cursor >= 1);

CREATE TRIGGER role_turns_require_current_runtime_proof
BEFORE INSERT ON role_turns
BEGIN
    SELECT CASE
        WHEN NEW.settlement_kind = 'historical'
        THEN RAISE(ABORT, 'new historical role turn receipts are forbidden')
        WHEN NEW.settlement_kind = 'current_runtime'
         AND (NEW.runtime_message_id IS NULL
          OR NEW.message_timeline_epoch IS NULL
          OR NEW.message_timeline_sequence IS NULL
          OR NEW.response_timeline_epoch IS NULL
          OR NEW.response_timeline_sequence IS NULL
          OR NEW.runtime_observation_cursor IS NULL)
        THEN RAISE(ABORT, 'a current role turn runtime proof must be complete')
        WHEN NEW.settlement_kind = 'late_handoff'
         AND (NEW.runtime_message_id IS NOT NULL
          OR NEW.message_timeline_epoch IS NOT NULL
          OR NEW.message_timeline_sequence IS NOT NULL
          OR NEW.response_timeline_epoch IS NOT NULL
          OR NEW.response_timeline_sequence IS NOT NULL
          OR NEW.runtime_observation_cursor IS NOT NULL)
        THEN RAISE(ABORT, 'a late handoff cannot claim current runtime proof')
        WHEN NEW.settlement_kind = 'current_runtime'
          AND (NEW.message_timeline_epoch <> NEW.response_timeline_epoch
          OR NEW.response_timeline_sequence <= NEW.message_timeline_sequence
          )
        THEN RAISE(ABORT, 'a role turn response must follow its current message in one epoch')
    END;
END;

PRAGMA user_version = 88;
