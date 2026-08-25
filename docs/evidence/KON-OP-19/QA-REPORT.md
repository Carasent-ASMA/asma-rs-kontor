VERDICT: PASS

# KON-OP-19 / ASMA-7967 — QA gate report

> **Seat:** tester (qa-gate, evidence `qa-report`)
> **Ticket:** KON-OP-19 / ASMA-7967 — "Make native container and seat naming a configurable deterministic template"
> **Epic:** ASMA-7869
> **Date:** 2026-08-21
> **Category:** report
> **Records no Kontor gate.** The LSA records the gate citing this verdict.

This report is a **relaunch**. A prior tester instance was killed part-way by a Paseo
restart (not by any failure). Its surviving on-disk evidence is reused and cited as such
below; the analysis is this instance's own.

---

## 1. Pinned tree

Tests were **not** run in the shared checkout `_tools/asma-rs-kontor` — other sessions are
active there and it moves branches mid-task. A clean detached worktree was reused:

```
tree:       ~/.cache/op19-qa/tree
HEAD:       df64004f901ff3edca1c10191939e9d6ce351b53
git status --porcelain:  (empty)
```

Verified clean and at `df64004` at the start of this relaunch, and still at `df64004` with
**no tracked modification** at the end. For precision: running the suite left one untracked
artifact directory in the worktree (`docs/evidence/KON-MVP-18/run-a0fb010881d5c240/`, written
by a test); no tracked file was touched, so the pin is intact for all reviewed content.

Nothing in the shared checkout `_tools/asma-rs-kontor` was committed, staged, branched,
stashed, reset, or modified. It moved again during this gate (now on
`feat/ASMA-7869-command-execution-mode`), which is exactly why the pinned worktree was used.
The only thing written there is this report as a new untracked file, left uncommitted.

`df64004` is PR #70's squash merge and the tip of the reviewed union #67–#70
(`c528e9b~1..df64004`).

---

## 2. Gates — run by this instance

`~/.cache/op19-qa/GATES.txt` did not exist at relaunch; the predecessor had written
`gates.sh` but was killed before it produced output. Run here. Exit codes are the real `$?`
of each cargo invocation — the script pipes nothing, so no `grep` swallows the status.

| Gate | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | **0** |
| `cargo clippy --workspace --all-targets` | **0** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **0** |

Logs: `~/.cache/op19-qa/logs/{fmt,clippy,clippy-deny}.log`. The `-D warnings` run closes the
gate the code review explicitly listed as *not verified by it*.

Still **not** run by anyone (out of scope for a Rust-workspace qa-gate, and unverified):
`cargo audit`, `cargo deny check`, `pnpm audit --prod`, and the `apps/console`
`verify:api` / `typecheck` / `test` / `build` gates.

---

## 3. Test suite — reused from the pre-restart run, plus a correction

`~/.cache/op19-qa/EXITCODES.txt` records **20 packages, every one `EXIT=0`, `attempts=1`**,
produced by the predecessor's sequential per-package run **on this same pinned `df64004`
worktree**. Reused as this gate's test evidence rather than re-run:

| Package | Exit | Duration |
| --- | --- | --- |
| kontor-core | 0 | 18s |
| kontor-store | 0 | 489s |
| kontor-calendar | 0 | 5s |
| kontor-scheduler | 0 | 15s |
| kontor-policy | 0 | 11s |
| kontor-runtime | 0 | 2s |
| kontor-profiles | 0 | 12s |
| kontor-teams | 0 | 5s |
| kontor-context | 0 | 3s |
| kontor-runtime-paseo | 0 | 17s |
| kontor-tests-contract | 0 | 72s |
| kontor-api | 0 | 31s |
| kontor-accounts | 0 | 30s |
| kontor-integrations-asma | 0 | 17s |
| kontor-runtime-ao | 0 | 11s |
| kontor-runtime-codex | 0 | 5s |
| kontor-cli | 0 | 30s |
| kontor-intake | 0 | 3s |
| kontor-mcp | 0 | 16s |
| kontor-desktop | 0 | 61s |

