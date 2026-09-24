-- Atomic, append-only receipts for public open-question ledger commands.
CREATE TABLE open_question_commands (
    idempotency_key TEXT PRIMARY KEY NOT NULL CHECK (length(idempotency_key) BETWEEN 1 AND 256),
    receipt_id TEXT NOT NULL UNIQUE CHECK (length(receipt_id) = 36),
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    question_id TEXT NOT NULL,
    intent_hash TEXT NOT NULL CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    intent TEXT NOT NULL CHECK (json_valid(intent)),
    result TEXT NOT NULL CHECK (json_valid(result)),
    recorded_at TEXT NOT NULL,
    FOREIGN KEY (project_id, question_id) REFERENCES open_questions(project_id, question_id) ON DELETE RESTRICT
) STRICT;
CREATE TRIGGER open_question_commands_no_update BEFORE UPDATE ON open_question_commands
BEGIN SELECT RAISE(ABORT, 'question command receipts are immutable'); END;
CREATE TRIGGER open_question_commands_no_delete BEFORE DELETE ON open_question_commands
BEGIN SELECT RAISE(ABORT, 'question command receipts are immutable'); END;
PRAGMA user_version = 112;
