# ASMA-8120 open-question ledger

Date: 2026-09-19
Task: Jira `ASMA-8120` / Kontor `01a07722-c3ed-7a63-94e6-cefd22e438ab`

This ledger records unresolved state before any rollout choice depends on it.
An open entry is a stop condition for its affected operation, not permission to
pick one of the listed options.

## OQ-8120-01 — post-deploy attachment convergence

- **Subject:** whether the deployed exact-binding rehydration fix has converged
  for the running ASMA-8120 scope seat.
- **Attaches to:** deployment bundle
  `ASMA-8120-20260919T205522Z-95bcf86`, merged PR #243, TeamRun
  `01a0bb61-b8e0-7bf0-a14f-b44713ca10d7`, scope AgentRun
  `01a0bb61-b8e0-7bf0-a14f-b450a46dafdc`, and the ASMA-8120 high-scope
  record.
- **Why the state is ambiguous:** the current Kontor projection reports the
  scope AgentRun attached and running, and a fresh ASMA-8049 native-name
  preview read both current seats. The daemon startup log nevertheless records
  a post-barrier attempt at `2026-09-19T20:56:38Z` where Paseo refused the
  scope workspace because it was not the exact container the runtime had
  prepared. The canonical runtime timeline was unavailable (`503`), so the
  ordering and finality of those observations cannot be proven.
- **Options seen:** (a) runtime history proves the warning was a stale retry
  after a successful exact attachment, so no correction is needed; (b) it is a
  remaining idempotent-replay defect requiring one root-cause fix and exact
  regression test; (c) deployment convergence failed, so rollout stops and the
  retained binary/definition rollback route is used.
- **Disposition:** **RESOLVED 2026-09-20** by approved revision
  `01a0bbc7-8d72-7910-b4dc-accf4e0538a9` / receipt
  `01a0bbc8-0227-7160-bd18-10155b3ea87a`, admitted to the implement turn:
  the later exact-binding challenged settlement is authoritative and the
  earlier `20:56:38Z` refusal remains history. This is option (a). The
  publication fence this entry held is lifted; publication proceeded on
  that authority.

## OQ-8120-02 — active epics without confirmed migration inputs

- **Subject:** completion treatment of four current active epics that do not
  have the confirmed Jira binding and Team Definition pin required by the
  supported upgrade route.
- **Attaches to:** TASK-005 acceptance text, the architecture section
  “Migration scope and failure handling,” and the baseline fleet census in the
  ASMA-8120 high-scope record.
- **Why the state is ambiguous:** the task says to migrate all remaining
  current epics, while the governing architecture limits Jira-key rendering to
  Jira-bound items and says a missing confirmation blocks the affected item.
  The live census contains four active legacy epics in that state: QNR v2
  Nonprod Delivery (no pin/binding), Kontor Operator Surface and Attention
  Channel (no pin/binding), Catalog workspace without gitlinks (no
  pin/binding), and Repair stale-native Core Team seat succession (Operational
  v3 pin, no Jira binding).
- **Options seen:** (a) first repair each binding/pin through a separately
  authorized supported workflow, then include it; (b) fence the four entries
  and define this rollout complete over the thirteen currently eligible
  confirmed/pinned epics; (c) classify individual entries as historical and
  explicitly remove them from “current” scope.
- **Disposition:** **RESOLVED 2026-09-20** by approved revision
  `01a0bbc7-afc9-7561-9551-33d1eeb9345a` / receipt
  `01a0bbc8-75d2-7c01-9ff2-227255a6f171`, admitted to the implement turn:
  option (b). Implement only the thirteen eligible confirmed/pinned epics;
  the four legacy epics stay mutation-ineligible pending their own supported
  repair or classification. No key is inferred and no replacement topology is
  created for them.

## OQ-8120-03 — the canary epic contains the seat migrating it

- **Subject:** how ASMA-8049 is migrated when the supported upgrade route
  refuses while that same epic holds live native topology work.
- **Attaches to:** the high-scope record's ordered rollout steps 4-6, canary
  epic ASMA-8049 (`01a0539a-51c9-7301-9bd7-26c09167b23e`), implement AgentRun
  `01a0bb74-ee78-7aa3-b6bc-bd9c43733496` (native
  `6426ec59-57d0-4001-922c-86fc0f2de258`), and TeamRun
  `01a0bb61-b8e0-7bf0-a14f-b44713ca10d7`.
- **Why the state is ambiguous:** `team_definition_upgrade_apply` for ASMA-8049
  against Operational v6 refused `409 placement_blocked`, rule "native topology
  work is in progress; retirement or migration must wait". The preview itself
  succeeds and is deterministic (hash
  `aa2c7dd13bd09671078e5906f3caa2890a9d8fec020941c6860001b0f04b9413`, eleven
  targets, seven changing), and the refusal left no partial effect: the pin is
  still Operational v3 and all seven renameable targets still read `KBI-`. The
  live work the rule names includes this implement AgentRun itself, which is
  `lifecycle: running`, `observed: running` and attached to native
  `6426ec59-...`, a seat inside the very topology being migrated. The record
  pins ASMA-8049 as the canary and fences the whole fleet behind it, so the
  rollout cannot route around this from inside the run it blocks on.
