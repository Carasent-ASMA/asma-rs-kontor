# ASMA-8196 MCP parity verification

Date: 2026-10-03
Task: Jira `ASMA-8196` (Paseo-direct)
Branch: `fix/ASMA-8196-mcp-parity`
Source head: `d162f5045fc40f356cf7f4999ee2451e4ca0a1b6`
Parent and base: `c93b1e43e482a6bd89d5bd3cbc87e1f8eb051845`

Tests ran from `git archive` snapshots under `/private/tmp/asma-8196-mcp-qa/`, with `CARGO_TARGET_DIR` set to `/private/tmp/asma-8196-mcp-qa/target-base` and `target-head`. This record does not replace earlier ASMA-8196 evidence. It is not a merge or a deployment claim.

## Verdict

`PASS` for the MCP parity correction on `d162f504`.

The two epic Core Team reads were unmapped at the base. The candidate maps them as Observer GETs. Routes, handlers, DTOs, OpenAPI, and the store are unchanged. The parity suite, the contract crate, the OpenAPI contract test, `cargo fmt --check`, and clippy all passed on the candidate archive.

## Claims

| Claim | Result |
|---|---|
| Base `c93b1e43` fails the two named parity tests | **Pass** (failures reproduced) |
| Candidate `mcp_parity` is 12 passed, 0 failed | **Pass** |
| Both ToolSpecs match the HTTP handlers at `applications.rs:10188` and `:10222` | **Pass** |
| `NON_AGENT_ROUTES` is unchanged | **Pass** |
| Canary arithmetic `documented == REGISTRY + 1` equals 195 | **Pass** |
| `kontor-api`, `kontor-daemon`, and `kontor-store` have zero diff against the base | **Pass** |
| Contract crate, OpenAPI contract, fmt, and clippy | **Pass** |

## Handlers

Both entry points are in `crates/kontor-api/src/applications.rs` on the candidate archive.

`epic_core_team` at line 10188 requires `CallerCapability::Observer`, then parses `ProjectId` and `MiniProjectId` from the path pair `(project_id, epic_id)`. The route is `GET /v1/projects/{project_id}/epics/{epic_id}/core-team`.

`epic_hosted_seat_occupancies` at line 10222 requires the same capability, then parses `ProjectId`, `MiniProjectId`, and `SeatBindingId` from `(project_id, epic_id, seat_binding_id)`. The route is `GET /v1/projects/{project_id}/epics/{epic_id}/core-team/seats/{seat_binding_id}/occupancies`.

`kontor_epic_core_team_get` and `kontor_epic_core_team_seat_occupancies_get` are `CallerTier::Observer`, `Method::Get`, `OpKind::Read`, and declare those path arguments as required. `project_id` is `ArgType::ProjectId`. `epic_id` is `ArgType::EpicSelector`, the registry rule for an addressed epic path subject, the same type `kontor_core_team_materialize` uses. `seat_binding_id` is `ArgType::SeatBindingId`. Neither tool takes a body or an idempotency header. `every_tool_declares_the_same_parameters_the_contract_does` and `the_tier_of_every_tool_is_the_one_the_daemon_requires` passed.

## Allowlist and canary

The `NON_AGENT_ROUTES` slice is byte-identical between the base and candidate archives. It is still `GET /v1/health` and `GET /v1/openapi.json`. The canary still asserts length 2.

On the candidate, `REGISTRY` is 194, `CLI_ONLY` is still the one account-profile amend tool, advertised tools are 193, and `documented()` is 195. `194 + 1 = 195`. The extra documented operation is health. The OpenAPI document route stays allowlisted and is not counted in `documented()`.

## Base reproduction

Archive of `c93b1e43`, then:

```
CARGO_TARGET_DIR=/private/tmp/asma-8196-mcp-qa/target-base cargo test -p kontor-tests-contract --test mcp_parity -- --test-threads=1 every_documented_operation_is_mapped_once_or_allowlisted_once the_snapshot_canary_holds_at_this_base
```

**Fail**, as required. 0 passed, 2 failed, 10 filtered out, 0.06s. Exit 101.

`every_documented_operation_is_mapped_once_or_allowlisted_once` at `mcp_parity.rs:179` named both GETs, the epic core-team route and the seat occupancies route, as having neither a tool nor an allowlist entry.

`the_snapshot_canary_holds_at_this_base` at `mcp_parity.rs:563` got `documented().len()` 195 against the frozen 193. The mapped-count assertion (192) did not fire.

## Candidate commands

Working directory `/private/tmp/asma-8196-mcp-qa/head`, `CARGO_TARGET_DIR=/private/tmp/asma-8196-mcp-qa/target-head`.

`cargo test -p kontor-tests-contract --test mcp_parity`

12 passed, 0 failed, 0.09s. Exit 0.

`cargo test -p kontor-tests-contract`

104 passed, 0 failed, 0 ignored, across `guardrails` 5, `mcp_cardinality` 11, `mcp_mutants` 11, `mcp_parity` 12, `profiles_teams` 11, `runtime_adapter` 52, and `scheduling` 2. The library and doc-test binaries ran 0 tests. Exit 0.

`cargo test -p kontor-api --test openapi_contract`

3 passed, 0 failed, 0.06s. Exit 0.

`cargo fmt --all -- --check`

Exit 0, no diff.

`cargo clippy -p kontor-mcp --all-targets -- -D warnings`

Exit 0.

`cargo clippy -p kontor-tests-contract --all-targets -- -D warnings`

Exit 0.

`diff -rq` of `crates/kontor-api`, `crates/kontor-daemon`, and `crates/kontor-store` between the base and candidate archives exited 0 for each tree.