No `EXIT=143` retries occurred (every row is `attempts=1`), so no result here is a
lock-contention artefact.

### 3.1 Correction to the handoff — two packages had NOT completed

The relaunch brief stated the run covered 20 packages "`kontor-daemon` included". It did
not. `EXITCODES.txt` has **no terminating `DONE` line** and stops at `kontor-desktop`;
`run-tests.sh`'s package list has 22 entries. `logs/kontor-daemon.log` was still being
appended at 20:20 — later than `EXITCODES.txt` (20:16) — and ends mid-stream with no
`test result:` summary. `logs/kontor-tests-e2e.log` did not exist at all.

So the pre-restart evidence covered 20 of 22 packages. `kontor-daemon` was killed in
flight (every test it had reported was `ok`) and `kontor-tests-e2e` never started.
`kontor-daemon` is the package that matters most here — it owns `prepare_native_names` and
the loopback census assertions — so this gap was closed rather than waived:

| Package | Exit | Duration | Source |
| --- | --- | --- | --- |
| kontor-daemon | **0** | 255s | RE-RUN BY THIS INSTANCE on the same pinned `df64004` tree |
| kontor-tests-e2e | **0** | 54s | RE-RUN BY THIS INSTANCE on the same pinned `df64004` tree |

Both `attempts=1` — no `EXIT=143` retry. `kontor-daemon`'s `tests/loopback_api` reports
**195 passed, 0 failed** (243.74s), which matches the count the reviewer observed
independently at the same SHA.

Recorded in `~/.cache/op19-qa/EXITCODES-RELAUNCH.txt`, logs in the same `logs/` directory.
Both are additionally corroborated by the code reviewer's independent clean run at the
identical SHA (`~/.cache/op19-review/clean/EXITCODES.txt`: `kontor-daemon exit=0`,
`kontor-tests-e2e exit=0`).

**All 22 workspace packages are green at `df64004`.**

### 3.2 Counts independently observed

`kontor-runtime-paseo` `tests/contract`: **138 passed, 0 failed** — confirmed from this
gate's own log, not taken from the review. This is the number F11 is about (see §5).

`kontor-daemon` `tests/loopback_api`: **195 passed, 0 failed**, including
`a_legacy_jira_import_materializes_semantic_epic_control_and_ticket_titles`, the single test
F7 is about.

---

## 4. Behavioural checks against the pinned tree

Three claims were checked directly in the code rather than accepted from the review.

### 4.1 Naming fails CLOSED on a missing token, with no derivation fallback — **confirmed**

`NativeNameTemplate::render` (`crates/kontor-core/src/naming.rs:249`) is pure: it takes only
a `NameSeparator` and an explicit `NativeNameValues` map. Each segment is either a literal
or `values.require(*token)`, collected into `DomainResult<Vec<_>>` — so the **first** absent
token short-circuits the whole render into `Err`. There is no `unwrap_or`, no
`unwrap_or_default`, no `or_else` fallback, and no derivation anywhere on the path.
`require` (`naming.rs:311-320`) maps each of the four tokens to a distinct refusal naming
the missing contract. The function's own doc comment states it: *"No fallback or derived
value is available to this function."*

`NativeNameValues` can only be populated through the four explicit `with_*` builders; its
`Default` is an empty map, which fails closed rather than filling anything in.

Covered by `crates/kontor-core/tests/native_naming.rs:112`
`every_missing_token_fails_closed_and_names_the_missing_contract`, which loops all four
tokens, renders against an empty `NativeNameValues`, and asserts each refusal names its
token — with the assertion message *"missing identity must never be inferred"*.

This is the ticket's central contract, and it is genuinely tested.

### 4.2 `preview_hash` really covers `capability` and `observed_title` — **confirmed**

The hash is computed at `crates/kontor-daemon/src/applications.rs:4879` over
`{schema_version, project_id, epic_id, project_revision, targets}`, where `targets` is
`Vec<NativeNameTargetDto>` serialized by `serde`.

