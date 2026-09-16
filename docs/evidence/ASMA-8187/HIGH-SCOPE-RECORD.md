# ASMA-8187 high-scope record

Date: 2026-09-16
Artifact: `high-scope-record`
Task: Jira `ASMA-8187` / Kontor `01a0a943-652b-79d0-9f0c-f4965a33d7ab`
Epic: Jira `ASMA-8186` / Kontor `01a0a943-652a-7711-b00c-e20bbb3c3fed`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-scope`
TeamRun: `01a0a945-4a98-70b1-a16d-b087017edb0d`
Scope AgentRun: `01a0a945-4a98-70b1-a16d-b09a99d78708`
Implementation AgentRun: `01a0a946-f58a-7f32-ba69-23b64e3ac185`

## Outcome

Extend the existing administrative Core Team route preview/apply operation so
it can recover one stale native occupant without replacing its logical
`SeatBinding`, ECP, TeamRun or topology. The operation retires only the exact
fenced predecessor, preserves its generation-1 hosted-seat history, and appends
one generation-2 native successor on the same approved model route.

This is a bounded repair of the existing Core Team route operation. It is not a
new recovery subsystem, a topology migration, a completion-state override or a
direct Paseo workflow.

## Authority and frozen baseline

The authoritative prescription is Committee run
`01a078a6-3507-7020-a8ea-ab9100221c22`, settled `non_compliant` at revision 6.
Its remediation hash is
`626a2c85df4402152afb6570d65f7685db287ff88005160e3e98be02bee2f4df` and
its result hash is
`ce45d4bd44d37dc400d6dca6a8e86dd35d794bdd7f8090f7658348afd414d0ab`.
That prescription requires supported admin preview/apply succession; it does
not authorize direct runtime mutation.

The implementation checkout is `_tools/asma-rs-kontor` on branch
`feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession`,
based on current `origin/master` commit
`f78d041`. Preserve this checkout and the TeamRun above.

The baseline already contains the seams this repair must reuse:

- `core_team_route_plan`, `preview_core_team_route` and
  `apply_core_team_route` in `crates/kontor-daemon/src/applications.rs`;
- the existing Core Team route preview/apply request and response contracts in
  `crates/kontor-api/src/applications.rs`;
- `has_fresh_provider_reported_headroom` for exact provider-reported capacity;
- `retire_hosted_seat` and `launch_hosted_seat_inner` in
  `crates/kontor-runtime-paseo/src/adapter.rs` for exact retirement, placement
  readback and correlated lost-ack launch recovery;
- `archive_hosted_topology_seat_route`,
  `replace_hosted_topology_seat_route` and
  `hosted_topology_seat_occupancy_generation` in
  `crates/kontor-store/src/repository.rs` for append-only predecessor history,
  atomic active-row replacement and replay;
- `require_completion_authority` in
  `crates/kontor-daemon/src/applications.rs` for current-occupancy completion
  authority at submission time.

Do not create a parallel endpoint, recovery state machine, headroom abstraction
or persistence model unless a failing required test proves that the existing
receipt/history/correlation seams cannot carry the contract.

## Identity-preserving succession contract

The successful transition has exactly this shape:

```text
same logical SeatBinding
  generation 1: exact stale predecessor -> immutable hosted-seat history
  generation 2: one active native successor on the same ECP and approved route
