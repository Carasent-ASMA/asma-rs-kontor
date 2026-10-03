# ASMA-8278 public-key ledger source evidence

Exact source candidate: `67dc4a7f7302d8e94101eb3786571af6d930c737`, tree
`9df5fe2ce450b8c367092aced84f32d15d90dcd2`, parent
`e3fdbe130b6ec3868d5b7d4dfa558eb9e51c5852`. Twelve core/store source and test
paths implement public-key persistence, immutable mappings, irreversible
revocation and refusal of unsupported nonempty-ledger export/restore.

The original [worker manifest](manifest.json) is preserved byte-for-byte:
SHA-256 `ce009be367771e8fbe0a323b95106ce041fb1acea4831322013a31ea36b312c6`.
[PROVENANCE.json](PROVENANCE.json) maps the pre-commit base label to the exact
candidate and records all twelve source hashes and copied evidence hashes.

Validation covers 990 tests across 54 core/store suites and five core doctests
(one executable and four privacy compile-fail cases), plus formatting, scoped
all-targets Clippy with warnings denied and diff checks. Coverage combines the
714 successful broad-run tests with 274 remaining/repaired tests and replaces
the older 18-test backup binary with the final 20-test run. The initial broad
command failed; its log and both historical-v115 fixture failures remain.
The manifest records this timing rather than claiming a single green full run.

Eight isolated mutants failed intended existing assertions after compilation;
the original and isolated twelve-path hashes were restored after each. Restored
ledger and three backup target baselines pass. The originally executed
`run_mutations.executed-v1.py` and first classifier report remain unchanged.
M03 and M05 initially reported unresolved because their assertions used custom
messages. The saved classifier correction and assertion proof identify the
exact existing test lines and raw panics; compiler failures are not kills.

The scripts retain their original private execution paths. Reproduction needs
an isolated full candidate checkout and explicit path configuration. Build
targets and the full isolated repository are excluded. No independent rerun is
claimed; the bounded independent source review is pending separately.

This proves bounded source behavior only. Detached reads grant no freshness or
authority. Issuance, current token/admission integration, custody, native
possession/fencing, qualified backup continuity, installed schema, pilot and
final epic acceptance remain unsupported or unqualified. Frozen R3 and the
original Goal and task closure conditions are unchanged.
