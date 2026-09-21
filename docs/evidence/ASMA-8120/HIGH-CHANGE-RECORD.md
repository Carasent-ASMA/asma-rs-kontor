# ASMA-8120 high-change record

**Current section: 2026-09-21 evidence-only closeout.** Everything below the
History divider is the earlier record, preserved verbatim including its
refusals, partial outcomes and superseded counts. Where the earlier text is
wrong, it is corrected here rather than edited there.

Task: Jira `ASMA-8120` / Kontor `01a07722-c3ed-7a63-94e6-cefd22e438ab`,
InProgress r2, workflow high-verification r3.
Project `01a0064a-e056-7603-9968-ef64fdaacb75`,
realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`.
Authored from AgentRun `01a0bb74-ee78-7aa3-b6bc-bd9c43733496`
(native `6426ec59-57d0-4001-922c-86fc0f2de258`) in the existing task worktree.
Root retains live deployment and topology ownership; the watchdog remains
stopped. This turn performed **no** Kontor mutation, migration, restart or
topology change — it reads durable evidence and edits this report only.

## Status: migration complete, restart proof for the full set still pending

All 13 approved eligible epics are migrated and pinned. This is **not** a
task-level PASS claim, and no PASS is asserted here: the coordinated restart
proof covering all 13 is outstanding (see "Restart coverage" below).

Independent supported read at snapshot cursor **4573**
(`kontor_native_names_preview`, read-only, observed 2026-09-21T10:13:16.340Z),
verified in this turn rather than taken on report:

- **88** current targets across the 13 epics
- **0** pending changes
- **0** observed titles still carrying a legacy item code (`KBI-`,
  `KGVCASWR-`, `KOP-`, `KCADR-`)
- 85 targets `unchanged`; 3 `rename_pending` (recorded below)
- single snapshot cursor 4573 across all 13 reads

## Corrections to the earlier record

The QA report [`QA-REPORT.md`](QA-REPORT.md) (verdict **BLOCKED**, candidate
`19521d12164a8513c48328581bd7d5517cffee5d`) is preserved unmodified. Its
evidence defects are corrected here:

- **Q-8120-QA-01 / count.** The earlier section says "the remaining eleven
  epics". That was wrong: neither attempted apply took effect, so **twelve**
  remained unmigrated at that time, not eleven. All twelve have since been
  migrated by root.
- **Q-8120-QA-02 / OQ-8120-04 wording.** That entry said "the seven migrations
  recorded in the high-change record". No migration had been recorded at that
  point — there were seven clean **previews** and zero successful applies from
  this seat. Corrected in the ledger.
- **Q-8120-QA-02 / truncated hashes.** The earlier section stored preview
  hashes as 8-character prefixes, which are not executable apply inputs. This
  section carries full hashes only.
- **Attribution.** Every migration, deployment and recovery below is attributed
  to the actual root receipt that performed it. This turn's own durable actions
  are limited to the four publications and the default selection.

## Publication and default selection — this turn's receipts

Published 2026-09-20 from this AgentRun against project revision 7. Each
candidate validated against live published topology bytes with zero violations,
and each `validation_hash` reproduced the hash ASMA-8117 recorded. Verified
still live at closeout.

| Target | Lineage | Ver | Canonical hash | Receipt | Idempotency key |
|---|---|---|---|---|---|
| Operational v4 | `01936f5a-2000-7000-8000-000000000001` | 4 | `d3775bd5fbebadeec06cc235793be23248de2fed23c4ef01e3a523fecc282284` | `01a0bbcd-7662-7c31-bd25-e43646e00e35` | `asma-8120-td-publish-operational-v4` |
| Operational v5 | `01936f5a-2000-7000-8000-000000000001` | 5 | `10a0682b16786a3f94400d1d9562dfd9a8dd34379fef9c319a18a92a95989fc1` | `01a0bbcd-98d6-79b0-983a-0d89bda881a1` | `asma-8120-td-publish-operational-v5` |
| Operational v6 | `01936f5a-2000-7000-8000-000000000001` | 6 | `24e2c5810060ef521e6af087e3430754bab86eaf138868fe125aa4fdd2904794` | `01a0bbcd-bd75-7462-a0c9-8be1871ab2e8` | `asma-8120-td-publish-operational-v6` |
| Recovery v3 | `01a07400-1000-7000-8000-000000008098` | 3 | `c4c2f19a52258872dd0fb68ec8c648ad9d6ae2674ad0e014cce42a270644ae73` | `01a0bbcd-d675-7e23-a950-d819a20d0d0e` | `asma-8120-td-publish-recovery-v3` |

**Default selection.** Operational v3 → v6, preview hash
`132ab73ffc319d5ba068537f6964a21b67460bf16a9383f3c69bedb809e4779f`, applied
`updated`, receipt `01a0bbce-43f4-7d62-bbaa-d0844d680340`, idempotency key
`asma-8120-td-default-select-operational-v6`, expected revision 7. Readback:
re-previewing the same target refuses `400 invalid_request`, "the project
already selects that Team Definition revision".

## All 13 identity tuples — before / apply / after / restart

Before pin and apply receipt from
`asma-8120-final-13epic-pin-receipts.json`; after state from the cursor-4573
`asma-8120-final-13epic-naming-readback.json`. Per-target native ids, parents,
cwds, seat bindings and titles for all 88 targets live in those two files,
whose SHA256 are recorded under "Cited evidence" — they are the authoritative
per-target detail and are not re-transcribed here.

| Epic | Before pin | After pin | Apply receipt (root) | Pinned at (UTC) | Targets | Pending | Legacy | Restart-covered |
|---|---|---|---|---|---|---|---|---|
| ASMA-7869 | v2 | v5 | `01a0c348-7862-7122-925c-6f3a24e6957a` | 2026-09-21T09:25:01.651234Z | 5 | 0 | 0 | yes |
| ASMA-8049 | v3 | v6 | `01a0bbf1-8f97-7012-a5a4-9bbf56ecf5c3` | 2026-09-19T23:12:45.454005Z | 12 | 0 | 0 | yes |
| ASMA-8098 | v2 | v3 | `01a0c348-7eec-7691-aa57-595131b76fd5` | 2026-09-21T09:25:03.3362Z | 9 | 0 | 0 | yes |
| ASMA-8101 | v3 | v6 | `01a0c349-9219-7da0-9618-c3a2b3d4a206` | 2026-09-21T09:26:13.777994Z | 11 | 0 | 0 | yes |
| ASMA-8108 | v2 | v5 | `01a0c069-be98-7013-a280-de6017f80f68` | 2026-09-20T20:02:30.672724Z | 3 | 0 | 0 | yes |
| ASMA-8109 | v1 | v4 | `01a0c06f-08f6-7a22-9fea-95a64e0f010f` | 2026-09-20T20:08:17.390911Z | 7 | 0 | 0 | yes |
| ASMA-8111 | v2 | v5 | `01a0c373-17a8-73e1-84cd-608247b938a4` | 2026-09-21T10:11:34.947969Z | 3 | 0 | 0 | **no — migrated after restart** |
| ASMA-8113 | v3 | v6 | `01a0c33f-c934-7fa1-a1ad-83b837ec51e1` | 2026-09-21T09:15:32.528189Z | 0 | 0 | 0 | yes |
| ASMA-8155 | v3 | v6 | `01a0c33f-c9fa-7730-b738-c6ae0026ca56` | 2026-09-21T09:15:32.726053Z | 0 | 0 | 0 | yes |
| ASMA-8186 | v3 | v6 | `01a0c33f-ccb3-7d71-bc05-84c2cf71deb8` | 2026-09-21T09:15:33.422662Z | 5 | 0 | 0 | yes |
| ASMA-8188 | v3 | v6 | `01a0c061-4ef5-76b2-9841-84a37827cc9d` | 2026-09-20T19:53:17.803964Z | 9 | 0 | 0 | yes |
| ASMA-8190 | v3 | v6 | `01a0c348-7c69-74e0-9fca-48e2d040ec4d` | 2026-09-21T09:25:02.691554Z | 24 | 0 | 0 | yes |
| ASMA-8208 | v3 | v6 | `01a0c33f-cad7-7970-9309-60fbe48adbf3` | 2026-09-21T09:15:32.943454Z | 0 | 0 | 0 | yes |

Only ASMA-8049 was migrated by this seat's preview; its apply receipt
`01a0bbf1-8f97-7012-a5a4-9bbf56ecf5c3` belongs to the ASMA-8049 LSA seat
`6301c2d4`, not to this turn. The other twelve are root's.

### Restart coverage — incomplete, and not claimed otherwise

The live deployment verified at **2026-09-21T10:02:36.117472Z**. ASMA-8111 was
pinned at **10:11:34.947969Z**, i.e. **after** that restart. Twelve of thirteen
epics therefore have post-migration restart evidence; ASMA-8111 does not.

**Final all-13 restart persistence proof remains pending** until root's next
coordinated restart and readback. No restart was performed by this turn and no
PASS is manufactured from the partial coverage.

## Live deployment in force

`/Users/igor/.local/state/kontor/asma/deployments/ASMA-8188-20260921T100129Z-44663e10/deployment.json`

- source commit `44663e10e063ad3d11c0c84ca6a87d680d75e835`, schema **118**, live
- `quick_check: ok`, `foreign_key_violations: 0`, `identities_preserved: true`,
  `signatures_valid: true`
- identity counts: 18 mini-projects, 136 tasks, 466 agent runs, 646 seat
  bindings, 393 runtime bindings, 21 hosted topology seats
- installed hashes — `kontor`
  `cef7ad90db3b30af4e10021cfe8141a977e6fb584ac6dbde0a6f507ef5b1fbe7`;
  `kontor-daemon`
  `d1d670f79486eda9b01e1be9fdb5e36708012dba6dd6730c0b3b1a727d741fce`;
  `kontor-mcp`
  `7d7210c84c472fbc835d8ab2bf5650c61e1836213a73e825ff963d4f1ee28316`
- watchdog remains stopped

**Launch-count anomaly retained, not closed.** The record carries
`operator_restart_count: 1` against `observed_launch_delta: 2`, with
`launch_anomaly: "Unexpected automatic launch count; see launchctl-after.txt;
root-cause review required"`. That review is outstanding and is carried forward
here rather than waived.

