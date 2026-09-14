# ASMA-8116 high-audit report — round 5

Date: 2026-09-12
Artifact: `high-audit-report-round-5`
Task: `ASMA-8116` / `01a07722-c34b-74f3-9880-c3361ff88021`
PR: `#215`
Decision: **REJECT**

## Frozen audit boundary

| Boundary | Exact value |
| --- | --- |
| pushed head and configured upstream | `a04a85a479eee4ba173913800447c1f9c2f4c97e` |
| pushed-head tree | `75f5c5ba8cfbffcdfb464af82e2b6aa7894c552a` |
| implementation candidate / pushed-head parent | `170c0b02e5ec9510baaa6bc4b75bef30514a04e2` |
| implementation tree | `34ac7322a0c369bd9e63b567fd1b2f251604f825` |
| implementation parent | `527decf58cfcd68ad69f60dad5a5d30a3551ac83` |
| high-verification gate receipt | `01a09630-486d-7a01-9696-bd5e878316da` |
| verifier evidence hash | `a1384bbcd5149755941d108521c22f901a99afe7c8f10230eef131b8cf7c1d63` |

`git diff-tree` confirms that the pushed head adds only
`HIGH-VERIFICATION-ROUND-5.md`; all executable and schema inspection was bound
to its parent and tree above. The candidate was exported with `git archive` to
isolated `/private/tmp` workspaces for independent tests and mutations. No
production source was changed by this audit.

`HIGH-AUDIT-REPORT.md` remains the historical rejection of `527decf` and is not
the current verdict. This report neither rewrites nor supersedes that evidence;
it audits the later exact head and candidate.

## Verdict

**REJECT.** The store-side immutable identity, resolver, monotonic occurrence,
typed refusal, public projection and legacy-compatibility work holds under the
reviewed evidence. One release blocker remains in the epic write path: after a
fresh read proves a same-issue rename and the ledger advances to the current Jira
key, the resident epic reconciler builds its intent and outbound Jira transition
from the pre-observation delegation, which still carries the superseded key.

This violates the requested end-to-end current-key selector authority. Jira may
temporarily redirect an old key, but redirect behavior is not the confirmed
public identity contract and cannot authorize writes through an identifier that
the resolver has already retired.

## F-8116-A2 — high: an epic same-issue rename writes through the superseded key

The defect is a single stale object reuse in `Services::reconcile_jira_epic`:

1. `observe_delegation` is constructed with the entry key at
   `crates/kontor-daemon/src/applications.rs:3291-3299`.
2. Fresh Jira identity is decided at lines 3307-3321. A valid same-ID rename
   returns the new `current_key`, and the binding is reconciled before policy.
3. Status conflicts and the durable transition row use that new local key, but
   the provisional intent and final delegation at lines 3417-3429 reuse
   `observe_delegation` and change only the idempotency key.
4. `JiraIssueDelegation::intent` serializes `self.issue_key`, and
   `build_write_request` copies the same value into `JiraRequest.issue_key`
   (`crates/kontor-jira/src/jira.rs:1187-1247`).
5. The native connector uses that request key for field writes, assignment and
   transition paths (`crates/kontor-jira/src/connector.rs:478-540`).

The task path does not have this defect. It explicitly rebuilds its projection
and delegation under `current_key` after the identity decision
(`crates/kontor-daemon/src/applications.rs:3691-3723`).

### Independent reproducer

An audit-only copy of the retained resident same-ID epic test was given a valid
route out of `DRAFT` (`10236`, `TO BE GROOMED`, with the compatible direct-route
selector also offered) and a recorder for every non-GET request. Production code
remained byte-identical to `170c0b0`.

```text
CARGO_TARGET_DIR=/private/tmp/asma-8116-current-target.7vmnrI \
  cargo test -p kontor-daemon --test loopback_api \
  the_resident_reconciler_follows_a_same_issue_rename_through_the_connector \
  -- --exact --nocapture
```

Observed result:

```text
FAILED: an epic rename must continue under Jira's current key:
["/rest/api/3/issue/ASMA-1/transitions"]
```

The same response proved Jira key `MOVED-9` and immutable issue ID `901`; the
store advanced the same epic UUID to `MOVED-9`, and the superseded `ASMA-1`
stopped resolving. Nevertheless the outbound transition selected `ASMA-1`.

