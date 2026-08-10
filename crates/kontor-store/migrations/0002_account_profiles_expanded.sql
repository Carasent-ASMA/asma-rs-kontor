-- ===========================================================================
-- Schema v2 — the non-secret account profile
--
-- Schema v1 gave `account_profiles` an id, a project, a label and an optional
-- external account id. That is enough to *pin* a run to an account and no more:
-- it cannot say which harness the account authenticates against, which approved
-- reference resolves its credentials, which environment variable names that
-- reference fills, or whether the profile may be selected at all.
--
-- This migration adds exactly those non-secret fields. Three rules shape it:
--
--  1. **Additive only.** Every column arrives through `ALTER TABLE ADD COLUMN`,
--     so v1's primary key, its `UNIQUE (project_id, id)` and the four
--     `ON DELETE RESTRICT` references that already point at this table are
--     untouched. Those references are also what makes "delete only an
--     unreferenced profile" true without a line of new SQL: a profile some run,
--     gate evaluation or override still names simply cannot be deleted.
--
--  2. **No invented state, of any kind.** *Every* added column is nullable with
--     no default, and a trigger requires all of them on insert. A row written by
--     v1 code therefore stays visibly incomplete instead of silently acquiring a
--     plausible harness, an empty alias, or — just as bad — an `enabled` flag
--     and a revision this migration made up. `enabled = 1` would be a launch
--     policy decision, and `revision = 1` would be a concurrency claim that no
--     writer ever made; both are the same silent-fallback class as a default
--     credential home, and neither belongs in a schema change.
--
--     The consequence is deliberate: a migrated v1 row is inert. The repository
--     refuses to load it (there is no complete profile to return), its
--     compare-and-swap can never match a `NULL` revision, and the update trigger
--     below freezes it. The only ways forward are to delete it — which the v1
--     `ON DELETE RESTRICT` references already permit exactly when nothing names
--     it — or to create a new profile. Neither route guesses.
--
--  3. **Credential identity is immutable.** A trigger refuses any update that
--     moves the harness, the reference, the environment/routing/capability
--     documents, the provider identity or the creation time, and requires the
--     revision to advance by exactly one. Rotating any of those is a new
--     profile id, so a queued, active or historical run's pin cannot change
--     meaning underneath it.
--
--     Every comparison in that trigger uses `IS NOT` rather than `<>`. With
--     nullable columns the two differ in exactly the case that matters:
--     `NULL <> 'x'` is `NULL`, which a `WHEN` clause reads as "no violation", so
--     `<>` would let raw SQL null out a complete row's harness or fill in an
--     incomplete row's — the two edits the freeze exists to stop.
--
-- What is deliberately *not* here: any column that could hold a resolved value.
-- There is no path, no keychain service, no account name, no token and no hash
-- of one. `credential_ref_alias` is an opaque name that means nothing without
-- the in-memory resolver policy, which is never written to this database.
-- ===========================================================================

-- The runtime family this account authenticates against — the same lexical rule
-- as every other open key in the domain.
ALTER TABLE account_profiles ADD COLUMN harness TEXT NULL
    CHECK (harness IS NULL
           OR (length(harness) BETWEEN 1 AND 128 AND harness NOT GLOB '*[^a-z0-9._-]*'));

-- The approved reference: a closed kind plus an opaque alias. Neither half is
-- resolvable without the resolver policy.
ALTER TABLE account_profiles ADD COLUMN credential_ref_kind TEXT NULL
    CHECK (credential_ref_kind IS NULL
           OR credential_ref_kind IN ('config_home', 'keychain'));
ALTER TABLE account_profiles ADD COLUMN credential_ref_alias TEXT NULL
    CHECK (credential_ref_alias IS NULL
           OR (length(credential_ref_alias) BETWEEN 1 AND 128
               AND credential_ref_alias NOT GLOB '*[^a-z0-9._-]*'));

-- Environment variable *names* mapped to opaque aliases. Stored as a canonical
-- document with its digest, exactly like every other frozen document in the
-- schema, so a re-indented or reordered copy is detected rather than accepted.
ALTER TABLE account_profiles ADD COLUMN environment_refs TEXT NULL
    CHECK (environment_refs IS NULL OR json_valid(environment_refs));
ALTER TABLE account_profiles ADD COLUMN environment_refs_hash TEXT NULL
    CHECK (environment_refs_hash IS NULL
           OR (length(environment_refs_hash) = 64
               AND environment_refs_hash NOT GLOB '*[^0-9a-f]*'));

-- Non-secret routing metadata (provider, model preference, …).
ALTER TABLE account_profiles ADD COLUMN routing TEXT NULL
    CHECK (routing IS NULL OR json_valid(routing));
ALTER TABLE account_profiles ADD COLUMN routing_hash TEXT NULL
    CHECK (routing_hash IS NULL
           OR (length(routing_hash) = 64 AND routing_hash NOT GLOB '*[^0-9a-f]*'));

