# Paseo-direct recovery

Use with [seat-takeover](../SKILL.md) when replacing a Paseo-direct occupant.
This is a procedural ownership fallback, including explicitly authorized
takeover into an already-open receiver. Kontor-governed recovery keeps its
selected runtime contract.

## One recorded owner

Use the owning task's existing brief/checkpoint for the predecessor → successor
link, exact native IDs, takeover coordinator, operation identity and current
state: prepared, predecessor retired, active successor or unoccupied recovery.
The existing task/brief identity plus old/new IDs may identify the operation;
do not invent a native generation when the direct runtime has none. Do not
require a new registry, daemon receipt or native adoption API. Re-read
the record and native sessions before retirement and before successor effects;
replay resolves the existing successor. If another takeover is in flight,
reconcile it rather than starting another. These checks are procedural, not an
atomic daemon fence.

An existing receiver must have suitable placement and authority and no
conflicting active work. It cannot occupy two live role slots in the same team
run: if it owns another slot, preserve that slot's obligations and record it as
vacant before activating the replacement assignment. A session that builds the
work cannot later provide its independent audit, even after another role change.
Bootstrap and read-only recovery may precede retirement;
task writes wait for successful predecessor archive, terminal readback and the
common checkout/authority checks. A failed or uncertain retirement blocks all
successor writes for this task and checkout, including edits to different files.
Continue only read-only recovery or unrelated work in a separate checkout where
applicable.
If retirement succeeds but admission fails, record an unoccupied seat; never
silently revive the predecessor or create another successor on retry.

