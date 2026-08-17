# KON-OP-06 architecture handoff

Date: 2026-08-17  
Status: approved for implementation  
Scope: versioned epic Completion Profiles, deterministic completion runs and
bounded remediation/closeout

## Decision

Compose OP-06 behind the `ApplicationOperations` boundary fixed by OP-03. A
Completion Profile is a closed, versioned recipe that compiles into the
existing task/dependency, TeamRun, gate, Committee and evidence machinery. It
is not a workflow language, a second scheduler or a new topology model.

```text
authenticated OP-03 /v1 operation
                |
                v
      kontor-daemon Services
      |         |          |
      v         v          v
kontor-      kontor-    kontor-teams /
scheduler    policy     existing effects
compiled     gates      TeamRun, Committee,
DAG/state    evidence   connector, seat wake
      \         |          /
       \        v         /
        durable completion aggregate
          + command/effect receipts
```

`kontor-scheduler` owns deterministic compilation and selection of the next
enabled stage. `kontor-policy` owns the evidence predicates, role authority,
round ceiling and terminal-state rules. `kontor-store` persists published
profile revisions, the epic pin, completion state, immutable rounds, evidence
references and receipts. `kontor-daemon::Services` authenticates the caller,
loads one revision, invokes one bounded transition and dispatches only typed
effects through existing ports.

No completion fact may be authoritative only in a `Mutex`, process-local map,
runtime transcript or chat message. A restart loads the same pinned profile,
compiled hash, round lineage and effect receipts, then resumes the missing
suffix. It never creates another TeamRun, CommitteeRun, LSA/TPM seat or
closeout effect because an acknowledgement was lost.

## Verified baseline

OP-01 supplies the data and persistence vocabulary OP-06 consumes:

- immutable published specifications and epic-pinned revision/hash snapshots;
- tracker-neutral MiniProjects, Tasks, dependency edges, gates and evidence;
- TeamTemplates, TeamRuns, role slots and durable command receipts;
- generic topology nodes and typed SeatBindings without completion-specific
  topology or phase rules.

OP-02 supplies the only accepted placement and seat-reconciliation path:

- topology-node identity plus exact native binding/readback;
- TeamRun placement in the task's TSW;
- stable ECP SeatBindings for epic roles;
- `placement_blocked`, attachment, activity and orphan evidence before launch;
- no name, branch or `cwd` inference and no fallback project creation.

OP-03 supplies the public contract and composition boundary:

- Completion profile list/preview/apply operations;
- epic completion read/advance/remediate operations;
- closed `/v1`, OpenAPI, registry, CLI/MCP, authority, expected-revision,
  idempotency and receipt conventions;
- typed `Unavailable` stubs in `Services` for all six operations.

OP-04 supplies the concrete epic control plane consumed through ports:

- one frozen MiniProject/ESW and one ECP;
- distinct persistent LSA and TPM SeatBindings in that ECP;
- frozen Core Team, topology, role-catalog and configuration references;
- exact-seat reconciliation and durable handoff patterns.

OP-05 supplies the consultation evidence consumed through ports:

- immutable Committee template/run references;
- `independent_review@1` with independent findings and a typed aggregate;
- append-only rounds, dissent and evidence references;
- at most one causally linked re-review and `NEEDS_HUMAN` on exhaustion;
- compatible CSW/seat reuse without duplicate consultation topology.

OP-06 must compile and test against those contracts, not import OP-04
promotion or OP-05 consultation implementation modules. OP-08 performs the
final real-service assembly through the same ports.

## Completion Profile

### Closed profile shape

An `EpicCompletionProfile` revision contains only typed references and bounded
policy:

1. ticket-entry requirements: every declared task goal, required artifact and
   gate that must be evidenced before integration;
2. an optional integration stage naming an existing TeamTemplate, required
   checks and typed integration outcomes;
3. an optional final-verdict stage naming an existing CommitteeTemplate and
   its pass/fail outcome mapping;
4. a remediation policy naming the LSA proposal authority, TPM routing
   authority and maximum additional rounds;
5. the fixed closeout requirements: merge, release, delivered-version
   inventory, final summary, notification outcome and archive disposition;
6. callback policy and, only where callbacks are unavailable, a finite polling
   fallback with an explicit attempt bound.

The profile does not contain executable code, arbitrary expressions, shell
commands, native topology data, concurrency ceilings or caller-authored role
names. Team, Committee, gate and evidence references resolve against exact
published revisions during preview/apply. Published Completion Profiles are
`project_shared` configuration; internal run state and receipts remain
`kontor_local`.

The built-in `operational_default@1` is exactly:

