-- The delivery task one topology node serves.
--
-- Admission has to answer "which node does this task's seat belong in" before
-- it may place anything, and until now nothing could: `seat_bindings` carries a
-- task, but a seat binding is what admission is trying to create, so it cannot
-- also be the thing that locates the node. The link therefore belongs on the
-- node.
--
-- Nullable because most nodes serve no task at all: a project root, an epic and
-- a control plane are not deliveries. Only the task-scoped workspace kind
-- carries one.
ALTER TABLE topology_nodes ADD COLUMN task_id TEXT REFERENCES tasks(id) ON DELETE RESTRICT;

-- One task has at most one live node. Two would make "the task's workspace"
-- ambiguous at exactly the moment admission needs it to be a single answer, and
-- picking either one would place a team's roles in both.
CREATE UNIQUE INDEX ux_topology_node_task
    ON topology_nodes(project_id, task_id)
    WHERE task_id IS NOT NULL AND lifecycle = 'active';

PRAGMA user_version = 27;
