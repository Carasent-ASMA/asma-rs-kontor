-- ===========================================================================
-- Schema v51. Concurrent quota windows, a depleting credit balance, and the
-- observation state a provider that cannot report headroom is recorded in.
--
-- Numbered after master's v49 (`command_execution_mode`) and v50 (`provider_report`
-- quota source). This increment adds headroom *routing* on top of that live
-- poller; it does not invent a second one.
--
-- v48 gave one `(account, provider)` row one state and one reset instant. That
-- is not enough to describe a real provider. The Claude plan was verified on
-- 2026-08-14 to hold a five-hour `session` window *and* a weekly one at the same
-- time, and a single `resets_at` cannot say when an account holding two of them
-- becomes usable — the answer is the **latest** reset among the spent ones, and
-- that is a fact about a set, not about a column.
--
-- Two things are added, and they are deliberately separate dimensions:
--
--   * `provider_quota_windows` — the set. One row per window kind, carrying the
--     span it measures, when it refills, and how much of it is spent.
--   * the credit columns below — the money. A prepaid balance that depletes and
--     returns only when someone pays.
--
-- They are never converted into each other. Verified 2026-08-14 by sampling:
-- the Claude org's `used_credits` did not move at all while a session window
-- climbed 11% -> 28%. Included windows are therefore free and are meant to be
-- spent to the limit; the credit is the guarded number and is the only place
-- money is a control in this schema.
--
-- Currencies are not converted either, and here that is enforced structurally
-- rather than trusted: the balance and its reserve share ONE `credit_currency`
-- column, so a row holding an EUR balance against a USD floor cannot be written
-- at all. The in-memory type still carries two `Money` values — an observer or
-- an API caller can construct a mismatch — and the headroom predicate refuses
-- one rather than rescaling it.
-- ===========================================================================

