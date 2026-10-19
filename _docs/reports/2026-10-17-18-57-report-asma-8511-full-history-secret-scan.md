# ASMA-8511 full-history secret scan of asma-rs-kontor

> **Date:** 2026-10-17 18:57
> **Status:** 🟢 Complete — no credential found, nothing to revoke
> **Author:** Kontor maintainers — ASMA-8511
> **Category:** report
> **Scope:** `Carasent-ASMA/asma-rs-kontor`: every branch, tag and pull request ref, every commit, every blob and the contents of every `.gz` file in history, every GitHub Actions run log, plus GitHub secret scanning
> **Summary:** A read-only gitleaks scan of the whole history and of the Actions logs found no credential. Every finding is a content digest, a test idempotency key, a synthetic test canary or prose. GitHub secret scanning has no alerts. No revocation or rotation was needed and history was not rewritten.

---

## When to Load

Load this report to see what the 2026-10-17 scan covered and how each finding
was classified, or as the baseline for the next scan. For what to do when a
secret is found, use the
[leaked-secret runbook](../operations/2026-10-17-18-57-runbook-leaked-secret.md).

## Result

| Question | Answer |
| --- | --- |
| Real credentials found | 0 |
| Revoked or rotated | Nothing needed |
| GitHub secret-scanning alerts | 0 (backfill complete for git, pull requests, issues and discussions) |
| History rewritten | No |

No finding is a credential, so step 2 of the runbook (revoke and rotate) has
nothing to act on.

## What was scanned

The scan read a mirror clone taken on 2026-10-17 at 18:43, with `master` at
`0971674d`:

- 58 branches, 319 pull request refs and 4 tags;
- 1,589 commits: 1,335 non-merge and 254 merge commits;
- 9,272 blobs (1.98 GB), every file version in the mirror;
- 224 `.gz` files.

Actions logs live outside git. The run list taken at 17:46Z held 767 workflow
runs: 370 `ci`, 363 `Publication branch and title identity` and 34
`Update asma-modules Pointer`. All 767 log archives were downloaded. Ten of
them, all from cancelled branch-checker runs, were empty. The other 757 held
316 MB of logs.

The tool was gitleaks 8.30.1 (darwin arm64), checked against the release's
published checksum, with its default rules and `--redact`, so no value reached
a report. It ran five passes:

| Pass | Command | Covered | Findings |
| --- | --- | --- | --- |
| Commits | `gitleaks git <mirror> --log-opts="--all" --redact` | 1,325 commits reported scanned, 137 MB of added lines | 4,689 |
| Merge commits | the same, with `--log-opts="--all --merges --diff-merges=first-parent"` | 250 merge commits, 840 MB | 52,447 |
| Blobs | `gitleaks dir` over every blob written out by `git cat-file --batch` | 9,272 blobs, 1.98 GB | 2,688 |
| Compressed files | `gitleaks dir` over every `.gz` blob, decompressed | 224 files, 3.3 MB | 2 |
| Actions logs | `gitleaks dir` over every run's log archive, unpacked | 757 run logs, 316 MB | 28 |

A git-mode scan skips commits that add no lines and does not look inside
compressed files. The 1,335 non-merge commits include 6 empty commits and 8
that only delete lines. The blob pass covers every file version, whichever
commit introduced it, and the compressed-file pass covers what the other
passes cannot read.

## Findings by kind and disposition

Each value was read from its file in memory and reduced to a shape (for
example, 64 lowercase hex characters) without being printed. A value counts as
a credential only if its shape, its context and the file around it say so.

### Commit pass

| Rule | Shape | Disposition | Count |
| --- | --- | --- | --- |
| generic-api-key | SHA-256 hex | Content digest (false positive) | 4,665 |
| generic-api-key | UUID | Identifier (false positive) | 4 |
| generic-api-key | SHA-1 or git object id | Content digest (false positive) | 3 |
| generic-api-key | Readable words, hyphens and digits | Test idempotency key | 9 |
| generic-api-key | Digit and letter runs | Synthetic test canary | 5 |
| generic-api-key | `word/word` | Prose (false positive) | 2 |
| gitlab-pat | The digits 0–9 in order, then ten consecutive letters | Synthetic test canary | 1 |
| **Total** | | | **4,689** |

The 4,672 digests and identifiers sit in evidence files: 4,628 under
`docs/evidence/` and 44 under `_docs/reports/evidence/`. Most are manifest
entries that map a file path to that file's SHA-256. gitleaks matches the
word in the file name (for example a path ending in `auth.json`,
`credentials.rs` or `secret_canaries.json`), not anything in the value. The
rest are `dedup_key`, `*_token_sha256` and similar digest fields.

The 17 other findings:

