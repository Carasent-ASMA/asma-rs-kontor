---
title: ASMA-8015 stopped-realm Jira credential delivery
date: 2026-09-26
status: source-correction-required
jira: ASMA-8015
orchestration: paseo-direct-recovery
---

# ASMA-8015 stopped-realm Jira credential delivery

The installed e29 native Jira connector reads `kontor-jira` credentials through the OS Keychain, but has no supported installation command. The historical `daf5149e3e1a0467a05f37105faa3b6a4057e3b9` installer is not in e29 and used a different macOS Keychain implementation. This candidate ports the operation to the current reader rather than crediting that historical patch as delivered.

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
