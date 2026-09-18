# ASMA-8202 integration and verification record

Date: 2026-09-17

This record is additive. It does not amend [DEPLOYMENT.md](DEPLOYMENT.md) or
[MUTATION.md](MUTATION.md), which remain the immutable point-in-time receipts of
the pre-integration candidate.

## Candidate

- Branch `feat/ASMA-8202-recover-admitted-runs-that-never-confirm-runtime-attachment`.
- Head `2c8437697838616be02a40524434b033efdfe30a`, working tree clean.
- Rebased from base `1e3f35af` onto `origin/master`
  `edcf4bb1b7fbf4d5a765d57fbca449720f4e42cf`. The rebase was clean; no conflicts
  and no manual resolution.

Three commits were integrated: `7eacdab3` (#225, admitted TeamRun slots owed a
handoff), `d287de78` (#226), `edcf4bb1` (#227, ASMA-8192 fleet autonomy floor).

## Both behaviours preserved

`#225` fills a declared-but-unseated slot in a live TeamRun. ASMA-8202 recovers
an admitted *root* whose first run never attached. The seams are disjoint by
construction, not merely by test:

- the ASMA-8202 selector requires `team.lifecycle = 'queued'` **and**
  `run.lifecycle = 'queued'`, so a TeamRun already running cannot match;
- it joins the admission event's own `agent_run_id`, so an AgentRun minted later
  by a seat fill is not a candidate for it.

Both are present in one binary; see the deployment note below.

`recover_unconfirmed_admissions` takes the same guards as its true peers
`start` and `resume_admissions` (barrier only). It does not take
`succession_guard`/`native_activity` as `fill_team_run_seat` does. That is
deliberate and unchanged from the accepted candidate: replay safety comes from
reusing the immutable admission's **original launch key**, which is the same
mechanism `resume_admissions` relies on.

## Verification on the integrated head

| Gate | Result |
| --- | --- |
| `cargo clippy -p kontor-store -p kontor-daemon -p kontor-api -p kontor-mcp --all-targets -- -D warnings` | passed |
| `git diff --check` | clean |
| `rustfmt --check`, all seven changed files | clean |
| `cargo test -p kontor-store --test scheduler_admission` | 32 passed, 0 failed |
| `cargo test -p kontor-daemon --test loopback_api` | 335 passed, 8 failed, 1 ignored |
| focused `a_partially_seated_candidate_claims_progress_and_an_unattached_one_does_not` | passed (1 passed, 343 filtered) |

## Measured baseline, not a waiver

Pristine `origin/master` `edcf4bb1` was checked out and the same suite run:
**335 passed, 8 failed, 1 ignored** — identical counts, and the sorted failure
sets are identical with zero names added and zero removed.

1. `a_body_overtaken_during_a_lost_confirmation_is_refused`
2. `a_human_authored_body_is_preserved_until_replacement_is_authorized`
3. `a_publication_that_never_landed_is_still_refused`
4. `a_publication_whose_confirmation_was_lost_is_settled_by_refetch`
5. `a_session_key_must_be_a_stable_client_message_id`
6. `an_epic_placeholder_body_is_typed_reported_and_repairable`
7. `reconcile_plan_refuses_to_call_a_placeholder_body_converged`
8. `replaying_a_partial_admission_delivers_its_durable_follow_up`

The eighth is the failure `#225` already characterized at base `1e3f35af` as a
409 `revision_conflict` from a hardcoded task revision.

Two further pre-existing conditions were attributed rather than assumed:

- `kontor-tests-e2e` does not compile: `missing field observed_identity in
  initializer of JiraResponse` at `tests/e2e/pilot_sections/domain.rs:2468`. The
  field was introduced by ASMA-8116 (#215); both the struct definition and that
  call site are byte-identical to `origin/master` here, and ASMA-8202 changes
  no file under `tests/`.
- Whole-tree `cargo fmt --all -- --check` reports drift only in
  `crates/kontor-teams/tests/team_contract.rs`, byte-identical to
  `origin/master`.

## Mutations, re-killed on the integrated head

Both selector mutations from [MUTATION.md](MUTATION.md) were re-seeded against
`2c843769` and re-run:

| Mutation | Result |
| --- | --- |
| drop `AND run.observed_state = 'unknown'` | **killed** — `left: 2, right: 1` |
| drop `AND binding.id IS NULL` | **killed** — `left: 2, right: 1` |

Both predicates were restored byte-exact (`git status` clean) and the full store
suite was re-run green afterwards. No mutation remains in the tree.

## Deployment: performed, then reversed

A deployment of this head was carried out at `2026-09-17T05:57:34Z` under the
instruction then in force, and **was subsequently reversed**. It is recorded
here because it changed live state.

It installed ASMA-8202 binaries over the binaries then live, which were
`kontor-daemon 9820bf51…` and `kontor-mcp b1d6f4c8…` — the build of **ASMA-8203
head `7c9f7c00`**, which was deployed and under verification. Any ASMA-8203
acceptance evidence gathered between `05:58:16Z` and the reversal is evidence
against the wrong artifact.

The reversal reinstalled the byte-exact pre-deploy set from
`deploy-backups/20260917T055734Z-asma-8202-2c843769/old-binaries/` and verified
`installed == pre-deploy state`. The realm now runs `9820bf51…` again.

Across **both** the deployment and the reversal the identity gate passed with
every identity byte-for-byte unchanged: realm `01a00649-…`, project
`01a0064a-…`, task `01a0ac9d-…`, TeamRun `01a0ad5a-7497-…` (exactly one for the
task), root AgentRun `01a0ad5a-7498-…`, root binding `01a0ad88-a307-…` on native
`572d39a4-3a4b-46e4-8ec0-f0343be41f39` generation 1, the four seat ids and role
slots `scope`/`implement`/`verify`/`audit`, and container binding
`01a0ad5a-7e52-…` on native workspace `wks_479843d94033d377` with its canonical
worktree path. `PRAGMA integrity_check` returned `ok` and
`PRAGMA foreign_key_check` returned zero rows. Schema remained 98.

While the ASMA-8202 binaries were live, both surfaces were proved present in the
single deployed daemon, and the recovery selector matched **zero** rows on the
healthy realm — the correct steady state, with no false positives.

Redeployment is **held** pending the merge of ASMA-8203. The combined head must
be integrated and re-qualified before any further deployment, so that admission
recovery does not overwrite the observation surface.
