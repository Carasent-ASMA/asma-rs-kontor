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

## Recorded runs (joined baseline — additive current-master join)

Baseline: **`7646bcd9ad1b2c0113f60b1a383a0a74def996db`** (tree
`1d7c94d44316c4f4a3c797cf5928249ff3e95514`), source clean at mutation time,
restored byte-identically after each. The five mutants are the same five; what
changed is the baseline they stand against, now that the candidate is joined
with `8acdbd17`.

Each mutant now has **two** logs, one per baseline, at distinct paths. The
`.patch` files are shared: the mutation text is identical at both baselines, so
there is one patch per mutant and it applies to either.

| Mutant | `41f48e69` run | joined `7646bcd9` run |
|---|---|---|
| `M-P22b1-receipt-not-unique` | `….log` — killed (101) | `…-joined-7646bcd9.log` — killed (101) |
| `M-P22b2-completion-instant-not-frozen` | `….log` — killed (101) | `…-joined-7646bcd9.log` — killed (101) |
| `M-P23-upgrade-assumes-coherent-bindings` | `….log` — killed (101) | `…-joined-7646bcd9.log` — killed (101) |
| `M-P24-readback-unbound-from-transition` | `….log` — killed (101) | `…-joined-7646bcd9.log` — killed (101) |
| `M-P25-unchanged-answers-before-proof` | `….log` — killed (101) | `…-joined-7646bcd9.log` — killed (101) |

### Correction: the originals were briefly displaced

`55cab6a8` wrote the joined runs over the five `41f48e69` logs at their original
paths, so for one commit this manifest's claim that the earlier records "remain
historical and unmodified" was not true of the working tree — only of Git
ancestry. The originals are restored here from `99b23720`, byte-identical, and
the joined runs now live under `*-joined-7646bcd9.log`. Recorded rather than
quietly fixed: the claim was wrong while it stood, and an evidence set that
overwrites its own history is the one thing a manifest exists to prevent.

The two retained first-attempt survivals are unaffected and unmodified.

One harness note, recorded because it is a property of the evidence rather than
of the code: the cleanliness guard now scopes to `crates/`. It previously
checked the whole tree and refused the second run of the round, because the
evidence logs the harness itself writes are tracked files from earlier rounds.
The mutation baseline is the source; the logs are its output.

## Control-validation mutant `M-CTL`, and a malformed first carrier

`M-CTL` makes `install_jira_credential` refuse every Realm at the schema
boundary. It exists because every assertion in the stopped-schema test is about
something *not* happening, so a blanket refusal would satisfy it; the mutant is
what shows the hardened control notices.

The result has always held — the stopped-schema test passes under the mutant and
the control fails on the reader it never reached — and an independent QA recheck
reproduced it in a disposable archive with byte-identical restoration. That is
additive evidence about the *behaviour*. It does not make the first artifact
valid, and it is not cited as if it did.

| Carrier | State |
|---|---|
| `M-CTL-refuse-every-realm.patch` | **invalid** — retained unchanged |
| `M-CTL-refuse-every-realm.log` | **incorrect identity** — retained unchanged |
| `M-CTL-refuse-every-realm-corrected-109131ba.patch` | valid, verified |
| `M-CTL-refuse-every-realm-corrected-109131ba.log` | truthful identity |

### What was wrong with the first pair

Two defects, both mine, both recorded rather than quietly replaced:

* **The patch was not a patch.** It was hand-written with a bare `@@` and no
  hunk coordinates, so `git apply --check` answers *"No valid patches in input"*
  and `patch --dry-run` cannot place it. Nobody could have reproduced the run
  from it. The corrected patch is generated by `git diff` and carries a real
  ranged header, `@@ -249,6 +249,10 @@`.
* **The log named the wrong baseline.** It claimed `132058ec` while recording
  preimage and restored hashes of `92015a21…`, which is the `109131ba` file;
  `132058ec`'s `main.rs` is `89f90e95…`. The corrected log states the source
  snapshot by hash first and the commit only as the way to reach it, which is
  the identity a mutation record actually needs.