## Four mutation-ineligible legacy epics (approved OQ-8120-02)

Per approved memory `open-question-disposition-oq-8120-02-20260920` — bounded
rollout option (b), evidence `approved_scope_revision`
`01a0bb85-bb98-74b2-a83f-c18288050587`, source commit `1e11e4ab`. These four
active legacy epics stay explicitly mutation-ineligible; no key, pin or
topology was inferred for them, and they did not block the eligible rollout:

1. QNR v2 Nonprod Delivery — no pin, no confirmed binding
2. Kontor Operator Surface and Attention Channel — no pin, no confirmed binding
3. Catalog workspace without gitlinks — no pin, no confirmed binding
4. Repair stale-native Core Team seat succession — Operational v3 pin, no
   confirmed Jira binding

Each requires its own supported binding-and-pin repair or governed historical
classification before it is eligible.

## Approved OQ-8120-04 / OQ-8120-05 disposition

From approved operator memory
`asma-8120-rollout-dispositions-20260920-direct-repair` (approved, current;
proposed by the Codex direct-repair coordinator 2026-09-20T18:02:14.813769Z,
provenance source `operator`, source id `ASMA-8120`):

- **OQ-8120-04 — resolved.** "A deterministic pre-effect preview refusal fences
  its exact epic. Uncertain apply/readback effects halt rollout globally until
  reconciled." This confirms the reading this seat recorded before acting, and
  the distinction it turns on: the five refusals were pre-effect and wrote
  nothing.
