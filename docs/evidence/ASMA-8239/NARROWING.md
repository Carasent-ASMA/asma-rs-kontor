# ASMA-8239 / PUB-09 — narrowing to the frozen high-scope record

Record: `docs/evidence/ASMA-8239/HIGH-SCOPE-RECORD.md`
SHA-256: `8ad432f88cfbd8c18c8e42748cf2a38ea9c217f1512d321754ff5d5822a85e33`
(verified locally with `shasum -a 256`; matches the value supplied in the handoff).

Branch `feat/ASMA-8239-pub-09-admin-container-recreation-and-operation-specific-applicability`,
head `267c67a7`, unchanged. No commit, reset, or checkout.

## Self-assessment against the record's conformance test

The record's disagreement section asks the release LSA to disposition the WIP as
conforming or nonconforming. Applying its own stated test to the WIP:

> nonconforming: it classifies arbitrary operation/subject pairs, exposes a
> reusable recreation route, or makes unrelated recovery paths depend on the
> NativeChild precondition.

`kontor_core::container_recreation::decide(operation, subject)` classified
arbitrary operation/subject pairs over a four-value operation registry. That is
the first clause. The implementer therefore assessed the abstraction as
**nonconforming** and narrowed it, per "the implementer must narrow it before
acceptance". This was not a disagreement carried forward; the record is treated
as frozen scope.

## What was withdrawn

| Change | Disposition |
| --- | --- |
| `crates/kontor-core/src/container_recreation.rs` (generic predicate, operation registry, 9 unit cases) | withdrawn; content preserved verbatim in `WITHDRAWN-GENERIC-APPLICABILITY.md` |
| `pub mod container_recreation;` in `crates/kontor-core/src/lib.rs` | withdrawn |
| `CommandKind::RecreateTopologyContainer` + project-target arm in `crates/kontor-core/src/receipt.rs` | withdrawn — the record freezes this as "not a new recovery operation"; CAS and history stay on the existing `RecoverTopologyContainer` / `recover_topology_container_with_intent` |
| `("recreate_topology_container", ...)` row in `crates/kontor-core/tests/domain_state.rs` | withdrawn with its kind |
| `operation: ContainerRecreationOperation` field and `ensure_applicable()` on `ContainerRecreationRequest` | withdrawn — this was the runtime half of the generic classification |

`git diff` now reports **no change** to any `kontor-core` file. Core is
byte-identical to `267c67a7`.

### Design concession, stated plainly

Removing the distinct command kind gives up a durable receipt-level distinction
between "adopted a live native" and "created a new one". The record places that
distinction in the preview disposition (`recreate_absent` vs single-candidate
adoption) and in the stored recovery evidence instead. The audit question is
still answerable; it is answered from the recovery row rather than from the
command vocabulary. Recorded here because it is a real trade, not a no-op.

## What was kept, and why it is believed conforming

The runtime seam survives, narrowed to a disposition of the existing operation:

- `crates/kontor-runtime/src/container.rs` — `ContainerRecreationRequest`,
  `ContainerRecreationOutcome`. Documented explicitly as the `recreate_absent`
  disposition of the existing Admin container-recovery flow, not a capability.
  No operation classification remains.
- `crates/kontor-runtime/src/adapter.rs` — `preview_container_recreation`,
  `recreate_container`, both defaulting to `UnsupportedCapability`.
- `crates/kontor-runtime-paseo/src/adapter.rs` — census, one-create, readback
  and lost-ack adoption.

Reachability, which is the test the record asks the LSA to apply: these are
adapter-trait methods with refusing defaults. They are not routes, carry no
operation registry, and no gate-rejection or evaluator-recovery path calls
them. Wiring them beneath the existing
`kontor_container_recovery_preview`/`apply` daemon flow is the remaining work;
no second entry point will be added.

## Paseo behaviour already matching the frozen apply contract

The census implements record steps 3-4 as written: zero candidates issues
exactly one create with the exact derived parent/path/title (no loop, no
fallback, no alternate parent); a fresh readback then requires exactly one
exact-title candidate before binding. Lost-ack retry re-runs the census first
and adopts the single exact candidate, reporting `created: false`, so no second
create is possible.

## Verification after narrowing

| Command | Result |
| --- | --- |
| `cargo check -p kontor-core -p kontor-runtime -p kontor-runtime-paseo` | clean |
| `cargo test -p kontor-runtime-paseo --test contract stale_container_recovery` | **3 passed, 0 failed** — `refuses_a_still_live_old_identity`, `requires_one_exact_parent_path_and_title_candidate`, `refuses_zero_multiple_and_wrong_title_candidates` |
| `cargo test -p kontor-store --test legacy_naming_recovery` | **2 passed, 0 failed** — includes `a_stale_container_recovery_cas_preserves_logical_identity_and_history` |

All four baseline regressions named in the record's "Existing seam and baseline
evidence" section are green after narrowing.

## Still-red, not caused by this ticket

`cargo test -p kontor-core --test domain_state` fails at `267c67a7` itself:
`correct_task_worktree must not be able to target a task`. Evidence and proof
that it predates ASMA-8239 are in `RESTORATION.md`. Unchanged by this
narrowing, and still owned by ASMA-8120.

## OPEN-QUESTIONS.md

Removed deliberately by the record ("The obsolete `OPEN-QUESTIONS.md` is
removed"), which resolves OQ-8239-01 via the confirmed ASMA-8188 epic
`01a0aaa3-64a2-7de1-ab2d-8f40bad733ad` and ASMA-8189 commit `47024af8`, and
OQ-8239-02 by being itself the accepted contract under standing authority
`01a0b9a6-6a23-7042-a780-1430cd021032`. Not restored. The earlier restoration of
that file in `RESTORATION.md` was correct at the time it was made and is
superseded here.
