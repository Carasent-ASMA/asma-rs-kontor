# ASMA-8120 open-question ledger

**Current section: 2026-09-21 evidence-only closeout.** Everything below the
History divider is the earlier ledger, preserved verbatim including dispositions
now superseded. Corrections are made here, not by editing the history.

Authored from AgentRun `01a0bb74-ee78-7aa3-b6bc-bd9c43733496` in the existing
task worktree. This turn recorded no Kontor mutation and settled no question on
its own authority; every disposition below cites an approved record.

## Current standing

| Entry | Standing | Authority |
|---|---|---|
| OQ-8120-01 | **Resolved**, option (a) | rev `01a0bbc7-8d72-7910-b4dc-accf4e0538a9` / receipt `01a0bbc8-0227-7160-bd18-10155b3ea87a` |
| OQ-8120-02 | **Resolved**, option (b) | rev `01a0bbc7-afc9-7561-9551-33d1eeb9345a` / receipt `01a0bbc8-75d2-7c01-9ff2-227255a6f171`; memory `open-question-disposition-oq-8120-02-20260920` |
| OQ-8120-03 | **Resolved**, option (a) | apply receipt `01a0bbf1-8f97-7012-a5a4-9bbf56ecf5c3`; success rev `01a0bbf7-25b5-7423-aad8-30e29f138ad8` |
| OQ-8120-04 | **Resolved** | memory `asma-8120-rollout-dispositions-20260920-direct-repair` |
| OQ-8120-05 | **Open** — source cause identified, root-owned repair in progress | same memory record |
| OQ-8120-06 | **Open** — restart proof for the full 13 | this section |
| OQ-8120-07 | **Open** — deployment launch-count anomaly | this section |
| OQ-8120-08 | **Open** — unfinished NULL-subject consultation | this section |

## OQ-8120-04 — resolved, and an earlier statement of it corrected

Approved disposition: "A deterministic pre-effect preview refusal fences its
exact epic. Uncertain apply/readback effects halt rollout globally until
reconciled."

This confirms the reading recorded before acting. **Correction:** the earlier
entry's closing sentence referred to "the seven migrations recorded in the
high-change record". No migration existed at that point — there were seven clean
**previews** and zero successful applies from this seat, and both attempted
applies were inert. The count in that entry and in the earlier high-change
section ("remaining eleven") was also wrong: **twelve** remained, not eleven.
Both are corrected in the current high-change section. All 13 are now migrated.

## OQ-8120-05 — open; this seat's hypothesis was wrong

Approved finding: "Services owns one `native_lifecycle_guard` RwLock across the
fleet. `native_lifecycle_change` uses `try_write`; `native_activity` uses
`try_read`. The refusal counts in-flight control-plane native operations, **not**
the model's running/idle state and **not** per-epic activity. A caller with
ongoing background readers may starve without a queued writer."
Source: `crates/kontor-daemon/src/applications.rs:955-971` at `0f649824`.

**Correction:** this seat's recorded option (a) — that the check counted its own
live turn — is wrong. The guard is fleet-wide and counts control-plane
operations; run lifecycle is irrelevant to it. Option (b), per-epic in-flight
work, is also wrong.

Disposition: "Root owns bounded lifecycle-guard repair and exact migration
preview/apply; do not cancel, park, replace or archive live roles to clear the
transient guard." Status `source_cause_identified_repair_in_progress`. **OPEN**
until that repair lands.

## OQ-8120-06 — restart persistence proof for all 13 epics

- **Subject:** whether the migrated fleet survives a coordinated restart, for
  the complete set of 13 rather than 12 of them.
- **Attaches to:** deployment `ASMA-8188-20260921T100129Z-44663e10` (schema 118,
  verified 2026-09-21T10:02:36.117472Z) and the ASMA-8111 apply receipt
  `01a0c373-17a8-73e1-84cd-608247b938a4` (pinned 2026-09-21T10:11:34.947969Z).
- **Why the state is ambiguous:** the restart **preceded** ASMA-8111's
  migration by roughly nine minutes, so 12 of 13 epics carry post-migration
  restart evidence and ASMA-8111 carries none. The cursor-4573 read shows all 88
  targets converged, but a converged read is not a restart-persistence proof.
