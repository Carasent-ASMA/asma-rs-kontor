Artifact: `high-change`

# ASMA-8239 / PUB-09 high-change record: Admin NativeChild container recreation

Date: 2026-09-20
Task: Jira `ASMA-8239` / Kontor `01a0bbbc-6e93-7b22-980e-3c023109b1fb`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-change`
TeamRun: `01a0bbc2-fa0f-7932-a6c6-887210308972`
Implement AgentRun: `01a0bbc3-1184-7ce0-9204-452107d52371`
Standing authority revision: `01a0b9a6-6a23-7042-a780-1430cd021032`
Source baseline: `267c67a7`
Branch head: `35cbbd40e1343ec39fbd088d44530295e99c8cbd`
Scope record: `docs/evidence/ASMA-8239/HIGH-SCOPE-RECORD.md`

## Why this record exists

The implementation was delivered and its evidence recorded, but no
`high-change` artifact was ever written for it. This record is that artifact
and nothing more: it is **documentation of work already committed**. No code
was reimplemented, no scope widened, and no test run was performed to produce
it. Every result quoted below was executed earlier in the implement turn that
produced the two commits, and is reproduced here with the command that
produced it.

## Outcome

**Production code changed.** ASMA-8239 adds the `recreate_absent` disposition
to the existing Admin `kontor_container_recovery_preview` /
`kontor_container_recovery_apply` operation: for an already-persisted
`NativeChild` whose exact native is absent and whose exact-parent/canonical-path
census returns zero candidates, apply may create exactly one replacement native
and bind it to the same logical topology node.

It is a second disposition of one operation, not a new one. There is **no new
route, no new command kind, no new workspace, run, seat, topology or
successor**, and no generic recreation entry point.

Frozen evidence items 1–7 are complete. Two matters are explicitly **not**
closed and are recorded below rather than waived: an inherited `kontor-core`
test failure that predates this branch, and an ASMA-8188/ASMA-8189 naming
distinction that must not be settled incorrectly.

## Commits

| Commit | Subject |
| --- | --- |
| `aaf5d9762fc425f94b78fc9ff9252e9f25699333` | `feat(kontor): recreate an absent NativeChild container (ASMA-8239)` |
| `35cbbd40e1343ec39fbd088d44530295e99c8cbd` | `test(kontor): complete the frozen ASMA-8239 evidence matrix` |

Both sit on baseline `267c67a7`. `35cbbd40` is the branch head and equals its
upstream; the working tree is clean.

## Changed surfaces

### `aaf5d976` — the disposition

```
 apps/console/src/api/schema.d.ts              |  32 ++-
 crates/kontor-api/contract/openapi.json       |  23 ++-
 crates/kontor-api/src/applications.rs         |  33 ++-
 crates/kontor-api/src/openapi.rs              |   1 +
 crates/kontor-daemon/src/applications.rs      | 279 +++++++++++++++++++++++---
 crates/kontor-runtime-paseo/src/adapter.rs    | 249 ++++++++++++++++++++++-
 crates/kontor-runtime-paseo/tests/contract.rs | 268 ++++++++++++++++++++++++-
 crates/kontor-runtime/src/adapter.rs          |  57 ++++++
 crates/kontor-runtime/src/container.rs        |  81 ++++++++
 9 files changed, 986 insertions(+), 37 deletions(-)
```

* **Runtime contract** (`kontor-runtime/src/container.rs`) —
  `ContainerRecreationRequest` / `ContainerRecreationOutcome`, documented as the
  `recreate_absent` disposition of the existing operation and carrying no
  operation classification.
* **Runtime port** (`kontor-runtime/src/adapter.rs`) —
  `preview_container_recreation` and `recreate_container`, both defaulting to
  `UnsupportedCapability` so "this runtime will not build containers" stays
  distinguishable from "this runtime found nothing to build".
* **Paseo** (`kontor-runtime-paseo/src/adapter.rs`) — the census, the single
  create, the mandatory readback, and lost-acknowledgement adoption.
* **API** (`kontor-api`) — `ContainerRecoveryDispositionDto`;
  `replacement_native_id` becomes optional, absent exactly on a
  `recreate_absent` preview because no native has been minted yet.
* **Daemon** (`kontor-daemon/src/applications.rs`) — the disposition decision
  inside the existing `prepare_container_recovery`, plus
  `prepared_container_recreation`, `recreate_prepared_container`, and the
  CAS/applied-DTO threading.
* **Contract parity** — `openapi.json` regenerated with
  `KONTOR_UPDATE_CONTRACT=1`; console types regenerated with
  `pnpm generate:api`.

### `35cbbd40` — the evidence matrix

```
 crates/kontor-daemon/tests/container_recreation.rs | 699 +++++++++++++++++++++
 crates/kontor-daemon/tests/harness/mod.rs          | 189 +++++-
 crates/kontor-daemon/tests/loopback_api.rs         |   2 +
 crates/kontor-runtime/src/fake.rs                  | 219 ++++++-
 4 files changed, 1102 insertions(+), 7 deletions(-)