The retained test passes 4/4 with the exact candidate because its shared fixture
claims to offer an inviting transition but actually offers only `On hold`
(`10231`) from `DRAFT`. That is not the pinned generic epic route from `DRAFT`,
so no write is attempted. The same-ID test then asserts the binding and read
count but never asserts a current-key mutation. The anti-rebind tests' zero-write
assertions remain valid; they do not cover this successful same-ID branch.

Impact: the durable transition row names the current key while its canonical
intent hash and Jira request are built from the old key. The external effect is
therefore dependent on Jira alias redirection after Kontor has already made the
old alias non-authoritative.

Required correction: rebuild `JiraIssueDelegation` with the post-decision
`current_key` before computing any intent, dry run or apply, while reusing the
original observation. Retain a valid-route regression that records the emitted
path, asserts every write names `MOVED-9`, and preserves the zero-extra-read
invariant.

## Frozen-scope disposition

| Requested contract | Audit result | Evidence |
| --- | --- | --- |
| exact same-external-issue proof | **pass** | connector retains Jira top-level ID; both ledgers retain it; store suite proves same-ID task/epic rename with stable Kontor UUID |
| typed different-ID anti-rebind | **pass** | identity decision is terminal before policy/effect; resident task and epic tests offer live routes and record zero writes |
| uniqueness across epic/task ledgers | **pass** | immediate transaction plus cross-ledger checks and database constraints; deterministic uniqueness, ambiguity and race tests return typed conflicts |
| project and subject-kind checks | **pass** | exact canonical key parsing, project-scoped union resolver, subject-kind checks, foreign-project and wrong-kind regressions |
| read selector authority and revision | **pass** | confirmed resolver returns subject UUID/kind, current key, readback hash, confirmation time and subject revision; old key stops resolving after rename |
| write selector authority | **fail** | task write path rebuilds under the current key; epic transition path reuses the pre-observation key — F-8116-A2 |
| drafts blocked from delivery | **pass** | planned/unconfirmed rows project `AwaitingJiraBinding`, do not resolve as confirmed, and block activation/delivery until confirmation |
| public DTO/registry/client compatibility | **pass** | structured awaiting/confirmed DTO fields are registered in OpenAPI and regenerated in the console client; API contract and console generation/typecheck pass |
| storage-monotonic rename occurrence | **pass** | v95 has task and epic `BEFORE UPDATE OF rename_sequence` rewind guards; both A→B→C→A/rewind tests pass; M11 is killed |
| permanent/transient classification | **pass** | only connector/backend failures remain transient; durable ledger refusals map to 409 `stale_binding`; restored regression passes and M12 is killed |
| current conflict key | **pass** | content conflicts source the rebuilt task projection and report `MOVED-1`; restored regression passes and M13 is killed |
| legacy `JiraItemCode` / token / hash preservation | **pass** | `backlog_identity.rs`, `naming.rs` and legacy identity tests are unchanged from the integrated base; explicit legacy codes remain compatible and omission does not mint a namespace |

## Evidence and gate correlation

The durable role-turn ledger and repository commit chronology agree:

- scope turn `01a0780e-ce64-7c91-bfb6-23e78868d50a` records
  `high-scope-record`, evidence hash
  `60add5e84c652063f332cc93e018dab50eeaa26138f8abe04264292aa61666b0`;
- implementation turn 6 `01a09621-9a1a-7582-a905-5affe4f39559` records
  `high-change`, evidence hash
  `d07e7af274745e66cde32310355e0ef24baef78218982f0139e717d3ac5a0aa6`,
  immediately after candidate `170c0b0`;
- verifier turn 5 `01a09630-0a86-7a21-96ce-4f6cf0ec9d45` records
  `high-verification-report`, the supplied evidence hash
  `a1384bbcd5149755941d108521c22f901a99afe7c8f10230eef131b8cf7c1d63`,
  and current-runtime settlement at `2026-09-12T15:15:25.627648Z`;
- downstream implementation turn 7 `01a09631-f597-7c53-8549-502e8a03919d`
  also records `high-change`, evidence hash
  `5513b50396515c7c6b1e5c65dd654833a0c107f5420678a13be44fdc7f336194`,
  at `2026-09-12T15:17:31.331202Z`; Git independently proves the intervening
  pushed head is evidence-only, so this does not widen the executable candidate;
- gate receipt `01a09630-486d-7a01-9696-bd5e878316da` has intent hash
  `d521ff86dd5f237fe81762a71bc1b5939fc4fcb755771fa3365f2eff94bc1e69`
  and requests a passed `high-verification-gate` using
  `["high-change","high-verification-report"]`;
