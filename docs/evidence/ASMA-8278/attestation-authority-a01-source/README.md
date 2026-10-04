# ASMA-8278 ledger A-01 correction evidence

Exact two-path source: `e68310ce08e0e561d8490fe85dcbd1d17a9a96fa`, tree
`53572442a020d575ac4e224d5afd4a6e3704771a`, parent
`1e9b7d3b87a70fb03e3546d9351bd3e1c826312e`.

Restore now refuses any nonempty destination WAL before main-file metadata or
classification, including missing and zero-length main files. The immutable
ledger inspection still requires a nonempty main. Supported missing/empty-main
restore with absent or empty WAL remains covered by four positive subcases.

The [original worker manifest](manifest.json), raw logs, saved mutation runner
and definition are byte-for-byte evidence. Manifest SHA-256:
`1297d4b11bee7491fe5544f39344c219ca0681912fdf657d319d4189b5a4169f`.
[ROOT-PROVENANCE.json](ROOT-PROVENANCE.json) pins source and copied artifact hashes.

Both new refusal tests fail at the intended assertion against exact published
`67dc4a7` restore bytes. The correction passes three focused tests and all
23 affected backup tests, plus store Clippy with warnings denied, formatting
and diff checks. Removing the exact new guard in isolation kills the mutant
through both existing regression assertions after successful compilation.
Before/after baselines pass, and original and isolated source hashes match
after restoration. The earlier 990-test regression was not repeated.

The fixture uses genuine committed SQLite donor WAL/SHM bytes, with an
independent reader confirming the committed public-key row. Destination main
loss/truncation is simulated. The saved experiment could not recover copied WAL
alone. This proves refusal and residue preservation, not live crash recovery.

The [independent affected source review](SOURCE-REVIEW.md) confirms A-01 corrected
at source with no new source blocker. Its original manifest-hash abbreviation
is retained; the separate provenance records the correct full value above.
It ran no tests and did not independently re-diff the unchanged root spec.
Scripts retain original private paths; rerunning needs an isolated full
candidate checkout and deliberate path configuration.

The original ledger bundle and Medium finding remain preserved in
[attestation-authority-source](../attestation-authority-source/README.md).
Custody, issuance/admission, native possession/fencing, qualified continuity,
installed schema and formal/live epic acceptance remain unqualified.
