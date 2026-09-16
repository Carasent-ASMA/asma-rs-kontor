# ASMA-8187 high-change record: stale-native Core Team succession

Date: 2026-09-16
Artifact: `high-change`
Task: Jira `ASMA-8187` / Kontor `01a0a943-652b-79d0-9f0c-f4965a33d7ab`
Epic: Jira `ASMA-8186` / Kontor `01a0a943-652a-7711-b00c-e20bbb3c3fed`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-implementation`
TeamRun: `01a0a945-4a98-70b1-a16d-b087017edb0d`
Implementation AgentRun: `01a0a946-f58a-7f32-ba69-23b64e3ac185`
Scope: [`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md) at `b892d71`, corrected at `52c9c4e`

## What this change is

The existing administrative Core Team route preview/apply operation now recovers
one stale native occupant without replacing its logical `SeatBinding`, ECP,
TeamRun or topology. It is a bounded repair of that operation: no new endpoint,
no recovery state machine, no schema migration, no parallel headroom
abstraction.

Everything here runs through `preview_core_team_route` / `apply_core_team_route`
and reuses `has_fresh_provider_reported_headroom`, `retire_hosted_seat`,
`launch_hosted_seat`, `archive_hosted_topology_seat_route`,
`replace_hosted_topology_seat_route`,
`hosted_topology_seat_occupancy_generation` and
`require_completion_authority` as they already stood.

## The compare-and-swap fence

Apply re-plans and compares one previewed hash before its first native effect,
so every value folded into the previewed intent document *is* a fence. The
document gained:

| Family | Fenced values |
| --- | --- |
| logical binding | `SeatBinding.revision`, role slot, role catalog id/version, role code, and the presence *or absence* of a TeamRun |
| occupancy | active occupancy generation |
| predecessor | provider session id, beside the runtime kind/host/generation/native id it already carried |
| ECP placement | topology node, container binding id, container native identity and generation, canonical `cwd` |
| topology | published `spec_id`, `spec_version`, `spec_hash` |
| Core Team | roster version, role-catalog hash, and a server-derived digest of the exact resolved seats document |
| completion profile | pinned profile id, version and definition digest — identity only |
| headroom | the pinned observation's id, account, provider, evidence digest, source, state, observed instant and computed freshness deadline |

Two exclusions are deliberate. Completion generation, round and state are
evidence, not fence: the remediation this repair unblocks advances them, and
fencing them would make the operation refuse the thing it exists to enable.
Container and topology node *revisions* are readback counters that ordinary
reconciliation moves; fencing them expired previews for reasons unrelated to
placement, which the lost-launch-acknowledgement test caught directly.

## Provider headroom

Preview selects one immutable stored observation for the exact approved account
and route and returns it as `headroom_evidence`. Apply carries back the
`headroom_observation_id` only — never a digest — and the server reloads that
row and refuses it unless it still belongs to the same project, account and
provider. That is what keeps the preview hash computable after newer readings
land.

Immediately before the first archive attempt, and separately from the pinned
row, apply selects the *current* fresh reading for that same pinned account and
route and requires admissible headroom. A newer observation is expected and does
not disturb the hash; capacity that has since lapsed refuses with no runtime or
store effect.

Ambiguity resolves at preview, not at apply: `approved_route_account` refuses
unless exactly one enabled profile can select the route's provider, so no
operator is handed a hash that could never have been applied.

This gate gates the stale-native branch only. An ordinary correction of a live
seat keeps its existing contract, because the seat it replaces is still
answering and its capacity was proven when it launched.

## Retirement

Retirement is fenced on the previewed ECP placement — workspace native id,
canonical `cwd` and provider conversation — which the flow previously passed as
`placement: None`, leaving the runtime's own fence unused.

A predecessor is archived only when the runtime proves it is live, idle and free
of open permission requests. `HostedSeatInspection` carried only
`Live | Archived | Missing`, so that evidence did not exist; it now carries
`idle` and `pending_permissions`, answered by the Paseo adapter from the agent's
own status and permission ledger, and failing closed for any disposition the
adapter has not audited. A predecessor that is already terminal, or that the
runtime no longer correlates to this seat, is proved gone and **not** archived —
archiving anything else to make the operation proceed is what the logical seat
must never do.

## Replay and convergence

Apply consults the idempotency ledger *before* comparing the fence. This was a
defect the widened fence exposed: a completed succession necessarily moves the
seat revision and occupancy generation the fence now covers, so a replay that
re-planned first refused its own recorded effect.

An exact replay resolves its successor from append-only evidence: the fenced
predecessor's history row must match on identity *and* generation, and the
successor is the occupancy recorded immediately after it. Anything that cannot
be proved that way fails closed as `stale_binding`. Reading "whoever is active
now" would answer a replay of the command that produced generation two with
generation three, which is the defect this resolution replaces.

