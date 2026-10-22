-- Existing attestations keep NULL: their governing bundle must never be invented.
-- The existing append-only triggers cover this column too.
ALTER TABLE publication_attestations ADD COLUMN source_bundle_hash TEXT NULL
    CHECK ((source_bundle_hash IS NULL OR
           (length(source_bundle_hash) = 64 AND source_bundle_hash NOT GLOB '*[^0-9a-f]*'))
           AND (accepted = 0 OR policy_revision < 5 OR source_bundle_hash IS NOT NULL));

PRAGMA user_version = 137;
