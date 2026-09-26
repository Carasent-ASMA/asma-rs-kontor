# ASMA-8121 export event-kind operational gap

> **Date:** 2026-09-26 08:28 Europe/Oslo
> **Status:** 🟢 Export gap resolved — qualified merge deployed and actual snapshot export/import continuity verified; ASMA-8049 census and workflow closeout remain pending
> **Author:** Codex
> **Category:** report
> **Scope:** ASMA-8049 / ASMA-8121 snapshot, export and identity continuity
> **Summary:** The initial installed export incorrectly applied the observation vocabulary to command intents. The corrected exporter proves immutable receipt authority, preserves observation redaction, and now exports the unchanged snapshot successfully. All 23,711 source hashes survive isolated foreign import without executable destination authority.

## When to Load

Load this report when qualifying ASMA-8121 export continuity, reviewing the event-kind correction, or deciding whether ASMA-8049 has its actual backup/export and restart evidence.

## Observed failure and ownership

Igor authorized Paseo-direct completion of Kontor and orchestration work, with direct SQLite closure reconciliation only after actual delivery proof. The failure was discovered during that qualification, rather than by changing live data to satisfy a closure projection.

At the initial observation, installed binaries matched the deployment receipt labelled `7ffd650d9091410887a3de6a360296be9d9a88fd`. The snapshot command succeeded at 2026-09-26T06:06:24Z. Supported offline restore into a disposable state root also succeeded. Export from an offline copy failed at 2026-09-26T06:08:01Z, exit 1, with:

> invalid ControlObservation at `name`: the durable control-plane log stores only known control-metadata fields

The owning task is ASMA-8121, under ASMA-8049. The final deployed-qualification section below supplies the actual correction, deployment and export continuity proof. No epic, task gate or Kontor trust PASS is claimed.

## Root cause and correction boundary

`runtime_events` has three event kinds. Runtime and census observations carry runtime-owned control metadata. Command intents carry application-owned canonical documents, including names, paths and graph arrays. The local and dispatch command writers store the same canonical intent in the command receipt and event log.

`backup::export::export_realm` incorrectly passed every kind through `ensure_control_metadata`, whose positive vocabulary deliberately excludes those command fields. The first stored project command exposes the error. Expanding the observation vocabulary would weaken the runtime-content boundary.

The isolated correction matches the event kind: both observation kinds retain the positive metadata check; command intents retain their canonical document and receive the existing recursive embedded-document canary scan, alongside their identical command-receipt intent. Unknown event kinds refuse. No export schema, field, identifier, digest or record is omitted to make the export succeed.

Source starts from merged `4c1c52cb87b87b44b77f2eee03bfa0fbb50b7a59`, retaining the independently qualified ASMA-8196 launch/attachment recovery. During source qualification, primary source, installed binaries and live SQLite remained unchanged. The later controlled deployment is recorded separately below.

## Snapshot and restore evidence

The [identity receipt](evidence/ASMA-8121/snapshot-restore-identity-receipt.json) records `quick_check=ok` and zero foreign-key violations for both disposable copies. Counts and SHA-256 digests match for all rows of Jira links, canonical task links, Team Definition pins, migration receipts, topology nodes, seat bindings, hosted native seats, runtime bindings, command receipts and completion state.

This is same-Realm snapshot/restore evidence. Redacted foreign import deliberately preserves source lineage without materializing source runtime authority; it is not an active-native restoration proof.

The [failed export log](evidence/ASMA-8121/baseline-export.txt), [snapshot log](evidence/ASMA-8121/deployed-snapshot.txt) and [restore log](evidence/ASMA-8121/offline-restore.txt) preserve their actual outcomes.

## Regression and mutation evidence

The new regression records a real `EnsureProject` local command with the observed name/path intent shape, then requires exact command payload/hash retention, both events, deterministic record bytes and parse round-trip. Against merged source it compiles and fails on `ControlObservation.name`, exit 101. The [actual red log](evidence/ASMA-8121/export-command-regression-red.txt) preserves that assertion.

