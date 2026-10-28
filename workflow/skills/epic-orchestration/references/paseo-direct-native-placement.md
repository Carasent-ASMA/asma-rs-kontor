# Paseo-direct native placement

How to place, name, read back and supervise seats on a Paseo-direct epic and
on the desks. The **placement owner** (Adam on new work, the TPM on existing
work) creates or reuses every workspace. The rest of this file says
"placement owner".

## Contents

- [Handoff first line](#handoff-first-line)
- [Paseo-direct epic placement](#paseo-direct-epic-placement-the-default-surface)
- [Terms](#terms)
- [Names](#names) and [provisional seat naming](#provisional-seat-naming)
- [Launch and readback](#launch-and-readback) and [session labels](#session-labels)
- [Seats and reuse](#idle-seat-reuse)
- [Autonomous coordination](#autonomous-coordination)
- [Capacity](#capacity)
- [Desks and lead workspaces](#desks-and-lead-workspaces-2026-10-24)
- [Tickets desk](tickets-desk.md)
- [Consultation callers and routing](#consultation-callers-and-routing)
- [Advisor sessions](#advisor-sessions)
- [Committee sessions](#committee-sessions-final-gates-root-cause-hard-problems)
  and [used alone](#used-alone-free-work)

## Handoff first line

Apply the
[first-line rule](../../agent-dispatch/SKILL.md#first-line-is-the-exact-seat-name)
to every handoff, takeover brief or prompt. For the naming source:

- Kontor-governed epic: the exact registered role code or Team Definition slot label.
- Paseo-direct work: the [provisional seat title](#provisional-seat-naming).

## Paseo-direct epic placement (the default surface)

- Apply this file to every epic with a tracker epic key that is not marked
  Kontor-governed. Under the
  [transition rule](orchestration-surface-transition.md#orchestration-surface-transition-rule)
  that is the default.
- For another runtime, do not create, move or rename Paseo projects, workspaces
  or sessions. Follow that runtime's own adapter and instructions.
- For Paseo-selected work, use only supported Paseo operations and keep the
  hierarchy below.

### Kontor-governed epics

- Do not place a Kontor-governed epic by hand. `kontord` owns placement and
  renders hierarchy and names from the exact pinned Team Definition revision.
- `ProjectSessionTopologySpec` validates structural legality; it is not a second
  naming authority.
- The current identity and naming rule is the project's public naming contract.
- Place by hand only inside one bounded, recorded `kontord` fallback. Even
  there, render the pinned definition; never invent a name.

## Terms

- **TSW** (Ticket Session Workspace): the runtime-agnostic ticket workspace
  that holds a ticket's persistent role sessions. One Paseo workspace backs it.
  Never call a TSW a "worktree"; a Git worktree is only its checkout isolation.
- **ECP** (Epic Control Plane): the specification-defined ESW child that groups
  the epic's persistent leadership, coordination and authorized ad-hoc seats.
  The selected operational default backs it with one Paseo workspace in the epic
  project.
- **ASW** (Advisor Session Workspace): a subject-and-topic-scoped, read-only
  consultation workspace for one epic or ticket question. An ESW holds zero or
  more ASWs; each holds one or more advisor seats from the pinned Team
  Definition.
- **CSW** (Committee Session Workspace): a topic-scoped, read-only committee
  workspace for one ticket or epic decision. One Paseo workspace inside the epic
  project backs it; it is never a Paseo project. `TSC` is a historical read/import
  alias only.
- **Seat**: one persistent role session. **Agent** and **agent session** are
  informal aliases.

## Names

- Use only the pinned Team Definition's separator, prefixes, templates, role
  codes, slot labels and hierarchy. Under the recommended definition the
  separator is space + U+2022 BULLET + space: ` • `.
- Create one Paseo project per tracker epic, named by the ESW template.
  Recommended: `ESW • <epic key>`.
- **Existing epics only:** inside the epic project, create exactly one
  local-checkout ECP workspace from the configured template. Recommended:
  `ECP • <epic key>`.
- The ECP directly hosts the configured epic roles: under the recommended
  definition, one persistent `LSA` seat and one persistent `TPM` seat. Name both
  by the provisional seat rule; their scope key is the epic key.
- LSA and TPM stay separate identities and authorities even though they share
  the workspace. Ticket delivery stays in TSWs.
- **A new epic gets no ECP.** Follow
  [Adam, DESK and RSW placement](#adam-desk-and-rsw-placement): its epic lead
  lives in a lead workspace of the `DESK • ADAM` project.
- Name each ticket TSW from the configured template. Recommended:
  `TSW • <task key>`. Name its persistent delivery seats by the provisional
  seat rule.
- If several slots use the same role, Team Definition JSON declares their exact
  distinct display labels. An agent never invents a suffix beyond the
  provisional pattern.
- A new naming or hierarchy rule needs a new immutable Team Definition revision
  and an explicit preview/apply upgrade. Existing pins keep their historical
  rendering.

### Provisional seat naming

While an epic runs outside `kontord`, a seat's session title is
`<role code or slot label> • <tracker key> • <short description>`, with the same
separator.

- TSW delivery seat: the confirmed task key and a short task summary.
- ECP leadership seat: the confirmed epic key and a short epic description.
- Advisor seat: the ASW's own scope key and topic.
- The label always comes first. Never use a draft or inferred key, and never
  carry another scope's key.
- Committee seats keep their exact protocol labels (`SEAT A`, `SEAT B`,
  `JUDGE`) without decoration.
- The rule covers only hand-placed Paseo-direct sessions. A Kontor-governed epic
  keeps the pinned Team Definition rendering: a seat is the bare role code or
  slot label.
- The rule is provisional; a human may open a dedicated naming topic to settle it.

<!-- example: project-specific -->
```text
ESW • EXM-8001
ECP • EXM-8001
TSW • EXM-7869
AUD • EXM-7869 • Fix token rotation
LSA • EXM-8001 • Release readiness
```
<!-- /example: project-specific -->

## Launch and readback

- Fail closed before launch unless the launcher accepts and reads back the
  seat's **selected placement**:
  - epic worker: the epic project `projectId` plus the exact TSW, ASW or CSW
    `workspaceId` (and the ECP `workspaceId` on an existing epic);
  - desk seat: the `DESK • <name>` project `projectId` plus the exact `LSA`,
    `TPM`, `ADHOC` or `RSW` `workspaceId`; a `DESK • TICKETS` ticket adds its
    `TSW` `workspaceId`.
- Do not use a team auto-launch transition for Paseo-selected work while its
  backlog API lacks those bindings. Paseo would otherwise create a standalone
  project rooted at the ticket worktree, even when the right TSW exists.
- Under the recommended operational default, an existing epic needs exactly one
  ECP with distinct LSA and TPM seats, exactly one TSW per ticket inside the
  epic project, and no project rooted at a ticket worktree.
- A new epic needs no ECP and follows the [new-work card](#adam-desk-and-rsw-placement).
- On mismatch, record `placement_blocked`, admit no seats, and keep existing
  sessions and evidence.
- Read back `projectId`, `workspaceId`, workspace kind and `cwd` after every
  create or resume. An identity or path mismatch blocks work and verdicts.
- If project rename is unsupported, record `rename_pending` and rename through
  the Paseo app. Never edit Paseo's internal state files.

<a id="session-labels"></a>

### Session labels

Every Paseo-direct seat create passes four Paseo session labels, which are
key/value metadata and not the seat's display name:

| Key | Value |
| --- | --- |
| `epic` | The confirmed tracker epic key of the seat's scope |
| `task` | The confirmed tracker task key of the seat's scope |
| `role` | The exact registered role code from the pinned role or team definition |
| `orchestration` | `paseo-direct` |

- The values come from the seat's own confirmed scope, never from the parent's
  or a sibling's labels: a parent often launches work for another scope. A seat
  with no confirmed epic or task key omits that label instead of guessing.
- Read all four back with the placement. A mismatch blocks work and verdicts
  like a placement mismatch.
- A Kontor-governed seat carries the labels `kontord` writes instead.
- With the Kontor plugin floor on, its workspace guard treats
  `orchestration=paseo-direct` as a guarded seat. Turn the floor on only once
  the installed guard accepts a Paseo-direct seat that has no Kontor binding.

<a id="idle-seat-reuse"></a>
## Seats and reuse

- An `idle` seat, including `attentionReason: finished`, is reusable and not
  terminal. Send the next role task to that same seat; do not archive it or
  create a successor.
- At team-run start, reconcile the snapshotted template and create every
  declared role slot exactly once in the ticket TSW. Leave roles idle until
  eligible; never invent undeclared seats.
- Put every team role session in its ticket's TSW, with exactly one persistent
  seat per `(team_run_id, role_slot_id)`.
- After each role turn, persist its handoff, verdict and evidence, and leave
  the same seat idle.
- Compact only under the [context policy](autonomous-tpm-operation.md#persistent-identity-and-fresh-working-context);
  never create a replacement just to shrink context.
- Replace a role session only when it is unusable or replacement is explicitly
  authorized. Follow [seat-takeover](../../seat-takeover/SKILL.md): keep evidence
  and workers before a cascading archive, verify retirement, then admit one successor.
- A suitable existing receiver may be the recorded Paseo-direct owner; native
  adoption or reparenting is optional. A terminal timestamp alone does not prove
  runtime closure.

## Autonomous coordination

- Use [autonomous TPM operation](autonomous-tpm-operation.md) for the
  plan-bound seat-finished wake, heartbeat, compact checkpoint and liveness
  contract.
- Keep the same placement owner and native ancestry; a heartbeat is not a new seat.
- During an approved root-led execution exception, root holds integration and
  admission authority; frozen supervisors do not resume delivery on their own.
- Read the Paseo watchdog/recovery policy at `<state-root>/supervision.yml`
  (the sample is [`config/examples/paseo-supervision.yml`](../../../../config/examples/paseo-supervision.yml))
  before dispatch. `{{ project.supervision }}` is the TPM cadence profile, not
  this file.
- Rely on seat finished (Paseo's background notification), then end the
  placement owner's turn and leave its session idle. Never keep it alive with
  `sleep` or status polling.
- The stalled-turn watchdog is only for a turn that never completes. It needs
  every configured stale-evidence predicate, reconciles the same seat first, and
  never creates a duplicate.
- A runtime that cannot report seat finished may use the configured
  bounded-polling fallback.

<a id="capacity"></a>
## Capacity

- One Paseo mini-project has a hard ceiling of
  `{{ project.capacity.max_parallel_tasks }}` concurrent team runs. Count active
  team-run envelopes, not persistent idle role tabs.
- Start admission at `{{ project.capacity.start_admission }}`. Raise it one
  slot at a time after clean capacity checks, never above the hard ceiling.
- `{{ project.capacity.max_delivery_seats }}` is a separate owner-set project
  value scoped per team run. It is not the team-run admission ceiling and is
  not supplied by this placement rule.
- Reduce admission without cancelling running work when runtime/provider, host,
  test/merge, reconciliation, permission or module pressure appears.

<a id="adam-desk-and-rsw-placement"></a>
## Adam, DESK and RSW placement (new work)

Applies to new work. Existing epics keep their names and placement. Names,
including the lead-workspace rule, follow the project's new-work naming card
and its desk layout amendment.

### Desks and lead workspaces (2026-10-24)

- Every durable desk project (`DESK • ADAM`, `DESK • TICKETS`, `DESK • PR REVIEW`
  and any later desk) has native workspaces named exactly `LSA`, `TPM` and
  `ADHOC` on the desk's checkout path. Create a missing one and read back its
  id, name and `cwd`. The navigator shows them first, in that order.
- `LSA` holds key-bound LSAs and durable desk LSA seats. `TPM` holds key-bound
  TPMs, including PR desk Adams. `ADHOC` holds only ad-hoc LSA sessions
  `LSA • ADHOC • <topic>`. `ADHOC` is a scope word, not a workspace-kind code.
- No new session is created in an `ADAM` workspace. A legacy `ADAM` workspace
  still renders until its tabs finish and it is archived when empty.
- One durable top-level Paseo project `DESK • ADAM` holds each epic's lead tab
  in `LSA`, or in `TPM` for a TPM. A bound epic lead never lands in `ADHOC`,
  which holds ad-hoc LSA sessions only. Tab names follow the project's new-work
  naming card.
- Promotion is conditional (supersedes the 2026-10-24 handoff-only rule). When
  a confirmed epic has no canonical epic LSA, promote the same native ADHOC LSA
  in place: preserve its native ID, conversation, model and operator title,
  bind the authoritative epic context, and project it under the epic's logical
  LSA node. The native `ADHOC` origin is retained; never claim a native
  workspace move. When a distinct canonical epic LSA exists, write the durable
  handoff summary through [seat-handoff](../../seat-handoff/SKILL.md) and
  require that exact lead's exact-revision acknowledgement before releasing
  the source; create no second lead. Unknown, incomplete or ambiguous discovery
  is not absence. A legacy fixed persona that conflicts with promotion needs a
  supported same-ID refresh, or the promotion remains an explicit legacy gap.
- The workforce lives in the epic project (or the RSW) as the lead's children.
- A new epic gets no `ECP` workspace. Its ticket TSWs and consultation
  ASW/CSW workspaces stay inside the epic project.
- `DESK • TICKETS` is the desk for small non-epic work. Each accepted ticket
  gets one `TSW • <ticket key> • <topic>` workspace in that desk. Routing, the
  tracker duty, the wall protocol and team staffing are in the
  [tickets-desk procedure](tickets-desk.md).
- The PR review desk is its own durable desk project. Each pull request gets
  one PR Adam tab in that project's `TPM` workspace and one RSW review room with
  its declared review seats. `DESK • PR REVIEW` offers ad hoc creation only.

## Consultation callers and routing

- Advisor and Committee invocation is not TPM-exclusive. Any role or session
  the pinned Advisor profile or Committee template authorizes may invoke a
  consultation, including the `LSA` and `TPM`.
- The result and evidence go to the initiating caller, which records the disposition.
- The placement owner is the epic router. The normal path sends deep epic-wide
  questions to the `LSA`, which may then invoke an Advisor or Committee. The
  placement owner may invoke one directly when it is the right caller.
- Invocation and placement are separate authorities. Only the placement owner
  creates or reuses a TSW, ASW or CSW, passing the calling work's `projectId`
  and reading back the workspace identity, kind and `cwd`.
- This is a placement rule, not a caller allowlist. Callers request placement
  through the supported orchestration path. A child seat, advisor, prototype or
  research session never creates a workspace or project directly.

## Advisor sessions

- One Advisor consultation occupies one read-only ASW sibling inside the
  existing epic project. Never put an advisor seat in a delivery TSW or a
  standalone project.
- The ASW name comes from the pinned Team Definition. Recommended:
  `ASW • <scope key> • <topic>`. The advised subject decides whether the scope
  key is the epic's or a task's.
- Each ASW holds one or more independent advisor seats, named by their exact
  configured registered professional role code or advisor-profile role code,
  for example `SA` or `AUD`. `ADVISOR` is not a universal seat name.
- On a Kontor-governed epic that code is the whole seat name; the tracker key
  and topic stay in the ASW name. On a Paseo-direct epic the provisional seat
  rule repeats the ASW's own scope key and topic after the code.
- If a role occurs more than once, the Team Definition declares exact distinct
  slot labels; `kontord` never invents suffixes.
- Reuse the same ASW and seats for follow-up rounds of the same consultation.
  A materially different subject or topic gets another ASW.
- Do not create one global epic ASW, and never encode a subject or topic in
  seat names that differs from the ASW's own. UI grouping may show all epic
  ASWs together without replacing their distinct native identities.
- Advisors in one ASW give independent findings: no Judge, quorum, voting or
  aggregate verdict. Use a CSW when formal deliberation or aggregation is needed.
- Advice is evidence only. The initiating caller records its accepted,
  partially accepted or rejected disposition through the owning workflow.

## Committee sessions (final gates, root-cause, hard problems)

A committee is one **CSW** in the project of the work that called it: the epic
project for epic work, the desk project for desk work. It sits beside that work,
never inside a working ticket TSW. Create it when the orchestration flow calls
for a committee: final close-out gate, root-cause analysis, competing plans,
reviewer disagreement or advisor uncertainty.

The committee method itself — read-only seats, fleet-bound models, contrasting
reviewers, a fresh judge, independent findings, aggregation and reuse — is owned
by [`committee-protocol`](../../committee-protocol/SKILL.md). This reference
owns only the Paseo-direct placement.

- The CSW title comes from the pinned Team Definition. Recommended:
  `CSW • <scope key | ADHOC> • <topic>` (task question, epic-wide question, or
  `ADHOC` for desk or ad-hoc work).
- Scope follows the debated subject, not the caller. An epic-wide CSW is still a
  Committee workspace, not an Advisor workspace. Desk work places its CSW in
  the calling desk project, beside the calling `TSW` or lead workspace.
- Only the placement owner creates or reuses the CSW
  ([routing](#consultation-callers-and-routing)).
- Every CSW create passes the calling work's `projectId` explicitly: the epic
  project for epic work, the desk project for desk work. Never rely on Paseo's
  fallback project inference.
- An omitted `projectId`, a newly created project, wrong-project readback, a
  duplicate topic CSW or a worktree-rooted project records `placement_blocked`
  and admits zero seats.
- Read back the CSW's `projectId`, `workspaceId`, kind and `cwd` before creating
  seats. Then pass that exact `workspaceId` for every seat the pinned Team
  Definition and Committee template declare.
- Every committee seat, `SEAT A`, `SEAT B` and `JUDGE` alike, passes the four
  [session labels](#session-labels): the debated scope's confirmed `epic` and
  `task`, `orchestration=paseo-direct`, and as `role` the role code the selected
  Committee template or role definition declares for that seat.
- Cardinality and aggregation come from the pinned template. `kontord` serves
  Operational Committee templates; a Kontor-governed epic uses the template its
  run pinned, never a hand-assembled shape.
- The recommended Independent Review configuration names its seats exactly
  `SEAT A`, `SEAT B` and `JUDGE`, with no tracker-key, topic or role-code
  suffix. The protocol's independence rules decide which models may fill them.
- Reuse the same committee seats for corrective re-reviews under the
  [context policy](autonomous-tpm-operation.md#persistent-identity-and-fresh-working-context).
  Do not create duplicate reviewer sessions. A materially different subject gets
  another CSW.

### Used alone (free work)

A `workflow: free` caller may run a committee without an epic or a TPM. The
caller acts as the coordinator and creates a CSW workspace inside the chosen
existing project, with the existing id and path readback. Reusing the chosen
workspace as the CSW is withdrawn. The
[committee protocol](../../committee-protocol/SKILL.md) is otherwise unchanged.
