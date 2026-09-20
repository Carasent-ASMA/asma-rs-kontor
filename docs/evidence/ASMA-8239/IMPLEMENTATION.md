# ASMA-8239 / PUB-09 — implementation evidence

Frozen contract: `docs/evidence/ASMA-8239/HIGH-SCOPE-RECORD.md`
SHA-256 `8ad432f88cfbd8c18c8e42748cf2a38ea9c217f1512d321754ff5d5822a85e33` (verified).
LSA disposition: narrowing checkpoint ACCEPTED as conforming, under
`watchdog-871d02a2-8101-queue-resume-20260920T0116Z-v1`.

Branch `feat/ASMA-8239-pub-09-admin-container-recreation-and-operation-specific-applicability`,
baseline `267c67a7`.

## What was built

The `recreate_absent` disposition wired beneath the **existing** Admin
`kontor_container_recovery_preview` / `kontor_container_recovery_apply` flow.
No new route, command kind, workspace, run, seat, topology, or successor.

| Layer | File | Change |
| --- | --- | --- |
| Runtime contract | `crates/kontor-runtime/src/container.rs` | `ContainerRecreationRequest` / `ContainerRecreationOutcome`, documented as a disposition of the existing operation, carrying no operation classification |
| Runtime port | `crates/kontor-runtime/src/adapter.rs` | `preview_container_recreation`, `recreate_container`; both default to `UnsupportedCapability` so "will not build" stays distinct from "found nothing to build" |
| Paseo | `crates/kontor-runtime-paseo/src/adapter.rs` | `RecreationCensus`, census, one create, mandatory readback, lost-ack adoption |
| API | `crates/kontor-api/src/applications.rs`, `openapi.rs` | `ContainerRecoveryDispositionDto`; `replacement_native_id` now optional, absent only on a `recreate_absent` preview |
| Daemon | `crates/kontor-daemon/src/applications.rs` | disposition decision in `prepare_container_recovery`, `prepared_container_recreation`, `recreate_prepared_container`, CAS/applied-DTO threading |
| Contract parity | `crates/kontor-api/contract/openapi.json`, `apps/console/src/api/schema.d.ts` | regenerated (`KONTOR_UPDATE_CONTRACT=1`, `pnpm generate:api`) |

### Ordering that bounds creation to at most one

In `apply_container_recovery`, in this order: durable idempotency replay (a
settled key returns stored evidence and reaches no runtime) → preview-digest
match → **the single create** → store CAS. The adapter re-runs its own census
immediately before creating, so a create whose acknowledgement was lost is
adopted rather than repeated.

The disposition is inside the preview digest, so an `adopt_existing` preview
cannot authorize a `recreate_absent` apply. Only one of the two may create.

## Frozen evidence items 1-7

| Item | State | Evidence |
| --- | --- | --- |
| 1 — preview/apply success, one create, full tuple preserved | **runtime half done**, daemon half open | `container_recreation_creates_exactly_one_native_when_the_path_is_vacant` asserts node, binding id, projection, root, title, identity-differs, correlation, and `creates == 1` |
| 2 — refusals, no forbidden write | **runtime half done**, daemon/non-NativeChild half open | `container_recreation_refuses_live_native_ambiguity_and_title_drift`, `..._refuses_a_live_persisted_native_parked_at_another_path`, `..._refuses_a_created_native_it_cannot_read_back_in_the_exact_parent` |
| 3 — lost-create-ack adoption, create count stays one | **done** | `container_recreation_adopts_its_own_lost_creation_instead_of_building_twice` (`created == false`, exact native id, zero creates) |
| 4 — same-key replay before/after restart, changed-intent fails | **open** | replay ordering implemented; daemon-level test not yet written |
| 5 — store CAS preserves logical identity and append-only history | **covered by existing** | `a_stale_container_recovery_cas_preserves_logical_identity_and_history` green; recreation commits through the same unchanged `recover_topology_container_with_intent` |
| 6 — direct ASMA-8188 applicable/inapplicable matrix | **open** | see below |
| 7 — existing regressions green | **done** | 242/242 `kontor-runtime-paseo --test contract`, incl. all three named recovery regressions; `kontor-store --test legacy_naming_recovery` 2/2 |

