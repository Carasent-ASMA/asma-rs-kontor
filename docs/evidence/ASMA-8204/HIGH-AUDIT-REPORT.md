# ASMA-8204 high-audit report

- Date: 2026-09-20
- Artifact: `high-audit-report`
- Task: `ASMA-8204` / `01a0ac9d-a96e-7de1-abfe-3d340ec5fea4`
- Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
- Audit run: `01a0bbcc-67d8-7bd0-8ab4-ff0e435d3e6f`
- Role slot: `audit`
- Producer account: `NULL` (`native_proved_unknown` after settlement)
- Acting evaluator account: `01a00751-5be9-7281-bba5-75d8c0c101e7`
- Evaluator role: `fleet-spec-auditor`
Decision: **PASS**

## Decision

ASMA-8204 satisfies the accepted high-scope contract at the qualified rebased
candidate and at the integrated PR254 candidate. There are no open P0 or P1
findings against the current code. The only P1 raised in this change's history,
HV-001, remains preserved as a real rejection of `0e5a0bec` and is closed by
component-correct containment plus non-vacuous unit and end-to-end regression
coverage.

This PASS is an audit verdict, not integration, release, deployment, native
cleanup, or lifecycle authority. No such action was taken by this seat.

## Exact source boundary

| Boundary | Commit | Tree | Disposition |
| --- | --- | --- | --- |
| current master base | `24290153c42c21743d606bcf5e1648681af4a69a` | `186dcd0a9ee8c3527197b3d347862cbd20f80a40` | integration base |
| auditor-qualified rebased candidate | `ebd23df44469897c91cdc42bfc206ebd7d267366` | `864a6bbffcaa4837f0f77cdd9b98a3e017723f64` | directly inspected and tested |
| earlier full-workspace candidate | `7aefe70a953d1048f52a562e8c52be832fbbc0ee` | `f5a5c0e60c8eadc41f3cf0fcb0605758ea2fe409` | full locked workspace suite passed on base `0f6498246d8c873a37453ec35e4a1db7d6be1468` |
| PR254 merge commit | `e8d96c9f18635193f503e6f7aeafc7e3e57c6655` | `2bfcc7d31488b3afa3ebbf83f9aa1994a48316b9` | current master plus ASMA-8204; test blocks append-ordered |
| integrated PR254 candidate | `5bb38a62085e7ee64582400497936204329bed94` | `9b88893b2df0a0376d6d912170e140f72d6b2714` | release-owner candidate; adds one e2e fixture correction |

The full trees of `ebd23df4` and `5bb38a62` are intentionally not byte-equal.
Direct object comparison proves:

- every path other than the two merged regression files and the e2e fixture is
  byte-identical;
- therefore all production files are byte-identical;
- `crates/kontor-daemon/tests/loopback_api.rs` has the same line multiset at
  both commits, SHA-256
  `24de08fcdfa2c29f951477a82d7f6555e7bffee552b14d9a7abf51a2a20d5d76`;
- `crates/kontor-runtime-paseo/tests/contract.rs` has the same line multiset at
  both commits, SHA-256
  `03be668df6323cb2b972719bcfef09ec31bec633b9e5ae32952813b12dd43f96`;
- the only remaining source delta is
  `producer_account: fixture.subject.account` to
  `producer_account: Some(fixture.subject.account)` in
  `tests/e2e/pilot_sections/gates.rs`.

The test-file difference is relocation of identical complete test blocks, not
assertion, fixture, or behavior drift. This is exact production equivalence and
exact regression-content equivalence, while explicitly not claiming equal Git
trees.

## P0/P1 findings

### Current candidate

- P0: none.
- P1: none.

### Preserved historical finding

