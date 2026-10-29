# Autonomous TPM operation

Load when planning epic supervision, starting a TPM, handling seat finished,
installing a heartbeat, or recovering stalled coordination. This is the shared
workflow for Paseo-direct and explicitly Kontor-governed epics. It grants no
identity, permission, model eligibility or deployment authority.

## Plan binding

Every epic plan has an **Autonomous TPM operation** section. Bind the absolute
receiving-checkout path to the selected TPM cadence profile
(`{{ project.supervision }}` in the selected config folder) and its content
hash; record approved overrides instead of copying policy into prompts. That key
names only the autonomous-TPM profile. The Paseo watchdog/recovery policy is the
separate `<state-root>/supervision.yml`, sampled by
`config/examples/paseo-supervision.yml`; do not use one key for both files. Keep
this workflow profile separate from the activated fleet bundle. It neither
changes selected models nor modifies existing pins.

The section records:

- Confirmed epic key, surface, TPM native/session/logical identity and parent;
  project, workspace, checkout and sole plan/integration writer.
- Heartbeat and liveness cadence/timezone, stale threshold, turn budget,
  renewal deadline and expiry; why the cadence fits the work.
- Seat-finished delivery route, owned heartbeat and liveness identifiers with
  actual installation/readback evidence, or the exact unsupported capability.
- Absolute checkpoint/history paths, backend, standing authority reference,
  context-refresh policy, blocker owners and resumption triggers.
- Parallel admission ceiling, collision constraints and final acceptance gates.
- Startup, pause, completion, ownership transfer and recovery shutdown rules.

Kickoff binds the contract. The delivery owner installs supported supervision
before ending its first delivery turn. An absent installation is an unmet setup
item, not permission to claim autonomous operation or to create another TPM.

## One coordination turn

1. Read the selected surface and current owner. If authority transferred, paused
   or completed, perform no delivery effects. Never let a late seat-finished
   event regain writer authority.
2. Read the compact checkpoint and changes since its revision. Use at most five
   relevant events and three actionable items within the configured budget.
   Retrieve an exact artifact only when needed; do not reload the transcript.
3. Reconcile actual native state once. Do not interrupt or prompt an active
   child again. Deduplicate seat-finished events by event identity and exact
   artifact head; a repeated event cannot repeat publication or placement.
4. Settle completed work; admit the next eligible work and useful independent
   streams under standing authorization. Routine continuation, approved
   fallback and reversible implementation do not need another "continue".
   Apply [selected fleet enforcement](fleet-selection.md) to every launch,
   resume or replacement, including supervisor recovery. The heartbeat points
   to the fleet selection record; it never embeds its own model blacklist or
   fallback chain. Missing fleet selection is an owned setup gap, not authority
   to use a runtime default.
5. For a blocker, record evidence, owner, next action and an observable
   resumption trigger. Continue unaffected work. Do not retry an unchanged
   authentication, capacity or permission failure on a timer.
6. Reconcile the tracker at real workflow boundaries. Update the one
   authoritative plan/checkpoint with exact receipts, active work and next
   action. Atomic checkpoint replacement must compare the expected revision and
   owner.
7. End the turn after durable checkpointing. Seat finished wakes the same TPM.
   A safety heartbeat recovers a missed seat finished or idle ready work.

No-change heartbeats end after a compact readback. Native heartbeats still invoke
the model; zero inference on an unchanged tick requires a supported deterministic
prefilter. Do not claim that prose installs one.

## Runtime installation and liveness

On Paseo-direct, the TPM uses native `create_heartbeat` from its own session;
read back the actual target, cadence and identifier. A root caller cannot use a
self-targeting operation and label the result a TPM heartbeat. Use supported
expiry and renewal; renew before expiry without creating duplicate registrations.
The prompt points to the current plan, profile and checkpoint, rather than
embedding a stale roster. Completion prompts use background notification.