```

The operation must:

1. preserve the same project, epic, TeamRun, topology node, ECP and logical
   `SeatBinding` identifiers;
2. preserve all generation-1 history and append, never rewrite, that history;
3. create exactly one generation-2 active occupancy;
4. keep the approved account/provider/model/reasoning route unchanged;
5. issue a generation-2-scoped credential to the successor;
6. refuse rather than adopt, relabel or retire any native identity not equal to
   the fenced predecessor or the uniquely correlated successor;
7. leave all frozen Team Definition, topology, Core Team and completion-profile
   pins unchanged.

If the exact predecessor is still present, runtime retirement is allowed only
after exact placement readback shows that native session is `idle` and has no
pending permission. If the exact predecessor is already archived, closed or
absent, treat that as an idempotent predecessor-retired state only when the
durable occupancy and runtime correlation still identify that exact
predecessor. Never archive a different session to make the operation proceed.

## Preview evidence and compare-and-swap fence

Preview is read-only. It must return one deterministic intent document and hash
covering every value below. Apply accepts that preview hash plus a caller
idempotency key and rereads every fenced value before its first native effect.

| Fence | Required exact evidence |
| --- | --- |
| logical binding | project, epic, TeamRun, role slot and `SeatBinding` id plus the current `SeatBinding.revision` |
| occupancy | active occupancy generation, required to equal generation 1 for the production case |
| predecessor | runtime kind, native id, runtime generation, provider session id and stored observed identity |
| ECP placement | logical ECP node id, native project id, native workspace id, canonical `cwd`, container generation and provider conversation/session correlation |
| approved route | account, provider, model, reasoning effort and approved route digest |
| Team Definition | pinned id, version and canonical document digest |
| topology/Core Team | pinned topology revision/hash plus Core Team roster revision, definition version and catalog hash |
| completion profile | pinned profile id, version and definition digest; completion generation, round and state are evidence only and are not changed by this task |
| remediation fence | current authoritative occupancy generation at TPM-route submission time |
| headroom | exact approved account/provider report digest, source, observed time, freshness deadline and available capacity state |

The server must derive this evidence from authoritative stored state and runtime
readback. A caller must not supply an unchecked digest or infer a native
project/workspace identity. The preview hash commits to a canonical encoding of
the complete intent and all evidence above, not merely the desired model route.

Apply must refuse without runtime or store effects when any readback differs,
including a changed `SeatBinding` revision, occupancy generation, predecessor
native id/runtime generation, ECP placement/cwd, provider session, approved
route/digest, Team Definition digest, topology pin, Core Team roster/catalog
pin, completion-profile digest or remediation-generation authority.

## Provider-headroom preflight

Preview must obtain fresh provider-reported headroom for the exact approved
account and route and record its evidence. Apply must repeat that preflight
immediately before the first predecessor archive attempt; a cached preview alone
is insufficient authority. Missing, stale, exhausted, ambiguous or wrong-account
evidence refuses with zero retirement, launch or store effects.

An exact completed idempotent replay is resolved from its durable receipt before
performing another provider probe. This keeps replay independent of later
capacity drift while preventing a first or incomplete apply from using stale
headroom.

## Apply ordering, durability and readback

Apply follows this order:

1. derive the request intent and check the idempotency ledger;
2. return an exact completed receipt for the same key and same intent, or refuse
   the same key with a different intent;
3. re-plan and compare every preview/CAS fence;
4. perform the immediate provider-headroom preflight;
5. inspect the exact predecessor and either archive that exact idle,
   permission-clear native or prove it is already retired;
6. read back the predecessor as archived/closed/absent without substituting an
   identity;
7. use the existing ECP container and existing launch correlation to create or
   recover exactly one generation-2 successor on the unchanged route;
8. read back the successor's native identity, route and exact ECP placement;
9. atomically append generation-1 history and install generation 2 as the active
   occupancy using the fenced active row and binding revision;
10. durably record the command receipt and return a readback containing the
    unchanged logical identities, both occupancy generations, current pins,
    exact predecessor/successor native identities, route and placement.

The durable receipt binds the idempotency key, request-intent digest, preview
hash, all CAS evidence, predecessor outcome, successor outcome, store transition
and final readback digest. It is written before success is acknowledged.

Lost acknowledgement must converge without duplicate retirement or launch:

- after predecessor retirement, replay observes only that exact predecessor as
  already retired and continues;
- after native launch, replay recovers the one successor through the existing
  binding/ECP launch correlation and refuses zero-or-many ambiguity;
- after store replacement, replay reconstructs and persists the same receipt
  from the exact active generation-2 row and generation-1 history;
- after receipt persistence, replay returns the same receipt/readback without
  runtime or store mutation.

## Remediation-generation fence timing

The remediation-generation fence is evaluated at `tpm_route` submission time,
not when the completion round was opened. The existing round-1 completion and
logical TPM `SeatBinding` remain unchanged across succession. After generation
2 is current:

- a credential scoped to generation 2 may submit the existing round-1
  `tpm_route` remediation;
- a generation-1 credential is stale and must be rejected;
- no caller may lower, bypass or rewrite the fence, round, generation, profile
  or prior remediation history.

ASMA-8187 implements and tests that authority boundary with isolated fixtures.
It does not submit the production remediation or directly modify ASMA-8098
completion state.

## Production acceptance case after deployment

Deployment and production execution are outside this source task. After the
compatible fleet is deployed, the authorized operator must run a new preview
against exactly:

| Field | Required value |
| --- | --- |
| source task | `ASMA-8187` |
| target task | `ASMA-8098` |
| role | `TPM` |
| SeatBinding | `01a070e5-7909-7c31-8843-9705948f4f9e` |
| expected current occupancy | generation 1 |
| expected predecessor native id | `f8c211e4-e41e-4897-ba58-4656eb35192e` |
| approved route | `codex-personal/gpt-5.6-sol/high` |
| approved route digest | `b341d4909a562e3d0ea3ccead843944e275fd6e7f5988c4d12bb198e49b08327` |
| expected successor occupancy | generation 2 |

The old apply key
`recover-asma-8098-tpm-stale-native-round1-v1` is forbidden. The operator must
use a fresh apply key created after deployment and tied to the new preview hash.
Neither this record nor implementation may pre-consume that key.

Production apply proceeds only if the new preview and immediate headroom
preflight return every exact current pin, placement and CAS value required
above. Success requires durable receipt/readback proving unchanged logical
binding/ECP/route/pins, immutable generation-1 history and one active
generation-2 successor. The authorized completion owner may then use the
generation-2 scoped TPM credential through the existing completion API; that is
a separate operation and receipt.

## Required refusal and recovery tests

Implementation must leave focused runnable tests proving:

1. the exact stale predecessor produces immutable generation-1 history and one
   active generation-2 successor while the logical `SeatBinding`, ECP, route and
   pins remain unchanged;
2. a present predecessor that is running, non-idle, permission-blocked or not
   the exact fenced native refuses before archive;
3. mismatched `SeatBinding` revision, occupancy generation, predecessor native
   id/runtime generation/provider session, ECP project/workspace/cwd/container
   generation, approved route/digest, Team Definition digest, topology pin,
   Core Team roster/catalog pin or completion-profile digest each refuses with
   zero effects;
4. missing, stale, exhausted, ambiguous or wrong-account provider headroom
   refuses before archive, and apply cannot reuse preview-time capacity as its
   immediate preflight;
5. an exact replay with the same fresh key returns the same receipt and performs
   no probe, archive, launch or store mutation; the same key with a changed
   intent refuses;
6. injected lost acknowledgements after archive, launch, store replacement and
   receipt persistence each converge on the same one successor and receipt;
7. exact post-apply readback includes both generations, native identities,
   placement, route, pins and receipt digest;
8. generation-2 TPM authority can submit the existing round-1 remediation while
   generation-1 authority is rejected at submission time;
9. existing non-stale route correction and replay behavior remain green;
10. OpenAPI, MCP registry and generated client contracts remain in sync if the
    DTOs gain fields.

Use isolated store/runtime fixtures. The production ASMA-8098 state and Paseo
runtime are not test fixtures. Kill one targeted mutant that removes an apply
CAS comparison or moves the headroom check after retirement, then restore the
source and rerun the focused test green.

## Smallest implementation surface

Expected files are limited to the existing flow and its focused contracts:

- `crates/kontor-api/src/applications.rs`;
- `crates/kontor-daemon/src/applications.rs`;
- `crates/kontor-runtime-paseo/src/adapter.rs` only if exact placement evidence
  is not already returned to the daemon;
- `crates/kontor-store/src/repository.rs` only if the existing atomic
  replacement/receipt readback cannot express the additional CAS;
- focused daemon/runtime/store tests and generated OpenAPI/MCP/client artifacts
  required by an actual contract change.

Reuse the existing route operation, headroom predicate, runtime correlations,
hosted-seat history and receipt machinery. A schema migration, new endpoint,
new dependency or console workflow is out of scope unless required by a failed
acceptance test and recorded before implementation expands.

## Prohibitions

- No direct Paseo API, UI or state-file mutation.
- No project, workspace, topology node, TeamRun, AgentRun, ECP or logical
  `SeatBinding` replacement.
- No adoption or mutation of an unrelated user-launched native session.
- No archive of any native other than the exact fenced idle predecessor.
- No account/model/reasoning failover and no relaxation, republish or migration
  of frozen Team Definition, topology, Core Team or completion-profile pins.
- No direct change to ASMA-8098 completion state from ASMA-8187.
- No reuse of the forbidden round-1 apply key or pre-deployment preview hash.
- No change to gate-rejection routing and no overlap with its ownership.
- No production-specific branch in generic code; the ASMA-8098 identifiers are
  post-deployment acceptance inputs, not hard-coded implementation behavior.

## Open questions

None. Values intentionally absent from this record are live authoritative
readbacks that preview must expose and apply must compare; inventing them in the
scope artifact would weaken the fence.

## Handoff

Implement this record through the existing TeamRun
`01a0a945-4a98-70b1-a16d-b087017edb0d` and already-bound implementation
AgentRun `01a0a946-f58a-7f32-ba69-23b64e3ac185`. Preserve concurrent work,
start with the focused refusal/replay tests, make the minimum change in the
existing Core Team route flow, and return `high-change` plus verification and
mutation evidence. Do not create replacement topology or another implementation
seat.

