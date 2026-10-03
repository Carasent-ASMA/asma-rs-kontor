# ASMA-8278 verification-only attestation source evidence

This bundle pins runtime source candidate `c708701b6b8f80ef4db57b0b0410976dfd4dc482`
and its specification at root `b84af82c976faaf02791ad5e04b38de570806441`.
It records a non-authorizing verifier, with no production caller or native effect.

[MANIFEST.json](MANIFEST.json) maps all five source hashes, portable evidence
files, checks, mutation results and remaining gates. The original worker
manifests retain their pre-commit base label and historical local paths;
[PUBLISHED-CANDIDATE-READBACK.json](PUBLISHED-CANDIDATE-READBACK.json) explains
the exact published-source mapping. Original reports and logs are copied unchanged.
The folder's `.gitattributes` permits generated blank lines at the end of raw
logs so their exact bytes remain intact; all other whitespace checks still apply.

The [full runtime suite](runtime-full-suite.log) passes 122 unit, 20 integration
and two compile-fail privacy checks. Fifteen unit tests are new signed-fixture
attestation cases. All four changed-boundary mutants fail by assertion; before
and after baselines pass and source hashes are restored. Formatting and scoped
Clippy passed as recorded in [worker validation](SOURCE-VALIDATION.worker.json).

The [independent verifier findings](review-attestation-source.md) find no
substantiated blocker to this source slice. Their A-04 log limitation is
preserved as the review-time observation; root supplied the full-suite log
subsequently. The [earlier combined findings](review-frozen-source.md),
[R-01 followup](review-r01-first-followup.md) and separate
[R-01 correction](review-r01-correction.md) preserve the original investigation.
R-01 is premise-refuted: pair-member consultation generations do not belong to
hosted Core Team history. Physical possession/liveness remains a separate gap.

A public-key verification result grants no current authority. Issuer custody,
authentic fresh key/revocation distribution, active/current occupancy,
native-session possession, atomic state/receipt and native-effect fencing,
positive finality/drain and enforced restrictions still need owner integration
and live qualification. This bounded source review does not satisfy the final
independent committee, all paired cases, deployment, selected pilot or rollback.