Both are retained so the correction is legible: an evidence set that deletes its
own bad artifacts is harder to trust than one that keeps them labelled.

## Recorded runs (corrected coherence baseline)

Baseline: the commit these logs name, with the projection coherence correction
in place. Seven mutants, all killed, each restored byte-identically.

| Mutant | Removes | Outcome |
|---|---|---|
| `M-P22b1` | the receipt uniqueness index | killed (101) |
| `M-P22b2` | the frozen completion instant | killed (101) |
| `M-P23` | the `0122` upgrade validation | killed (101) |
| `M-P24` | binding the readback to its transition | killed (101) |
| `M-P25` | the receipt join ordering | killed (101) |
| `M-CTL` | the realm read at the schema boundary | killed (101) |
| `M-COH` | the single store acquisition in the occupancy chain | killed (101) |

### Two harness defects found and fixed, both recorded

Neither was in the production code; both would have corrupted the evidence.

* **`M-COH` first attempt hung for ~48 minutes and produced no verdict.** The
  harness asserted inside `std::thread::scope` before releasing the barrier, so
  the scope waited forever to join a reader nobody would wake. A hang is not a
  kill, and it is recorded as **inconclusive** with full process evidence in
  `M-COH-first-attempt-INCONCLUSIVE.txt`. The coordination is now bounded end to
  end and the second attempt killed the mutant in 4.78s.
* **The mutation runner skipped its own restoration.** `set -e` aborted the
  script at `wait` whenever a mutant was killed — which is every successful run
  — so the file was left mutated and the *next* mutant snapshotted an already
  mutated file. One run (`M-P22b2`) was invalidated that way and was redone.
  Restoration is now unconditional through an `EXIT` trap. Evidence that
  corrupts the next measurement is worse than no evidence.

Every patch in this group is mutation-only against its own named snapshot —
produced by `diff -u` against that snapshot rather than `git diff`, which would
sweep in uncommitted work — and each log records `git apply --check` and
`patch --dry-run` accepting it, plus a hard wall-clock limit whose `124` verdict
is reported as inconclusive rather than as a kill.

## Correction: `8910ebc6` overwrote twelve immutable carriers

An independent audit of `8910ebc6` found that the refreshed runs of that round
were written **over** twelve historical carriers at their original paths rather
than placed beside them. This is the second time the same mistake has been made
in this evidence set — the first is recorded above under "the originals were
briefly displaced" — and it is the mistake this manifest exists to prevent.

The twelve were restored from their pre-`8910ebc6` blobs (`5e00a238`), verified
byte-identical, and the refreshed runs now live at distinct `*-8910ebc6` paths.
Both generations are enumerated below. Nothing was rewritten to hide the error;
`8910ebc6` still contains the overwrite, and this section is the correction.

| Mutant | Historical carrier (restored) | Refreshed run (this round) |
|---|---|---|
| `M-P22b1-receipt-not-unique` | `….patch` / `….log` | `…-8910ebc6.patch` / `…-8910ebc6.log` |
| `M-P22b2-completion-instant-not-frozen` | `….patch` / `….log` | `…-8910ebc6.patch` / `…-8910ebc6.log` |
| `M-P23-upgrade-assumes-coherent-bindings` | `….patch` / `….log` | `…-8910ebc6.patch` / `…-8910ebc6.log` |
| `M-P24-readback-unbound-from-transition` | `….patch` / `….log` | `…-8910ebc6.patch` / `…-8910ebc6.log` |
| `M-P25-unchanged-answers-before-proof` | `….patch` / `….log` | `…-8910ebc6.patch` / `…-8910ebc6.log` |
| `M-CTL-refuse-every-realm` | `….patch` / `….log` — the **invalid** pair, still invalid | `…-8910ebc6.patch` / `…-8910ebc6.log` |

The `M-CTL` originals are the malformed pair described above. They are restored
to exactly the bytes that were always wrong — patch sha256 `0795f7f5…`, log
sha256 `519da4d9…` — because a carrier labelled invalid is only useful if it
still holds the artifact it labels.

