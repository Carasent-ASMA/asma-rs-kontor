# ASMA-8278 diagnostic owner-comparison source evidence

> **Date:** 2026-10-03 19:10 Europe/Oslo
> **Status:** Source validated; live qualification remains open
> **Author:** Root under Igor’s recorded execution mandate
> **Category:** report
> **Summary:** Exact published owner comparisons, tests, mutation evidence and bounded source findings.
> **When to load:** Assess module candidate26b and its remaining owner integration boundaries.

Runtime source candidate `26b1c8713d14684a6a3483c00fe4b74f4ed3d223` contains
three paths against base `32909ae1de9729d4e8b4bcd334420039f42ee31a`.
The specification is root `68626851460916488be6613084ca0d33118fcb4d` section9.4.
The result reports diagnostic consistency only; it grants no authority.

[MANIFEST.json](MANIFEST.json) pins the source and every portable evidence file.
The original [worker manifest](MANIFEST.worker.json), raw logs and mutation
runners are preserved unchanged. [Published provenance](PUBLISHED-CANDIDATE-READBACK.json)
records that worker head labels identify the pre-commit base while its source
hashes identify the published candidate. Preliminary11-test and final12-test
focused logs remain distinct. No earlier observation was rewritten.

The [full runtime log](owner-runtime-regression.log) records134 unit,
20 integration and two compile-fail privacy checks passing. Formatting,
whitespace and scoped Clippy passed. Seven isolated owner mutants fail by
assertion, with passing before/after baselines and restored source hashes.
The log-only whitespace attribute preserves generated trailing blank lines.

The [independent bounded findings](review-owner-source.md) identify no
substantiated blocker to this source slice. O-01 requires the eventual owner
to bind exact key material to authentic revision provenance; O-02 requires the
correct consultation-or-hosted generation domain. Neither detached supplied
facts nor revision equality establishes those owner contracts.

Authentic observation acquisition, transaction-held freshness, current
revocation storage, exclusive issuer custody, session possession, execution
fencing, positive finality/drain and enforced restrictions remain unqualified.
Final independent review, all paired cases, deployment, pilot and rollback
remain separate closure gates. No production caller or live effect is added.
