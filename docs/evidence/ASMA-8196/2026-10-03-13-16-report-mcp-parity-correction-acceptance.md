# ASMA-8196 MCP parity correction independent acceptance

> **Date:** 2026-10-03 13:16 CEST
> **Status:** 🟢 Approved
> **Author:** Codex AUD seat
> **Category:** report
> **Scope:** Independent acceptance of the MCP parity correction at `d162f5045fc40f356cf7f4999ee2451e4ca0a1b6`
> **Summary:** Accepts the two Observer-tier MCP mappings for the merged ASMA-8196 read port after independent source inspection and affected-gate execution. Preserves the base failures and procedure deviations as provenance without treating this record as publication, merge, deployment, or runtime acceptance.

---

## When to Load

**Load this document when:**

- deciding whether the ASMA-8196 acceptance extends to the MCP parity correction at `d162f504`;
- checking the mapping, authority, canary, or preserved failure provenance for the two epic Core Team reads.

**Do NOT load for:** source publication, PR, merge, deployment, Jira, admission, permission, or live-runtime claims.

---

## Verdict

`accept`

The ASMA-8196 acceptance extends to source head `d162f5045fc40f356cf7f4999ee2451e4ca0a1b6` as the accepted MCP parity correction. There are no new findings.

The reviewed source tree is `c09139c109da2d83949b7391f81bf43d38f83d1e`, with exact parent/default `c93b1e43e482a6bd89d5bd3cbc87e1f8eb051845`. The consumed QA PASS is the docs-only evidence commit `86c69289891ddadfa6dc5342320a9162c2cff2cd`, whose parent is the reviewed source head. The root affected-test receipt independently pins the same source, tree, and base; it was supporting evidence and explicitly did not grant this acceptance.

Independence basis: this was performed by the Codex AUD seat, not by an author of either the correction or its QA verification. The auditor read the pinned source, base, QA record, and root receipt, then ran the affected gates from a separate archive and target directory.

## Delta and mapping decision

The isolated `c93b1e43..d162f504` delta changes only:

| Path | Delta | Candidate blob |
| --- | ---: | --- |
| `crates/kontor-mcp/src/registry.rs` | +50/-0 | `80430cdd08e606cb566582c810ebce63a8e20702` |
| `tests/contract/mcp_parity.rs` | +11/-4 | `e76a4fa8ccab2e7d34002c9ba04b06d2fecf9029` |

There is zero diff in `crates/kontor-api`, `crates/kontor-daemon`, and `crates/kontor-store` against the base.

| Tool | HTTP contract and handler | Required path arguments | Decision |
| --- | --- | --- | --- |
| `kontor_epic_core_team_get` | GET `/v1/projects/{project_id}/epics/{epic_id}/core-team`; router targets `epic_core_team`; handler requires `CallerCapability::Observer` | `project_id: ProjectId`, `epic_id: EpicSelector` | `CallerTier::Observer`, `Method::Get`, `OpKind::Read`; correct and complete |
| `kontor_epic_core_team_seat_occupancies_get` | GET `/v1/projects/{project_id}/epics/{epic_id}/core-team/seats/{seat_binding_id}/occupancies`; router targets `epic_hosted_seat_occupancies`; handler requires `CallerCapability::Observer` | `project_id: ProjectId`, `epic_id: EpicSelector`, `seat_binding_id: SeatBindingId` | `CallerTier::Observer`, `Method::Get`, `OpKind::Read`; correct and complete |

Neither mapping declares a body or idempotency argument. The base and candidate `NON_AGENT_ROUTES` extracts are byte-identical, with SHA-256 `d68d348a9c17edde1fca369b800e5e62c4a3de6e9faa60b7576809d5076503fb`, and remain the two non-agent GET routes. The candidate arithmetic is internally exact: `REGISTRY = 194`, `CLI_ONLY = 1`, advertised tools `= 193`, and documented operations `= 195`; therefore `documented == REGISTRY + 1` is `195 == 194 + 1`.

## Preserved provenance

- The QA archive reproduction on base `c93b1e43` remains authoritative correction provenance: the two named parity tests produced 0 passed, 2 failed, 10 filtered, exit 101. One failure named both unmapped GET operations; the other observed 195 documented operations against the frozen 193. I read the actual base registry and assertions and did not duplicate that already exact base run.
- The writer's initial tier-table failure remains on record. The accepted candidate closes it by adding both tools to the independent expected-authority table as `Observer`; this is provenance, not a surviving finding.
- The disclosed raw `git worktree add` remains an out-of-workflow procedure deviation. The root receipt records the existing LSA procedure disposition as pending and grants no retroactive authorization. This source acceptance neither reopens nor resolves that separate disposition.

## Exact acceptance checks

Source execution used archive `/private/tmp/asma-8196-aud-parity-d162-codex` and fresh target `/private/tmp/asma-8196-aud-parity-target-d162-codex`.

| Check | Result |
| --- | --- |
| `cargo test --offline --locked -p kontor-mcp -p kontor-tests-contract` | PASS, exit 0: 178 passed, 0 failed, 0 ignored; includes `mcp_parity` 12/0 |
| `cargo test --offline --locked -p kontor-api --test openapi_contract` | PASS, exit 0: 3 passed, 0 failed |
| `cargo fmt --all -- --check` | PASS, exit 0, no diff |
| `cargo clippy --offline --locked -p kontor-mcp --all-targets -- -D warnings` | PASS, exit 0 |
| `cargo clippy --offline --locked -p kontor-tests-contract --all-targets -- -D warnings` | PASS, exit 0 |
| `git diff --check c93b1e43..d162f504` | PASS, no output |
| Protected-crate diff and extracted allowlist comparison | PASS, both exit 0 with no diff |

## Boundaries

This review adds only this evidence record. It changes no source or prior evidence and makes no source-publication, push, PR, merge, deployment, Jira, admission, permission, integration, or live-runtime claim.
