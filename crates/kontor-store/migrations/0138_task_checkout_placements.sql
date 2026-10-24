-- Freeze host checkout identity and base before a task's first Git effect.
-- Historical claims and native bindings are untouched; no inferred source pin.
CREATE TABLE task_checkout_placements (
    project_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    worktree TEXT NOT NULL CHECK (worktree GLOB '/*'),
    destination TEXT NOT NULL CHECK (destination GLOB '/*'),
    task_revision INTEGER NOT NULL CHECK (task_revision >= 1),
    module_key TEXT,
    intent TEXT NOT NULL CHECK (json_valid(intent)),
    intent_hash TEXT NOT NULL CHECK (length(intent_hash) = 64 AND intent_hash NOT GLOB '*[^0-9a-f]*'),
    created_at TEXT NOT NULL,
    readback TEXT CHECK (readback IS NULL OR json_valid(readback)),
    PRIMARY KEY(project_id, task_id, worktree),
    FOREIGN KEY(project_id, task_id) REFERENCES tasks(project_id, id) ON DELETE RESTRICT
) STRICT;

CREATE TRIGGER task_checkout_placements_immutable BEFORE UPDATE ON task_checkout_placements
WHEN OLD.project_id <> NEW.project_id OR OLD.task_id <> NEW.task_id
 OR OLD.worktree <> NEW.worktree OR OLD.destination <> NEW.destination
 OR OLD.task_revision <> NEW.task_revision OR OLD.module_key IS NOT NEW.module_key
 OR OLD.intent <> NEW.intent OR OLD.intent_hash <> NEW.intent_hash
 OR OLD.created_at <> NEW.created_at OR OLD.readback IS NOT NULL OR NEW.readback IS NULL
BEGIN SELECT RAISE(ABORT, 'checkout placement identity and confirmed readback are immutable'); END;
CREATE TRIGGER task_checkout_placements_no_delete BEFORE DELETE ON task_checkout_placements
BEGIN SELECT RAISE(ABORT, 'checkout placements are not deletable'); END;

-- A fresh graph may be written concurrently with native work. It cannot take
-- another task's active pin; old pins stop reserving a path after correction.
CREATE TRIGGER task_checkout_claim_insert BEFORE INSERT ON task_worktrees
WHEN EXISTS (
 SELECT 1 FROM task_checkout_placements pin
 JOIN task_worktrees claim ON claim.project_id = pin.project_id
  AND claim.task_id = pin.task_id AND claim.worktree = pin.worktree
 WHERE pin.destination = NEW.worktree
  AND (pin.project_id <> NEW.project_id OR pin.task_id <> NEW.task_id)
)
BEGIN SELECT RAISE(ABORT, 'checkout destination is claimed by another task'); END;
CREATE TRIGGER task_checkout_claim_update BEFORE UPDATE OF worktree ON task_worktrees
WHEN EXISTS (
 SELECT 1 FROM task_checkout_placements pin
 JOIN task_worktrees claim ON claim.project_id = pin.project_id
  AND claim.task_id = pin.task_id AND claim.worktree = pin.worktree
 WHERE pin.destination = NEW.worktree
  AND (pin.project_id <> NEW.project_id OR pin.task_id <> NEW.task_id)
)
BEGIN SELECT RAISE(ABORT, 'checkout destination is claimed by another task'); END;

CREATE TRIGGER task_checkout_container_insert BEFORE INSERT ON topology_node_containers
WHEN EXISTS (
 SELECT 1 FROM task_checkout_placements pin
 JOIN task_worktrees claim ON claim.project_id = pin.project_id
  AND claim.task_id = pin.task_id AND claim.worktree = pin.worktree
 JOIN topology_nodes node ON node.id = NEW.topology_node_id
 WHERE pin.destination = NEW.canonical_cwd
  AND (pin.project_id <> NEW.project_id OR node.task_id IS NOT pin.task_id)
)
BEGIN SELECT RAISE(ABORT, 'checkout destination is claimed by another task or consultation'); END;
CREATE TRIGGER task_checkout_container_update BEFORE UPDATE OF canonical_cwd ON topology_node_containers
WHEN EXISTS (
 SELECT 1 FROM task_checkout_placements pin
 JOIN task_worktrees claim ON claim.project_id = pin.project_id
  AND claim.task_id = pin.task_id AND claim.worktree = pin.worktree
 JOIN topology_nodes node ON node.id = NEW.topology_node_id
 WHERE pin.destination = NEW.canonical_cwd
  AND (pin.project_id <> NEW.project_id OR node.task_id IS NOT pin.task_id)
)
BEGIN SELECT RAISE(ABORT, 'checkout destination is claimed by another task or consultation'); END;

PRAGMA user_version = 138;