```text
all ticket goals/artifacts/gates pass
  -> Team C integration and required integration evidence
  -> independent_review@1 round 1
       fail -> LSA proposal -> TPM routes one remediation round
            -> integration recheck -> independent_review@1 round 2
       pass -> closeout
  -> merge -> release -> versions -> summary -> notification -> archive
  -> done
```

It permits one remediation round, therefore no more than two Committee rounds.
It includes Team C and the final Committee. A different published profile may
omit either only through an explicit typed option whose remaining evidence
requirements still prove the epic goals.

### Deterministic compilation

Preview and apply compile the canonical profile into an acyclic conditional
DAG using stable stage keys and the ordinary scheduler conditions `success`,
`pass` and `fail`. The remediation edge is statically expanded up to the
declared finite bound; it is not a runtime back-edge. Validation refuses:

- an unresolved or non-current TeamTemplate, CommitteeTemplate, gate or
  evidence reference;
- duplicate stage keys or unreachable stages;
- a success path that bypasses required ticket or closeout evidence;
- a failure path with no terminal `NEEDS_HUMAN` outcome;
- a remediation budget that is absent, negative or unbounded;
- a polling fallback with no attempt limit;
- a callback/polling policy that could emit two wake paths for one observation.

The preview hash covers the canonical typed profile, resolved immutable
references and compiled DAG. Apply recompiles at the named project revision,
compares the hash and publishes version one or the exact next immutable
revision. It creates no CompletionRun, TeamRun, CommitteeRun or seat.

An epic freezes the exact profile id/version/hash when it is applied or
explicitly upgraded. Later project publication cannot alter that epic or its
already compiled completion graph.

## Durable completion state

Persist one completion aggregate per epic with:

- the pinned profile reference and compiled hash;
- current phase, stage and aggregate revision;
- stable ordered blockers and the evidence references satisfying each gate;
- immutable integration outcomes for every affected repository/module,
  including PR or revision and the root submodule-pointer outcome when one is
  required;
- immutable Committee round ids, findings, aggregate verdicts and dissent;
- failed-round delivery receipt to the exact LSA SeatBinding;
- immutable remediation proposal, approval and TPM routing receipts;
- merge, release, version-inventory, summary, notification and archive
  receipts;
- handled observation ids, requested effect ids and exact TPM wake receipts;
- terminal `done` or typed `NEEDS_HUMAN` payload.

Every transition is pure over a loaded snapshot plus one stored observation.
The application transaction first records the new state and planned stable
effect identities, then dispatches effects. Replaying the idempotency key and
canonical intent returns the original outcome. A different intent conflicts;
a stale revision has no effect. Lost acknowledgement reconciles the planned id
and continues only the missing effect suffix.

## Gate and evidence semantics

### Ticket and integration entry

Completion cannot start Team C while any declared task goal, required artifact
or task gate is absent or failing. A task lifecycle value such as `done` is not
substitute evidence. Blockers are reported in stable task/goal/artifact order.
Duplicate or undeclared evidence does not satisfy a requirement.

Team C is an ordinary delivery TeamRun in its own TSW, admitted by the existing
scheduler and capacity policy. Its evidence records the configured PR,
code-quality, functional, end-to-end and concept checks. Polyrepo integration
is a collection of typed module/repository outcomes plus the root pointer where
applicable; completion never assumes one repository, one branch or one commit.

### Final Committee

The final gate consumes a settled OP-05 Committee round by immutable id. Only a
typed passing aggregate with its required evidence enables closeout. A missing,
incomplete, inconclusive or failed aggregate cannot be coerced into a pass by
`completion:advance`.

A failed round is appended unchanged and its complete findings, evidence and
dissent are delivered to the exact epic LSA SeatBinding. Delivery itself has a
receipt and is replay-safe. Completion then waits for an LSA-authored
remediation proposal; it neither edits the failed round nor guesses corrective
work.

### LSA proposal and TPM routing

The LSA proposal names the failed round, affected goals/tasks, the bounded
correction, required new or reopened Tasks and supporting evidence. The daemon
derives and verifies the caller's exact epic LSA SeatBinding. An absent,
foreign, stale or non-LSA seat cannot propose or approve remediation.

Once the proposal is approved under the pinned policy, the same epic's exact
TPM SeatBinding may record the next-round route: the approved task set,
dependencies, TeamTemplate selections and target Committee round. The TPM may
route the approved correction but cannot silently change its technical scope.
No remediation TeamRun or second Committee round launches before both receipts
are durable.

The next round waits for its remediation tasks and integration recheck to
satisfy the same declared gates. It appends new evidence and reuses compatible
OP-05 CSW/SeatBindings after exact reconciliation. Round one remains readable
and immutable. Under `operational_default@1`, a second failure or an attempt to
open a third Committee round enters `NEEDS_HUMAN`; it never reopens the loop.

