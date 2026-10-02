# ASMA-8187 — mutation evidence manifest

Two kinds of record live in this directory, and they are not interchangeable.

## Historical runs (remedies 2, 4, 5 — candidate `9ac33c19` and earlier)

`M-P1*`, `M-P2*`, `M-R2*`, `M-R4*`, `M-R5*`. Every patch and raw log is retained
exactly as produced. They are **not independently reproducible**: fourteen of the
fifteen patches were taken against intermediate working-tree states rather than a
named object, and the logs do not record the baseline commit, the exact command,
the toolchain, or pre/post source hashes. What they do establish is what the
report claimed at the time — each mutant compiled, ran, and produced the recorded
outcome — and the final source state was verifiably restored.

They are preserved unchanged. Nothing below rewrites them.

One result in that set is a survival rather than a kill, and it is restated here
because a manifest that lists only kills is not evidence:

* **`M-R4c` survived.** Removing the claim's other-key refusal changes no
  observable behaviour, because the claim read-back added for the occupancy pin
  refuses the second caller independently. `M-R4c2` removes the exclusivity
  itself — the application check *and* the uniqueness index — and is killed. The
  application check alone is therefore not what the concurrency test proves.

## Recorded runs (this dispatch — the six audit remedies)

`M-P11*`, `M-P12*`, `M-P13`, `M-P21*`, `M-P22*`. Each was produced by
`mutate.sh`, against a named baseline commit with a clean working tree, and each
log records:

* the baseline commit and tree object,
* the exact `cargo` command and the filter it ran,
* the toolchain and platform,
* the SHA-256 of every file the mutant touches, *as the baseline holds it*,
* the SHA-256 after mutation,
* the raw output and the process exit status (non-zero = killed),
* the SHA-256 after restoration, and an explicit byte-identical verdict.

The patches are mutation-only diffs against that baseline, so each applies and
reverses cleanly against it.

Baseline for every recorded run: **`a088e037c083b2e6c2c2ddff670df4ea2d918371`** (tree `9a8114d00b87a90270bddde5aa37683c1a91f912`),
working tree clean at mutation time, restored byte-identically after each.

| Mutant | Removes | Killed by | Kind of kill |
|---|---|---|---|
| `M-P11a` | the occupancy fence in the effect latch | `a_reused_native_id_in_a_later_occupancy…` | **rule text only** — see below |
| `M-P11b` | the runtime generation from the active-occupant proof | `an_active_native_of_another_runtime_generation…` | outcome: the latch committed |
| `M-P11c` | kind/host/generation from the transition's identity test | `a_reused_native_id_in_a_later_occupancy…` | outcome: the later route never committed |
| `M-P12a` | both seat-bearer prefixes from the value scanner | `a_real_format_seat_bearer_is_refused_by_value…` | outcome: a bearer canonicalized |
| `M-P12b` | the foreign-document rescan in `verify` | `a_foreign_document_carrying_a_seat_bearer…` | outcome: the document imported |
| `M-P13` | the generation-13 strip from `canonical_records_bytes` | `an_authentic_schema_twelve_document…` | outcome: an authentic v12 document failed its own digest |
| `M-P21a` | typing the readback at the write boundary | `a_succession_readback_is_refused_at_the_store_boundary` | outcome: an incomplete readback persisted |
| `M-P21b` | recomputing the grant-subject digest | `a_readback_whose_derived_or_redundant_values…` | outcome: a fabricated digest persisted |
| `M-P22a` | the command-receipt identity join | `a_succession_cannot_bind_a_receipt_recorded_for_another_command` | outcome: a foreign receipt bound |
| `M-P22b` | receipt uniqueness and the frozen instant (migration) | `a_succession_cannot_bind_a_receipt_recorded_for_another_command` | outcome: the completion instant was rewritten |

## `M-P11a` is a weak kill, and why it stays that way

Removing the occupancy fence does **not** change whether the abandoned
succession is refused. The identity comparison refuses it anyway, because the
reused native carries a different runtime generation, and the test fails only
because it pins the exact rule the refusal must name.

That is not a defect in the test; it is the shape of the domain. For the fence
to be the *only* thing standing between an abandoned row and a receipt, a seat
would have to hold a byte-identical native identity at two different
occupancies — and it cannot: `hosted_topology_seat_history` is keyed
`(project_id, seat_binding_id, native_id)`, so one seat cannot retire the same
external id twice. The fence's independent value is therefore against a row
written by some path other than this one, and no test in this suite can
demonstrate that without planting such a row.

Recorded here rather than resolved by weakening the assertion.

## Recorded runs (audit of `2ae5ae77` — P2 ×3)

Baseline for every row below: **`41f48e695bcf40498c807ca0a246b5d0c7ddb8a7`**,
clean tree, restored byte-identically after each run. Same harness, same
recorded fields.

| Mutant | Removes | Killed by | Outcome |
|---|---|---|---|
| `M-P22b1` | the receipt uniqueness index | `one_receipt_cannot_be_bound_to_two_successions` | killed |
| `M-P22b2` | the frozen completion instant | `a_bound_succession_cannot_have_its_completion_instant_rewritten` | killed |
| `M-P23` | the `0122` upgrade validation | `a_mismatched_historical_binding_stops_the_upgrade_at_0122` | killed |
| `M-P24` | binding the readback to its transition | `every_readback_field_is_bound_to_the_transition_it_commits` | killed |
| `M-P25` | the receipt join ordering | `a_replayed_binding_is_proved_before_it_answers_unchanged` | killed |

### Two survivals, retained, because each one found a real defect

The original `M-P22b` removed uniqueness **and** the frozen instant together, so
its kill could not say which guard did the work. Splitting it is what this
remedy asked for, and splitting it immediately showed that neither half had been
proved:

* **`M-P22b1-first-attempt-SURVIVED`** (baseline `129466cc`). Uniqueness removed,
  test still green: the second succession in that version was committed but
  incomplete, so `0120`'s "a receipt only once both effects have landed" rule
  refused the planted binding before the index was consulted. The test now
  carries that succession to complete.
* **`M-P25-first-attempt-SURVIVED`** (baseline `e5aea100`). Ordering reverted,
  test still green: the test binds an *unbound* row, so `bound` is `None` and the
  early return whose order changed is never reached.
  `a_replayed_binding_is_proved_before_it_answers_unchanged` was added to arrive
  at it.

Both patches and raw logs are kept beside the kills. The original `M-P22b`
record and its limited claim are unchanged.
