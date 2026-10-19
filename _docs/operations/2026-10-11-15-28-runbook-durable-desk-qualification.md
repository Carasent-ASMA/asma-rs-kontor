# Qualify and release durable desk support

> **Date:** 2026-10-11 15:28
> **Status:** 🟡 Merged in PR #299 (`813135fd`, 2026-10-11); the hosted desk MCP repair followed in PR #300 (`0971674d`). Merged is not deployed (see the [router](../index.md)), and no record of release steps 3–8 below exists in this repository.
> **Author:** Codex — ASMA-8454 qualification owner
> **Category:** runbook
> **Scope:** ASMA-8454 / ASMA-8449; disposable qualification realms only
> **Summary:** Reproduce the integrated acceptance and mutation evidence, then hand a frozen candidate to the coordinator for Independent Review and the release gateway. Installing support preserves existing selections; selection and rollback are rehearsed only in qualification scope.

---

## When to Load

Load this document when reproducing ASMA-8454 evidence, reviewing its candidate or preparing its released-build acceptance receipt.

Do NOT load as authorization to migrate a real durable seat, change a watchdog, adopt Paseo-direct recovery or select a successor for a live epic.

---

## Ownership and readiness

The qualification owner writes only the ASMA-8454 checkout. Its Rust tests own their temporary realms and scripted native fixtures; `TempDir` removes them after each test. The HTTP/MCP/CLI parity test owns an ephemeral loopback server and aborts it before cleanup. No test configures the user's Paseo service, provider account or Jira connector.

The coordinator owns admission, the authoritative plan, Independent Review and the merge/release gateway. Two reviewers from different vendors and a separate JUDGE must review the frozen candidate. Igor's 2026-10-10 permission covers this epic's build, merge and release batch; it does not authorize a live durable-seat cutover. A passing source test is not a released-build receipt.

Same-vendor LSA→TPM and `keep_native` binding remain refused pending Igor's seat-credential decision. Preserve that fence in review and release reports; do not count refusal as acceptance of either capability.

## Reproduce the source qualification

Run from the isolated qualification checkout, containing all four merged implementations. Keep adapter installs out of commits. The recorder writes logs and JSON receipts under ignored `.qualification/`; it places fixture repositories and the Cargo target inside this checkout and removes inherited `KONTOR_AUTH` from test processes.

```sh
python3 scripts/qualify-durable-desks.py baseline
python3 scripts/qualify-durable-desks.py declarations \
  --declaration-source $HOME/.paseo/worktrees/0vl4ss0m/asma-8419-pr-draft-status-checker/config/orchestration/pr-desk.yml
python3 scripts/qualify-durable-desks.py mutations
```

The first command runs the four daemon acceptance suites, including the integrated four-structure snapshot/restore/replay/rollback rehearsal. The second reads the explicitly named source without writing it and compares its exact hash, titles, roles, skills and dated owner with the fixture's supported seat report. `premium` stays a source chain declaration; the comparison does not resolve it into a model route or launch/adopt either permanent seat.

The third command requires a fresh green baseline for every target suite, seeds one fault at a time and restores its original bytes in `finally`. Each result includes the source line and hash, seeded hash, exact command, test log and verdict. Compilation failures are invalid results. Any survivor or invalid result blocks completion. The final restored baseline must pass, and the restored source bytes and Git status must match the baseline. The two authorized qualification repairs remain in the candidate.

The pinned ASMA-8419 source is an **uncommitted dated decision**, branch `feat/ASMA-8419-pr-draft-status-checker`, coordinator-reported HEAD `c4f9957c`, owner Igor / 2026-10-08. Its exact SHA-256 is `2297aba99eb830c35d9a5f386635162a008056dcdcd1bc1d3c019db33ea9cb05`. The coordinator's 2026-10-11T13:16Z Q-002 disposition accepts these bytes as the comparison source. A changed hash, later commit or PR is a newer revision requiring Igor's explicit decision; this qualification does not follow it automatically.

For offline Swagger builds, the recorder uses `.qualification/swagger-ui.zip` when present. Record the archive's hash and origin; do not change a dependency or lockfile to bypass the build. `RUSTC_WRAPPER` is cleared because this sandbox cannot use the host sccache daemon. `GIT_CEILING_DIRECTORIES` prevents disposable repositories inside the worktree from discovering its parent Git repository.

Also run the HTTP/MCP/CLI parity test and the recorded Paseo adapter contracts with the same fixture/build environment:

