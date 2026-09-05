# KON-OP-22 — Open questions

Interim markdown ledger. The durable `open_questions` aggregate still has no
operator write path on `origin/master` — `raise_question` appears only in the
core trait, the store implementation and the store tests, with no MCP tool and
no command in the deployed `kontor` binary — so this file continues to stand in
for it. That gap is OQ-4 and remains open.

## OQ-9 — Legacy consultation topics · RESOLVED 2026-09-05

- **Attaches to:** epic `01a0074f-6719-7570-adf7-95ee3ec69875`
  (`ASMA-7869 · Kontor Operational MVP`, backlog code now `KOP`), project
  `01a0064a-e056-7603-9968-ef64fdaacb75`, realm
  `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`. Named as the next operation by the
  2026-09-04 entry in `OPERATIONAL-GAPS.md`.
- **State read back:** Team Definition `01936f5a-2000-7000-8000-000000000001`
  v2 (`31cdff80e27cbe1e4043e150d2cdbc43ff79a8fa7fd8d892d2b5049a600f2e13`,
  topology v4) is **already published**, and the untracked
  `asma-operational-team-definition-kop-v2.json` matches the published document.
  The epic still pins `team_template {01936f5a-0000-…-0001, v1}` and carries no
  Team Definition. Project revision is 5. The epic holds 21 tasks with
  `scheduling_open: true`, and its pinned topology is 26 nodes — 10 active, all
  bound — carrying **122 seats**.
- **Ambiguity:** `kontor_team_definition_upgrade_preview` refuses closed with
  `invalid_request`: *"legacy_topics must map every legacy consultation that
  lacks a topic and no non-consultation node"*. Five active consultation nodes
  need an explicit topic and **no topic exists to read**:

  | Topology node | Native id | Native title |
  |---|---|---|
  | CSW `01a0298c-6284-7a83-8d35-159b90bd9d87` | `wks_aae60870c1056a3f` | `Committee Session Workspace` |
  | CSW `01a02ba2-d1cd-7fc0-9bdb-1710390cd34a` | `wks_aae2e5c64dd01036` | `Committee Session Workspace` |
  | CSW `01a02bb5-fcf6-7ea0-9849-168a6975c671` | `wks_88b7239acec72548` | `Committee Session Workspace` |
  | CSW `01a02bb3-2614-7711-8a02-8978ab947be8` | `wks_163b779bb853680e` | **absent** from Paseo active workspaces |
  | ASW `01a02d6e-4db9-7372-b2b8-c815da222dc5` | `wks_124e30e7ebcff8f1` | **absent** from Paseo active workspaces |

  Three carry the generic `Committee Session Workspace` title, and the topology
  record holds no `topic` field at all. The topic is therefore not recoverable
  from any authoritative record — it has to be *authored* by someone who knows
  what each committee was convened to decide. Supplying a guess would invent an
  identity token, which is precisely the defect class (4) this ticket removed and
  the same mistake the 2026-09-04 `ECP • KOP-7869` attempt already made once.

  Other workspaces in the same Paseo project do carry readable topics
  (`CSW · ASMA-7869 · Runtime boundary and exit cost`,
  `CSW · ASMA-7869 · Agent Orchestrator feature harvest`,
  `CSW · ASMA-7874 · KON-OP-05 · Release rejection root cause`), but they use the
  `·` separator, are `local_checkout` rather than `worktree`, and are **not**
  among this epic's bound nodes. Mapping them onto the three untitled node ids
  would be inference, not evidence, so it is not proposed here.

- **Second, independent finding:** two of the five nodes are recorded
  `lifecycle: active`, `placement: bound` in Kontor while their native ids are
  absent from Paseo's active workspaces. That is live drift, and it should be
  judged *before* a migration whose whole job is to rename bound nodes from a
  template.
- **Options seen:** (a) the delivery owner supplies the three-to-five legacy
  topics explicitly, from knowledge of what those consultations decided, and the
  two absent nodes are recovered or retired first; (b) retire the legacy
  consultation nodes so no legacy consultation lacking a topic remains, which
  makes `legacy_topics` empty and the refusal moot — this discards those
  consultation identities and needs its own authority; (c) extend the migration
  to accept a recorded absence rather than requiring a topic, which is a Kontor
  change, not an operation.