`NativeNameTargetDto` (`crates/kontor-api/src/applications.rs:2040-2071`) carries
`observed_title`, `capability`, `provider_session_id` and `would_change`, and I confirmed
**no field bears `skip_serializing_if`** — so every one of them is a hash input, including
when `observed_title` / `provider_session_id` are `None` (serialized as `null`, not omitted).

Consequence, which is what makes #68's sparse-seat tolerance safe: a seat that flips
`ready` ↔ `rename_pending`, or whose observed title moves, between preview and apply changes
the digest, and the apply refuses the **whole plan** with `RevisionConflict` rather than
half-applying.

### 4.3 #69's stale-binding vs correlation-drift distinction is observable only at
`prepare_native_names`, and is tested — **confirmed, with one nuance**

I re-verified the containment myself rather than assuming it:

* `ApiError::from_runtime` (`crates/kontor-api/src/error.rs:575-581`) folds
  `StaleBinding | CorrelationFailed | SessionAlreadyBound` into **one** `ApiErrorCode::StaleBinding`
  with **one** identical message.
* `ProbeRefusal::of` (`crates/kontor-accounts/src/capacity.rs:76-84`) folds both into
  `Unreachable` via a `_` wildcard.
* `grep -rn "CorrelationFailed" crates/kontor-daemon/src/` returns **nothing** — the daemon
  never matches the variant.

The single typed observation point is the match in `prepare_native_names`
(`applications.rs:4813-4818`): `StaleBinding | ProviderUnavailable` → the seat degrades to a
`rename_pending` census target and the epic proceeds; any other error propagates and refuses
the whole plan. So post-#69 a missing agent is tolerated where pre-#69 it refused the epic.

**Tested — at two layers, joined only by convention.** This is the nuance:

* The Paseo classification is tested: `crates/kontor-runtime-paseo/tests/contract.rs:4043`
  `seat_retitle_classifies_an_exact_missing_native_agent_as_stale`, asserting
  `StaleBinding` off the durable `protocol/agent-not-found.json` fixture. A regression of
  #69 back to `CorrelationFailed` **would** turn this test red.
* The daemon-level census tolerance is tested in `crates/kontor-daemon/tests/loopback_api.rs`
  (`:16570`, `:16598`) — `capability == "rename_pending"`, `observed_title == null`,
  `would_change == false`, surviving into the post-apply readback, while an independent stale
  container in the same epic is still repaired.
* **But the daemon test drives `ScriptedFakeRuntime`, not Paseo.** It induces the state with
  `world.fake.forget_seat(...)`, and the fake independently hardcodes
  `RuntimeError::StaleBinding { rule: "the persistent native seat is absent" }`
  (`crates/kontor-runtime/src/fake.rs:2018-2021`). The fake *mirrors* the real classifier by
  convention; nothing joins them. If the Paseo adapter regressed, the daemon loopback test
  would stay green and only the single contract test at `contract.rs:4043` would fire.

That is adequate — the guard exists — but it is a **single-test guard** on the classification
this ticket's #69 turns on, and the end-to-end join is unproven. Recorded as a coverage
observation, not a defect.

---

## 5. Main deliverable — would the suite catch a regression of each finding?

Read against the code review at `~/.cache/asma-7869-evidence/KON-OP-19-REVIEW-NOTES.md`
(**VERDICT: PASS**, 11 findings: 3×P2 — F1, F3, F4; 8×P3; none blocking; F1 flagged as a
prompt follow-up).

Method: static — the test surface at `df64004` was inspected directly (test-function
inventory per file, plus exhaustive greps for the states and helpers each finding needs).
For every gap below the proof is **absence of the precondition**: a test cannot assert
behaviour for a state it never constructs. No mutants were run; the prompt scoped this
relaunch to analysis, and absence-of-precondition is conclusive on its own.

