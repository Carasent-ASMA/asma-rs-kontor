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
- Final correction: `jira.json` may declare bounded additional create fields
  independently for epics and tasks. It cannot override Kontor-owned
  structural fields. The ASMA operator configuration supplies Task Product
  option `10459`; Jira 400 diagnostics retain only safe field identifiers and
  discard Jira's prose.
- Fifth promotion: commit `7d13379999fe6f9aa45ba7fd00a85bbc7311741d`
  merged through PR #162 as
  `d224b153bd6430f1df37e6c9a96a59cec9ab17b0`. The complete local release
  verifier passed formatting, workspace Clippy, every locked Rust test, schema
  and recovery suites, Cargo audit/deny, frontend typecheck, all 296 frontend
  tests and the production dependency audit. GitHub Actions remained disabled
  by the explicit Kontor policy.
- Live deployment: the exact merge daemon hash is
  `248e7a385a5cafcaa990087b9d7f1e42fb914348a5506b34d35caf0f180b8744`.
  LaunchAgent PID `43055` serves the healthy schema-v83 realm. The coherent
  rollback unit and verified pre-deploy snapshot are under
  `/Users/igor/.local/state/kontor/asma/deploy-backups/20260903T232131Z-kon-op-22-d224b15/`.
- Recovery result: the original materialization receipt
  `01a0683f-7579-7d82-90b7-00781902f8b3` activated as
  `01a06994-ff6e-7501-82d5-1199259eea08` and created exactly `ASMA-8088`,
  `ASMA-8089` and `ASMA-8090`. An identical replay returned those same receipt,
  activation, batch, link and issue identities.
- Final readback: the three issues are Jira `Task` items under `ASMA-7869`,
  carry Product `Both`, have distinct immutable Kontor markers and are
  `In Development`. All 21 task reconciliation plans for this epic return
  `converged: true` with empty diffs. Jira Epic `ASMA-7869` is also
  `In Development`, and its Kontor epic has no unresolved Jira conflict.
- Owner/status: closed end to end. No direct Jira or database mutation was used
  for recovery; the only fallbacks were the documented read-only diagnostics.

## 2026-09-04 — KOP correction exposed a legacy ECP title dependency

- Intended Kontor operation: preview the evidence-preserving recovery of ECP
  topology node `01a00c26-2862-7121-ad2c-3e3028497669` in project
  `01a0064a-e056-7603-9968-ef64fdaacb75`, after the one-time epic backlog-code
  correction changed epic `01a0074f-6719-7570-adf7-95ee3ec69875` from `OP` to
  `KOP` under receipt `01a06d91-d3a1-7142-8d82-f96f3ef29931`.
- Failure class: `unsupported_capability` / workspace mismatch. The live runtime
  proved one exact-parent/exact-path replacement candidate for stale native id
  `wks_0c695ce96e2c4296`, but that candidate carried
  `ECP • ASMA-7869 • Kontor Operational MVP` while the epic's still-pinned
  topology-v1 template rendered `ECP • ASMA-7869 • OP`. Preview failed closed
  before changing either identity.
