# KON-OP-02 architecture handoff

## Decision

Route every accepted production placement through the Operational topology
already persisted by OP-01. A task or TeamRun may remain optional tracker
metadata, but neither may identify a native container.

The current multi-task `PaseoExecutionScope` work is a bootstrap bridge, not the
OP-02 endpoint. The endpoint is:

```text
published node kind + pinned spec revision
                    |
                    v
       validated projection capabilities
                    |
                    v
 durable topology-node binding --exact native id--> Paseo readback
                    |
                    v
      SeatBinding(topology node, role slot)
```

No second topology model is needed. Reuse `SessionTopologyNode`,
`TopologySnapshot`, `NodeProjectionCapability`, and the logical `SeatBinding`
introduced by OP-01.

## Verified baseline

The branch already fixes the immediate Foundation launch boundary:

- one epic project root is separate from each task worktree;
- several explicit task scopes can share one Paseo adapter;
- created workspaces pass an exact epic `projectId` and are read back;
- provider permission modes are requested and checked;
- persisted admissions resume after a refused launch.

Those fixes are necessary, but the core runtime contract is still
TeamRun/ticket-shaped:

- `kontor-runtime/src/workspace.rs::WorkspaceBinding` requires
  `(team_run_id, task_id)`;
- `WorkspacePrepareRequest` requires the same pair;
- `RuntimeCapability` has `PrepareWorkspace` but not `PrepareProject`;
- `PaseoExecutionScope` derives names and correlation from Jira/ticket fields;
- `PaseoAdapter::prepare_workspace` caches and labels by `team_run_id`;
- `Services::seat` constructs a task workspace without resolving a pinned
  topology node or logical SeatBinding;
- `read_seat_attachments` derives the deadline from `created_at`, treats
  `last_confirmed_at` as activity, and hard-codes `parent_closed: false`.

Leaving any of those on the accepted production path fails OP-02 even if the
Foundation contract suite stays green.

## Minimum implementation shape

### 1. Runtime-neutral container projection

Replace workspace ownership with one container request/binding keyed by:

- `TopologyNodeId`;
- the node's immutable `TopologySnapshot`;
- the validated `Vec<NodeProjectionCapability>` from the pinned kind;
- optional task, TeamRun, and consultation references;
- desired display name, expected parent native binding, and canonical `cwd`
  only where the declared capability requires them.

Keep session operations in `RuntimeCapability`; add only `PrepareProject` there.
Dispatch containers by `NodeProjectionCapability`:

- `logical_only`: persist/reconcile no native container;
- `native_root`: bind or prepare a native project;
- `native_child`: bind or prepare below the exact native root/project;
- `session_host`: permit declared SeatBindings in that container.

Migrate production callers in the same slice. A short compatibility wrapper for
old contract fixtures is acceptable; it must not be callable by `Services::seat`.

### 2. Durable native and liveness evidence

Add the smallest migration/repository seam that persists one current native
container binding per topology node. It must preserve the exact runtime kind,
host, generation, native id, observed project/workspace kind, canonical `cwd`,
and binding/readback instants across restart/export/restore.

Extend logical SeatBinding state with the evidence OP-REQ-039 needs:

- attachment deadline fixed at seat creation;
- last observed attached instant;
- last observed activity instant;
- exact owning epic SeatBinding, so orphanhood derives from Kontor lifecycle;
- released/replaced state and the runtime's self-report as quoted evidence only.

Do not infer activity from a generic confirmation timestamp. A successful
readback may confirm attachment; activity must come from an observed runtime
event/turn position. A closed or replaced parent wins over a healthy runtime
self-report.

### 3. Paseo projection rules

Validate capabilities before the first native effect.

- Adopt configured PSW `prj_da432f9269aa936f` by exact readback. It has no
  rename/archive authority; unmatched children are `foreign_unmanaged`.
- Give every ESW its own native project.
- Place ECP/TSW/ASW/CSW as workspaces in the exact bound ESW project.
- Keep PSW-to-ESW and other unsupported nesting logical.
- Host LSA and TPM in one ECP workspace; never create role workspaces.
- Use topology-node id for correlation. Names and `cwd` are validation evidence,
  never identity.
- For an existing node binding, reconcile only by stored native id.
- For first binding, exclude candidates already bound or correlated to another
  node; adopt one truly unbound candidate, create on zero, and block on several.
- Never fall back to a new project when an ESW binding is missing or wrong.

### 4. Admission and reconciliation

`Services::seat` must resolve the task's TSW and `(topology_node_id,
role_slot_id)` SeatBinding before calling the runtime. Missing/duplicate nodes,
wrong kind/capabilities, absent parent binding, wrong `cwd`, or duplicate live
slot stops before launch as `placement_blocked`.

Reconciliation reports disagreement and refuses the invalid state. It must not
silently rewrite either Kontor or Paseo to match the other. Seat watch/reap/stale
paths resolve exact Kontor bindings and native readbacks; they do not consult
AgentsRoom files.

## Coherent checkpoints

1. Runtime-neutral container request/binding plus `PrepareProject`, fake-runtime
   coverage, and compatibility migration of callers.
2. Native-container and SeatBinding liveness persistence, restart/export tests,
   and the three OP-REQ-039 mutants.
3. Paseo capability dispatch, exact-id correlation, adopted-PSW/ESW-child
   placement, collision/lost-ack/restart fixtures.
4. Daemon admission/reconciliation integration and one disposable live epic;
   capture the normal host project-id set before and after and require equality.

Each checkpoint must leave the workspace buildable. Keep the pre-admission API
JSON and scheduler-resume fixes as separate commits; they are root-cause launch
repairs, not topology abstractions.

## Required negative proofs

At minimum, kill these shortcuts:

- binding/caching by TeamRun instead of topology node;
- accepting a name/`cwd` match already owned by another node;
- hard-coding Operational kind names in the adapter;
- creating a fallback project when the ESW binding is absent;
- counting an unattached, orphaned, or stalled seat as progress/capacity;
- treating runtime `running` or a generic confirmation as activity;
- silently repairing a task/runtime disagreement;
- reading `.agentsroom/team-runs` or writing `.agentsroom/sessions`.

