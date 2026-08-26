# Committee failure remediation and clean re-review contract (ASMA-7869)

> **Status:** Architecture boundary for the non-destructive historical-round
> implementation in PR #123. PR #123 supersedes the obsolete PR #120 path;
> the published-revision preservation work in PR #122 is a prerequisite.
> This document is not a waiver, a finding or verdict fabrication path, an
> immutable-row repair procedure, or permission to reuse a poisoned run.
>
> **Delivery order:** merge and deploy PR #122, merge this corrected boundary,
> then rebase, renumber, regenerate, revalidate, independently review, and merge
> PR #123.

## Problem

A failed Committee decision is durable evidence. Advancing that same run to an
internal second round before governed remediation is complete can permanently
poison it: immutable non-compliant findings from the premature round cannot be
removed, overwritten, or made compliant by replacing their native seat
fillers. Completion therefore needs a non-destructive way to ingest the failed
decision, require independently authorized remediation, and consume only a
separate clean re-review.

Legacy runs that already advanced are also evidence. Recovery must reconstruct
their failed round without changing the run, confusing a settled internal
round-two result with round one, or choosing a convenient hash from a different
round.

## Contract invariants

### 1. A terminal failure freezes; it does not mutate into its re-review

When a Committee decision settles non-compliant, Kontor freezes both the
failed result and its remediation document on the source run. The source run
does not advance to another internal round and does not dispatch a re-review.
Its findings, result, remediation, receipts, and provenance remain immutable.

Historical runs that already advanced are handled by the legacy reconstruction
path below; the new path never converts a terminal failed run into its own
review successor.

### 2. Legacy reconstruction is exact and round-scoped

Kontor leaves an already-advanced legacy run untouched and reconstructs the
exact failed round-one settlement from its durable round-one findings and
remediation evidence. If that run also has a settled internal round-two result,
that result belongs to round two: it is not compared with, substituted for, or
rewritten as the reconstructed round-one result.

Reconstruction fails closed when the evidence cannot identify one exact
conjunctive settlement. It never edits findings, results, remediation rows, or
the consultation run.

### 3. Completion owns immutable failed-round and remediation evidence

The completion state records a `CompletionRound` containing the failed
Committee run, its evidence hash, its result hash, and its remediation hash.
Ingesting that evidence moves the completion from `Verdict(1)` to
`AwaitRemediation(1)` (exposed as `awaiting_lsa`) without changing the source
Committee run.

The failed-round record is the durable basis for both remediation commands and
the later clean re-review. A different run, round, evidence document, result,
or remediation document cannot be silently substituted.

### 4. Remediation requires two distinct, current, seat-scoped authorities

The LSA proposal and TPM route are different durable commands:

- the proposal must be authored by the current ECP LSA `SeatBinding` using the
  bearer for its current occupancy generation;
- the route must be authored by the current ECP TPM `SeatBinding` using the
  bearer for its current occupancy generation; and
- the two bindings must be distinct.

Realm-operator authority, a wrong role, a foreign-project seat, a stale
occupancy generation, or one seat binding attempting both halves is rejected.
Recording the LSA proposal alone does not move completion. The TPM route moves
`AwaitRemediation(1)` to `Remediating(1)`. The governed integration evidence
then moves completion to `Verdict(2)`.

Each command is claimed atomically with its receipt, effect, compare-and-swap
transition, and scheduler wake. Exact replay returns the original result;
conflicting replay or an unclaimed effect fails closed.

### 5. Re-review is a separate, provenance-fenced Committee run

After remediation integration is frozen, the authorized caller invokes a new
Committee run with a canonical `re_review` provenance document naming:

- the failed completion round and completion revision;
- the failed Committee run and failed result hash;
- the governed remediation hash; and
- the remediation integration receipt digest.

Kontor reconstructs and validates this evidence itself, verifies that it
matches the completion freeze and pinned Committee template, and freezes the
server-derived evidence with the new run before any native seat placement.
The new run has its own identity, seats, findings, Judge settlement, and
receipts; the failed source remains unchanged.

Only one new run may claim a canonical completion freeze. Exact idempotent
replay returns the original run. A concurrent or differently keyed duplicate
conflicts before launch, so duplicate reviewers are not placed.

### 6. `Verdict(2)` consumes only the matching clean settlement

Completion at `Verdict(2)` accepts exactly one settled Committee run whose
frozen re-review provenance matches the completion's failed-round,
remediation, and integration evidence. The failed source run, a legacy
internal round two, or any other poisoned or unlinked run is excluded.

The ordinary pinned-template rules still apply. Every required reviewer
finding must be durable and the Judge must settle the run. For the conjunctive
Independent Review profile, all required reviewers and the Judge must be
compliant; missing evidence or a non-compliant required seat cannot be waived
by the re-review path.

## Persistence boundary

PR #123 currently carries three append-only persistence changes for:

1. durable Committee remediation-round evidence;
2. generation-scoped LSA remediation-proposal authority; and
3. atomic remediation command claims plus unique clean re-review provenance.

PR #122 is deployed at schema 63 and owns
`0062_profile_selection_outcomes.sql` plus
`0063_imported_profile_selection_outcomes.sql`. After this boundary lands, PR
#123 must be rebased and its three branch-local migrations renumbered
append-only to 0064, 0065, and 0066. Every generated schema/OpenAPI/console
artifact must then be regenerated and checked. No deployed migration may be
renamed or rewritten.

The previous proposal to add a mutable `completion_round` selector to an
existing consultation run is not this contract. Completion linkage is frozen
as canonical provenance on a distinct re-review run.

## Required regression evidence

The implementation is not releasable without focused and broad evidence for
all of the following:

- a new non-compliant source run becomes terminal with frozen result and
  remediation evidence and emits no round-two placement or messaging;
- both running and settled legacy advanced-run shapes reconstruct the exact
  failed round-one result, while a settled internal round-two hash remains
  scoped to round two and is neither compared nor rewritten;
- `CompletionRound` records the exact source run/evidence/result/remediation
  hashes and enters `AwaitRemediation`;
- current-generation, distinct ECP LSA/TPM seats succeed, while operator,
  wrong-role, foreign-project, stale-generation, and same-seat attempts fail;
- proposal, route, effects, receipts, state transition, and wake are atomic,
  replay-safe, and crash-recoverable;
- the server validates and freezes canonical clean re-review provenance before
  native placement, and concurrent duplicate keys yield one run, one conflict,
  and no duplicate seat launch;
- only the matching settled clean run can satisfy `Verdict(2)`; the failed
  source and any poisoned legacy round are excluded;
- reviewer cardinality, immutable findings, Judge settlement, template
  matching, result hashes, and conjunctive aggregation remain exact; and
- store migrations, schema snapshots, generated API/CLI/MCP surfaces, daemon
  loopback coverage, formatting, linting, and the broad workspace suite pass on
  the exact reviewed commit after the final rebase and migration renumbering.

## Explicitly out of scope

- no waiver, fabricated finding, fabricated Judge verdict, or authority
  substitution;
- no manual edit, deletion, withdrawal, or replacement of immutable Committee
  findings, results, remediation rows, receipts, template revisions, or source
  runs;
- no cleanup of historical runs as a condition of completion;
- no reuse of a poisoned run as the clean re-review;
- no credential disclosure or operator credential standing in for an LSA,
  TPM, reviewer, or Judge seat;
- no ASMA-8001 product-code change in these control-plane PRs; and
- no modification, deletion, retirement, or garbage collection of preserved
  CAT-11 local artifacts.
