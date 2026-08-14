# KON-MVP-23 / ASMA-7821 — Memory-mutant audit

Date: 2026-08-14 (Europe/Oslo)

Seat: persistent Audit seat, canonical TSW `wks_0fe677df8a595067`.

Audited commit: `b1863d3ed1ff8f8ae09b69df433fe8d179c943f2`

Archive parent: `e4242122ee00875231210d061d2fcc07a9dbf90c`

## Verdict

**AUDITED_TRUE**

The four committed memory-ledger killer tests genuinely cover ML01, ML03,
ML07, and ML09. The target commit contains no production-code change, the
mutation evidence is consistent with the test bodies, and all requested
standalone store gates pass. No hidden scope reduction or surviving
non-equivalent mutant was found.

## Per-area checklist

- **[AUDITED_TRUE] ML01 — aggregate revision monotonicity.**
  `crates/kontor-store/src/memory.rs:765-800`,
  `reproposal_never_resets_the_aggregate_revision`, proposes two revisions
  for one item, then compares `memory_items.aggregate_revision` with the
  maximum stored revision and requires `(2, 2)`. This kills the seeded reset
  to zero and pins the aggregate not being reset during re-proposal.

- **[AUDITED_TRUE] ML03 — one current revision after two approvals.**
  `memory.rs:803-853`,
  `two_approvals_leave_exactly_one_current_revision`, approves two successive
  revisions, requires both approvals, requires exactly one `current`, and
  requires the second revision to be that current revision. This directly pins
  supersession/current-pointer uniqueness.

- **[AUDITED_TRUE] ML07 — frozen Context Pack revision hash.**
  `memory.rs:856-890`,
  `frozen_revision_hash_is_the_approved_stored_hash`, freezes an approved
  revision and compares the frozen ordered revision hash with both the stored
  history document hash and the proposal document hash. This pins the frozen
  pack to the approved stored revision bytes, not merely to selection metadata.

- **[AUDITED_TRUE] ML09 — FTS projection excludes proposals.**
  `memory.rs:893-922`,
  `proposal_never_enters_fts_before_approval`, inserts an unapproved proposal,
  counts FTS rows lacking a matching approval, and requires zero. This pins
  FTS as a rebuildable approved projection rather than a proposal index.

- **[AUDITED_TRUE] Production scope.**
  `git diff --name-status e424212..b1863d3` is only
  `M crates/kontor-store/src/memory.rs`; `git diff --numstat` is `160 0`.
  The complete delta is four test functions/test assertions inside the test
  module. `git diff --check` passes; no production logic or other dependency
  changed.

- **[AUDITED_TRUE] Mutation evidence.**
  `docs/evidence/KON-MVP-23/QA-MEMORY-MUTANTS.md:25-34` records one-at-a-time
  disposable seeds: ML01 fails with `(0, 2)`, ML03 with current count `2`,
  ML07 with a differing frozen hash, and ML09 with one unapproved FTS row.
  Each targeted killer exits 101, each restored test exits 0: `4/4 killed,
  0 survivors`. The seeded defects and failures are consistent with the
  assertions above.

- **[AUDITED_TRUE] Focused and full store gates.**
  `cargo test -p kontor-store memory::tests --locked`: **8 passed, 0 failed**.
  `cargo test -p kontor-store --locked`: **257 passed, 0 failed**.
  `cargo clippy -p kontor-store --all-targets --locked -- -D warnings` and
  `cargo fmt --all -- --check`: **pass**.

- **[AUDITED_TRUE] Cargo.lock identity.**
  `Cargo.lock` is byte-identical to target HEAD. Both
  `git show HEAD:Cargo.lock` and the worktree hash are
  `0cace4c7d2709181c3f863678f30bc0aed5fee73`; `cmp` and the lock diff check
  pass.

- **[AUDITED_TRUE] Worktree and foreign evidence preservation.**
  The committed tree is exact at `b1863d3`. Tracked changes are absent. The
  only non-foreign untracked KON-23 evidence files are the QA file cited above
  and this audit record; the
  untracked `docs/evidence/KON-MVP-18/run-40870492d74e3b3a/`,
  `run-89a688943e1099bf/`, and `run-97d55adc7ea6a9ef/` directories remain
  untouched and preserved.

## Integration qualification

Migration `0022` renumbering, the final integrated Cargo.lock regeneration,
and the full integrated workspace/clippy/test/license rerun remain the known
KON-20/integration-time shipment prerequisites. They are non-blocking for this
standalone KON-23 memory-mutant audit, but shipment still requires those
integrated gates to pass after both branches and the migration renumber merge.
