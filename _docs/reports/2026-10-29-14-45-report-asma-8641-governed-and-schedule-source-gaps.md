# ASMA-8641 governed transfer and scheduled-duty source gaps

- **Status:** Boundary candidate. No executable governed transfer and no schedule
  transfer are implemented; the current governed refusal and scheduled-duty
  `unknown` are preserved.
- **Base:** `a047050a` on `master`, the current base after `68c9c16a` and the four
  summary fixes. The runner behaviour is the integrated `8499cd7b` behaviour and
  the client UI is `333a1bbd`.
- **Scope inspected:** `apps/paseo-plugin`, `crates/kontor-api`,
  `crates/kontor-daemon`, and the public `@getpaseo/client` schedule contract.
  No merge, install, live host/provider call, native fallback, fork or custom
  host/SDK is involved.

## 1. Governed ownership transfer

### What exists

The plugin recognises a governed seat from its `kontor.seat_binding_id` label and
refuses it in `session-lifecycle.ts` before any resolver, summary, route or
native effect (`GOVERNED_ADAPTER_GAP`). The refusal is also applied to a
continuity receipt whose surface is `governed`. The reason is not a missing
label or an unreadable owner: it is that the only installed authority writers
cannot accept the handoff before the successor has task effects.

| Surface | What it can do | Why it is not the takeover writer |
| --- | --- | --- |
| `CoreTeamRouteApplyRequest` (`crates/kontor-api/src/applications.rs:1140`) | Admin-only, preview-hash fenced, preserves one `SeatBinding`, archives one exact native and launches a successor; `takeover: true` is the idle/failed takeover arm. | It has no handoff, summary revision or bootstrap/ack field. The replacement launch prompt (`crates/kontor-daemon/src/applications.rs:30465`) contains only the predecessor/seating facts and authorises bounded Kontor work; it does not carry the takeover summary. A later `HostedSeatMessage` is after the successor can act. |
| `ReplaceSeatRequest` (`crates/kontor-api/src/applications.rs:7205`) | Admin-only, fenced by predecessor/task revisions and binding generation, links one successor to a runtime-terminal or never-bound predecessor. | It has no caller-supplied handoff/summary field. The daemon may generate its own terminal-run `SuccessionHandoff` from the predecessor transcript, but that is not the takeover's saved final summary. It also requires a terminal/abandoned run, so it cannot replace an idle or failed live source. |
| `CoreTeamSeatClaimApplyRequest` (`crates/kontor-api/src/applications.rs:1424`) | Attaches an already-running native to a seat and records a predecessor in the topology. | Non-destructive adoption of an existing native; no handoff field and no takeover archive/launch sequence. It is not a replacement writer. |
| `HostedSeatMessageRequestDto` (`crates/kontor-api/src/applications.rs:1480`) | Sends a bounded message to an attached seat. | Post-launch delivery; it cannot place the handoff before the successor's first task. |

The route and replacement handlers both require `CallerCapability::Admin`
(`preview_core_team_route`, `apply_core_team_route`, `replace_seat`). The plugin's
composed MCP server is fixed at `--credential-tier observer`; `readConfig` says
the plugin never acquires a write credential, and the plugin path has no Admin
client. A host may have an Admin credential for an operator-run tool, but the
plugin does not have an implemented Admin-authority path.

### Exact minimal boundary

The minimal authority boundary is a handoff-bearing, Admin-authenticated
governed apply, or a Kontor-mediated writer that holds the Admin credential
outside the plugin:

1. **Extend the route writer.** Add a fenced `handoff`/`successor_bootstrap`
   input to `CoreTeamRoutePreviewRequest` and `CoreTeamRouteApplyRequest`
   (summary text or a content-addressed reference, summary revision/digest,
   source native and generation). The preview hash must cover it; apply must
   recheck it and include it in the successor's launch prompt before the first
   task. The outcome/readback must record the handoff digest and the bootstrap
   acknowledgement requirement. `ReplaceSeatRequest` needs the same field if it
   is also to serve as a writer; otherwise its terminal-only, daemon-generated
   handoff remains a separate path.
2. **Or add one Kontor-mediated takeover operation.** The plugin submits the
   exact route fences plus the prepared handoff to Kontor; Kontor applies the
   route with its own Admin credential and returns the existing succession
   readback. The plugin would not hold the Admin credential.
3. **Or keep the handoff and the Admin call entirely operator-side.** The
   plugin may prepare the summary and exact fences for an operator-run Admin
   tool, but it must not claim a transfer until that tool returns a successor
   readback. This is a handoff, not a plugin writer.

