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
Status: verification remediation, after
[`HIGH-VERIFICATION-REPORT.md`](HIGH-VERIFICATION-REPORT.md) rejected candidate
`1fcd42d`

## This turn: closing the four verification findings

High-verification gate sequence 1 rejected `1fcd42d` on four blocking findings.
Its green results stood; what it refused was missing recovery and authority
evidence. This section records the repair. Everything below it describes the
first implementation and is retained unchanged, except where a statement it
makes is now superseded — those are called out here rather than silently
rewritten.

Authoritative state this turn started from: `high-implementation` at workflow
revision 4, task revision 2, with `high-verification-gate` sequence 1 rejected.

### F-8187-V1 — closed: the store transition and its recovery evidence commit together

A new durable ledger, `core_team_route_successions` (schema v97, migration
`0097_core_team_route_succession_recovery.sql`), is written **inside the same
transaction** as the history append and the active-row replacement. There is no
longer an interval in which the seat has moved and the means to reconstruct the
command does not exist: `replace_hosted_topology_seat_route` takes the record
and commits both or neither.

Apply now consults that ledger *before* the receipt, because in the interval
this closes the receipt is exactly what is absent. A key that finds a row has
already moved the seat: the resume records the receipt it never got, binds it to
the row, and answers from the row's durable readback. `record` is keyed and the
row's trigger refuses a second binding, so the resume is safe to run on every
replay rather than only the one that recovers.

The ledger row is undeletable by trigger and its evidence is frozen on commit;
only `receipt_id`/`receipted_at` may move, once, from absent to present.

### F-8187-V2 — closed: the approved route and Team Definition are server-derived and fenced

The preview document gained two authorities, and the preview DTO now returns
them:

- **approved model route** — `approved_model_route` and `approved_route_digest`.
  The digest is over a server-assembled document: provider, model, effort, the
  runtime this realm places Core Team seats on, and the *governed account
  authority* this project resolves the provider to. The account is read from
  stored account profiles, never from the request.
- **Team Definition** — the epic's pinned `definition_id`, `version` and
  `canonical_hash`, from `get_mini_project_team_definition`. A distinct
  authority from the role catalog, the topology and the resolved Core Team,
  none of which speaks for which Team Definition revision governs the epic.

Apply re-derives both through the same plan and compares the hash, so drift in
either expires the preview before the first native effect.

One deliberate asymmetry. `approved_route_account` refuses ambiguity, which is
right where capacity is about to be spent, and the stale-native branch still
goes through it. The *fence* instead records whichever resolution the server
currently reaches — the account id, `ambiguous`, `none`, or `unreadable` — so a
resolution that changes expires the preview without inventing a new
precondition on the ordinary-correction path, which has never required a
governed account because the seat it corrects is still answering. The four
outcomes are distinguishable on purpose: collapsing `none` and `ambiguous` would
let a realm move from zero to two selectable accounts without disturbing the
hash.

### F-8187-V3 — closed: the readback is complete, durable and replayable

`CoreTeamRouteOutcomeDto` gained `readback` and `readback_hash`.
`CoreTeamRouteReadbackDto` carries what the scope requires and the previous
outcome did not: both native identities with runtime kind, host, runtime
generation and provider session; **both occupancy generations**; the exact ECP
placement including container native id and canonical `cwd`; the server-derived
approved route and its digest; every frozen pin (topology spec triple, Core Team
version/catalog/definition digests, Team Definition, completion profile); and
the retirement instant.

It is persisted, not recomputed. A replay deserializes the stored bytes and
answers with them, so a command that produced generation two still answers with
generation two after the seat has reached generation three. The digest is bound
to the receipt through the ledger row rather than folded into the command
intent: the intent must stay derivable from the request alone, which is what
lets the ledger be consulted before the fence at all.

The previous history-walk replay path is retained for corrections that replaced
no native and therefore recorded no succession.

### F-8187-V4 — closed: every named fence and all four loss points

`a_core_team_succession_refuses_drift_in_each_fenced_identity` replaces the
two-family representative test with twelve independently drifted cases, each
asserting refusal, no `RetireHostedSeat`/`LaunchHostedSeat` call, unchanged
durable seat shape, and no succession row:

