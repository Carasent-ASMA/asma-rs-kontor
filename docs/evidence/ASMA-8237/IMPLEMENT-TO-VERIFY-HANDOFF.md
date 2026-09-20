# ASMA-8237 implement → verify handoff

Date: 2026-09-20
Task: Jira `ASMA-8237` / Kontor `01a0b77d-110f-7401-ab3d-ca36da7ab5f7`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
TeamRun: `01a0bfd6-cb03-7160-a381-3d561b48e2bd` / TSW `wks_f4bc61a339794e3a`
Governing record: `high-scope-record-asma-8237-20260920` rev 1,
revision_id `01a0bfde-c2fe-7051-a52b-8b6d5a080a40`,
approval `01a0bfde-d8c0-70d1-b2f3-078d038ddfec`.
Scope turn settled: `01a0bfe8-2813-78d2-a06c-f4ba18374efa`.
Worktree: `asma-modules/.worktrees/asma-8237/asma-rs-kontor`, branch
`feat/ASMA-8237-kontor-model-catalog-add-luna-glm-5-3-flash-cursorauto-nemotron-publish-approved-watchdog-chain-poli`.

**This document records no gate verdict.** It hands evidence to independent
verification.

## What this seat changed

One file, `crates/kontor-daemon/src/applications.rs`:

1. **`model_route_is_catalogued` (`:12581`)** — four added match arms making the
   six approved watchdog fallback routes recognizable at exactly the advertised
   effort ceilings. This resolves OQ-002.
2. **Two unit tests** in `applications::tests` — one negative/request-level
   refusal test (resolves OQ-001), one positive/ceiling test (guards the
   repair).

Commit `9f61c74a` was **not** duplicated: `git cherry -v origin/master` returns
`- 9f61c74a…`, i.e. already patch-equivalent upstream. No topology, schedule,
watchdog, migration or policy file was created or altered.

## OQ-001 — the refusal evidence the scope record asked for

The scope record asked: return existing governed rejection evidence if catalog
omission is the enforced boundary, else add the minimum focused request-level
refusal test.

Omission from the advertised catalog (`model_catalog`, `GET /v1/catalog`) is
**not** the enforced boundary. The enforced boundary is
`model_route_is_catalogued`, consulted on write paths, with refusal surfaced by
`parse_runtime_model_route` (`:12773` → `:12790`) as
`DomainError::invalid("RuntimeModelRouteRequest", "the model route is not in
the governed catalog")`, and a second site at `:12858` for team-draft rungs.

No pre-existing test asserted either refusal — grepping both rule strings
across `crates/` and `tests/` returned only the source sites. There was
therefore no existing evidence to return, so the minimum focused test was owed
and added:
`requesting_a_model_the_runtime_does_not_list_is_refused_by_the_governed_catalog`.

## OQ-002 — the defect this seat found and repaired

Every one of the six routes `9f61c74a` advertises measured `admitted=false`
against `model_route_is_catalogued` — indistinguishable from an invented alias
on every write path, including `provider_fallbacks` admission
(`crates/kontor-daemon/src/runtimes.rs:558`), which is the surface those
fallback routes would be installed through.

Re-verified against **current** `origin/master` `0f649824` (two commits past the
`e9184291` the scope record captured): the predicate is byte-identical and the
mismatch still exists. Repaired under explicit direct-repair authority.

### Scope of the repair — recognition only

Preserved unchanged, and asserted nowhere in the new tests: account headroom,
role eligibility, provider availability (`provider_available`), template
authority, and the approved watchdog chain. `default_model_for_provider` was
deliberately **not** given a `cursor` entry, so cursor cannot become a family
fallback default. Invented aliases and off-ceiling efforts — including the
invented Nemotron `max` — remain refused.

## Mutation evidence (each mutant seeded, observed red, reverted)

| # | Seeded defect | Expected catch | Result |
|---|---|---|---|
| 1 | `_ => false` → `_ => true` | negative test | red — `Ok(ModelRung{…"glm-5.3-flash"})` |
| 2 | dropped `gpt-5.6-sol` from codex arm | admitted control | red — control refused |
| 3 | removed `cursor/auto-smart` arm | positive test | red — "approved chain route cursor/auto-smart@high is nameable" |
| 4 | Nemotron ceiling + `"max"` | ceiling test | red — `Ok(…effort: Some(Max))` |

Mutants 1–2 were run against the negative test before the repair existed;
3–4 against the positive test after it. Every revert was followed by `touch`
so the rebuilt binary could not retain a mutant.

## Verification runs (after final revert, formatted)

| Command | Result |
|---|---|
| `cargo test --locked -p kontor-daemon --test loopback_api the_model_catalog_ -- --nocapture` (record's exact command) | 2 passed; 0 failed; 343 filtered out |
| `cargo test -p kontor-daemon --lib` | 83 passed; 0 failed |
| `cargo test --locked -p kontor-daemon --no-fail-fast` | see pre-existing failures below |
| `cargo fmt -p kontor-daemon` | clean (per-crate; the workspace is not fmt-clean at every commit) |
| `cargo clippy --locked -p kontor-daemon --lib --tests` | clean |

## Pre-existing failures — NOT caused by this change

`cargo test --locked -p kontor-daemon --no-fail-fast` reports 9 failures:
8 in `loopback_api` (Jira publication/body/placeholder/session-key) and 1 in
`mcp_journey::an_empty_realm_is_bootstrapped_through_mcp_tools_alone`.

These were baselined by restoring the pristine file at clean HEAD `9f61c74a`
and re-running the same targets: **the identical 9 failures occur without this
change** (`loopback_api`: 336 passed / 8 failed, both with and without). The
`mcp_journey` failure is `placement_blocked` — "the epic has no active
immutable backlog code" — unrelated to model routing.

Verification should treat these as inherited, and should not accept this change
as having fixed or worsened them.

## What verification should independently confirm

1. The four added arms match the approved record's effort ceilings exactly, and
   nothing else changed in `model_route_is_catalogued`.
2. No separate gate (headroom, eligibility, availability, template authority)
   was weakened or asserted-away.
3. The 9 failures above reproduce at clean HEAD, i.e. are genuinely inherited.
4. That failover itself is **not** claimed here — only that the chain's rungs
   are nameable.

## Current-master integration qualification (2026-09-20)

The recovery coordinator integrated only the three unique producer commits onto
`24290153c42c21743d606bcf5e1648681af4a69a` in isolated branch
`fix/ASMA-8237-integrate-route-recognition`. The only cherry-pick conflict was
the test import list; current hosted-seat autonomy and kickoff imports were
preserved alongside the new request-parser import. Production recognition and
both regression bodies are unchanged from the independently verified candidate.

On that integration base, all **88 daemon library tests** and both catalog
loopback tests passed. Daemon formatting, diff checks and
`cargo clippy --locked -p kontor-daemon --lib --tests -- -D warnings` passed.
The actual independent report from producer commit `cfae9767` is retained as
`HIGH-VERIFICATION-REPORT.md`; its four killed mutants and historical baseline
results refer to the exact source candidate recorded there. This integration
record does not claim deployment, automatic failover or watchdog execution.
