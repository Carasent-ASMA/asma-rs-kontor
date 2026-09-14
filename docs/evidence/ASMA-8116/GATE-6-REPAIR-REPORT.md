Artifact: gate-repair-report-round-6

# ASMA-8116 — repair of F-8116-V14

Rejected candidate: `560db2c`
Verifier report: `HIGH-VERIFICATION-ROUND-6.md`, commit `3e905cf`, preserved verbatim

## F-8116-V14 — evidence is re-established at the confirmed current key

The verifier's boundary was exact. Selecting the right address was necessary but
not sufficient: `JiraConnector::live` hashes a canonical document containing the
key it was *asked* under. An observation requested as `ASMA-1` therefore carries
`ASMA-1` in its hash, the rebuilt dry run reads `MOVED-9` and hashes `MOVED-9`,
and `validate_expected` refuses `IncompatibleHumanMove` before any write.

**The smallest design that fixes it:** after a rename, re-observe through the
current-key delegation so the evidence and the address agree, and re-prove the
same immutable issue at the new address before continuing.

What is preserved, deliberately:

- **Same-issue proof.** `IdentityDecision::Proceed` now carries the proven
  `issue_id`, and the refreshed answer must report that same id *and* the
  expected key. A key that moved to a different issue between the two reads is
  an anti-rebind, not a rename, and stops the subject.
- **Anti-rebind refusal**, **immutable UUID**, **external issue id**, **exact
  revision/authority**: untouched. Only the address and its evidence change.
- **Human-move protection.** The refreshed observation becomes the baseline this
  pass validates against, so a genuine human move after it still refuses. The
  protection is re-based, not removed.
- **No numeric inference.** Nothing is derived from the shape of either key.

## Read budget — changed, and stated

Steady state is unchanged: confirming that an identity still agrees costs
nothing, because it rides the status observation already being made. A rename
now costs **exactly one** additional read, in the pass that follows it. That is
the price of evidence that matches its address, and the retained assertion in
`the_resident_reconciler_follows_a_same_issue_rename_through_the_connector`
states it as `replay_reads + 1 == reads_for_one_pass` rather than being relaxed.

## Regressions retained

- **`an_epic_same_issue_rename_never_addresses_the_superseded_key`** — now
  non-vacuous. It uses an explicit legacy backlog code (a code, never a Jira
  key) and the next configured DRAFT route (`10236`, TO BE GROOMED), and asserts
  in order: the write list is **not empty**, some write addresses
  `/rest/api/3/issue/MOVED-9/`, and no write names `ASMA-1`. The previous
  version permitted an empty list, which the verifier correctly called out.
- **`an_epic_key_that_moved_to_another_issue_is_still_refused_after_the_refresh`**
  — a different immutable issue at the same key is refused, the binding stays
  on `ASMA-1`, and nothing is written. Re-observing must not become a way to
  adopt another issue.

## Mutation

| # | Mutation | Test | Result |
| --- | --- | --- | --- |
| M15 | Skip the refresh, keep the pre-rename observation (`renamed: true` → `renamed: false`) | `an_epic_same_issue_rename_never_addresses_the_superseded_key` | **killed** |

The mutant reproduces the verifier's probe exactly:
`JiraReconcileReport { epic_subjects: 1, applied: 0, blocked: 1, renamed: 1, .. }`
with an empty write list. Restored and re-run green.

## Correction to the previous repair report

`GATE-5-AUDIT-REPAIR-REPORT.md` recorded an unexplained failure of
`a_rewound_rename_sequence_cannot_reactivate_spent_task_authority` and
attributed the missing epic effect to OQ-004. Both were wrong:

- The store failure was **build contamination** from the earlier M11 mutant in
  the shared `target/`. In a fresh `CARGO_TARGET_DIR` the target is 30/30 and
  `schema_v1` is 58/58. There is no store defect and the blocker is withdrawn.
- The missing epic effect was **not** OQ-004. The verifier supplied the legacy
  code and still reached the native boundary, where the stop was observation
  hash alias drift — F-8116-V14, fixed here. OQ-004 remains real for unplaced
  epics without an immutable legacy code; it was simply not this.

All gates in this round were run with an isolated `CARGO_TARGET_DIR`.

## Gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy` (4 crates, `-D warnings`) | clean |
| `cargo test -p kontor-store --test jira_materialization` | ok, **30/30** |
| `cargo test -p kontor-store --test schema_v1` | ok, **58/58** |
| `cargo test -p kontor-jira` | ok, 9 + 19 |
| `cargo test -p kontor-api` | ok, 23 + 5 + OpenAPI 3 |
| `cargo test -p kontor-daemon --lib` | ok, 81/81 |
| `cargo test -p kontor-daemon --test loopback_api jira` | ok, 9 (1 predeclared ignored) |
| identity/rename targets (6) | ok, 4 + 1 + 1 + 1 + 1 + 1 |

## Pre-existing failure, not from this round

`an_epic_placeholder_body_is_typed_reported_and_repairable` fails with
`503 unavailable — the configured native Jira connector could not answer`. It
fails identically at `560db2c` with this round's changes reverted, in the same
clean target, so it predates this repair and is **not** caused by it. It is
recorded rather than fixed: it is outside F-8116-V14's scope and unattributed.

## Preserved

Every earlier repair, legacy `JiraItemCode`/token/hash behaviour, ASMA-8117
ownership, OQ-002/OQ-004/OQ-005 and migration `0095` ordering. No audit, merge,
deploy, Jira or topology change.