```sh
cargo test --locked -p kontor-cli --test durable_desk_parity -- --nocapture
cargo test --locked -p kontor-runtime-paseo --test contract
```

These prove the real router/client operation and refusal, and recorded Paseo protocol behavior. They do not prove live lost-reply timing.

Run and record every gate from the [local verification policy](../architecture/2026-09-01-13-25-architecture-kontor-local-verification-policy.md), including archive verification against the candidate commit. Retain an exact reason and coordinator disposition for any blocked gate. No GitHub-CI result substitutes for these local checks.

## What the integrated rehearsal compares

The test starts a legacy epic under its original immutable Team Definition before publishing any desk vocabulary. It publishes and explicitly selects a combined qualification definition and its exact topology hash through Admin `/v1` operations. It then materializes both desk projects/workspaces, declares the two Paseo-direct permanent seats without runtime effects, launches an unrelated daemon-owned fixture seat, places two epic Adams and an ad-hoc Adam, and opens two distinct PR subjects for one confirmed synthetic ASMA key.

The snapshot is created through the existing backup implementation. After stopping the fixture daemon, the test drains its retained in-process SQLite connection's WAL, restores through the supported recovery function under the exclusive state-root lock and compares the full redacted export records hash before startup. Startup reconciliation must open the scheduling barrier. Replay must reuse the recorded bindings without another native launch.

Selection rollback explicitly chooses desk-less v1. Fresh-key re-drives of both existing desk keys must keep their own pins, native ids/kinds/titles/cwd and seat/subject records. Reselecting the qualification successor must preserve the same census. Closing PR 101 must leave PR 102, both desks, the permanent-seat declarations and the old epic pin intact. This exercises selection rollback, not live migration or binary downgrade.

The machine evidence includes the snapshot manifest, export hash, old pin and canonical hash, qualification definition/topology documents and hashes, supported PR/seat reports and the normalized logical/native census. Readback timestamps are observations, not native identity; the census compares stable binding ids, native identities, kinds, titles, cwd and parentage.

## Supported operator operations

Every command must name an explicitly allocated disposable realm and its loopback endpoint. Do not rely on default roots or inherited seat credentials. Read the project's current revision before a new write. Reuse the exact request body and key on replay; a changed intent needs a new key.

| Purpose | Supported operation | Required evidence |
| --- | --- | --- |
| Preview/apply selection | `team-definition-selection:preview` / `:apply` | Old selection, target id/version/hash, preview hash, expected revision, unchanged existing pins |
| Ensure/materialize/read desk | `topology:ensure`, `topology:materialize`, `topology:drift`, `topology:inspect` | `target: {"scope":"desk","desk_key":"adam"}` or `pr-review`; exact native ids/kinds/titles/cwd/parents |
| Record permanent seat | `desks/{key}/seats:declare` | Exact dated title/role/procedure; `recovery_owner: paseo_direct`; no native binding or launch |
| Observe/reconcile | `desks/{key}/seats`, `seats:reconcile` | Only selected daemon-owned fixture keys; explicit `restart_missing`; no effect for busy/unchecked/archived/duplicate |
| Open ad-hoc Adam | `desk-adams/ad-hoc:open` | Topic, declared leadership, optional exact route; no forged epic identity |
| Open/read/close PR | `desks/{key}/pr-reviews:open`, `pr-reviews`, `pr-reviews:close` | Authorized repository/number and confirmed ASMA publication binding; head and exact parent/cwd readback; sibling isolation |

CLI names derive from the MCP registry. For example, using an explicitly supplied qualification client, root and endpoint:

```sh
"$QUAL_KONTOR" --state-root "$QUAL_STATE_ROOT" --base-url "$QUAL_BASE_URL" --tier observer \
  topology-inspect --project-id "$QUAL_PROJECT_ID"
"$QUAL_KONTOR" --state-root "$QUAL_STATE_ROOT" --base-url "$QUAL_BASE_URL" --tier operator \
  topology-materialize --project-id "$QUAL_PROJECT_ID" \
  --target '{"scope":"desk","desk_key":"adam"}' \
  --expected-revision "$QUAL_PROJECT_REVISION" --idempotency-key qual-materialize-adam
"$QUAL_KONTOR" --state-root "$QUAL_STATE_ROOT" --base-url "$QUAL_BASE_URL" --tier observer \
  desk-seats-list --project-id "$QUAL_PROJECT_ID" --desk-key pr-review
"$QUAL_KONTOR" --state-root "$QUAL_STATE_ROOT" --base-url "$QUAL_BASE_URL" --tier observer \
  desk-pr-reviews-list --project-id "$QUAL_PROJECT_ID" --desk-key pr-review
```

