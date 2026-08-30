# KON-OP-20 — deterministic agent permission posture at spawn (builder evidence)

- **Task:** `01a02a7f-8e47-7682-be52-1b9f2a632ac4`, `ai_short_name: permission posture`.
- **Jira:** none — Kontor-native. **This is not ASMA-7968** (see `README.md` in this directory).
- **Module:** `asma-rs-kontor`, branch `feat/KON-OP-20-permission-posture-at-spawn`, baseline `origin/master` = `e814661`.
- **Plan:** `_docs/ai-orchestration/plans/2026-08-30-15-05-plan-kontor-op20-permission-posture-reconciliation.md` (supersedes the 2026-08-23 plan).
- **Design authority:** `2026-08-22-13-00-design-agent-permission-posture-at-spawn.md` (LSA ASMA-8001 → TPM ASMA-7869 handoff).

## What was wrong

An opencode seat's session mode is not its permission posture. Kontor spawned
seats with `--mode build` and nothing else, so posture fell through to whatever
the machine's harness config carried. On 2026-08-22 twelve of fifteen delivery
seats for the ASMA-8001 catalog epic blocked mid-turn on permission prompts no
human was watching; Kontor recorded them as running and `scheduler-plan` refused
re-admission. Two wedged prompts held an eleven-ticket epic for ~2.5h.

The only thing fixing it at the time was a machine-local edit to
`~/.config/opencode/opencode.json`, which does not travel — a fresh host
reproduces the outage.

## OQ-OP20-1 — answered empirically, not assumed

The plan required the builder to verify on a clean seat that opencode honors a
project-level config before relying on it. Verified against opencode 1.18.15 with
`opencode debug config` (the resolved-configuration readback):

| # | Probe | Result |
| --- | --- | --- |
| A | project `opencode.json` in a non-git dir | honored, deep-merged with global |
| B | the same inside a git repo | honored |
| C | nested dir, config only in an ancestor | honored — opencode walks up |
| D | isolated `HOME`/XDG, **no global config at all** | project config alone resolves exactly as written — the clean-machine case |
| F | root `opencode.json` **and** `.opencode/opencode.json` | both read and merged |
| I | same key in both | `.opencode/` **wins** |
| G | `OPENCODE_CONFIG=<file>` | read, but project config merges *over* it |
| H | `OPENCODE_CONFIG_CONTENT=<json>` | highest precedence of all |

**Answer: yes.** A project-level block is honored, works with no global config
present, and `.opencode/opencode.json` takes precedence over the repository's
root `opencode.json`.

## OQ-OP20-4 (new) — the block is written to `.opencode/opencode.json`

The plan's D3 says `<cwd>/opencode.json`. That target is wrong for the
repositories these seats actually run in, and the deviation is recorded here for
the architect rather than taken silently:

- A seat's cwd is a worktree of the `asma-modules` superproject, which **tracks**
  a root `opencode.json` (model, instructions, mcp). Verified:
  `git ls-files --error-unmatch opencode.json` succeeds.
- Git applies no ignore rule to a tracked file, so D3's stated mitigation —
  `info/exclude` — cannot keep a merge into it out of the seat's own diff.
  Verified: `git check-ignore -v opencode.json` matches nothing.
- Merging there would dirty every seat's worktree and leave Kontor's safety floor
  one `git add` away from being committed as project configuration.

`.opencode/opencode.json` preserves D3's intent exactly — worktree-local,
spawn-time, idempotent, `info/exclude`-hidden, kill-switch respected — while
being untracked (verified) and higher precedence than the committed root file
(probe I). The operator's committed configuration survives untouched.

## OQ-OP20-2 — `auto_accept` remains unwired, deliberately

Re-verified at **Paseo 0.6.1**: `paseo agent run --help` exposes `--mode` and no
`--feature`/`--auto-accept`; `agent update` exposes none either. The live provider
catalogue confirms `auto_accept` exists as a per-agent feature for `opencode` and
`cursor` (and that `claude` and `codex` expose no features at all). Kontor drives
the CLI, not the MCP surface where the feature is settable, so the renderer
derives the intended value and nothing consumes it yet. The permission block is
the guaranteed spawn-time mechanism; the value is derived in the same place as
everything else so a future spawn surface needs no second decision.

## Provider vocabulary — verified, not guessed

Read live from the Paseo provider catalogue rather than assumed, because a wrong
mode spelling is refused at spawn and strands the seat (Paseo 0.4.0 rejecting
`default` for Codex left every replacement verifier permanently queued):

- `claude`: `plan`, `default`, `acceptEdits`, `auto`, `bypassPermissions`
- `codex`: `auto`, `auto-review`, `full-access` — **no read-only mode**, so an
  advisory Codex seat is refused rather than run under a writing one
- `cursor`: `agent`, `plan`, `ask`
- `opencode`: `build`, `plan` only — `build` is described by the provider as
  "Executes tools based on configured permissions", which is the direct
  confirmation that posture belongs in the block, not the mode

## What was built

| Plan step | Where |
| --- | --- |
| Abstract `autonomous\|ask\|plan` vocabulary mapped to `SeatAutonomy` | `kontor-daemon/src/runtimes.rs` — `PermissionPosture` + `From` both ways |
| Versioned migration, back-compatible default | `RUNTIMES_SCHEMA = 5`, `READABLE_SCHEMAS = [4, 5]`; absence resolves to `ask` |
| One shared renderer for launch and readback | `kontor-runtime-paseo/src/posture.rs` — `seat_posture` |
| Native translation, Claude/Codex/Cursor/OpenCode | `client.rs` mode tables (Cursor added) + `posture.rs` |
| Guaranteed OpenCode permission block | `seat_mcp.rs` — `<cwd>/.opencode/opencode.json` |
| Bounded per-task override | `PaseoTaskSetting.permission_overrides` → `PermissionAllowance` |
| Destructive patterns deny, not ask | `DESTRUCTIVE_BASH_DENIES`, applied under every writing posture |
| Consultation stays read-only | `SeatPosture::read_only()` at the consultation launch |

The resolution order is slot → plane default → `ask`, so a template that already
declared a seat's autonomy is never overruled by a plane-wide default.

## Out of scope, untouched

- `launch_hosted_seat_inner` (ECP leadership seats) stays `Supervised`.
- The machine-local `~/.config/opencode/opencode.json` stopgap is documented, not
  redone, and is never written by this composition.
- The superproject gitlink is not advanced here.
