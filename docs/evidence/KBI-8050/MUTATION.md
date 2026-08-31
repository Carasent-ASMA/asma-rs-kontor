# KBI-8050 mutation ledger

Date: 2026-08-31  
Jira: ASMA-8050  
Branch: `fix/ASMA-8050-harden-in-place-jira-recovery`

Each mutant below was applied alone with `apply_patch`, executed against its
named test, observed red, reverted with `apply_patch`, and then observed green.
No mutant remains in the delivery tree.

| Mutant | Test that killed it | Red evidence | Green readback |
| --- | --- | --- | --- |
| Remove exact `SeatBindingId` correlation from Paseo Committee permission inspection | `committee_permission_is_bound_to_the_exact_run_seat_and_native` | Exit 101 at `contract.rs`: the wrong logical seat unexpectedly received `pending_permissions: [perm_1]` | Exit 0, 1 passed |
| Permit a later Jira materialization batch to overwrite an epic's confirmed full key by removing the guard and making the UPSERT replace `external_issue_key` | `a_confirmed_epic_binding_cannot_be_replaced_by_a_later_batch` | Exit 101: “a later batch may not replace a confirmed epic identity” | Exit 0, 1 passed |

The ordinary green suite also pins the in-place recovery batch/item/marker
ledger, zero replacement batches, ordinal mapping, strict Jira readback, durable
permission confirmation, and replay without a second native effect.
