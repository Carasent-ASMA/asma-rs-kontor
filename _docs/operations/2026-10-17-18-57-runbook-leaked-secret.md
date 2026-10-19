# Respond to a secret published in this repository

> **Date:** 2026-10-17 18:57
> **Status:** 🟡 In Review
> **Author:** Kontor maintainers — ASMA-8511
> **Category:** runbook
> **Scope:** `Carasent-ASMA/asma-rs-kontor`, a public repository: its commits, branches, pull requests, issues, comments, Actions logs and release assets
> **Summary:** Revoke and rotate the secret first. Then remove it from the current tree, assess the exposure and record the incident. A history rewrite comes last, and only if the product owner decides on one.

---

## When to Load

Load this runbook when a credential, token, private key, password, session
cookie or provider-account binding is found in anything this repository
publishes. The finding can come from GitHub secret scanning, push protection,
a private vulnerability report, a gitleaks run or a reader.

Do NOT load it as permission to rewrite history, force-push, delete branches
or change repository rules. Those steps are not part of this runbook.

---

## The order

Anything pushed to a public repository is already public: other people may
have cloned or forked it, and caches and mirrors may hold a copy. Removing
the value from the repository does not make the secret safe again. Revoking
it does. So the order never changes:

1. **Contain.** Do not repeat the value anywhere: not in an issue, a pull
   request, a commit message, chat or a ticket. Refer to the secret by its
   kind and its location (commit, path and line), never by its value.
2. **Revoke and rotate.** The owner of the credential revokes it at the
   service that issued it, issues a replacement, and installs the
   replacement wherever the old one was used. This happens at once. It
   does not wait for steps 3 to 6 or for any discussion about history.
3. **Prove the revocation.** Confirm that the old value is refused by the
   service. A rotation that leaves the old value working has not happened.
4. **Assess the exposure.** Find when the value was first pushed and every
   place it is reachable from: branches, tags, pull request refs, forks,
   release assets and Actions logs. Check the issuing service's audit log
   for any use of the old value between that time and the revocation.
5. **Remove it from the current tree.** Open an ordinary reviewed pull
   request that removes the value, and add a test or fixture rule that
   would have refused it. This does not remove it from history.
6. **Record the incident.** Write the full record (kind, location, times,
   revocation proof, exposure assessment) somewhere private: Kontor or a
   restricted ticket. A public record names only the kind and what was done
   about it.
7. **Decide about history.** Only after steps 2 and 3 are done, the product
   owner decides whether to rewrite history. The decision is Igor Efrem's to
   make at that time (2026-10-17). No one else rewrites, force-pushes or asks
   GitHub to purge cached views without that decision.

## Who does what

| Step | Owner |
| --- | --- |
| Contain | Whoever finds it |
| Revoke, rotate, prove | The owner of the credential. If that owner cannot be reached, a repository administrator who can revoke it at the issuing service |
| Assess, remove from the tree, record | A Kontor maintainer |
| Rewrite history | Only on the product owner's decision |

## Revocation by kind

| Kind | Where to revoke and rotate |
| --- | --- |
| GitHub token or App private key | The GitHub account or App settings that issued it. For an App key, generate a new key, replace the PEM file the daemon reads, then delete the old key |
| Jira or other Atlassian API token | The Atlassian account's API token page. Reinstall the replacement through the stopped-realm `install-jira-credential` command |
| Kontor realm credential | `kontor-daemon --state-root <dir> rotate-credentials` on a stopped realm, or `kill -HUP <pid>` on a serving one (see [`RECOVERY.md`](../../RECOVERY.md)) |
| Model-provider API key or login | The provider's console. Sign the affected account out everywhere, and rotate any key derived from it |
| Cloud, database or other service credential | The issuing service's console, following that service's own procedure |
| Provider-account binding or other account data (no secret) | Nothing to revoke. Remove it from the tree (step 5) and record it (step 6) |

## A history rewrite

A rewrite is never a substitute for revocation. When the product owner
weighs one, these facts apply:

- Clones and forks made before the rewrite keep the value.
- A rewrite changes every later commit id, so links to those commits break.
  Open pull requests have to be rebuilt.
- Pull request refs and cached views on GitHub can outlive a rewrite. Only
  GitHub Support can remove them.

If a rewrite is decided, it is planned and announced as a separate change.
This runbook does not cover how to carry it out.

## Scanning the whole history

Rerun this read-only scan after any suspected leak, and before the repository
is promoted in a new place. It needs a mirror clone, `gh` with read access to
the repository's Actions, and
[gitleaks](https://github.com/gitleaks/gitleaks) (8.30 or later, with its
checksum checked against the release).

A single git scan is not enough. It skips merge commits and commits that add
no lines, and it never looks inside compressed files. Actions logs are not in
git at all. Run five passes:

```sh
git clone --mirror https://github.com/Carasent-ASMA/asma-rs-kontor.git kontor-mirror.git

# 1-2. The lines each commit adds, then the first-parent diff of each merge.
gitleaks git kontor-mirror.git --log-opts="--all" --redact \
  --report-format json --report-path gitleaks-commits.json
gitleaks git kontor-mirror.git --log-opts="--all --merges --diff-merges=first-parent" \
  --redact --report-format json --report-path gitleaks-merges.json

# 3. Every blob, whichever commit introduced it.
mkdir blobs
git -C kontor-mirror.git cat-file --batch-all-objects \
  --batch-check='%(objectname) %(objecttype)' |
  awk '$2 == "blob" { print $1 }' |
  while read -r id; do git -C kontor-mirror.git cat-file blob "$id" > "blobs/$id"; done
gitleaks dir blobs --redact --report-format json --report-path gitleaks-blobs.json

# 4. The contents of every gzip blob.
mkdir gz
for f in blobs/*; do
  gzip -t "$f" 2>/dev/null && gzip -dc "$f" > "gz/${f##*/}"
done
gitleaks dir gz --redact --report-format json --report-path gitleaks-gz.json

# 5. Every workflow run's log archive.
mkdir actions
gh api --paginate 'repos/Carasent-ASMA/asma-rs-kontor/actions/runs?per_page=100' \
  --jq '.workflow_runs[].id' |
  while read -r id; do
    gh api "repos/Carasent-ASMA/asma-rs-kontor/actions/runs/$id/logs" > "actions/$id.zip" &&
      unzip -q -o "actions/$id.zip" -d "actions/$id"
  done
gitleaks dir actions --redact --report-format json --report-path gitleaks-actions.json
```

`--redact` keeps values out of the report. A cancelled run can return an
empty log archive; count those rather than treating them as failures. Classify
each finding by the shape of the value (a content digest, an identifier, a
synthetic test canary or a credential) without printing it, and handle any
credential through the order above.

The record of the 2026-10-17 scan is the
[full-history secret scan report](../reports/2026-10-17-18-57-report-asma-8511-full-history-secret-scan.md).
