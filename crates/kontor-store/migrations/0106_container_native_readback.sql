-- Schema v106. Persist the complete read-only native container observation.
-- Existing rows remain NULL: their desired shape, topology ancestry and titles
-- are not historical readback evidence and must not be promoted into it.

ALTER TABLE topology_node_containers
    ADD COLUMN observed_projection TEXT
        CHECK (observed_projection IS NULL OR observed_projection IN ('native_root', 'native_child'));
ALTER TABLE topology_node_containers ADD COLUMN visible_title TEXT;
ALTER TABLE topology_node_containers ADD COLUMN parent_runtime_kind TEXT;
ALTER TABLE topology_node_containers ADD COLUMN parent_host TEXT;
ALTER TABLE topology_node_containers ADD COLUMN parent_generation INTEGER
    CHECK (parent_generation IS NULL OR parent_generation >= 0);
ALTER TABLE topology_node_containers ADD COLUMN parent_native_id TEXT;
ALTER TABLE topology_node_containers ADD COLUMN topology_correlation TEXT;

PRAGMA user_version = 106;
