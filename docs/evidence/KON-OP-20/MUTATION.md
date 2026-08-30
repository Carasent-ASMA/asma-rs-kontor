# KON-OP-20 — permission-posture mutation proof

Date: 2026-08-30 (revised after inspector review)
Task: `01a02a7f-8e47-7682-be52-1b9f2a632ac4` (permission posture). **Not ASMA-7968.**
Branch: `feat/KON-OP-20-permission-posture-at-spawn`, baseline `origin/master` `e814661`.
Sweep run at `53dba77`.

## Correction to the previous revision of this file

The earlier revision claimed **"9 of 9 mutants killed"**. That overstated what the
floor was defended against, and the inspector was right to say so. None of those
nine mutants changed the floor's *membership*; they changed its *value*
(`deny` → `ask`), which the self-referential loops did catch. A deterministic
sweep confirmed that removing four of the five floor patterns left the suite
**green**. That gap is now closed by literal membership assertions, and the sweep
below includes the membership mutants that exposed it.

The earlier revision also left the OpenCode evaluator as an open assumption. It is
now verified, and one piece of the reasoning is corrected: **the ordering is not
insertion order.**

## The evaluation mechanism, verified at the exact installed version

Extracted from the Bun-compiled `/opt/homebrew/Cellar/opencode/1.18.15/bin/opencode`,
which embeds its JS verbatim:

```js
// evaluate — last match wins, unmatched tools default to ask
function c(j,J,...K){
  return K.flat().findLast((z)=>g.match(j,z.permission)&&g.match(J,z.pattern))
      ?? {action:"ask",permission:j,pattern:"*"}
}
// fromConfig — walks Object.entries for the outer map and each nested one
function RA(j){let J=[];for(let[K,z]of Object.entries(j)){ ... }}
```

Four facts, each checked here rather than assumed:

1. `fromConfig` iterates `Object.entries` outer **and** nested, so the rule list
   preserves the key order of the file as serialized.
2. `evaluate` resolves with **`.findLast`** — the *last* matching rule wins.
3. An unmatched tool defaults to `{action:"ask"}`.
4. The order is **lexicographic**, not insertion order. This workspace pins
   `serde_json = "=1.0.151"`, and its `Cargo.lock` entry depends only on
   `itoa, memchr, serde, serde_core, zmij` — no `indexmap`, so `preserve_order` is
   off and `serde_json::Map` is a `BTreeMap`.

`"*"` is a prefix of every floor pattern, so it sorts **before** all of them and
the specific denials are evaluated after the catch-all and win. Stating this as
insertion order would be wrong, and would be exactly the reasoning that later
justifies appending a non-floor allowance — which is the F5 defect.

## The sweep

Each mutation was applied to the corrected source, **verified to have landed**
(`git diff --quiet` on the mutated file), the named tests were run against that
broken build, and the mutation was reverted. A mutant producing a *compile* error
rather than a test failure counts as survived. Floor mutants are edited inside the
constant's own block, so they cannot accidentally hit a test literal instead.

Run with an isolated `CARGO_TARGET_DIR`; the tree was verified clean afterwards.

### F1 — the floor's membership (the gap the inspector found)

| # | Mutation | Result |
| --- | --- | --- |
| F1-remove ×5 | delete each of the five patterns from `DESTRUCTIVE_BASH_DENIES` | **all 5 killed** |
| F1-drift ×5 | respell each pattern (`*rm -rf *`→`*rm -rf*`, `*git clean -*`→`*git clean*`, `*git rm --cached*`→`*git rm --cache*`, `*submodule update*`→`*submodule updates*`, `*submodule deinit*`→`*submodule de-init*`) | **all 5 killed** |

Killed by `the_floor_is_exactly_the_five_published_patterns` and
`every_published_pattern_is_denied_by_literal_name`, which name all five patterns
as literals and so cannot be silenced by editing the constant.

### F5 — the bounded override must be an exact floor key