- **Disposition:** resolved — (a). The operator supplied all five topics
  explicitly on 2026-09-05 and they were passed verbatim, never inferred:
  `01a0298c…` = "Operational MVP closeout"; `01a02ba2…` = "Operational MVP
  closeout late corrections"; `01a02bb3…` = "Operational MVP closeout
  consultation paths"; `01a02bb5…` = "Operational MVP closeout final review";
  ASW `01a02d6e…` = "Codex home provenance". The preview then cleared this
  validation and failed further in, at OQ-11 below.

## OQ-10 — This seat's role for the 2026-09-05 turn was never stated · OPEN

- **Attaches to:** this document.
- **Ambiguity:** the handoff said only that the builder finished and its
  artifacts are recorded. The complete evidence set already contains
  `IMPLEMENTATION.md`, `MUTATION.md`, `QA-REPORT.md`, `REVIEW-NOTES.md` and
  `RELEASE-NOTES.md`, so the verification roles appear discharged, while the one
  named pending operation — the Team Definition migration — is a live mutation
  that would retitle containers and up to 122 seats on an epic that is currently
  scheduling work. A verifier and a delivery owner do not take the same action
  here.
- **Options seen:** (a) operator names the role and, if it is delivery owner,
  authorises the migration window; (b) treat the turn as verification only and
  report; (c) assume delivery authority and execute.
- **Disposition:** proceeding under (b). (c) was not taken: the operation is
  outward-facing, hard to reverse, and blocked on OQ-9 regardless.

## OQ-11 — The migration census refuses on a stale native binding · OPEN · BLOCKING

- **Attaches to:** epic `01a0074f-6719-7570-adf7-95ee3ec69875`; the same five
  legacy consultation nodes as OQ-9.
- **Observed:** with all five operator-supplied topics passed,
  `kontor_team_definition_upgrade_preview` clears the `legacy_topics` rule and
  then refuses `409 stale_binding` — *"the binding no longer names a session
  this runtime will act on"*, action *"settle the run to learn what its runtime
  now reports"*. Nothing was committed; preview commits nothing by contract.
- **Most probable subject, on read-only evidence:** two pinned nodes are
  `lifecycle: active`, `placement: bound` in Kontor while their native ids are
  absent from Paseo's active workspaces, and they carry **four active seats**
  between them:

  | Node | Native id | Active seats |
  |---|---|---|
  | CSW `01a02bb3-2614-7711-8a02-8978ab947be8` | `wks_163b779bb853680e` — absent | 3 (`reviewer-a`, `reviewer-b`, `judge`) |
  | ASW `01a02d6e-4db9-7372-b2b8-c815da222dc5` | `wks_124e30e7ebcff8f1` — absent | 1 (`advisor`) |

  This is not proven to be the exact binding the census tripped on: the refusal
  names no id, and every call that would pin it down —
  `kontor_runtime_settle`, `kontor_topology_drift`, `kontor_seat_attention` —
  either settles a run or records observation evidence, i.e. mutates. Under a
  verification-only mandate none was made.
- **Options seen:** (a) a delivery-authorised seat settles the stale run(s), or
  recovers/retires the two absent nodes, then re-previews; (b) the migration
  census tolerates a provably absent native binding on a node it is about to
  rename, which is a Kontor change rather than an operation; (c) the epic is
  drained of scheduling first, so the census runs against a quiet fleet.
- **Disposition:** none. Verification stops at the mutation boundary.

## OQ-12 — Migrating to v2 renames consultation seats, not just containers · OPEN

- **Attaches to:** Team Definition `01936f5a-2000-7000-8000-000000000001` v2.
- **Observed:** v2's CSW block defines three slots whose seat names render from
  `SLOT_DISPLAY_NAME` — `SEAT A`, `SEAT B`, `JUDGE`. The 11 live active
  consultation seats all carry `role_code: AUD` / `standard_title: Auditor`
  (and the ASW seat `SA` / `Software Architect`), so their current names derive
  from the role code. Migration therefore renames seats as well as containers.
  Two CSW nodes also hold 4 seats each against v2's 3 slots, one retired.
- **Ambiguity:** whether that seat rename is the intended effect of the KOP
  migration or an unreviewed side effect. The preview never reached its rename
  census, so the exact set is unenumerated.
- **Disposition:** none. Flagged for whoever holds the apply.