Additional regressions require both observation kinds to refuse a planted runtime-content alias and require a planted command-intent secret to be refused by the recursive scan without echoing its value. The first suite run had one fixture collision with the one-event-per-receipt index; its [actual failed log](evidence/ASMA-8121/initial-backup-export-fixture-failure.txt) is retained and supplies no acceptance credit. The disposable corruption fixture now explicitly bypasses only its own append-only update trigger to plant the refused payload; production triggers remain unchanged.

The corrected focused contracts pass. Three distinct source mutants compiled and failed the intended behavioral assertions, exit 101: rejecting valid commands, accepting runtime/census session content, and skipping the embedded canary scan. Exact candidate-relative patches, log hashes and post-restoration source/test hashes are in the [mutation receipt](evidence/ASMA-8121/export-mutation-results.json). After restoration, the [complete backup/export suite](evidence/ASMA-8121/backup-export-green-restored.txt) passes all 21 tests, zero failed, exit 0. Tests use the owned worktree and disposable SQLite; the compiler target cache is shared at `_tools/asma-rs-kontor/target`. The b42 candidate’s full gate passed; it does not qualify the later authority correction. Actual snapshot export and deployed continuity remain pending.

Separately, ASMA-8049's MUT-003 was actually executed on installed-source bytes: removing confirmation-time live-census parity compiled and failed `confirmation_re_proves_parity_against_the_live_census`, exit 101. Baseline and restored runs passed. The [receipt](evidence/ASMA-8121/jira-migration-mut003-results.json) and [baseline](evidence/ASMA-8121/jira-migration-mut003-baseline.txt), [mutant](evidence/ASMA-8121/jira-migration-mut003.txt), [restored](evidence/ASMA-8121/jira-migration-mut003-restored.txt) logs preserve that proof. It supplies only this mutant's qualification.

## Exact-candidate source qualification

Candidate `b42b51970a254a98db2c0997f676e413b5cd7377` passed the required archive gate: formatting, workspace Clippy with warnings denied, 2,901 Rust tests across 149 test binaries (zero failed, nine ignored), dependency audit/deny, frozen console install, typecheck, 305 console tests and production dependency audit. The separate API-generation comparison also passed. Both commands exited 0. The [qualification receipt](evidence/ASMA-8121/b42b5197-source-qualification.json), [full raw log](evidence/ASMA-8121/b42b5197-full-gate.txt) and [API log](evidence/ASMA-8121/b42b5197-verify-api.txt) preserve exact identity and results.

## Independent audit and receipt-authority correction

The [actual independent b42 audit](evidence/ASMA-8121/authority-correction/actual-lsa-b42-audit.txt) found a P1: a command-kind label did not establish export authority. The original b42 full gate remains genuine for that revision, but does not qualify this later correction.

Export now looks up the exact exported immutable command receipt, matches project, canonical payload bytes and stored hash, and recomputes the payload hash before accepting a command event. Missing receipts, different projects, unrelated payloads and matching-but-wrong stored hashes refuse without echoing content. Observation allowlisting and recursive canary scanning remain intact. Production schema and triggers are unchanged; only disposable corruption fixtures bypass their own guards.

Three corruption regressions [failed against c08](evidence/ASMA-8121/authority-correction/authority-regression-red.txt). The [corrected focused run](evidence/ASMA-8121/authority-correction/authority-focused-green.txt) passed six tests, and the [restored complete export suite](evidence/ASMA-8121/authority-correction/backup-export-restored.txt) passed 25 with zero failures. Strict [store Clippy](evidence/ASMA-8121/authority-correction/store-clippy.txt) exited 0. Six compiled behavioral mutants were killed with exit 101; their exact patches, logs and restored source/test hashes are in the [corrective receipt](evidence/ASMA-8121/authority-correction/results.json).

