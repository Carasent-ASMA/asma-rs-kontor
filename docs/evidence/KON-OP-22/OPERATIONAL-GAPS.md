# KON-OP-22 operational gaps

Date: 2026-09-03

This record is delivery evidence for the temporary Kontor-first recovery rule.
It records only bounded fallbacks; neither fallback changes control-plane
authority.

## GAP-1 — branch preparation could not use the ASMA wrapper

- Owning project: `01a0064a-e056-7603-9968-ef64fdaacb75`
- Owning epic: `01a0074f-6719-7570-adf7-95ee3ec69875` (`ASMA-7869`)
- Owning task: `01a030c2-1c65-79b1-ac84-7bc8baae8977` (`KON-OP-22`)
- Intended operation: create/resume the task branch through `asma worktree` and
  its confirmed Jira identity.
- Failure class: configuration/legacy identity gap. The wrapper refused because
  `JIRA_BASE_URL` was not set and this historical Kontor child has no confirmed
  child Jira binding.
- Bounded fallback: reuse the existing KON-OP-22 worktree and create
  `fix/KON-OP-22-current-master-reconstruction` directly from remote master
  `1a4320f7d7cf8f673bce3771afd2a1046f7c0104`.
- Effects: one local branch; no Jira mutation, no additional worktree, no change
  of backlog authority.
- Resume checkpoint: implementation and local verification in the existing
  worktree.
- Owner/status: branch fallback closed by PR #156 and merge commit `7c27f4d`.
  Worktree cleanup is the final delivery housekeeping action; untracked
  unrelated evidence is preserved outside the worktree before removal.

## GAP-2 — architect role retry was unavailable

- Team run: `01a030c7-200e-7041-af08-b9fe4f72ac5c`
- Role run: `01a030c7-200f-7602-9fca-07a85cec0da2` (`architect`)
- Intended operation: resume the existing architect role through Kontor for a
  current-master divergence verdict.
- Failure class: runtime availability; Kontor returned HTTP 503.
- Bounded fallback: no replacement topology or direct Paseo mutation was made.
  The existing tester and inspector roles were messaged through Kontor, and the
  specialized read-only repository audit continued without changing runtime
  identity.
- Effects: none on Jira or runtime topology. The unavailable role remains
  attributable in the original team run.
- Resume checkpoint: independent review after the complete local gate set.
- Owner/status: closed for delivery. The final independent review is attached
  in `REVIEW-NOTES.md` and returned `APPROVE` with no P0/P1 blocker; no
  replacement topology or direct runtime mutation was needed.

## GAP-3 — bundled epic workflow could not route the live ASMA draft

- Subject: Kontor epic `01a0539a-51c9-7301-9bd7-26c09167b23e`, confirmed Jira
  Epic `ASMA-8049`.
- Intended operation: resident reconciliation of the active epic to Jira
  `In Development (10214)` through the installed generic epic workflow.
- Failure class: specification capability gap. Generic epic workflow revision 1
  declared only one `reopen` staging status. Jira offered `DRAFT (10237)` →
  `TO BE GROOMED (10236)`, so Kontor correctly recorded durable
  `no_live_transition` conflict `01a06761-49a2-7832-a11c-2b91e491a9a4`
  instead of guessing a multi-hop route.
- Bounded fallback: none. Jira was inspected read-only. Current Epic transition
  and changelog evidence verified the exact route `New (10227)` →
  `DRAFT (10237)` → `TO BE GROOMED (10236)` → `Groomed (10233)` →
  `READY FOR DEVELOPMENT (10213)` → `In Development (10214)`.
- Effects: no direct Jira mutation and no replacement runtime topology. The
  conflict remained durable while workflow revision 2 and its deterministic
  route contract were implemented and promoted.
- Resume checkpoint completed: revision 2 installed with receipt
  `01a067cf-beda-72c2-ac30-6042125a1f89`; the resident controller confirmed all
  four remaining hops to `In Development`; the backstop added no duplicate;
  the superseded conflict was resolved by receipt
  `01a067d0-6c17-7d02-a46d-602f57b1e5f3`.
- Owner/status: closed. Live convergence and conflict resolution were read back
  with four confirmed revision-2 intents and zero open conflicts.

## GAP-4 — exact Jira recovery was split across legacy pending batches

- Owning project: `01a0064a-e056-7603-9968-ef64fdaacb75`.
- Owning epic: `01a0074f-6719-7570-adf7-95ee3ec69875` (`ASMA-7869`).
- Owning task: `01a030c2-1c65-79b1-ac84-7bc8baae8977` (`KON-OP-22`).
- Intended operation: apply preview
  `c59b9fbb3e27a3020b31b9b50710d3af19d3b21a7ad8bfebc79200fb43eb8029`
  through Kontor, linking the confirmed epic and 18 confirmed children while
  creating Jira children for `KON-OP-20`, `KON-OP-21` and `KON-OP-22`.
- Failure class: durable materialization-recovery gap. Kontor returned HTTP 503
  after recording command receipt `01a0683f-7579-7d82-90b7-00781902f8b3`.
  Read-only diagnostics proved that one legacy planned batch owns ordinals
  0–20 and another owns ordinal 21. The create-marker fence correctly refused
  a third batch, but recovery could only adopt one complete batch and therefore
  could not resume the exact union.
- Bounded fallback: read-only SQLite inspection was used only after the
  supported Kontor API repeated the same 503 across a managed daemon restart.
  No database row was edited, no direct Jira mutation was made and no Paseo
  topology was created or replaced.
- Correction: mixed link/create applies now participate in recovery. The store
  accepts only one complete, non-overlapping union of pending legacy fragments
  and records every original item and batch in the immutable recovery ledger.
  Original batches and item ownership are never rewritten. Missing or
  overlapping coverage still fails closed, and replay reuses the same recovery
  set without a duplicate Jira effect.
- First promotion: PR #158 merged as `2b544ac5692c5c239b2bfd3fc435572206831322`,
  passed the complete clean-archive verifier and was deployed as daemon hash
  `847d49120977a6d4762e9ff7136d9c8f5ada3ea06177a07822b2139acaa0ec1d`.
  The exact persisted apply then failed closed with HTTP 409 before any new
  Jira issue or binding was committed. Recovery had incorrectly strengthened
  every historical `Link` item to require Kontor's creation marker, although
  those pre-existing issues were never created by Kontor.
- Follow-up correction: marker proof is now selected per recovered item.
  Historical `Link` items are proven by exact key, project, issue type and
  parent; only a recovered `Create` requires its immutable marker. Both mixed
  recovery tests now use an ordinary linked epic with no marker and deliberately
  different Jira prose, while the adopted `Create` remains marker-checked.
- Resume checkpoint: merge and deploy the verified correction, replay the same
  Kontor apply receipt, then read back all three new ASMA bindings and a second
  effect-free replay before closing this gap.
- Owner/status: follow-up correction verified locally; final promotion and
  recovery readback pending.
