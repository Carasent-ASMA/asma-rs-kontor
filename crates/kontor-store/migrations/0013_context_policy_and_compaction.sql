-- ===========================================================================
-- Schema v13. What a seat's context window was, and every attempt to compact it.
--
-- Two immutable tables, both following the canonical-document convention the
-- rest of this schema uses: the whole value as validated JSON beside its
-- SHA-256, so a replay re-admits exactly the bytes that were written rather
-- than a reassembly of them.
--
-- Neither table repeats `realm_id`. The database file *is* the isolation
-- boundary (see 0001's note), and both tables reach `projects` through
-- `agent_runs`, so a lookup in one realm cannot name a row in another.
--
-- Why these are not decomposed into per-enum columns:
--
-- * the requested/effective pair is hashed, and the hash covers the exact
--   document. Splitting it into columns would mean rebuilding the document to
--   verify the hash, and a rebuild that drifts from the original is precisely
--   the failure the hash exists to catch;
-- * the closed enums are already validated by `serde` on the way in and out.
--   A second `CHECK` list here would be a copy that can disagree with the Rust
--   one, and the disagreement would only ever surface as a write failure long
--   after the value was accepted.
--
-- What *is* lifted into columns is only what a query needs to filter or order
-- by without parsing JSON: the run, the binding, the status, and the times.
-- ===========================================================================

-- One immutable requested/effective pair per agent run.
--
-- Written once, before the native session exists, and never updated: the
-- `no_update` trigger below is what makes "a later edit to a template cannot
-- reach backwards into a live run" a property of the database and not only of
-- the code path that happens to write it.
CREATE TABLE run_context_policies (
    agent_run_id     TEXT    NOT NULL PRIMARY KEY
                             REFERENCES agent_runs (id) ON DELETE RESTRICT,
    -- The winning declaration. A column because "why does this seat have this
    -- window" is the first question anybody asks of this table.
    source           TEXT    NOT NULL CHECK (source IN
                             ('authorized_run_override', 'role_slot', 'work_profile',
                              'role_seed', 'standard_fallback')),
    requested_class  TEXT    NOT NULL CHECK (requested_class IN
                             ('lean', 'standard', 'deep', 'extended', 'native')),
    -- NULL is `native`, or a runtime that could not be configured at all. It is
    -- never 0: a trigger of zero tokens would mean "compact immediately", which
    -- is a different claim from "there is no trigger".
    requested_tokens INTEGER NULL CHECK (requested_tokens IS NULL OR requested_tokens > 0),
    effective_tokens INTEGER NULL CHECK (effective_tokens IS NULL OR effective_tokens > 0),
    enforcement      TEXT    NOT NULL CHECK (enforcement IN ('best_effort', 'required')),
    capability       TEXT    NOT NULL CHECK (capability IN
                             ('configured', 'not_enforced', 'pending')),
    clamp            TEXT    NOT NULL CHECK (clamp IN
                             ('none', 'to_safe_ceiling', 'to_minimum_trigger')),
    requested        TEXT    NOT NULL CHECK (json_valid(requested)),
    requested_hash   TEXT    NOT NULL
                             CHECK (length(requested_hash) = 64 AND requested_hash NOT GLOB '*[^0-9a-f]*'),
    effective        TEXT    NOT NULL CHECK (json_valid(effective)),
    effective_hash   TEXT    NOT NULL
                             CHECK (length(effective_hash) = 64 AND effective_hash NOT GLOB '*[^0-9a-f]*'),
    resolved_at      TEXT    NOT NULL
                             CHECK (resolved_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

CREATE TRIGGER run_context_policies_no_update
BEFORE UPDATE ON run_context_policies
BEGIN
    SELECT RAISE(ABORT, 'a run context policy is immutable once resolved');
END;

CREATE INDEX run_context_policies_by_capability
    ON run_context_policies (capability);

-- One immutable row per compaction attempt.
--
-- The primary key is the receipt id, which is also the idempotency key: a
-- replay of the same attempt finds the row and returns it, and the same id
-- carrying different content is a conflict rather than an overwrite. The
-- `no_update` trigger makes that structural — a terminal receipt cannot be
-- regressed by a late or out-of-order write, because nothing can rewrite it at
-- all.
CREATE TABLE compaction_receipts (
    id                TEXT    NOT NULL PRIMARY KEY
                              CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    agent_run_id      TEXT    NOT NULL REFERENCES agent_runs (id) ON DELETE RESTRICT,
    binding_id        TEXT    NOT NULL
                              CHECK (length(binding_id) = 36 AND binding_id NOT GLOB '*[^0-9a-f-]*'),
    trigger_kind      TEXT    NOT NULL CHECK (trigger_kind IN
                              ('threshold', 'scope_boundary', 'operator')),
    status            TEXT    NOT NULL CHECK (status IN
                              ('confirmed', 'not_enforced', 'unsupported', 'pending', 'failed')),
    -- The native session before and after. Equal values are what a `confirmed`
    -- receipt means; the domain type refuses to build one otherwise.
    native_before     TEXT    NOT NULL CHECK (length(native_before) BETWEEN 1 AND 256),
    native_after      TEXT    NULL CHECK (native_after IS NULL
                              OR length(native_after) BETWEEN 1 AND 256),
    generation_before INTEGER NOT NULL CHECK (generation_before >= 0),
    generation_after  INTEGER NULL CHECK (generation_after IS NULL OR generation_after >= 0),
    -- The whole receipt, canonical, beside its digest. Everything above is a
    -- projection of this and is only here so a query need not parse it.
    receipt           TEXT    NOT NULL CHECK (json_valid(receipt)),
    receipt_hash      TEXT    NOT NULL
                              CHECK (length(receipt_hash) = 64 AND receipt_hash NOT GLOB '*[^0-9a-f]*'),
    recorded_at       TEXT    NOT NULL
                              CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z')
) STRICT;

CREATE TRIGGER compaction_receipts_no_update
BEFORE UPDATE ON compaction_receipts
BEGIN
    SELECT RAISE(ABORT, 'a compaction receipt is immutable once recorded');
END;

-- The read model asks "what happened to this run's context, most recent first".
CREATE INDEX compaction_receipts_by_run
    ON compaction_receipts (agent_run_id, recorded_at DESC);

PRAGMA user_version = 13;
