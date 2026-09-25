# ASMA-8239 / PUB-09 — frozen evidence items 1-7, completed

Frozen contract: `docs/evidence/ASMA-8239/HIGH-SCOPE-RECORD.md`
SHA-256 `8ad432f88cfbd8c18c8e42748cf2a38ea9c217f1512d321754ff5d5822a85e33`.
Parent checkpoint: `aaf5d9762fc425f94b78fc9ff9252e9f25699333` (accepted partial).

## The fixture that unblocked the daemon legs

`World::seed_native_child_container` in `crates/kontor-daemon/tests/harness/mod.rs`
persists the smallest shape the Admin container-recovery operation can act on: a
`PSW` node bound as a native project, a `QSW` node below it bound as a native
workspace, and a canonical working directory on the child.

Two deliberate choices:

- **Project-level (`PSW` -> `QSW`), no epic.** `QSW`'s pinned name template is
  the literal `Quick Session Workspace`, so the rendered title needs neither an
  execution scope nor a Team Definition. The title under test stays a fact about
  the specification rather than about whatever naming authority a test seeded.
- **A canonical v7 UUID for the container-binding id.** The recovery path parses
  it back into a `ContainerBindingId`; a readable slug is refused by the type
  that makes a native container id unspellable as a binding id.

The create counter is `World::container_creates`, which counts
`AdapterCall::CreateNativeContainer` and nothing else. The fake emits that
variant only where a native is actually minted — never on the adopt path and
never on a preview — so "exactly one" and "exactly zero" are assertions about
natives built, not calls received.

## Items 1-7

| Item | Where | Status |
| --- | --- | --- |
| 1 — preview/apply success, one create, full tuple preserved | `recreate_absent_previews_then_applies_with_exactly_one_create` (daemon) + `container_recreation_creates_exactly_one_native_when_the_path_is_vacant` (runtime) | complete |
| 2 — refusals, no forbidden write/create | daemon: `a_non_native_child_subject_is_refused_before_any_create`, `a_stale_project_revision_is_refused_before_any_create`, `an_apply_with_a_foreign_preview_hash_is_refused_before_any_create`, `a_non_admin_caller_cannot_reach_a_create`; runtime: `..._refuses_live_native_ambiguity_and_title_drift`, `..._refuses_a_live_persisted_native_parked_at_another_path`, `..._refuses_a_created_native_it_cannot_read_back_in_the_exact_parent` | complete |
| 3 — lost-create-ack adoption, create count unchanged | `a_lost_create_acknowledgement_is_adopted_rather_than_rebuilt` (daemon) + `container_recreation_adopts_its_own_lost_creation_instead_of_building_twice` (runtime) | complete |
| 4 — same-key replay before and after restart; changed intent refused | `a_same_key_apply_replays_without_a_second_create`, `a_same_key_apply_replays_across_a_restart_without_a_second_create`, `a_same_key_apply_with_a_changed_intent_is_refused` | complete |
| 5 — store CAS preserves logical identity and append-only history | existing `a_stale_container_recovery_cas_preserves_logical_identity_and_history`; recreation commits through the same unchanged `recover_topology_container_with_intent` | complete |
| 6 — direct ASMA-8188 applicable/inapplicable matrix | `gate_rejection_recovery_never_creates_a_native`, `evaluator_recovery_never_creates_a_native`, `only_container_recovery_reaches_a_create_in_the_same_world_shape` | complete |
| 7 — existing regressions green | `kontor-runtime-paseo --test contract` 242/242 incl. all three named recovery regressions; `kontor-store --test legacy_naming_recovery` 2/2 | complete |

### How item 6 is proved, and why it is direct

Both negatives seed a recoverable `NativeChild` **first**, so the recreation
path is genuinely available in the world under test. A create counter of zero
therefore means "this operation never reached it", not "there was nothing to
reach". Each test also asserts the refusal is the operation's *own* — no
`container`, `native_child` or `recreate` text may appear in it — which is the
symmetric half the record requires: those operations must not acquire a
recreation capability, and must not be blocked by a NativeChild precondition
that has nothing to do with their subjects. Finally, each checks the seeded
node's binding is untouched.

There is no predicate to interrogate because there is no predicate. The
applicability is a fact about reachability at the existing surfaces, and that is
what these tests measure.

