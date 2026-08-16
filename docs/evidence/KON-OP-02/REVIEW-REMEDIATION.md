# KON-OP-02 — remediation of the rejected code-review gate

Answers `docs/evidence/KON-OP-02/REVIEW-NOTES.md` (receipt
`01a00b6f-b304-7c73-a4ca-47e0a15f7246`).

Commits: `41d2199`, and the commit this file lands in.

## F1 — `Services::seat` resolved placement, then discarded it

**Closed.**

`Services::seat` no longer calls `prepare_workspace`, and neither does any other
production path: `grep -rn prepare_workspace crates/kontor-daemon/src/` returns
nothing. `Services::seat`, `fill_slot` and `replace_seat` all place through
`prepare_container`, keyed by `TopologyNodeId`.

Consuming the placement required the launch to be able to *carry* it, so
`LaunchParts` now carries a `LaunchPlacement` — `Workspace(..)` for the older
TeamRun-keyed path, `Container(..)` for a topology-node-keyed one. An enum
rather than two optional fields, so a launch cannot present both. `PlacementClaim`
replaces `WorkspaceClaim` in `OperationContext`; an absent placement is still
refused rather than treated as nothing to verify.

Paseo resolves a presented container through its own ledger **by the node the
container names**, never by the team run, and reads the expected root off the
binding rather than recomputing it from the configured task scope.

`fill_slot` no longer prepares anything: the container `seat` prepared is carried
through `Seating`, so one team run has one container rather than one per seat.

## F2 — the topology plane had no production writer

**Closed for the node, container and placement plane. Partially closed for seat
bindings — see the open question below.**

Admission is now the writer. `Services::seat` → `resolve_placement` →
`ensure_task_node`, which on first admission:

1. publishes the bundled Operational topology and role catalog for the project
   and selects it as the project default, if the project has selected none;
2. pins the epic to that revision, if it is unpinned — an already-pinned epic is
   never repinned, because that would silently move every node under it;
3. creates the project-root node, the epic node and the task node, each
   idempotently.

`get_task_topology_node` therefore returns `Some` on the production path, and the
`placement_blocked` refusals are reachable. `ensure_container` then walks the
lineage root-downwards, presenting each level's exact binding to the next, and
persists every readback through `bind_topology_node_container`.

Every kind is read from data — the specification's own `root_kind` and a new
`delivery` section in `fixtures/operational-domain.json`. Several bundled kinds
are `native_child` session hosts below an epic, so "which one serves a task" is
not derivable from capabilities; naming it in the daemon would have been the
hard-coded vocabulary NP#3 forbids.

### The `placement_blocked` end-to-end test

`a_task_placed_on_a_node_that_hosts_no_session_is_refused_before_anything_starts`
(`crates/kontor-daemon/tests/loopback_api.rs`) drives a real refusal through the
HTTP API and asserts that no `PrepareContainer` call reached the runtime.

Mutation-tested: removing the `session_host` check from `resolve_placement`
kills exactly this test and no other (110 passed, 1 failed).

### Refusal that was removed rather than made reachable

`"the parent node holds no native container to place this seat below"` is gone.
Preparation now walks the lineage from the root down, so asking before
preparation refuses the ordinary first admission of an epic, and asking after it
is asking whether the call that just returned had returned.
`ContainerRequest::validate` still refuses a `native_child` with no exact parent
binding, which is the invariant that check was protecting.

## Open question — the Foundation-to-Operational role correspondence

**OQ-OP-02-1. Closes: LSA (architectural — ownership of seeded specification
data). Raised, not closed here.**

`create_seat_binding` requires a `CatalogRoleRef` validated against a published
role catalog. The Foundation role vocabulary (`architect`, `builder`, …) and the
Operational standard-role catalog (`SA`, `SWE`, `QA`, …) are deliberately
separate and **no bridge between them was ever seeded** — OP-01's own
`OPEN-QUESTIONS.md` (OQ-OP-01-1) records that the Foundation fixture migration
is remaining OP-01 work.

Rather than block, this run seeds the correspondence as data in
`fixtures/operational-domain.json` under `delivery.role_bindings`, following the
precedent OP-01 set for its own uncertainties:

| Foundation slot | Standard code | Standard title |
|---|---|---|
| `architect` | `SA` | Software Architect |
| `builder` | `SWE` | Software Engineer |
| `tester` | `QA` | Quality Assurance Engineer |
| `inspector` | `AUD` | Auditor |
| `therapist-verifier` | `UAT` | User Acceptance Test Specialist |
| `researcher` | `BA` | Business Analyst |
| `judge` | `AUD` | Auditor |
| `synthesizer` | `TW` | Technical Writer |
| `reviewer` | `AUD` | Auditor |

**This mapping is a proposal, not a decision.** It is data in one file and one
optional field; correcting it is a JSON edit. The last four rows are the least
certain.

A role slot with **no** entry gets no seat binding and says so in the log. It is
not refused: the correspondence is seeded data an operator's own team templates
can outrun, and a gap in that data is not a reason the work cannot run. A code
the catalog does not declare *is* refused, because that is the seed contradicting
itself.

**Consequence for F2, stated plainly.** `read_seat_attachments` uses the bound
topology path for a team run whose slots are in the table, and still falls
through to `read_legacy_seat_attachments` for one whose slots are not. Every
bundled team is covered; a deployment with custom slots is not until the table
is extended. `read_legacy_seat_attachments` therefore cannot be deleted yet.

## Verification

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings`
and `cargo test --workspace` are all clean (`CARGO_EXIT=0`, 105 test binaries).
All 110 pre-existing daemon loopback tests pass on the new topology path.
