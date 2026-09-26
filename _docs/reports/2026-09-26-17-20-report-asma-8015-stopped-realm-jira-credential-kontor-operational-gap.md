---
title: ASMA-8015 stopped-realm Jira credential delivery
date: 2026-09-26
status: corrected-source-qualified-independently-verified-live-delivery-pending
jira: ASMA-8015
orchestration: paseo-direct-recovery
---

# ASMA-8015 stopped-realm Jira credential delivery

At the start of this work, the installed e29 native Jira connector read `kontor-jira` credentials through the OS Keychain, but has no supported installation command. The historical `daf5149e3e1a0467a05f37105faa3b6a4057e3b9` installer is not in e29 and used a different macOS Keychain implementation. This candidate ports the operation to the current reader rather than crediting that historical patch as delivered.

## Goal

Make Kontor the single owner of Jira workflow policy, replacing the `asma jira sync --request-json -` subprocess delegation with a native connector composed directly into `kontord`.

This is the existing ASMA-8015 task Goal. This report covers its credential-installation gap; it does not claim the complete task or ASMA-7869 live journey.

## Initial candidate behavior at 3a35 (source support withheld)

`kontor-daemon --state-root <absolute-root> install-jira-credential --alias <nonsecret-alias>` accepts its document only through stdin. The JSON has exactly `email` and `api_token`; unknown fields, blank or oversized values, control characters and malformed text are refused. Input reads stop after 8,193 bytes, enforcing the 8,192-byte document limit. Credential buffers have zeroizing ownership. Installation holds the exclusive state-root lock through readback and refuses a running realm before reading stdin or reaching any credential effect.

The installer and native connector share the same strict validator. MacOS writes use the existing Apple-signed `/usr/bin/security` identity with `-i -q` in argv; the secret travels through stdin, with quotes and backslashes escaped and control characters refused. The generated command must fit Apple's 4,096-byte interactive buffer; oversized generated commands are refused before launch, even if their document fits the parser limit. Writer and reader subprocesses have a five-second deadline, with kill and reap on expiry. Non-macOS uses the existing keyring backend.

A successful writer result is insufficient: the production-compatible reader must return the exact canonical stored document, which is validated again. Errors expose only static, redacted messages. No secret, secret digest or backend diagnostic is printed. Supplying stdin through a password-manager pipe keeps credential material out of command arguments and shell history.

## Preliminary verification

The initial sandboxed attempt failed at sccache with `Operation not permitted`, before tests ran: [retained failed attempt](evidence/ASMA-8015/stopped-realm-credential/initial-jira.txt.json). It earns no qualification or mutation credit. With sccache disabled and an isolated cache cloned from a completed private test cache, the [initial Jira library run](evidence/ASMA-8015/stopped-realm-credential/initial-jira-no-sccache.txt.json) passed 13 tests.

The subsequent [account and Jira contracts](evidence/ASMA-8015/stopped-realm-credential/credential-contracts.txt.json) passed 154 tests; the [daemon operator contracts](evidence/ASMA-8015/stopped-realm-credential/operator-contracts.txt.json) passed five, including locked-realm refusal, lock retention through installation, malformed-input refusal and alias-only CLI parsing. [Strict scoped Clippy](evidence/ASMA-8015/stopped-realm-credential/strict-clippy.txt.json) exited zero. These are preliminary working-tree checks, not an exact immutable-candidate archive gate. The final candidate also preserves configuration-error classification for invalid aliases and uses a different valid document in the reader-mismatch fixture so equality refusal is independently testable.

All five raw logs are preserved through strict base64 carriers with decoded lengths and hashes. [SHA256SUMS.json](evidence/ASMA-8015/stopped-realm-credential/SHA256SUMS.json) binds the carriers. No live credential was installed; macOS tests exercise command construction, quoting, bounds and subprocess timeout with synthetic values. Real OS persistence/readback and service application remain distinct proof obligations.

## Remaining delivery boundary

An immutable source checkpoint must receive genuine behavioral mutation kills, green baseline/restoration, exact source restoration and full source qualification before merge/build/deployment. An independent source assessment must retain any defects and limits. Later stopped-realm application must prove actual writer/reader compatibility and restart/readback without exposing a credential. The bootstrap/client/MCP surface, ASMA-7869 isolated live acceptance, producer-provenance questions and governed-summary gap remain separate. Existing historical FAILs and findings remain unchanged; no Jira, SQLite, runtime, deployment, gate or closure credit follows from this source candidate.

## Exact-source focused mutation qualification

