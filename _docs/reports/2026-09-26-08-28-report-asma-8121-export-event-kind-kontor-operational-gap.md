# ASMA-8121 export event-kind operational gap

> **Date:** 2026-09-26 08:28 Europe/Oslo
> **Status:** 🟡 In Review — receipt-authority correction verified locally; fresh full qualification, corrective audit, continuity and deployment pending
> **Author:** Codex
> **Category:** report
> **Scope:** ASMA-8049 / ASMA-8121 snapshot, export and identity continuity
> **Summary:** The deployed export command applies the runtime-observation field vocabulary to application command intents. A legitimate project command containing `name` aborts export. Snapshot and offline restore preserve the audited identities; export remains unqualified.

## When to Load

Load this report when qualifying ASMA-8121 export continuity, reviewing the event-kind correction, or deciding whether ASMA-8049 has its actual backup/export and restart evidence.

## Observed failure and ownership

Igor authorized Paseo-direct completion of Kontor and orchestration work, with direct SQLite closure reconciliation only after actual delivery proof. The failure was discovered during that qualification, rather than by changing live data to satisfy a closure projection.

Installed binaries match the deployment receipt labelled `7ffd650d9091410887a3de6a360296be9d9a88fd`. The snapshot command succeeded at 2026-09-26T06:06:24Z. Supported offline restore into a disposable state root also succeeded. Export from an offline copy failed at 2026-09-26T06:08:01Z, exit 1, with:

> invalid ControlObservation at `name`: the durable control-plane log stores only known control-metadata fields

The owning task is ASMA-8121, under ASMA-8049. This gap remains open until the correction is qualified, deployed and exercised through actual supported export and continuity readback. No epic, task gate or Kontor trust PASS is claimed.

## Root cause and correction boundary

`runtime_events` has three event kinds. Runtime and census observations carry runtime-owned control metadata. Command intents carry application-owned canonical documents, including names, paths and graph arrays. The local and dispatch command writers store the same canonical intent in the command receipt and event log.

`backup::export::export_realm` incorrectly passed every kind through `ensure_control_metadata`, whose positive vocabulary deliberately excludes those command fields. The first stored project command exposes the error. Expanding the observation vocabulary would weaken the runtime-content boundary.

The isolated correction matches the event kind: both observation kinds retain the positive metadata check; command intents retain their canonical document and receive the existing recursive embedded-document canary scan, alongside their identical command-receipt intent. Unknown event kinds refuse. No export schema, field, identifier, digest or record is omitted to make the export succeed.

Source starts from merged `4c1c52cb87b87b44b77f2eee03bfa0fbb50b7a59`, retaining the independently qualified ASMA-8196 launch/attachment recovery. Primary source, installed binaries and live SQLite remain unchanged.

## Snapshot and restore evidence

The [identity receipt](evidence/ASMA-8121/snapshot-restore-identity-receipt.json) records `quick_check=ok` and zero foreign-key violations for both disposable copies. Counts and SHA-256 digests match for all rows of Jira links, canonical task links, Team Definition pins, migration receipts, topology nodes, seat bindings, hosted native seats, runtime bindings, command receipts and completion state.

This is same-Realm snapshot/restore evidence. Redacted foreign import deliberately preserves source lineage without materializing source runtime authority; it is not an active-native restoration proof.

The [failed export log](evidence/ASMA-8121/baseline-export.txt), [snapshot log](evidence/ASMA-8121/deployed-snapshot.txt) and [restore log](evidence/ASMA-8121/offline-restore.txt) preserve their actual outcomes.

## Regression and mutation evidence

The new regression records a real `EnsureProject` local command with the observed name/path intent shape, then requires exact command payload/hash retention, both events, deterministic record bytes and parse round-trip. Against merged source it compiles and fails on `ControlObservation.name`, exit 101. The [actual red log](evidence/ASMA-8121/export-command-regression-red.txt) preserves that assertion.

Additional regressions require both observation kinds to refuse a planted runtime-content alias and require a planted command-intent secret to be refused by the recursive scan without echoing its value. The first suite run had one fixture collision with the one-event-per-receipt index; its [actual failed log](evidence/ASMA-8121/initial-backup-export-fixture-failure.txt) is retained and supplies no acceptance credit. The disposable corruption fixture now explicitly bypasses only its own append-only update trigger to plant the refused payload; production triggers remain unchanged.

