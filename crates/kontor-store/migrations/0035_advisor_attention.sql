-- The recommendation and tried path behind one `needs_human` consultation.
--
-- `needs_human` is an explicit attention state, not a synthetic success and not a
-- stalled `placed`. What makes it useful rather than an apology is that it names
-- what the next reader should do and what has already been tried, so nobody
-- repeats the deliberation that failed. A state column alone would assert
-- attention was needed while carrying nothing a human could act on.
--
-- One row per consultation, immutable: the state is terminal for the Advisor's
-- own path, so a second recommendation would mean the first was reconsidered
-- without a record of it.
CREATE TABLE advisor_attention (
    advisor_run_id TEXT    NOT NULL PRIMARY KEY REFERENCES advisor_runs (id) ON DELETE RESTRICT,
    recommendation TEXT    NOT NULL CHECK (length(recommendation) BETWEEN 1 AND 65536),
    tried          TEXT    NOT NULL CHECK (length(tried) BETWEEN 1 AND 65536),
    created_at     TEXT    NOT NULL
                           CHECK (created_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

CREATE TRIGGER advisor_attention_is_immutable
BEFORE UPDATE ON advisor_attention
BEGIN SELECT RAISE(ABORT, 'a recorded attention state is immutable'); END;

CREATE TRIGGER advisor_attention_is_permanent
BEFORE DELETE ON advisor_attention
BEGIN SELECT RAISE(ABORT, 'a recorded attention state cannot be withdrawn'); END;

PRAGMA user_version = 35;
