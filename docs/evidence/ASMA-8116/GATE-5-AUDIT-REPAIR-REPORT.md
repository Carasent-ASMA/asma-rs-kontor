Artifact: gate-repair-report-round-5-audit

# ASMA-8116 — repair of high-audit F-8116-A2

High-audit gate receipt: `01a09656-9b8e-7bd2-a0aa-b089c2f220d7`
Audit evidence: `70c9df972624a0981953921c131bd5b2369756f3af503adae42b052923add9cb`
Auditor report: `HIGH-AUDIT-ROUND-5.md`, committed verbatim
(SHA-256 `bc538b15eb39a2214ecc30498bf6b4570237d340d130f52395dcec665b0d80cc`, verified before staging)
Repaired candidate: `a04a85a` plus this round.

## F-8116-A2 — the epic write path addresses the current key

The audit's analysis was exact: `observe_delegation` is built before identity is
decided and carries the key the pass entered with; the provisional intent and
the final delegation reused it via `..observe_delegation`, so a valid same-ID
rename advanced the binding to `MOVED-9` while the transition went to
`/issue/ASMA-1/transitions`.

Two changes:

1. **A named selector.** `Services::write_key_for(entry_key, decided)` returns
   the key every write in the pass must address. It exists as a named unit
   because the defect was not a wrong expression but a *stale object*, and a
   single place to derive the key is what makes that unrepeatable.
2. **A rebuilt delegation.** `reconcile_jira_epic` constructs
   `current_delegation` from that key after the decision, and the provisional
   intent, the apply delegation, the dry run and the apply all derive from it.
   Only the key changes: the pinned specs, the epic's projection revision and
   the observed evidence are the same objects, so the immutable UUID, the
   external issue id, the evidence/revision checks and the anti-rebind decision
   are untouched.

## Regressions retained

- **`a_same_issue_rename_selects_the_current_key_for_every_write`** (unit, in
  `applications.rs`). After a valid same-ID rename the selected write key is
  `MOVED-9` and `ASMA-1` is unreachable; an unchanged identity still writes
  through its own key, so the selector cannot be satisfied by always returning
  something new. This is the deterministic seam the whole write path reads from.
- **`an_epic_same_issue_rename_never_addresses_the_superseded_key`**
  (end-to-end). Proves the binding advances to `MOVED-9` and that no outbound
  request addresses `ASMA-1`.

**Why the end-to-end case does not assert the transition itself.** The epic pass
blocks after the rename on legacy epic setup unrelated to identity — the same
family as OQ-004 — so the apply is not reachable from a focused fixture. Rather
than assert something the fixture cannot reach, that case asserts what it can
prove and the transition-target property is proved at the factored seam. The
test carries this note inline. The fixture is retained, not deleted, and will
assert the transition once the epic apply path is reachable. No Jira key is
carried in any legacy backlog-code field; the fixture's legacy code is explicit.

## Mutation

| # | Mutation | Test | Result |
| --- | --- | --- | --- |
| M14 | Stale selector: `write_key_for` returns the pre-observation entry key for `Proceed` | `a_same_issue_rename_selects_the_current_key_for_every_write` | **killed** (`left: ExternalId("ASMA-1")`) |

Restored and re-run green.

## Gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy` (4 crates, `-D warnings`) | clean |
| `cargo test -p kontor-daemon --lib` | ok, 81/81 |
| `cargo test -p kontor-daemon --test loopback_api jira` | ok, 9 (1 predeclared ignored) |
| identity/rename cases (5 targets) | ok, 4 + 1 + 1 + 1 + 1 |
| `cargo test -p kontor-jira` | ok, 9 + 19 |
| `cargo test -p kontor-api` | ok, 23 + 5 + OpenAPI 3 |
| `cargo test -p kontor-store --test jira_materialization` | **FAILED, 29/30** — see below |

## Remaining blocker, not repaired here

`a_rewound_rename_sequence_cannot_reactivate_spent_task_authority` fails: the
raw `UPDATE jira_task_binding_confirmations SET rename_sequence = 0` succeeds
where the monotonic guard should abort it. Diagnostics show the stored sequence
is `3` and exactly one confirmation row matches the link, so the guard's `WHEN`
predicate should hold; `schema_v1` (58/58) asserts both
`jira_*_rename_sequence_monotonic` triggers are installed, and neither the
migration nor the store changed in this round.

The two observations contradict each other and the cause is **not isolated**.
This round changed no store or migration file, so the state is inherited from
`a04a85a` rather than introduced here — it was green when round 4 was pushed and
is reproducibly red now, which is itself unexplained. It is recorded as an open
blocker rather than presented as a passing gate.

## Preserved

Every earlier repair, legacy `JiraItemCode`/token/hash behaviour
(`backlog_identity.rs` and `naming.rs` unchanged from the integrated base),
ASMA-8117 ownership, OQ-002/OQ-004/OQ-005, and migration `0095` ordering. No
merge, deploy, Jira or topology change.