| # | Sev | Finding (short) | Caught by the suite? |
| --- | --- | --- | --- |
| F1 | P2 | Native-name repair can erase a live seat's `provider_session_id` on a `Some → None` observation | **NO — GAP** |
| F2 | P3 | Dead expression: `rename_pending` target always reports `provider_session_id: null` | **NO — GAP** |
| F3 | P2 | Epic execution-scope row demanded before the template is consulted; token-free container refused | **NO — GAP** |
| F4 | P2 | `validate` has no `native_root` container rule, arming name-keyed adoption | **NO — GAP, and counter-locked** |
| F5 | P3 | Non-`Active` seat skip is untested and silent | **NO — GAP** |
| F6 | P3 | F1's `Some → None` transition untested; helper already supports it | **NO — GAP (this finding *is* the gap)** |
| F7 | P3 | Nine mutants share one 96-assertion killer | N/A — confirmed, test-architecture risk |
| F8 | P3 | `retire_session` `# Errors` doc stale after #69 | **NO — unfalsifiable by tests** |
| F9 | P3 | Migration 47 canonicalizes only the bundled spec | **NO — GAP** |
| F10 | P3 | One spec revision applied to a whole lineage | **NO — GAP** |
| F11 | P3 | `MUTATION.md` gate counts under-report the tree | **NO — doc drift; confirmed by me** |

**Seven behavioural coverage gaps: F1, F2, F3, F5, F6, F9, F10.** F4 is worse than a gap.
F7, F8 and F11 are not behavioural claims, so "covering test" is the wrong frame for them.

### Per-finding evidence

**F1 — NO covering test.** The only provider-session-refresh test is `Some → Some`:
`loopback_api.rs:16190` sets `set_seat_provider_session(&lsa_native, Some(resumed_provider_session))`.
`grep -rn "set_seat_provider_session" crates/` outside `fake.rs` returns **exactly that one
line**, and it passes `Some`. Nothing anywhere asserts the `None` outcome.

I confirmed the erasure path is reachable and *not* blocked by any downstream guard:
`bind_hosted_topology_seat` (`crates/kontor-store/src/repository.rs:2221-2237`) tests replay
equality on `model_rung` and `native_identity` **only**, then writes
`provider_session_id = ?3` unconditionally; and the apply-side correlation check
(`applications.rs:9964`) requires `outcome.provider_session_id == request.provider_session_id`,
which is *satisfied* when both are `None` — because #70 builds the request with `None`
(`:4809`) and preview freezes the observation into it (`:4849`). The write at `:9986` is
`hosted.provider_session_id = outcome.provider_session_id.clone()` with no `is_some()` guard
and no `.or(persisted)`.

So the reviewer's one-line fix could be applied **and later reverted with the suite still
fully green**. This is the most consequential gap, and it is the finding already flagged as
the prompt follow-up. F6 is the same gap stated as a test task; `ScriptedFakeRuntime::set_seat_provider_session`
(`crates/kontor-runtime/src/fake.rs:1273-1282`) already takes an `Option`, so the covering
test is one line.