`HV-001` was a P1 against rejected candidate
`0e5a0becc700759c04bd710d69f1df4426f2ad99`: string-prefix containment treated
`/dangling-session` as outside an admitted filesystem root `/`, allowing an
irreversible exact-ID project removal to proceed while a live unarchived
session remained. The authentic rejection is
`artifact-asma-8204-high-verification-report-0e5a0bec` revision 1,
revision ID `01a0b34f-e702-7973-8276-6ac7a86f97be`.

The corrected implementation compares `std::path::Path` components. It keeps
textual-prefix siblings outside, makes `/` contain every absolute descendant,
and fails closed on malformed runtime paths. The correction was originally
implemented at `158fc5a0`, is represented after rebase by `b7bc0942`, and is
present unchanged in both current candidates. The current approved verification
artifact records HV-001 `CLOSED`.

## Regression results

### Auditor-run current-master qualification at `ebd23df4`

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo test -p kontor-runtime-paseo` | PASS: 119 unit + 268 contract = 387 passed; 6 live-only ignored |
| `cargo test -p kontor-store a_retired_but_unarchived_child_blocks_its_parents_archive` | PASS |
| exact daemon `an_epic_root_archives_last_and_only_when_its_completion_is_done` | PASS |
| exact current-master compatibility regression `a_new_epic_reports_leadership_declared_but_not_materialized` | PASS |
| `cargo clippy --workspace --all-targets --exclude kontor-tests-e2e -- -D warnings` | PASS |
| `git diff --check 24290153...ebd23df4` | PASS |

The complete locked workspace suite was also run at exact commit `7aefe70a`,
tree `f5a5c0e6`, before the final master advance. It passed across the workspace,
including 421 passed / 1 ignored in `loopback_api`, both MCP journeys, both e2e
pilot targets, and all doc tests. The final rebase onto `24290153` was then
qualified with the current-master focused and affected broad checks above.

### Integrated PR254 qualification at `5bb38a62`

The integration owner reports all five scope-required archive verification
groups passing at the exact integrated candidate, including the preserved
closeout and committed-artifact-registry regressions. The integration owner
also reports:

```text
cargo clippy --locked -p kontor-tests-e2e --all-targets -- -D warnings
PASS
```

The auditor did not repeat the complete suite for a test-order-only merge plus
the one-line fixture type correction. Direct Git-object comparison provides the
transfer proof above; the integration-owned test results are identified as
such rather than represented as auditor-run commands.

## Mutation results

Mutation score: **9/9 killed**, with every mutation restored.

| Mutant | Seeded defect | Killer | Result |
| --- | --- | --- | --- |
| `MUT-009a` | remove all-children-archived store guard | retired-child parent-archive store regression | KILLED |
| `MUT-009b` | restore prior native-root archive refusal | epic-root closeout regression | KILLED |
| `MUT-8204-c` | bypass completion gate | epic-root closeout regression | KILLED |
| `MUT-8204-d` | admit any terminal phase, not exactly `Done` | epic-root closeout regression | KILLED |
| `MUT-8204-e` | remove zero-workspace guard | unsafe-root-target contract | KILLED |
| `MUT-8204-f` | trust removal acknowledgement and skip absence readback | acknowledgement-is-not-proof contract | KILLED |
| `MUT-8204-g` | let a root name a parent project | request-shape contract | KILLED |
| `MUT-8204-h` | remove `projectRemove` capability gate | unsafe-root-target contract | KILLED |
| `MUT-8204-i` | restore rejected prefix-with-separator containment | containment unit test and filesystem-root live-session contract | KILLED at both seams |

The current high-change artifact records all nine mutants re-seeded and killed
against the corrected tree. Independent verification records the prior eight
as killed at the predecessor seam and independently re-killed `MUT-8204-i` at
both the helper and end-to-end effect seams. No mutation result is inferred from
a clean diff alone.

## Baseline e2e lint discrepancy

On current-master base `24290153` plus `ebd23df4`, the otherwise-green strict
workspace clippy command failed while compiling `kontor-tests-e2e` at
`tests/e2e/pilot_sections/gates.rs:1043`:

```text
expected Option<AccountProfileId>, found AccountProfileId
```

The e2e file was outside the ASMA-8204 delta. Current master had changed
`producer_account` to `Option<AccountProfileId>` but had not wrapped this fixture
value. Excluding only `kontor-tests-e2e`, strict workspace clippy passed. This
was a baseline integration discrepancy, not an ASMA-8204 P0/P1 and not a waived
failure.

Candidate `5bb38a62` supplies exactly `Some(fixture.subject.account)`, and the
integration owner reports the locked strict e2e clippy target passing. The
discrepancy is therefore resolved in the integrated candidate.

## Evidence provenance

- High-scope document:
  `docs/evidence/ASMA-8204/HIGH-SCOPE-RECORD.md`, SHA-256
  `c754710065733569780d85edbebdb2f4beb99dd04079f821dc4fbb9d553eeee8`.
- Current high-change producer document:
  `/private/tmp/8204-artifact-2.json`, SHA-256
  `24d70ede2bf5c8b761abe825ebf4a5e9842bea0b4534816308760bc9f885ae29`.
- Committed high-change record:
  `docs/evidence/ASMA-8204/HIGH-CHANGE-RECORD.md`, SHA-256
  `6fe686fa93fa2ff115a3f49526a8e5314d86e88379fabe748a681c6e59e55f46`.
- Approved current implementation artifact:
  `artifact-asma-8204-high-change-81367993`, revision 1,
  `01a0b53f-c3c7-7a61-96b9-8b016040716f`.
- Approved current verification artifact:
  `artifact-asma-8204-high-verification-report-81367993`, revision 1,
  `01a0b622-5c9c-7ed0-9050-f1513d329993`.
- Both current memory histories report
  `history_unavailable: false` and `legacy_last_write_wins: false`.
- The historical rejected high-change/report identities are retained and are
  not overwritten or re-labelled as current evidence.

## Control-plane disposition

The exact live audit run is attached and running with
`account_profile_id: null`. That is the report producer's authentic state and
must remain `NULL`; it is not replaced with the account used to record a later
gate verdict. The committed-artifact registry can attribute the settled report
as `native_proved_unknown` from the run's exact native proof.

The separate acting evaluator account is verified as
`01a00751-5be9-7281-bba5-75d8c0c101e7` (`Igor · Local Paseo`). The current
native session readback identifies the unaliased local `codex` provider on
`paseo-local`; the supported provider catalog distinguishes that route from
`codex-work` and `codex-personal`, and the supported Kontor account read maps
the local Paseo acting profile to that exact enabled account. No producer
account is inferred from this acting-account evidence.

The report verdict is PASS, but the high-audit gate is not recorded in this
checkpoint. The newly deployed committed-artifact registry requires an exact
settled turn that already claims `high-audit-report`; that receipt cannot exist
before this turn's terminal response and settlement. In addition, the root
coordinator is diagnosing the gate receipt classification path. The precise
post-turn requirement is:

1. settle this audit turn on run
   `01a0bbcc-67d8-7bd0-8ab4-ff0e435d3e6f`, role slot `audit`, task revision 2,
   claiming `high-audit-report`, with the runtime's exact correlation proof;
2. have the coordinator register this committed report blob using the
   supported CLI, that settled role-turn ID, its full evidence commit,
   repository-relative path and verified SHA-256;
3. reread the artifact receipt and the current workflow revision;
4. after the gate receipt classification issue is cleared, record
   `high-audit-gate = passed` with acting evaluator account
   `01a00751-5be9-7281-bba5-75d8c0c101e7`, evaluator role
   `fleet-spec-auditor`, and the four declared durable artifact evidences;
5. reread the gate receipt and task/workflow projection before treating the
   gate as passed.

Until those receipts exist, the substantive verdict is PASS and the native
gate remains precisely pending. Completion of this audit creates no authority
for native cleanup.
