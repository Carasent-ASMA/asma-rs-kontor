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
- Second promotion: PR #159 merged as
  `eba40aacc3b0dcfe935c17f56b4901203a46ec2d`, passed both staged and clean
  archive verification, and was deployed as daemon hash
  `73e36142d743e965bf7dc58a239837c77b061877dcbfaef564c305fa263e700a`
  with PID `77742`. Its rollback unit is
  `/Users/igor/.local/state/kontor/asma/deploy-backups/20260903T201428Z-kon-op-22-eba40aa/`.
  The replay correctly confirmed the legacy Epic `ASMA-7869`, then failed
  closed at ordinal 1 before creating a new Jira issue.
- Remaining failure class: Kontor's internal item kind `task` was incorrectly
  compared with Jira's literal issue-type name `Task`. The 18 historical links
  are valid Jira hierarchy-level-zero children of `ASMA-7869`: 16 are
  `User Story` and two are `Tech tasks`.
- Bounded fallback: after Kontor exposed only the typed conflict and its normal
  ticket reconciliation reported the first task converged, read-only Atlassian
  inspection was limited to the exact 18 `ASMA` keys in this materialization.
  It established issue type, hierarchy and parent only. No Jira or database
  mutation was made. This inspection was necessary because the current Kontor
  read surface does not expose a materialization item's observed Jira type and
  parent in its refusal.
- Final correction: an ordinary explicit `Link` for a Kontor task accepts a
  non-subtask Jira work item at hierarchy level zero, independent of Jira's
  project-specific type name. New creates and recovered `Create` intents remain
  strict to the literal Jira `Task` type Kontor chose, exact parent, content and
  immutable marker.
- Third promotion: PR #160 merged as
  `162d80710045cb37662d976a64217153f9f65132`, passed both the staged and clean
  archive verifiers, and was deployed as daemon hash
  `f1592ef87a42616345ffd6def9a6d2489cd263536a1eef2f9462c7cb2fc5cbbd`
  with PID `90200`. Its rollback unit is
  `/Users/igor/.local/state/kontor/asma/deploy-backups/20260903T213548Z-kon-op-22-162d807/`.
  The replay accepted Jira hierarchy readback and was then refused before any
  Jira create because confirmation searched only the new `connector.jira`
  spelling while the immutable canonical ledger selected a preserved legacy
  `jira` row.
- Final ledger correction: materialization confirmation now consults the
  canonical Jira task-link ledger directly. An exact migrated legacy binding
  supplies its existing stable link id; a different task or issue still fails
  closed. The regression reconstructs the v80 legacy-only row, applies the v81
  canonical-ledger migration, confirms the pending materialization and proves
  that no duplicate link is created.
- Fourth promotion: PR #161 merged as
  `3705ba96aaedc1c98730baa6ce9cceca62a795e7`. The staged exact-tree verifier
  passed every local code, Rust, schema, policy, frontend and dependency gate;
  the final npm advisory request alone timed out after the immediately
  preceding PR #160 archive verifier reported no production vulnerabilities.
  The exact merge was deployed as daemon hash
  `be248241ce784172e258601eb2ae5b18bb2dc72bb7f1ecd575241072000b8ee6`
  with PID `33075`; its rollback unit is
  `/Users/igor/.local/state/kontor/asma/deploy-backups/20260903T223249Z-kon-op-22-3705ba9/`.
- Live replay result: ordinals 0–18, including the epic and all historical
  links, are confirmed. Ordinals 19–21 remain planned. The first Jira `Task`
  create returned a non-success response, but the connector collapsed every
  Jira rejection to a generic transport-unavailable error.
- Bounded fallback: the Atlassian metadata connector returned HTTP 405. A
  read-only request through Kontor's configured keychain credential then
  inspected only ASMA create metadata and the ASMA-7869 children; it made no
  Jira mutation and printed no credential. Jira's `Task` type is a valid
  hierarchy-level-zero non-subtask, but its create screen requires Product
  (`customfield_10251`) with no default. Every existing child of ASMA-7869 uses
  Product `Both`, option `10459`.
- Current correction: `jira.json` may declare bounded additional create fields
  independently for epics and tasks. It cannot override Kontor-owned
  structural fields. The ASMA operator configuration supplies Task Product
  option `10459`; Jira 400 diagnostics retain only safe field identifiers and
  discard Jira's prose.
- Resume checkpoint: promote this create-contract correction, replay the same
  Kontor apply receipt, read back all three bindings, then prove an exact
  effect-free replay before closing the gap.
- Owner/status: implementation, connector regression and strict clippy pass;
  full local release verification, promotion and recovery readback pending.