- **OQ-8120-05 — source cause identified, repair owned by root.** The refusal
  was **not** this seat's running/idle state, which is what this seat had
  hypothesised. Actual cause: "Services owns one `native_lifecycle_guard`
  RwLock across the fleet. `native_lifecycle_change` uses `try_write`;
  `native_activity` uses `try_read`. The refusal counts in-flight control-plane
  native operations, not the model's running/idle state and not per-epic
  activity. A caller with ongoing background readers may starve without a
  queued writer." Source: `crates/kontor-daemon/src/applications.rs:955-971` at
  `0f649824`. Disposition: "Root owns bounded lifecycle-guard repair and exact
  migration preview/apply; do not cancel, park, replace or archive live roles to
  clear the transient guard."

The same disposition records these census corrections, which this section
honours: seven clean previews are not seven successful migrations; twelve
remained, not eleven; the canary already had a success receipt; and truncated
historical hashes are not executable apply inputs.

## Unfinished NULL-subject consultation results — preserved, not resolved

From `asma-8120-rollout-consultation-readback.json`, 10 consultation runs read:

- **1 refuses**: `01a0298c-6284-7a83-8d35-158c9c4e82e4` →
  `400 invalid_request`, subject `ModelRung`, rule "a value did not satisfy the
  invariant of its type". This is the unfinished NULL-subject case and is
  **carried forward unresolved**.
