Artifact: gate-repair-report-round-4

# ASMA-8116 — repair of high-verification round 4

Gate receipt: `01a0960d-1bed-7a43-b1fd-ec93c662fdd7`
Verifier turn evidence: `0101a215831d0ecca1d31e33ff34957a383847cd6dc6d534c84c107478d1bcdb`
Repaired candidate: `527decf` plus this round.

## Blocker 1 — the occurrence counter is storage-monotonic

The key-change guards required an advancing sequence, but nothing stopped a
separate update from lowering it. Winding it back made a spent authority match
again, which is the A→B→C→A reactivation the ledgers exist to prevent.

Both ledgers now carry a `BEFORE UPDATE OF rename_sequence` trigger refusing any
value below the current one. Advancing stays unrestricted, so restore, migration
and future repair paths are untouched; only going backwards is refused. The
v94→v95 gate asserts both triggers are installed.

Regressions, one per ledger:
`a_rewound_rename_sequence_cannot_reactivate_spent_task_authority` and
`a_rewound_rename_sequence_cannot_reactivate_spent_epic_authority` drive
A→B→C→A, prove the rewind is refused, prove advancing still works, and prove the
spent authority stays spent.

## Blocker 2 — failures keep their kind

Two erasures, both fixed:

- `confirmed_jira_identities` was read through `.ok()`, so a repository failure
  became "this binding cannot prove itself" — permanent advice about a transient
  condition. The lookup now matches on the error and answers `Transient`.
- Every reconciliation error became `Transient`. Only
  `RepositoryError::Backend` is now transient; a durable ledger refusal answers
  the new `IdentityRefusal::RenameRefused`, which maps to HTTP 409
  `stale_binding` with its own rule.

The classification is now three-way and each branch is reachable: a different
immutable issue (409 `stale_binding`), an unproven identity
(`placement_blocked`), a ledger refusal (409 `stale_binding`, distinct rule),
and a genuine outage (`unavailable`).

## Blocker 3 — the content conflict names the current key

`TicketContentConflictDto.external_issue_key` was built from the link row, which
after a same-issue rename still holds the key the pass asked under. It is now
built from the post-observation projection.

## Mutations — three seeded, three killed, sources restored

| # | Mutation | Test | Result |
| --- | --- | --- | --- |
| M11 | Task sequence-rewind guard removed | `a_rewound_rename_sequence_cannot_reactivate_spent_task_authority` | **killed** |
| M12 | Every reconciliation error classified transient | `a_ledger_refused_rename_is_permanent_while_only_outages_are_transient` | **killed** |
| M13 | Conflict key taken from the superseded link row | `a_same_issue_rename_reports_its_content_conflict_against_the_current_key` | **killed** |

### Why M12 and M13 first survived, and what changed

Both survived their first seeding, and in each case the fault was in the test,
not the mutant — so the gate was **not** claimed on them.

**M13** was asserted on a plan taken *after* the resident pass had already
renamed the binding. By then the link row itself holds the new key, so reading
either source gives the same answer and the assertion cannot tell them apart.
The new regression asserts during the *first* plan — the pass that performs the
rename and still starts from the pre-rename link. That plan needs a converged
subject to succeed at all, so the fixture's task state became a parameter:
`blocked` where a planned transition is needed for a write-boundary assertion,
converged here where only the conflict is under test.

**M12** was aimed at the anti-rebind test, whose refusal is `DifferentIssue` and
never reaches the reconciliation-error arm at all — a semantically inert target.
It was replaced with a fault that violates the frozen requirement directly: the
new regression plants a second confirmed binding on the destination key, so the
ledger refuses the rename on a durable rule. With the mutation that refusal is
reported as `unavailable`; restored, it is 409 `stale_binding`.

## Gates

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy` (4 crates, `-D warnings`) | clean |
| `cargo test -p kontor-store --test jira_materialization` | ok, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | ok, 58/58 |
| `cargo test -p kontor-jira` | ok, 9 + 19 |
| `cargo test -p kontor-api` | ok, 23 + 5 + OpenAPI 3 |
| `cargo test -p kontor-daemon --test loopback_api jira` | ok, 9 (1 predeclared ignored) |
| identity suite (`the_resident_reconciler*`) | ok, 4/4 |
| new blocker regressions | ok, 1/1 each |

`mcp_journey` and the console gate were not re-run this round; neither is
claimed. `mcp_journey` remains OQ-004's predicted failure.

## Preserved

`JiraItemCode` and the legacy naming files are untouched — `backlog_identity.rs`
and its tests remain byte-identical to `origin/master`. ASMA-8117 untouched.
OQ-002, OQ-004, OQ-005 and the migration-sequencing attribution are unchanged;
no migration number is guessed.