Source checkpoint `3a35b7c6db8ef65e51aaa14c7ba69df9bf1e1c0f`, tree `ea784274c592100b9154f8c9a1fbd4a297ab7ffc`, was archived into a disposable source directory with a separate private cloned cache. The [execution results](evidence/ASMA-8015/stopped-realm-credential/mutations-3a35b7c6/execution-results.json) retain the exact original private execution locators. The adjacent [publication receipt](evidence/ASMA-8015/stopped-realm-credential/mutations-3a35b7c6/publication-receipt.json) binds every log to its published strict base64 carrier, decoded length and hash. The [runner](evidence/ASMA-8015/stopped-realm-credential/mutations-3a35b7c6/runner.py) restores each changed file in `finally`.

All six faults compiled and were killed by exactly one intended assertion, exit 101: removing the realm lock, accepting a different valid readback, skipping field validation, removing the stdin bound, removing the Apple command-length bound, and removing quoting. Each actual patch is preserved as a `.diff.json` carrier alongside its log. No compilation or harness failure is credited as a kill. Baseline and restoration each passed 77 account unit tests, 14 Jira unit tests and five daemon operator tests, totaling 96 with zero failures or ignored tests. Entire extracted-source inventories before and after match exactly; no installed binary, live SQLite or primary source was mutated.

The six focused kills qualify only the tested boundaries of 3a35. They do not cover the independent defects recorded below and cannot qualify a corrected successor. This result is not OS Keychain persistence, service deployment, workflow verification, epic acceptance or closure.

## Actual independent audit and rejected-source gate

The [complete actual LSA finding](evidence/ASMA-8015/stopped-realm-credential/audit-3a35b7c6/actual-lsa-source-audit.txt.json), preserved without normalization with its [publication receipt](evidence/ASMA-8015/stopped-realm-credential/audit-3a35b7c6/publication-receipt.json), withholds source support: no P0, two P1 and two P2. Another directory's lock can authorize replacement of the same host-global alias, and failed verification can leave a changed credential installed. The five-second transport deadline and absolute-root claim are also incomplete. The initial behavior description above is therefore not proof of a sound stopped-realm effect fence or atomic installation.

The obsolete full gate was stopped after that finding, by signalling only its identified cargo process and test child. Its [unchanged execution receipt](evidence/ASMA-8015/stopped-realm-credential/audit-3a35b7c6/receipt.json) records actual exit 1; the [operator cancellation](evidence/ASMA-8015/stopped-realm-credential/audit-3a35b7c6/operator-cancellation.json) and [raw log carrier](evidence/ASMA-8015/stopped-realm-credential/audit-3a35b7c6/interrupted-full-gate.txt.json) retain the actual outcome. No full-gate PASS is claimed. Corrected source requires new immutable qualification and another actual independent assessment.

## Authorized isolated authority correction

Igor explicitly authorized the isolated source correction and synthetic/fake-backend tests on 2026-09-26. The prior automatic-approval hold and rejected 3a35 source assessment remain historical evidence; this authorization permits no live Keychain mutation, credential extraction, installation or deployment.

The corrected installer requires an absolute canonical directory, holds its canonical-root lock, reads an already-initialized Realm through a read-only store preflight, and requires the exact alias in strict `jira.json` before reading stdin. It creates no missing database and applies no migration. The credential address is `kontor-jira:<realm-id>:<SHA-256-of-canonical-root-bytes>` plus the exact alias. The production connector and installer derive that same address. A copied Realm database in another root, a different Realm, or a duplicate alias therefore cannot replace the original consumer's entry. Canonical symlink aliases resolve to the same root and lock. There is no fallback to the old host-global address; existing global entries are not read, moved or deleted by this repair. Separate stopped-realm enrollment must precede any future deployment that needs Jira credentials.

Before a write, the installer establishes the previous state through the reader. Access failure refuses before mutation. If a write or exact validated readback fails, it restores the exact previous bytes, or deletes a newly created entry, and verifies restoration or absence. Typed redacted outcomes distinguish verified rollback from rollback that could not be proven; an uncertain rollback requires reconciliation before restart. This is verified rollback behavior, not a claim of a transactional OS Keychain API.

The macOS transport now starts one deadline before spawn and uses safe nonblocking pipe operations for stdin and stdout, with a 16,384-byte output bound and checked child termination/reaping. Synthetic regressions block a large pipe write and a stalled child, then prove its PID is neither running nor still waitable. The deadline accounts for spawning, pipe delivery and response collection; OS process creation and cleanup are synchronous operations, so no hard real-time bound on an unresponsive operating system is claimed. The existing lockfile's `rustix` 1.1.5 is reused as an exact workspace dependency for safe nonblocking descriptors and process checks; no unsafe application code or secret-bearing command arguments are introduced.

