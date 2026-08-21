# KON-OP-19 / ASMA-7967 — native topology naming mutation proof

> **Date:** 2026-08-20 23:06
> **Status:** 🟢 Verified
> **Category:** report
> **Scope:** `_tools/asma-rs-kontor` configurable native container and persistent-seat naming
> **Summary:** First-hand, one-at-a-time mutation receipts for the deterministic naming, restart recovery, replacement-chain and identity-preservation acceptance rules. Every mutant was restored immediately and its focused killer was rerun green before the next mutant.

---

## When to Load

**Load this document when:**

- reviewing ASMA-7967 naming correctness or its release gates;
- changing native-name rendering, whole-epic preview/apply, persistent-seat retitle, or replacement chains;
- verifying that the QNR-P1 repair and restart behavior are asserted rather than merely exercised.

**Do NOT load for:** unrelated Kontor scheduling, UI, or connector work.

---

## First-hand mutation kills

| # | Deliberate mutation | Killer test | Observed failure |
| --- | --- | --- | --- |
| M1 | Store `KONTOR_BACKLOG_CODE` under the `AI_SHORT_NAME` token, allowing the descriptive title path to displace the explicit short code | `the_backlog_code_wins_when_a_descriptive_ai_short_name_is_also_present` | killed: rendering refused with `missing KONTOR_BACKLOG_CODE` instead of producing `ESW • ASMA-7675 • QNR-P1` |
| M2 | Restore the U+00B7 middle-dot default separator | `the_v1_matrix_renders_exact_bullet_separated_bytes` | killed byte-for-byte: `[32, 194, 183, 32]` differed from the required U+2022 bytes `[32, 226, 128, 162, 32]` |
| M3 | Record the whole-epic `reconcile_native_names` receipt only after all runtime effects | `a_legacy_jira_import_materializes_semantic_epic_control_and_ticket_titles` | killed: after the first rename committed and its acknowledgement was lost, the same-key retry returned 409 `native names or identities changed since the caller's complete preview` instead of resuming without a duplicate mutation |
| M4 | Restore oldest-first `.next()` selection for a delivery role instead of resolving the unique current replacement-chain leaf | same whole-epic QNR regression | killed: the preview targeted the archived predecessor AgentRun/native id carrying `ARCHIVED PREDECESSOR` instead of the bound successor |
| M5 | Restore the initial Core Team formatter `ROLE · JIRA` | same whole-epic QNR regression | killed: initial LSA materialization received `LSA · ASMA-7675` instead of the pinned ECP seat template `LSA • ASMA-7675 • QNR-P1` |
| M6 | Restore the provider-route successor formatter `ROLE · JIRA` | same whole-epic QNR regression | killed: replacement LSA received `LSA · ASMA-7675` instead of `LSA • ASMA-7675 • QNR-P1` |
| M7 | Make whole-epic census container-only by suppressing every SeatBinding row | same whole-epic QNR regression | killed: the current delivery leaf was absent from preview, so the mixed container/hosted/delivery identity census failed |
| M8 | Restore Paseo's adapter-local `TSW · JIRA · short-code` formatter in `prepare_workspace` | `two_epics_share_one_plane_without_sharing_a_project_or_static_task_scope` | killed: Paseo received `TSW · ASMA-9001 · QNR-01` instead of the caller-rendered `TSW • ASMA-9001 • QNR-2` |
| M9 | Change the fake seat-generation bound from `persisted <= current` to strict `persisted < current` | `persisted_seat_generation_is_a_bound_and_future_generation_is_refused` | killed: an exact-current generation binding was rejected with `StaleBinding` |
| M10 | Apply the same strict-generation mutant to Paseo `preview_retitle_seat` | `seat_retitle_accepts_an_older_persisted_generation_and_refuses_a_future_one` | killed: exact-current generation preview was rejected before correlation readback |
| M11 | Suppress the empty-AgentRun guard so a declared logical seat is forced through replacement-chain leaf resolution | same whole-epic QNR regression | killed: preview returned 409 `a delivery role has no current replacement-chain leaf` instead of omitting the not-yet-native seat and repairing existing targets |
| M12 | Stop classifying an exact stale runtime seat as `rename_pending` | same whole-epic QNR regression | killed: preview returned 409 `the binding no longer names a session this runtime will act on` instead of retaining the exact identity as pending while repairing the independent stale TSW |
| M13 | Classify Paseo's exact `agent: null` readback as generic correlation drift | `seat_retitle_classifies_an_exact_missing_native_agent_as_stale` | killed: the contract observed `CorrelationFailed` instead of the typed `StaleBinding` that whole-epic repair preserves as `rename_pending` |
| M14 | Restore the persisted provider-session id as a strict native-name preview prerequisite after the same Paseo agent resumes onto a new provider thread | `a_legacy_jira_import_materializes_semantic_epic_control_and_ticket_titles` | killed: whole-epic preview returned 409 `stale_binding` instead of learning the new thread from the unchanged native agent and workspace |
| M15 | Refuse to refresh the provider-session observation unless it still equals the first hosted-seat binding | same whole-epic QNR regression | killed: apply returned 409 `revision_conflict` instead of preserving the SeatBinding/native agent/model route and durably recording the resumed thread |

