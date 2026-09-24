# ASMA-8237 high-audit report

Date: 2026-09-20
Artifact: `high-audit-report`
Task: Jira `ASMA-8237` / Kontor `01a0b77d-110f-7401-ab3d-ca36da7ab5f7`
Phase: `high-audit`
TeamRun: `01a0bfd6-cb03-7160-a381-3d561b48e2bd`
Auditor AgentRun: `01a0c031-c81f-7333-b455-38e876367239`
Auditor native: `1ae38e13-53b6-4e34-9615-2d94a355f420`
Decision: **PASS**

## Verdict

The bounded ASMA-8237 request-recognition correction passes audit with no P0 or
P1 findings.

The production delta adds only four match arms to the existing governed route
predicate. Those arms make exactly six approved provider/model routes nameable
at their explicit effort ceilings. The three rejected aliases and four tested
off-ceiling efforts remain refused. No fallback selection, automatic failover,
watchdog activation, schedule, topology, migration, runtime configuration or
default-provider behavior changed.

This verdict is recognition-only. It does **not** claim that automatic
pre-start failover executes, that the watchdog ran, or that the published chain
was activated.

## Frozen evidence boundary

| Evidence | Exact audited value |
| --- | --- |
| approved scope memory | `high-scope-record-asma-8237-20260920` revision 1, revision ID `01a0bfde-c2fe-7051-a52b-8b6d5a080a40` |
| scope export | commit `8e5890a67b48a6c78a5c9cc05bb60ec902af6ed1`, `docs/evidence/ASMA-8237/HIGH-SCOPE-RECORD.approved.json`, SHA-256 `7c310040ca685e305dd17f03ac0bce9031c522bb5f29a333eff6fe437f06f887` |
| registered scope evidence | `01a0c037-afe1-7ed3-8d02-c77155073376`, associated with settled scope turn `01a0bfe8-2813-78d2-a06c-f4ba18374efa` |
| implementation evidence head | `1e229b33aad855c2b1ff36ae0a2c11e152eb63af` |
| source correction | `da22b32398330c252d1f0e5bd3ac7487689a4449` |
| request-refusal regression | `6ab089ef87599c20d1fe08f390096e8d69c9d7e3` |
| registered change evidence | `01a0c024-977f-7d33-9f1f-0577eba4cc10` |
| verifier report | `cfae9767293496eb44f20dabd95f75443ddadd3d`, `docs/evidence/ASMA-8237/HIGH-VERIFICATION-REPORT.md`, SHA-256 `7ee810a59d9b5c453fc596b18c5b627383346bce68349045797c07986319082a` |
| registered verifier evidence | `01a0c027-1da1-7a72-a542-5967bf2c46a5` |
| verification gate | PASS receipt `01a0c02a-18c2-7ad0-a470-245fb56f258c`; task readback reports `high-verification-gate=passed` |
| release integration head | `68650a76a70c6639abd3d7d77ba0e4b49b81d1b3` |
| PR merge | PR #255, merge `f373565856c76ddc832bbbbfee2f2fbda217f39b`; integration head is its second parent |
| deployed source | `9dae7f790552bc4746255cd01d8ad1160cab7211`, a descendant of PR #255 |

The scope export is an operator serialization of the exact immutable document
returned by `kontor_memory_history`; its sidecar preserves the original
role-turn provenance and explicitly does not claim that the original producer
committed those file bytes. The original scope producer account remains
unchanged.

## Acting identity and account attribution

`kontor_run_get` read the live audit seat as role `audit`, attached generation 1
on native `1ae38e13-53b6-4e34-9615-2d94a355f420`, with
`account_profile_id: null`. That `NULL` producer pin is preserved.

The current Paseo native readback reports provider `codex/gpt-5.6-sol`, not an
alias-routed `codex-work` or `codex-personal` provider. The enabled project
account profile for the unaliased local Paseo harness is `Igor · Local Paseo`,
account `01a00751-5be9-7281-bba5-75d8c0c101e7`. This is the acting evaluator
account attribution; it does not rewrite the run's `NULL` producer pin.

## Source and scope audit

PR #255's first-parent delta contains only:

- `crates/kontor-daemon/src/applications.rs`;
- `docs/evidence/ASMA-8237/HIGH-VERIFICATION-REPORT.md`;
- `docs/evidence/ASMA-8237/IMPLEMENT-TO-VERIFY-HANDOFF.md`;
- `docs/evidence/ASMA-8237/OPEN-QUESTIONS.md`.

The only production behavior change is in
`model_route_is_catalogued`. The approved explicit effort sets are:

| Provider/model route | Explicit efforts admitted |
| --- | --- |
| `codex`, `codex-work`, `codex-personal` / `gpt-5.6-luna` | `low`, `medium`, `high`, `xhigh`, `max` |
| `cursor/auto-smart` | `low`, `medium`, `high`, `xhigh` |
| `opencode/openrouter/z-ai/glm-5.3-flash` | `low`, `high`, `max` |
| `opencode/openrouter/nvidia/nemotron-3-ultra-550b-a55b:free` | `medium`, `high` |

That is six provider/model routes: three Codex account aliases plus Cursor,
GLM, and Nemotron. The predicate's complete extracted body has SHA-256
`1308bdcec1499faf8f633cf1754444bf33fcaaf097dc45e8143c4af5959889ae`
at the verified candidate, integration head, and deployed source commit.

The request-level regression still refuses the guessed aliases
`opencode/glm-5.3-flash`, `opencode/nemotron-3-ultra:free`, and
`cursor/cursor-auto` with the governed error, while retaining an admitted
control. The ceiling regression refuses Nemotron `max`, Cursor `max`, Luna
`ultra`, and GLM `medium`. Its positive assertions establish only that the
approved routes are nameable.

The two regression bodies are byte-identical between the verified candidate
and integration head. Their extracted SHA-256 values are respectively
`000e64d11ff16c82d6ce5fa9d16a945474002c5b31cbd9902d9a5b6d2298ba5c`
and `a33d894a16ffd037171e89e91c45af8fc2ace4f11a741ee80bf4907943b65f1d`.
The integration conflict affected only the surrounding import list.

`default_model_for_provider` remains unchanged and still has no Cursor entry.
The inspected callers continue to preserve their separate controls:

- request and team-draft parsing still validate the route through the governed
  predicate;
- initial Committee recovery still requires a pinned slot and catalogued
  routes;
- materialization recovery still requires exact aliases, runtime validation,
  provider-family diversity, fresh provider-reported headroom, eligible
  accounts and the existing placement resolver;
- runtime `provider_fallbacks` still validate through the same predicate;
- provider availability remains part of placement resolution.

No caller or adjacent gate changed in the PR delta. The correction therefore
repairs one shared recognition seam without authorizing reachability or
execution.

## Independent audit checks

All commands ran in the isolated integration checkout at exact head
`68650a76a70c6639abd3d7d77ba0e4b49b81d1b3`.

| Check | Result |
| --- | --- |
| `cargo test --locked -p kontor-daemon --lib` | PASS, 88 passed / 0 failed |
| `cargo test --locked -p kontor-daemon --test loopback_api the_model_catalog_ -- --nocapture` | PASS, 2 passed / 0 failed / 427 filtered out |
| `cargo clippy --locked -p kontor-daemon --lib --tests -- -D warnings` | PASS |
| source correction stable patch ID, producer vs integration | equal: `1aea44421786f0f2951e567e27c8eab5ca20e6d3` |
| verifier report stable patch ID, producer vs integration | equal: `0df39adb44a6be616812bfa2ee290515d39ebe79` |
| live `kontor_model_catalog_get` | all six routes present with the approved explicit effort lists |

The verifier independently killed 4/4 mutants and restored a clean candidate.
This audit checked the exact report hash, the retained discriminating tests,
their unchanged integration bodies, the production predicate across candidate,
integration and deployed source, and reran the bounded integration suites. It
did not reseed the same four mutants.

`git diff --check` on the PR first-parent delta reports one extra blank line at
EOF in the exact verifier report. That document is already hash-addressed and
registered; the condition is non-executable and is not a release finding. This
audit does not rewrite it.

## Deployment and operational limits

The deployment receipt at
`/Users/igor/.local/state/kontor/asma/deployments/ASMA-7869-20260920T185808Z-9dae7f79/deployment.json`
records source `9dae7f790552bc4746255cd01d8ad1160cab7211`, verified at
`2026-09-20T18:59:21.759604+00:00`, live schema 114, successful quick check,
zero foreign-key violations, preserved identities, and watchdog status
`remains stopped`. Git proves that deployed source descends from PR #255 and
retains the same audited route-predicate body.

The published watchdog configuration independently reads `enabled: false`.
Its ordered chain uses the approved `xhigh`/`high`/`high`/`high` route efforts,
but neither that policy nor any activation surface changed in this PR.

## Findings and control-plane handoff

Current P0 findings: none.

Current P1 findings: none.

No unresolved acceptance ambiguity was found, so no new open-question ledger
entry is required. This audit produces only this committed report and its
approved memory record. It performs no source repair, release, Jira, lifecycle,
gate, turn-settlement, native cleanup, seat, schedule, watchdog or topology
mutation. The coordinator retains ownership of terminal turn settlement,
committed-body registration, gate recording and release state.
