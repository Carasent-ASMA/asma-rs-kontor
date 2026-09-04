# KON-OP-20 — permission posture at spawn (inspector review notes)

- **Task:** `01a02a7f-8e47-7682-be52-1b9f2a632ac4`, `ai_short_name: permission posture`.
- **Jira:** none — Kontor-native. **This is not ASMA-7968.** `CLOSEOUT.md` in this
  directory belongs to the topology-selection work and was not read as evidence
  for this task.
- **Role:** inspector (AUD), seat binding `01a0306e-8fda-75d2-9f47-5ce1add8016b`
  on topology node `01a0306e-6de7-7c90-aaa6-4995ea6dc074`.
- **Reviewed:** `feat/KON-OP-20-permission-posture-at-spawn` at `4c93739`
  (`44c247c`, `2a6fd8e`, `eeed9ad`, `4c93739`), baseline `origin/master` `e814661`.
- **Plan:** `_docs/ai-orchestration/plans/2026-08-30-15-05-plan-kontor-op20-permission-posture-reconciliation.md`.
- **Verdict:** **BLOCKED on two findings (F1, F5).** Everything else inspected
  passes.

## Verdict in one line

The design, the v4→v5 migration, the resolution order, the kill switch, the
launch/readback mode agreement and the OpenCode discovery contract are all
correct and independently reproduced, and the exact-version evaluator semantics
confirm the destructive floor beats the catch-all. Two holes remain in that
floor: its *membership* is asserted by tests that use the floor as their own
oracle (F1), and a bounded override that is not an exact floor key silently
defeats it under last-match evaluation (F5).

## What was checked independently

Focused checks, run in this worktree at `4c93739`:

| Check | Result |
| --- | --- |
| `cargo test -p kontor-runtime-paseo --lib posture::` | 12 passed |
| `cargo test -p kontor-runtime-paseo --lib seat_mcp::` | 15 passed |
| `cargo test -p kontor-daemon --lib runtimes::` | 16 passed |
| `cargo test -p kontor-daemon --lib seat_autonomy` | 1 passed |
| `cargo clippy -p kontor-runtime-paseo -p kontor-daemon --all-targets` | clean, exit 0 |

### The OpenCode contract, re-verified against the installed 1.18.15

Not taken from the builder's evidence — re-run here with `opencode debug config`
in throwaway git repositories:

| Probe | Result |
| --- | --- |
| `.opencode/opencode.json` alone | discovered; **deep-merged** with the global config, project winning on conflicting keys (`read: ask` beat global `read: allow`) |
| root `opencode.json` **and** `.opencode/opencode.json`, same keys | both read; **`.opencode/` wins** (`read: ask` over root's `deny`); non-conflicting keys from both survive |
| isolated `HOME`/`XDG_CONFIG_HOME`, no global config | the composed block resolves exactly as written; unlisted tools fall to opencode's own built-in defaults |

This confirms the load-bearing claim in `seat_mcp.rs` — `.opencode/opencode.json`
is discovered and outranks the repository's committed root `opencode.json`. The
choice of `.opencode/` over the plan's D3 `<cwd>/opencode.json` is correct for
this repository: the superproject tracks a root `opencode.json`, and
`info/exclude` cannot hide a tracked file. `OPENCODE_EXCLUDED` naming the exact
file rather than the `.opencode/` directory is also correct — the superproject
tracks eight files under `.opencode/` (`command/*.md`, `skills`).

### The evaluator, verified at the exact version from the installed binary

`MUTATION.md` records as an open assumption that "a specific `deny` pattern beats
`"*": "allow"` inside one `bash` map". That assumption is now **closed and
confirmed**, verified here against the installed OpenCode 1.18.15 itself — the
Bun-compiled `/opt/homebrew/Cellar/opencode/1.18.15/bin/opencode` embeds its JS,
and the Permission module extracts verbatim:

```js
// evaluate — exported as `evaluate: () => c`
function c(j,J,...K){
  return K.flat().findLast((z)=> g.match(j,z.permission) && g.match(J,z.pattern))
      ?? {action:"ask", permission:j, pattern:"*"};
}
// fromConfig — exported as `fromConfig: () => RA`
function RA(j){ let J=[];
  for (let [K,z] of Object.entries(j)) {
    if (typeof z === "string") { J.push({permission:K, action:z, pattern:"*"}); continue; }
    J.push(...Object.entries(z).map(([B,X]) => ({permission:K, pattern:LA(B), action:X})));
  } return J; }
```

Confirmed: `fromConfig` iterates `Object.entries` outer and nested, preserving
file key order (`LA` only expands `~`/`$HOME`); `evaluate` resolves by
**`.findLast`**, so the *last* matching rule wins; and an unmatched tool defaults
to `ask`.

**One mechanism correction.** The ordering does *not* come from insertion order.
This workspace pins `serde_json = "=1.0.151"` with no `indexmap` in its
dependency tree, so `preserve_order` is off and `serde_json::Map` is a
`BTreeMap` — keys serialize **lexicographically**. Rendering the real block
confirms what reaches disk:

```json
{ "*": "allow", "*git clean -*": "deny", "*git rm --cached*": "deny",
  "*rm -rf *": "deny", "*submodule deinit*": "deny", "*submodule update*": "deny" }
```

The conclusion survives the correction, and for a stronger reason than insertion
order: `*` is a prefix of every floor pattern, so it sorts first under `BTreeMap`
*and* would be inserted first under `IndexMap`. The catch-all therefore precedes
every deny under either build configuration, and `findLast` gives the floor the
win. An exact-key allowance replaces that one deny in place, changing its action
and not its position — also confirmed by rendering it. **But this holds only for
allowances that are exact floor keys; see F5.**

### Other dimensions

- **Launch/readback drift** — none. `seat_posture` derives `mode` by calling
  `paseo_mode`, and `verify_agent_route_with_mode` (adapter.rs:1923) calls the
  same `paseo_mode`. Allowances provably cannot reach `mode`
  (`allowances_never_move_the_mode_or_the_feature`), so readback comparing
  `paseo_mode` is equivalent to comparing `posture.mode`. See F4 for the limit.
- **Provider mode spelling** — matches the Paseo 0.6.1 surface. `paseo agent run
  --help` exposes `--mode` and no `--auto-accept`/`--feature`, so OQ-OP20-2's
  "derived but unwired" is accurate as written.
- **v4→v5 compatibility** — `READABLE_SCHEMAS = [4, 5]`, additive fields with
  `#[serde(default)]`, v3 still refused. The realm's live
  `<state-root>/runtimes.json` is **schema_version 4** and reads under this build,
  resolving to `ask`. No migration is required.
- **Resolution order** — `slot ?? plane_default ?? Supervised`, tested both ways
  including the case where the plane default is the *wider* one.
- **Permission-block safety** — `deny` beats `ask` for the floor; `Supervised`
  names only `bash` so the change cannot make a supervised seat stall more than
  it did.
- **Bounded overrides** — allow-only, wildcard/blank refused at both the type
  boundary (`PermissionAllowance::parse`) and the config boundary
  (`compose_paseo`), and cannot reach `mode` or `auto_accept`.
- **Kill switch** — `KONTOR_SEAT_MCP=off` resolved once in the daemon
  (`runtimes.rs:470`), handed to the adapter as `None`; withdraws the permission
  block as well as the MCP files. Only the exact spelling `off` disables.
- **Secret/path leakage** — none found. No logging in `posture.rs` or
  `seat_mcp.rs`; the composed files carry a tier *name* (`--credential-tier
  operator`) and the realm state-root path, never a credential; `PaseoSetting`
  has no `Debug` derive; the permission block contains no paths at all.

## Findings

### F1 — BLOCKING · the destructive floor's membership is not pinned by any test

`DESTRUCTIVE_BASH_DENIES` is the safety core of this change, and
`CONFIGURATION.md` publishes its five patterns as a contract. Every floor
assertion in `posture.rs` iterates the constant as its own oracle:

```rust
for pattern in DESTRUCTIVE_BASH_DENIES {
    assert_eq!(permission["bash"][*pattern], "deny", ...);
}
```

Delete a pattern from the constant and the loop simply stops checking it. A
deterministic sweep — each mutant verified applied before running
`cargo test -p kontor-runtime-paseo --lib` — gives:

| Pattern removed from the floor | Suite |
| --- | --- |
| `*submodule update*` | **green — survived** |
| `*submodule deinit*` | **green — survived** |
| `*git rm --cached*` | **green — survived** |
| `*git clean -*` | **green — survived** |
| `*rm -rf *` | red (`seat_mcp::tests::a_task_scoped_exception_reaches_the_composed_block`) |

Only `*rm -rf *` is caught, and only incidentally: `seat_mcp.rs:764` happens to
assert it as a string literal. A literal-occurrence sweep confirms the mechanism —
`*submodule update*` and `*submodule deinit*` appear nowhere but the constant,
and `*git rm --cached*` / `*git clean -*` appear elsewhere only as *allowance*
arguments, asserted `"allow"`, never as floor members.

**Failure scenario.** A refactor, a bad merge, or a well-meant "simplify the
floor" drops `*git rm --cached*`. Every test stays green, clippy stays clean, and
autonomous opencode seats silently gain the ability to strip gitlinks — the exact
CAT-09 hazard the `PermissionAllowance` machinery was built to grant *narrowly*
and on purpose. The floor stops being a floor and nothing reports it.

**Fix.** One assertion pinning the exact set, e.g.

```rust
#[test]
fn the_floor_is_exactly_these_patterns() {
    assert_eq!(
        DESTRUCTIVE_BASH_DENIES,
        &["*submodule update*", "*submodule deinit*",
          "*git rm --cached*", "*git clean -*", "*rm -rf *"],
        "the floor's membership is a published contract (CONFIGURATION.md)"
    );
}
```

This also closes the gap in `MUTATION.md` (added in `67755bd`, after the commit
range under review): its M4 mutates the floor's *value* (`deny` → `ask`), which
the self-referential loops do catch, but no mutant changes the floor's
*membership*, so "9 of 9 mutants killed" overstates what this gate is defended
against.

### F5 — BLOCKING · a bounded override that is not an exact floor key defeats the floor

This follows directly from the exact-version semantics verified above, and is
invisible without them: `findLast` + lexicographic key order means **a broader
allowance sorts after the deny it overlaps and therefore beats it**.

`PermissionAllowance::parse` rejects only empty, blank, and all-`*` strings, so
`*git*` is accepted as a "named" pattern. Rendering it:

```json
{ "*": "allow", "*git clean -*": "deny", "*git rm --cached*": "deny",
  "*git*": "allow", "*rm -rf *": "deny", ... }
```

`*git*` sorts *after* both git denies — at index 4 of the pattern, `' '` (0x20)
precedes `'*'` (0x2A). Evaluating `git clean -fdx` matches `*` (allow),
`*git clean -*` (deny) and `*git*` (allow); `findLast` returns the last one:
**allow**. The same shape defeats every floor family — `*rm*` sorts after
`*rm -rf *`, `*submodule*` after both submodule denies.

**Failure scenario.** A ticket declares `"permission_overrides": ["*git*"]`
because it does a lot of git work. `compose_paseo` accepts it, the type accepts
it, every test passes, and that ticket's autonomous seats can now run
`git clean -fdx` and `git rm --cached` — the entire git half of the floor is
gone, from one config line that never spells `*`.

This contradicts the stated invariant of the feature. `posture.rs` argues an
override "that could say `*` would be `allow-all` spelled as an exception, and
the floor would hold only until somebody needed it not to"; the plan's D5 says
"Never seat-side, never `allow-all`". The type exists precisely to make the
bounded override safe by construction, and it does not achieve that. The test
`an_allowance_relaxes_exactly_one_named_pattern` only ever exercises an exact
floor key (`*git rm --cached*`), which is the one case that *is* safe.

**Fix.** Require an allowance to name a pattern that is already in
`DESTRUCTIVE_BASH_DENIES` — which is exactly the real use case, since an
allowance is only meaningful against a rule the floor otherwise denies:

```rust
pub fn parse(pattern: &str) -> Option<Self> {
    let pattern = pattern.trim();
    DESTRUCTIVE_BASH_DENIES
        .contains(&pattern)
        .then(|| Self(pattern.to_owned()))
}
```

That makes the allowance set a subset of the floor by construction, so an
override can only ever flip one existing deny and can never introduce a new,
later-sorting rule. It also subsumes the wildcard and blank refusals rather than
enumerating them. If a non-floor allowance is genuinely wanted later, it must be
inserted such that the floor still evaluates last, which lexicographic
serialization cannot guarantee.

### F2 — non-blocking · `ask` posture is still ambient-dependent for non-bash tools

Because opencode deep-merges rather than replaces, and the `Supervised` block
deliberately names only `bash`, every other tool resolves from the repository's
root `opencode.json` and then the machine-global config. Reproduced on this
operator machine, which still carries the 2026-08-22 stopgap at
`~/.config/opencode/opencode.json`, composing exactly the block the code writes
for `Supervised`:

```
read               = "allow"
edit               = "allow"
external_directory = {"*": "allow"}
bash[*]            = "ask"
```

An `ask` seat edits files without asking. The same block under an isolated
`HOME`/`XDG_CONFIG_HOME` leaves those keys unset, and the evaluator extracted
above defaults an unmatched tool to `{action:"ask"}` — so on a clean host the
`Supervised` block does mean `ask` for every tool, and **the code is correct as
designed**. The leak is entirely the ambient stopgap, and this is a strict
improvement on the prior state (where `bash` was ambient too). It is recorded
because neither `CONFIGURATION.md` nor the builder evidence states the
operational precondition: until the machine-local stopgap is removed from
operator hosts, `ask` means what it documents only for `bash`. The plan puts the
stopgap out of scope ("document only"); this is the sentence that documenting it
is missing.

### F3 — non-blocking, latent · `cursor` gained delivery postures the same file calls uncontained

`44c247c` adds `cursor` to `BUILT_INS`, `paseo_mode` (`agent`) and
`permission_mode` (`ask`), and `paseo_mode`'s `Advisory` arm (`plan`). Before it,
a cursor delivery seat was refused. Thirty lines away, `consultation_permission_mode`
refuses cursor on an attested finding that "its ACP runtime permits shell writes
in both modes (and file writes in `ask`)". `CONFIGURATION.md` defines `plan` as
"Read and propose, never act", and `posture.rs` deliberately gives cursor no
permission block — so an advisory cursor seat would carry a containment claim
with no mechanism behind it, contradicted in-repo.

**Latent, not live:** the governed model catalog exposes no `cursor` provider
(`codex`, `codex-work`, `codex-personal`, `claude`, `claude-work`,
`claude-personal`, `opencode`), so no route can select it today. The guard that
stops it is the catalog, not the posture layer. Worth either refusing cursor's
`Advisory` arm outright or recording the caveat before cursor is ever catalogued.

### F4 — non-blocking, observation · readback cannot see the posture on opencode

`verify_agent_route_with_mode` compares `current_mode_id` only. For opencode both
`autonomous` and `ask` spell `build`, so readback cannot distinguish the two
postures, and the permission block — the sole carrier of the difference — is never
re-read after launch. A `.opencode/opencode.json` deleted or edited post-spawn
passes verification. This is an inherent limit of verifying through Paseo rather
than a defect introduced here; `MUTATION.md` notes the evaluator is unexercised
but not this readback blind spot.

## Open questions

- **OQ-OP20-5 (new, ledger)** — *the governed inspector turn could not be settled
  from this seat.* `kontor_turn_settle` requires an `agent_run_id`, and no
  inspector agent run for this task could be evidenced: `paseo ls` lists no OP-20
  agent, the control-plane event stream (cursors 1400–1568) carries none, this
  worktree has no composed `.mcp.json`, and the topology's OP-20 node reports
  `observed_at 2026-08-29` with the architect run `01a0306e-6de5-7bb2-a8b6-18bd37dfb036`
  at `freshness: stale`. The topology seat binding
  (`01a0306e-8fda-75d2-9f47-5ce1add8016b`) is a Core Team seat binding, a
  different namespace from a run binding, and does not yield a run id. **No
  settlement was attempted rather than guessing an id** — recording a turn
  receipt against an invented or borrowed run would put a false statement in the
  durable record. This is the bounded direct-prompt fallback the plan's §5
  anticipates under `operational-gap-asma-7869-stale-runtime-binding-unsettleable-20260830`;
  it reconciles once a governed inspector run exists.
- **OQ-OP20-6 (new)** — should `permission_overrides` be readable back at all?
  F4 means a task-scoped exception is invisible to verification. Per-task
  overrides are the plan's OQ-OP20-3 choice; this is the verification half of
  that decision, not a re-litigation of it.

## Scope notes

- `67755bd` (`MUTATION.md`) landed on the branch *during* this review, after the
  assigned range. It is outside the four commits under review. Two of its
  statements are addressed here: its floor claim is contradicted by the
  membership sweep (F1), and its one declared open assumption about the OpenCode
  evaluator is closed and confirmed above — with the ordering mechanism corrected
  from insertion order to `BTreeMap` lexicographic serialization. `MUTATION.md`
  should be amended to state the corrected mechanism, since "the map serializes
  in insertion order" is the reasoning that would later justify appending a
  non-floor allowance and would be wrong.
- No deploy, push, merge, session or workspace was created. The nested working
  tree was left clean; every mutant was reverted and verified with
  `git diff --quiet`.