The M9 and M10 killers also retain the complementary future-generation case and assert that it is refused before mutation. The QNR regression restarts the fake runtime between durable binding and repair, then proves the old-generation hosted and delivery identities remain unchanged.

M11 reproduces the ASMA-7869 live shape in which topology declared a role slot
before any AgentRun existed. M12 removes a previously materialized native seat
from the fake runtime while keeping its Kontor identity, then proves preview and
apply preserve that identity as `rename_pending` and still repair a stale native
container in the same complete plan. Both mutations failed before the restored
test reran green.

M13 reproduces Paseo 0.4.0's live missing-agent wire shape using the durable
`protocol/agent-not-found.json` fixture. Returning to the former
`CorrelationFailed` classification makes the new adapter contract fail at its
exact `StaleBinding` assertion. The restored distinction keeps a missing exact
id recoverable as evidence while a response carrying another agent id remains
hard correlation drift.

M14/M15 reproduce the live QNR LSA shape: Paseo kept native agent
`10c16ec0-…` in the same ECP workspace but resumed it from provider thread
`01a01ea3-…` onto `01a02084-…`. The restored behavior uses the exact native
agent plus container as the stable identity, freezes the freshly read provider
thread into apply correlation, and refreshes only that observation in the
hosted-seat row. The logical SeatBinding, native identity, and frozen model
route remain byte-for-byte unchanged.

## Restoration receipt

No mutant remains. The following focused restored runs passed after their respective mutations:

```text
cargo test -p kontor-core --test native_naming the_backlog_code_wins_when_a_descriptive_ai_short_name_is_also_present -- --exact
cargo test -p kontor-core --test native_naming the_v1_matrix_renders_exact_bullet_separated_bytes -- --exact
cargo test -p kontor-runtime fake::retitle_seat_generation_tests::persisted_seat_generation_is_a_bound_and_future_generation_is_refused -- --exact
cargo test -p kontor-runtime-paseo --test contract seat_retitle_accepts_an_older_persisted_generation_and_refuses_a_future_one -- --exact
cargo test -p kontor-runtime-paseo --test contract seat_retitle_classifies_an_exact_missing_native_agent_as_stale -- --exact
cargo test -p kontor-runtime-paseo --test contract two_epics_share_one_plane_without_sharing_a_project_or_static_task_scope -- --exact
cargo test -p kontor-daemon --test loopback_api a_legacy_jira_import_materializes_semantic_epic_control_and_ticket_titles -- --exact
```

The full Paseo contract suite was also rerun after correcting the immutable-recording test setup:

```text
cargo test -p kontor-runtime-paseo --test contract -- --nocapture
test result: ok. 135 passed; 0 failed
```

## Full gates

The final restored source passed every declared local gate on 2026-08-20:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir apps/console verify:api
pnpm -r typecheck
pnpm -r test
  Test Files  16 passed (16)
  Tests       295 passed (295)
pnpm audit --prod
  No known vulnerabilities found
cargo audit
  no vulnerabilities; 19 repository-allowed maintenance warnings
cargo deny check
  advisories ok, bans ok, licenses ok, sources ok
```

The workspace run included the 190 daemon loopback tests, 135 Paseo adapter
contracts, OpenAPI/MCP parity, schema 46→47 upgrade and refusal fixtures, crash
recovery, and the end-to-end pilot. No mutation or generated-artifact drift
remained in the verified tree.

## Live sparse-seat repair verification

The post-deployment live preview exposed two additional preflight shapes before
any title mutation occurred: an active logical SeatBinding with no AgentRun, and
an exact persisted seat whose runtime session was no longer readable. M11/M12
cover those shapes. After restoring both guards, the complete 2026-08-21 gate
set passed again:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
  kontor-daemon loopback: 190 passed
  kontor-runtime-paseo contract: 135 passed
  schema fixtures: 45 passed, including v46 -> v47
  OpenAPI/MCP parity and both E2E pilots: passed
pnpm --dir apps/console verify:api
pnpm --dir apps/console typecheck
pnpm --dir apps/console test
  Test Files  16 passed (16)
  Tests       295 passed (295)
pnpm --dir apps/console build
pnpm audit --prod
  No known vulnerabilities found
cargo audit
  no vulnerabilities; 19 repository-allowed maintenance warnings
cargo deny check
  advisories ok, bans ok, licenses ok, sources ok
```