The detailed request contracts and refusal rules remain in the shipped references: [desks](../reference/2026-10-10-23-55-reference-kontord-durable-desks.md), [seats](../reference/2026-10-11-02-20-reference-kontord-durable-desk-seats.md), [Adams](../reference/2026-10-11-05-36-reference-kontord-desk-adams.md) and [PR reviews](../reference/2026-10-11-11-35-reference-kontord-pr-reviews.md). They include opt-in definition construction; installing the binary does not select those policies. A topology/definition version number alone is insufficient: verify its canonical hash. Refuse lineage-address collisions rather than replacing an immutable revision.

The CLI preserves the daemon refusal body. Exit codes tell the operator what to do next:

| Codes on these operator paths | Exit | Corrective action |
| --- | --- | --- |
| `placement_blocked`, `ensure_mismatch`, `handoff_unsettled` | 3 | Resolve the declared placement or handoff decision before retrying |
| `stale_binding`, `idempotency_conflict`, `revision_conflict`, `delivery_unconfirmed` | 4 | Read the binding/revision/timeline; preserve the original key after an unconfirmed delivery |
| `proof_scan_incomplete`, `provider_usage_throttled`, existing unavailable codes | 5 | Obtain a complete proof or wait for dependency capacity |
| `unsupported_capability`, `rename_pending`, existing absent codes | 6 | Use a supported operation or finish renaming before addressing the requested target |

The additive map leaves all previously mapped codes in their existing classes. `placement_blocked` previously fell through to protocol exit 1; the qualification repair pins exit 3 in the HTTP/MCP/CLI parity fixture. A generic refusal action can still say “nothing changed” after partial PR close; use subject readback and convergent retry as described below.

PR close is non-atomic. A lost/failed retirement answer may follow successful earlier retirements. Inspect the subject and retry the **same** close intent/key to converge; do not infer that a refusal means no session changed. A title collision between different owners with the same displayed repository name/PR number is a documented refusal. The unproved-container close branch is source-verified only in this qualification.

## Coordinator release and exact readback

1. Freeze the candidate commit, committed evidence manifest and verification receipts. Obtain the required independent reviewer/JUDGE decision against that exact candidate; unresolved non-equivalent mutants or a missing decision are not release acceptance.
2. Route merge through the supported ASMA/product gateway with its judged authorization. Record the merge commit and verify that the shipped production/test inputs match the qualified candidate. A different implementation requires requalification.
3. Build the merged source with the pinned lockfiles, recording command, toolchain, source hash and SHA-256 for **each actual executable**. A proposed build command is `cargo build --locked --release -p kontor-daemon -p kontor-cli`. Package/version output alone cannot identify a source revision. The coordinator selects and records the existing supported installation/release mechanism; this runbook does not invent one.
4. Install capability support while retaining old selections. No real durable seat, recovery owner, epic pin, credential, watchdog or fleet policy is changed. Record release artifact/path/digest and actual installed-executable digest equality.
5. Allocate a separate disposable released-build realm and isolated native fixture, with explicit project/workspace ids, cwd and cleanup owner before effects. Start the **released executable**, record its digest, and use the supported client at its explicit loopback endpoint. Publish the qualification successor against exact topology/definition hashes and select it only there. Never copy temporary native ids or credentials from an earlier realm as authority.
6. Capture supported before/after topology, seat and PR reports, immutable old-pin hashes, native identity/title/kind/cwd/parentage and replay/rollback results. Rehearse snapshot/export, stop, restore and startup reconciliation as described in [RECOVERY.md](../../RECOVERY.md). Use the realm's own current schema/export generations; this batch adds no migration/export generation. Repeat both permanent-seat zero-effect checks and sibling closeout isolation with the released executable.
7. Preserve machine-readable commands, outputs, timestamps, executable and definition hashes, supported readbacks and the cleanup receipt. Remove only the named qualification fixtures; record who verified cleanup and when. Source fixture evidence cannot replace this released-process receipt.
8. Only then may the coordinator resolve the Reports row and task/epic closure. Record the pending Igor seat-credential decision separately and honestly; it is not same-vendor acceptance.

If selection is rolled back, retain existing desks and their pins; only future work follows the previous default. A binary downgrade to a build that cannot read schema 134 requires restoring its compatible pre-upgrade snapshot while stopped, not opening the newer database with an older binary. Preserve the outgoing database and compare pins/native identities before any restart. No binary downgrade or live cutover is authorized by this rehearsal.