- **3 unfinished, result `null`**: `01a02ba2-d1cd-7fc0-9bdb-1704dd4a544c`
  (materializing), `01a02bb3-2614-7711-8a02-896d545a9292` (materializing),
  `01a02d6e-4db9-7372-b2b8-c8024f41f3e7` (running).
- **6 settled with durable results**, round 1, 3 findings each — compliant:
  `01a0758b-ec8f-7271-a91f-ccd93c1b7202`,
  `01a075bf-c12f-7140-8c19-380229a13ef2`; non-compliant:
  `01a02bb5-fcf6-7ea0-9849-167564b9bdaf`,
  `01a073e1-7d9e-7ce1-8100-12c4cc0ca35c`,
  `01a078a6-3507-7020-a8ea-ab9100221c22`,
  `01a0aaf0-1c89-7f31-989c-b6ff7b0d7b95`.

No consultation subject was inferred, repaired or settled by this turn.

The three `rename_pending` targets in the cursor-4573 readback are the seats of
an unfinished consultation in ASMA-8188, each with a null observed title:
`e5b2e5d7-fa35-4e14-99cc-49770a0faf08` (desired `SEAT A`, seat binding
`01a0c02a-ca73-7b71-a70e-6fc8568761e3`),
`9156c0ce-5851-4371-b419-fdcd6569f4d1` (`SEAT B`,
`01a0c02a-ca73-7b71-a70e-6fd4afd42e6c`) and
`a805e24f-d2fe-4ded-a497-b4b59633d195` (`JUDGE`,
`01a0c02a-ca73-7b71-a70e-6fe4792346d3`). They are pending, not failing, and
carry no legacy item code.

## Logical retirement receipts (root)

From `asma-8120-retirement-receipts.jsonl`, 108 records covering **12 logical
node retirements** and **24 seat retirements**, each with a before-state, the
retire receipt and an exact readback:

- 12 × `before_node_retire` / `node_retire` / `node_readback`
- 24 × `before_seat_retire` / `seat_retire` / `seat_readback`

`asma-8120-retirement-final-readback.json` records 12 node readbacks and 9
preservation checks, **all 9** reporting
`state_result_revision_subject_unchanged: true`. Retirements are logical: tasks,
terminal TeamRuns and retired seats remain as historical evidence and no
workspace was recreated. These are root's receipts, not this turn's.

## Cited evidence and integrity

| Artifact | SHA256 |
|---|---|
| `asma-8120-final-13epic-naming-readback.json` | `79b250baa2f4f2ec7d0589ae6d253b5d4b45367920c4cb927ff6996fea9b42ae` |
| `asma-8120-final-13epic-pin-receipts.json` | `71bdbab454e1556d933409a86d404ef73d88a1a363f94284de016e59f514317e` |
| `asma-8120-retirement-receipts.jsonl` | `8dcb741e000d820e54db2a5ea1483b9ec48d5d27e4372e73c06b007df51d1c45` |
| `asma-8120-rollout-consultation-readback.json` | `35b83858d1a6347a0bb83e2a2e662d1ce50c294fb0d93d9b33789229a734e71c` |
| `deployment.json` (ASMA-8188-20260921T100129Z-44663e10) | `cdb7d6905d18316a5839be11d9786d88d074fbef678ecf301b1277b1ba42a41a` |

All five live under
`/Users/igor/.local/state/kontor/asma/repair-worktrees/20260921/` except the
deployment record, which is under
`/Users/igor/.local/state/kontor/asma/deployments/ASMA-8188-20260921T100129Z-44663e10/`.

## Source regression repaired under this task

