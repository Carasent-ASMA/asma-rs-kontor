-- Context belongs to the selection, not the historical definition identity.
-- Existing rows remain unpinned; no source or active bundle is inferred.
ALTER TABLE project_team_definition_defaults
    ADD COLUMN operational_domain_bundle_hash TEXT NULL
    CHECK (operational_domain_bundle_hash IS NULL OR
           (length(operational_domain_bundle_hash) = 64 AND
            operational_domain_bundle_hash NOT GLOB '*[^0-9a-f]*'));

ALTER TABLE mini_project_team_definition_snapshots
    ADD COLUMN operational_domain_bundle_hash TEXT NULL
    CHECK (operational_domain_bundle_hash IS NULL OR
           (length(operational_domain_bundle_hash) = 64 AND
            operational_domain_bundle_hash NOT GLOB '*[^0-9a-f]*'));

PRAGMA user_version = 139;