`topology-spec-hash`, `binding-role-slot`, `binding-role-code`,
`predecessor-provider-session`, `predecessor-runtime-generation`,
`occupancy-generation`, `ecp-container-native-id`, `ecp-container-generation`,
`ecp-canonical-cwd`, `core-team-version`, `core-team-catalog-hash`,
`core-team-definition`, `completion-profile-digest`, `team-definition-digest`.

Zero-effect is asserted as a before/after comparison captured *after* the drift
is staged, so a case that stages history of its own is still held to "this apply
changed nothing".

Two named fences are deliberately absent, with reasons rather than omissions:

- `topology_nodes.spec_version` and `seat_bindings.role_catalog_id`/
  `role_catalog_version` are foreign keys onto the published spec and catalog.
  They cannot drift alone in a database state SQLite would accept; the free
  columns of both families (`spec_hash`, `role_slot_id`, `role_code`) are
  covered.
- The TeamRun absence fence has no stageable drift: giving a control-plane seat
  a TeamRun requires a real `team_runs` row, and no endpoint produces that
  state. It remains structurally fenced through the preview hash and untested
  by a focused case.

The approved-route authority is covered by its own test rather than a SQL
drift, because it moves through the API:
`a_core_team_succession_refuses_an_approved_route_authority_that_moved` previews
against one enabled account, enables a second for the same alias, and proves the
apply refuses with no effect.

Acknowledgement-loss points, all four:

| Point | Test |
| --- | --- |
| after archive | `a_lost_archive_acknowledgement_converges_on_one_core_team_successor` |
| after launch | `a_lost_launch_acknowledgement_recovers_the_same_core_team_successor` |
| **after store replacement** | `a_succession_lost_after_its_store_commit_converges_on_one_receipt` |
| **after receipt persistence** | `an_exact_replay_after_the_receipt_landed_answers_from_durable_evidence` |

The third stands inside the interval deliberately, through a new
`fault-injection` cargo feature on `kontor-store`. `lose_next_core_team_succession_ack`
lets the transaction commit and then fails the caller, which is what a process
death immediately after the commit looks like from the daemon. The feature is
off by default and enabled only on `kontor-daemon`'s dev-dependency edge, so a
release build carries neither the field nor the branch. The fourth needs no
seam: a landed receipt with a lost response *is* a same-key retry, and the test
asserts an identical readback, the same receipt id, zero runtime calls and
unchanged durable state.

### Contract artefacts, this turn

`CoreTeamRouteReadbackDto`, `CoreTeamRouteOccupantDto`,
`CoreTeamRoutePlacementDto`, `CoreTeamRoutePinsDto`,
`CoreTeamRouteTopologyPinDto` and `CoreTeamRouteCompletionPinDto` were added and
registered; `PinnedTeamDefinitionDto` gained `Deserialize` so a stored readback
rehydrates. Regenerated with the same two commands as before; both artefacts
reproduce with no drift.

### Sequence-2 remediation (V6/V7/V8) and dual-lineage integration

Gate sequence 2 (receipt `01a0b640-8343-7cf2-8d95-655478796422`) routed the task
back to `high-implementation` at workflow revision 6. This section records that
turn.

**V6 — the canonical preview intent is exposed.** `preview` returns
`preview_intent`, the exact canonical document `preview_hash` is taken over, and
`preview_intent_schema_version`. One canonicalization produces both, so a caller
re-deriving the digest checks the bytes the server hashed.

**V7 — placement identity corrected and completed.** The field that called the
Kontor project UUID `native_project_id` was wrong and is renamed `project_id`.
Beside it the placement now carries `native_parent_project_id` — Paseo's own
`prj_*`, resolved through the persisted container-binding ancestry walk that the
retitle, archive and recovery requests already use. It is resolved *before* the
preview document is canonicalized, so it is inside the compare-and-swap intent,
and it is persisted as its own ledger column. Replay compares the readback's
parent against that independent column, `Option` shape included, so a rehashed
wrong-parent readback is refused. Placement also gained container runtime kind,
host, generation and the fenced provider correlation.