All subsequent dispatches, callbacks and watchers must resolve the recorded
current owner before causing task effects. Settle the predecessor's schedules
and establish any needed finite successor schedule through the existing
[supervision procedure](../../epic-orchestration/references/autonomous-tpm-operation.md#finite-heartbeat-and-script-watchdog).
Do not prompt the archived predecessor: an ordinary explicit prompt can
unarchive it. Titles and parent labels do not fence writes.

## Deterministic source tooling

`apps/paseo-plugin/scripts/seat-takeover.mjs` calls the existing plugin owner’s
`review`/`apply`/`check` RPCs through the public host connector. It does not create
another archive/create/send executor or register the retired admission-import
RPCs. Run it from the plugin directory with a qualified public Node connector:

```sh
node scripts/seat-takeover.mjs review /absolute/request.json /absolute/checkpoint.json
node scripts/seat-takeover.mjs apply /absolute/checkpoint.json
node scripts/seat-takeover.mjs check /absolute/checkpoint.json
```

The request names the selected host and daemon ID, plugin ID, exact predecessor,
configured binding or explicit route, and optionally a prepared brief path/SHA256.
Without a brief, the script uses the same owner route as the ordinary UI, keeping
its schedule, child and ownership refusals. It makes no claim that unread
provider-native background duties were transferred. With a brief, that
brief carries the scope, criteria, authorized next action, findings, retry
counters, supervisor, procedure revision, authority/history references and
checkout snapshot. The review pins these before retirement; preserve the
checkpoint and its apply-intent sidecar for recovery. An ambiguous apply uses
Check on that same operation, never a new review to replace the uncertainty.

The prepared-brief path also requires the plugin's provider-native background
reader (`server/background-reader.ts`) and the qualified schedule inventory.
The reader accounts only for work the source runtime owns without a Paseo ID:

- **Claude:** every main-chain `run_in_background` launch and every `Monitor`
  needs a `<task-notification>` in the source transcript whose status is
  completed, failed, killed or stopped. A Monitor event or expiry without a
  status is not terminal.
- **OpenCode:** no source `task` tool part may be pending or running, and every
  child session reached through `parent_id` (depth 8, 500 rows) needs no
  pending or running tool and a completed latest assistant message.
- **Codex** and unverified provider aliases are refused.

A positive read writes one immutable content-addressed snapshot, and review and
apply must read the same revision. A gap, a read error or a changed revision
refuses retirement; an empty caller list is not evidence. Schedules, managed
children and workspace terminals keep their existing owners. A gap is not
permission to use a private host module or to edit an installed SDK.

Known limits: Claude writes no terminal record after a UI stop, a Monitor
timeout or teardown (275 of 427 main-chain launches on one measured host), so
long-lived Claude seats are usually refused here; use ordinary Take over, which
makes no background-transfer claim. Codex background work is not covered. No
reader produces `checkpointed` workers. Synthetic source tests do not qualify
installed use. A candidate needs exact-head review followed by MAIN proof with
disposable sessions before this route can be reported as delivered.

## Preserve workers before archive

Inspect actual managed children, workspace identities and provider-native
subagents. Do not infer worker survival from a different cwd or an idle status.

- A managed Paseo child in a different workspace, or with an actual open agent
  tab, survives parent archive by being detached. Other managed children are
  archived recursively. Read back the affected child identities
  and lifecycle after retirement.
- Explicitly protect each managed child that must survive, including the
  intended receiver if it is a child, with the supported CLI
  `paseo agent detach <exact-id>` or SDK detach operation **before** parent
  archive. User-authorized takeover includes this necessary preservation step;
  it does not require a separate permission question per child. Verify detach
  and keep its existing workspace, checkout and unfinished work. Do not rely
  on an open tab staying open between preflight and archive. If the current
  runtime lacks detach, use an available supported CLI/SDK or defer the affected
  retirement until the worker finishes. Never fake an open-tab label or assume
  an unavailable MCP detach tool exists.
- Provider-native subagents and background tools may belong to the old runtime,
  without an independently managed Paseo ID. Detach cannot preserve them.
  Finish/checkpoint their work or establish recovery of unfinished work before
  closing that runtime; otherwise defer the affected retirement. A successful
  managed-child check does not account for these workers.

Keep the native tree as observed. Qualified native reparenting may be used,
but arbitrary parent-label edits are not an ownership or callback transfer.

## Restore reporting without interrupting workers

Record each unfinished worker's exact ID, current logical supervisor, last
progress and supported observation route in the existing checkpoint.

- For the next normal dispatch/follow-up, use background execution with
  `notifyOnFinish: true` when supported. Verify the notification reaches the
  current owner; do not claim an old registration was transferred.
- For an already-running worker, use a read-only SDK subscription or bounded
  status/activity checks through the existing supervisor/watchdog. Do not send
  a routing-only prompt: prompting can steer or replace the active run.
- Observation fallback names its owner, next check, finite deadline/run bound
  and terminal condition (completion, failure, legitimate wait or escalation).
  Deduplicate by worker/event or progress checkpoint; reconcile missed events
  after reconnect. Stop obsolete watchers and schedules. No repeated LLM turns
  on unchanged state and no second recovery scheduler.

Missing native callback transfer, stale parent display or a daemon restart
must select this reporting fallback rather than block otherwise safe direct
takeover. Verify read access/registration and record the observed route before
claiming supervision restored. If every supported reporting route fails, defer
the affected supervisory effects and record the specific gap.

Acceptance criteria, open findings, retry counters, permission checks, review
and merge gates stay unchanged. Recovery changes the occupant, not the work's
quality bar.

## Native evidence and capability limits

Recheck the installed runtime version's supported operations before relying on
any mechanism below; a description in this file is not a capability guarantee.

- **Lifecycle and archive cascade:** archive closes the runtime and handles
  children; `archivedAt` can be set before close fails. Require a successful
  archive response and no executing runtime, not the timestamp alone.
- **Supported detach CLI and SDK:** detach preserves the managed child;
  subscriptions can observe without dispatching work.
- **Prompt and finish-notification handling:** callbacks are caller-specific,
  one-shot and held in process memory; prompting may replace a run or unarchive
  a target. Reparenting alone does not migrate callbacks.
- **Schedule lifecycle:** archive attempts to settle target schedules and
  archived targets do not receive heartbeats; verify settlement, since callback
  failures are logged. Successor schedules need their own supported registration.

The project's host configuration names the installed binary and its supported
operations; read them live rather than copying a version string into this
skill.
