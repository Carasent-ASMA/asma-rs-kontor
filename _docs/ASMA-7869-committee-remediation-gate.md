# Committee remediation gate — exact second-patch boundary (ASMA-7869)

> **Status:** Plan of record for the follow-on PR. PR 1 (`fix/ASMA-7869-completion-historical-committee-round`)
> ships the read path that unblocks the live 503; this branch carries the exact
> patch boundary for the behavior change that prevents the failure from
> recurring and gives ASMA-8001 a clean re-review. Not code — the boundary the
> next PR must implement.
> **Author:** ASMA-7869 implementer
> **Flag:** LSA — this is the second half of the live-epic recovery; PR 1 alone
> takes the completion to `awaiting_lsa` → `remediating`, and then stalls at
> `Verdict(2)` because the live run's round-two findings are frozen
> non_compliant (immutable, not deletable, seat replacement cannot fix them).

## Why the current auto-dispatch is wrong

`settle_committee_run` (crates/kontor-daemon/src/applications.rs, `settle` arm)
records only a recommendation/tried_path, then immediately calls
`store.remediate_committee_run` — which advances the run to round two and
`dispatch_committee_round_two` — which messages the same native seats to
re-review. In ASMA-8001 this launched round two before CAT-12 existed:
reviewer-b durably recorded non_compliant for round two, and because
`committee_findings` is immutable and the aggregation is conjunctive, the
round-two verdict of that run can never become compliant. Replacing native
seats changes nothing; the frozen finding is the verdict.

Required supported behavior (from the live-epic operator brief):

1. A failed review must not dispatch re-review until the governed remediation
   is durably completed/frozen.
2. Existing immutable premature findings/runs remain untouched.
3. There is an identity-safe way to launch a clean re-review after completion
   remediation, bound to the correct completion round, so ASMA-8001 can finish
   without waiving or fabricating a finding.
4. Replay/crash safety, template matching, and conjunctive settlement are
   preserved; current settled-compliant behavior remains.

## Design (smallest robust)

One durable binding plus one settle change; the completion machine is the
remediation gate it was already designed to be.

### 1. Non-compliant settle stores its result and does not advance

In `settle_committee_run`, the `outcome == NonCompliant` branch with
`run.round < template.round_limit` must:

- write the immutable remediation row exactly as today (same document shape,
  same `failed_result_hash`), and
- persist the frozen result via `store.advance_consultation_run(...,
  ConsultationRunState::Settled, Some((&result, result_document.hash())))` —
  the same call the compliant branch already makes.

It must NOT call `store.remediate_committee_run` (no internal round advance)
and NOT call `dispatch_committee_round_two`. The run ends `settled` at its
internal round with a durable non-compliant result; completion consumes it
through the existing settled path (and the PR-1 historical path stays as the
read path for runs that already advanced — the live run).

Consequence: `remediate_committee_run` and `dispatch_committee_round_two`
become legacy-only (the internal round-two machine is replaced by fresh runs).
Keep them for historical runs that are already mid-advance; the follow-on may
remove the dispatch call sites only.

### 2. Fresh re-review bound to a completion round

Migration `crates/kontor-store/migrations/0060_committee_completion_round.sql`:

```sql
ALTER TABLE consultation_runs ADD COLUMN completion_round INTEGER
    NULL CHECK (completion_round IS NULL OR completion_round BETWEEN 1 AND 2);
PRAGMA user_version = 60;
```

- `invoke_committee_run` accepts an optional `completion_round` (epic scope
  only). It is frozen with the run (add to `StoredConsultationRun`, the
  migration-0036 frozen-input trigger list, `freeze_committee_run`, the
  `invoke` intent document, and `consultation_run_id`/row readers).
- Uniqueness: refuse a second run bound to the same `(project, mini_project,
  completion_round)` — the completion machine must never have to choose.
- `observe_completion` Verdict(round): match a settled run by
  `run.completion_round == Some(round)` first; fall back to the existing
  `run.round == round` legacy match, then the PR-1 historical path. Template
  matching stays name-only (PR 1).

### 3. ASMA-8001 recovery flow (after both PRs land)

1. Completion `Verdict(1)` ingests the historical failed round (PR 1) →
   `awaiting_lsa` → LSA proposes, TPM routes → `remediating(1)`.
2. The remediation TeamRun performs the governed remediation (CAT-12 +
   waivers) and lands; TPM advances completion with the integration evidence →
   `Verdict(2)`.
3. TPM invokes a **fresh** Committee run (`committee-runs:invoke` with
   `completion_round: 2`, current template revision). New run id, new seats,
   new CSW — nothing about the stuck run is touched.
4. Fresh reviewers review the post-remediation state; conjunctive
   compliant → settle compliant → completion `Verdict(2)` consumes it →
   `closeout`. The stuck run stays as immutable evidence of round one's
   failure and the premature round-two review.

## Test boundary (must land with the follow-on)

- Update `a_seeded_committee_runs_and_settles_instead_of_returning_503`:
  non-compliant settle now answers `settled` with a non-compliant `result` and
  the remediation row, and does NOT open round two (no `LaunchConsultation` /
  `MessageConsultation` for the round-two dispatch). The round-two part of
  that test moves to a fresh run bound to `completion_round: 2`.
- New: a fresh run invoked with `completion_round: 2` settling compliant is
  consumed by completion `Verdict(2)` → `closeout`, while the legacy run at
  internal round 2 is ignored for that round.
- New: invoking a second run for the same `completion_round` conflicts.
- New: settle at `round == round_limit` non-compliant still escalates to
  `needs_human` (unchanged).
- Existing PR-1 tests must stay green unchanged (historical path for the
  already-advanced live shape).

## Explicitly out of scope

- No change to `committee_findings` / `committee_remediations` immutability.
- No waiver or fabricated verdict path anywhere.
- No deletion of the live stuck run or its rows.
