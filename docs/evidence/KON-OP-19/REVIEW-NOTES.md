VERDICT: PASS

# KON-OP-19 / ASMA-7967 — independent post-hoc review

> **Reviewer:** independent inspector seat (replacement for the seat lost in the 2026-08-21 Codex outage)
> **Date:** 2026-08-21
> **Status:** complete — verdict above
> **Scope:** the union of PRs #67–#70 in `_tools/asma-rs-kontor`, reviewed as one change set
> **Category:** report

This review is post-hoc: all four PRs were merged to `origin/master` with green CI and
without code review. Nothing here records a Kontor gate; the LSA records it citing this
verdict.

---

## What was reviewed

Reviewed as one change set, by explicit SHA (all four are ancestors of the checkout's
current HEAD, so the ranges resolve regardless of which branch is checked out):

| PR | Merge | Own commit | Subject |
| --- | --- | --- | --- |
| #67 | `c528e9b` | `a18221b` | feat(topology): Render native names from pinned specification |
| #68 | `0c58e72` | `1defdb2` | fix(topology): Tolerate sparse native-name seats |
| #69 | `367a711` | `4ec7ab1` | fix(ASMA-7967): classify missing Paseo seats as stale |
| #70 | `df64004` | `df64004` (squash) | fix: Refresh hosted seat provider session on native-name repair |

Union range: `git log --oneline c528e9b~1..df64004`. Each merge's own range was diffed
separately. `docs/evidence/KON-OP-19/MUTATION.md` was read first and then checked against
the code rather than trusted; deviations are recorded in F7 and F11.

Every `file:line` citation below is against **`df64004`**, read from a clean detached
worktree, not from the shared checkout (whose line numbers differ).

## Tests run

The shared checkout `_tools/asma-rs-kontor` is worked in concurrently by other sessions and
moved branches twice during this review. Tests were therefore **not** run there. A clean
detached worktree was created at `~/.cache/op19-review/tree` pinned to `df64004`:

```
tree HEAD:  df64004f901ff3edca1c10191939e9d6ce351b53
tree dirty: []                      (git status --porcelain: empty)
```

Per-package, sequential, full logs in `~/.cache/op19-review/clean/`. Exit codes are the real
`$?` of each `cargo test` invocation — no pipes, no `grep` swallowing the status:

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | **0** |
| `cargo test -p kontor-core` | **0** |
| `cargo test -p kontor-store` | **0** |
| `cargo test -p kontor-runtime` | **0** |
| `cargo test -p kontor-runtime-paseo` | **0** |
| `cargo test -p kontor-runtime-ao` | **0** |
| `cargo test -p kontor-runtime-codex` | **0** |
| `cargo test -p kontor-teams` | **0** |
| `cargo test -p kontor-profiles` | **0** |
| `cargo test -p kontor-api` | **0** |
| `cargo test -p kontor-mcp` | **0** |
| `cargo test -p kontor-daemon` | **0** |
| `cargo test -p kontor-tests-contract` | **0** |
| `cargo test -p kontor-tests-e2e` | **0** |

Notable counts at `df64004`: `kontor-runtime-paseo` `tests/contract` **138 passed**;
`kontor-daemon` `tests/loopback_api` **195 passed**, including
`a_legacy_jira_import_materializes_semantic_epic_control_and_ticket_titles`, the test that
`MUTATION.md` names as the killer for nine of its fifteen mutants.

### Contaminated first attempt — disclosed, not used

A first run was started in the shared checkout before its state was known, and was killed
partway. That tree was `HEAD 1a9354e` on branch `feat/ASMA-7882-quota-signal-classifier`
(master + one unrelated quota-signal commit) with `M crates/kontor-daemon/tests/loopback_api.rs`
(+168 lines) belonging to another session, plus two untracked directories. Its partial
results (`~/.cache/op19-review/EXITCODES-contaminated-tree.txt`, 8 packages, all 0) are
retained for completeness and **attest nothing about the merged change set**. The verdict
rests only on the pinned-`df64004` run above.

### Gates NOT run

