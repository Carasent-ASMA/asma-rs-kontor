# PR reviews in kontord

> Status: ASMA-8453 (ASMA-8449) draft candidate. It describes what the PR
> review contract supports at this revision and what it refuses. Naming rules
> are in [`docs/NATIVE_NAMING.md`](../../docs/NATIVE_NAMING.md#pr-reviews-asma-8453).
> The desk itself is described in
> [Durable desks in kontord](2026-10-10-23-55-reference-kontord-durable-desks.md),
> its long-lived seats in
> [Durable desk seats in kontord](2026-10-11-02-20-reference-kontord-durable-desk-seats.md).

## What a PR review is

A PR review is one pull request's Adam tab and review room on the PR desk.
Each pull request gets its own of both:

- **The Adam**: a topology node directly below the desk's workspace. Like a
  desk Adam subject, it owns no native container. Its kind declares only
  `parent_session_host`, so its one seat runs in the desk workspace's
  container, project and working directory. Its title is
  `TPM • <Jira key> • <repo>#<n>`.
- **The room**: a topology node directly below the desk, owning a native
  workspace of the desk's own project, titled `RSW • <Jira key> • <repo>#<n>`.
  It hosts the declared reviewer seats, `REV A`, `REV B` and `JUDGE` in the
  recommended shape, in a working directory of its own.

A pull request is identified by its repository and its number, and by
nothing else:

- Two pull requests of one Jira key are two reviews.
- The same number in two repositories is two reviews.
- Neither half is ever read back out of a title. The `<repo>#<n>` a title
  shows is derived from the stored pair.

One `pr_review_subjects` row (schema 134) per review is keyed by project, desk,
repository and number:

- It is written before either node and carries both node ids, so a retry
  reconciles the same nodes instead of placing a second Adam or room.
- It records the exact confirmed Jira key, its epic, its task when the key is
  a task's, and the Team Definition revision that placed it.
- It is immutable and permanent: database triggers refuse UPDATE and DELETE.
  Each node serves one review, and a task key's task must belong to the
  row's epic.

The row itself does not inspect parent nodes when it is inserted. Ancestry is
enforced where the nodes are placed and used:

- The daemon creates the Adam directly below the desk's workspace and the room
  directly below the desk. If a node the row names already exists with another
  kind, parent, scope or topology revision, the open is refused
  (`placement_blocked`).
- Topology validation admits each kind only below the parents its revision
  allows. When the review's revision is newer than the desk's, the store's
  bridge admits only the two edges the row registers: the Adam below the
  desk's workspace and the room below the desk.
- The daemon checks the parents again where it uses the review's native
  placement (`placement_blocked` otherwise): the Adam must be below the desk's
  registered workspace, and the room directly below the desk. Both checks run
  when an open with routes prepares the room and the Adam's workspace, before
  any session is placed or its title rendered, and when close proves the
  workspaces. Readback repeats only the Adam's check: it renders the room's
  seat titles without checking the room's parent.

The Adam and room nodes are not scoped to the epic. The epic, and the task
when there is one, are facts on the row, so nothing is written into the
epic's own graph or ledger.

### Native placement

Every review session runs on ASMA-8451's epic-free hosted-seat launcher, in
the desk's own native project (`HostedSeatScope::Desk`), with no epic or
mini-project label:

- The Adam's session runs in the desk workspace, like a desk Adam.
- The room is materialized as a native workspace below the desk project, from
  the binding the desk recorded. Its working directory is
  `runtime-roots/<project>/desk-<desk node>-room-<room node>`, beside the
  desk's own. The room's seats run there.

## Declaring a PR review

A Team Definition opts in with `pr_review`:

```json
"pr_review": {
  "desk_key": "pr-review",
  "adam_kind": "PRA",
  "room_kind": "RSW",
  "room_roles": [
    { "slot_id": "rev-a", "role_code": "AUD" },
    { "slot_id": "rev-b", "role_code": "AUD" },
    { "slot_id": "judge", "role_code": "AUD" }
  ]
}
```

Validation refuses the revision unless all of these hold:

- `desk_key` is a desk this revision declares.
- `adam_kind` and `room_kind` are two distinct kinds of their own: neither is
  the desk's kind, its workspace kind, or a `desk_leadership` kind.
- **The Adam:**
  - Its parent is the desk's workspace kind, and its only projection
    capability is `parent_session_host`.
  - It declares exactly one slot, with a `role_code`, and no team slots.
  - Its seat template starts with `ROLE_CODE` and renders `SCOPE_JIRA_KEY`
    and `PULL_REQUEST`.
- **The room:**
  - Its parent is the desk kind, and its capabilities are exactly
    `native_child` and `session_host`.
  - Its name template uses only literals, `PREFIX`, `SCOPE_JIRA_KEY` and
    `PULL_REQUEST`, and renders the last two.
  - Its seat template is exactly `SLOT_DISPLAY_NAME`, so each seat is titled
    by its declared label.
  - It declares at least one slot and no team slots.
- `room_roles` names every room slot exactly once. Publication checks only a
  role code's spelling. An open refuses a role the role catalog does not
  declare (see below).
- The Adam's slot id differs from every room slot id, because an open's
  routes are keyed by slot.
- No other container renders `PULL_REQUEST`, and no other container's seat
  template renders `SCOPE_JIRA_KEY`.

A definition without the field keeps its exact canonical bytes and hash.

No bundled revision declares a PR review. A project publishes a successor
topology (with the Adam and room kinds) and Team Definition through `/v1`, then
selects them, as the desk reference describes. The structure is legal only
where the topology allows it: the Adam kind's parent is the workspace kind
and the room kind's parent is the desk kind. The kernel has no branch on
`DESK`, `ADAM`, `TPM` or `RSW`. It knows only the declared kinds and their
capabilities.

## Supported operations

| Operation | Route | MCP tool | Tier |
| --- | --- | --- | --- |
| Read a desk's reviews | `GET /v1/projects/{id}/desks/{desk_key}/pr-reviews` | `kontor_desk_pr_reviews_list` | observer |
| Open a review | `POST /v1/projects/{id}/desks/{desk_key}/pr-reviews:open` | `kontor_desk_pr_review_open` | operator |
| Close a review | `POST /v1/projects/{id}/desks/{desk_key}/pr-reviews:close` | `kontor_desk_pr_review_close` | operator |

The CLI serves the same three tools through the shared registry. Receipts are
`open_pr_review` and `close_pr_review`, and both target the project: a
pull request is not an aggregate, and the PR desk belongs to no epic.

Open and close are operator tier on purpose. Placing a review room and its
seats on an existing desk is a desk-operator operation, like declaring and
reconciling durable seats (ASMA-8451). Opening an ad-hoc desk Adam
(ASMA-8452) creates leadership, so it stays admin tier.

### Open

```json
{
  "expected_revision": 7,
  "external_key": "ASMA-8449",
  "pull_request": { "repository": "Carasent-ASMA/asma-rs-kontor", "number": 101 },
  "model_routes": {
    "tpm":   { "provider": "codex",  "model": "gpt-5.6-sol",   "effort": "xhigh" },
    "rev-a": { "provider": "claude", "model": "claude-opus-5", "effort": "high" },
    "rev-b": { "provider": "codex",  "model": "gpt-5.6-sol",   "effort": "xhigh" },
    "judge": { "provider": "claude", "model": "claude-opus-5", "effort": "high" }
  }
}
```

Every check runs before any effect. A refused open writes nothing and asks
the runtime nothing.

| Check | Refusal |
| --- | --- |
| `repository` is one exact `owner/name`; an unknown field is present | `invalid_request` (schema) |
| `number` is positive and fits the stored integer | `invalid_request` |
| `expected_revision` is the project's | `revision_conflict` |
| The key names an exact confirmed Jira binding of an epic or task in this project | `placement_blocked` |
| That binding may publish to the repository | `forbidden` |
| This pull request is not already reviewed under another key | `ensure_mismatch` |
| The definition declares `pr_review` on this desk | `placement_blocked` |
| The role catalog declares every seat's role | `placement_blocked` |
| `model_routes`, when given, names the Adam's slot and every room slot exactly once | `invalid_request` |
| The definition places the desk exactly as the desk's pin does | `placement_blocked` |
| No other review on the desk renders the same title | `placement_blocked` |
| A live session of this review runs under the same route | `placement_blocked` |

`external_key` is the confirmed Jira key. The MCP vocabulary names no tracker,
so the field does not either. Repository authority comes from that key, never
from a title:

- The key must resolve, spelled exactly, to the confirmed Jira binding of an
  epic or task in this project.
- Its ASMA-8101 publication binding must list the repository. An epic's key
  authorizes the root repository and every module its own tasks own. A
  task's key authorizes the root and the task's own module only.

A new review uses the project's selected Team Definition and pins it for
life. A reopened review keeps its own pin.

Without `model_routes`, an open records the review, both nodes and their
seats, and launches nothing. With routes, it also:

1. materializes the desk natively if needed;
2. materializes the room's native workspace below the desk project;
3. launches each seat that holds no session at its next generation.

A seat whose session is already live is reused; nothing is relaunched.

If the room or the desk workspace no longer matches its binding (another
directory, another native, or no longer below the desk project), the open is
refused before any launch, and the stored binding is not rewritten. The code
is the runtime's: `stale_binding` for another native, `unsupported_capability`
for another directory or parent.

A replay with the same key returns the original receipt and the review as it
stands, with no effects. The same key with another body is
`idempotency_conflict`. A fresh key for a review that is already open
converges on the same nodes, seats and sessions.

This differs, deliberately, from the ad-hoc desk Adam open (ASMA-8452).
Replaying that open re-runs its effects and converges on the same subject.
Replaying a review open answers `unchanged` and asks the runtime nothing,
which is what "replay reuses each subject's bindings" requires. Both are
convergent: repeating either one never places a second subject, node, seat
or session.

### Close

```json
{
  "expected_revision": 9,
  "pull_request": { "repository": "Carasent-ASMA/asma-rs-kontor", "number": 101 }
}
```

1. Only this review's two nodes are read. If any seat's launch is prepared but
   unrecorded, the close is refused (`placement_blocked`) before anything is
   fenced. Retry the open with its own key first.
2. Every workspace a live session runs in is proved against the runtime: the
   desk workspace for the Adam, the room for the reviewers. If one no longer
   matches its binding, the close is refused whole before any session is
   fenced, so the Adam's session is never retired while the room's survive.
   A workspace with no container binding at all is refused the same way
   (`stale_binding`). The daemon's own open binds the workspace before it
   launches a session, so only a session row written outside that sequence
   can reach this; the store does not require the binding when it records a
   hosted session.
3. Each live session of the Adam and room is retired in the desk project and
   moved into its seat's history. A session the runtime proves already gone
   counts as fenced.
4. Each seat is released.

Close is not atomic. The proofs come first, but the sessions are then retired
one at a time. If the runtime fails after the first retirement, for example
when a retirement's answer is lost, the close is refused with the runtime's
code (`unavailable` for a lost answer). That refusal's generic advice says
nothing was changed, which is not true here:

- The sessions already retired stay retired and move into their seats'
  history.
- No seat is released and no receipt is recorded, so the review still reads
  back as open, with those seats showing no native.

Retrying the close with the same key converges. The failed attempt recorded
no receipt, so the retry runs in full:

- It retires only the sessions still recorded as live. A session the runtime
  had already retired counts as fenced.
- It then releases every seat of the review and records the receipt. A
  replay after that answers `unchanged`.

Nothing else is touched: not the desk, its workspace, its durable seats, or
any other review. The row, both nodes and the room's native workspace stay.
Opening the same pull request again reuses the nodes and the room and places
fresh seats at generation 1.

Keeping the room is preservation, not a leak. Closing a pull request never
archives the desk or its durable seats (REQ-006). The emptied room stays
bound to its review, so a reopen lands in the same room rather than
creating a second one. A pull request with no review on the desk is
`not_found`. A replayed close returns its receipt with no effects.

### Readback

A review reads back:

- desk, repository, number, the Jira key (`external_key`), epic, task when there is one, the pinned
  Team Definition reference, `open` (any seat placed) and the creation time;
- for the Adam and the room: node id, kind and parent;
- the native container the node uses: topology node, native id, observed kind,
  title, working directory and parent native id. For the Adam this is the desk
  workspace's container;
- each placed seat: binding, slot, role code, rendered title and, when it has
  a session, the native id and occupancy generation.

The list is ordered by when each review was opened.

## Support limits

- **Governed project only.** A repository is authorized through the ASMA forge
  mapping, which only the governed ASMA project has. In any other project
  every open is refused (`forbidden`).
- **Titles show the repository's name without its owner.** The approved title
  format (REQ-006) is `TPM • <key> • <repo>#<n>` and `RSW • <key> • <repo>#<n>`,
  with no owner. Identity still includes the owner, but two repositories of
  one name under different owners can render one title. A review whose title
  would equal another review's on the same desk, such as the same name and
  number under another owner, is refused (`placement_blocked`). It is never
  renamed or retitled. If the case appears live, it becomes a scoped
  question, not a silent format change.
- **Close keeps the room.** By design (see Close), its native workspace stays
  in the desk project, empty, until the same pull request is opened again.
  Kontor offers no operation that removes it.
- **No rerouting or in-place relaunch.** A route differing from a live
  session's is refused (`placement_blocked`). Opening over a session that
  died answers `stale_binding` and relaunches nothing. In both cases, close
  the review (a dead session counts as gone) and open it again.
- **Close is not atomic.** A runtime failure part-way through a close leaves
  the sessions retired so far retired; retrying the close with the same key
  completes it (see Close).
- **No supervision.** Kontor does not watch or restart review sessions. The
  durable seats on the desk are reconciled as ASMA-8451 describes; review seats
  are not durable seats.
- **The review engine is out of scope.** Each seat gets a prompt naming its
  role, the pull request, the desk and the key. Reviewing, judging and merging
  follow the existing reviewer protocol. Fix and takeover room variants are
  not modelled.
- **The desk must be placed identically.** The reviewing definition must place
  the PR desk exactly as the desk's pinned definition does: same declaration,
  separator, topology lineage and desk containers.
- **Native evidence.** The daemon's native effects (room materialization below
  the desk, desk-scoped launches, titles, retire on close, reuse on reopen) are
  proved against the scripted fake. No Paseo contract test covers a room
  below a desk yet, and the plain-directory room cwd has not been exercised
  against a live Paseo runtime (CON-007).

## Schema, backup and rollback

- Schema 134 adds `pr_review_subjects` and rebuilds `command_receipts` as
  ASMA-8452's schema 133 left it, with the two new command kinds.
- Export generation 17 carries the review records. Generations 15 and 16 are
  ASMA-8451's and ASMA-8452's. An older generation cannot represent a database
  that holds reviews and is refused for one.
- Reviews are permanent. Selecting a definition without `pr_review` stops new
  reviews; existing ones keep the revision they pinned.
- A pre-134 build refuses a schema-134 database. Restore the snapshot taken
  before the upgrade, per [`RECOVERY.md`](../../RECOVERY.md).