- append-only gate evaluation sequence 5 records that pass for verifier run
  `01a09541-637c-7883-ad90-6c8e0d51ebee` at
  `2026-09-12T15:15:41.805387Z`, after four recorded rejections.

The gate receipt and verifier report are authentic evidence for the round-4
repairs; they are not a substitute for this later independent audit. The new
finding is outside M11-M13's discriminating assertions and therefore does not
contradict their 3/3 mutation score.

## Independent test and mutation record

All Rust audit commands below ran against isolated exports of exact candidate
`170c0b0`; console checks ran from head `a04a85a`, whose only delta is evidence.

| Check | Independent audit result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| affected-crate clippy, all targets, `-D warnings` | pass from the fresh restored candidate export |
| `cargo test -p kontor-store --test jira_materialization` | pass, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | pass, 58/58 |
| `cargo test -p kontor-jira` | pass, unit 9/9 + native 19/19 |
| `cargo test -p kontor-api` | pass, library 23/23 + error envelope 5/5 + OpenAPI 3/3 |
| resident reconciler filter | pass, 4/4; F-8116-A2 explains the same-ID epic coverage hole |
| public task snapshot revision regression | pass, 1/1 |
| legacy epic backlog-code compatibility regression | pass, 1/1 |
| console `verify:api` | pass |
| console `typecheck` | pass |
| console tests | inherited failure, 299/300: expected `DeepSeek V4 Flash`, received `DeepSeek V4.1 Flash`; the failing file is absent from the ASMA-8116 delta |
| exact MCP bootstrap journey | expected inherited OQ-004 failure: `placement_blocked`, `the epic has no active immutable backlog code`, `started=[]` |
| audit-only valid-route same-ID epic probe | **fail**, emitted `/rest/api/3/issue/ASMA-1/transitions` after confirming current key `MOVED-9` |

The schema run is one green observation only. Per OQ-002 it does not determine
the failure-rate attribution of the load-sensitive concurrent-first-open test.

The documented round-4 mutations were independently re-seeded one at a time in
a fresh archive, killed by their exact retained test, then restored:

| Mutant | Observed kill | Baseline after restoration |
| --- | --- | --- |
| M11: remove task sequence monotonic trigger | rewind assertion failed, 0/1 | covered by restored 30/30 store suite |
| M12: classify every reconciliation error transient | observed 503 `unavailable` instead of 409 `stale_binding`, 0/1 | exact test passed, 1/1 |
| M13: use stale link key in conflict DTO | observed `ASMA-1` instead of `MOVED-1`, 0/1 | exact test passed, 1/1 |

Mutation score: **3/3 killed**. Restoration SHA-256 values equal the verifier's
exact-candidate hashes:

- migration 0095:
  `c2fe860c8808a77f81fae4d0819d1874dc467491295a2f68d66463866b3114b2`;
- `applications.rs`:
  `770fe61b447af2fabe31e715274d88b7416534eb3315fc2f896328ef8e7a8cbe`.

The codebase graph was freshly indexed at the exact head before structural
tracing. Cited Rust files have no recorded coverage gap. Migration 0095 is
partially parsed by the graph's SQL parser, so its full text and both trigger
definitions were inspected directly and exercised by the store and mutation
tests.

## Inherited conditions and integration ordering

- **OQ-002 remains open.** The independent 58/58 schema run neither blames nor
  clears migration 0095; the recorded repeated two-arm experiment is still
  required to settle failure-rate attribution.
- **OQ-004 remains open.** The independent MCP journey reproduces the intended
  `placement_blocked` outcome before any seat starts. No legacy code was minted,
  no Jira key was forced into `JiraItemCode`, and the token/hash contract was not
  changed to conceal the gap.
- **Console condition remains inherited.** Generation and typecheck pass; the
  unrelated DeepSeek display fixture remains 299/300 and is not waived by this
  audit.
- **Schema 0095 collision is not waived.** ASMA-8116 is intended to merge first.
  ASMA-8117 must be integrated afterward with its competing migration renumbered
  or rebased according to the integration sequence. This report performs and
  authorizes no merge or deployment.
- `git diff --check 527decf..170c0b0` still reports the verifier's non-source
  blank line at the end of historical `HIGH-AUDIT-REPORT.md`; the docs-only head
  delta itself is whitespace-clean. This is not the rejection basis.

No Jira, PR, Kontor lifecycle, gate, topology, merge or deployment state was
mutated by the audit. The release decision remains **REJECT** until F-8116-A2 is
corrected and a valid-route, current-key epic write regression passes at the new
exact candidate.