A native direct create/archive remains forbidden. A handoff sent after
`apply` is not a substitute for any of the three options.

### Current behaviour

`session-lifecycle.ts:911` and `:1021` still return `refused` with
`GOVERNED_ADAPTER_GAP`; the review is `refused`, the digest and pinned state are
empty, and no archive, create or send effect is reached. This is intentional and
must stay until one of the boundaries above exists.

## 2. Ongoing scheduled-duty preservation

### What exists

The direct path reads a complete, host-qualified schedule inventory when the
host exposes `createHostClient()` and `scheduleList()` (or the installed SDK's
`schedules.forAgent` reader). It treats only rows whose target is `agent` or
`self` and whose target ID equals the source, with a non-`completed` status, as
ongoing source-linked duties. Managed children are counted separately for
detach/readback; they are never schedule coverage and never make a wake read
clear. A live source with any ongoing duty returns `wakes: "unknown"` and a gap;
the current `readDirectWakeState` message names retarget and create-deduplication
as the missing contract. Partial, unreadable, host-unqualified or malformed
inventory also stays `unknown`.

### Why the public host surface cannot safely preserve it

The inspected public `@getpaseo/client` contract, including the 0.13.2 package
that provides `createHostClient`, has:

- `CreateScheduleOptions.target` for `self`, an exact agent or `new-agent`.
- `UpdateScheduleOptions` with `name`, `prompt`, `cadence`, `newAgentConfig`,
  `maxRuns` and `expiresAt`; **no target field**.
- `ScheduleSummary` with `id`, `prompt`, `cadence`, `target`, `status`, times and
  run limits; no schedule revision/ownership generation.
- `scheduleDelete` and `scheduleCreate`; `scheduleCreate` accepts a request ID,
  but the contract does not prove that request ID is a durable deduplication
  key across a lost reply.

Changing the target therefore requires delete-and-create, which is not an
atomic retarget: the old duty can fire, the new duty can be duplicated or lost,
and the new schedule identity and history are not the old ones. Pausing first
would avoid a duplicate active duty, but without a target update the paused
duty cannot be moved and cannot be safely completed after the source is
archived. Child count, including zero children, does not close this gap.

### Exact API delta and safe sequence

The minimal public host delta is one of:

1. Add `target` plus `expectedUpdatedAt`/`expectedRevision` (and an idempotency
   key) to `UpdateScheduleOptions`/`schedule/update`, with a host-qualified
   readback that proves the exact schedule was retargeted while preserving its
   ID, prompt, cadence and history.
2. Add an atomic `schedule/retarget` operation keyed by
   `scheduleId + expectedUpdatedAt + targetAgentId`, returning the exact updated
   schedule.
3. Add a host ownership-transfer operation that atomically rebinds every
   source-linked schedule to the successor under a source generation and returns
   a per-schedule receipt. This is a larger contract and is only needed if
   individual retargets cannot be made atomic.

With (1) or (2) the safe sequence is: pause each exact source-linked duty and
verify the pause, archive the source, create the successor, retarget the paused
duty to the successor, verify the target and status, then resume. A failure
before retarget leaves the duty paused and the takeover `unknown` rather than
silently dropping it. With (3) the host can perform the same transition in one
receipt. The current refusal remains the correct behaviour until one of these
exists.

## Verification

Exact base `a047050a` with the checked-out `apps/paseo-plugin` sources:

```text
tsc --noEmit                                         PASS
vitest run:
  test/session-lifecycle-takeover.test.ts
  test/server-navigator-capabilities.test.ts
  test/paseo-host.test.ts
  test/paseo-cli.test.ts                             4 files, 186 tests PASS
```

The tests were run through the existing workspace dependency tree; no install
or lockfile change was made. No Rust suite was run because this candidate adds no
Rust or API behaviour.

## Named remaining gaps

- **GOVERNED_HANDOFF_API:** `CoreTeamRouteApplyRequest` and `ReplaceSeatRequest`
  have no fenced handoff/bootstrap input; the route replacement prompt cannot
  carry the takeover summary before task effects.
- **GOVERNED_ADMIN_PATH:** the plugin is observer-only and has no implemented
  Admin-authority path; an Admin-authenticated or Kontor-mediated writer is
  required.
- **SCHEDULE_RETARGET_API:** the public schedule contract has no target update
  or atomic ownership transfer, and create request IDs are not proven durable
  deduplication.
- **GOVERNED_ADAPTER_GAP:** governed review remains refused until the two
  authority gaps above are closed.
- **SCHEDULED_DUTY_GAP:** a live source with an ongoing source-linked duty
  remains `unknown`; child count is explicitly not evidence.
