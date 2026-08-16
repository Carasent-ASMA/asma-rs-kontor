# KON-OP-02 code-review-gate — review notes

Verdict: **passed** (inspector seat, ASMA-7871), on re-review after remediation.

- Round 1 (`b7cca64`): **rejected** — receipt `01a00b6f-b304-7c73-a4ca-47e0a15f7246`, seq 2. Findings F1, F2.
- Round 2 (`d6da0bb`): **passed** — receipt `01a00baf-25ef-7b30-8fe1-4bd03e4169f5`, seq 3. F1 and F2 closed; one follow-up tracked.
- Round 3 (`9cf1473`): **passed**, re-affirmed against the QA/release-gate remediation. The tracked follow-up is now closed too.

Evidence under review: `ff54325`, `7a8a9ee`, `9b6b3ad`, `89cdffc`, `b7cca64`,
`41d2199`, `d6da0bb`, `34404e1`, `9cf1473`. Superproject `9fba086`, gitlink
`9cf14733ce88ae750a6b0f30b3069be9a89c4b25` verified equal to submodule HEAD.

Judged against `docs/evidence/KON-OP-02/ARCHITECTURE.md`.

## F1 — closed

Was: `Services::seat` resolved a placement, bound it to `_placement`, and called
`prepare_workspace` anyway.

Verified closed:

- `prepare_workspace` has **zero** callers in `crates/kontor-daemon/src/`. The
  only production occurrences anywhere are the three adapter trait impls
  (ao/codex/paseo) and the capability name string in `capability.rs:94`.
- All three production paths route through `ensure_container` →
  `prepare_container`, keyed by `topology_node_id`:
  - `Services::seat` — `applications.rs:6082` resolves, `:6094` prepares, `:6097`
    records the seat bindings;
  - `replace_seat` — `applications.rs:4803-4806`, deliberately reusing the
    predecessor's node;
  - `fill_slot` — carries `Seating.container` and states "Neither the plane nor
    the container is prepared again here", so one team run has one container.
- `LaunchPlacement::Workspace` has no production constructor; the daemon only
  builds `LaunchPlacement::Container`.

## F2 — closed

Was: nothing in production wrote a topology node, so `get_task_topology_node`
always returned `None`, every `placement_blocked` refusal was unreachable, and
seat attachment always fell through to `read_legacy_seat_attachments`.

Verified closed:

- Production writers now exist in `kontor-daemon/src/applications.rs`:
  `create_topology_node` (:6525), `create_seat_binding` (:6574),
  `bind_topology_node_container` (:6796), `publish_topology_spec` (:6353),
  `set_project_topology_default` (:6372), `pin_mini_project_topology` (:6450).
- `resolve_placement` now returns `SessionTopologyNode` rather than
  `Option<…>` — the pre-topology `Ok(None)` escape is gone. `ensure_task_node`
  creates the root → epic → task lineage idempotently, and `ensure_container`
  walks that lineage root-down presenting each level's exact binding to the next.
- The refusals are reachable and proven end-to-end:
  `a_task_placed_on_a_node_that_hosts_no_session_is_refused_before_anything_starts`
  (`loopback_api.rs:5795`) drives the real HTTP API through arm → plan → start,
  asserts `started` is empty, `blocked[0].code == "placement_blocked"`, the exact
  rule string, and — the part that makes it a proof rather than wiring — that no
  `AdapterCall::PrepareContainer` ever reached the runtime.
- The legacy fallback is dead on the production path: all five Foundation slots
  resolve a catalog code via `slot.as_role_key()` (the `verifier` slot carries
  role `therapist-verifier` → `UAT`), so every admission writes seat bindings and
  `read_seat_attachments` takes `conclude_seat_attachments`. The legacy function
  survives only for pre-OP-02 team runs, which is correct — bindings cannot be
  invented retroactively.

## Checks