-- ---------------------------------------------------------------------------
-- The header row, rebuilt for one new state and the credit columns
-- ---------------------------------------------------------------------------
--
-- Same mechanism as v8, v12 and v38: `state` is a closed CHECK list, SQLite
-- cannot alter a CHECK in place, so the table is rebuilt and the rows carried
-- across. v50 already rebuilt the table to admit `provider_report`; this
-- rebuild carries that source forward and adds the credit columns plus
-- `cannot_report`. The table holds operator-recorded and polled routing
-- state, so the copy is small and total.
--
-- `cannot_report` is the fifth state, and it is NOT a spelling of `unknown`.
-- Both describe an absence of numbers and they are opposite instructions:
--
--   * `unknown` means *this reading failed* — a refusal nobody parsed, or an
--     observation too old to act on. It fails closed, because a state nobody
--     could establish is not a permission.
--   * `cannot_report` means *this provider has no such number to give*.
--     OpenRouter's `:free` routes under FND-005/DEC-001 answer
--     `limit_remaining: null` beside a dollar-denominated counter that stays at
--     zero for them, and no later reading improves on that. Such a provider is
--     used **reactively**: run until it refuses, then record the reset it
--     states. Failing closed on it would retire it permanently on the strength
--     of a number it was never going to have.
--
-- A row in either state carries no reset instant, so the v48 pairing CHECK is
-- kept exactly as it was: only `exhausted` has one, and it must have one.
CREATE TABLE provider_quota_states_v51 (
    project_id         TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    account_profile_id TEXT NOT NULL,
    provider           TEXT NOT NULL CHECK (length(provider) BETWEEN 1 AND 64
                                            AND provider NOT GLOB '*[^0-9a-z._-]*'),
    state              TEXT NOT NULL CHECK (state IN
                            ('available', 'exhausted', 'drained', 'unknown', 'cannot_report')),
    resets_at          TEXT
                            CHECK (resets_at IS NULL
                                   OR resets_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- What is left of a prepaid balance, and the floor new work may not eat
    -- into. Both are integer minor units in `credit_currency`. All three are
    -- present together or absent together: a subscription provider has windows
    -- and no balance, and inventing a zero balance for it would refuse every
    -- launch on it forever.
    credit_minor_units         INTEGER CHECK (credit_minor_units IS NULL
                                              OR credit_minor_units >= 0),
    credit_reserve_minor_units INTEGER CHECK (credit_reserve_minor_units IS NULL
                                              OR credit_reserve_minor_units >= 0),
    credit_currency            TEXT    CHECK (credit_currency IS NULL
                                              OR (length(credit_currency) = 3
                                                  AND credit_currency NOT GLOB '*[^A-Z]*')),
    evidence_hash      TEXT NOT NULL
                            CHECK (length(evidence_hash) = 64
                                   AND evidence_hash NOT GLOB '*[^0-9a-f]*'),
    -- v50's `provider_report` is kept: this rebuild must not drop an authority
    -- the live poller already writes.
    source             TEXT NOT NULL CHECK (source IN
                            ('runtime_observation', 'provider_report', 'operator')),
    observed_at        TEXT NOT NULL
                            CHECK (observed_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    revision           INTEGER NOT NULL CHECK (revision > 0),
    updated_at         TEXT NOT NULL
                            CHECK (updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (project_id, account_profile_id, provider),
    FOREIGN KEY (project_id, account_profile_id)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    -- Carried unchanged from v48: an exhausted allowance knows when it returns
    -- and a drained balance does not. A `drained` row carrying a reset instant
    -- would be requeued on a timer, which is a retry loop against a dead key.
    CHECK ((state = 'exhausted' AND resets_at IS NOT NULL)
           OR (state <> 'exhausted' AND resets_at IS NULL)),
    -- One currency, two amounts. This is the "never compare two currencies"
    -- rule as a constraint rather than as a convention.
    CHECK ((credit_minor_units IS NULL) = (credit_currency IS NULL)
           AND (credit_reserve_minor_units IS NULL) = (credit_currency IS NULL))
) STRICT;

INSERT INTO provider_quota_states_v51
        (project_id, account_profile_id, provider, state, resets_at,
         credit_minor_units, credit_reserve_minor_units, credit_currency,
         evidence_hash, source, observed_at, revision, updated_at)
SELECT   project_id, account_profile_id, provider, state, resets_at,
         NULL, NULL, NULL,
         evidence_hash, source, observed_at, revision, updated_at
FROM     provider_quota_states;

DROP TABLE provider_quota_states;
ALTER TABLE provider_quota_states_v51 RENAME TO provider_quota_states;

-- The selection read, recreated: the index went with the dropped table.
CREATE INDEX ix_provider_quota_states_project
    ON provider_quota_states (project_id, provider);

-- ---------------------------------------------------------------------------
-- The windows
-- ---------------------------------------------------------------------------
--
-- Keyed by kind rather than by an ordinal, because two rows of the same kind on
-- one pair is not a richer reading — it is two readings, one of which is stale.
-- The primary key makes that unwritable instead of leaving a scheduler to pick.
--
-- `kind` is classified from the provider's own window LENGTH and never from the
-- name of the field it arrived in. The Codex payload carries its span as
-- `window_minutes` beside keys named `primary` and `secondary`, and a reader
-- that trusted those names recorded a weekly allowance as whatever `primary`
-- meant that quarter. The number is the fact; the key is the vendor's layout.
--
-- Every window has a reset instant, unconditionally. A window is a span with an
-- end; an allowance that cannot say when it returns is the header row's
-- `unknown` state and not a window at all.
CREATE TABLE provider_quota_windows (
    project_id         TEXT NOT NULL,
    account_profile_id TEXT NOT NULL,
    provider           TEXT NOT NULL,
    kind               TEXT NOT NULL CHECK (kind IN
                            ('session', 'daily', 'weekly', 'monthly')),
    resets_at          TEXT NOT NULL
                            CHECK (resets_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    -- A share of the window, never an absolute count. Providers report the two
    -- interchangeably and only the share is comparable across them.
    used_percent       INTEGER NOT NULL CHECK (used_percent BETWEEN 0 AND 100),
    PRIMARY KEY (project_id, account_profile_id, provider, kind),
    -- Cascade, uniquely in this schema, and on purpose: a window has no meaning
    -- without the pair it was observed on. Every other table here uses RESTRICT
    -- because its rows are evidence someone may need to read after the fact; an
    -- orphaned window is not evidence, it is a stale number a scheduler would
    -- still route on.
    FOREIGN KEY (project_id, account_profile_id, provider)
        REFERENCES provider_quota_states (project_id, account_profile_id, provider)
        ON DELETE CASCADE
) STRICT;

PRAGMA user_version = 51;
