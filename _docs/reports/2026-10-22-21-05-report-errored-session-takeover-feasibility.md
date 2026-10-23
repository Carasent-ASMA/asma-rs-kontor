# Errored-session takeover: feasibility and UI proposal

> **Status:** Investigated; proposed, not implemented or qualified.
> **As of:** 2026-10-22 21:05 UTC.
> **Scope:** The Paseo Sessions navigator at `61f6b07518aadcf741ccdc60c127b0aff60e63f5`.
> **Publication class:** P; all examples below are synthetic.

The navigator can show **reported successor links** using its existing reads.
It cannot prove completed takeovers or safely offer a general replacement action
from those reads alone. Stop before adding an executable takeover button. This
report proposes the display contract and records the missing command/evidence
contract; it does not change the workflow or require native adoption for a
coordinator's existing procedural takeover.

## What the source actually supports

The plugin pins the client, plugin and protocol SDKs to `0.11.4` and declares
Paseo `>=0.11.4` in its manifest. The additional host-source inspection below
uses public Paseo `v0.12.0`, commit
[`cafe4b01c8522ff39560241c5deb01427dc8dc5b`](https://github.com/getpaseo/paseo/commit/cafe4b01c8522ff39560241c5deb01427dc8dc5b).
This is source evidence, not a statement about an installed daemon's version,
feature flags, user sessions or provider history. No live host was queried.

| Operation or fact | Supported contract and limit |
| --- | --- |
| Directory and exact-ID read | `paseo.agents.list(...)`, including archived sessions, and `paseo.agents.ref(id).refresh()`. Handle `refresh()` fetches metadata; it does **not** call the host's provider-session refresh command. |
| Same-session retry | `ref(id).send(...)` sends to that exact Paseo ID. The existing plugin Resume action checks current error, no active turn/permission wait, unchanged revision, available matching provider/model/effort, persistence, mode, cwd and readable history, then persists intent before sending. It reports **submitted**, not recovered. |
| Create a successor candidate | `agents.create(...)` or `workspaces.ref(id).agents.create(...)` supports an explicit ID, idempotency key, labels, title, config and prompt. This creates a session; it neither retires a predecessor nor transfers its logical seat, grants or conversation automatically. |
| Lower-level persistence resume | `resume_agent_request` accepts a provider persistence handle and overrides. The host handler calls `resumeAgentFromPersistence` without an existing Paseo ID, which can allocate a new ID. The handle's provider is retained. This is not a safe cross-provider fork or a recorded takeover. Do not confuse it with the plugin's same-ID Resume action. |
| Archive and detach | `ref(id).archive()` and `ref(childId).detach()` exist. Archive marks the record before closing the runtime and can cascade to managed children. An archive timestamp alone cannot prove retirement. Detach removes native parentage; it does not transfer workflow ownership or callbacks. |
| Native identity and parentage | Paseo ID identifies a managed session; `persistence.sessionId`/`nativeHandle` identify provider history. The host uses `paseo.parent-agent-id` for managed parentage. None is a predecessor/successor receipt. |
| Labels | `labels` are arbitrary string metadata. Native snapshots persist them, and updates persist changes, but there is no reserved `takeover.*` schema, author attribution, completion time or immutable succession event in the inspected contract. |
| Events and history | Agent create/close/archive/turn events can trigger a fresh read. Plugin lifecycle events are live and best effort, without replay; plugin-appended timeline rows are in memory. Neither substitutes for a durable, queryable takeover receipt. Provider history may be unavailable and can require opening a provider runtime. |

Public source anchors:

- [SDK create options and agent handle](https://github.com/getpaseo/paseo/blob/cafe4b01c8522ff39560241c5deb01427dc8dc5b/packages/client/src/index.ts): `PaseoAgentCreateOptions`, `PaseoAgentHandle`, `createAgentHandleFactory`.
- [Snapshot schema](https://github.com/getpaseo/paseo/blob/cafe4b01c8522ff39560241c5deb01427dc8dc5b/packages/protocol/src/messages.ts#L813): status, permissions, persistence, runtime, labels and archive metadata; no typed takeover field.
- [Persistence resume and archive](https://github.com/getpaseo/paseo/blob/cafe4b01c8522ff39560241c5deb01427dc8dc5b/packages/server/src/server/agent/agent-manager.ts#L1340): `resumeAgentFromPersistence`; `archiveAgentUnlocked` at line 1828; `cascadeArchiveChildren` at line 1876; `writeLabels` at line 2130.
- [Resume handler](https://github.com/getpaseo/paseo/blob/cafe4b01c8522ff39560241c5deb01427dc8dc5b/packages/server/src/server/session.ts#L4451) and [native parent-label helper](https://github.com/getpaseo/paseo/blob/cafe4b01c8522ff39560241c5deb01427dc8dc5b/packages/protocol/src/agent-labels.ts).
- [Prompt orchestration](https://github.com/getpaseo/paseo/blob/cafe4b01c8522ff39560241c5deb01427dc8dc5b/packages/server/src/server/agent/agent-prompt.ts#L303): explicit prompts unarchive by default, before loading and running the target.
- [Public plugin reference](https://paseo.sh/docs/plugins/reference#lifecycle-hooks), read on the date above: event delivery and timeline persistence limits. Current web documentation is supplementary, not proof that a feature exists in the pinned SDK or a selected host.

Repository consumers:

- [`session-directory.ts`](../../apps/paseo-plugin/client/session-directory.ts): `toDirectoryEntry` retains native ID, raw status, parent field and labels. `SessionDirectory` walks all pages and tracks completeness, live updates, reconnects and stale/offline reads. Archive visibility is a separate selection.
- [`session-results.ts`](../../apps/paseo-plugin/client/session-results.ts): `roleLanes` puts an entry in Earlier when another task-linked entry names its exact ID in `takeover.predecessor`, or when it is archived. This is a label-based grouping, not proof of replacement. It reads neither a coordinator identity nor a takeover time.
- [`sessions-screen.tsx`](../../apps/paseo-plugin/client/sessions-screen.tsx): `SessionRow` and `SessionResultRow` render current state and earlier-error cues; `SessionDetailSection` supplies grouped details. No row currently resolves a successor or exposes a takeover action.
- [`server/session-lifecycle.ts`](../../apps/paseo-plugin/server/session-lifecycle.ts): `lifecycleRow` excludes Resume when the predecessor carries `takeover.successor`, `takeover.replaced-by` or `recovery.successor`. These aliases are defensive checks, not a native evidence contract. The service's durable per-session fences cover its own Resume/Archive calls, not all host clients.

## Proposed display

Keep current native state visible; a link never turns Error into success.
An earlier-error cue on an idle or closed predecessor remains history. A
continuation line appears directly below the session title in both grouped and
flat results, with the full evidence in Details.
Resolve against the whole selected host directory before applying search, role,
task or workspace filters. Never join by title, task number, recency or parent ID.

| Evidence available | Visible wording |
| --- | --- |
| No link in a complete, current read | **Errored — no successor known.** This does not prove that nobody took over; a procedural record may exist elsewhere. |
| No link in a partial, archived-excluding or offline read | **Errored — successor unknown.** Name the coverage limitation and last inventory-read time in Details. |
| Exact label link, target read successfully | **Successor reported: SWE • SAMPLE-100 • sample work — Running.** Details name the source label, both exact IDs and read time; **takeover completion unverified**. |
| Authoritative completed takeover receipt plus fresh successor read | **Taken over by [recorded coordinator] — job continues as [successor title].** Show the receipt's takeover time separately from the latest native observation. Use “Running” only while observed; idle, waiting, errored, closed and archived successors retain those words. |
| Missing target, conflicting claims, self-link or cycle | **Successor link unresolved** or **Conflicting successor links**. Keep the error visible and list exact claims; never select the newest. |

Use `takeover.predecessor` on a reported successor and the three existing forward
aliases above only as **claims**. Preserve their source keys. Reciprocal labels
can corroborate identity but still cannot establish actor, time, authorized
ownership or successful continuation. Follow chains with a visited-ID set and a
bound; forks and truncated reads stay explicit. Older sessions without links
cannot be repaired by guessing from a title such as “revision 3” or “revision 4”.

“By whom” distinguishes the recorded coordinator from the successor's observed
provider/model. Neither is a verified human identity. A title, `updatedAt`,
`createdAt`, attention timestamp or label edit time is not a takeover timestamp.
An idle successor is not proof that the job finished, and a running successor is
not proof of completion or resolution of the predecessor's failure.

## Proposed manual control and eligibility

Place a `GitBranchPlus` icon beside Details and Archive, outside the session-open
press target. Accessible name: **Take over errored session: [title] ([ID])**;
tooltip: **Review takeover**. Use the existing `Action`, `useControlHelp` and
`AnchoredMenu` patterns, theme colors, a visible focus ring and a 44×44 minimum
touch target. Keyboard Enter/Space opens review; Escape cancels and returns
focus to the trigger, or a stable list control if the row disappears. The icon
and text identify the action without relying on color.

There is currently **no qualified eligibility predicate for replacement** in
this navigator. Do not render an enabled button, a pretend-success fixture or a
button that merely creates a session. Same-ID retry remains the separately named
Resume operation. A future replacement preview must resolve the orchestration
surface, current logical owner, admissible route, brief, permission provenance,
exact checkout and child/reporting obligations through the existing workflow.
A raw error or an editable seat label cannot grant that authority.

Confirmation must name the predecessor title and exact ID, intended successor's
original active title, selected provider/model/effort and permission posture,
workspace/checkout, and the effect on the predecessor. Preserve the predecessor
by default. If the selected route requires archive, require explicit confirmation
of **Archive [exact predecessor] and create [intended successor]**; keep history
and files and explain child impact. Never kill or archive as an undisclosed side
effect. Preserve healthy children before retirement or refuse the operation.

Reuse the [seat-takeover procedure](../../workflow/skills/seat-takeover/SKILL.md):
prepare the brief and route first, verify retirement, inherit the original active
title, restore permissions from policy rather than copying generation grants,
read back placement and successor configuration, then allow task effects. An
optional stale display title is a later, recorded change, never retirement proof.
Paseo-direct can still use its documented procedural owner/checkpoint fallback;
this proposal does not require a native adoption API for it. Governed work must
use its existing generation/binding owner, without a direct-native bypass.

## Source-grounded gap and smallest upstream seam

**TAKEOVER-01 — tool advisory; action-blocking; open.** Owner: the selected
runtime's lifecycle API owner, with the workflow owner qualifying its use.

The native primitives support a coordinator's staged procedure, but the inspected
API has no durable predecessor-keyed takeover claim, immutable lineage receipt,
or conditional replacement transaction joining retirement to one successor.
Ordinary prompts can revive archived agents; a plugin-local lock does not fence
native callers. The directory also lacks the common workflow evidence envelope
and an admissible successor route. Therefore `error → create + labels`, an
arbitrary persistence resume, or an archive/create pair is insufficient to offer
a generally safe automatic replacement and claim the work continued.

The smallest proposed runtime addition is a **native replacement claim and
readable receipt**, extending existing lifecycle serialization and creation
idempotency rather than adding a scheduler. Names below describe a proposed
contract, not available methods:

- Claim by predecessor ID, expected native revision and operation ID. Bind an
  intended successor ID before effects; replay and competing claims return that
  same operation. Persist intent before retirement or creation.
- Keep predecessor history queryable. Either fence execution while preserving
  it, or require explicit archive authorization and positively confirmed close.
  Refuse prompts/resume that would reactivate a replaced occupant unless a
  separately authorized ownership transition has completed.
- Read a durable receipt by either native ID, including phase, original title,
  old/new IDs, correlated creation/retirement evidence, authenticated actor
  attribution when available, and actual event times. Unknown actor/time remains
  unknown. Creation is not task-continuation proof.
- Expose unfinished, failed and unknown effects for read-only reconciliation;
  never launch another successor after a lost acknowledgement. A failure after
  retirement leaves an explicit unoccupied/recovery state.

The workflow owner still prepares the brief, routes permissions, protects
children and authorizes task effects. The host receipt does not write scheduler
truth or grant a logical seat. A qualified explicitly archived procedural route
could be implemented without a new native adoption API, but needs that complete
workflow integration and must advertise its weaker cross-client guarantees.

**Closure condition:** a versioned supported command/evidence contract and its
workflow adapter pass replay, competing-client, crash/lost-ACK, predecessor
reactivation, protected-child and exact-ID readback tests; release and selected
host readback are recorded before enabling the control. Labels alone cannot
close this advisory.

## Exact implementation and verification boundary

No runtime, client, RPC, SDK pin or installed configuration changes are made in
this slice. The manual action and all UI states above are **proposed only**.

When the contract is qualified, the smallest plugin change reaches:

- `shared/session-takeover.ts` and `server/session-takeover.ts` for typed
  preview/execute/read contracts over the owning operation; register once in
  `index.server.ts`. Do not turn the existing Resume service into a second
  replacement authority.
- `client/session-takeover.ts` for claim/receipt interpretation and bounded
  lineage resolution; `client/session-takeover-view.tsx` for the line, review
  and outcome; `client/sessions-screen.tsx` for both row forms and Details.
  Reuse `session-directory.ts` and its fresh metadata reads; explicitly fetch
  referenced archived IDs or keep coverage unknown.
- Synthetic resolver and interaction tests for no successor, reverse/forward
  aliases, reciprocal links, cross-filter/archived targets, missing/offline
  reads, conflicts, cycles, stale time/identity, already-completed takeover,
  confirmation/cancel, permission refusal, error and lost-ACK outcomes,
  concurrent clicks/clients, restart/replay and refresh without optimistic
  success. Mutation checks must reject duplicate creation and label-only proof.
- Offline actual-component fixture renders at 320/390/1440 in light/dark,
  keyboard and focus return, tooltip dismissal, touch targets and outcome
  announcements. Block all real-host sockets and external requests; capture
  screenshots outside git. Run plugin typecheck, focused tests and full suite,
  then the repository gates required by `CONTRIBUTING.md`.

For this investigation, no real predecessor was opened, resumed, archived,
killed or replaced. No screenshot or functional-takeover claim is made; there
is no implemented UI to render. Exact validation commands and results belong
in the draft pull request, with generated logs outside the public tree.
