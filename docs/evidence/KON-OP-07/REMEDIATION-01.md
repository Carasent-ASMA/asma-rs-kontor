# KON-OP-07 remediation 1 — answering the code-review gate

Date: 2026-08-18
Reviewed delivery: submodule commit `3b95141` (`65412ba..3b95141`)
Gate at review: `code-review-gate: rejected`, receipt
`01a01397-f939-7422-81a7-4c03b5622785`
Findings answered here: 1, 2, 4. Finding 3 is **blocked** on the epic owner.

## Finding 1 — import was not atomic across items and manifest

Accepted in full; the reviewer's reproduction was correct and
`CHECKPOINT-01.md`'s atomicity claim was false. The item loop committed, then
`record_subject_import` opened a second transaction for the manifest and receipt.
Because `already_imported` is derived from the manifest, a failure between the two
left items that the retry could not see: it re-ran the loop, hit their primary
key, and the subject could never reach its switch.

The import is now one transaction. Three things moved into it:

- `record_subject_import_in` — the manifest and receipt writer, split out of
  `SqliteStore::record_subject_import` so a caller can join its own transaction.
  The public method is now that function plus a commit.
- `subject_authority_in` — the origin/authority check, so the check and the write
  it guards see the same transaction. It was previously read in a transaction that
  had already ended before the items were written.
- `memory_readback_hash` — hoisted to a free function over `&Connection`, so the
  hash is computed from the rows this transaction wrote rather than from whatever
  the store's connection could see after a separate commit.

The FTS rebuild stays outside the transaction on purpose: it is a derived,
rebuildable projection, and a failure there costs an index rebuild rather than the
import.

**Proof.** `an_import_that_fails_while_recording_its_manifest_leaves_nothing_and_resumes`
injects the failure where the reviewer's probe did — on the manifest insert, after
every item is written. It asserts `memory_items`, `memory_revisions`,
`memory_approvals` and `subject_import_manifests` are all empty, that the export
still previews as pending, that the retry imports each item exactly once, and that
attest and switch then succeed. Mutation-checked: restoring the commit between the
items and the manifest turns it red (`left: 2, right: 0` on `memory_items`).

## Finding 2 — backlog authority advertised but enforced nowhere

Accepted. Both halves of the finding are answered, because either alone would
still ship a lie.

**Enforced.** `require_backlog_authority` guards the two writes that *are* the
backlog — `apply_epic` (the mini-project/task/dependency graph) and
`transition_task` (its lifecycle) — inside their existing transactions. It refuses
with a new `RepositoryError::AuthorityWithheld`, deliberately not a `Conflict`,
following the reasoning already written on `CapacityExhausted`: a conflict tells a
caller to re-read and retry, and re-reading withheld authority returns the same
answer. `ApiError::from_repository` maps it to `forbidden`, which is the same
answer the native memory path already gives for the same fact.

**Made true.** `projects:ensure` now refuses `backlog_origin: legacy_pending`
outright, and the MCP tool's enum no longer offers the value. There is no backlog
import, readback or switch yet, so that origin named a state no operation could
clear: a project declared that way would have been unwritable forever with nothing
to tell the caller why. The value stays in the domain enum for checkpoint 5, which
is what will give it an exit.

**Proof.** `a_backlog_a_legacy_system_owns_refuses_the_writes_that_are_the_backlog`
asserts the declaration is refused 400 `invalid_request`, then seeds a
pending-backlog project through the store (the layer below the policy, which still
accepts it — as `create_project` does) and asserts `epics:apply` returns 403
`forbidden` and that authority did not quietly move. Mutation-checked: removing
the guard from `apply_epic` turns it red (`left: 200, right: 403`).

## Finding 4 — contract declared 409, route returned 400

Accepted. Aligned by declaring 400, which is what the route returns and the truer
of the two: the request names a realm-wide operation that no longer exists, and no
re-read or retry makes it valid, while 409 would invite exactly that retry. The
contract document and the console types generated from it were regenerated.

**Proof.** The freeze assertion in
`an_empty_realm_is_bootstrapped_through_public_operations_alone` now reads
`/v1/openapi.json` and asserts the declared status set equals the status the route
returned. Asserting only the returned code is what let the two drift.

## Finding 3 — the Jira half of OP-07 — BLOCKED, not declined

Confirmed accurate: `TicketDelegation { asma: &AsmaExecutable }` is intact, there
is no `kontor-jira` crate, and `kontor-integrations-asma` is still a
`kontor-daemon` dependency. That is checkpoints 2-4 of
`docs/evidence/KON-OP-07/ARCHITECTURE.md` (native crate and keychain-backed
connector, materialization and ASMA Epic activation, the reconcile/comment
implementations), and it is roughly half the ticket's acceptance surface.

No work was done on it here, and none was re-scoped away. Whether those clauses
stay in ASMA-7876 or move to their own ticket is the epic owner's decision, and
this seat does not hold it. Pending that call, this remediation is confined to the
authority cutover, which is the part of the delivery the review found defects in.

## Gates

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, `cargo test --workspace --no-fail-fast`, `cargo deny check`,
`pnpm --filter kontor-console verify:api`, `pnpm -r typecheck`, `pnpm -r test`.