Two independent readings of this are both on the record and are not merged: QA
judged historical preservation a PASS on the grounds that Git ancestry retains
every byte, and the audit judged it a finding on the grounds that the working
tree is what a reader reads. The audit's finding is the one acted on here. Both
stand.

### The baseline these logs name

The refreshed logs originally said `baseline commit : 5e00a238`. That was the
commit `git rev-parse HEAD` returned while the harness ran, but it is not the
state that was mutated: `5e00a238`'s `applications.rs` is `4e04205f…`, and
these runs mutated the file as `8910ebc6` holds it. Each refreshed log now
leads with the snapshot hash — the identity that actually pins a mutation — and
names the commit only as the way to reach it.

### The `M-CTL` control pair, re-established at this baseline

`M-CTL-control-pair-8910ebc6.{patch,log}` runs **both** halves under the same
mutant, in one process, serially, so neither half can be quoted without the
other:

| Test | Under `M-CTL` | What it means |
|---|---|---|
| `a_realm_stopped_before_the_current_schema_refuses_without_reading_or_installing` | **ok** | blind — a refusal is what it already expects |
| `a_current_realm_reaches_the_credential_reader_and_fails_after_the_gate` | **FAILED** | the half that distinguishes a correct refusal from a blanket one |

The stopped-schema coverage is therefore not self-certifying, and the control is
what makes it mean anything.

### One raw receipt is lost, and is not reconstructed

The `M-P22b2` run invalidated by the restoration defect wrote to the same path
its redo later used, so the redo overwrote it. No copy exists: it is not in
`/tmp`, and every committed version of that path is either an older round's
record or the redo itself (`8910ebc6`'s copy differs from the current refreshed
carrier only in the baseline label corrected above — same snapshot hash, same
compile, same run).

The run is therefore described but not evidenced. It is not reconstructed from
memory and no substitute is offered in its place, because a fabricated receipt
would be worse than an acknowledged gap. What the gap costs is small — the
invalidated run was superseded by a clean redo at the same mutation, and that
redo is fully recorded — but it is a gap, and it is named here rather than left
for an auditor to discover.

## The core-team projection: four mutants to reach one honest assertion

The occupancy chain was proved by `M-COH`. The core-team projection needed its
own, because its hybrid failure mode is different: it reads a native identity,
an occupancy generation, and a persona, and a split acquisition can pair a
generation-1 native with a generation-2 persona — a seat that existed at no
instant. Reaching a mutation that actually demonstrates this took four attempts,
and all four are retained.

| Mutant | Result | Why it is kept |
|---|---|---|
| `M-COH-TEAM-first-attempt-INEFFECTIVE` | survived | the mutation was wrong, not the test — it moved the persona read but left the barrier before the capture loop, so natives and personas were both read after the succession and agreed |
| `M-COH-TEAM-persona-after-release` | killed (101) | killed by the **escape guard**, not by the persona assertions — so it proves the barrier works, not that the hybrid is caught |
| `M-COH-TEAM-hybrid-persona-vs-native` | survived | escape guard silenced, persona read deferred alone — and it still agrees, because the deferred lookup is keyed on the generation captured in the *first* acquisition. A persona-only split is self-consistent. A fact about the split, not a hole in the test |
| `M-COH-TEAM-hybrid-generation-and-persona` | **killed (101)** | escape guard silenced, **generation and persona** both deferred — the faithful restoration of the pre-correction split. Killed on `role_persona.occupancy_generation` `left: 2, right: 1` against `native_seat.generation: 1` |

The last row is the one that matters: with the escape guard deliberately
disabled, the persona/native agreement assertion is what fails. That assertion
is load-bearing on its own.

The third row is the one worth reading anyway. It is a survival, it is kept, and
it is not relabelled — it says something true that the kill does not: deferring
only the persona cannot produce a hybrid, so a test that caught *that* would be
asserting something the code cannot do.