Fake-backend coverage includes denied previous-state reads, mismatched/malformed/denied post-write reads, write failure after an effect, restoration of an existing opaque value, deletion of a new value, failed rollback writes/deletes/verification, same-alias copied-Realm isolation, actual connector address agreement, global-address refusal, and preflight rejection before stdin. All test realms and subprocesses are disposable. Focused working-tree checks are preliminary until a new immutable archive is qualified; the six historical 3a35 kills supply no corrected-source qualification.

### Corrected working-tree evidence

The [preliminary receipt](evidence/ASMA-8015/credential-authority-correction/preliminary/preliminary-receipt.json) records 128 passing focused tests with no failures or ignored tests, followed by 41 passing Jira checks after canonical-path compatibility was added. Corrected strict scoped Clippy exited zero. The first Clippy failure and check failure remain preserved; they are not counted as passes. The [14-member manifest](evidence/ASMA-8015/credential-authority-correction/preliminary/SHA256SUMS.json) binds these preliminary files:

- [account-unit.txt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/account-unit.txt.json)
- [check-after-test-fix.txt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/check-after-test-fix.txt.json)
- [check.txt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/check.txt.json)
- [daemon-operator.txt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/daemon-operator.txt.json)
- [focused-results.json](evidence/ASMA-8015/credential-authority-correction/preliminary/focused-results.json)
- [historical-source-approval-hold.json](evidence/ASMA-8015/credential-authority-correction/preliminary/historical-source-approval-hold.json)
- [isolated-source-authorization.json](evidence/ASMA-8015/credential-authority-correction/preliminary/isolated-source-authorization.json)
- [jira-canonical-root-contracts.txt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/jira-canonical-root-contracts.txt.json)
- [jira-contracts.txt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/jira-contracts.txt.json)
- [preliminary-receipt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/preliminary-receipt.json)
- [realm-preflight.txt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/realm-preflight.txt.json)
- [runner.py](evidence/ASMA-8015/credential-authority-correction/preliminary/runner.py)
- [strict-clippy-corrected.txt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/strict-clippy-corrected.txt.json)
- [strict-clippy.txt.json](evidence/ASMA-8015/credential-authority-correction/preliminary/strict-clippy.txt.json)

The existing 3a35 audit and all historical mutation/log carriers remain unchanged. Exact corrected-source mutation and archive qualification, independent source assessment and any subsequent merge/build/deployment remain pending.

## Exact integrated e53 source audit and mutation evidence

Integrated source `e53f19b744b80862b166b613dcb8992bcf371215`, tree `24a8970facd62418aa77e6def93c7ef65f31e2c5`, composes the isolated credential correction with deployed ASMA-8114 source `173d399bfd44ec598332b4fe1cdf882af024fe5c`. The archival-read guard, strict effect renderer, GET 409 contract and dedicated regressions are preserved. The installed daemon remains 173d; this composition is source only.

The [complete actual independent LSA source finding](evidence/ASMA-8015/credential-authority-correction/audit-e53f19b7/actual-lsa-source-audit.txt.json) and [receipt](evidence/ASMA-8015/credential-authority-correction/audit-e53f19b7/publication-receipt.json) preserve its leading commentary and actual final-byte state. The finding reports no P0/P1/P2 and resolves the four earlier source defects within the stated boundary. Its review took no credit for then-running exact-e53 qualification and supplies no unconditional merge/deployment support.

The [unchanged exact-e53 execution receipt](evidence/ASMA-8015/credential-authority-correction/mutations-e53f19b7/execution-results.json), [runner](evidence/ASMA-8015/credential-authority-correction/mutations-e53f19b7/runner.py), and [publication bindings](evidence/ASMA-8015/credential-authority-correction/mutations-e53f19b7/publication-receipt.json) now establish nine genuine compiled behavioral kills, each exit 101 with its intended assertion. The faults remove root namespace isolation, verified rollback, prior-state refusal, alias preflight, absolute-root refusal, subprocess deadline, bounded stdin delivery, child reaping, and read-only database opening. Actual contextual patch bytes and raw logs are losslessly encoded; their original private execution paths remain unchanged in the receipt.

Baseline and restoration each passed 78 account, 19 Jira unit, two scope integration, seven daemon operator and two Realm preflight tests: **108 passed, zero failed or ignored**. The [before](evidence/ASMA-8015/credential-authority-correction/mutations-e53f19b7/source-entries-before.json) and [after](evidence/ASMA-8015/credential-authority-correction/mutations-e53f19b7/source-entries-after.json) inventories are byte-identical across all 2,849 extracted file contents and reproduce the immutable Git archive. Each decoded patch independently reproduces its recorded mutated-source hash. The mutation cache is a distinct private clone; source restoration does not imply source-to-binary provenance.

