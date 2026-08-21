-- Per-account, per-provider quota state.
--
-- Why this is not `PaseoAdapterConfig.unavailable_providers`: that set is a
-- settings field read once when the adapter is composed, so excluding a provider
-- needs a daemon restart, is not per-account, and never stops being true. An
-- allowance that returns on Saturday has to stop blocking on Saturday without
-- anyone remembering it.
--
-- Why not a column on `availability_overrides` or a second
-- `capacity_observations` conclusion: both of those are keyed on the account
-- alone, and under Paseo one account profile serves every provider. "Codex is
-- exhausted, Claude is fine" is therefore not a fact either table can hold —
-- and it is exactly the state the 2026-08-21 outage left the realm in, where
-- every Codex-pinned seat stopped while the Claude routes were untouched.
--
-- The provider is a plain string rather than a reference: the model catalog is
-- still a projection this build serves from code, so there is no provider table
-- to point at yet. When there is, this column becomes the foreign key and the
-- CHECK below goes away.
--
-- No verbatim provider text is stored. The message that produced a row is
-- carried as a digest only: it is vendor output containing account hints and
-- URLs, and this store already refuses to persist caller-supplied strings on
-- exactly that reasoning (see `FailoverReason`, which has no free-text note).
CREATE TABLE provider_quota_states (
    project_id         TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    account_profile_id TEXT NOT NULL,
    provider           TEXT NOT NULL CHECK (length(provider) BETWEEN 1 AND 64
                                            AND provider NOT GLOB '*[^0-9a-z._-]*'),
    -- `available`  — nothing is standing in the way.
    -- `exhausted`  — a plan allowance ran out and recovers on a clock.
    -- `drained`    — a credit balance ran out and recovers only on payment.
    -- `unknown`    — something refused and this row cannot say what.
    state              TEXT NOT NULL CHECK (state IN
                            ('available', 'exhausted', 'drained', 'unknown')),
    -- Only ever set for `exhausted`; see the CHECK below.
    resets_at          TEXT
                            CHECK (resets_at IS NULL
                                   OR resets_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- Digest of the evidence, never the evidence.
    evidence_hash      TEXT NOT NULL
                            CHECK (length(evidence_hash) = 64
                                   AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    -- Who concluded it. An operator assertion and a parsed runtime message are
    -- different authorities and a projection has to be able to tell them apart.
    source             TEXT NOT NULL CHECK (source IN ('runtime_observation', 'operator')),
    observed_at        TEXT NOT NULL
                            CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revision           INTEGER NOT NULL CHECK (revision > 0),
    updated_at         TEXT NOT NULL
                            CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, account_profile_id, provider),
    FOREIGN KEY (project_id, account_profile_id)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    -- The invariant that keeps a scheduler honest, in SQL rather than in every
    -- call site: an exhausted allowance knows when it returns, and a drained
    -- balance does not. A `drained` row carrying a reset instant would be
    -- requeued on a timer, which is a retry loop against a dead key.
    CHECK ((state = 'exhausted' AND resets_at IS NOT NULL)
           OR (state <> 'exhausted' AND resets_at IS NULL))
) STRICT;

-- The selection read: every provider state for one project's accounts.
CREATE INDEX ix_provider_quota_states_project
    ON provider_quota_states (project_id, provider);

-- No command-kind widening, deliberately.
--
-- A provider quota write is an availability assertion at finer grain than the
-- account, so it rides the existing `override_availability` kind and names
-- itself in the intent document, the way every other operation here does. The
-- alternative was rebuilding `command_receipts` to add one CHECK value — and
-- that CHECK now lists more than fifty kinds accumulated across v29-v35, every
-- one of which a hand-copied rebuild could silently drop. A migration whose
-- failure mode is "some commands become unreadable" is not worth one enum value.

PRAGMA user_version = 48;