| # | Mutation | Result |
| --- | --- | --- |
| F5-lax | `parse` accepts any non-blank pattern (the reviewed defect) | **killed** |
| F5-lax | the same defect, checked at the fleet-composition boundary | **killed** |
| F5-case | `parse` matches a floor pattern case-insensitively | **killed** |
| F5-prefix | `parse` accepts a prefix of a floor pattern | **killed** |

### The original gates, re-checked on the corrected source

| # | Mutation | Result |
| --- | --- | --- |
| M1 | `READABLE_SCHEMAS` drops generation 4 | killed |
| M2 | resolution never consults the plane default | killed |
| M3 | the plane default overrules the role slot | killed |
| M4 | the floor renders `ask` instead of `deny` | killed |
| M6 | the `ask` posture also spells `read: ask` | killed |
| M7 | a task exception leaks into the launch mode | killed |
| M8 | Cursor's `ask` renders as `agent` | killed |

**21 of 21 mutants killed.**

## Superseded scope note (2026-08-30, fail-closed disposition)

OpenCode delivery is now refused before any native call, so the mutants below
that exercise the OpenCode composition and readback describe the **re-enabled**
path rather than what runs today. They are retained because the code they test is
retained. The gate itself carries its own mutation evidence: removing the refusal
in `seat_posture` turns both `opencode_delivery_is_refused_until_it_can_be_proved`
and the launch-boundary test
`an_opencode_delivery_launch_is_refused_before_any_native_call` red.

## What this still does not prove

- **Readback cannot see the posture on OpenCode** (inspector F4). Both
  `autonomous` and `ask` spell `build`, and the permission block — the only
  carrier of the difference — is never re-read after launch. A
  `.opencode/opencode.json` deleted or edited post-spawn passes verification.
  This is an inherent limit of verifying through Paseo, not something this change
  introduced, and it is retained as a reviewed observation rather than fixed here.
- **No live autonomous seat was run on a clean host.** The evaluator semantics are
  verified by extraction and the rendering by test; an end-to-end seat that
  provably never blocks has not been observed.
- **Cursor's delivery postures are latent** (inspector F3). Cursor is now in the
  mode tables but the governed model catalog exposes no cursor provider, so no
  route can select it. The containment claim behind an advisory cursor seat rests
  on the catalog, not on this layer.

---

# CURRENT DISPOSITION — 2026-08-30 (supersedes every status above)

**OpenCode delivery is fail-closed.** `preflight::attest_spawn_environment`
refuses every OpenCode delivery launch, before the capability read, the provider
diagnostic, the owned root and the preflight — so nothing native is read or
created.

**Why.** Two inputs to an OpenCode seat's permission are decided outside
everything Kontor pins: the auth-backed **active-org remote configuration**,
which follows whichever credentials the spawned process resolves, and the
**system managed layer**, read at process start. Both sort *after* the owned
configuration, so both can state a permission the preflight never saw. Whose
credentials the spawned process uses depends on the inherited `HOME` /
`XDG_DATA_HOME`, and `paseo provider diagnostic` reports neither — it prints
`Daemon PATH`, a `Daemon shell`, and an *unexpanded*
`~/.local/share/opencode/auth.json`. No Paseo call reports the configuration a
created agent actually resolved.

**Correcting an earlier claim in this file.** The owned root does **not** remove
the active-org or managed layers. It displaces the user global and every project
layer; the later two are only *observed* at preflight time by full-object
comparison, and observation before creation is not proof about the process that
is created afterwards.

**What is built and stays under test**, waiting on that dependency: the
`autonomous|ask|plan` vocabulary, schema v5 and resolution order, the shared
renderer and destructive floor, the exact-floor allowance rule, the Cursor
correction, the owned per-seat configuration root with no-follow containment, the
typed six-key `agent run --env` surface with redaction, the daemon version pin,
the posture digest for lost-acknowledgement recovery, and the installed-binary
preflight.

**Lifts when** Paseo can attest either the spawned process's environment or the
configuration a created agent resolved.