### Closeout

`done` is a conjunction, not a Committee synonym. It requires all of:

1. every ticket and epic goal/evidence requirement;
2. confirmed integration evidence and a passing final aggregate verdict;
3. merge receipt(s) for every logical integration target;
4. release receipt(s), or an explicit typed not-applicable disposition allowed
   by the pinned profile;
5. a complete module/service version or revision inventory;
6. a final goal/evidence/change summary;
7. a notification outcome recording delivered, partially delivered or failed;
8. an archive/idle/retain disposition for every completion-created TeamRun and
   consultation resource.

The closeout state stores receipt ids and referenced authoritative records, not
caller booleans. Merge, release and notification automation may use a native
`kontord` connector. Otherwise an authorized operator records a typed external
receipt. No automated completion path invokes `asma git`, `asma acp`, `asma
jira`, another ASMA command or a raw runtime tool.

Archive is the last closeout prerequisite. It uses existing exact-node and
exact-seat retirement/archive operations and their readback. Completion never
claims archive success merely because a native session stopped.

## `NEEDS_HUMAN`

Completion uses the existing attention state; it adds no Escalation aggregate,
interactive prompt channel or second notification transport. Every transition
to `NEEDS_HUMAN` is invalid unless it carries:

- a concrete recommended resolution and its author; and
- an ordered tried-deliberation path naming every role, Advisor/Committee
  consultation, failed round and remediation attempt already used.

The state is reached when the bounded remediation is exhausted, required
authority is missing after reconciliation, a gate/evidence dependency cannot
be satisfied, a consultation is inconclusive, a callback/polling fallback is
exhausted, or another non-self-clearing refusal cannot be resolved. It is not
used to ask for permission to perform an already authorized reversible step.
Known self-clearing capacity exhaustion continues to park until reset unless
the declared deadline/horizon makes human action necessary.

The payload is validated on construction and restore so an old or corrupt row
cannot project an incomplete human request.

## Waking the idle epic TPM

Completion and attention observations address the existing TPM SeatBinding in
the epic ECP. They never create a TPM seat, workspace or long-running watcher.
For each observation requiring coordination, the transaction appends one
outbox wake intent keyed by `(epic_id, completion_revision, reason,
seat_binding_id)`.

The dispatcher:

1. resolves the frozen TPM role slot and exact SeatBinding;
2. verifies the ECP/native identity and reconciles that same seat;
3. refuses an orphaned, replaced, active-conflicting or mismatched binding;
4. sends one bounded role turn carrying the new completion projection;
5. records the callback/runtime acknowledgement and leaves the seat idle when
   the turn settles.

Duplicate observations or callback delivery replay the same wake receipt. They
cannot create a second turn for the same completion revision. Callback is the
default. Where the runtime cannot callback, a declared scheduled fallback may
issue only its persisted finite attempts; there is no `sleep` loop and no TPM
turn kept alive for polling. Exhaustion uses the required `NEEDS_HUMAN`
payload.

## Successor contracts that gain behavior

Every path below remains under `/v1/projects/{project_id}` with the authority,
handler, OpenAPI operation and `ToolSpec` fixed by OP-03.

| Contract | Behavior supplied by OP-06 |
| --- | --- |
| `GET /completion-profiles` | Return published immutable Completion Profile revisions in stable order, including `operational_default@1`, with aggregate revision and snapshot cursor. |
| `POST /completion-profiles:preview` | Parse a closed profile definition, resolve exact referenced revisions, validate the finite graph and evidence coverage, compile deterministically and return violations plus `preview_hash`; write nothing. |
| `POST /completion-profiles:apply` | Recompile and revalidate the named preview under expected project revision, publish the exact next immutable version and return one receipt. It starts no completion work. |
| `GET /epics/{epic_id}/completion` | Return the pinned profile/hash, typed phase/stages, ordered blockers, integration and round lineage, closeout evidence, attention state, wake status, revision and cursor. It performs no observation or effect. |
| `POST /epics/{epic_id}/completion:advance` | Reconcile already recorded task, TeamRun, Committee and closeout observations; commit one deterministic transition and stable effect intents; dispatch/reconcile only the enabled existing work; return the durable state and receipt. |
| `POST /epics/{epic_id}/completion:remediate` | Record either the exact LSA proposal/approval or the exact TPM next-round route through a closed tagged request, enforce authority and the pinned round ceiling, and return the durable state and receipt. |

The current `Services::{completion_profiles, preview_completion_profile,
apply_completion_profile, completion, advance_completion,
remediate_completion}` methods all return `ApiErrorCode::Unavailable`. OP-06
replaces those stubs only when the relevant repository-backed service and
ports are composed. A partially wired method must keep returning `Unavailable`;
it must not return an empty profile catalog or synthetic success. OP-08 wires
the real OP-04/OP-05 adapters without changing these contracts.