| File | Rule | Disposition | Count |
| --- | --- | --- | --- |
| `crates/kontor-core/tests/spec_validation.rs` | gitlab-pat | Synthetic canary in a list of token-shaped inputs the test expects to be refused | 1 |
| `crates/kontor-store/tests/backup_export.rs` | generic-api-key | Synthetic canary the test plants for the export's secret scanner to find | 3 |
| `crates/kontor-daemon/tests/recovery_security.rs` | generic-api-key | Synthetic canary the test expects to be redacted | 2 |
| `crates/kontor-daemon/tests/loopback_api.rs` | generic-api-key | Test idempotency key | 3 |
| `crates/kontor-daemon/tests/mcp_journey.rs` | generic-api-key | Test idempotency key | 2 |
| `crates/kontor-mcp/src/dispatch.rs` | generic-api-key | Idempotency key in a unit test | 1 |
| `crates/kontor-store/tests/jira_materialization.rs` | generic-api-key | Test idempotency key | 1 |
| `docs/evidence/KBI-8050/high-scope-record.json` | generic-api-key | The same idempotency key, quoted as evidence | 2 |
| `docs/evidence/KON-OP-01/` (two review files) | generic-api-key | Prose: a `word/word` phrase after the word "keys" | 2 |

### Merge-commit pass

A first-parent diff of a merge repeats the content it brings in, so this pass
sees the same values again. All 52,447 findings repeat a finding of the commit
pass (same rule, file and redacted match). By shape: 52,386 SHA-256 digests,
6 SHA-1 digests, 49 test idempotency keys and synthetic canaries of the shapes
above, 4 prose matches, and the `gitlab-pat` canary of
`crates/kontor-core/tests/spec_validation.rs` twice.

### Blob pass

This pass reads each file version once, so its counts are per blob rather
than per commit. The 2,688 findings:

| Disposition | Count |
| --- | --- |
| SHA-256 digest | 2,307 |
| SHA-1 digest | 3 |
| UUID | 2 |
| Test idempotency key, in versions of the files listed above | 327 |
| Synthetic test canary (`api_key` and `token` canaries, versions of `backup_export.rs` and `recovery_security.rs`) | 32 |
| `gitlab-pat` canary, 13 versions of `spec_validation.rs`, each the digits 0–9 followed by ten consecutive letters | 13 |
| Prose (`word/word` after "keys") | 4 |
| **Total** | **2,688** |

No value has a shape the commit pass did not already explain.

### Compressed-file pass

Both findings are `openapi_sha256` content digests on line 5 of two
contract-parity logs: `final-combined-2026-10-04/receipts/fc-gate1-contract-parity.log.gz`
and `production-prep/receipts/f3-generated-console-contract-parity.log.gz`,
both under `docs/evidence/ASMA-8159/`.

### Actions-log pass

All 28 findings are `generic-api-key` matches on an `"idempotency_key"` field
holding a UUID, each value different. They come from 28 `ci` runs on
2026-08-12, in the `Rust workspace gates` step, where the test binary
`crates/kontor-cli/tests/envelopes.rs` printed CLI refusal envelopes. Each
envelope echoes the synthetic `/v1/commands/launch_run` request the test sent,
and the test made a fresh random key for each run. That test existed only on
2026-08-12 (commit `5e1c8520`, KON-MVP-16). Disposition: test idempotency key
(false positive).

## What this scan does not cover

- Forks and clones made by others, and GitHub's cached views. A credential
  found later is revoked first, whatever these hold.
- Workflow runs started after the 17:46Z run list. The next scan picks them
  up. The repository has 2 releases with no assets and no stored workflow
  artifacts, so neither has anything to scan.
- Secrets that gitleaks' default rules do not recognise. GitHub secret
  scanning, now enabled with push protection, runs its own provider patterns
  on every push.

## Related clean-up in the same task

ASMA-8511 also removed run receipts and the owner's home path from the public
tree. It removed provider-account aliases from evidence, documentation, plans
and configuration examples, but not from code: 51 files under `crates/` and
`apps/` still name them, listed one by one in `ALIAS_CODE_EXEMPTION`. These
are the route and provider catalogs, the bundled team pack and committee
preset, the generated contract, the Tokenomics labels, and the doc comments and
tests that follow them. Renaming them waits on a host decision, an open KR-30
gap. `scripts/public_tree.py` now refuses all three kinds anywhere else. None
of these is a secret, so none needs revoking. The evidence README records the
clean-up: [`docs/evidence/README.md`](../../docs/evidence/README.md).

## Rerunning

The [leaked-secret runbook](../operations/2026-10-17-18-57-runbook-leaked-secret.md#scanning-the-whole-history)
gives the commands. Compare the next result with the counts above: any
finding that is not one of the shapes listed here gets classified by hand.
