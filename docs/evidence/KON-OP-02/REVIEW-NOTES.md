# KON-OP-02 code-review-gate — review notes

Verdict: **rejected** (inspector seat, ASMA-7871).

Evidence under review: `b7cca64`, `89cdffc`, `9b6b3ad`, `7a8a9ee`, `ff54325`.
Superproject gitlink `b7cca6428fddb435af33fde49fc2b5bd291414fa` verified equal to
submodule HEAD.

Judged against `docs/evidence/KON-OP-02/ARCHITECTURE.md`.

## Verdict

Checkpoints 1–3 substantially hold and are good work. Checkpoint 4 does not.
CP1–CP3 build a correct mechanism that CP4 never connects to the production
path, so two of the required negative proofs remain violated where it counts.

## Blocking findings

### F1 — `Services::seat` resolves placement, then discards it

`crates/kontor-daemon/src/applications.rs:6054` binds the `resolve_placement`
result to `_placement`. Line 6066 then calls
`prepare_workspace(WorkspacePrepareRequest { team_run_id, .. })` — the
TeamRun-keyed legacy path — unconditionally on every admission.

`prepare_container` has **zero** production callers; the only callers are in
`crates/kontor-runtime-paseo/tests/contract.rs`.

ARCHITECTURE.md §1 is explicit: a compatibility wrapper "must not be callable by
`Services::seat`". It is. That violates required negative proof #1
(binding/caching by TeamRun instead of topology node) on the accepted production
path — which ARCHITECTURE.md:53 says "fails OP-02 even if the Foundation
contract suite stays green".

### F2 — the topology plane has no production writer

`create_topology_node`, `create_seat_binding` and `bind_topology_node_container`
have callers only under `crates/kontor-store/tests/` — verified across all 19
crates.

Consequences on the production path:

- `get_task_topology_node` always returns `None`, so `resolve_placement` returns
  `Ok(None)` at its first check and **every `placement_blocked` refusal is
  unreachable**;
- `read_seat_attachments` always falls through to `read_legacy_seat_attachments`
  (`crates/kontor-store/src/repository.rs:2668`), whose own doc comment states it
  carries all three OP-REQ-039 weaknesses and that "Checkpoint 4 retires this
  function by giving every production seat a binding". CP4 does not.

So negative proof #5 (unattached/orphaned/stalled seat counted as
progress/capacity) and #6 (a generic confirmation treated as activity) remain
violated on the accepted production path.

## On the builder's self-flagged gap

The builder honestly flagged that the five `placement_blocked` refusals have no
dedicated end-to-end test.

Judged on the evidence, that gap **on its own would not have blocked the gate**:
the refusals are pure functions of Kontor rows, the store tests cover the row
semantics, and the failure mode is a fail-closed refusal-to-start rather than
corruption. It would have passed with the gap tracked.

It is moot. The refusals are not merely untested — they are unreachable (F2).
No test could reach them through `Services::seat` as it currently stands. The
self-assessment was honest but understated the problem.

## What holds

- **CP1** — `ContainerRequest::validate` enforces projection exclusivity and the
  exact parent for `native_child`; `PrepareProject` added across the paseo, codex
  and ao adapters; fake-runtime coverage present.
- **CP2** — migration 0026 preserves runtime kind, host, generation, native id,
  canonical `cwd` and the binding/readback instants across restart/export/restore.
  `conclude_seat_attachments` implements OP-REQ-039 correctly: orphanhood is read
  from the owner's row, and a missing parent is treated as closed rather than
  still open.
- **CP3** — genuinely strong. Zero hard-coded Operational kind names in the
  adapter (NP#3 clean); adoption by configured exact id only, with no
  adopt-by-display-name branch (NP#2 clean); `child_ownership` keys on the label
  into `ThisNode` / `AnotherNode` / `ForeignUnmanaged`, with the older
  `kontor-team-` label correctly reading as `ForeignUnmanaged`; reconcile by
  stored id; no fallback-to-configured-project branch for a child (NP#4 clean).
- **CP4 live proof** — well designed and its assertion is real: adopt-not-create
  because 0.3.1 can register a project but not delete one, the disposable unit is
  the archivable child workspace, and host project-id set equality is asserted
  before and after.
- **NP#7** — no silent repair: `resolve_placement` reports and refuses, never
  rewrites either side.
- **NP#8** — no `.agentsroom` access: zero references; `list_team_runs_for_task`
  is a Kontor store read, not a file read.

## Remediation

1. Consume the resolved placement: route the seat's container through
   `prepare_container` rather than `prepare_workspace`.
2. Persist the container binding and the `(topology_node_id, role_slot_id)`
   SeatBinding on the production path, which is what retires
   `read_legacy_seat_attachments`.
3. Add the production topology-node writer, or record it explicitly as OP-01 open
   debt that blocks OP-02 completion.
4. The `placement_blocked` end-to-end fixture becomes reachable once the above
   lands, and should ship with it.