`AttestRetiredEvaluatorEvidence` has no route of its own; evaluator recovery is
reached through `gates/{gate_id}/record` with `recovery_agent_run_id` and
`recovery_session_digest`, which is the surface the negative drives.

## Mutation evidence

Mutants applied in place and restored with `touch` afterwards to defeat
mtime-staleness. Byte-exact restoration verified by SHA-256, not by eye.

| Mutant | Change | Result |
| --- | --- | --- |
| M2 | still-live-native guard deleted from the Paseo recreation census | **killed** by `container_recreation_refuses_a_live_persisted_native_parked_at_another_path` |
| M5 | preview-digest fence deleted from `apply_container_recovery` | **killed** by `an_apply_with_a_foreign_preview_hash_is_refused_before_any_create` |
| M7 | durable idempotency replay no longer short-circuits the runtime | **killed** by both `a_same_key_apply_replays_without_a_second_create` and `a_same_key_apply_replays_across_a_restart_without_a_second_create` |

M5 is worth a note: `a_same_key_apply_with_a_changed_intent_is_refused` stayed
green under it, because a changed intent is caught by the idempotency conflict
rather than by the digest fence. The two mechanisms are genuinely distinct and
each has its own killing test.

### Byte-exact restoration receipts

| File | SHA-256 before mutants | SHA-256 after restore |
| --- | --- | --- |
| `crates/kontor-runtime-paseo/src/adapter.rs` | `696f669d1f96ff8f1234c5b2d7d353e054c4f60abc4571a92544aa5e2dbf5904` | identical |
| `crates/kontor-daemon/src/applications.rs` | `c9a9a240392a02cd7763f99a93ee1f6ffdba447729c8027e0150ec78ff911f2e` | identical |

## A flake this work introduced and then removed

`a_same_key_apply_with_a_changed_intent_is_refused` originally altered the
preview digest by overwriting its last character with `'0'`. The digest is
content-derived and the fixture generates fresh UUIDs per run, so roughly one
run in sixteen produced an "altered" digest identical to the original — the
replay then returned 200 and the test asserted nothing while still passing. It
now substitutes a character guaranteed to differ and asserts the difference
before sending. Confirmed over five consecutive runs.

This is recorded rather than quietly fixed because a test that passes for the
wrong reason is exactly what the mutation discipline above exists to catch.

## Final verification receipts

| Command | Result |
| --- | --- |
| `cargo test -p kontor-runtime -p kontor-runtime-paseo -p kontor-api --no-fail-fast` | **463 passed, 0 failed** (12 suites; `contract` 242/242) |
| `cargo test -p kontor-daemon --no-fail-fast` | **535 passed, 0 failed** (10 suites; `loopback_api` 400, `container_recreation` 12) |
| `cargo test -p kontor-tests-contract -p kontor-store -p kontor-mcp -p kontor-cli -p kontor-core --no-fail-fast` | **1061 passed**, 65 suites ok, exactly **1** failure — see below |
| `cargo fmt -p {kontor-runtime,kontor-runtime-paseo,kontor-daemon,kontor-api} -- --check` | clean |
| `cargo clippy -p {kontor-runtime,kontor-runtime-paseo,kontor-api,kontor-daemon} --all-targets` | clean, zero warnings |

**2059 passed across every crate this change touches.**

The single failure is
`every_command_kind_declares_its_legal_targets_revision_rule_and_desired_state`
in `kontor-core` — the inherited ASMA-8120 defect proved against `267c67a7`
itself in `RESTORATION.md`. `kontor-core` has **no changes** in this branch;
`git diff` reports none. It remains attributed and untouched.

## Shared-harness dead code

`crates/kontor-daemon/tests/harness` compiles into every test binary that
declares it, so a binary using part of it warns about the rest. The established
pattern in this tree is `#[allow(dead_code)]` on the `mod harness;` declaration
(as `account_pinning.rs` already does); it is now applied to the new
`container_recreation.rs` and to `loopback_api.rs`, which began warning about
the new fixture items it does not use.

## Scope held

No new route, command kind, or generic applicability layer. No live topology,
workspace, seat, or run created or operated. The existing Admin
`kontor_container_recovery_preview` / `apply` routes, the narrowed
`recreate_absent` disposition, logical node and binding identity, the ASMA-8120
attribution, and PUB-07 successor `01a0ba00-4f76-7781-8f30-87df14681521` are all
preserved and untouched.
