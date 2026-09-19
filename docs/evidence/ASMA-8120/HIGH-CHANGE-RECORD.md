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
