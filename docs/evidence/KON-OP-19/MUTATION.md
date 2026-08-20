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

The M9 and M10 killers also retain the complementary future-generation case and assert that it is refused before mutation. The QNR regression restarts the fake runtime between durable binding and repair, then proves the old-generation hosted and delivery identities remain unchanged.

## Restoration receipt

No mutant remains. The following focused restored runs passed after their respective mutations:

```text
cargo test -p kontor-core --test native_naming the_backlog_code_wins_when_a_descriptive_ai_short_name_is_also_present -- --exact
cargo test -p kontor-core --test native_naming the_v1_matrix_renders_exact_bullet_separated_bytes -- --exact
cargo test -p kontor-runtime fake::retitle_seat_generation_tests::persisted_seat_generation_is_a_bound_and_future_generation_is_refused -- --exact
cargo test -p kontor-runtime-paseo --test contract seat_retitle_accepts_an_older_persisted_generation_and_refuses_a_future_one -- --exact
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
