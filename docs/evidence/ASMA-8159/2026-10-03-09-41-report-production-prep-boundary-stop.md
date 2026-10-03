# Production-prep credential-boundary stop — ASMA-8159

> **Date:** 2026-10-03 09:41 Europe/Oslo
> **Status:** 🔴 Draft — stopped at reserved boundary; TPM/LSA routing required
> **Category:** report
> **Scope:** ASMA-8159 / MEM-04 / TASK-004 of ASMA-8155; Paseo-direct
> **Summary:** Source inspection found no supported production Cognee alias registration/resolution path. Item 6 of the resumption brief requires a stop before inventing a credential boundary. This additive evidence checkpoint preserves every accepted source byte and records remote/default divergence and outstanding obligations.

## When to load

Load when routing the credential decision, resuming this same writer, or admitting
the eventual combined candidate. This report is a stop finding, not a completed
production-prep implementation or an independent verdict.

## Authority and immutable inputs

Operational authorization:
`44d5d916-d778-46b8-b00d-2cca01cc6553/8155-production-prep`.
The standing-authority record was read at
`/Users/igor/.local/state/asma/watchdogs/kontor-closeout-20261003/standing-authority.json`;
it delegates task-scoped technical decisions through the existing LSA and keeps
subsequent narrower user constraints in force.
The current user brief and the TPM's production-prep resumption record authorize
source/fake work and preparation; they prohibit real credentials, production
corpus access, runtime activation, deployment, push/PR and Jira actions.

Branch: `feat/ASMA-8155-kontor-experience-memory-and-analogue-recall`.
Checkout: `/Users/igor/.paseo/worktrees/0n8yzbno/feat-asma-8155-kontor-experience-memory-and`.
Starting HEAD: `fe5373f2d2a2c147a6e2b6398c07b5091eaa1ca0`, tree
`c95275e6ecb307a24d0fb0d2accd63fa127099e1`.

| Accepted pin | Exact SHA | Carried acceptance |
| --- | --- | --- |
| ASMA-8156 | `655e07ead8b87bebf3a5d43ddccc15c860114e05` | QA+Audit PASS |
| ASMA-8158 | `831dd7c1dc95a57258c75f5896bff5b76f3830fa` | QA+Audit PASS; auditPassed=true |
| ASMA-8157 docs | `7d19280857ae3c60a870e017da3f733f9ff2625b` | Corrective docs delta independently reviewed PASS, per TPM record |
| Prior ASMA-8159 package | `fe5373f2d2a2c147a6e2b6398c07b5091eaa1ca0` | Local package accepted; F1/F3/F4 carried |

LSA contract:
`/private/tmp/ASMA-8155-full-lsa-readiness-original-20261002.txt`, SHA-256
`7ee35150e2e90cdc4fab83bac999ecd2cef6286d236df128b0ad5b8c211d828b`.
TPM record:
`/Users/igor/.local/share/paseo/epic-projects/ASMA-8155/2026-10-02-22-06-readiness-ASMA-8155-delivery-admission-prep.md`,
production-prep resumption, lines 121–128. Its owner/status observations are
consumed by reference; no owner was contacted and no external lane was modified.

## Reserved decision and evidence

The resumption brief says: “If a genuinely reserved boundary or design decision
appears (credential boundary, canonical authority, outbound-data scope), stop
and report it to the TPM/LSA instead of inventing an answer.”

The LSA contract at line 95 also requires Cognee aliases to be separate
deployment inputs resolved through an existing supported credential boundary,
and forbids Jira-secret reuse or Keychain/configuration amendments in memory work.

| Source | Observed contract |
| --- | --- |
| [runtimes.rs](../../../crates/kontor-daemon/src/runtimes.rs) lines 422–435 | Explicitly says the daemon has no configuration surface for approved config homes/keychain targets; creating that surface belongs to account/CLI work. |
| [resolver.rs](../../../crates/kontor-accounts/src/resolver.rs) lines 1–34, 896–913 | Approved alias-to-target policy exists for account profiles. Its resolved material has private fields and deliberately exits only through `Command::env`; there is no supported HTTP-secret accessor. |
| [Jira credentials](../../../crates/kontor-jira/src/credentials.rs) lines 14–41 | The implemented registration/lookup scope is Jira-specific: immutable Realm plus canonical-root hash, service `kontor-jira:{realm}:{root_hash}`. It cannot be adopted as a Cognee scope. |
| [GitHub publication](../../../crates/kontor-daemon/src/github_publication.rs) lines 10–17 | Its credential input is a configured GitHub App private-key path, not Cognee alias resolution. |
| [Cognee client](../../../crates/kontor-memory-cognee/src/lib.rs) lines 140–158 | `Client::new(config, Option<SecretString>)` accepts already supplied material and performs no Keychain/env lookup. This is a transport injection seam, not an alias resolver. |
| [Daemon config/start](../../../crates/kontor-daemon/src/lib.rs) lines 344–349, 572–586 | `with_memory_cognee` injects an existing client. Enabled file configuration without one refuses startup. |
| [Executable startup](../../../crates/kontor-daemon/src/main.rs) lines 294–310 | Shipped `serve` constructs ordinary `DaemonConfig` and calls `start_configured`; it does not compose a Cognee client. |
| [Rebuild operation](../../../crates/kontor-daemon/src/applications.rs) lines 19263–19286 | Checks preview freshness and returns typed `projection_unavailable`; no qualification/activation production caller exists yet. |

