-- Schema v74. A quota row can say an account is blocked and carry a digest of
-- the evidence, and still not answer the operator's actual question: *which*
-- item, on *which* run, said so. The digest proves two observations carried the
-- same thing; it cannot be read back into a decision anybody can audit.
--
-- The durable control-plane event log cannot hold that answer either. It is a
-- closed vocabulary of flat scalar fields on purpose -- it is the one place a
-- transcript could accumulate -- and it rightly refuses a nested provenance
-- object.
--
-- So provenance is its own append-only record of modeled scalars: identifiers,
-- instants and enums, and nothing else. No refusal text, no transcript, no
-- arbitrary JSON, no open map. The quota row points at it, so a readback can
-- follow one reference from "this account is blocked" to "this exact item on
-- this exact run, under this exact signal revision, said so".
--
-- STAGING NUMBER. This file is 0074 because that is the only contiguous number
-- that compiles on the schema-73 base this branch is cut from.
--
-- FINAL RESERVATION: 0077, SCHEMA_VERSION 77.
--
-- Not 0076. OP-22's merge with current master showed its branch carried its own
-- 0073 as well as 0074 and 0075, and master has since taken 0073 for retryable
-- Jira reconciliation, so OP-22 renumbers its three to 0074/0075/0076 and ends
-- at schema max 76. This file follows at 0077.
--
-- Integration moves, together: this file, its PRAGMA user_version,
-- SCHEMA_VERSION, the position in the migration registry, the export's
-- QUOTA_OBSERVATION_PROVENANCE_SCHEMA_VERSION constant and the generation tests
-- that pin 73/74. The rename is mechanical -- nothing here depends on the
-- ordinal -- but 0076 is spoken for, and shipping it twice would collide.
CREATE TABLE provider_quota_observation_provenance (
    id                     TEXT NOT NULL PRIMARY KEY
                                CHECK (length(id) = 36 AND id NOT GLOB '*[^0-9a-f-]*'),
    project_id             TEXT NOT NULL REFERENCES projects (id) ON DELETE RESTRICT,
    account_profile_id     TEXT NOT NULL,
    provider               TEXT NOT NULL CHECK (
        length(provider) BETWEEN 1 AND 128
        AND provider NOT GLOB '*[^a-z0-9._-]*'
    ),

    -- Which signal revision authorized the decision. The alias above is
    -- routing; this is identity. Two logins of one vendor carry the same
    -- wording, so without these a record could not say which fingerprint fired.
    signal_id              TEXT NOT NULL CHECK (length(signal_id) BETWEEN 1 AND 512),
    signal_version         INTEGER NOT NULL CHECK (signal_version > 0),
    -- Digest of the signal's complete definition. A reworded signal under the
    -- same id and version produces a different hash, which immutable history is
    -- entitled to refuse.
    signal_definition_hash TEXT NOT NULL CHECK (
        length(signal_definition_hash) = 64
        AND signal_definition_hash NOT GLOB '*[^0-9a-f]*'
    ),

    -- Which run, on which native session, at which binding generation. Carrying
    -- the generation is what stops evidence being transplanted onto a sibling
    -- seat or a previous binding.
    agent_run_id           TEXT NOT NULL
                                CHECK (length(agent_run_id) = 36
                                       AND agent_run_id NOT GLOB '*[^0-9a-f-]*'),
    runtime_binding_id     TEXT NOT NULL
                                CHECK (length(runtime_binding_id) = 36
                                       AND runtime_binding_id NOT GLOB '*[^0-9a-f-]*'),
    native_id              TEXT NOT NULL CHECK (length(native_id) BETWEEN 1 AND 256),
    binding_generation     INTEGER NOT NULL CHECK (binding_generation >= 0),

    -- Which exact item. The envelope below is a bound, not the record: a
    -- collapsed entry may cover several *disjoint* ranges, and start/end alone
    -- cannot say which. The exact set lives in the child table.
    item_epoch             INTEGER NOT NULL CHECK (item_epoch >= 0),
    item_seq_start         INTEGER NOT NULL CHECK (item_seq_start > 0),
    item_seq_end           INTEGER NOT NULL CHECK (item_seq_end >= item_seq_start),
    item_kind              TEXT NOT NULL CHECK (length(item_kind) BETWEEN 1 AND 128),
    -- The instant the *item* was emitted, never the instant Kontor read it.
    item_observed_at       TEXT NOT NULL
                                CHECK (item_observed_at GLOB
                                       '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),

    -- What was concluded, and on what basis.
    decision_basis         TEXT NOT NULL CHECK (decision_basis IN ('runtime_refusal')),
    decided_state          TEXT NOT NULL CHECK (decided_state IN
                                ('available', 'exhausted', 'drained', 'unknown')),
    parsed_resets_at       TEXT NULL
                                CHECK (parsed_resets_at IS NULL
                                       OR parsed_resets_at GLOB
                                          '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    reset_zone             TEXT NULL CHECK (reset_zone IS NULL
                                            OR length(reset_zone) BETWEEN 1 AND 128),
    -- The digest the quota row cites, and it is *not* a digest of text alone:
    -- it covers the bounded refusal together with the item provenance that
    -- carried it, so the same sentence from a different item, run or generation
    -- digests differently. Equality with `provider_quota_states.evidence_hash`
    -- is enforced by the writer.
    -- How many source ranges this record has, declared by the writer in the
    -- same transaction that appends them.
    --
    -- Immutability alone did not seal the child collection: the primary key
    -- stops a duplicate ordinal, but nothing stopped a later INSERT at a fresh
    -- one, which would change the exact source set behind a record and an
    -- evidence digest that are both supposed to be final. Bounding the ordinal
    -- by this count closes that -- every slot must be filled in the creating
    -- transaction, a later append can only reuse a taken ordinal (refused by the
    -- primary key) or reach past the count (refused by the trigger below), and
    -- deletion is already refused.
    source_range_count     INTEGER NOT NULL CHECK (source_range_count > 0),
    evidence_digest        TEXT NOT NULL CHECK (
        length(evidence_digest) = 64 AND evidence_digest NOT GLOB '*[^0-9a-f]*'
    ),
    recorded_at            TEXT NOT NULL
                                CHECK (recorded_at GLOB
                                       '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),

    -- Only an exhausted allowance names a return instant, exactly as the quota
    -- row itself requires.
    CHECK ((decided_state = 'exhausted' AND parsed_resets_at IS NOT NULL)
           OR (decided_state <> 'exhausted' AND parsed_resets_at IS NULL)),
    -- The link from a quota row is per project, never id-only.
    UNIQUE (project_id, id),
    FOREIGN KEY (project_id, account_profile_id)
        REFERENCES account_profiles (project_id, id) ON DELETE RESTRICT,
    -- The exact immutable binding tuple. Naming the binding alone would let a
    -- record cite a sibling seat's run, a previous generation, or another
    -- native session and still satisfy the schema; the whole tuple has to be
    -- the one the store holds.
    FOREIGN KEY (project_id, runtime_binding_id, agent_run_id, binding_generation, native_id)
        REFERENCES runtime_bindings (project_id, id, agent_run_id, generation, native_id)
        ON DELETE RESTRICT
) STRICT;

-- The exact sequence set the item covered, in configured order. A collapsed
-- entry can span disjoint ranges, and an envelope would silently include
-- sequences the item never carried.
CREATE TABLE provider_quota_observation_source_ranges (
    provenance_id TEXT    NOT NULL,
    project_id    TEXT    NOT NULL,
    ordinal       INTEGER NOT NULL CHECK (ordinal >= 0),
    seq_start     INTEGER NOT NULL CHECK (seq_start > 0),
    seq_end       INTEGER NOT NULL CHECK (seq_end >= seq_start),
    PRIMARY KEY (provenance_id, ordinal),
    FOREIGN KEY (project_id, provenance_id)
        REFERENCES provider_quota_observation_provenance (project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER provider_quota_observation_source_ranges_fill_their_parent
BEFORE INSERT ON provider_quota_observation_source_ranges
WHEN NEW.ordinal >= (
    SELECT source_range_count FROM provider_quota_observation_provenance
    WHERE id = NEW.provenance_id
)
BEGIN
    SELECT RAISE(ABORT, 'a quota observation source range is outside its record''s declared set');
END;

CREATE TRIGGER provider_quota_observation_source_ranges_are_immutable
BEFORE UPDATE ON provider_quota_observation_source_ranges
BEGIN
    SELECT RAISE(ABORT, 'a quota observation source range is immutable');
END;

CREATE TRIGGER provider_quota_observation_source_ranges_are_permanent
BEFORE DELETE ON provider_quota_observation_source_ranges
BEGIN
    SELECT RAISE(ABORT, 'a quota observation source range cannot be withdrawn');
END;

-- The tuple FK above needs this to point at.
CREATE UNIQUE INDEX runtime_bindings_exact_tuple
ON runtime_bindings (project_id, id, agent_run_id, generation, native_id);

CREATE INDEX provider_quota_observation_provenance_latest
ON provider_quota_observation_provenance (
    project_id, account_profile_id, provider, recorded_at DESC, id DESC
);

CREATE TRIGGER provider_quota_observation_provenance_is_immutable
BEFORE UPDATE ON provider_quota_observation_provenance
BEGIN
    SELECT RAISE(ABORT, 'a quota observation provenance record is immutable');
END;

CREATE TRIGGER provider_quota_observation_provenance_is_permanent
BEFORE DELETE ON provider_quota_observation_provenance
BEGIN
    SELECT RAISE(ABORT, 'a quota observation provenance record cannot be withdrawn');
END;

-- The quota row's reference to the record that last moved it. Nullable because
-- an operator assertion and a provider report have no runtime item behind them,
-- and because every row that predates this migration has none.
-- SQLite cannot add a composite foreign key with ALTER TABLE, so the
-- same-project link is enforced by a trigger pair instead: an id-only reference
-- would let one project's row cite another project's record.
ALTER TABLE provider_quota_states ADD COLUMN provenance_id TEXT NULL;

CREATE TRIGGER provider_quota_states_provenance_is_same_project_insert
BEFORE INSERT ON provider_quota_states
WHEN NEW.provenance_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a quota state may only cite provenance from its own project')
    WHERE NOT EXISTS (
        SELECT 1 FROM provider_quota_observation_provenance
         WHERE id = NEW.provenance_id AND project_id = NEW.project_id
    );
END;

CREATE TRIGGER provider_quota_states_provenance_is_same_project_update
BEFORE UPDATE ON provider_quota_states
WHEN NEW.provenance_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a quota state may only cite provenance from its own project')
    WHERE NOT EXISTS (
        SELECT 1 FROM provider_quota_observation_provenance
         WHERE id = NEW.provenance_id AND project_id = NEW.project_id
    );
END;

PRAGMA user_version = 74;
