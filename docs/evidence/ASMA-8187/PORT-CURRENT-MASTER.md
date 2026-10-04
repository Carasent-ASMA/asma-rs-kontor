# ASMA-8187 — Core Team route succession, ported to current master

Date: 2026-09-27
Artifact: port evidence (additive; historical bodies unchanged)
Task: Jira `ASMA-8187` / Kontor `01a0a943-652b-79d0-9f0c-f4965a33d7ab`
Epic: `ASMA-8186`
Approved plan: superproject `50d084afbc7e0a9c08db2571ae6ffc7619ae823d`
Port base: module master `173d399bfd44ec598332b4fe1cdf882af024fe5c`
(tree `85536418e4fe240144f6779858ba13a895f1b054`, schema 119)
Working branch: `feat/ASMA-8187-current-core-team-succession`
Preserved old lineage: `6bef9d64dad522a18ca29c82e2fdee50ed5485e3` (local and origin)

## Why a port and not a merge

The old branch is a divergent 48-commit lineage; current master is 92 commits
ahead of their common base. It is neither merged nor cherry-picked. What is
reconstructed is only the owned capability, on current APIs.

Master had already absorbed most of the surrounding work — the placement proof,
`proves_hosted_predecessor_absent`, per-generation role personas, frozen
autonomy, launch intents and the `0109` launch-intent supersession. The one
piece genuinely missing is the succession ledger: master's own
`0109_launch_intent_supersession.sql` header says it re-expressed *only* the
supersession half and defers `core_team_route_successions` as "a separate
concern". Grepping master, that name appeared only in that comment.

## Migration

Allocated **0120**, checked free across all 186 remote branches rather than
merely observed free. `SCHEMA_VERSION` 119 → 120. The old bundled migration is
not transplanted; this one carries the ledger alone.

## Acceptance interpretations, as implemented

Recorded here additively as the root decisions binding this port.

1. **Credential readback is non-secret identity only.** A seat credential is
   derived from the operator secret, so neither it nor a digest of it is
   recorded. The readback carries the *subject* a grant is scoped to — the
   public `(seat binding, occupancy generation)` pair — and a digest over
   exactly that pair. The node is named `grant_subject`, not `credential`:
   the realm's canonical-document guard forbids a node called `credential`
   outright, and the first implementation was refused by that guard. The rename
   is both safe and more accurate.

2. **Grant isolation.** The successor's credential generation is its own
   occupancy generation, enforced by a trigger
   (`successor_credential_generation <> successor_occupancy_generation` aborts).
   The predecessor's grant is not copied or widened because it is not recorded
   at all, and its subject digest differs by construction. Proved with synthetic
   authorization assertions; no live grant or runtime mutation.

3. **Durable completion includes the trailing effects.** A route commit alone is
   not the whole succession. `launch_intent_installed` and
   `seat_binding_observed` are separate latched columns; `is_complete()` requires
   both. A replay of a committed-but-incomplete succession *reconciles* those
   effects rather than reporting success. Effects are deliberately **outside**
   the hashed readback — they are live ledger state, not evidence of what the
   command did — and are answered as `succession_effects` on the outcome.

4. **Exclusivity precedes the duplicable effect.** The claim is taken before the
   retire and the launch, and `ux_core_team_route_succession_owner` is unique per
   `(project, seat, predecessor occupancy)`. A uniqueness index rather than an
   application check, because two concurrent transactions both pass a check and
   only one passes an index. The native-activity read lock only serialises, and
   a compare-and-swap after the launch is too late — the second native already
   exists.

5. **Export/import completeness.** The ledger joins realm export at generation
   **13** (`EXPORT_SCHEMA_VERSION` 12 → 13) with guards both ways: an older
   generation carrying succession records is refused, and an older generation
   claiming to represent a schema-120 database is refused. Older documents
   back-fill the array empty so they still parse.

6. **Preservation.** `0109` behaviour, the persona/routing/lineage/headroom/
   placement/pin fences and every existing report are untouched. Terminal
   predecessor `f8c211e4-e41e-4897-ba58-4656eb35192e` is not referenced by this
   port.

## Regressions added

| Test | Witnesses |
| --- | --- |
| `two_fresh_keys_converge_on_one_core_team_successor_and_one_transition` | one successor, one history entry, one occupancy step, one ledger row, no extra mint; plus the claim refusing a second key directly |
| `a_succession_owned_by_another_key_refuses_before_any_effect` | the concurrent half — refusal lands before retire/launch, nothing minted, seat unmoved |
| `a_successor_derives_its_own_generation_scoped_credential_subject` | grant subject, generation 2 ≠ 1, reproducible public digest, no bearer-like string in the durable row, recorded predecessor identity/generation |
| `a_committed_succession_with_pending_effects_is_reconciled_by_replay` | pending effects entered through a deterministic seam, replay reconciles and binds |
| `a_replayed_succession_answers_with_its_own_durable_readback` | same successor, same evidence, same receipt, zero runtime calls |

The pending-effects interval is entered through a new `fault-injection` cargo
feature on `kontor-store` — off by default, enabled only on `kontor-daemon`'s
dev-dependency edge. It cannot be staged by un-latching afterwards: the ledger's
own trigger refuses that, because an effect that has landed may never un-land.

## Mutation evidence

Each seeded alone and restored; the daemon source compared byte-identical
afterwards and zero `MUTANT` markers remain.

| Plan-suggested mutant | Result |
| --- | --- |
| inherit the predecessor grant (`successor_credential_generation` ← predecessor) | **killed** |
| accept a wrong native generation (recorded predecessor generation + 1) | **killed** |
| retire before a refused preflight (claim refusal made non-fatal) | **killed** |

Two of the three initially **survived** and are reported as such rather than
quietly re-seeded: the wrong-generation mutant survived until a test asserted
the ledger's recorded predecessor identity, and the preflight mutant survived
until a test made the *daemon's* claim refusal the thing that stops the effects.
Both gaps were real, and both regressions exist because the mutants found them.

## Suite results

```text
core_team 9/9 · succession 4/4 · stale 10/10 · replay 36/36
launch_intent 6/6 · hosted_seat 2/2
kontor-store schema_v1        64/64   (includes the 119 → 120 upgrade and reopen)
kontor-store backup_export    25/25   (round trip and completeness)
cargo fmt --all -- --check    exit 0
openapi_contract              3 passed; client regenerated
```

## Qualification frontier

Proven here: the ledger's structure, exclusivity, replay/reconciliation,
non-secret grant subject, schema upgrade and export round trip, against the fake
runtime and synthetic fixtures.

Not proven here, and not claimed: any live succession, any deployed behaviour,
any credential enrollment, and independent verification. The earlier rejected
verification at `7864a3ca` remains immutable FAIL history and is not converted
by this port. No gate, Jira, deployment or closure action was taken.