The read-only source search excluding test directories for `ResolverPolicy::builder`,
`AccountResolver::new`, `.keychain(` and `KeychainTarget::new` returns only Jira's
target constructor and account transport fixtures. The cached codebase graph
was used as a locator; these findings were corroborated against actual checkout
source. Absence is scoped to this tree and its shipped composition path.

Proceeding would require choosing or adding the trusted Cognee alias registration
surface, target scope/namespace and secret extraction mechanism. The existing
account policy is useful infrastructure, but does not supply those production
inputs or authorize a new extraction path. This writer made no such decision.

The TPM/LSA disposition needed to resume is either an exact existing supported
Cognee credential resolver/registration path with its ownership and contract,
or an explicit owner-routed design disposition for the missing composition
boundary. Memory code must continue receiving supplied material; configuration
stays disabled by default. No real secret is needed for this disposition.

## Commands and results

[verify-boundary-stop.py](verify-boundary-stop.py) reproduces the read-only
capture. [production-prep-boundary-stop.json](receipts/production-prep-boundary-stop.json)
records its exact argv, source digest, command argv/cwd/start/end/exit/stdout/stderr,
source excerpts with whole-file hashes, ancestry and integrity results.

Invocation: `python3 docs/evidence/ASMA-8159/verify-boundary-stop.py`.

| Check | Result |
| --- | --- |
| Status, HEAD/tree, branch, accepted-pin ancestry | Correct branch/base; accepted 8156/8158/prior package reachable; initial tracked tree clean; adapters excluded |
| Accepted non-evidence source manifest | 1,142 files checked, zero changes; files-map SHA-256 `81c6d5ac5e55c809deb367d47e2b064c2cac6878daf314d63858099d3ca9a239` |
| Both actual remote default heads | Read with `git ls-remote --symref origin HEAD`; exact pins below |
| Read-only divergence and merge-base checks | Module 0/3; root versus docs 10/3; exact merge bases below |
| `asma git --help` | Exit 0; no rebase/cherry-pick/replay/local branch-merge verb exposed |
| Migration file numbering | 120 migrations, max 120; zero missing or duplicate numbers |
| Source constructor search and excerpts | Captured with source file SHA-256; no credential backend invoked |
| `git diff --check` | Exit 0 |

Runtime suites, mutations, clippy, contract/lane generation, cargo-deny and F4
were not run in this resumption: implementation stopped at the reserved boundary
before source edits. Earlier `fe5373f2` receipts remain historical evidence and
are not represented as a fresh combined-source qualification.

## Remote/default and migration reconciliation

| Repository | Actual remote default | Divergence from relevant frozen candidate |
| --- | --- | --- |
| `Carasent-ASMA/asma-rs-kontor` | `refs/heads/master` → `8acdbd17e83d9d7ec545221c4b957e0325916997` | Starting `fe5373f2` is 3 ahead / 0 behind; merge base is the remote head. No upstream module advance relative to the accepted source base. |
| `Carasent-ASMA/asma-modules` | `refs/heads/master` → `25de2b541da5d8dd61082e2d150c5c8087b09ac6` | Accepted docs `7d192808` is 3 ahead / 10 behind; merge base `f77c3b31bd73148a93cb3370d0ae34bf807fe2ff`. Docs reconciliation remains with its owner. |

The evidence checkpoint adds one local commit; counts above intentionally pin
the user-supplied starting candidate. Remote reads did not fetch, switch, pull,
rebase or merge. The installed CLI exposes `merge-pr` (attested squash PR merge),
but no supported rebase/cherry-pick/replay or local branch-merge operation for
external candidate reconciliation. That capability gap is also reported by the
8187 owner. No raw-Git exception is inferred.

Local `0120_experience_memory_projection.sql` remains unchanged and the chain is
contiguous at schema 120. ASMA-8187's unmerged `0120–0122` remain external candidate
numbers, not reservations. The [existing reconciliation plan](2026-10-03-01-11-plan-migration-reconciliation.md)
still applies: serialize accepted external joins, inspect the actual resulting
maximum, reallocate/reconcile unshipped SQL through supported owning operations,
update registration/version/rewind/fixture/export assertions, regenerate contracts,
then rerun fresh/upgrade/rollback and all combined gates. No number is claimed.
The 8187 owner reports at least eight affected expectation/fixture sites; this
requires more than renaming a file.