At the store, `replace_hosted_topology_seat_route` takes an optional
`HostedSeatRouteFence` re-checked inside the same transaction. The caller's
pre-effect comparison ran before the runtime retired and launched; only this
check refuses drift that appeared while that work was in flight. The seat-claim
command passes `None` and keeps its contract.

## Remediation-generation fence

Unchanged in behaviour and correct as it stood: `require_completion_authority`
reads the current occupancy generation at `tpm_route` submission time. This
change adds the missing TPM evidence — generation one refuses with
`stale_binding`, generation two clears the authority fence — beside the LSA
coverage that already existed.

## Tests

Added to `crates/kontor-daemon/tests/loopback_api.rs`:

- headroom absent → preview refuses; admissible once the account reports room,
  with generation-1 history preserved and the full evidence returned;
- stale, exhausted and wrong-account readings each refuse at preview;
- an ambiguous provider account refuses at preview;
- capacity that lapsed after the preview refuses before any archive;
- a drifted `SeatBinding` revision refuses a valid preview;
- one representative drift per fenced family (topology `spec_hash`, binding
  `role_slot_id`);
- a predecessor mid-turn, and one waiting on a permission request, each refuse
  before archive;
- a lost archive acknowledgement converges on one successor and one retirement;
- a lost launch acknowledgement recovers the *same* native through launch
  correlation rather than minting a second;
- a committed succession admits no second transition through either door;
- a replay after the seat moved to generation three still answers with its own
  generation-2 successor.

Two fake seams were added for this: `lose_next_hosted_launch_ack`, and
`occupy_hosted_seat` / `block_hosted_seat_on_permission`. Both are test-only
controls on `ScriptedFakeRuntime`; no production path consults them.

## Mutation results

| Mutant | Killed by | Observed |
| --- | --- | --- |
| apply CAS comparison removed | `a_core_team_succession_refuses_a_preview_whose_seat_revision_moved` | apply returned 200 and replaced the occupant; the test requires 400 |
| pre-archive headroom preflight removed | `stale_core_team_succession_refuses_when_capacity_lapsed_after_its_preview` | succession proceeded on lapsed capacity; the test requires 409 |
| remediation-generation fence removed | `advance_and_remediate_judge_the_key_before_the_revision` | a generation-one credential spoke for a generation-two seat |

Each was seeded, observed red, then restored and re-verified green; the restore
was confirmed byte-exact with `diff -q` and zero `MUTANT` markers remaining.

## Contract artefacts

The DTOs gained `CoreTeamRouteHeadroomEvidenceDto`, the preview's
`headroom_evidence`, and the apply's `headroom_observation_id`. Regenerated:

- `crates/kontor-api/contract/openapi.json` via
  `KONTOR_UPDATE_CONTRACT=1 cargo test -p kontor-api --test openapi_contract`;
- `apps/console/src/api/schema.d.ts` via
  `pnpm --filter kontor-console generate:api`.

## Residual issues

- **Pre-existing, not introduced here.** Seven `kontor-daemon` tests fail on
  clean `52c9c4e` with my changes stashed: six on the Jira connector answering
  `unavailable`, one on `replaying_a_partial_admission_delivers_its_durable_follow_up`.
  `cargo clippy --workspace` also fails to compile `kontor-tests-e2e`, because
  `observed_identity` was added to `JiraResponse` in `6355b85` (ASMA-8116)
  without updating `tests/e2e/pilot_sections/domain.rs`. Clippy is clean on every
  crate this change touches.
- **`verify-tree.py --mode archive`** fails at its repository-wide reproducible-
  lock gate: current crates.io resolution produces a `Cargo.lock` differing from
  the committed one. Unrelated to this change, and not repaired here without
  separate evidence.
- **The store-commit-before-receipt window** is unreachable rather than untested.
  Command receipts have been undeletable by database trigger since `0001_init`,
  and a different key presenting a spent preview is refused by the fence. Both
  doors are pinned by
  `a_committed_succession_admits_no_second_transition_through_either_door`.

## Handoff

Implemented through TeamRun `01a0a945-4a98-70b1-a16d-b087017edb0d` and
AgentRun `01a0a946-f58a-7f32-ba69-23b64e3ac185`. No topology, TeamRun, AgentRun,
ECP or `SeatBinding` was replaced; no ASMA-8098 completion state was touched; no
Paseo mutation was performed; no frozen pin was relaxed. The forbidden round-1
apply key named in the scope record is not consumed, referenced or reachable
from any code or test in this change.

Production execution remains out of scope. The acceptance case in the scope
record requires a deployment, a fresh preview, and a new apply key created after
it.