- **Options seen:** (a) root's next coordinated restart plus full readback
  closes it for all 13; (b) a narrower supported restart/readback scoped to
  ASMA-8111 suffices; (c) the existing coverage is accepted with ASMA-8111
  explicitly carved out and recorded.
- **Disposition:** **OPEN.** No restart was performed by this turn and no PASS
  is claimed from partial coverage. Awaiting root's next coordinated
  restart/readback.

## OQ-8120-07 — deployment launch-count anomaly

- **Subject:** why the live deployment observed more launches than the operator
  performed restarts.
- **Attaches to:** `deployment.json` for
  `ASMA-8188-20260921T100129Z-44663e10` — `operator_restart_count: 1`,
  `observed_launch_delta: 2`, `launch_anomaly: "Unexpected automatic launch
  count; see launchctl-after.txt; root-cause review required"`.
- **Why the state is ambiguous:** the deployment is otherwise healthy
  (`quick_check: ok`, 0 foreign-key violations, identities preserved, signatures
  valid), so the extra launch is unexplained rather than obviously harmful. Its
  cause is not established by any evidence read here.
- **Options seen:** (a) a benign launchd re-launch explained by
  `launchctl-after.txt`; (b) an unintended supervisor or watchdog path still
  able to start the daemon while the watchdog is recorded as stopped; (c) a
  deployment-script defect double-starting the service.
- **Disposition:** **OPEN, narrowed 2026-09-21** by
  [`LAUNCH-ANOMALY-ANALYSIS.md`](LAUNCH-ANOMALY-ANALYSIS.md). Established:
  **neither metric is wrong** — `operator_restart_count` is a literal 1 for one
  intended restart, and `observed_launch_delta` correctly reports launchd's own
  `runs` counter moving 3 → 5. A second spawn really occurred. Exactly one spawn
  reached the program: `logging::install()` is the first statement of `main()`,
  so any spawn entering the program logs even on failure, and the daemon log
  holds a single `realm claimed` line with nothing whatsoever between
  10:01:54.858610Z and 10:02:05.638511Z. The extra spawn therefore died at the
  exec/load stage and `KeepAlive{SuccessfulExit=false}` recovered it. Three
  hypotheses were tested under private throwaway launchd labels and **refuted**:
  KeepAlive double-counting, slow graceful shutdown, and in-place binary
  replacement. The shared fleet was never restarted — it still reads
  `runs = 5`, `pid = 39728`. **Still open** on the narrower question: the precise
  reason that one spawn failed to exec is unproven, because the process left no
  output by construction and the launchd system log no longer covers the window.
  No `kontor` source correction is warranted; the weakness is in root-owned
  deploy tooling, and the safe next action is recorded in the analysis.

## OQ-8120-08 — unfinished NULL-subject consultation and pending seats

- **Subject:** how the consultation with no durably recorded subject, and the
  three `rename_pending` ASMA-8188 seats, are closed.
- **Attaches to:** consultation `01a0298c-6284-7a83-8d35-158c9c4e82e4`
  (`400 invalid_request`, subject `ModelRung`, "a value did not satisfy the
  invariant of its type"); the three ASMA-8188 seats
  `e5b2e5d7-fa35-4e14-99cc-49770a0faf08`,
  `9156c0ce-5851-4371-b419-fdcd6569f4d1` and
  `a805e24f-d2fe-4ded-a497-b4b59633d195`; and consultations
  `01a02ba2-d1cd-7fc0-9bdb-1704dd4a544c`,
  `01a02bb3-2614-7711-8a02-896d545a9292` (materializing) and
  `01a02d6e-4db9-7372-b2b8-c8024f41f3e7` (running).
- **Why the state is ambiguous:** these are pending rather than failing. They
  carry no legacy item code and contribute zero pending naming changes, so they
  do not affect the migration result — but they are unfinished, and the refusing
  consultation cannot be read at all through the supported route.
- **Options seen:** (a) let the in-flight consultations finish and the seats
  bind naturally, then re-read; (b) a supported repair records the missing
  subject for `01a0298c`; (c) governed historical classification for the
  refusing consultation if it can never be completed.
- **Disposition:** **OPEN.** No subject was inferred, repaired or settled here,
  and no seat was bound, renamed or retired. Preserved as carried-forward state.

---

# History — preserved verbatim below this line

The following is the earlier ledger exactly as written, including the OQ-8120-04
and OQ-8120-05 text the current section corrects.

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