```

`fake.rs` is production source in a test-support role: it gains the
recovery/recreation methods and an `AdapterCall::CreateNativeContainer` variant
emitted **only** where a native is actually minted, which is what makes
"exactly one" and "exactly zero" assertions about natives built rather than
calls received.

### Not touched

`crates/kontor-core` — **0 files changed** across both commits
(`git diff --name-only 267c67a7..HEAD -- crates/kontor-core`). An earlier
generic applicability predicate was assessed as nonconforming against the scope
record's own test, withdrawn, and preserved verbatim for review in
`WITHDRAWN-GENERIC-APPLICABILITY.md`. Its withdrawal left no residue.

## Ordering that bounds creation to at most one

In `apply_container_recovery`, in this order: durable idempotency replay (a
settled key returns stored evidence and reaches no runtime) → preview-digest
match → the single create → store CAS through the unchanged
`recover_topology_container_with_intent`. The disposition is inside the preview
digest, so an `adopt_existing` preview cannot authorize a `recreate_absent`
apply.

A create whose acknowledgement is lost is adopted, never repeated: the adapter
re-runs its census immediately before creating and treats exactly one
exact-parent/path/title candidate as its own prior creation.

## Executed test evidence

Commands and results as run during the implement turn. Reproduced, not re-run
for this record.

| Command | Result |
| --- | --- |
| `cargo test -p kontor-runtime -p kontor-runtime-paseo -p kontor-api --no-fail-fast` | **463 passed, 0 failed** (`contract` 242/242) |
| `cargo test -p kontor-daemon --no-fail-fast` | **535 passed, 0 failed** (`loopback_api` 400, `container_recreation` 12) |
| `cargo test -p kontor-tests-contract -p kontor-store -p kontor-mcp -p kontor-cli -p kontor-core --no-fail-fast` | **1061 passed**, 65 suites ok, **1 failed** — inherited, see below |
| `cargo fmt -p {kontor-runtime,kontor-runtime-paseo,kontor-daemon,kontor-api} -- --check` | clean |
| `cargo clippy -p {kontor-runtime,kontor-runtime-paseo,kontor-api,kontor-daemon} --all-targets` | clean, zero warnings |

**2059 passed across every crate this change touches.**

A fully green `cargo test --workspace` is not achievable on this branch, for the
inherited reason below; per-crate runs covering every changed crate were used
instead, and that substitution is stated here rather than hidden.

## Executed mutation evidence

Three mutants, applied in place and restored with `touch` afterwards to defeat
mtime staleness.

| Mutant | Change | Result |
| --- | --- | --- |
| M1 | lost-ack adoption branch downgraded to `Vacant` | **killed** by `container_recreation_adopts_its_own_lost_creation_instead_of_building_twice` |
| M2 | still-live-native guard deleted from the Paseo census | **initially SURVIVED**; see below |
| M3 | preview-digest fence deleted from `apply_container_recovery` | **killed** by `an_apply_with_a_foreign_preview_hash_is_refused_before_any_create` |

M2 surviving was a real coverage gap, not a false alarm. Every original test
placed the live native *on* the canonical path, where the readback's absent-id
check catches it incidentally. The dangerous case — the persisted native alive
in the exact parent but parked at *another* path — leaves the canonical path
genuinely empty, so a census without the guard builds a second native beside a
live one, and nothing downstream can detect it: the new native reads back
perfectly at the right parent, path and title.
`container_recreation_refuses_a_live_persisted_native_parked_at_another_path`
was added for exactly that case; it fails with M2 applied and passes without it.

Byte-exact restoration verified by SHA-256, not by eye:

| File | SHA-256 before mutants | After restore |
| --- | --- | --- |
| `crates/kontor-runtime-paseo/src/adapter.rs` | `696f669d1f96ff8f1234c5b2d7d353e054c4f60abc4571a92544aa5e2dbf5904` | identical |
| `crates/kontor-daemon/src/applications.rs` | `c9a9a240392a02cd7763f99a93ee1f6ffdba447729c8027e0150ec78ff911f2e` | identical |

### A flake introduced and removed

`a_same_key_apply_with_a_changed_intent_is_refused` originally altered the
preview digest by overwriting its last character with `'0'` — a no-op whenever
the content-derived digest already ended in `0`, so roughly one run in sixteen
passed while asserting nothing. It now substitutes a character guaranteed to
differ and asserts the difference before sending; confirmed over five
consecutive runs. Recorded rather than quietly fixed, because a test that passes
for the wrong reason is what the mutation discipline exists to catch.

## Artifact paths and checksums

All committed at `35cbbd40`; each verified against its committed blob with
`git cat-file blob` at the time this record was written.

| Artifact | SHA-256 |
| --- | --- |
| `docs/evidence/ASMA-8239/HIGH-SCOPE-RECORD.md` | `8ad432f88cfbd8c18c8e42748cf2a38ea9c217f1512d321754ff5d5822a85e33` |
| `docs/evidence/ASMA-8239/EVIDENCE-ITEMS.md` | `7396b968724e70e257f182fcf629717375117d382757ff4cb3fc7512077e575f` |
| `docs/evidence/ASMA-8239/IMPLEMENTATION.md` | `71a1dd84f3f912d96f6f72b8265e90ed672f8c4b81f27fbdbf73a42646b14490` |
| `docs/evidence/ASMA-8239/NARROWING.md` | `1fca75dd6199e465a2e73b8d060633ee2f85b26132eaeda35e004b3f7b600a71` |
| `docs/evidence/ASMA-8239/RESTORATION.md` | `ec8f122be316d4e557c96cd51af89e443c0d90199985a121a265762588cbf7e0` |
| `docs/evidence/ASMA-8239/WITHDRAWN-GENERIC-APPLICABILITY.md` | `89f9eec33e89ed877197cf8f3bbfa86e8759aba1c7d5b1ef95528438c8a154fb` |

This record is not listed among them: its own checksum is reported in the
handoff that accompanies its commit, because a file cannot contain its own
digest.

## Inherited failure — `correct_task_worktree`, explicitly not ours

```
cargo test -p kontor-core --test domain_state
every_command_kind_declares_its_legal_targets_revision_rule_and_desired_state
  panicked: correct_task_worktree must not be able to target a task
