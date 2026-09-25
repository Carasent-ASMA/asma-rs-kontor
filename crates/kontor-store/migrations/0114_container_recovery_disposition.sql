-- Recovery was adoption-only before native recreation shipped. Preserve that
-- historical fact and persist the exact disposition for every new command.
ALTER TABLE topology_container_recoveries ADD COLUMN disposition TEXT NOT NULL
    DEFAULT 'adopt_existing' CHECK (disposition IN ('adopt_existing', 'recreate_absent'));
PRAGMA user_version = 114;
