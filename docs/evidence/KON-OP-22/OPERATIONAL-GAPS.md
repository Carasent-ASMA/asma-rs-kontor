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
- Owner/status: KON-OP-22 delivery owner; open until the branch is merged and
  the reused worktree is removed.

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
