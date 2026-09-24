Artifact: high-scope-record

# ASMA-8204 high-scope record

## Settlement

This record settles the implementation contract for task
`01a0ac9d-a96e-7de1-abfe-3d340ec5fea4` (ASMA-8204) in project
`01a0064a-e056-7603-9968-ef64fdaacb75`.

The change extends the existing seat and topology-node lifecycle operations. It
does **not** add a second, cascading `topology:closeout` API. Those operations
already provide revision checks, durable idempotency receipts, native-effect
retry, and exact-addressed readback. The missing behavior is at their shared
archive boundary: native roots are refused, completion evidence is not checked,
and a parent may be archived while a child is merely retired.

## Evidence that fixes the boundary

- `RuntimeAdapter::archive_container` and `Services::move_node_lifecycle` are
  the existing archive path for every topology node.
- `SqliteStore::transition_topology_node` already permits only
  `active -> retired -> archived`, refuses active children and seats on retire,
  and preserves the topology row and its revision history.
- Task-node native cleanup already refuses an open TeamRun through
  `list_open_team_runs`; `RunLifecycle::Parked` is terminal.
- The Paseo 0.8.0 server-info fixture advertises `projectRemove: true`, the
  installed CLI exposes `paseo project delete <project-id>`, and the supported
  protocol operation is `project.remove.request` with exact `projectId` and an
  accepted response. Root cleanup therefore has a supported physical effect;
  logical-only archival is not the admitted substitute.
- ASMA-8001 completion is durably `done`; all scoped non-root topology is
  archived, while its ESW, ECP, and seven ECP seats remain active. It is the
  required live proof after the implementation is integrated and deployed.

## Closeout contract

1. **Existing operations remain authoritative.** Closeout uses the current
   seat-retire and node-retire/archive routes. Each step uses the revision it
   read and its own stable idempotency key. Replaying a key returns the original
   durable receipt and performs no second native effect.
2. **Child-to-root order is enforced, not documented only.** A node may retire
   only after all direct children are non-active and all hosted seats are
   non-active. A node may archive only after every direct child is archived.
   The archive rule belongs in the store transition shared by all callers; the
   runtime adapter repeats the native occupancy refusal before a destructive
   effect.
3. **Delivery work closes first.** A task-scoped TSW may not archive while its
   TeamRun envelope is open. Terminal TeamRuns and their immutable receipts are
   retained. No run, seat, node, binding, or receipt row is deleted.
4. **Seats precede their host.** Persistent and delivery seats are retired
   through the existing exact-seat path before their TSW, ASW, CSW, or ECP can
   retire. Native session retirement and its readback remain unchanged.
5. **Leaf containers precede roots.** TSW, ASW, and CSW native children are
   retired and physically archived first; ECP is retired and physically
   archived only after those children; ESW is retired and physically archived
   last. Archived topology and container bindings remain readable evidence.
6. **Completion is the root gate.** An epic-scoped NativeRoot may neither retire
   nor archive unless that epic has a durable Completion state whose phase is
   exactly `Done`. Missing, unreadable, active, or `NeedsHuman` completion
   refuses before any runtime effect or logical transition.
7. **ESW cleanup is physical.** A non-adopted ESW NativeRoot is deleted through
   Paseo's supported project-remove operation. Before deletion, fresh exact-ID
   readback must prove the stored project identity and canonical root, zero
   remaining workspaces, and zero unarchived sessions. After the command, a
   fresh complete project listing must prove that exact project ID absent.
8. **Lost acknowledgement is replay-safe.** If project removal happened but its
   acknowledgement was lost, retry proves exact-ID absence and returns
   `changed = false`; it never creates a replacement and never deletes by name,
   path scan, or cached adapter state.
9. **Adopted and project-wide roots are retained.** An operator-configured
   adopted NativeRoot is never removed. The unscoped PSW has no epic completion
   and remains outside the epic node-lifecycle route. ASMA-8204 removes only a
   Kontor-created, epic-scoped ESW.
10. **Identity evidence survives cleanup.** The logical node, native container
    binding, native ID, canonical cwd, command receipt, completion state, and
    TeamRun/seat history remain stored after native absence is proved.

The resulting order is:

`terminal TeamRuns -> retired seats -> archived TSW/ASW/CSW -> archived ECP -> archived ESW`

## Bounded implementation

- Generalize `ArchiveContainerRequest` from child-only to the existing
  `ContainerProjection::{NativeChild, NativeRoot}` shapes; a child requires an
  exact parent project ID, while a root requires no parent.
- Extend the fake and Paseo adapters at `archive_container`; add only the Paseo
  project-remove wire/command shape and advertised feature check needed there.
- Put the completion preflight in `move_node_lifecycle`, where both root retire
  and archive pass, and put the all-children-archived invariant in
  `transition_topology_node`, where every caller passes.
- Keep the current API, MCP registry, OpenAPI schema, completion machine, and
  database schema unchanged.

## Required verification

1. A focused loopback regression closes an epic in the exact order above,
   proves early ECP/ESW retirement or archival is refused with zero root archive
   effect, proves missing/non-done Completion refuses ESW retirement, and proves
   Done permits it.
2. The same regression loses the ESW archive acknowledgement, retries the same
   idempotency key, observes one native root removal, one durable receipt, the
   same native binding, and an archived ESW after readback.
3. Paseo adapter contract tests prove exact-id/cwd matching, adopted-root
   refusal, occupied-root refusal, project-remove capability refusal, complete
   post-remove absence, and retry after prior absence.
4. Store coverage proves a retired-but-not-archived child blocks parent archive.
5. MUT-009 is killed twice: removing the child-archive guard admits parent-first
   closeout, and restoring the prior root refusal prevents ESW archival. Both
   mutants must fail the focused tests and be reverted.

## Live repair boundary

The implementation and hermetic checks may proceed on this branch. The live
ASMA-8001 repair may proceed only from a same-fleet deployed artifact that also
contains ASMA-8201 commit `11fdc130d018aae41bcdceb99c2fe4317cb73c15`
(or its integrated equivalent), followed by fresh completion, TeamRun, seat,
topology, and native identity readback. This task must not cherry-pick that
dependency into its isolated feature branch or use root deletion to conceal an
in-flight TeamRun.

No live runtime state is changed by this scope settlement.