- Exact scope: Realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`, Paseo project
  `prj_b95f6d73b8de6c59`, ECP topology node above, and its sole exact-path live
  candidate. No other workspace, project, seat or Jira issue is in scope.
- Bounded fallback: Paseo was used only to retitle that already-proved candidate.
  An initial `ECP • KOP-7869` attempt exposed that recovery deliberately renders
  the currently pinned topology rather than the project's unpinned Team
  Definition; the immutable topology-v1 document was then read through Kontor,
  and the same workspace was corrected to `ECP • ASMA-7869 • OP`. Readback kept
  workspace `wks_900499bd8e2e59cb`, project `prj_b95f6d73b8de6c59`, and the exact
  canonical path unchanged. No other Paseo mutation and no direct Kontor
  database write occurred.
- Recovery result: the resumed Kontor preview hash was
  `a91b1dd0d62c71b3b7c96c7198893c7964c3797c74d6d7ac0c2c27e05dc577bf`.
  Kontor atomically replaced only stale native id `wks_0c695ce96e2c4296` with
  `wks_900499bd8e2e59cb` under receipt
  `01a06d95-d086-75a2-863c-5ff19d93d692`, preserving the topology node and
  canonical path.
- Owner/status: closed end to end by the KON-OP-22 delivery owner. The original
  Kontor workflow resumed at the recovery checkpoint; Team Definition migration
  remains a separate next operation.

## 2026-09-05 — legacy consultation container bindings blocked native-name migration

- Intended Kontor operation: preview the identity-preserving upgrade of epic
  `01a0074f-6719-7570-adf7-95ee3ec69875` to Team Definition
  `01936f5a-2000-7000-8000-000000000001` v2, using the five explicitly supplied
  legacy consultation topics recorded in `OPEN-QUESTIONS.md`.
- Failure class: `stale_binding`. CSW topology node
  `01a02bb3-2614-7711-8a02-8978ab947be8` named absent workspace
  `wks_163b779bb853680e`; ASW node
  `01a02d6e-4db9-7372-b2b8-c815da222dc5` named absent workspace
  `wks_124e30e7ebcff8f1`. The migration refused before changing a title or pin.
- Bounded fallback: Paseo was used only to create empty replacement workspaces
  under the existing epic project and the exact canonical paths that Kontor
  required. No agent, seat, project, Git worktree, branch or code was created.
  Kontor then previewed and adopted CSW `wks_5dff06d16622a38d` under receipt
  `01a07091-b37a-7591-b244-7e7ababb9742` and ASW
  `wks_6b2fdfdfff62cd89` under receipt
  `01a07091-d525-7bd0-9120-b7d0a1799112`, retaining both topology-node
  identities and before/after evidence.
- Contained correction: an initial CSW candidate
  `wks_f6989674acc87b20` was created with the wrong path. It held zero agents and
  was immediately archived; its empty mistaken directory was removed. It was
  never offered to or adopted by Kontor and changed no control-plane identity.
- Owner/status: the two consultation container gaps are closed. Migration
  preview resumed and exposed the next separately recorded stale binding.

## 2026-09-05 — completed OP-20 topology named a missing historical TSW

- Intended Kontor operation: resume the same Team Definition upgrade preview.
- Failure class: `stale_binding`. The completed OP-20 task still had an active
  TSW topology node naming absent workspace `wks_f4a6…`, although its TeamRun
  had succeeded and every seat was already retired.
- Bounded fallback: a direct Paseo create was attempted only through the
  supported surface and was refused as an unsafe duplicate. No bypass and no
  native mutation followed. Kontor first upgraded the epic topology pin from v1
  to v4 under receipt `01a07095-0c7a-7893-a313-91a53eb0d780`, then retired only
  the stale OP-20 TSW node under receipt
  `01a07095-ca5d-76b2-b626-de6463058220`. The task, TeamRun, SeatBindings,
  branch, Git worktree, commits and historical native binding remain preserved.
- Owner/status: closed without deletion. The migration resumed from the same
  checkpoint.

## 2026-09-05 — bounded runtime census and verdict readback

- Intended Kontor operations: identify the still-unnamed migration subject and
  read the terminal inspector responses needed for evidence-bound role-turn
  settlement.
- Failure class: the whole-epic migration refusal did not identify the stale
  binding, and Kontor's timeline projection deliberately omits message bodies.
- Bounded fallback: read-only Paseo status/activity calls were limited to the
  exact OP-21/OP-22 delivery seats and the three legacy consultation natives.
  They proved which immutable native ids still existed, their exact workspace
  ids, and the inspectors' already-rendered verdicts. No Paseo message, title,
  workspace, agent or lifecycle mutation was made by this census.
- Resume checkpoint: use Kontor's exact runtime positions for turn settlement,
  close the evidence gaps identified by the inspectors, and continue the
  configuration-driven migration. Owner/status: open until those original
  Kontor operations complete.

## 2026-09-05 — legacy Committee seats referenced an absent predecessor workspace

- Intended Kontor operation: resume the whole-epic Team Definition migration
  after every active container passed individual readback.
- Failure class: `stale_binding` / seat-container correlation drift. CSW node
  `01a02bb5-fcf6-7ea0-9849-168a6975c671` correctly bound active workspace
  `wks_88b7239acec72548`, while reviewer natives
  `d1ad7093-0786-4a56-9b9c-fdb2b4eefb61` and
  `55444de6-e5f1-42b4-8a81-373e88b1a2ee` still reported absent predecessor
  workspace `wks_6ebdd414d51c49e8`.
- Supported recovery: Kontor resumed an already-prepared, empty-profile
  provider recovery for reviewer A and selected governed alias `claude-work`;
  receipt `01a070b0-c897-7ad3-b8c9-6ec73d65cbf6` preserved SeatBinding
  `01a02bb5-fcf6-7ea0-9849-1697ca5918e6` and installed native
  `e83106cf-14e4-4a2d-81d3-a2d5cd605c48`. It then recovered reviewer B to
  governed alias `codex-work`; receipt
  `01a070b1-2c0d-7310-8640-f0a7c55ff91a` preserved SeatBinding
  `01a02bb5-fcf6-7ea0-9849-16add5e1076a` and installed native
  `497ce202-d2c1-4cef-b807-7fe7a318a837`. Both successors read back in the
  existing `wks_88b…` CSW and the old native fillers were archived by the
  supported recovery transaction.
- Effects: no logical seat, Committee, workspace, project, worktree, branch,
  finding or evidence identity was replaced. The Committee's provider-family
  diversity remains Claude plus Codex.
- Owner/status: closed. The resumed migration then exposed a separate archived
  Advisor compatibility condition addressed by the regression below.

## 2026-09-05 — archived Advisor exact fetch was misclassified as live drift

- Intended Kontor operation: preflight the remaining ASW seat while preserving
  archived consultation history.
- Failure class: runtime compatibility. Paseo exact fetch returned archived
  advisor `64233745-6091-4b8d-a184-407c785dac0e` with its historical workspace
  `wks_124e30e7ebcff8f1`; Kontor assumed archived agents would be absent and
  therefore compared that historical workspace with the recovered active ASW
  container `wks_6b2fdfdfff62cd89` as though it were a live seat.
- Correction: `preview_retitle_seat` now classifies an exact archived agent as
  `StaleBinding` before live provider-session/workspace correlation. The
  whole-epic naming planner already converts that supported class to
  `rename_pending`, so it preserves the logical SeatBinding and archived native
  evidence while applying no seat rename. A captured archived-agent contract
  fixture proves the behavior and no native update.
- Owner/status: implementation and focused regression are complete; deployment
  and the resumed live migration remain the closeout checkpoint.