The new foreign-realm case binds exact source event payload/hash through the exported row digest and preserves source realm, record identity and non-executable import lineage. Imported commands/events do not become destination runtime authority or re-exported local commands. No claim is made that foreign import stores the raw source payload.

The PR title now follows the required key-plus-space grammar; its new publication check succeeded. The corrected candidate received its own complete qualification and independent corrective audit; actual integration and deployment remain separate.

## Remaining acceptance

The source checkpoints below preserve their original acceptance frontier. The deployed-qualification section records later completed integration, build, deployment and export steps. Still required for ASMA-8049: a complete all-thirteen joined post-restart census, genuine ASMA-8120/8121 verification/audit evidence, and actual workflow settlement. Preserve all historical failures and the 89-versus-88 temporal lifecycle distinction. [SHA256SUMS.json](evidence/ASMA-8121/SHA256SUMS.json) anchors the raw receipts and logs.

The [actual corrective LSA audit](evidence/ASMA-8121/authority-correction/actual-lsa-79-audit.txt)
confirms the prior P1 authority and P2 foreign-round-trip gaps are resolved at
`79b07f31028704a6a98203b89877264fd179a535`. It found no P0/P1 and conditionally
supports merge after that candidate's own full gate exits 0. Its wording P2 is
addressed above by explicitly naming b42; the historical finding is unchanged.
Deployment and actual snapshot/export/restart qualification remain uncredited.

## Corrected exact-source qualification

Candidate `79b07f31028704a6a98203b89877264fd179a535`, tree `ebd20eb03d5659b30747bc2202e2b2ee0896528d`, passed its own archive gate with actual exit 0: formatting, strict workspace Clippy, 2,905 Rust tests across 149 binaries, zero failed and 9 ignored, dependency audit/deny, frozen installation, typecheck, 305 console tests and production audit. Separate API-generation verification exited 0. The [exact qualification receipt](evidence/ASMA-8121/79b07f31-source-qualification.json), [raw archive log](evidence/ASMA-8121/79b07f31-full-gate.txt) and [API log](evidence/ASMA-8121/79b07f31-verify-api.txt) bind these outcomes to the source candidate. These results satisfy the corrective LSA's full-gate condition; its historical finding remains unchanged. ASMA-8098's verified Jira/SQLite Done closeout releases its installed-7ffd hold for qualified repairs, but supplies no test credit to this source. Actual snapshot export, merge/build/deployment and fresh joined post-restart census remain pending.

## Actual merged and deployed qualification

The [final independent source finding](evidence/ASMA-8121/authority-correction/actual-lsa-source-merge-support.txt) reports no P0/P1/P2 and verifies every prior conditional merge requirement. PR #274 actually merged at `dddb72870c6d58d5d6383a69227844dbc8dca45e` at 08:00:58Z. The [merge identity receipt](evidence/ASMA-8121/deployed-dddb7287/merge-source-identity-receipt.json) verifies an empty implementation/test/dependency diff from qualified `79b07f31`. The isolated merged archive [built successfully](evidence/ASMA-8121/deployed-dddb7287/release-build.txt); its [build receipt](evidence/ASMA-8121/deployed-dddb7287/build-receipt.json) binds the daemon, MCP and CLI hashes to that archive.

The [reviewable deployment plan](evidence/ASMA-8121/deployed-dddb7287/deployment-plan.json) and [applied script](evidence/ASMA-8121/deployed-dddb7287/deployment-apply.py) stop only the configured Kontor service, preserve database/binary backups, install the exact built bytes and provide rollback on failure. The [actual deployment receipt](evidence/ASMA-8121/deployed-dddb7287/deployment-receipt.json) records successful controlled restart at 08:17:15Z, schema 119, `quick_check=ok`, zero FK violations, exact Realm HTTP 200, unchanged runtime/fleet configurations and preserved ASMA-8098 Done revision 14. It records zero manual SQLite writes. Installed daemon SHA-256 is `2249946d70d31bbd021b1eedb17daac9601cde09109ad39dcfe501d8eb1fa420`; the MCP bytes remain identical because its implementation did not change.

