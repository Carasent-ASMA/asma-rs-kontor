# Twenty-four-case analogue benchmark worksheet

> **Date:** 2026-10-03 01:11 Europe/Oslo
> **Status:** 🔴 Draft — labelled corpus and real embedding measurements pending
> **Category:** plan
> **Scope:** ASMA-8159 / REQ-018–019; no benchmark executed
> **Summary:** Freeze 24 cases against exact canonical identities, measure analogue quality, invalid exclusions and warm latency, and keep lexical degradation distinct from semantic success.

## When to load

Load at the authorized pinned-Cognee/copied-corpus qualification frontier.
Fixture identity/transport tests do not provide these quality measurements.

## Frozen inputs and labels

Seed families A, B and C are the evidence-backed admission/hold,
deployed-readback and evidence-versus-experience lessons in the census worksheet.
They are intended relevance themes, not existing approved records. Before a
benchmark run, replace these labels with exact approved project/item/revision/
content hashes; pin every distractor and invalid tuple too. Freeze the complete
canonical corpus manifest and its receipt cursor, Cognee dataset/digest/image/
release/commit, provider/model/embedding versions and configuration hashes.

Each case records authoritative task title, module, declared scope and selected
work-profile phase. Do not send hidden freeform query text. Titles below are
draft structured task inputs; module/scope/phase and relevance judgments must be
filled and hashed before execution. Independently label relevance 0–3 and the
full relevant identity set for each quality case; writers do not score their own
retrieval output or change labels after seeing it.

| Case | Class | Draft authoritative task title / controlled input | Required observation |
| --- | --- | --- | --- |
| B01 | Literal A | Verify revoked hold before default-allow admission | A in top five |
| B02 | Literal B | Verify deployed binary hash after restart | B in top five |
| B03 | Literal C | Classify operational evidence before memory proposal | C in top five |
| B04 | Paraphrase A | Keep a queued job stopped until its permission changes | A despite no literal hold/admission phrase |
| B05 | Paraphrase A | Explain why an inactive work item became runnable | A; relevant permission lesson |
| B06 | Paraphrase B | The new code landed but the running process stayed old | B; distinguish running identity |
| B07 | Paraphrase B | Prove which executable answered the canary | B; exact runtime evidence |
| B08 | Paraphrase C | A useful incident report crowded later task prompts | C; evidence storage boundary |
| B09 | Paraphrase C | Preserve a retrospective without recalling its progress log | C; distilled settled lesson |
| B10 | Lexical distractor A | Block unintended automatic execution | A outranks a typed but irrelevant deployment lesson sharing execution words |
| B11 | Lexical distractor B | Establish the running release's identity | B outranks a typed source-summary lesson sharing release words |
| B12 | Lexical distractor C | Choose a reusable lesson after an incident | C outranks a typed unrelated lesson with many incident keywords |
| B13 | Stale revision | High-ranked superseded revision, alongside current valid B | Old revision excluded; current B retained |
| B14 | Pending/unapproved | High-ranked proposed revision, alongside valid A | Pending revision excluded |
| B15 | Tombstoned | High-ranked tombstoned item, alongside valid C | Tombstoned identity excluded |
| B16 | Foreign project | Same lesson with a foreign project/item/revision | Foreign tuple excluded; no project leakage |
| B17 | Wrong kind | operational_gap/generic evidence tuple | Non-memory document excluded |
| B18 | Malformed typed document | Unknown field/enum or unresolved evidence identity; hostile returned prose canary | Malformed ineligible tuple/prose excluded |
| B19 | Hash mismatch | Correct ids with wrong content hash | Mismatched tuple excluded |
| B20 | Provider policy | Current approved local_only tuple from semantic output | Excluded from semantic projection/selection; local eligible FTS remains supported |
| B21 | Duplicate and ties | Repeated valid tuple plus tied observed/inferred lessons | Deduplicate before budgets; confidence then item-id tie order |
| B22 | Nonfinite score | NaN/infinite/invalid ranking shape | Typed malformed degradation; no invalid inclusion |
| B23 | Work/response bound | >64 candidates or >256 KiB response | Bounded malformed degradation; no unbounded parse/fallback |
| B24 | Empty/foreign-only | Empty then foreign-only candidate variants | Typed empty/no-eligible reason; eligible lexical_degraded or none |

All invalid cases use controlled fixtures/copies, never new bad live records.
Pin any synthetic invalid tuple separately from the real corpus. B18 must
distinguish rejection of typed-invalid canonical records from parser refusal of
malformed upstream identity. Upstream lesson text is never prompt authority.

## Measurement and acceptance

For each ranked quality case B01–B12:

    recall@5 = |unique returned top-5 relevant identities| / |all relevant identities|
    DCG@5 = sum((2^relevance(rank) - 1) / log2(rank + 1), rank = 1..5)
    nDCG@5 = DCG@5 / ideal_DCG@5

Macro-average the 12 predeclared nonempty quality cases. Report each numerator,
denominator, result list and relevance grade as well as aggregate scores.
Required semantic recall@5 ≥0.80 and nDCG@5 ≥0.75. Report invalid inclusions over
all 24 cases and all exclusion variants; required count is zero. Empty-relevance
negative variants report correct-empty/exclusion behavior separately, never
inflate quality metrics with an arbitrary perfect score. Report FTS-only
results side-by-side; lexical success earns no semantic analogue credit.

Measure complete local recall, including authoritative rehydration, budget and
binding freeze where applicable. Use the exact historical 55-item scale and
separately the current N-item migration scale. Warm the pinned index/connection,
then collect 20 independent new selections per case (480 timed runs per scale);
new run identities avoid benchmarking cached replay as retrieval. Preserve
every elapsed sample and p50/p95/p99/max. Required warm local result ≤2000 ms;
flag every overrun. Record semantic time, local/FTS time, failure mode and the
1500-ms semantic cap/500-ms lexical reserve. Separately measure replay and cold
startup, clearly labelled.

Record selected ordered item/revision/hash pairs, modes/reasons/exclusions,
exact canonical-array UTF-8 byte length/hash, projection/memory cursors, query
derivation hash, binding/result/context hashes and actual root prompt canary.
Check ≤8 items and ≤32768 bytes including JSON escaping/container overhead.
Keep ninth-item, oversized-top/next-fitting, Unicode 32768/32769, validation/
freeze race, restart/tombstone/task-edit replay and purge-refusal synthetic
regressions alongside the 24-case quality results; they do not replace it.

## Artifacts owed

cases.json (24 ids with structured intent and frozen labels), corpus-manifest.json,
invalid-fixtures.json, labels.sha256, projection-release.json, results.jsonl,
metrics.json, latency-samples.csv, prompt-canary.json and commands/results/log
hashes. These names describe future outputs; this package contains no fabricated
results. If a pinned endpoint cannot qualify, record the boundary and retain the
unchanged analogue/live Goal as outstanding.
