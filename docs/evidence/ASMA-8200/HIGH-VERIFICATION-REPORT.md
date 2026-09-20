# ASMA-8200 high-verification report

Date: 2026-09-20
Artifact: `high-verification`
Task: Jira `ASMA-8200` / Kontor `01a0ac9d-a95c-7291-91e8-b472b7b331bc`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-verification`
TeamRun: `01a0b033-78bd-7691-90e0-a8a34eb46030`
Candidate: `ad4f47cb0017b4b9c88fc77debf25b520165c555`
Frozen base: `2e4b9985fc1073dd25c22b78349d8f36d6b1d4ec`

## Verdict

**REJECT.** One P1 contract defect remains in the startup precedence path.
The focused positive paths, daemon unit suite, all-targets check, in-scope
formatting and MCP parity are green, but a present valid stored policy is not
authoritative when the caller supplies an invalid seed. The implementation
validates the seed before it opens and reads the store, even though the frozen
scope says the seed is used only when no stored row exists.

No merge, gate, Jira transition, deployment or live restart is authorized by
this report.

## HV-8200-001 — P1 — the unused seed can veto an authoritative stored policy

The frozen decision in `HIGH-SCOPE-RECORD.md` is exact: a present stored
configuration is authoritative, and `DaemonConfig::capacity` is the seed used
only when no row exists. Candidate `ad4f47cb` does the opposite for one reachable
case:

1. `Daemon::start_with_supervision` validates `config.capacity` at
   `crates/kontor-daemon/src/lib.rs:447-451`;
2. only after that succeeds does it open the store at line 457;
3. only after the store opens does it replace the seed with
   `capacity_in_force` at line 463.

An independent temporary regression created an isolated realm, wrote a complete
valid stored policy (`9/7/5/3/2/6`, adaptive `2/1/5/1`), then started that same
realm with a zero `global_max_in_flight` seed. The expected result under the
frozen precedence rule was a daemon composed at stored global ceiling 9. The
actual result was:

```text
a present stored policy is authoritative over the unused seed:
Capacity { source: Invalid {
  subject: "CapacityConfig",
  rule: "every ceiling must be positive"
} }
```

Command:

```text
cargo test -p kontor-daemon --lib \
  a_present_stored_capacity_is_authoritative_over_the_seed -- --nocapture
