---
name: seat-takeover
description: Resume unfinished work or resume after legitimate wait on the same seat; take over by exact native ID after confirmed unusability or explicit user authorization. Preserves work, retires the predecessor, and verifies one successor. Not downstream handoff, first launch, or a recovery scheduler.
---

# Seat takeover

**Resume** continues the same occupant. **Takeover** replaces an unusable
occupant, or a healthy occupant when the user explicitly requests it, with
**one** successor on the same logical seat, scope and phase.

Vocabulary: [Kontor glossary](../../../docs/GLOSSARY.md#roles-seats-and-personas).
Compaction: [autonomous TPM operation](../epic-orchestration/references/autonomous-tpm-operation.md#context-policy).
Contract: [the workflow guide](../../../docs/WORKFLOW.md#3-kickoff-to-closeout).

## Orchestration surface

Resolve the surface with the
[transition rule](../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule).

| Step | Paseo-direct | Kontor-governed |
| --- | --- | --- |
| Identity read | Supported session/seat reads; exact ID preferred | `kontord` seat/run surfaces |
| Resume | Same native ID continues | Supported resume bindings |
| Replacement | Verified archive, one recorded successor; native adoption optional | Revisioned recovery + generation rules |
| Correlation / replay | Procedural checks; label weaker than API idempotency | Durable correlation when qualified |

Distinguish **procedural requirements** from **observed runtime readback**.
Prose alone does not prove retirement, adoption, parent binding or write fences.

## Common takeover evidence fields

At resolve, briefing and readback, retain the
[common evidence envelope](../seat-handoff/references/evidence-envelope.md)
shared with `seat-handoff`; use references/hashes, not copied policy tables.

For ad-hoc or non-Git work, mark absent workflow, repository, branch and `HEAD`
fields **not applicable**; never invent them. An unavailable field required by
the selected workflow is missing evidence, not "not applicable". Keep actual
workspace/cwd and permission provenance; invent no tracker key.

Record observed departures from the brief, including `rename_pending`, a failed
launch leaving the seat unoccupied, or weaker correlation guarantees.

## Lookup order

1. **Exact native session ID**, then verify scope and current occupant.
2. **Exact title** within known epic/workspace via logical seat.
3. **Descriptive search** → candidate list (title, ID, role, scope, state).

Proceed only with **one** verified target. Known predecessor ID resolves to
**recorded successor**, not title recency. Several title matches → report
candidates; **no mutation** until resolved.

## Resume vs replacement

| Situation | Action |
| --- | --- |
| Legitimate wait (dependency, permission, hold, operator pause) | Keep waiting while the condition is active; resume the same occupant after it is resolved and resume is authorized |
| Seat finished, awaiting the next dispatch | Reuse the seat through the owning dispatch procedure ([idle seats are reusable](../epic-orchestration/references/paseo-direct-native-placement.md#idle-seat-reuse)); not takeover |
| Supported same-session recovery / compaction | **Resume**; preserve native identity |
| Confirmed unusable occupant or explicit replacement authority | **Takeover** sequence below |
| Planned brain change (same or different vendor) | See [seat-handoff](../seat-handoff/SKILL.md) (brain-change handoff); same-work replacement after failure or explicit user authorization follows this skill |
| Context pressure alone | Not sufficient for takeover |

Reconcile the same occupant first. Automatic recovery requires an authorized
request and the configured stale predicates; this skill adds no timer or loop.

## Takeover sequence

For **Paseo-direct**, an existing receiving session (including "you take over")
may become the recorded sole task owner after verified predecessor retirement
and checkout/authority checks. Native adoption or reparenting is **optional**;
its absence alone must not block takeover or cause creation of a third session.
Use the [Paseo-direct recovery reference](references/paseo-direct-recovery.md)
for worker preservation, ownership recording and reporting fallback.

For **Kontor-governed** work, keep the selected supported adoption/binding,
generation and ownership contract. Preflight that route before retirement;
the direct fallback does not bypass it or migrate an existing plan's pins.

1. **Resolve and re-read.** Predecessor ID, logical seat, supervisor, generation,
   original active title, scope, obligations, replacement authority, and the
   **common evidence envelope**. Detect completed or in-flight takeover;
   **replay returns the existing result**. Verify identity rather than title recency.
   Record one takeover coordinator; reconcile competing attempts before mutation.
2. **Prepare before retirement.** Validate admissible successor route, instructions,
   permission posture, checkout and recovery surface. Persist **takeover brief**
   including the full common envelope plus: goals/criteria, decisions, phase,
   uncommitted state, evidence, open findings/counters, authorized next action,
   history references and original active title. Follow the
   [first-line rule](../agent-dispatch/SKILL.md#first-line-is-the-exact-seat-name).
   The initial successor prompt permits bootstrap only until readback succeeds.
   Brief generation failure must not erase underlying evidence.
3. **Preflight route and preserve workers.** If preparation fails, keep predecessor
   and evidence intact. Before archive, inspect children and preserve their work
   through the selected supported route. For Paseo-direct supervisor replacement,
   prepare notifications, a subscription or bounded observation; native callback
   transfer is not required. Governed reporting keeps its selected contract.
4. **Retire predecessor.** Use the surface's supported retirement operation and
   verify successful completion plus terminal readback before successor effects.
   Paseo-direct requires archive and no executing predecessor; a failed close
   with `archivedAt` set is insufficient. Renaming, idle status, or a no-write
   promise is **not** retirement. Never two writers on the same task/checkout.
5. **Mark predecessor display.** Under the qualified selected naming policy, set
   its title to `STALE • <original active title>` **once**; do not prefix an already
   marked title again. Retain the original title and old→new identity link. If
   archived sessions cannot be renamed, or retitle fails, record `rename_pending`.
   Renaming never substitutes for verified retirement.
6. **One successor.** Create, adopt or (Paseo-direct) record the existing receiver
   with the same logical seat, scope and workflow position. The successor inherits the
   predecessor's **original active title**, without `STALE • `; preserve the
   selected naming-policy qualification. **Recompute** credentials and permission
   posture from the recorded policy; **never copy** predecessor generation grants.
7. **Read back before task effects (observed).** Provider/model/effort, posture,
   native ID, logical supervisor and observed native parent, project/workspace,
   **absolute cwd**, **absolute checkout**, branch, `HEAD`, applicable artifact
   SHA and expected active title.
   A workspace, checkout, permission or other required mismatch **blocks task
   effects** until corrected and verified. Record drift; bootstrap only until
   checks succeed. On Paseo-direct, a recorded native-tree limitation with a
   working reporting fallback is not a placement mismatch.
8. **Acknowledge and continue.** Successor confirms brief and **observed** repo
   state; record old→new link with procedure revision and policy provenance;
   notify supervisor. Counters, criteria, findings and scope survive.

## Partial failure

| Failure | State |
| --- | --- |
| Route preflight fails | Predecessor remains; evidence intact |
| Retirement succeeds; successor launch fails | Seat **unoccupied**; explicit recovery pending; no silent revival |
| Successor placement or authority mismatches | Block task effects; correct through supported operations and re-read. If correction is unavailable, retire that successor and record explicit recovery pending |
| Runtime effect unknown | Reconcile by identity/correlation before retry |
| Stale-title fails | Visible naming gap; not proof task completed |

Reconcile an existing successor before retry; no second active successor or
concurrent writer. Verify retirement of a mismatched successor before admitting
another through an explicit supported recovery. Store operation/correlation
identity in owning workflow evidence.

## Supervisor replacement

TPM or Adam takeover preserves logical supervisory seat and **healthy children**.
Protect workers **before** parent archive through the selected surface's route.
Native parent IDs and old callbacks may still reference the retired session.
On Paseo-direct, use qualified reparenting or a recorded logical supervisor with
verified notifications, a subscription or finite observation; see the
[direct recovery reference](references/paseo-direct-recovery.md).
On Kontor-governed work, verify that child notifications reach the new supervisor
before claiming reporting restored, preserving the selected recovery contract.
A mapping alone is not a reporting route. Record which route actually works;
do not claim native callback transfer from a fallback. **Never restart healthy
children** to fix appearance.

## Fresh seat, compaction and loading

Restore required router/role/procedure revisions on launch, resume and
post-compaction. Compaction keeps the native identity and follows
[autonomous TPM operation](../epic-orchestration/references/autonomous-tpm-operation.md#persistent-identity-and-fresh-working-context);
replacement is a separate action requiring retirement evidence.

## Qualification reference

For authoring or reviewing this procedure, load [qualification scenarios and
capability ownership](references/qualification.md). Scenario preparation is not
runtime proof; this skill does not implement adoption, reparenting, surface
transfer or a recovery scheduler.

## Related skills

- [epic-orchestration](../epic-orchestration/SKILL.md) owns checkout and supervisor context.
- The full lifecycle package also provides `agent-dispatch` for role starts and
  `seat-handoff` for phase transitions. If absent, follow the owning workflow;
  do not invent an operation or substitute takeover.