Not verified by me, and `MUTATION.md`'s claims about them are unchecked:
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo audit`, `cargo deny check`,
`pnpm audit --prod`, and the `apps/console` `verify:api` / `typecheck` / `test` / `build`
gates.

---

## The four judgements asked for

### 1. Is native naming genuinely driven by the pinned specification? — **Yes.**

Verified, not taken on trust:

* The pre-change daemon built native names with hardcoded and *derived* strings.
  `a18221b` deletes `format!("Advisor · {}", run.id.as_text())`,
  `format!("{} · {}", slot.id.as_str(), run.id.as_text())`, two more per-seat `format!`
  blocks, and — most importantly — the catch-all container fallback
  `format!("{template} · {}", node.id)`. That last one is exactly the "derived string" the
  ticket exists to eliminate.
* Names now come from `NativeNameTemplate::render` (`crates/kontor-core/src/naming.rs:249`),
  which is pure: it takes only a separator and an explicit `NativeNameValues` map, and a
  missing token is a refusal (`naming.rs:300-320`) with **no** fallback and no derivation.
  `crates/kontor-core/tests/native_naming.rs` asserts the exact bullet bytes, the
  separator-only revision, and that each of the four tokens fails closed naming itself.
* The template is read from the node's **own pinned revision** —
  `get_topology_spec(project_id, node.topology.spec_id, node.topology.version)` at
  `crates/kontor-daemon/src/applications.rs:4543`, `:18829` and `:19121` — not from the
  latest published spec.
* Token *values* cannot smuggle a separator: literals carrying `U+2022`/`U+00B7` are refused
  at `validate` (`naming.rs:225-233`), `NameSeparator::parse` refuses `U+00B7`
  (`naming.rs:38-43`), `AiShortName::parse` refuses both glyphs (`naming.rs:92-97`), and the
  remaining token types are `ExternalId`/role codes.
* Paseo no longer formats titles at all. Every title in the adapter comes from
  `request.display_name` / `request.desired_title` (`crates/kontor-runtime-paseo/src/adapter.rs:2227`,
  `:2252`, `:3434`, `:3586`, `:4319`, `:4734`, `:4802`). This confirms M8: the adapter-local
  `TSW · JIRA · short-code` formatter in `prepare_workspace` is gone.
* The adapter's config-driven `effective_scope` (`adapter.rs:1346`) overrides only
  correlation tokens (Jira key, short title, task issue key, short code) and the plan-item
  key. It never reaches a title. I checked every one of its ~20 call sites.

The one remaining non-spec path is `render_legacy_container_name`
(`applications.rs:19674`), read-compatibility for pre-v47 opaque template strings. It does
placeholder substitution only and **refuses** if no recognized placeholder resolved
(`:19693-19697`), so it is strictly narrower than the id-appending fallback it replaced. It
is unit-tested at `applications.rs` `mod tests`, including that opaque prose is refused.

### 2. Does #68's sparse-seat tolerance hide real drift? — **No.**

It tolerates only absence that is legitimate by the domain's own vocabulary, and it does not
weaken any drift check:

* `current_delivery_role_leaf` returns `None` **only** when the role has zero `AgentRun`s
  (`applications.rs:1003`) — a logical role slot declared before its first run. Every
  broken-chain refusal is untouched (`:988`, `:994`, `:1017`, `:1023`, `:1033`). I confirmed
  `prepare_native_names` is its **only** caller, so the `Option` signature change has no
  other blast radius.
* The `seat.lifecycle != TopologyLifecycle::Active` skip (`:4727`) matches the enum's own
  documentation: `Active` is defined as "Available for materialization and reconciliation"
  (`crates/kontor-core/src/state.rs:1776`). Skipping `Retired`/`Archived` seats is correct.
* An unreadable seat is **not** silently dropped. It stays in the census as
  `capability: "rename_pending"` with `observed_title: null` (`:4816-4831`), and the
  post-apply `readback` re-runs the full preflight so it remains visible to the operator
  (`:10000-10010`).
* The preview→apply gate is tight, which is what actually prevents a masked partial apply.
  `NativeNameTargetDto` (`crates/kontor-api/src/applications.rs:2039-2071`) has **no**
  `skip_serializing_if`, so `capability`, `observed_title`, `provider_session_id` and
  `would_change` are all inputs to `preview_hash`. A seat that flips ready↔pending between
  preview and apply changes the hash and the apply refuses with `RevisionConflict`
  ("native names or identities changed since the caller's complete preview"), rather than
  half-applying. Identity drift and structural ambiguity still refuse the whole plan
  (`:4844-4848`, `:4699-4703`).
* The receipt is recorded **before** the first external effect, with a comment saying so,
  which is what makes a lost acknowledgement replay safely. This confirms M3.

The one soft edge: a `rename_pending` target reports `would_change: false`. A consumer that
looks only at `would_change` would read a pending seat as converged. The distinct
`capability` value is the mitigation, and it is in the hash. Recorded as a documentation
point rather than a defect.

### 3. Is #69 correct for an archived seat versus one that never existed? — **Correct, and NEUTRAL for the `replace_seat` deadlock. Partially tested.**

The classification itself is right: a `null` agent means the exact persisted binding is
stale, whereas an answer carrying a *different* agent id is genuine correlation drift, and
`fetch_agent` (`adapter.rs:1293-1303`) now distinguishes exactly those two.

**Blast radius is contained, and I verified this rather than assuming it.** `fetch_agent` has
~20 callers, so the reclassification could have leaked. It does not:

* `ApiError::from_runtime` maps `StaleBinding`, `CorrelationFailed` and
  `SessionAlreadyBound` to the *same* `ApiErrorCode::StaleBinding` with the *same* message
  (`crates/kontor-api/src/error.rs:575-581`).
* `ProbeRefusal::of` folds both into `Unreachable` (`crates/kontor-accounts/src/capacity.rs:76-84`).
* No code in `crates/kontor-daemon/src` matches on `RuntimeError::CorrelationFailed` at all.

So the only place in the system that can observe the difference is the typed match in
`prepare_native_names` (`applications.rs:4815-4818`) — the intended one.

**On the concrete deadlock** (an archived-but-dead seat in this realm cannot reach
runtime-observed terminal state, wedging `replace_seat`):

* `replace_seat` requires `terminal.outcome == Cancelled` **and**
  `terminal.source == RuntimeObservation` (`applications.rs:15927-15942`).
* Reaching that evidence goes through `retire_predecessor_for_replacement`, which first
  requires an **in-process frozen capability snapshot**: `state.sessions().get(binding.id)`,
  else `StaleBinding` "this process holds no frozen capability snapshot for the predecessor"
  (`:17514-17519`). After a daemon restart there is none. That, plus the terminal-evidence
  gate, is the actual cause of the wedge.
* Neither is touched by #67–#70. And for the missing-agent case specifically, `retire_session`
  → `fetch_agent_with_archive` → `fetch_agent` produced `CorrelationFailed` before and
  `StaleBinding` now, which `from_runtime` collapses to an identical refusal. **The caller
  cannot tell the difference.**

Therefore: **neutral — neither helps nor hurts.** One mild secondary benefit, from #68 rather
than #69: because non-`Active` seats are now skipped (`:4727`) and unreadable exact seats
degrade to `rename_pending`, a dead archived seat no longer refuses the *whole* epic's name
reconcile. That unblocks naming, not `replace_seat`.

**Tested?** Partially. The adapter-level classification is tested
(`crates/kontor-runtime-paseo/tests/contract.rs`,
`seat_retitle_classifies_an_exact_missing_native_agent_as_stale`, using the durable
`protocol/agent-not-found.json` fixture), and the daemon-level pending census is tested in
the loopback mega-test. The `retire_session` / `replace_seat` path with a missing agent is
**not** tested. Because both variants map identically there, that is a coverage gap rather
than a behaviour risk. See F5/F6.

**Startup reconciliation — explicitly out of scope of this change's effect.** Flagged because
another session is concurrently changing that area on
`fix/ASMA-7869-serve-api-during-startup-reconciliation`. Pinned to the four SHAs above: both
startup/restore call sites read the agent as
`let Ok(agent) = self.fetch_agent(...).await else { continue }` — `restore_bindings` at
`adapter.rs:3705-3710` and the placement recovery at `:5728-5730`. Both discard the error
*variant* entirely. A missing agent was skipped before #69 and is skipped after it,
identically. **#69 provably cannot change startup-reconciliation behaviour**, so nothing in
this verdict should be read as a judgement on that concurrent work.

### 4. Can #70's provider-session refresh clobber a healthy seat's route? — **The route is safe. The provider-session handle is not.** See F1.

* The **route** is safe: `bind_hosted_topology_seat` still compares `model_rung` and
  `native_identity`, and a mismatch is still `RepositoryError::Conflict`
  "a persistent seat cannot change its route or native identity"
  (`crates/kontor-store/src/repository.rs:2221-2243`). #70's test asserts both survive a
  provider-thread resume.
* Correlation is still exact: apply verifies `outcome.provider_session_id ==
  request.provider_session_id` (`applications.rs:9964`), and because `provider_session_id` is
  in `preview_hash`, a thread that moves *again* between preview and apply forces a
  `RevisionConflict` rather than a silent write.
* But a `Some → None` transition is unguarded and erases durable evidence. That is F1.

---

## Findings

Severity: **P1** blocks, **P2** should be fixed but does not block, **P3** informational.
None of the findings below produces a wrong name, a lost identity, or a wrong model route.

### F1 — P2 — does not block
**A native-name repair can erase a live seat's durable provider-session handle.**
`crates/kontor-store/src/repository.rs:2222` and `crates/kontor-daemon/src/applications.rs:9986`

#70 removed `provider_session_id` from the replay-equality guard in
`bind_hosted_topology_seat` (`repository.rs:2221-2223`) and made the `UPDATE` overwrite it
unconditionally (`:2226-2237`). Independently, `prepare_native_names` queues a seat action
whenever the persisted handle merely *differs* from the freshly observed one
(`applications.rs:4866-4869`) — including when the observation is `None`. Nothing in either
site distinguishes "resumed onto a newer thread" (the intended case, per the store's own new
doc comment at `repository.rs:2211-2214`) from "reported no thread at all".

Reachable because `PaseoAgent::provider_session_id()`
(`crates/kontor-runtime-paseo/src/wire.rs:696-700`) is
`self.persistence.as_ref().and_then(|h| h.session_id.as_deref())` — `None` whenever the
`persistence` block or its `session_id` is absent. `preview_retitle_seat` only checks a
*supplied* expectation (`adapter.rs:4788-4796`, `is_some_and`), and since #70 the daemon
supplies `None`, so nothing objects.

*Failure scenario:* a hosted LSA seat holds `provider_session_id = 01a01ea3-…`. An operator
runs `native-names:apply` at a moment when Paseo returns that same agent with no
`persistence` block. Preview observes `None`, `None != Some(01a01ea3-…)` so the action is
queued, apply writes `hosted.provider_session_id = None`, and the handle is gone. Before #70
that same combination was a loud `RepositoryError::Conflict`.

*Impact is bounded to evidence, not routing.* I traced every use of the stored field —
written at `applications.rs:6079`, `:6446`, `:9986`, `:11064`, `:11244`, surfaced through
`NativeNameTargetDto.provider_session_id` and `crates/kontor-api/src/applications.rs:660` —
and **none is a decision input**. No launch, resume, or message path reads it. So this is
observability loss, which is why it is P2 and not P1.

*Fix (one line):* refresh only on a positive observation —
`if outcome.provider_session_id.is_some()`, or `.or(persisted)` at `applications.rs:9986`.

*Aside:* because nothing reads the field, #70's stated justification ("the current provider
thread becomes the durable readback for later messages") overstates present necessity. The
change makes the observation truthful, which is worth having; no consumer depends on it yet.

### F2 — P3 — does not block
**Dead expression from an unreviewed #68 ↔ #70 interaction: every `rename_pending` target
reports a null provider session.**
`crates/kontor-daemon/src/applications.rs:4826`

#68 populated the pending target with `provider_session_id: request.provider_session_id.clone()`,
which at that time carried the *persisted* handle. #70 then changed the request to be built
with `provider_session_id: None` (`:4809`) and assigns the learned value only at `:4849` —
**after** the match arm. So `:4826` now unconditionally clones `None`, and a `rename_pending`
target always reports `provider_session_id: null` regardless of what Kontor holds. Neither PR
revisited the other's line; this is the characteristic residue of merging both without review.

Harmless today (the value is evidence only, and the field still participates in
`preview_hash` consistently), but it silently degrades the pending census and the expression
is dead. Fix: read the persisted value into the pending target, or drop the field there and
say so.

### F3 — P2 — does not block
**The durable epic-scope row is demanded before the template is consulted, so a
token-free container name is refused for want of tokens it never uses.**
`crates/kontor-daemon/src/applications.rs:19036-19045` (seat twin at `:19157-19166`)

In the epic branch, `get_epic_execution_scope(...).ok_or_else(... "the epic has no durable
native-name tokens")` fires **unconditionally**, before `render` is reached and therefore
before anything knows whether the pinned template actually requests
`KONTOR_BACKLOG_CODE` or `AI_SHORT_NAME`.

Failing closed here is *deliberate and correct in general* — the author documented exactly
this at `:11573-11579`: "if they omit the declaration, the pinned topology renderer below
fails closed before contacting the runtime rather than deriving a name from the Quick-session
purpose or generated epic id." That is the ticket's whole point, and I explicitly do **not**
report it as a defect. I initially assessed this as a P1 regression and withdrew that on
finding this comment.

What survives is narrower: the refusal also fires for kinds whose container name needs no
durable token at all. In the bundled v47 spec, `QSW`, `ASW` and `CSW` containers are
single-literal templates (`crates/kontor-store/src/migrations.rs:606-620`).

*Failure scenario:* a Quick session promoted to an epic without an `execution_scope`
declaration — a body shape the promotion path explicitly tolerates (`:11580`, "Older bodies
remain decodable") — cannot materialize its `ASW`/`QSW` container, with the refusal
"the epic has no durable native-name tokens", even though the name that would have been
rendered is the constant `"Advisor Session Workspace"`. Untested in either direction.

*Fix:* move the row lookup into `if let Some(durable) = …` and let `render` produce the
precise `missing KONTOR_BACKLOG_CODE` refusal, which is strictly more actionable anyway.
Compare the excellent task-path message at `:479-484` ("preview and apply an explicit epic
task mapping before materialization or retitle"), which tells the operator what to do; the
epic path does not.

### F4 — P2 — does not block
**`validate` gained a capability-conditional rule for seats but none for native roots,
leaving name-keyed project adoption armed for any future literal-named root.**
`crates/kontor-core/src/spec.rs:704-719`

`validate` now requires a `session_host` kind to declare a `seat_name_template` (`:706-718`),
but places no constraint on a `native_root` kind's container template — it may be a bare
literal, identical for every node that uses it.

That matters because Paseo's **only** name-keyed correlation adopts an exactly-one
display-name match and refuses only at two or more (`adapter.rs:2035-2061`,
"several Paseo projects carry this epic's display name"). One pre-existing native project
carrying the same title is silently *adopted*, not refused.

The bundled v47 spec does exactly the risky thing: `PSW` is `native_root` with the single
literal `"Project Session Workspace"` (`migrations.rs:605`). The new test suite's own
baseline fixture is a `native_root` + bare-literal container and asserts that it **validates**
(`crates/kontor-core/tests/spec_validation.rs:86-93`, `topology_naming_requires_typed_container_and_hosted_seat_templates`).
Before `a18221b` this was structurally impossible: every container name had the node id
appended, so no two could collide.

*I could not construct a live firing path and therefore record this as latent arming, not a
live defect.* The Paseo adapter keys its project binding by its single configured
`mini_project_id` (`adapter.rs:2016-2022`), and `ESW` — the native root actually reached
through `ensure_container` — renders uniquely per epic. But the template is now
operator-configurable, which is the ticket's purpose, so a future revision may legally put a
literal on a `native_root` and arm silent mis-adoption with no validation objecting.

*Fix:* mirror the `session_host` rule — require at least one token segment when
`projection_capabilities` contains `native_root`.

### F5 — P3 — does not block
**The non-`Active` seat skip is untested and silent.**
`crates/kontor-daemon/src/applications.rs:4727`

The guard is correct (see judgement 2), but no test constructs a `Retired` or `Archived`
`SeatBinding` and asserts it is excluded, and excluded seats are omitted from the census
entirely rather than reported — an operator cannot tell that an epic holds archived seats the
repair deliberately will not touch. This is the guard that matters most for the
archived-but-dead seat in judgement 3, so it deserves a test.

### F6 — P3 — does not block
**F1's `Some → None` transition is untested, and the test helper already supports it.**
`crates/kontor-daemon/tests/loopback_api.rs` (#70's additions)

#70 tests only `Some(old) → Some(new)`. `ScriptedFakeRuntime::set_seat_provider_session`
(`crates/kontor-runtime/src/fake.rs:1273-1282`) already takes an `Option`, so covering the
erasure is a one-line test.

### F7 — P3 — does not block
**Nine of fifteen mutants share a single 96-assertion killer.**
`docs/evidence/KON-OP-19/MUTATION.md`

M3–M7, M11, M12, M14 and M15 all name "same whole-epic QNR regression" — the single test
`a_legacy_jira_import_materializes_semantic_epic_control_and_ticket_titles`, which carries 96
assertions and runs 266s. I confirmed the kills are real and the test passes, and I verified
by reading the code that each cited guard exists. But nine independent contracts sharing one
red signal means a future edit that breaks it yields one failure for nine guarantees, and
attribution will be painful.

### F8 — P3 — does not block
**`retire_session`'s `# Errors` documentation is stale after #69.**
`crates/kontor-runtime-paseo/src/adapter.rs:2489-2491`

It still documents `CorrelationFailed` for a readback that does not report the agent
archived; a *missing* agent now arrives as `StaleBinding`. Behaviour at the API boundary is
unchanged (both collapse identically); only the doc drifted.

### F9 — P3 — does not block
**Migration 47 canonicalizes only the bundled spec, and neither detects nor reports any
other published revision.**
`crates/kontor-store/src/migrations.rs:456-467`

The migration rewrites only `spec_id = OPERATIONAL_TOPOLOGY_SPEC_ID` at `version = 1`, and is
strict about it — an unexpected prior hash or an unknown reference hash aborts the upgrade
(`:483-489`, `:511-521`), which is the right instinct. Two v46→v47 refusal fixtures cover
that.

Any *other* published topology revision keeps its pre-v47 opaque string. Such a spec is
still readable — `get_topology_spec` deserializes without validating
(`crates/kontor-store/src/repository.rs:4026-4049`), and the `Legacy` variant exists for
exactly this — but `NativeNameTemplate::validate` now always refuses it, so its containers
fall to `render_legacy_container_name` (which refuses prose lacking a recognized placeholder,
`applications.rs:19693-19697`) and its seats refuse outright for want of a
`seat_name_template` (`:19133-19140`). The migration says nothing about such rows and no test
covers one. Bounded by whether any realm ever published a custom spec, which I did not
inspect (I did not touch the live realm).

### F10 — P3 — does not block
**One spec revision is applied to a whole lineage, so a mixed-revision ancestor is named from
the wrong revision.**
`crates/kontor-daemon/src/applications.rs:18829` and `:18934`

`ensure_container` reads a single spec from the **leaf** node's pinned `(spec_id, version)`
(`:18829`) and then renders every ancestor level with it (`:18934`), as well as resolving each
level's `projection_capabilities` from it. A lineage can span a project-level node (pinned via
`project_topology_defaults`) and epic-level nodes (pinned via
`mini_project_topology_snapshots`), so the revisions can differ if a project default is
republished without upgrading an existing epic.

Pre-existing shape — the same shared `spec` already fed `projection_capabilities` before
`a18221b` — and inherited rather than introduced by the new renderer. Noted because
judgement 1 turns on "the pinned specification", and here an ancestor is named from a
revision that is not its own.

### F11 — P3 — does not block
**`MUTATION.md`'s final gate counts under-report the tree they claim to attest.**
`docs/evidence/KON-OP-19/MUTATION.md` ("Full gates" and "Live sparse-seat repair
verification")

Both blocks state "kontor-runtime-paseo contract: 135 passed"; at `df64004` the suite has
**138**. The doc's 2026-08-21 block reads as attesting the final state but its figures were
captured mid-series, before #69 and #70 added tests. The claim direction is safe (more tests
pass than claimed, none fail), but the numbers should be refreshed if this document is cited
as a release gate.

---

## Judgement

`a18221b` replaces a genuinely worse regime — hardcoded per-seat `format!` strings and a
container fallback that appended the internal node id — with a closed four-token vocabulary
rendered from the node's own pinned revision, failing closed on every missing token with no
derivation anywhere. `1defdb2` narrows the whole-epic repair to legitimate absence without
weakening a single drift check, and the `preview_hash` covering `capability` and
`observed_title` is what makes its tolerance safe rather than merely convenient. `4ec7ab1` is
a correct distinction whose blast radius I verified to be exactly one call site. `df64004`
preserves identity and route as it claims.

Every `MUTATION.md` claim I sampled held up against the code. The four judgement questions all
resolve favourably, and the one concern I was prepared to escalate to P1 — the fail-closed
refusal for an epic with no durable execution scope — withdrew on finding it deliberate and
documented in-code. Conversely I did not soften F1, F3 or F4 because the series was merged
unreviewed.

No finding produces a wrong name, a lost identity, or a wrong model route. F1 is the one an
operator could actually be bitten by, and it is a one-line fix worth making before the next
hosted-seat repair runs in anger.

**VERDICT: PASS** — with F1 recommended as a prompt follow-up, and F3/F4 as cheap hardening
of the configurability this ticket introduced.

*This review records no Kontor gate. The LSA records it citing this verdict.*
