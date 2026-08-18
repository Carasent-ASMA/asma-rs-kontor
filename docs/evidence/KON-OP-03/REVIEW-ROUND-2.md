# KON-OP-03 code-review gate — round 2 review notes

Reviewed: `ff09f92` (super `5e01e83`; gitlink `ff09f92` == submodule HEAD), the
six remediation commits on top of `44bceeb`.
Round 1: rejected, receipt `01a00d02-7675-7331-8802-c3d3f973c16d` (`REVIEW.md`).

Verdict: **rejected** — receipt `01a00d69-e485-70d0-b41b-43f227786e0e`, sequence 2.

## Mechanical checks — all green

| Check | Result |
| --- | --- |
| `cargo test --workspace` | green — 105 suites, 1328 passed, 0 failed, 8 ignored |
| `crates/kontor-daemon/tests/loopback_api.rs` | green — 123 passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean, exit 0 |
| `cargo fmt --all -- --check` | **clean, 0 violations** |

## Round-1 findings: all four genuinely closed

- **M1 — fmt.** `cargo fmt --all --check` reports zero violations.
- **M2 — orphaned DTOs.** Contract orphans fall from 14 to 7, and the remaining
  seven (`ApiErrorBody`, `EventDto`, `ReceiptDto`, `RunDto`, `StreamFrameDto`,
  `StreamRefusalDto`, `TaskDto`) are the pre-existing baseline set. Every DTO
  added by OP-03 is now reachable from an operation.
- **M3 — Delivery Team slot.** `TeamDraftRequest.slots` is now
  `Vec<TeamDraftSlotRequest>` with `role: RoleSelectionDto`, and the request
  carries `#[serde(deny_unknown_fields)]`, so a caller-authored raw role or
  standard title is refused rather than ignored. `TeamDraftSlotDto` echoes the
  stored selection, and its doc comment is honest about why it is not yet a
  resolved `ResolvedRoleRefDto`. Console consumer and tests updated.
- **M4 — `/v1/commands/{kind}`.** Gone from router, registry and generated
  contract. `NON_AGENT_ROUTES` now contains only `/v1/health` and
  `/v1/openapi.json`. `there_is_no_generic_command_route_behind_the_named_operations`
  locks it out with a 404 assertion across three former command kinds, and the
  generic proofs were ported onto concrete routes.

This was careful, complete remediation of everything round 1 raised.

## The contract surface is now complete

51 operations, matching the handoff's tables exactly:

| Family | Operations |
| --- | --- |
| Topology specification / catalog / reference | 7 |
| Semantic topology | 8 |
| Native capacity and exact-seat | 9 |
| Successor-ticket contracts | 27 |

`REGISTRY` 71 → 115, documented operations 73 → 116, contract paths 73 → 116.
Route, OpenAPI operation, `ToolSpec` tier and generated client agree per
operation. `SemanticTopologyTargetDto` remains a closed union of Kontor-owned
ids; no node kind, parent, native id, name, `cwd`, threshold, pid or argv is
accepted on any request.

## Why the gate still rejects

### The surface is complete and hollow

`grep -c "not composed in this build" crates/kontor-daemon/src/applications.rs`
returns **50**. There is exactly one `impl ApplicationOperations for Services`,
and it refuses `unavailable` for essentially every operation OP-03 added —
including the families that were required to be *composed*, not stubbed:

- all 8 semantic-topology operations (`inspect`, `drift`, `ensure`,
  `materialize`, node `retire`/`archive`, `upgrade-preview`/`upgrade-apply`);
- all 9 capacity and exact-seat operations, including
  `refresh_capacity` — "no native capacity collector is composed in this build";
- the original 7 specification/catalog/code-help operations.

The handoff licenses contract-only delivery narrowly and explicitly, and only
for the successor families:

> "OP-04, OP-05 and OP-06 own the application behavior below … Until the owning
> service is composed, the daemon returns typed `unavailable` before any effect."

That covers 27 operations. It does not cover CP2, which requires semantic
topology "all reusing the OP-01 store and OP-02 materializer", nor CP3, which
requires account-owned capacity records and native collectors.

### CP3 ownership transfer did not happen

- `crates/kontor-integrations-asma/src/fleet.rs` still calls `AsmaExecutable`
  for `preflight`/`status`/`block`, and `lib.rs:40` still exports `pub mod fleet`.
  The handoff says to delete these.
- `crates/kontor-accounts/src/lib.rs:41` still states that "`asma fleet` stays
  authoritative for cooldown mechanics" — verbatim the baseline the handoff
  requires be moved into `kontor-accounts`.
- No collectors were ported into `kontor-accounts` (`launch.rs`, `profile.rs`,
  `resolver.rs` only); there is no raw-observation-first persistence path.

Nothing in `kontor-daemon` now *depends* on `fleet`, which is progress, but the
production `AsmaExecutable` edge the handoff names is still in the tree.

### CP4 is untouched

- `crates/kontor-daemon/src/lib.rs:95` — `DEFAULT_CAPACITY` still has
  `mission_max_in_flight: 8` and `adaptive.ceiling: 8`. The handoff specifies
  `mission=7` and `ceiling=7`.
- `crates/kontor-daemon/src/applications.rs:1810` —
  `adaptive_window: AdaptiveWindow::start(self.capacity.adaptive)`. Every
  scheduling snapshot still starts a fresh window at four, which the handoff
  forbids in as many words ("Do not … reset state in `Services::snapshot`").
  `AdaptiveWindow::restore` exists at `kontor-scheduler/src/model.rs:800` and is
  never called from production.
- `AdaptiveAdmissionState` appears only in `kontor-core` state/repository and
  `kontor-store`; it is absent from the production scheduling path, and nothing
  seeds it on epic apply/pin.

### Consequence for the required negative proofs

Roughly nine of the fourteen listed proofs cannot be satisfied by a refusal and
remain unkilled — among them a published or epic-pinned specification changing
in place, materialization outside OP-02's exact-id path, a capacity refresh that
stores only derived state, an override rewriting raw evidence, a snapshot
resetting the adaptive window to four, one clean observation growing the window,
and counting seats instead of active TeamRuns.

The suite grew by 5 tests for 44 new operations, which is the expected shape when
the operations refuse: there are no write paths to prove.

## Assessment

The task is "Expose Operational application services **and** `/v1` contracts".
The `/v1` contracts half is now complete and, on the evidence, well built. The
application services half is not started: CP1 is done, CP2/CP3/CP4 are contract
without behavior.

## To clear this gate

1. Compose the semantic-topology operations against the OP-01 store and the
   OP-02 materializer (CP2), with the stale-revision and replayed-key black-box
   tests each write path owes.
2. Port the native capacity collectors into `kontor-accounts`, persist the raw
   observation before deriving availability, and move cooldown ownership off
   `asma fleet`; delete `kontor-integrations-asma::fleet` and its
   `AsmaExecutable` calls (CP3).
3. Land the persisted adaptive controller and active-TeamRun accounting, set
   `mission=7` / `ceiling=7`, seed `AdaptiveAdmissionState` on epic apply, and
   restore the persisted width in `Services::snapshot` via
   `AdaptiveWindow::restore` (CP4).

The 27 successor-ticket contracts are correctly left refusing and need no
further work here.

If the intent is that OP-03 ships the contract surface only and CP2/CP3/CP4 move
to a successor ticket, that is a legitimate call — but it is a scope change to
the task and the handoff, and it belongs to the orchestrator, not to this gate.
