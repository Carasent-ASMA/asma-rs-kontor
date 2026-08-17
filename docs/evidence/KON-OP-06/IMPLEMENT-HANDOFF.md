# KON-OP-06 implementation handoff — checkpoint 4

Date: 2026-08-17
Ticket: ASMA-7875
Against: `docs/evidence/KON-OP-06/ARCHITECTURE.md` (approved for implementation)

## What this change closes

Checkpoint 4 of the approved architecture: the durable composition behind the six
`/v1` completion operations, the polyrepo/closeout projections, and the exact-seat
TPM wake outbox with its declared bounded-polling fallback.

Checkpoints 1–3 landed in `ea95e10` as pure domain code — the profile shape, the
deterministic compiler, the revision-checked `advance` machine and the policy
predicates. Nothing in that commit was reachable from the API. This change
composes it.

## Landed

### Persistence — schema v32 (`0032_epic_completion.sql`)

Four tables, and the reason each is a row rather than process state:

| Table | Why it is durable |
| --- | --- |
| `completion_profile_revisions` | The canonical published bytes with their digest beside them. Preview hashes the document and apply may only publish the document that hash was taken over, so the bytes cannot be reassembled from normalized columns and hoped to be identical. |
| `epic_completion` | The pinned profile as columns beside the state document, so a restore can refuse a pin that disagrees with the profile it is handed without first decoding the state. |
| `epic_completion_wakes` | The primary key `(epic, completion revision, reason, seat)` *is* the one-wake-per-observation rule, so a replayed callback collides instead of opening a second turn. |
| `epic_completion_remediation_proposals` | The LSA half of a two-authority approval needs somewhere that is not the completion state — a half-filled authorization stored there would read as approved to every consumer of it. |

Three command kinds were added to the closed vocabulary:
`apply_completion_profile` (targets the project, like `apply_core_team`),
`advance_completion` and `remediate_completion` (target the epic). They are kept
distinct because an advance receipt must never be replayable as the authority
that launched a remediation round.

### Contract — the narrow OP-03 DTO corrections

- `CompletionStateDto.phase: String` and `outstanding: Vec<String>` are replaced
  by typed `phase`, `blockers`, `integrations`, `rounds`, `closeout`, `wakes` and
  `needs_human` projections.
- `RemediateCompletionRequest.reason` is replaced by a closed tagged
  `RemediationActionDto` — `lsa_proposal` or `tpm_route`.
- `AdvanceCompletionRequest` stays evidence-free, as specified.
- `CompletionProfile` and `PollingFallback` gained `deny_unknown_fields`, so an
  unmodelled key is refused *before* the definition is hashed.

**Deviation from the architecture text, deliberate.** §"Narrow OP-03 DTO
corrections" says to decode `ProfilePreviewRequest.definition` and
`ProfileApplyRequest.definition` as a strict `EpicCompletionProfileSpec`. Those
two DTOs are *shared* by `advisor-profiles`, `committee-templates` and
`completion-profiles`; retyping the field would force advisor and committee
definitions to be completion specs and break OP-05's contract. The strict decode
therefore happens inside the completion handlers, which is what the requirement
is actually for — unknown fields are refused before hashing either way.

`contract/openapi.json` was regenerated (`KONTOR_UPDATE_CONTRACT=1`). The
console's TypeScript types still need `pnpm --filter kontor-console generate:api`
— not run here, no Node toolchain was exercised in this change.

### Composed operations

| Operation | State |
| --- | --- |
| `GET /completion-profiles` | Answers. Built-in `operational_default@1` plus published revisions; catalog revision is its publication count. |
| `POST /completion-profiles:preview` | Answers. Strict decode, compile, violations, `preview_hash`; writes nothing. |
| `POST /completion-profiles:apply` | Answers. Recompiles, compares the preview hash, checks expected revision, publishes the next immutable revision, one receipt. |
| `GET /epics/{id}/completion` | Answers when a run exists; `404` when it does not. |
| `POST …/completion:advance` | Starts the run on first call, then commits one deterministic transition per observation it can authoritatively derive. |
| `POST …/completion:remediate` | Answers. Records the exact LSA proposal, then the exact TPM route; only both together launch a round. |

`operational_default@1` is a reserved built-in rather than a seeded row: a
per-project copy could not be corrected by a later build, and projects created
either side of a seed would carry different catalogs. Publishing under that id is
refused.

## Deliberately still refusing

Per the architecture (§"Successor contracts that gain behavior"): *"A partially
wired method must keep returning `Unavailable`; it must not return an empty
profile catalog or synthetic success."*

`completion:advance` derives only observations whose authoritative source is
composed in this build. Where the source does not exist it refuses with the
reason named:

- **Committee verdicts** — OP-05's consultation service is still an `Unavailable`
  stub, so no `CommitteeRun` exists and no round can settle. A synthesized pass
  would close an epic on a verdict nobody reached.
- **Integration and remediation TeamRun outcomes** — no completion-owned
  integration observation path is composed yet.
- **Closeout receipts** — recorded by native `kontord` connectors, uncomposed.

The ticket gate *is* derived, from composed sources: each task's pinned work
profile supplies the declared gates (goals) and artifact contracts (evidence
keys), and `artifact_evidence` supplies what is recorded. A task lifecycle value
is deliberately never consulted — `done` is a state a task can reach, not evidence
that the things it promised exist.

OP-08 assembles the OP-04/OP-05 services through these same ports without
changing any contract here.

## One defect found and fixed during implementation

The first cut checked `expected_revision` *before* recognising an idempotency
replay. A retry after a lost acknowledgement therefore presented a revision that
its own earlier effect had already moved, and was refused permanently — the exact
failure the receipt ledger exists to prevent. All three writes now judge the key
first and apply the revision guard only when nothing was replayed.
`advance_completion`'s canonical intent is keyed by the revision the *caller*
named, not the one standing now, so the retry reproduces the same intent.

## Verification

- `cargo test --workspace` — green.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo fmt --all --check` — clean.

New coverage:

- `kontor-store/tests/repository_roundtrip.rs` — immutable republication refused;
  a superseded-revision transition writes nothing and does not overwrite the
  winner; a replayed wake reuses its intent while a distinct revision/reason/seat
  does not; a second proposal for one round is refused.
- `kontor-daemon/tests/loopback_api.rs` —
  `completion_answers_from_its_own_repository_and_never_synthesizes`: the catalog
  answers with the built-in, an unstarted run is `404` and carries no projection,
  the built-in id cannot be shadowed, an unknown field is refused before hashing,
  and publish → replay → stale-revision behave as specified.
- The pre-existing `every_successor_contract_refuses_rather_than_answering_emptily`
  was renamed to `…every_uncomposed_successor_contract…` and narrowed to the OP-05
  routes, which still refuse. It previously asserted that OP-06's routes refuse;
  that assertion is now the opposite requirement and lives in the test above.

## Not in this change

Required proofs from the architecture that remain unbacked, because the
observations they need are not composed: the fail-remediate-pass restart matrix
against live TeamRun/Committee ports, the polyrepo integration fixture, the
closeout-prerequisite mutants, and the duplicate/lost-ack matrix across the
integration and verdict stages. The pure-domain equivalents of several of these
are already covered by `ea95e10`'s
`fail_remediate_pass_survives_restart_at_every_stage_and_closes_only_with_evidence`.