- **Options seen:** (a) the refusal counts this turn's own live seat, so the
  canary is applied by an authorized caller after this turn settles, reusing
  the same preview and idempotency key; (b) some other in-flight native
  operation is responsible and clears on its own — nothing in the inspected
  ASMA-8049 topology showed a transitional node or seat, so this is
  unevidenced; (c) ASMA-8049 must be migrated from outside itself or ordered
  last, which changes the record's pinned canary choice and needs the scope
  owner.
- **Disposition:** **RESOLVED 2026-09-20**, option (a). The ASMA-8049 LSA seat
  `6301c2d4` applied the canary at 2026-09-19T23:20:49Z while this implement
  AgentRun was idle, reusing the same preview hash and idempotency key: apply
  receipt `01a0bbf1-8f97-7012-a5a4-9bbf56ecf5c3`, durable success revision
  `01a0bbf7-25b5-7423-aad8-30e29f138ad8` (approval
  `01a0bbf7-54dc-7d73-bc3d-ad923fa330de`). Independently verified from live
  state: the epic now refuses a v6 preview as already pinned, all eleven
  targets read `ASMA-*` with zero pending change, and every native id is
  unchanged from the pre-apply preview.

## OQ-8120-04 — does a preview-time typed refusal fence one epic or the fleet?

- **Subject:** whether the five epics whose `team_definition_upgrade_preview`
  refuses stop only themselves, or stop all later rollout including the seven
  that preview clean.
- **Attaches to:** the high-scope record's ordered rollout step 6 and its stop
  conditions, the baseline fleet census paragraph on unbound active nodes, and
  the watchdog resume mandate
  `watchdog-4bbd577d-8049-8120-implement-resume-20260919T2337Z-v1`.
- **Why the state is ambiguous:** the record says both. The census paragraph
  says "the supported preview decides whether their containing epic can proceed
  and any typed refusal fences that exact epic" — one epic. The step-6 stop
  conditions say an "ambiguous legacy consultation topic" (exactly the
  ASMA-7869 and ASMA-8108 refusal) "stops that epic **and all later rollout**"
  — the fleet. The step-6 sentence sits in a paragraph otherwise about
  apply-time failure handling (revision conflict, transport timeout, partial
  effect, readback mismatch), where stopping everything protects against
  *uncertain* state. These five refusals are different in kind: they are
  preview-time, typed, deterministic, and wrote nothing, so nothing about them
  is uncertain.
- **Options seen:** (a) preview-time refusals fence their own epic, so the
  seven clean epics proceed and the five are returned to their owners; (b) the
  step-6 sentence governs literally and the whole fleet stops at zero of twelve
  until the five are repaired; (c) the scope owner re-scopes step 6 to an
  explicit eligible subset.
- **Disposition:** **OPEN, proceeded under option (a)** and recorded here
  before acting, per the standing instruction to log an assumption that cannot
  be evidenced. Rationale: the refusals are preview-time with no write
  attempted, the census paragraph addresses exactly that case, the canary has
  since proven the route end-to-end with exact identity preservation, and the
  watchdog resume mandate directs continuation of the migration. If the scope
  owner intends option (b), the seven migrations recorded in the high-change
  record are the exact set to review; each is an identity-preserving retitle
  with before/after native ids, not a topology change.

## OQ-8120-05 — the apply window the remaining epics need

- **Subject:** what must be quiescent, and which caller must act, for
  `team_definition_upgrade_apply` to pass its placement check on the eleven
  epics still unmigrated.
- **Attaches to:** high-scope rollout step 6, the resume section of the
  high-change record, and watchdog resume
  `watchdog-4bbd577d-8049-8120-implement-resume-20260919T2337Z-v1`.
- **Why the state is ambiguous:** seven of the twelve remaining epics preview
  cleanly, but both applies attempted from this live turn refused — ASMA-8109
  with `placement_blocked` ("native topology work is in progress") and
  ASMA-8113 with `revision_conflict` and then `placement_blocked` on the
  fresh preview the record prescribes. Neither produced any effect. The project
  reports eleven active TeamRuns. The one apply that has ever succeeded, the
  canary, was performed by another seat while this AgentRun was idle. Whether
  the placement check is project-scoped or epic-scoped is not established: the
  two epics failed with different codes on first attempt, which argues against a
  single uniform project-wide gate, but nothing read here proves the scope
  either way.
- **Options seen:** (a) the check counts this live implement turn, so the
  remaining epics are applied by the LSA seat once this AgentRun is idle,
  exactly as the canary was; (b) the check is per-epic and each of the eleven
  has its own in-flight native work to wait out, making this a scheduling
  problem across the fleet rather than one window; (c) the eleven are applied by
  a dedicated quiescent-window caller the watchdog schedules, with the seven
  ready preview hashes and keys reused.
- **Disposition:** **OPEN.** No further apply was attempted. The seven ready
  previews and their idempotency keys are recorded for exact reuse. This seat
  did not cancel, park or retire any run or seat to clear the check, and did not
  retry a typed refusal unchanged.