The corrected focused contracts pass. Three distinct source mutants compiled and failed the intended behavioral assertions, exit 101: rejecting valid commands, accepting runtime/census session content, and skipping the embedded canary scan. Exact candidate-relative patches, log hashes and post-restoration source/test hashes are in the [mutation receipt](evidence/ASMA-8121/export-mutation-results.json). After restoration, the [complete backup/export suite](evidence/ASMA-8121/backup-export-green-restored.txt) passes all 21 tests, zero failed, exit 0. Tests use the owned worktree and disposable SQLite; the compiler target cache is shared at `_tools/asma-rs-kontor/target`. The exact-candidate full gate now passes; actual snapshot export and deployed continuity remain pending.

Separately, ASMA-8049's MUT-003 was actually executed on installed-source bytes: removing confirmation-time live-census parity compiled and failed `confirmation_re_proves_parity_against_the_live_census`, exit 101. Baseline and restored runs passed. The [receipt](evidence/ASMA-8121/jira-migration-mut003-results.json) and [baseline](evidence/ASMA-8121/jira-migration-mut003-baseline.txt), [mutant](evidence/ASMA-8121/jira-migration-mut003.txt), [restored](evidence/ASMA-8121/jira-migration-mut003-restored.txt) logs preserve that proof. It supplies only this mutant's qualification.

## Exact-candidate source qualification

Candidate `b42b51970a254a98db2c0997f676e413b5cd7377` passed the required archive gate: formatting, workspace Clippy with warnings denied, 2,901 Rust tests across 149 test binaries (zero failed, nine ignored), dependency audit/deny, frozen console install, typecheck, 305 console tests and production dependency audit. The separate API-generation comparison also passed. Both commands exited 0. The [qualification receipt](evidence/ASMA-8121/b42b5197-source-qualification.json), [full raw log](evidence/ASMA-8121/b42b5197-full-gate.txt) and [API log](evidence/ASMA-8121/b42b5197-verify-api.txt) preserve exact identity and results.

## Independent audit and receipt-authority correction

The [actual independent b42 audit](evidence/ASMA-8121/authority-correction/actual-lsa-b42-audit.txt) found a P1: a command-kind label did not establish export authority. The original b42 full gate remains genuine for that revision, but does not qualify this later correction.

Export now looks up the exact exported immutable command receipt, matches project, canonical payload bytes and stored hash, and recomputes the payload hash before accepting a command event. Missing receipts, different projects, unrelated payloads and matching-but-wrong stored hashes refuse without echoing content. Observation allowlisting and recursive canary scanning remain intact. Production schema and triggers are unchanged; only disposable corruption fixtures bypass their own guards.

Three corruption regressions [failed against c08](evidence/ASMA-8121/authority-correction/authority-regression-red.txt). The [corrected focused run](evidence/ASMA-8121/authority-correction/authority-focused-green.txt) passed six tests, and the [restored complete export suite](evidence/ASMA-8121/authority-correction/backup-export-restored.txt) passed 25 with zero failures. Strict [store Clippy](evidence/ASMA-8121/authority-correction/store-clippy.txt) exited 0. Six compiled behavioral mutants were killed with exit 101; their exact patches, logs and restored source/test hashes are in the [corrective receipt](evidence/ASMA-8121/authority-correction/results.json).

The new foreign-realm case binds exact source event payload/hash through the exported row digest and preserves source realm, record identity and non-executable import lineage. Imported commands/events do not become destination runtime authority or re-exported local commands. No claim is made that foreign import stores the raw source payload.

The PR title now follows the required key-plus-space grammar; its new publication check succeeded. Fresh qualification of the corrected immutable candidate and independent corrective review remain required.

## Remaining acceptance

Complete independent review and actual export on the preserved offline snapshot; then integrate, build and deploy the qualified candidate. Repeat actual export on the unchanged snapshot and offline restore and compare the preserved record identities and digests. Obtain a fresh all-thirteen joined post-restart census, current runtime identity/health and genuine ASMA-8120/8121 verification/audit evidence. Preserve all historical failures and the 89-versus-88 temporal lifecycle distinction. [SHA256SUMS.json](evidence/ASMA-8121/SHA256SUMS.json) anchors every raw receipt and log in this checkpoint.
