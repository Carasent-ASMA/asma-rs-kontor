# ASMA-8454 hosted desk MCP composition repair candidate

> **Date:** 2026-10-11 21:25 +0200
> **Status:** Historical carrier — AUD B1 rejected at 255d8c93; newer rework and released re-proof pending
> **Author:** Codex — ASMA-8454 qualification owner and authorized repair implementer
> **Category:** report
> **Scope:** ASMA-8449 TASK-005; verified hosted-directory composition; Paseo-direct
> **Summary:** Repairs enabled Claude leadership MCP composition in a proved non-Git desk directory. This dated report records the source, filesystem publication/recovery tests and whole-change mutations supplied to QA, AUD and the completion committee; it does not resolve released acceptance.

See the [new rework record](2026-10-11-22-03-report-asma-8454-desk-mcp-rework.md) for AUD B1, healthy-native narrowing and the newer source. This record retains the original source and test history; its credential wording is corrected below.

## When to Load

Load when reviewing the hosted-directory repair, reproducing its failure-injection results, or preparing the coordinator's new released-build rehearsal. The original frozen qualification report and operator runbook are separate, unchanged records.

## Authority and frozen source

The coordinator accepted the desk-seat MCP advisor verdict on 2026-10-11 and explicitly authorized this bounded repair without a new Igor scope decision. The exact [verdict](../../docs/evidence/ASMA-8454/mcp-repair/authority/advisor-verdict.md) and its digest are retained in this bundle. The advisor owns the recommendation; the coordinator owns scope closure, review routing, merge and the release gateway.

Base: `813135fd8b46deb1893b6181a04d42f4785752ca`. Authorized branch: `fix/ASMA-8454-desk-seat-mcp-composition`, created in the ASMA-8454 worktree through `asma git checkout -b --from ASMA-8454 --type fix --title desk-seat-mcp-composition --manual`. The dry-run confirmed the owned module and starting HEAD. Manual branch naming reused the already-confirmed Jira key; no Jira operation was performed.

Qualified source checkpoint: `4c7c2a1110259dddc802c0ee647129f156f2ac41` (tree `94ebdb12a0442080a7344c1572be9f8a3cc5a962`). The evidence/report carrier preserves these seven runtime/test input hashes. Its exact final head is read back and supplied in the handoff, avoiding a report self-hash cycle. [Source manifest](../../docs/evidence/ASMA-8454/mcp-repair/source-manifest.json), [gate receipts](../../docs/evidence/ASMA-8454/mcp-repair/gates.json) and [bundle manifest](../../docs/evidence/ASMA-8454/mcp-repair/manifest.json) pin the bytes.

| File | SHA-256 |
| --- | --- |
| `Cargo.lock` | `0eeee22c33074018b56239506d48d03acb329b115af508dc987044c350249a02` |
| `crates/kontor-runtime-paseo/Cargo.toml` | `0dc6bbe90312d2fa95804daea84d060521bcdaaa90d502a06bf66b2d05bf941d` |
| `crates/kontor-runtime-paseo/src/adapter.rs` | `a6712e84a6fb895fa06b18bb66f2067d0fbf274b256edd18f5d943441127b264` |
| `crates/kontor-runtime-paseo/src/seat_mcp.rs` | `c3300ec16a47eceb7079817878c6b98bf51d2946c118afbf2845a392afdc5fa2` |
| `crates/kontor-runtime-paseo/src/seat_mcp_directory_tests.rs` | `d602d6320e6a9ab82a02bd9dfa893c15add5d0944a943e06d410a49a84bb08a6` |
| `crates/kontor-runtime-paseo/tests/contract.rs` | `bc213f1745ae6edd8500d4f943a89a5d176fea76ab79bdf96b6ad664fbc018bd` |
| `crates/kontor-runtime-paseo/tests/contract/hosted_directory.rs` | `223495d119905948426a56cdc8792fdfa61085c8c0fec185b42951a163779872` |

No real durable seat, credential, recovery owner, epic pin, watchdog, fleet policy, shared checkout, plan or Jira was changed. All repair fixtures are synthetic and owned by Codex in this worktree. This repair uses no schema migration or export-generation change. The original held discovery realms and packages remain preserved; this phase does not repin, migrate or clean their rows.

## Repair behavior and boundaries

At `adapter.rs:6099`, the existing container correlation, generation, root and hosting-workspace checks run first. Only `(HostedSeatScope::Desk, Directory | LocalCheckout)` selects `HostedCompositionMode::OwnedDirectory` (`adapter.rs:6101`). Eligibility is derived from those proved types, not a title or arbitrary cwd. `adapter.rs:6175` calls composition for both a new native and an exact recovered directory native before create or the existing reload boundary. Capacity, placement, route, labels, scope and secret-channel checks remain in place.

