# ASMA-8202 mutation proof

Date: 2026-09-17

The focused regression
`unconfirmed_admissions_are_unknown_unbound_queued_roots_only` was run against
two temporary mutations in the durable recovery selector:

- Removing `run.observed_state = 'unknown'` failed with two selected admissions
  instead of one, proving a run with runtime observation cannot be retried as an
  unconfirmed launch.
- Removing `binding.id IS NULL` failed with two selected admissions instead of
  one, proving an already-bound run cannot be retried as an unconfirmed launch.

Both predicates were restored. No mutation remains in the tree.