```

Result: **FAILED**, 0 passed, 1 failed. The probe was removed immediately after
the run; `git diff --check`, an empty source diff and `git status` confirmed the
candidate source was restored before the remaining verification.

This is not the required fail-closed behavior for a present unusable durable
document: the durable document in the probe was valid. It is the caller's seed,
which the selected durable policy makes unused, that caused the refusal.

Required remediation:

1. select the durable policy before deciding whether the seed needs validation;
2. validate and return `StartupError::Capacity` for the seed only on the absent-row
   path;
3. retain `StartupError::StoredCapacity` for a present unreadable or invalid
   durable policy;
4. retain a regression equivalent to the independent probe above; and
5. update the startup documentation that currently promises all seed validation
   occurs before the state root is touched, because that promise and durable
   precedence cannot both hold for an existing realm.

## Positive verification

| Check | Result |
| --- | --- |
| `git diff --check` | pass |
| `cargo fmt -p kontor-daemon --check` | pass |
| `cargo check -p kontor-daemon --all-targets` | pass |
| `cargo test -p kontor-daemon --lib` | pass, 82/82 |
| stored unreadable/invalid startup refusal | pass, 1/1 |
| new reopen/effective/scheduler regression | pass, 1/1 |
| existing capacity CAS/replay regression | pass, 1/1 |
| configured-capacity admission regression | pass, 1/1 |
| `cargo test -p kontor-tests-contract --test mcp_parity` | pass, 12/12 |

The restart regression independently proves the intended ordinary path: with a
valid seed and valid stored replacement, the running process remains unchanged
until restart, the reopened configuration equals the stored document field for
field, revision and realm identity remain stable, `restart_required` clears,
and the scheduler's project projection uses mission ceiling 5 rather than the
seed's 12.

## Broader-suite and baseline results

`cargo test -p kontor-daemon --no-fail-fast` completed every daemon target. The
capacity regressions and all 82 library tests passed. The wider baseline is red:

- loopback API: 336 passed, 8 failed, 1 ignored;
- MCP journey: 1 passed, 1 failed;
- account pinning: 5/5 passed;
- quota observation: 21/21 passed;
- recovery security: 6/6 passed;
- succession handoff: 3/3 passed.

None of the nine wider failures is in a file changed by `ad4f47cb`, and the MCP
journey failure is the already-recorded missing-active-backlog-code integration
gap. This report does not waive those reds; their disposition remains open below.

`cargo test -p kontor-api --test openapi_contract` returned 2 passed and 1
failed because the checked-in `contract/openapi.json` differs from the document
the unchanged `kontor-api` source generates. Candidate `ad4f47cb` changes no
`kontor-api`, `kontor-core`, `kontor-runtime`, `kontor-scheduler`, workspace
manifest or lockfile path, so the candidate has no source edge into this test.
The live API-to-MCP oracle, which generates the contract in memory rather than
reading the stale snapshot, passed 12/12.

`cargo fmt --all --check` reproduces `OQ-8200-01`: only
`crates/kontor-teams/tests/team_contract.rs:467` differs, outside this candidate
and identically present at the frozen base. The in-scope daemon formatting check
passes.

## Evidence-integrity discrepancy

The high-change handoff says the startup loader and configuration read surface
both go through `applications::stored_capacity`. They do not. The startup loader
calls it at `lib.rs:373`; `Services::capacity_configuration` still deserializes
`StoredCeilings` directly and discards conversion errors with `.ok()` at
`applications.rs:20368-20376`. Public writes validate before persistence, and
startup validates before composition, so no second release blocker was assigned
for that pre-existing live-corruption edge. The high-change statement must still
be corrected or the read surface must actually share the helper before approval;
verification evidence may not claim a call path the candidate does not contain.

## Open-question ledger

### OQ-8200-02 — repository-wide format gate remains red

**Subject.** Whether the pre-existing `kontor-teams` rustfmt drift must be fixed
inside the ASMA-8200 candidate before a later verification can approve it.

**Attaches to.** This verification report and `HIGH-CHANGE-RECORD.md`
`OQ-8200-01`.

**Why ambiguous.** The frozen scope names `cargo fmt --all --check` as a minimum
gate but withholds unrelated crate changes. The exact hunk is present at the
frozen base and `cargo fmt -p kontor-daemon --check` is green.

**Options seen.** (a) fix the one base-format hunk in this candidate; (b) land it
separately and rebase; (c) approve an explicit baseline exception. No option is
selected by this verify seat.

### OQ-8200-03 — checked-in OpenAPI snapshot is stale at the unchanged base

**Subject.** Disposition of the failing checked-in OpenAPI parity pin.

**Attaches to.** This verification report's contract-parity rerun.

**Why ambiguous.** The parity test is required at independent verification and
is red, but the candidate changes none of the sources or dependencies from which
that test generates the document. Regenerating the snapshot would widen this
candidate into unrelated API and console artifacts.

**Options seen.** (a) repair and regenerate on a separate baseline task, then
rebase; (b) deliberately carry the generated API and console artifacts in the
remediation candidate; (c) approve a specifically owned baseline exception. No
option is selected here.

### OQ-8200-04 — unrelated broader daemon failures need baseline disposition

**Subject.** The eight loopback failures and one MCP journey failure from the
broader daemon rerun.

**Attaches to.** This verification report's broader-suite result.

**Why ambiguous.** None is in a candidate-modified path and the MCP journey gap
is already recorded elsewhere, but a clean independent baseline run was not
part of the implementation handoff. The failures therefore cannot be silently
called either candidate regressions or accepted baseline debt.

**Options seen.** (a) reproduce the exact failures at frozen base
`2e4b9985`; (b) reconcile the branch onto a clean governed base and rerun; (c)
record explicit owners and exceptions for each baseline red. Approval requires
one evidenced disposition; this rejected turn does not choose one.

## Handoff

Return the candidate to `implement` for HV-8200-001. Keep the same TeamRun and
worktree. The remediation handoff must include the authoritative-over-invalid-
seed regression, corrected evidence text for the actual shared-helper call
paths, and fresh focused checks. Independent verification must then rerun the
capacity matrix and dispose OQ-8200-02 through OQ-8200-04 before any merge or
deployment decision.