### Narrow OP-03 DTO corrections

Keep the routes and envelopes, but close the bodies/projections that cannot
represent the required semantics:

- decode `ProfilePreviewRequest.definition` and
  `ProfileApplyRequest.definition` as a strict `EpicCompletionProfileSpec`,
  rejecting unknown fields before hashing;
- replace `CompletionStateDto.phase: String` and
  `outstanding: Vec<String>` with typed phase, stage, blocker, round,
  closeout-receipt, wake and `NEEDS_HUMAN` projections;
- keep `AdvanceCompletionRequest` evidence-free: it names only expected
  revision because the daemon reads already authoritative observations;
- replace `RemediateCompletionRequest.reason` with a closed tagged LSA-proposal
  or TPM-route action referencing existing task/round/evidence ids;
- retain `CompletionOutcomeDto { state, receipt }` and the existing
  idempotency/expected-revision rules.

Regenerate OpenAPI, registry and generated clients with those corrections in
the implementation change. Do not add routes, accept arbitrary JSON or let a
caller submit native ids, verdicts, evidence bodies or seat authority.

## Topology and phase implications

- Completion is epic-local state attached to the existing MiniProject. It is
  not a topology node and creates no Completion workspace.
- Team C and remediation delivery are ordinary Tasks/TeamRuns placed in TSWs
  through OP-02. They count once each against mission capacity while active.
- The final Committee remains an OP-05 CommitteeRun in one CSW. Completion
  consumes its immutable verdict; it does not own consultation placement.
- LSA and TPM remain distinct persistent SeatBindings in the existing ECP.
  Completion wakes/reuses them; it never creates role workspaces or successors.
- Persistent idle LSA/TPM/Committee seats do not count as active TeamRuns.
- The completion phases are `ticket_gate`, `integration`, `verdict`,
  `awaiting_lsa`, `remediation`, `closeout`, `done` and `needs_human`. They are
  completion projection state, not topology kinds or a replacement for Task
  lifecycle.
- `done` and `needs_human` are terminal for that immutable round lineage. A
  human-authorized expansion creates a new explicit revision/round; it never
  resets counters or rewrites prior evidence.
- Project profile publication affects new epics only. Existing epic pins,
  topology, seats and previous completion rounds remain unchanged.

## Composition checkpoints

1. Add the strict Completion Profile schema, deterministic compiler,
   `operational_default@1`, catalog/preview/apply persistence and contract
   corrections.
2. Add the repository-backed completion aggregate, ticket/integration evidence
   gates and deterministic TeamRun/Committee ports with restart fixtures.
3. Add failed-round LSA delivery, approved proposal, TPM routing, one bounded
   remediation round and mandatory `NEEDS_HUMAN` payload.
4. Add closeout receipts, polyrepo inventory, exact-seat TPM wake/outbox and
   bounded polling fallback, then replace the six `Unavailable` stubs.

Each checkpoint must build and must not enable a success path whose durable
dependencies are still absent.

## Required proofs

- identical profile input and immutable references compile to byte-identical
  DAGs; invalid/unbounded graphs refuse before publication;
- no integration run starts until every declared task goal/artifact/gate has
  evidence;
- one polyrepo fixture records module PR/revision outcomes plus the required
  root pointer and never assumes one branch;
- a failed Committee round cannot reach closeout and its full immutable
  evidence reaches the exact LSA seat;
- only an authorized LSA proposal followed by the exact TPM route launches
  remediation; either receipt missing blocks;
- round one never changes, round two appends, and a second failure/third-round
  attempt under `operational_default@1` reaches valid `NEEDS_HUMAN`;
- a passing verdict without any one merge/release/version/summary/
  notification/archive prerequisite cannot reach `done`;
- duplicate/lost-ack observations create one TeamRun, Committee round,
  closeout effect and TPM wake across restart;
- completion wakes the same idle TPM exactly once and creates no seat,
  workspace, sleep loop or unbounded polling process;
- every stalling path refuses an incomplete `NEEDS_HUMAN` payload;
- completion builds and its deterministic suite passes with `asma` absent and
  no production dependency or command path to ASMA.

## Out of scope

OP-06 does not implement OP-04 promotion/Core Team behavior, OP-05 Advisor or
Committee internals, OP-07 Jira connectors/cutover, OP-08 final cross-feature
CLI/MCP assembly, OP-09 diagnostic UI or OP-10 live disposable proof. It does
not add a workflow editor, generic action/plugin engine, topology kind,
notification transport, second scheduler or automatic Git implementation.