The explicit mode and dispatch are at `seat_mcp.rs:343` / `:348`. The directory path canonicalizes cwd and holds one shared lock across adapter instances and subjects (`:426`, `:839`). A genuine Git worktree delegates to the original composer. A fallback is allowed only for a genuine non-repository; a broken repository, Git operational failure or wrong placement refuses (`:585`). Delivery, consultation and planning-pair callers retain the existing Git requirement. The shared JSON entry content is reused, and global `merge_json` remains unchanged.

Both settings and MCP objects are read, validated and rendered before creating `.claude` or modifying destination files (`:562`, `:839`). Unrelated JSON survives. The daemon-owned `hosted-directory-publications/<canonical-cwd-hash>.json` record contains version, canonical cwd, attempt id, exact originals/absence and rendered bytes (`:396`, `:785`). It is synced before destination changes. No new seat credential is written. Unrelated pre-existing JSON bytes, including any secrets already present there, are preserved and may be copied into the private publication journal. The new SeatBinding credential remains in the creation secret channel; authority remains in each binding.

Changed bytes are staged beside each destination, synced, then atomically replaced per file: settings first, `.mcp.json` last. Identical bytes are skipped. Complete file readback precedes native create/reload. Unix opens and mutations use directory-relative operations with `NOFOLLOW` and owner-only new files; symlink/nonregular configuration paths refuse (`:436`, `:475`, `:488`, `:631`). This phase qualifies that path on macOS; it does not claim Windows qualification.

Handled failures restore exact previous bytes/absence and remove only this attempt's temporaries and an empty `.claude` directory it created. An incomplete rollback preserves the record and returns the precise rule at `seat_mcp.rs:368`: “hosted directory MCP cleanup is incomplete; publication recovery must finish before create or reload”. Adapter logs include the underlying error. A pending pair is reconciled before a later subject/route/create/reload: retain a proved complete pair, or restore originals and then render the requested configuration (`:793`). External edits or malformed records refuse without clobbering foreign data. Per-file atomic replacement alone is not presented as crash-safe pair atomicity; persisted recovery closes that boundary. Native-create failure does not delete shared configuration. Existing prepared launch intents remain unchanged and are reused by normal launch recovery.

Metadata-only `retitle_seat`, `claim_hosted_seat` and `restore_bindings` receive no composition call. Tests deliberately leave malformed shared files in place while these metadata operations succeed with retained identities/scope and unchanged file bytes. `restore_bindings` restores delivery RuntimeBinding snapshots; hosted prepared intents recover through the tested hosted launch boundary, which validates/rebuilds composition before a relaunch.

## Own-author gates and reproduction

The locked command/environment receipts identify the actual tested input bytes and runtime checkout HEAD separately. A runtime HEAD label during dirty-source tests identifies the checkout and does not attest those source bytes. The final restored gates run on the source checkpoint recorded above. Each distinct mutation target has its own green baseline; no baseline is claimed separately for every seed.

| Gate | Observed result | Output SHA-256 |
| --- | --- | --- |
| paseo | test result: ok. 145 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.37s; test result: ok. 314 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.75s | `fe85eee75e33a30b1e97cf6dfe47eed7464303eaa830f82c03c86db945442c99` |
| fmt | exit 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| clippy | exit 0 | `b18a75663b6ed44af48eb2c03d797fd38807c56d583637e709e46b44c2b6e095` |
| durable_desks | test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 612 filtered out; finished in 2.96s | `d2d79d8e23bdf8fc305697354a3271aa6b167056fbebd6460088b6d898b4409e` |
| durable_seats | test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 608 filtered out; finished in 3.77s | `80176472f7be14dfea1f1ba2e3bb8cb0456ae959603201a38b1304b7dc8ce1de` |
| desk_adams | test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 596 filtered out; finished in 11.17s | `66bfc55e3fec0bcd2177d0a8a4cf4d59d1bfad6098902f895bd0b65703a5b3d6` |
| pr_reviews | test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 606 filtered out; finished in 7.20s | `80889a51a0febdc17ed4e487ac3374775d992810969fbefd081491007b4dddbe` |
| cli_parity | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.97s | `f38ef9f801ed1547615271c2c9db96bd7857630ea98ea7d087a968e85666528e` |