The preceding audit/mutation publication took no credit for the then-running exact-e53 full gate. Its completed exit-zero receipt is recorded below; independent receipt verification remains required before source merge consideration. The 4,096-byte Apple interactive-command platform limit, actual OS persistence/reader compatibility, enrollment at every configured scoped alias, verified rollback under actual OS failures, and controlled same-Realm restart remain separate live-boundary proof obligations. This authorized work performs no real Keychain operation, credential extraction, installation or deployment and grants no task, workflow, gate, epic or closure credit.

The [complete correction manifest](evidence/ASMA-8015/credential-authority-correction/SHA256SUMS.json) binds all preliminary, source-audit and exact-mutation publication files. Earlier 3a35 findings, six kills, interrupted gate and the initial automatic-approval hold remain unchanged history.

## Completed exact-e53 full archive qualification

The completed [full-gate carrier](evidence/ASMA-8015/credential-authority-correction/qualified-e53f19b7/full-gate.txt.json) strictly decodes to 758,762 bytes with SHA-256 `1e4afcd4d7f932c26e533343796460e3e51fcaaf67a16e78149047d895cd985f`. The [unchanged original execution receipt](evidence/ASMA-8015/credential-authority-correction/qualified-e53f19b7/execution-receipt.json), [runner](evidence/ASMA-8015/credential-authority-correction/qualified-e53f19b7/runner.py), and [source qualification](evidence/ASMA-8015/credential-authority-correction/qualified-e53f19b7/source-qualification.json) bind exact e53 commit/tree/archive and terminal exit **0** at `2026-09-26T19:32:22.062775+00:00`. Earlier statements that this gate was running describe the preceding publication, not this completed receipt.

Independent recomputation from the raw log totals 151 Rust summaries, **2,940 passed, zero failed, 9 ignored**, plus **305 console tests**. Formatting, strict workspace Clippy, locked workspace tests, audit, deny, frozen install, typecheck and production audit all completed. All nine allowed audit warnings (eight unmaintained and one unsound) and nine ignored tests are preserved; this is not universally warning-free or unignored. The exact archive SHA-256 is `0b0d9701d7471390439b8475017e255222686312bdac83c4034a274c4f6673b0`. The complete mutation before/after inventories remain the source-restoration proof; no additional final extracted-directory inventory is claimed for the full gate.

The independently reviewed source design and completed synthetic qualification have received the actual independent receipt verification recorded below; merge remains a separate authorized action. The authorized scope remains source and fake-backend tests only. No real Keychain operation, enrollment, credential extraction, installation, deployment, integration, workflow, gate, task, epic or closure credit is supplied. The operating-system/platform and future governed enrollment boundaries stated above remain pending.

## Actual completed-qualification disposition

The same persistent LSA independently verified the completed packet at `03c3eeec5c5aa618ed2ed7e00ab583f375e3db9f`. Its [complete actual native qualification finding](evidence/ASMA-8015/credential-authority-correction/audit-qualified-e53f19b7/actual-lsa-qualification-audit.txt.json) and adjacent [publication receipt](evidence/ASMA-8015/credential-authority-correction/audit-qualified-e53f19b7/publication-receipt.json) retain the exact leading horizontal rule, whitespace, native identity, absent final LF and absent terminator. Only the activity viewer wrapper was removed.

The finding reports **P0/P1/P2 none** in the authorized source/fake-backend scope and supports source merge consideration for PR #278. It independently reproduces all 54 reviewed manifest hashes, 39 carriers, 16 immutable source objects, the exact archive, all nine compiled behavioral kills, 108 baseline/restored passes, complete source restoration, full-gate exit zero, 2,940 Rust and 305 console passes, nine ignored tests and all nine allowed audit warnings. Context policy remains best-effort `not_enforced`; no compaction success is claimed and the persistent native/session is unchanged.

This completes the isolated source-and-test correction phase. Merge authorization/provenance, an exact merged archive build, governed enrollment at the canonical Realm/root/alias address, real macOS Keychain command-boundary/persistence/ACL/update/rollback proof, and controlled same-Realm restart/connector readback remain pending. Current authorization supplies no live credential access or extraction, installation, deployment, live Jira/SQLite/Kontor writes, task/workflow settlement, epic acceptance or closure. Historical rejected 3a35 findings and its interrupted gate are preserved; present resolutions do not rewrite that round.