```

Proof that it predates this branch, taken against baseline `267c67a7` itself:

* `git show 267c67a7:crates/kontor-core/tests/domain_state.rs | grep -c correct_task_worktree` → `0`
* `git show 267c67a7:crates/kontor-core/src/receipt.rs` grants `CorrectTaskWorktree`
  a `Task` target in the `witness(matches!(target, A::Task))` arm.

A kind that grants a target with no row in `LEGAL_COMMAND_TARGETS` fails that
test by construction. `CorrectTaskWorktree` was introduced by `267c67a7`
("fix(kontor): add exact task worktree claim repair (ASMA-8120) (#241)") without
its table row. ASMA-8239 changes **0 files** in `kontor-core`.

One line — `("correct_task_worktree", "task", "witness", None)` — makes the
whole suite green. It was deliberately **not** taken here: it belongs to
ASMA-8120, and editing another ticket's landed work is outside this scope. A
verifier running `--workspace` will observe this failure and must not attribute
it to ASMA-8239.

## Evidence item 6 — ASMA-8188, not ASMA-8189

Item 6 implements the **ASMA-8188** applicability matrix as the scope record
specifies it: negatives on `kontor_gate_rejection_recover` and on evaluator
recovery via `gates/{gate_id}/record`, each asserting a create counter of
exactly **zero**, plus the positive case showing container recovery reaching
exactly one create in the same world shape.

The scope record names ASMA-8188 as epic `01a0aaa3-64a2-7de1-ab2d-8f40bad733ad`
and ASMA-8189 as its **already-landed child commit `47024af8`**.
**No separate ASMA-8189 remediation negatives were authored**, and this head
must not be settled as though it contains them.

Both negatives seed a recoverable `NativeChild` *first*, so the recreation path
is genuinely available in the world under test: a create counter of zero
therefore means "this operation never reached it", not "there was nothing to
reach". Each also asserts the refusal is the operation's *own* — no
`container`, `native_child` or `recreate` text may appear in it — which is the
symmetric half the scope record requires. There is no predicate to interrogate,
because there is no predicate; applicability is a fact about reachability at the
existing surfaces.

## What is not claimed

No deployment, no gate recorded, no attestation, no merge, no publication, no
live replay, and no Jira or Kontor lifecycle write. The task and epic are **not**
claimed Done.

No native topology, workspace, seat, run or agent was created or operated.
PUB-07 node `01a075df-2c7a-7432-bb97-d6551fe96e9e` and successor AgentRun
`01a0ba00-4f76-7781-8f30-87df14681521` are preserved and were not operated. The
queued verifier AgentRun `01a0bbc3-29ec-7cb2-8164-344ebdc1b606` has not been
started.

Head `35cbbd40` is ready for independent verification, subject to the inherited
failure and the item-6 naming distinction above. Independent review, supported
publication and deployment remain release-owner decisions.
