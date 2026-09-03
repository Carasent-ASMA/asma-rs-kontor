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