`77148a4c` — `fix(kontor): Declare the worktree repair's legal target
(ASMA-8120)`. PR #241 (`267c67a7`) legalized `CorrectTaskWorktree` against a
task without declaring the pair in the `domain_state` expectation table,
leaving `kontor-core` red from that commit onward including on `origin/master`
`95bcf868`. Production behaviour unchanged. QA independently reproduced the
causal baseline failure at `1e11e4ab` and its disappearance at `77148a4c`, and
recorded `cargo fmt --all -- --check`, strict Clippy, 38/38 `domain_state` and
324/324 across 17 `kontor-core` targets as passing.

## Remaining evidence before a PASS can be sought

1. Coordinated restart and post-restart readback covering **all 13** epics,
   including ASMA-8111 — the single outstanding identity-tuple gap.
2. Root-cause disposition of the deployment launch-count anomaly
   (`operator_restart_count: 1` vs `observed_launch_delta: 2`).
3. Closure or explicit governed carry-forward of the unfinished NULL-subject
   consultation `01a0298c-6284-7a83-8d35-158c9c4e82e4` and the three
   `rename_pending` ASMA-8188 seats.
4. Completion of `OQ-8120-05`'s bounded `native_lifecycle_guard` repair, which
   is root-owned and still `source_cause_identified_repair_in_progress`.
5. Re-presentation to QA against this corrected record.

The settled high-change claim `01a0bc11-f213-76c0-ad25-a8fc9ddfa837` is genuine.
Root may register a verified Git blob with explicit
`operator_recovered_git_blob` provenance once this content is complete. No
workflow rollback or new TeamRun is required merely to update this evidence.

---

# History — preserved verbatim below this line

The following is the earlier record exactly as written, including counts and
conclusions now superseded above.

# ASMA-8120 high-change record — PARTIAL, fleet fenced

