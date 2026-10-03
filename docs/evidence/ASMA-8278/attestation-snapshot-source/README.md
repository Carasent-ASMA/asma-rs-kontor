# ASMA-8278 public snapshot codec source evidence

> **Date:** 2026-10-03 19:32 Europe/Oslo
> **Status:** Source validated; independent source review pending
> **Author:** Root under Igor’s recorded execution mandate
> **Category:** report
> **Summary:** Exact bounded public snapshot codec and preserved validation evidence.
> **When to load:** Assess module845e36c source and its public-configuration boundary.

Source candidate `845e36c36efa9bbf3912c61f91fb7fdb2b590ea9` has three paths
against base `26b1c8713d14684a6a3483c00fe4b74f4ed3d223`. Its specification
is root `68626851460916488be6613084ca0d33118fcb4d` section9.3.

[MANIFEST.json](MANIFEST.json) pins source hashes and every portable evidence file.
The original [worker manifest](MANIFEST.worker.json), mutation runner and raw
logs are unchanged. [Published provenance](PUBLISHED-CANDIDATE-READBACK.json)
maps the original base label to the exact published-source hashes.

The [runtime log](runtime.log) records141 unit,20 integration and two
compile-fail privacy checks passing. Seven codec tests cover roundtrip,
strict fields, schema, size and shared structural refusals. Formatting,
whitespace and scoped Clippy passed. Four isolated mutants fail by assertion:
preparse size, exact schema, shared validation and UTF-8 byte counting.
Before/after baselines pass; original and isolated source hashes are restored.
The log-only whitespace attribute preserves generated trailing blank lines.

The codec bounds public JSON transport and returns freely constructable,
untrusted configuration. It establishes no cryptographic DER validity,
authenticity, freshness or authority, and has no production caller, signing,
keychain, store, native or CanonicalDocument wiring. Independent bounded
source review is pending for this exact candidate. Owner acquisition,
atomic current authority, custody, possession, execution fencing, live paired
acceptance, final committee, pilot and rollback remain separate gates.