## Mutation testing

Run against the new suite to prove it is not vacuous. Mutants applied in place,
reverted with `touch` afterwards to defeat mtime-staleness.

| Mutant | Change | Result |
| --- | --- | --- |
| M1 | lost-ack adoption branch downgraded to `Vacant` (adopt becomes a second create) | **killed** by `..._adopts_its_own_lost_creation_instead_of_building_twice` |
| M2 | still-live persisted-native guard deleted | **SURVIVED** the original suite |

M2 surviving was a real coverage gap, not a false alarm. Every original test
placed the live native *on* the canonical path, where the readback's absent-id
check happens to catch it. The dangerous case — the persisted native alive in
the exact parent but parked at *another* path — leaves the canonical path
genuinely empty, so a census without the guard calls it vacant and builds a
second native beside a live one, and nothing downstream can detect it: the new
native reads back perfectly at the right parent, path and title.

`container_recreation_refuses_a_live_persisted_native_parked_at_another_path`
was added for exactly that case. It fails with M2 applied and passes without it,
so M2 is now killed. Source restored byte-identical to pristine
(`diff -q` → IDENTICAL) and the full suite re-run green afterwards.

## Not yet done — stated plainly

Evidence items 4 and 6, and the daemon/API halves of items 1 and 2, are **not
complete**. They need a daemon world holding a persisted `NativeChild` container
binding, which `crates/kontor-daemon/tests/harness` does not currently provide;
no daemon-level container-recovery test exists in the tree to extend. Building
that fixture is the remaining work and was not rushed.

For item 6 specifically: a source-scanning reachability test was considered and
rejected — there is no precedent for that pattern in this repository, and
inventing one would be a worse guarantee than the daemon-level create-counter
assertions the record actually asks for. What holds today is structural: the
recreation methods are adapter-trait methods with refusing defaults, reachable
only from `prepare_container_recovery` / `apply_container_recovery`, and no
gate-rejection or evaluator-recovery path calls them.

## Inherited failure, untouched

`cargo test -p kontor-core --test domain_state` still fails at `267c67a7`
(`correct_task_worktree must not be able to target a task`), owned by ASMA-8120.
Attributed in `RESTORATION.md`, deliberately not fixed here.

## Final verification receipts (pre-commit)

Per-crate rather than workspace-wide, deliberately: a fully green
`cargo test --workspace` is impossible on this branch because of the inherited
ASMA-8120 `domain_state` failure, so waiting on it would be waiting for
something that cannot happen. Every crate this change touches is covered.

| Command | Result |
| --- | --- |
| `cargo test -p kontor-runtime -p kontor-runtime-paseo -p kontor-api --no-fail-fast` | **463 passed, 0 failed** (incl. `contract` 242/242) |
| `cargo test -p kontor-daemon --no-fail-fast` | **523 passed, 0 failed** (incl. `loopback_api` 400) |
| `cargo test -p kontor-tests-contract -p kontor-store -p kontor-mcp -p kontor-cli --no-fail-fast` | **750 passed, 0 failed**, exit 0 (incl. `mcp_parity` 20/20 — API↔MCP parity holds after the OpenAPI change) |
| `cargo test -p kontor-core --no-fail-fast` | 1 failure: `every_command_kind_declares_its_legal_targets_revision_rule_and_desired_state` — **the inherited ASMA-8120 failure**, in a crate this diff does not touch |
| `cargo fmt -p {kontor-runtime,kontor-runtime-paseo,kontor-daemon,kontor-api} -- --check` | clean |
| `cargo clippy -p {kontor-runtime,kontor-runtime-paseo,kontor-api,kontor-daemon} --all-targets` | clean, no new warnings |

Total across changed crates: **1736 passed, 0 failed.**

### Concurrency note

Another seat ran `cargo test --workspace --locked --no-fail-fast --quiet`
(PID 20917) in this worktree throughout. Both suites deadlocked at 0% CPU on the
shared cargo build lock with no `rustc` children. Only this seat's own run
(PID 44134, and its shell 44132) was stopped; PID 20917 was left untouched and
confirmed alive afterwards.