- `cargo fmt --all --check` — exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` — exit 0.
- `cargo test --workspace` — exit 0, **1314 passed, 0 failed** (round 3; 1312 at round 2).

## Negative proofs

All eight hold. Re-verified this round: no binding/caching by TeamRun on the
production path (#1); adoption by exact id only, ownership by label (#2); zero
hard-coded Operational kind names in the adapter, and the daemon reads the kinds
from the seeded `delivery` data and the pinned revision rather than its own copy
(#3); no fallback project — a `native_child` presents the exact parent binding or
stops (#4); no silent repair — an epic pinned to a different revision, a bound
container in another directory, and a duplicate live slot each refuse rather than
rewrite (#7); no `.agentsroom` access (#8).

## Round 3 — the tracked follow-up is closed

`9cf1473` closes both gaps the QA and release gates raised, which are the same
two recorded below as this review's follow-up.

**R1 — the observation writer.** `Services::observe_seat` now records at the
three points a seat's state is genuinely observed, and the split between them is
the OP-REQ-039 requirement rather than a detail:

- the launch (`applications.rs:6229`, `:7235`) and a settled turn (`:4430`)
  record **activity**, because each is a discrete observed event with its own
  native session id;
- the inspect in `settle_runtime` (`:5080`) records **attachment** and quotes
  `runtime_reported`, and deliberately records **no activity** — treating
  `running` as activity is exactly what NP#6 forbids and what would make a hung
  seat read as busy for as long as its process survives.

Wiring it exposed a second-order effect the builder handled correctly: never-
observed activity reads as `Stalled`, so attachment alone would have made every
seat stalled from the instant it started. Making the launch the seat's first
activity fixes that without weakening stall detection — a seat that starts and
then does nothing still stalls once the idle window closes.

**R2 — parent-derived orphanhood.** Admission opens one control seat per epic on
the `ECP` node (`control_kind`/`control_role_code` now seeded in the delivery
data, matching ARCHITECTURE.md §3's "host LSA and TPM in one ECP workspace"),
and every delivery binding names it. The control seat carries `task_id: None` so
it never enters one task's progress evidence, and it is the root of the ownership
chain. Closing the epic releases it; the delivery seats are *read* as orphans
from its row and never rewritten, so NP#7 holds. Release also retires the row so
it stops occupying the unique `(node, role_slot)` key it no longer holds.

`34404e1` is doc-only in source: the QA gate correctly refused
`resolve_placement`'s stale promise of `Ok(None)`, and the claim was retired
rather than the behaviour restored — restoring it would have reinstated a second
TeamRun-keyed way to place a production seat, which is F1.

Both are proven by behavioural tests, not wiring checks:
`a_delivery_seat_whose_owner_closed_is_orphaned_and_holds_no_progress` asserts
seats are `Attached` and hold progress while the owner is open, then all
`Orphaned` and refusing progress once it closes;
`a_project_with_no_topology_is_seeded_one_rather_than_placed_outside_it` asserts
nothing is configured before the start and that afterwards the revision, node and
container all exist with no `PrepareWorkspace` call anywhere.

### The original follow-up text, for the record

`observe_seat_binding` — the only writer of `last_attached_at`,
`last_activity_at`, `runtime_reported`, `released_at` and
`replaced_by_seat_binding_id` — still has no production caller; the callers are
in `kontor-store/tests/operational_liveness.rs` only.

Consequence: `last_attached_at` stays NULL, so `evaluate_seat_attachment`
(`state.rs:225`) returns `Pending` inside the 10-minute grace and
`AttachmentFailed` after it, never reaching the activity branch.

Bounded, and why it does not block:

- `certify_task_progress_from_store` runs **only** on a transition to
  `TaskState::InProgress` (`repository.rs:3256`), and `can_hold_progress()`
  accepts `Pending`, so the admission-time transition succeeds normally.
- The reachable failure is a *re-entry* to `InProgress` more than 10 minutes
  after seat creation on an open team run, which would be refused with "every
  open team run of this task has lost all of its seats". The legacy path
  previously supplied `last_confirmed_at` as attachment evidence there, so this
  is a narrow regression on the resume path.
- It is fail-closed: it refuses a transition rather than placing work wrongly.
- It violates none of the eight negative proofs. #5 is not violated — it errs
  toward under-reporting attachment, the opposite of counting a dead seat as
  capacity; #6 is not violated because nothing is treated as activity at all.
- It is the seat watch/reap/stale observation path of ARCHITECTURE.md §4, not a
  CP1–CP4 deliverable.

Recommend wiring the observation writer before OP-02 is called operationally
complete. It is not disclosed in `REVIEW-REMEDIATION.md`; the disclosure should
be added.

**Resolved in round 3.** The writer is wired (R1 above) and
`REVIEW-REMEDIATION.md` now documents both R1 and R2, including a mutation table
showing that disabling `observe_seat` turns the delivery seats back into
`[Pending, …]` and fails the suite. The disclosure gap is closed.

## Hygiene note (not a gate finding)

`34404e1` and `9cf1473` also commit 106 files of unrelated `KON-MVP-18` run
evidence, and a further MVP-18 run directory is left untracked. Evidence
artifacts rather than source, so it does not affect this verdict, but the OP-02
commits would read more cleanly without another ticket's runs in them.