Five [fresh read-only live tests](evidence/ASMA-8121/deployed-dddb7287/postdeployment-read-only-live.txt) pass against Paseo 0.9.1 after deployment, with the disposable mutating workspace case explicitly filtered and exit 0. These are new merged-source results. The earlier ASMA-8098 qualification supplies no deployment credit.

## Actual unchanged-snapshot export and foreign continuity

The preserved original snapshot remains SHA-256 `5689cc0a1257bff94861d0a61bc5c17c3831d40419a26323f7ffc5c73ec928d7`. The merged [staging export](evidence/ASMA-8121/deployed-dddb7287/source-export.txt) and later [installed-binary export](evidence/ASMA-8121/deployed-dddb7287/installed-binary-source-export.txt) both exit 0 on its offline copy. Opening that copy changed exactly six SQLite header bytes; its size and logical schema/content SHA3 remain identical to the original. Byte identity applies to the preserved original; export continuity applies to unchanged logical snapshot content. The [installed export receipt](evidence/ASMA-8121/deployed-dddb7287/installed-export-receipt.json) compares every exported record body and the aggregate digest: 23,711 records in 97 collections, records hash `bc4178ca20b785db89acdc302cc82f8a2953eebe7605e65fbe5557b9a28d92b5`, same source Realm. The [initial export receipt](evidence/ASMA-8121/deployed-dddb7287/historical-initial-export-receipt.json) is retained unchanged; its null count/Realm fields were an extraction error, corrected explicitly in the later receipt.

Only a disposable destination was created. Its [operator-tier refusal](evidence/ASMA-8121/deployed-dddb7287/destination-project-ensure-operator-refusal.txt), subsequent [authorized project creation](evidence/ASMA-8121/deployed-dddb7287/destination-project-ensure.txt), [exact destination shutdown](evidence/ASMA-8121/deployed-dddb7287/destination-stopped.json) and [supported offline import](evidence/ASMA-8121/deployed-dddb7287/destination-import.txt) retain their actual outcomes. Production state and native placement were not involved in import.

The [foreign continuity receipt](evidence/ASMA-8121/deployed-dddb7287/foreign-continuity-receipt.json) independently recomputes all 23,711 source row hashes using canonical compact sorted UTF-8 JSON plus LF, matches every imported lineage, verifies exact primary-key identities for eleven critical collections, and verifies 3,417 command events against immutable receipt payload/project/hash and independent hash recomputation. Fourteen configuration records materialize; 23,697 records remain lineage only. Source commands/events never become destination executable authority. Destination re-export retains the lineage but exports only 21 destination-local records, including its own explicit project-creation command/event. This foreign continuity proof does not claim restoration of active source native authority; same-Realm snapshot/offline restore supplies the separate identity-preservation proof.

The deployed export gap is resolved. ASMA-8049 remains open: the fresh naming preview passes twelve migrated epics without pending changes, while ASMA-8190 refuses ambiguous delivery replacement-chain leaves. That current census obstacle and actual task/epic verification, audit and settlement require their own proof. Neither this report nor the deployed repair establishes global Kontor trust.

The [actual independent deployed-export audit](evidence/ASMA-8121/deployed-dddb7287/actual-lsa-deployed-export.txt) and [publication receipt](evidence/ASMA-8121/deployed-dddb7287/lsa-deployed-export-publication-receipt.json) support this bounded checkpoint with no P0/P1/P2. The auditor independently reproduces all 63 reviewed hashes, 23,711 lineage hashes, eleven identity sets, 3,417 command/receipt authority checks, installed hashes and destination non-executable lineage. Its snapshot-header precision note is incorporated above; its outstanding bridge-selection and all-thirteen/workflow boundaries remain explicit.