-- Non-secret declared account capabilities.
ALTER TABLE account_profiles ADD COLUMN capability TEXT NULL
    CHECK (capability IS NULL OR json_valid(capability));
ALTER TABLE account_profiles ADD COLUMN capability_hash TEXT NULL
    CHECK (capability_hash IS NULL
           OR (length(capability_hash) = 64 AND capability_hash NOT GLOB '*[^0-9a-f]*'));

-- An optional non-secret provider identity hint. Opaque, like every other
-- foreign-owned identifier in the schema.
ALTER TABLE account_profiles ADD COLUMN provider_identity TEXT NULL
    CHECK (provider_identity IS NULL OR length(provider_identity) BETWEEN 1 AND 256);

-- Whether a launch may select this profile. A disabled profile is the retirement
-- path for a profile that cannot be deleted because runs still reference it.
--
-- No default: whether an account may be launched through is a policy decision,
-- and defaulting it to enabled would have this migration silently arm every
-- pre-existing row.
ALTER TABLE account_profiles ADD COLUMN enabled INTEGER NULL
    CHECK (enabled IS NULL OR enabled IN (0, 1));

-- Optimistic concurrency. No default either: a revision is a claim about a
-- sequence of writes, and asserting `1` for a row this migration did not write
-- would hand a compare-and-swap a number nobody ever committed to.
ALTER TABLE account_profiles ADD COLUMN revision INTEGER NULL
    CHECK (revision IS NULL OR revision >= 1);

ALTER TABLE account_profiles ADD COLUMN updated_at TEXT NULL
    CHECK (updated_at IS NULL
           OR updated_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z');

-- `ALTER TABLE` cannot add a table-level constraint, and the fields above are
-- all-or-nothing: a profile with a harness but no reference, or a reference with
-- no environment map, is not a profile anything may launch through. The
-- requirement is therefore a trigger, which is the same mechanism the rest of
-- this schema uses for rules SQLite cannot express as a column check.
CREATE TRIGGER account_profiles_identity_required
BEFORE INSERT ON account_profiles
WHEN NEW.harness IS NULL
     OR NEW.credential_ref_kind IS NULL
     OR NEW.credential_ref_alias IS NULL
     OR NEW.environment_refs IS NULL
     OR NEW.environment_refs_hash IS NULL
     OR NEW.routing IS NULL
     OR NEW.routing_hash IS NULL
     OR NEW.capability IS NULL
     OR NEW.capability_hash IS NULL
     OR NEW.enabled IS NULL
     OR NEW.revision IS NULL
     OR NEW.updated_at IS NULL
BEGIN SELECT RAISE(ABORT, 'an account profile must carry its full non-secret identity'); END;

-- The same predicate on update. Completeness is a one-way door: `enabled` is
-- deliberately mutable, so without this an ordinary `UPDATE ... SET enabled =
-- NULL` would walk a complete profile back into the incomplete state that only a
-- v1 row is ever supposed to be in — and the immutability trigger below would
-- not object, because it exists to freeze identity, not to police the two
-- columns that are meant to change.
CREATE TRIGGER account_profiles_state_required
BEFORE UPDATE ON account_profiles
WHEN NEW.harness IS NULL
     OR NEW.credential_ref_kind IS NULL
     OR NEW.credential_ref_alias IS NULL
     OR NEW.environment_refs IS NULL
     OR NEW.environment_refs_hash IS NULL
     OR NEW.routing IS NULL
     OR NEW.routing_hash IS NULL
     OR NEW.capability IS NULL
     OR NEW.capability_hash IS NULL
     OR NEW.enabled IS NULL
     OR NEW.revision IS NULL
     OR NEW.updated_at IS NULL
BEGIN SELECT RAISE(ABORT, 'an account profile must keep its full non-secret identity'); END;

-- Only the label and the enabled flag ever change, and every change advances the
-- revision by exactly one. Everything a resolution depends on is frozen, so the
-- profile revision a launch receipt snapshots keeps meaning what it meant.
CREATE TRIGGER account_profiles_credential_identity_immutable
BEFORE UPDATE ON account_profiles
WHEN OLD.project_id IS NOT NEW.project_id
     OR OLD.created_at IS NOT NEW.created_at
     OR OLD.harness IS NOT NEW.harness
     OR OLD.credential_ref_kind IS NOT NEW.credential_ref_kind
     OR OLD.credential_ref_alias IS NOT NEW.credential_ref_alias
     OR OLD.environment_refs_hash IS NOT NEW.environment_refs_hash
     OR OLD.routing_hash IS NOT NEW.routing_hash
     OR OLD.capability_hash IS NOT NEW.capability_hash
     OR OLD.provider_identity IS NOT NEW.provider_identity
     OR OLD.external_account_id IS NOT NEW.external_account_id
     OR NEW.revision IS NOT OLD.revision + 1
BEGIN SELECT RAISE(ABORT, 'an account profile credential identity is immutable'); END;

PRAGMA user_version = 2;