There are 14 added composer unit entries (including one subprocess helper) and 8 added production-adapter contract tests. The fresh Claude fixture uses `claude-opus-5`, high thinking, `seat_mcp: Some`, a genuine non-Git directory and the verified Desk scope. It proves shared JSON, leadership server/tool-policy framing, secret-channel separation and subject-specific identities. Missing-native replacement, account-qualified Claude route correction, fenced cross-vendor replacement, lost acknowledgement and exact-native recovery/reload use the same production boundary. Ad-hoc, epic, durable and PR subjects all enter this Desk boundary; existing daemon suites also prove their logical application paths. These are recorded Paseo transport tests with real filesystem composition, not an installed executable or live provider acceptance.

Exact test commands (each with the receipt's pinned environment):

```sh
cargo test --locked -p kontor-runtime-paseo --lib --test contract
cargo test --offline --locked -p kontor-cli --test durable_desk_parity -- --nocapture
cargo fmt --all --check
cargo clippy --locked -p kontor-runtime-paseo --all-targets -- -D warnings
cargo test --locked -p kontor-daemon --test loopback_api durable_desks:: -- --test-threads=2
cargo test --locked -p kontor-daemon --test loopback_api durable_seats:: -- --test-threads=2
cargo test --locked -p kontor-daemon --test loopback_api desk_adams:: -- --test-threads=2
cargo test --locked -p kontor-daemon --test loopback_api pr_reviews:: -- --test-threads=2
```

`reproduce-gates.py` records every command, start/end, exit, output digest and tested input hash. The Cargo target is the existing owned `.qualification/target-carrier-archive` cache; this is focused repair validation, not a fresh full-workspace archive gate. Loopback socket tests require the sandbox's scoped localhost permission. The zip URI, TMPDIR, Git ceiling and offline build settings are explicit in every receipt; no application `.env` is read.

## Failure-injection and lifecycle matrix

| Boundary | Injection or observation | Required outcome observed | Executable check |
| --- | --- | --- | --- |
| Pre-write validation | Invalid second JSON, array root, non-object mcpServers | No destination/directory/journal publication | `invalid_second_json_is_refused_before_any_file_or_directory_change` |
| Staging/publication/readback | Stage settings, stage MCP, publish settings, publish MCP, readback × absent/populated originals (10 cases) | Exact prior bytes/absence, attempt cleanup, supported retry succeeds | `handled_stage_publish_and_readback_failures_restore_exact_bytes_and_absence` |
| Rollback refusal | MCP publication plus settings rollback failure | Pending record and precise incomplete-cleanup error; route to Codex restores exact originals before future Claude | `rollback_failure_blocks_launch_with_precise_pending_recovery_then_converges` |
| Interrupted pair | Panic after settings; separate child process exits 86 before MCP, without unwinding | Record survives; next route restores originals and then composition succeeds | `interruption_between_publications_restores_originals_before_next_subject`; `real_process_exit_between_publications_recovers_originals_from_the_daemon_record` |
| Complete pair, pending cleanup | Fail record cleanup after complete readback | Pair retained, launch refused until record cleanup; retry keeps identical bytes | `interruption_after_complete_pair_retains_it_and_requires_cleanup_before_use` |
| Adapter launch barrier | Invalid JSON or externally edited pending pair | Zero native create; preserve foreign bytes and pending journal | `enabled_claude_directory_refuses_invalid_configuration_and_incomplete_publication_without_create` |
| Lost acknowledgement/reload | Create response lost; recovered native lacks initialized session; pending pair plus external edit | One create total; zero reload while incomplete; same-native retry reconciles then reloads once | `enabled_claude_directory_lost_ack_recovers_same_native_and_reconciles_before_reload` |
| Cross-subject serialization | Two subjects, canonical cwd alias, controlled blocking | Second cannot compose until first finishes; one shared configuration without a new seat credential | `two_subjects_serialize_on_canonical_cwd_and_keep_one_shared_nonsecret_surface` |
| Isolation and lifecycle | Two bindings/credentials/subjects in one workspace; missing predecessor; fenced cross-vendor replacement; account-qualified route | Separate native/binding authority, common files, no new seat credential written | `enabled_claude_directory_launch_replacement_and_subjects_keep_distinct_authority`; `enabled_directory_cross_vendor_replacement_and_route_correction_keep_shared_files` |
| Containment | Symlink config/journal, substituted parent during staging, directory-relative write | Foreign directory unchanged; incomplete journal retained when necessary | Three Unix containment tests |
| Metadata-only operations | Malformed shared files during retitle/claim/restore | Exact retained identity/scope and files untouched | Three adapter metadata regression tests |
| Existing Git/provider behavior | Broken Git marker, real Git repeat, disabled/foreign provider | Git errors refuse; exclude idempotence/clean status; no foreign provider publication | Existing composer tests plus two directory regression tests |

Fault callbacks are private test injection into the new composer and are no-ops in production. The real child exit is a test-binary process interruption, not a new released-daemon rehearsal. The adapter's incomplete-publication tests prove the create/reload barrier; individual injected filesystem steps are composer tests.

## Whole-change mutation phase

**20/20 non-equivalent mutants KILLED; zero survivors or invalid seeds.** 11 distinct exact command targets had green baselines. All mutated files and the prior Git status were restored byte-exact.

The first complete pass killed 19/20; removing the shared-cwd lock survived because a 50 ms timeout could pass under slow scheduling. Before freezing, the existing concurrency test was strengthened to observe the actual in-progress publication holding the shared canonical-cwd mutex, while retaining its two-subject exclusion test. No production change was needed for this finding. The [entire first pass](../../docs/evidence/ASMA-8454/mcp-repair/first-mutation-pass/results.json) is preserved. The full 20-seed phase was then repeated with the strengthened test.

The original plan's 12 desk/seat/Adam/PR and qualification-repair seeds are rerun, together with 3 adapter and 5 composer seeds for this repair. One seed is applied at a time; a `finally` restores the exact dirty input bytes before the next seed. [Catalog](../../docs/evidence/ASMA-8454/mcp-repair/mutations/catalog.json), [results with failing assertions](../../docs/evidence/ASMA-8454/mcp-repair/mutations/results.json), per-target baseline logs and [byte restoration](../../docs/evidence/ASMA-8454/mcp-repair/mutations/restoration.json) are durable. The composer/adapter seeds include the escaped eligibility fault, recovery bypass before reload, ignored composition refusal, skipped publication, ignored journal, skipped rollback, removed cwd lock and malformed-JSON acceptance.

| Seed | File:line | Fault | Result |
| --- | --- | --- | --- |
| MUT-001 | `crates/kontor-daemon/src/applications.rs:10755` | An epic prerequisite prevents independent desk materialization | KILLED |
| MUT-002-busy | `crates/kontor-core/src/durable_seat.rs:492` | Busy becomes authority to launch | KILLED |
| MUT-002-unchecked | `crates/kontor-core/src/durable_seat.rs:492` | Unchecked becomes authority to launch | KILLED |
| MUT-002-replay | `crates/kontor-daemon/src/applications/durable_seats.rs:246` | A recorded launch is re-executed after its native vanishes | KILLED |
| MUT-003-host | `crates/kontor-daemon/src/applications.rs:10736` | All Adam subjects on the shared host answer an epic-control lookup | KILLED |
| MUT-003-authority | `crates/kontor-daemon/src/applications.rs:18775` | Completion control authority accepts any active seat in the shared desk, including its durable seat and sibling subjects, regardless of role | KILLED |
| MUT-003-ecp | `crates/kontor-daemon/src/applications/desk_adams.rs:78` | The control resolver always requires historical ECP | KILLED |
| MUT-004-repository | `crates/kontor-store/src/repository/pr_reviews.rs:119` | Repository is dropped from PR lookup identity | KILLED |
| MUT-004-close | `crates/kontor-daemon/src/applications/pr_reviews.rs:1040` | Closing one PR enumerates every desk and sibling subject seat | KILLED |
| MUT-005 | `crates/kontor-store/src/repository.rs:23903` | An existing epic reads the project successor instead of its own frozen pin | KILLED |
| MUT-Q-CLI | `crates/kontor-cli/src/output.rs:81` | placement_blocked falls through to protocol exit 1 while the refusal body remains identical | KILLED |
| MUT-Q-ATOMIC | `crates/kontor-store/src/repository/desks.rs:48` | Commit the successor reservation before node validation, leaving a desk row after revision_conflict | KILLED |
| MCP-eligible | `crates/kontor-runtime-paseo/src/adapter.rs:6105` | Recreate the escaped non-Git Claude refusal | KILLED |
| MCP-recovered | `crates/kontor-runtime-paseo/src/adapter.rs:6169` | Bypass pending publication before exact-native reload | KILLED |
| MCP-ignore-refusal | `crates/kontor-runtime-paseo/src/adapter.rs:6175` | Ignore incomplete composition and create a native | KILLED |
| MCP-write | `crates/kontor-runtime-paseo/src/seat_mcp.rs:845` | Skip the directory publication | KILLED |
| MCP-journal | `crates/kontor-runtime-paseo/src/seat_mcp.rs:850` | Ignore interrupted pair originals on next route/subject | KILLED |
| MCP-rollback | `crates/kontor-runtime-paseo/src/seat_mcp.rs:975` | Delete journal/temps without restoring the prior bytes | KILLED |
| MCP-cwd-lock | `crates/kontor-runtime-paseo/src/seat_mcp.rs:849` | Allow two subjects to publish concurrently in the same canonical cwd | KILLED |
| MCP-json-guard | `crates/kontor-runtime-paseo/src/seat_mcp.rs:571` | Accept malformed second JSON and publish an empty object | KILLED |

## Preserved discovery evidence and limits

The original MCP HOLD is an enabled Claude launch refusal in a genuine non-Git desk cwd, after supported topology v5 and definition v8 were selected. Its [finding](../../docs/evidence/ASMA-8454/mcp-repair/discovery/mcp-finding.txt) and released request/response remain byte-pinned. The prior wrong-workspace fixture and mismatched-selection discovery realms stay immutable. Correct effect accounting remains: zero Claude launch at the MCP refusal, not zero total logical effects. This repair does not clear those discovery realms.

Three accepted follow-up defects stay out of this repair: (1) message-proof TEXT timestamp ordering, with original failed-log SHA `c0b596e9a7a31c2ac9057cefe88e7c3d10b6fa4369973962a7533359d5bdf7d2`; (2) shutdown's strong ownership cycle retaining the WAL connection, with supported stop → absence → released export drain → require empty WAL → restore; (3) epic-first partial-row ordering that predates the batch and is extended by TASK-003's desk-subject write ordering. The coordinator owns separate follow-up routing. The accepted ordering and WAL findings and their manifests are linked in this bundle; no implicated machinery is changed here.

Same-vendor LSA→TPM handoff and `keep_native` binding stay refused pending Igor's seat-credential decision. No test or report here claims same-vendor acceptance. Live Paseo lost-reply timing remains unproven; same-name/same-number/other-owner PR title collision stays a refused limit; close remains non-atomic with convergent retry and generic `unavailable` advice can incorrectly say “nothing was changed” after partial retirement; the no-proved-container branch remains disclosed rather than directly tested. The two Paseo-direct declarations remain governed by Igor's byte-pinned uncommitted dated Q-002 decision, not silently adopted. The earlier observation remains: the first released daemon-level reconcile with a Paseo-shaped plane occurred in this rehearsal; an optional follow-up is a daemon reconcile test with faithful directory fixtures and enabled composition.

The four JUDGE wording corrections belong in the eventual release record, without amending the frozen report: runtime-HEAD labels are checkout labels; mutation baselines are per distinct target; Q-002 is a blocked block, not a blocked file; generation 17 in the original source bundle was source-derived. Later released-export receipts read generation 17 from the executable itself. This repair does not claim export generation 18.

## Remaining gates and released re-proof

As of this report, QA and AUD and the coordinator's contrasting-vendor SEAT A / SEAT B / JUDGE review have not run on this repair. The PR is review-only/draft while that explicit prerequisite remains. The coordinator receives the exact carrier head and routes those gates. No released acceptance, merge, Reports resolution or runbook PR is performed by this report.

After approval/merge, the coordinator's released re-proof must:

1. Freeze merged source and locked build hashes for daemon, CLI and companion `kontor-mcp`; verify installed/executing daemon and companion identity. Retain every original HOLD package.
2. Run the released HTTP surface through the production adapter, composition enabled, the exact failing Claude route and a genuine directory fixture; read both JSON files and creation leadership frame/tool policy/secret separation.
3. Read epic LSA and supported TPM placement in DESK ADAM/ADAM with no ECP, exact ids/session/title/kind/cwd, binding and occupancy generations, membership/parentage, labels and ledger. Include both subjects sharing ADAM, all four structures, a Claude PR hosted seat and two PRs for one key; preserve isolated authority/evidence.
4. Prove restart/replay/lost-ack convergence and publication failure/recovery barriers; preserve Git/exclude behavior, unrelated JSON, account-qualified routes and foreign/disabled behavior.
5. Select both exact fixture successor hashes through supported operations before desk-led apply; compare old canonical bytes/pins/native identities; perform real stop → process absence → export drain → empty WAL assertion → restore/replay; desk-less rollback/reselection; partial PR close/readback/same-key retry and misleading-advice observation.
6. Record named Codex cleanup and all receipts, the disclosed limits and the four wording corrections. Only then complete the release record and small separately approved docs-only pre-restore runbook PR; the coordinator decides release and resolves the plan Reports row.

Cleanup of this repair's own test leftovers is recorded in [repair cleanup](../../docs/evidence/ASMA-8454/mcp-repair/cleanup.json). Original discovery packages and realms, production data and unrelated build caches are excluded from that cleanup. The earlier 226-entry qualification cleanup and orphan-desk sweep receipts remain unchanged.