Date: 2026-09-20
Artifact: `high-change` (incomplete: rollout stopped at the canary fence)
Task: Jira `ASMA-8120` / Kontor `01a07722-c3ed-7a63-94e6-cefd22e438ab`
Epic: Jira `ASMA-8049` / Kontor `01a0539a-51c9-7301-9bd7-26c09167b23e`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`, revision 7
Realm: `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`
Implement AgentRun: `01a0bb74-ee78-7aa3-b6bc-bd9c43733496`
(native `6426ec59-57d0-4001-922c-86fc0f2de258`)
TeamRun: `01a0bb61-b8e0-7bf0-a14f-b44713ca10d7`

Consumed approved scope revision `01a0bb85-bb98-74b2-a83f-c18288050587`,
source commit `1e11e4ab`, settled scope turn
`01a0bbc7-0631-7f81-ad6a-8f4793e82aba`.

**This artifact is not the complete evidence the scope record requires.**
Rollout steps 2 and 3 are durable and verified. Step 5 refused, so steps 5-7
carry no receipts. See [OQ-8120-03](OPEN-QUESTIONS.md).

## Admitted question resolutions

| Entry | Disposition | Approved revision | Receipt |
|---|---|---|---|
| OQ-8120-01 | option (a); later exact-binding challenged settlement authoritative | `01a0bbc7-8d72-7910-b4dc-accf4e0538a9` | `01a0bbc8-0227-7160-bd18-10155b3ea87a` |
| OQ-8120-02 | option (b); 13 eligible only, 4 legacy epics mutation-ineligible | `01a0bbc7-afc9-7561-9551-33d1eeb9345a` | `01a0bbc8-75d2-7c01-9ff2-227255a6f171` |

OQ-8120-01 held the publication fence. It was lifted on that authority, and
publication proceeded only after it.

## Baseline, re-read before the first mutation

Project revision 7 and schema 108, matching the scope record exactly.

| Check | Before | After |
|---|---|---|
| `PRAGMA integrity_check` | ok | ok |
| `PRAGMA foreign_key_check` | 0 rows | 0 rows |
| `PRAGMA user_version` | 108 | 108 |

## Step 2 — successor publication (complete, verified)

All four candidates validated against live published topology bytes with
**zero violations**, and each `validation_hash` reproduced the hash recorded by
ASMA-8117. Each manifest source hash was confirmed byte-identical to its live
published source before publishing.

| Target | Lineage | Version | Canonical hash | Applied | Receipt | Idempotency key |
|---|---|---|---|---|---|---|
| Operational v4 | `01936f5a-2000-7000-8000-000000000001` | 4 | `d3775bd5fbebadeec06cc235793be23248de2fed23c4ef01e3a523fecc282284` | created | `01a0bbcd-7662-7c31-bd25-e43646e00e35` | `asma-8120-td-publish-operational-v4` |
| Operational v5 | `01936f5a-2000-7000-8000-000000000001` | 5 | `10a0682b16786a3f94400d1d9562dfd9a8dd34379fef9c319a18a92a95989fc1` | created | `01a0bbcd-98d6-79b0-983a-0d89bda881a1` | `asma-8120-td-publish-operational-v5` |
| Operational v6 | `01936f5a-2000-7000-8000-000000000001` | 6 | `24e2c5810060ef521e6af087e3430754bab86eaf138868fe125aa4fdd2904794` | created | `01a0bbcd-bd75-7462-a0c9-8be1871ab2e8` | `asma-8120-td-publish-operational-v6` |
| Recovery v3 | `01a07400-1000-7000-8000-000000008098` | 3 | `c4c2f19a52258872dd0fb68ec8c648ad9d6ae2674ad0e014cce42a270644ae73` | created | `01a0bbcd-d675-7e23-a950-d819a20d0d0e` | `asma-8120-td-publish-recovery-v3` |

Readback via `team-definitions-list` confirms all four exact
`{id, version, canonical_hash}` triples, and that the three prior Operational
revisions and two prior Recovery revisions are unchanged.

## Step 3 — project default selection (complete, verified)

Preview: current Operational v3 `3818bade075891fe…` → target Operational v6
`24e2c5810060ef52…`, preview hash
`132ab73ffc319d5ba068537f6964a21b67460bf16a9383f3c69bedb809e4779f`. The preview
named the default move only and no epic rename.

Apply: `updated`, receipt `01a0bbce-43f4-7d62-bbaa-d0844d680340`, idempotency
key `asma-8120-td-default-select-operational-v6`, expected revision 7.

Readback: re-previewing the same target now refuses `400 invalid_request`,
rule "the project already selects that Team Definition revision" — the exact
confirmation that the default is Operational v6.

## Step 4 — canary preview (captured, deterministic)

ASMA-8049 from Operational v3 to v6. Preview hash
`aa2c7dd13bd09671078e5906f3caa2890a9d8fec020941c6860001b0f04b9413`, stable
across repeated calls. Eleven live targets, matching the scope record's census
(ESW, ECP, five TSWs, four seats) with no omission or addition.

| Subject | Observed | Desired | Changes |
|---|---|---|---|
| `prj_c393c7173c5a4b19` ESW | `ESW • KBI-8049` | `ESW • ASMA-8049` | yes |
| `wks_dbba8b9698eb6b66` ECP | `ECP • KBI-8049` | `ECP • ASMA-8049` | yes |
| `wks_ae58a58825d07e12` TSW | `TSW • KBI-8116` | `TSW • ASMA-8116` | yes |
| `wks_ed2ba2b56094f14d` TSW | `TSW • KBI-8117` | `TSW • ASMA-8117` | yes |
| `wks_5ec1eb84e5d2782c` TSW | `TSW • KBI-8118` | `TSW • ASMA-8118` | yes |
| `wks_a177fd8f877b54d5` TSW | `TSW • KBI-8119` | `TSW • ASMA-8119` | yes |
| `wks_240446460981b853` TSW | `TSW • KBI-8120` | `TSW • ASMA-8120` | yes |
| `6301c2d4-…` seat LSA | `LSA` | `LSA` | no |
| `9f98a668-…` seat TPM | `TPM` | `TPM` | no |
| `6aabb528-…` seat SA | `SA` | `SA` | no |
| `6426ec59-…` seat SWE | `SWE` | `SWE` | no |

Every desired name is an exact confirmed Jira key; none is inferred from a
title. The seat carrying this implement turn (`6426ec59-…`) is `unchanged`.

## Step 5 — canary apply REFUSED, no effect

`team_definition_upgrade_apply` for ASMA-8049 against Operational v6, with the
preview hash above and idempotency key `asma-8120-td-upgrade-asma-8049-v6`,
returned:

```text
409 placement_blocked
rule: native topology work is in progress; retirement or migration must wait
action: resolve where the work belongs in the topology, then retry
```

Non-effect verified by re-preview: ASMA-8049's pin is still Operational v3
(`3818bade075891fe…`), all seven renameable targets still read `KBI-`, and the
preview hash is unchanged. Database integrity and foreign-key checks stayed
clean across the refusal.

The live native work the rule names includes this implement AgentRun itself:
`lifecycle: running`, `observed: running`, `attached: true`, bound to native
`6426ec59-57d0-4001-922c-86fc0f2de258` — a seat inside the topology being
migrated. The canary epic contains the seat performing its own migration. This
is recorded as OQ-8120-03 and is **OPEN**.

## Steps 6-7 — not attempted

The record fences the fleet behind the canary, so the twelve remaining eligible
epics were neither previewed nor applied, and the daemon was not restarted.

| Target | Epics | Disposition |
|---|---|---|
| Operational v4 | ASMA-8109 | not attempted — blocked behind fenced canary |
| Operational v5 | ASMA-7869, ASMA-8108, ASMA-8111 | not attempted — blocked behind fenced canary |
| Operational v6 | ASMA-8049 (canary), ASMA-8101, ASMA-8113, ASMA-8155, ASMA-8186, ASMA-8188, ASMA-8190, ASMA-8208 | canary fenced; remainder not attempted |
| Recovery v3 | ASMA-8098 | not attempted — blocked behind fenced canary |

Mutation-ineligible under OQ-8120-02 option (b), untouched and not previewed:
QNR v2 Nonprod Delivery; Kontor Operator Surface and Attention Channel; Catalog
workspace without gitlinks; Repair stale-native Core Team seat succession.

## Source regression repaired in this turn

`77148a4c` — `fix(kontor): Declare the worktree repair's legal target (ASMA-8120)`.