**F2 — NO covering test.** `loopback_api.rs:16570-16598` asserts the pending target's
`native_id`, `observed_title`, `capability` and `would_change` — but **never**
`provider_session_id`. The dead clone at `applications.rs:4826` is therefore unpinned in
both directions: the current always-`null` behaviour is unasserted, and reading the persisted
value instead would break no test. The field does feed `preview_hash`, but no test asserts a
literal digest for the pending case (the apply echoes the preview's own hash back), so the
hash provides no signal here either.

**F3 — NO covering test.** The two refusal strings, `"the epic has no durable native-name tokens"`
(`applications.rs:19043`) and `"the epic seat has no durable native-name tokens"` (`:19164`),
appear **only** in `src/` — a repo-wide grep finds no test referencing either. Neither the
deliberate fail-closed behaviour nor the over-broad firing on a token-free template
(`QSW`/`ASW`/`CSW` single-literal containers, `migrations.rs:606-620`) is exercised. Matches
the review's "Untested in either direction".

**F4 — NO covering test, and the suite actively locks in the permissive behaviour.** This is
the one I would flag beyond the review's own framing. `validate`
(`crates/kontor-core/src/spec.rs:705-719`) conditions on `SessionHost` to require a
`seat_name_template`, and imposes **no** constraint on a `native_root` kind's container
template. Worse, the baseline fixture at `crates/kontor-core/tests/spec_validation.rs:86-93`
declares `PSW` with `"projection_capabilities": ["native_root", "session_host"]` and a
bare-literal container template `"Project Session Workspace"`, and
`topology_naming_requires_typed_container_and_hosted_seat_templates` (`:106`) **asserts that
this validates**. So the reviewer's recommended fix — require ≥1 token segment for
`native_root` — cannot be applied without editing this test. The gap is not merely uncovered;
it is pinned open. Worth saying plainly in the follow-up so the fix is not mistaken for a
test regression.

**F5 — NO covering test.** `grep -rn "TopologyLifecycle::Retired\|TopologyLifecycle::Archived" crates/*/tests/`
returns **zero hits across every test directory in the workspace**. No test constructs a
`Retired` or `Archived` `SeatBinding`, so nothing can assert the skip at
`applications.rs:4727`. Deleting that `continue` would leave the suite green. This is the
guard the review's judgement 3 leans on for the archived-but-dead seat, which makes it the
most under-tested guard relative to the weight placed on it.

**F6 — the finding is itself the gap.** Confirmed above under F1.

**F7 — confirmed, and slightly larger than stated.** The killer test
`a_legacy_jira_import_materializes_semantic_epic_control_and_ticket_titles` spans
`loopback_api.rs:15698` to `:16609` (~911 lines, ~106 `assert*` macros; the review said 96).
I verified it is the **sole owner** of every daemon-level native-name assertion I could find:
both `rename_pending` assertions, the readback assertion, and the `Some → Some`
provider-session refresh. So the concentration is real: one edit reddens one test standing
in for nine contracts, and attribution will be painful. Not a defect — a test-architecture
risk, correctly rated P3.

**F8 — unfalsifiable by tests.** `crates/kontor-runtime-paseo/src/adapter.rs:2487-2491` still
documents `CorrelationFailed` for a readback that does not report the agent archived. It is
prose in a `# Errors` block, not a doctest, so neither `cargo test` nor `cargo test --doc`
can observe the drift. Zero behavioural risk (§4.3: both variants collapse identically at
the API boundary); pure documentation debt.

**F9 — NO covering test.** Three v47 migration tests exist and are good ones:
`v46_to_v47_canonicalizes_only_the_known_builtin_hash_and_every_reference`
(`crates/kontor-store/tests/schema_v1.rs:456`), `v47_refuses_the_builtin_identity_when_its_prior_hash_is_unknown`
(`:500`), and `v47_refuses_an_unknown_reference_hash_before_rewriting_the_builtin` (`:523`).
But all three seed only the bundled spec via `seed_v46_operational_topology`, and the
canonicalization test reads each table with `SELECT … LIMIT 1` — a single-row world. **No
test seeds a second, custom published topology revision** and carries it through v47, which
is exactly F9's scenario. Bounded by whether any realm ever published a custom spec, which I
did not inspect (see §6).

**F10 — NO covering test.** `project_topology_defaults` appears in tests only inside
`schema_v1.rs` (`:113`, `:401`, `:477`, `:533`), all migration-shaped. No test constructs a
lineage whose project-level node and epic-level nodes are pinned to *different*
`(spec_id, version)` pairs, so the wrong-revision ancestor rendering at
`applications.rs:18829`/`:18934` is unexercised. Pre-existing shape, inherited rather than
introduced by this change set.

**F11 — confirmed independently.** `MUTATION.md` claims "kontor-runtime-paseo contract: 135
passed" in both its "Full gates" and "Live sparse-seat repair verification" blocks. My own
log for this gate shows **138 passed, 0 failed** at `df64004`
(`~/.cache/op19-qa/logs/kontor-runtime-paseo.log`). The direction is safe — more tests pass
than claimed, none fail — but the figures were captured mid-series, before #69 and #70 added
tests, and should be refreshed before that document is cited as a release gate.

### What *is* well covered

Worth stating, because the gap list above is long and could read as alarming. The 11 findings
are precisely the **periphery**; the change set's central contracts are genuinely tested:

* Fail-closed rendering of all four tokens, exact bullet bytes, separator-only revision, and
  `AI_SHORT_NAME` unicode handling — `crates/kontor-core/tests/native_naming.rs` (5 tests).
* Legacy / unknown / empty / duplicate / punctuated template refusal and separator validation
  — `spec_validation.rs:126`, `:163`.
* `preview_hash` covering the full target shape, so preview↔apply drift refuses the whole
  plan rather than half-applying (§4.2).
* #69's missing-agent classification at the adapter — `contract.rs:4043`.
* #70's route-and-identity preservation across a provider-thread resume — the store guard at
  `repository.rs:2221-2243` plus the loopback refresh test.
* The sparse-seat census surviving into the post-apply readback while an independent stale
  container is still repaired — `loopback_api.rs:16540-16598`.

No finding produces a wrong name, a lost identity, or a wrong model route — my reading of the
code agrees with the review on that.

---

## 6. Not verified

Stated plainly rather than implied:

* **Mutation testing not performed.** No mutants were seeded or run; `mutate.sh` was left
  unused by the predecessor and `MUTANTS.txt` does not exist. Coverage conclusions in §5 are
  static (absence of the required precondition in every test), which is conclusive for the
  gaps claimed but does not independently re-validate `MUTATION.md`'s 15 mutant kills. The
  review checked those against the code and found them real; I did not re-derive them.
* **Non-Rust gates.** `cargo audit`, `cargo deny check`, `pnpm audit --prod`, and the
  `apps/console` `verify:api` / `typecheck` / `test` / `build` gates were not run by anyone
  across the review or this gate.
* **The live realm.** Not inspected, not touched. Whether any realm has published a custom
  topology revision — which bounds F9's real-world reach — is therefore unknown.
* **F3's failure scenario not reproduced.** Judged from the code path (the unconditional
  `ok_or_else` at `applications.rs:19036-19045` preceding `render`), not by driving a
  Quick-session promotion lacking an `execution_scope`.