The initiating Adam/LSA's existing supervision may perform the independent
liveness check. Give it a disjoint responsibility: check TPM availability,
checkpoint age and heartbeat expiry; notify/resume the existing TPM under the
selected recovery procedure. It cannot become a second task admission or plan
writer. A failed/stale TPM requires actual state and ownership preflight before
resume or authorized takeover; age alone cannot retire an occupant.

On Kontor-governed work, use the existing supported scheduler, checkpoint and
recovery operations. Do not add a second admission engine, bypass pinned gates
or demand `kontord` containment/repair from Paseo-direct delivery.

<a id="finite-heartbeat-and-script-watchdog"></a>
## Finite heartbeat and script watchdog (new work)

Applies to new work.

- Seat finished comes first. A heartbeat is only a safety net.
- Every heartbeat is finite: always record `max-runs` and `expires-in`.
- A "no change" beat ends at once.
- Delete the heartbeat when no child is working; create a new one per round.
- A script, not an AI, watches for events and wakes the right seat. On the PR
  desk this is the desk watchdog. It uses no tokens while nothing happens. The
  woken seat decides what to do; the script never decides.

<a id="context-policy"></a>
## Persistent identity and fresh working context

This is the context policy for every seat. The detailed decision is the
project's seat context-window compaction policy, when installed.

- Keep the seat's native identity and history; resume the same occupant when
  usable.
- Compact only at a trigger: the token threshold, an enabled coherent scope
  boundary, or an authorized operator request. A completed turn alone is not a
  trigger.
- Compact only through a supported same-session runtime capability. For any
  claimed same-session compaction, read back unchanged native and session
  identities.
- `required` enforcement blocks the affected action or reuse until it is proved.
- `best_effort` records `not_enforced` when enforcement cannot be confirmed and
  may continue while context is usable.
- Never archive, reload, start a new run or create a replacement merely to
  shrink context or simulate fresh context.
- The TPM writes a concise run summary every turn. Without supported compaction
  it uses the bounded checkpoint as current working input.
- Compaction does not erase provider tokens or promise a physically empty
  context.

Use `kontord` memory only when available and selected: store a summary/reference,
not duplicate authority. On Paseo-direct the owner-only checkpoint/history is
sufficient; missing memory capability does not stop coordination. Never persist
hidden reasoning, credentials or full conversation dumps.

## Shutdown and method exceptions

Completion disables this epic's owned heartbeat/liveness registration and leaves
the reusable TPM idle. Pause and ownership transfer disable the predecessor's
delivery wakes before the new writer acts. Blocked delivery keeps the
seat-finished wakes of its dependencies; a liveness pass may report an unchanged
blocker but cannot retry it. Never alter another epic's registrations.

**Pause is a hold.** A Paseo-direct pause is not an absence of work and does not
extend itself by silence. Before the pause takes effect the plan or checkpoint
records a `hold:` block with the same four fields a plan hold carries:

- `reason`: one of `waiting_on_dependency`, `waiting_on_capacity`,
  `waiting_on_decision`, `waiting_on_external_party` or `deprioritised` (by
  {{ project.owner }});
- `owner`: the person or seat that acts when the hold lifts;
- `lifts_when`: an observable condition `kontord` or a human can check;
- `review_by`: an ISO date at most 30 days ahead.

A pause with a missing field is refused, and a pause whose `review_by` has
passed is expired: the owner resumes, closes it with a reason, or renews it once
with new evidence. Silence never extends a hold. The plan-file form of the same
block is checked by the pack's `check` command.

An explicit, dated, user-approved execution-method exception:

- may give the operator combined LSA/TPM/lead responsibility and consolidate
  implementation validation and review at the end;
- records its date, scope, temporary owner, preserved artifacts and validation
  boundary in the plan;
- keeps named responsibilities and exclusive checkouts for child workers, and
  keeps the Goal, source evidence, pins and unsupported-effect boundaries
  explicit;
- never fabricates evidence, silently changes the Goal, alters model pins or
  authorizes unsupported live effects.

During root-led delivery, frozen supervisors do not install competing delivery
wakes; restore normal TPM ownership only through an explicit handoff.