**V8 — the live ASMA-8098 shape.** The TPM recovery previewed cleanly and then
refused at apply with 409 `stale_binding` for a *closed* predecessor, because
only `CorrelationFailed` was read as absence. `RuntimeError::proves_hosted_predecessor_absent`
now classifies a closed list of terminal/missing `StaleBinding` rules as absence,
in both the plan and the pre-archive apply inspection. The list is matched
exactly and fails closed: a working, permission-waiting, wrong-runtime,
wrong-generation or unaudited disposition still refuses with no effect, and every
CAS and identity fence is unchanged.

**Authority drift isolated.** The approved-route test now enables a second
account *without* seeding a provider report, so authority drift is separated
from headroom drift, and it asserts the exposed digest moves — which is what
kills an authority-removal mutant.

**Dual-lineage integration.** The live realm runs `b84315cd` on
`origin/chore/ASMA-8190-combined-integration-head` at schema 105; that line is
not an ancestor of `origin/master` `ae8b401f` (31/2, master at schema 99). Both
are now ancestors of this branch. The deployed line keeps 0100–0105 unchanged and
the ASMA-8187 migration became **0106**, so a live schema-105 realm upgrades by
exactly one migration. Proven on a `.backup` copy of the live realm: 105 → 106,
`integrity_check` ok, `foreign_key_check` clean, both new tables present, the
live file untouched at 105.

### Validation run this turn

Observed first-hand, on the exact tree this commit contains:

```text
$ cargo test -p kontor-daemon --test loopback_api core_team
PASS: 18 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api succession
PASS: 11 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api \
    a_core_team_succession_refuses_drift_in_each_fenced_identity -- --exact
PASS: 1 passed, 0 failed (12 independently drifted cases)

$ cargo test -p kontor-daemon --test loopback_api \
    a_succession_lost_after_its_store_commit_converges_on_one_receipt -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api \
    an_exact_replay_after_the_receipt_landed_answers_from_durable_evidence -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-daemon --test loopback_api \
    a_core_team_succession_refuses_an_approved_route_authority_that_moved -- --exact
PASS: 1 passed, 0 failed

$ cargo test -p kontor-store --test operational_liveness
PASS: 11 passed, 0 failed

$ cargo fmt --all -- --check
PASS: exit 0

$ KONTOR_UPDATE_CONTRACT=1 cargo test -p kontor-api --test openapi_contract
PASS: 3 passed, 0 failed

$ pnpm --filter kontor-console generate:api
PASS: openapi-typescript 7.13.0 regenerated apps/console/src/api/schema.d.ts
```

The regenerated contract diff is additive: +268 lines in `openapi.json`, +121 in
`schema.d.ts`, no removals.

### Validation NOT run this turn

This remediation was bounded to the focused evidence above. The following were
part of the verification exit criteria and have **not** been run against this
candidate; they are the verify seat's to execute and classify, and nothing in
this record should be read as claiming them:

- the full `kontor-store` suite — a duplicate invocation deadlocked on the cargo
  build lock and was terminated. Only `operational_liveness` (11/11) was
  observed green; the remaining groups are unobserved, not passing;
- `cargo test -p kontor-api -p kontor-core -p kontor-runtime -p kontor-runtime-paseo`;
- `cargo test -p kontor-mcp`;
- `cargo clippy` on the touched crates and on the workspace;
- the full `cargo test -p kontor-daemon` suite;
- exact-parent reproduction of the inherited baseline failures;
- the three named mutants from the first implementation, and any mutant covering
  the new ledger, authority or readback paths.

### Inherited baseline

Unchanged from the first implementation and untouched here: the seven
pre-existing `kontor-daemon` failures, the `kontor-tests-e2e` compile break from
ASMA-8116, and the repository-wide `Cargo.lock` reproducibility gate. Per the
gate instruction, classifying them is the independent verify seat's task with
exact-parent reproduction; this turn neither repaired nor re-classified them.

### Superseded statements below

- "no schema migration" in *What this change is* no longer holds: this turn adds
  schema v97. The rest of that paragraph stands — still no new endpoint, no
  recovery state machine, no parallel headroom abstraction.
- The *Replay and convergence* section's account of replay resolving its
  successor from append-only history now describes only the no-replacement
  path. A succession that replaced a native answers from its ledger row.

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