* **The end-to-end join in §4.3.** That `ScriptedFakeRuntime`'s hardcoded `StaleBinding`
  still mirrors the real Paseo classifier is true at `df64004` by inspection, but no test
  enforces the correspondence, so it can silently drift.
* **Concurrent work.** `fix/ASMA-7869-serve-api-during-startup-reconciliation` is being
  changed by another session. Per the review, #69 provably cannot alter
  startup-reconciliation behaviour (both call sites discard the error variant), so nothing
  here judges that work.

---

## 7. Verdict

**VERDICT: PASS**

All 22 workspace packages are green at `df64004`, and all three formatting/lint gates pass
including `clippy -D warnings`, which the code review had left unverified. The ticket's
central claim — deterministic naming rendered from the node's own pinned specification,
failing closed on any missing token with no derivation anywhere — holds under direct
inspection and is properly tested. `preview_hash` covers the full target shape, which is what
makes #68's sparse-seat tolerance safe rather than merely convenient. #69's classification is
contained to one observation point and is guarded by a test.

Nothing found here blocks. The gate passes on behaviour; the deficit is in test coverage of
the periphery, and it is recorded rather than waived:

1. **F1/F6 — `Some → None` provider-session erasure has no covering test** (the flagged
   prompt follow-up). The one-line fix could be applied and reverted with the suite green.
   Fix and test together, or the fix will not stay fixed.
2. **F4 — the fix is pinned open by a test.** `spec_validation.rs:86-93` + `:106` assert that
   a `native_root` with a bare-literal container validates. Whoever hardens this must edit
   that fixture; it is not a test regression.
3. **F5 — zero tests in the entire workspace construct a `Retired`/`Archived` seat**, so the
   lifecycle skip that judgement 3 leans on is wholly unguarded.
4. F2, F3, F9, F10 are uncovered as described; F7 (single-killer concentration), F8 (stale
   doc) and F11 (stale `MUTATION.md` counts — 135 claimed, 138 actual) are documentation and
   test-architecture debt.

*This report records no Kontor gate. The LSA records it citing this verdict.*
