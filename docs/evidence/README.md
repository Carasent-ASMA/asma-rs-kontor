# Implementation evidence

Files below this directory are immutable or point-in-time delivery evidence.
Literal ESW, ECP, TSW, ASW, CSW and historical `Advice`/`ADVICE`, fixed
`ADVISOR`, `ASW · ...`, `Advisor · ...` or one-seat names record what a
specific revision proposed, tested or read back. They are not current naming
templates.

For new or migrated names, use [`../NATIVE_NAMING.md`](../NATIVE_NAMING.md).
One immutable pinned Team Definition JSON revision owns hierarchy, prefixes,
separators, templates, exact seat labels and capabilities. Historical evidence
must not be rewritten to resemble the current convention.

## Public tree, 2026-10-17 (ASMA-8511)

The repository is public, so evidence here holds no run receipts, no home
directory paths and no provider-account aliases. On 2026-10-17 the tree was
changed as follows, without rewriting history:

- Run receipts and deployment records (every `KON-MVP-18/run-<id>/` bundle,
  every `receipts/` directory, the `deployed-<commit>/` records and every
  `*receipt*.json` under an evidence root) were removed. Each removed file is
  listed with its git blob, SHA-256 and size in
  [`removed-run-evidence-2026-10-17.tsv`](removed-run-evidence-2026-10-17.tsv),
  and `git show 0971674d:<path>` still prints it.
- The owner's home directory was replaced with `$HOME` in text evidence, and
  with the synthetic `/Users/operator` where code needs an absolute path.
- Provider-account aliases were replaced with neutral ones that keep their
  order: each `-personal` alias became `-a` and each `-work` alias became `-b`
  (for example `claude-a`, `codex-b`).
- The process census in
  `ASMA-8159/contract-regeneration-2026-10-04/01-census.json` keeps only the
  `ps` lines of the processes it matched.
- The fixtures tests read moved into the crates: the Team Definition successor
  set to `crates/kontor-core/tests/fixtures/team-definition-successors/` and
  the synthetic census to `crates/kontor-daemon/tests/fixtures/census.sql`.

Hashes, byte counts and manifests recorded inside an evidence file describe
its bytes at `0971674d`. Where a replacement above changed a file, its recorded
hash no longer matches the public bytes; the original is in git history.

`scripts/public_tree.py`, the first gate of `scripts/verify-tree.py`, refuses
any of these coming back.