## Cross-lane map consumed from TPM coordination

These are TPM-record observations, not new independent acceptance or direct
branch inspections. A missing integration designation remains missing. This
writer is the sole ASMA-8155 integration writer and takes no external lane.

| Lane | Existing writer/contact | Integration authority/designation | Carried candidate/status |
| --- | --- | --- | --- |
| ASMA-8187 | `19b7107d-ce1c-46e5-89e4-0d2413a1c50d` | ASMA-8186 TPM; writer holds no shared integration claim | `99b237201e71…`, unaccepted P2 rework; isolated, local, replay capability gap |
| ASMA-8196 | `ba4ded2e-4943-45d3-b267-7eca5ee5440f` | No integration owner designated; ASMA-8190 TPM `74250c1e-7540-440a-9809-25952915fd2a` coordinates | Rework `940138c5578b67c1590587a0d29c10390a58847a`, QA evidence `f77c32501c92aecad1ee10180edbfd3158cd06fb` PASS; AUD pending in consumed record |
| ASMA-8114 | `27bd6bb0-95ba-4bbb-bf74-bc554293ed54` | ASMA-8113 TPM/LSA; seat holds writer role only | `04a8d8c09d291a7445af7d96c4d6b3a776181135`, frozen unaccepted; LSA requalification pending |
| ASMA-8340 | `1da997b5-a5ab-49a7-ab4e-38ac0bbdfe15` | ASMA-7869/8340 orchestration; no separate integrator designated | `d3fa8068` (only abbreviated pin supplied), pushed candidate, unaccepted; no PR |
| ASMA-8341 | `cf1b2891-3a4b-4acb-abc7-a4bfe960afa9` | Same seat explicitly designated integration owner for its own lane | `4a3eb2438fa375063db2ab0399c9afa4cacdf7a6`, unaccepted, security remediation hold; no PR |
| ASMA-8155 / ASMA-8159 | `07d0401f-7018-4afb-9fec-ca1ff092cc49` | This persistent writer; exclusive epic integration branch | Accepted 8156 + 8158 + prior evidence; this evidence-only checkpoint |

Proposed serialized source order remains baseline PR #278/#280 → accepted 8187 →
accepted 8196 → accepted 8114 → committed/accepted 8340 → accepted memory 8156 →
accepted launch 8158. Join accepted 8341 Cargo/bootstrap before final lock/artifact
freeze; join docs by its independent frozen pin. Actual acceptance and the
supported integration operation must be established by the TPMs before any join.
No overlapping merge writer is authorized while ownership/order remain unresolved.

## Unperformed obligations and production prerequisites

F1 remains open. Composed-path fixtures and MUT-007 were **not seeded or run**;
the historical store-only MUT-007 kill is not a composed-path kill. MUT-001–006
were not rerun; their previous results stay pinned to the prior package.

No copied database or synthetic corpus was created in this resumption. Thus there
is no new copied-data provenance, destination, retention period or later-credential
claim to accept. The [55-slot worksheet](historical-55-census.csv) remains pending;
the [census/rehearsal plan](2026-10-03-01-11-plan-corpus-census-and-rehearsal.md)
remains a scaffold, not the requested executable rehearsal. Any later same-Realm
copy must have accepted provenance, an isolated root, explicit retention/teardown
and source aggregate/history-hash preservation before production data is accessed.

Still owed after the boundary disposition: source/fake composition and canary/CAS
activation caller; composed fixtures and full seven-mutant pass; executable
synthetic census/rehearsal; F3 safe full loopback breadth, named contract targets,
contract-crate clippy, generated OpenAPI/console parity, appropriate lanes and
licenses/bans/sources; F4 forced rollback/fresh/upgrade tests. The inherited macOS
Keychain-touching daemon lib/usage suite remains excluded pending an exact safe
isolation; no inherited ignored test is silently promoted to executed evidence.

`yoke-derive 0.8.3` remains externally routed to ASMA-8113 TPM
`0cd11b4f-6623-4925-b1f6-0f7f2b070a2b`. Consume its correction when accepted;
no dependency update or duplicate remediation was attempted here.

Release additionally requires independently accepted combined artifacts after
external joins, final committee/gates, approved credential and outbound/provider
scope, accepted copied-corpus provenance/retention/teardown, backup/rollback/isolation
and readback execution on the reviewed artifact, migration reconciliation, the
8113 correction, real pinned Cognee/provider qualification and 24-case benchmark,
and ordinary gated publication/deployment. None is satisfied by this checkpoint.

Only this report, its read-only reproduction/receipt and the package-index route
are staged. All accepted source, inherited receipts, adapters and other worktrees
remain untouched. The exact new commit SHA is supplied in the final handoff.
