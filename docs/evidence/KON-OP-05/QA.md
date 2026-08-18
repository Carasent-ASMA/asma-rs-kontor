# KON-OP-05 / ASMA-7874 — Wave-3 QA gate

Date: 2026-08-18
Kontor task: `01a0074f-6726-7823-be5a-719cc3d8ecc1`
Team run: `kontor-team-01a01054-a6ec-70a0-a5b5-9d969055a893`
QA successor run: `kontor-run-01a0137f-6f25-7563-8143-be74dd29cabc`
Predecessor tester run: `01a01054-f780-7503-9911-bec7c3fbddd8` (lost/terminal)
Frozen submodule commit: `867c6f97df48470bbe73a9e71f3a099fe2d8f9b1`
Implementation commit: `e338b9d`
Inspector verdict: `passed` in `REVIEW.md` (commit `867c6f9`)

## Typed verdict

`passed`

Checkpoint 1 is ready to advance. The accepted scope is the versioned,
immutable Advisor-profile and Committee-template publication slice. The five
consultation run operations remain intentionally typed `Unavailable` stubs;
this is not a failure for this checkpoint.

## Gate artifacts

| Artifact key | Result | Evidence |
| --- | --- | --- |
| `qa.op05.acceptance.scope.v1` | `passed` | `ARCHITECTURE.md` and `IMPLEMENTATION.md` agree that Checkpoint 1 delivers the two typed definitions, migration, seed, and six read/Admin operations only. |
| `qa.op05.workspace-tests.v1` | `passed` | `cargo test --workspace --locked` exited 0. The run includes the OP-05 core specification tests (22), store profile tests (9), preset tests (5), and 161 daemon loopback API tests. |
| `qa.op05.public-contract.v1` | `passed` | The loopback API coverage passed its preview/apply/read-back, stale-revision, preview-hash, replay, unknown-field, cross-family, empty-catalog, and no-topology-side-effect cases. |
| `qa.op05.persistence.v1` | `passed` | The store profile suite passed first-version, consecutive-version, gap/refusal, per-profile versioning, family separation, canonical byte read-back, deterministic ordering, and project isolation cases. |
| `qa.op05.preset-and-policy.v1` | `passed` | The core and profile tests passed the closed-specification, conjunctive-outcome, provider-diversity, two/five-seat cardinality, and shipped `independent_review@1` fixture checks. |
| `qa.op05.static-quality.v1` | `passed` | `cargo clippy --workspace --all-targets --locked -- -D warnings` exited 0. |
| `qa.op05.format.v1` | `passed` | `cargo fmt --all -- --check` exited 0. |
| `qa.op05.review-handoff.v1` | `passed_with_followups` | The Inspector's authoritative gate is `passed`; its two highest-priority non-blocking findings are preserved below. |

## Acceptance assessment

1. **PASS — closed, typed publication definitions.** The feature test surface
   validates Advisor profiles and Committee templates rather than treating the
   document as arbitrary JSON; malformed fields, invalid role/slot shapes,
   invalid rounds, and invalid provider diversity are refused.
2. **PASS — immutable, versioned persistence.** Publication begins at version
   one, permits only the next version, separates the two catalogs, preserves
   canonical bytes on read-back, and scopes reads to their project.
3. **PASS — pure preview and durable apply.** Preview writes no profile,
   receipt, topology, or seat. Apply is bound to its preview hash and expected
   catalog revision; replays publish only once and preserve the original result.
4. **PASS — route/authority boundary.** The public API tests prove family is
   selected by route, an Advisor profile cannot be published as a Committee
   template, unknown fields are rejected, and publishing creates no workspace
   or seat.
5. **PASS — seeded independent-review policy.** The sole bundled preset parses
   and validates with two reviewers, one Judge, conjunctive aggregation,
   provider-family diversity, and a bounded round limit.
6. **PASS — checkpoint boundary held.** No test or evidence claims that Advisor
   or Committee invocation, findings, settlement, context freezing, or native
   placement is ready. Those remain the declared Checkpoints 2–4 work.

## Environment note

The first workspace-test attempt under the restricted sandbox stopped at
`kontor-cli/tests/memory_parity.rs` because the sandbox denied a loopback bind
(`Operation not permitted`). The identical `cargo test --workspace --locked`
command was rerun with local socket permission and completed successfully. This
is an execution-environment restriction, not a product-test failure.

## Preserved follow-ups (not QA blockers)

The inspector recorded these as non-blocking and they remain open rather than
being represented as fixed:

1. `command_receipts` immutability/no-delete triggers are lost by historic
   table-rebuild migrations. Application-level replay protection remains in
   force. The next command-receipt rebuild must restore and schema-test both
   triggers.
2. A failure after profile publication but before receipt recording leaves a
   durable revision with no receipt; retry currently receives a revision
   conflict. Reconcile the existing canonical row before CP2 composes run
   behavior on that receipt trail.

The inspector additionally noted diversity policy rationale, preview violation
aggregation, unbound `RoleKey` validation, and interrupted-apply/unknown-project
coverage as follow-ups. None invalidates the delivered Checkpoint 1 contract.

## Scope and tree integrity

QA changed no production source, migration, contract, fixture, lockfile, or
other task scope. The only intended QA change is this evidence artifact. The
submodule also contains pre-existing untracked `docs/evidence/KON-MVP-18/run-*`
directories; they were neither inspected as evidence nor modified.