PR #241 (`267c67a7`, ASMA-8120) legalized `CorrectTaskWorktree` against a task
in the command matrix without declaring the pair in the `domain_state`
expectation table, leaving `kontor-core` red from that commit onward, including
on `origin/master` `95bcf868`. The production rule was correct and untouched;
the independent declaration table gained the missing row plus a focused guard.

```text
cargo test --locked -p kontor-core --test domain_state --no-fail-fast
# 38 passed; 0 failed
cargo test --locked -p kontor-core --no-fail-fast
# 17 targets, 324 passed, 0 failed
```

The guard was proven non-vacuous against a semantic mutant: retargeting the
command from Task to Project fails both the guard and the table test; reverting
restores green. Deleting the production arm is a compile error rather than a
behavioural mutant, because the matrix match is exhaustive.

## Fences held

No new topology, run, workspace, worktree, or native identity was created. No
key was inferred. No archived or retired node was touched. No run or seat was
cancelled, parked or retired to clear the canary refusal, no alternate canary
was chosen, and the refusal was not retried past. No merge, deploy or daemon
restart was performed. The retained binary and immutable predecessor
definitions remain the rollback route; no database image was written over live
state.

---

# Resume turn — 2026-09-20, watchdog 23:30Z pass

Resume key `watchdog-4bbd577d-8049-8120-implement-resume-20260919T2337Z-v1`
under standing mandate `watchdog-1f9cedad-standing-authority-20260919`
(receipt `01a0b9a6-d29b-7bb0-85b8-6bdcb2b8f045`).

## Canary consumed and independently verified

OQ-8120-03 is closed. The canary was applied by the ASMA-8049 LSA seat
`6301c2d4` (turn-32, 2026-09-19T23:20:49Z), realizing option (a) of that entry.
Receipts supplied by the watchdog: apply `01a0bbf1-8f97-7012-a5a4-9bbf56ecf5c3`,
original key `asma-8120-td-upgrade-asma-8049-v6`, preview hash
`aa2c7dd1…`, post-state hash
`c83dedbff998308bd660f45c180e09875c4743ae6065e057b6033a7fde9b258f`, OQ success
revision `01a0bbf7-25b5-7423-aad8-30e29f138ad8` (approval
`01a0bbf7-54dc-7d73-bc3d-ad923fa330de`).

Verified against live state rather than taken on report:

- `team_definition_upgrade_preview` for ASMA-8049 → v6 now refuses
  `400 invalid_request`, "the epic already pins that Team Definition revision".
- `native_names_preview` returns 11 targets, **0** still reading `KBI-`, **0**
  pending change.
- Every `native_id` is identical to the pre-apply preview recorded above
  (`prj_c393c717…`, `wks_dbba8b96…`, the five TSW workspaces, the four seats),
  so identity was preserved exactly.

