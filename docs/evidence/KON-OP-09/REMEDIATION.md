# KON-OP-09 QA remediation

Date: 2026-08-18
Task: `KON-OP-09` / Jira `ASMA-7878`
Scope: the two blocking acceptance gaps recorded in `QA.md`
Seat: builder (`code` work profile, implementation phase)

Answers the QA gate rejection (receipt `01a013a6-6486-7ed0-9c64-5cdff3cae04c`)
at `efd4670`. Both findings are acceptance proofs from `ARCHITECTURE.md`, not
executable-check failures: every gate was already green when QA rejected.

Changed files: `apps/console/src/views/ProjectView.tsx` and its test. No
contract, no `.rs` file, no server route, no other panel.

## Finding 1 — idempotency replay

`ARCHITECTURE.md` mutation rule 2 and the required proof *"replay uses the
original idempotency key"*. Every mutation minted `crypto.randomUUID()` inline
at activation, so retrying an uncertain request presented a key the daemon
could not recognize as a replay — and for `quick-sessions:ensure`, which carries
no `expected_revision`, that is a second durable workspace.

One hook now owns the key:

```ts
function useIntentKey(): { keyFor: (intent: unknown) => string; release: () => void }
```

The key is derived from a fingerprint of the request itself, so it survives a
retry of the unchanged intent, is replaced when the intent changes, and is
released once the realm confirms a receipt — after which the next activation is
a new intent rather than a replay.

All seven activation sites route through it: Core Team apply, Quick Session
ensure, promotion apply, Advisor invoke, Committee invoke, completion advance
and completion remediate. `crypto.randomUUID()` now appears exactly once in the
file, inside the hook. The consultation forms hold their own key rather than
receiving one minted by the parent, so the `invoke` prop takes a `commandId`.

Each panel holds one key per distinct command, so switching between advance and
remediate — or between the ensure and the promotion — cannot make one command
inherit another's key.

## Finding 2 — a failed sibling read erasing valid evidence

`ARCHITECTURE.md` independent-projection loading and the proof *"a failed panel
does not erase successful sibling projections"*. Two panels were rendered only
when both their own read and a catalog sibling succeeded.

- Core Team rendered on `coreTeam.value && roles.value`. It now renders on its
  own read. The role catalog feeds the editor only: with the catalog absent the
  roster stays visible, the editor degrades (no selectable role, Add disabled),
  and the server's refusal message is shown in place of the generic
  no-catalog-revision banner.
- Completion rendered on `completion.value && completionProfiles.value`. The
  published profile catalog and the epic's completion state are now two
  independent children of the section, each with its own ready-or-refused
  result, so either can fail without hiding the other.

No panel merges reads into a synthetic revision, and each still preserves the
server's own error text.

## Regression coverage and its proof

Five tests added to `ProjectView.test.tsx` (285 → 290).

| Test | Pins |
| --- | --- |
| `replays one uncertain intent under its original idempotency key` | a failed apply, retried unchanged, reaches the client twice with one key and one body |
| `mints a new idempotency key once the intent itself changes` | editing the composition between attempts produces a different key |
| `keeps the Core Team roster when the role catalog read fails` | roster rendered, refusal visible, editor disabled |
| `keeps epic completion when the profile catalog read fails` | phase and Advance still present |
| `keeps the profile catalog when the epic completion read fails` | the reverse direction |

The tests were verified against the defects rather than assumed to cover them.
Reintroducing per-activation minting (`if (true)` in `keyFor`) turns
`replays one uncertain intent…` red and the rest green. Restoring the three
original coupled guards turns all three independence tests red. Both mutations
were reverted and the tree left clean.

## Gate results

Run on this source tree; the only later change is this file, which no check
reads.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace` | pass — 1394 passed, 0 failed |
| `tsc --noEmit` | pass |
| `vitest run` | pass — 16 files, 290 tests |
| `playwright test` | pass — 4 flows, desktop and phone |
| `openapi-typescript` drift | none |

The committed desktop and phone screenshots are unchanged: neither fix alters
what the console renders when every read succeeds.

## Still open

Non-blocking, carried from `REVIEW.md` items 5 and 6 and unchanged here:

- no client method for `seats:materialize`, `advisor-runs:settle`,
  `committee-runs/findings:record` or `committee-runs:settle`, so runs can be
  invoked and never settled from the console; and
- the `table-scroll` topology region has no `tabindex`, `role` or accessible
  name, so a keyboard-only operator cannot scroll it at phone width.

`apps/console/test-results/` and the retained `docs/evidence/KON-MVP-18/run-*`
bundles are untracked build artifacts in this worktree, not part of this change.