The canary was not re-applied or re-published.

## Remaining twelve — read-only preview sweep

| Epic | Target | Preview | Detail |
|---|---|---|---|
| ASMA-8109 | Op v4 | OK | 13 targets, 5 change, `972dc9bb…` (legacy scheme `KGVCASWR-`) |
| ASMA-8113 | Op v6 | OK | 0 targets, pin move only, `81167d67…` |
| ASMA-8155 | Op v6 | OK | 0 targets, pin move only, `db70de90…` |
| ASMA-8208 | Op v6 | OK | 0 targets, pin move only, `bf6daf88…` |
| ASMA-8186 | Op v6 | OK | 5 targets, 2 change, `fc0bb47d…` |
| ASMA-8188 | Op v6 | OK | 3 targets, 3 change, `a9c75eb2…` |
| ASMA-8190 | Op v6 | OK | 30 targets, 17 change, `510f0cb8…` |
| ASMA-7869 | Op v5 | **REFUSED** | `409 placement_blocked` — "the consultation has no durably recorded subject to name" |
| ASMA-8108 | Op v5 | **REFUSED** | `409 placement_blocked` — same rule |
| ASMA-8111 | Op v5 | **REFUSED** | `409 stale_binding` — "the binding no longer names a session this runtime will act on" |
| ASMA-8101 | Op v6 | **REFUSED** | `409 stale_binding` — same rule |
| ASMA-8098 | Rec v3 | **REFUSED** | `409 stale_binding` — same rule |

The five preview refusals are typed, deterministic and wrote nothing. Whether
they fence only themselves or the fleet is [OQ-8120-04](OPEN-QUESTIONS.md),
recorded before acting.

## Apply attempts — both refused, both inert

Two of the seven clean epics were attempted. Neither produced any effect.

| Epic | Attempt | Result |
|---|---|---|
| ASMA-8109 → v4 | fresh preview `972dc9bb…`, key `asma-8120-td-upgrade-asma-8109-v4` | `409 placement_blocked` — native topology work is in progress |
| ASMA-8113 → v6 | fresh preview `81167d67…`, key `asma-8120-td-upgrade-asma-8113-v6` | `409 revision_conflict`, then `409 placement_blocked` on the fresh-preview retry the record prescribes for a revision conflict |

Non-effect verified: ASMA-8109 still pins v1 and ASMA-8113 still pins v3, both
preview hashes unchanged, ASMA-8049's migration intact and converged, and
`integrity_check = ok` / 0 foreign-key violations / schema 108 throughout.

`capacity_get` reports **11 active TeamRuns** in the project. The refusal the
applies hit is the same rule that blocked the canary from this seat, and the
canary only succeeded once it was applied by another seat while this AgentRun
was idle. Attempts stopped there rather than retrying typed refusals.

## Next owner and exact prerequisite

The remaining eleven epics are **not** this seat's to apply from inside a live
turn.

- **Owner:** the ASMA-8049 LSA seat `6301c2d4`, the caller that demonstrably
  holds this capability and applied the canary under the same rule.
- **Prerequisite:** a window with no in-progress native topology work for the
  target epic. The only observed successful apply happened while this implement
  AgentRun `01a0bb74-ee78` was idle and the project was quiescent enough for the
  placement check to pass.
- **Inputs ready to reuse:** the seven preview hashes above and the idempotency
  keys `asma-8120-td-upgrade-<epic>-v<version>`. A revision conflict takes a
  fresh preview; the hash is deterministic and has reproduced identically across
  every call.
- **Separately owned repairs, not startable here:** ASMA-7869 and ASMA-8108
  need their legacy ASW/CSW consultation subjects durably recorded before
  `--legacy-topics` could be supplied — inventing a topic is exactly what the
  record forbids. ASMA-8111, ASMA-8101 and ASMA-8098 need their stale seat
  bindings repaired through the supported recovery route.

## Fences held on resume

No re-apply or re-publish of the canary. No typed refusal retried unchanged
beyond the single fresh-preview retry the record prescribes for a revision
conflict. No run, seat or topology cancelled, parked, retired or created to
clear a placement check. No restart, no new identity, no duplicate topology. No
legacy consultation topic inferred. No Jira key inferred.
