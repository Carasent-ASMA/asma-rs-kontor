# ASMA-8114 legacy consultation archival-read correction

> **Date:** 2026-09-26 18:22 Europe/Oslo
> **Status:** 🟠 Source candidate — three exact-source mutants killed; full gate, API contract and deployment pending
> **Category:** report
> **Scope:** ASMA-8113 / ASMA-8114 archival Committee and Advisor reads
> **Summary:** Omit an unprovable canonical name from a legacy read DTO while preserving strict native subject enforcement and every historical finding.

## When to Load

Load when qualifying this source candidate or recovering supported reads of the three settled legacy consultations. This correction supplies no task/workflow, epic or portfolio acceptance.

## Cause and bounded correction

Installed `e29b895d0f045c499144870a2fa610fc642b6cc0` returns 409 for three historical Committee GETs because optional name projection invokes strict subject authority before reading findings. The [full actual independent diagnosis](evidence/ASMA-8114/legacy-consultation-read/actual-lsa-legacy-read-diagnosis.txt.json), with its [unchanged native receipt](evidence/ASMA-8114/legacy-consultation-read/lsa-diagnosis-publication-receipt.json), identifies P1 archival-read enforcement and P2 missing read-contract coverage. Original observations, nine findings and native discrepancies are durably preserved in [superproject checkpoint 132f9d0f](https://github.com/Carasent-ASMA/asma-modules/blob/132f9d0f96def235f51be5d16ca947e0c5ab69bc/_docs/ai-orchestration/reports/2026-09-26-18-05-report-asma-8114-legacy-consultation-read-kontor-operational-gap.md).

`consultation_container_name` now returns `None` for a missing legacy subject. That helper serves the Committee and Advisor read DTOs. It does not infer an epic, backfill a subject or rewrite a title. `subject_task_for_container` and native effect paths retain their exact strict implementation.

The isolated implementation base is `eef31733cd987986f93a763aa98ad00d22a869fc`, documentation-only over installed e29. The [source proof](evidence/ASMA-8114/legacy-consultation-read/source-and-evidence-boundary.json) records that relationship, exact source/test hashes, the unchanged strict renderer and the bounded worktree base adjustment. No installed source or credential was changed.

## Actual focused verification

The initial [fixture attempt](evidence/ASMA-8114/legacy-consultation-read/red-before-correction.txt.json) and [receipt](evidence/ASMA-8114/legacy-consultation-read/red-before-correction.json) remain preserved. An Advisor fixture supplied only one of its required advisors, and the Committee fixture retained an older naming definition. That run earns no qualification credit.

After correcting only those fixtures, [both regressions](evidence/ASMA-8114/legacy-consultation-read/red-current-naming-fixtures.txt.json) compiled and failed the archival GET assertion with the exact production 409; [exit 101 and original source hash](evidence/ASMA-8114/legacy-consultation-read/red-current-naming-fixtures.json) are recorded. No source correction was present in that run.

After the one read-projection guard changed, [five focused checks passed](evidence/ASMA-8114/legacy-consultation-read/green-after-correction.txt.json), zero failed or ignored, with [all four commands exiting 0](evidence/ASMA-8114/legacy-consultation-read/green-after-correction.json):

- Settled Committee GET keeps the actual non-compliant result and all three findings, with topic, run/node/binding/native identities and revisions exact.
- Settled Advisor GET keeps every independently recorded advice and its disposition.
- Both legacy reads omit an unprovable name and expose no invented subject evidence; every persistent row is identical before/after and the scripted runtime receives no call.
- Unknown-subject native-name preview still returns its strict 409 before effects.
- Existing Committee and Advisor tests still render exact task/epic canonical names.

The [executed focused runner](evidence/ASMA-8114/legacy-consultation-read/focused-runner.py) used disposable synthetic Realms and a private APFS-cloned cache. The [27-artifact manifest](evidence/ASMA-8114/legacy-consultation-read/SHA256SUMS.json) binds exact receipts and lossless log carriers. Decode `rawBase64` strictly without normalization.

## Remaining qualification and delivery

The [complete actual independent source audit](evidence/ASMA-8114/legacy-consultation-read/actual-lsa-source-audit-51cfef9c.txt.json) and [adjacent receipt](evidence/ASMA-8114/legacy-consultation-read/lsa-source-audit-51cfef9c-publication-receipt.json) preserve activity update 3066 exactly: no P0 or functional P1, with one adjacent P2. Both Advisor and Committee GET OpenAPI declarations must advertise their possible 409 for inconsistent subjectful naming state. The audit supports the bounded design conditionally and credits no running qualification work.

The [exact-51 three-mutant receipt](evidence/ASMA-8114/legacy-consultation-read/results.json), [carrier bindings](evidence/ASMA-8114/legacy-consultation-read/mutation-publication-bindings.json) and [executed runner](evidence/ASMA-8114/legacy-consultation-read/mutant-runner.py) record three compiled behavioral kills with exit 101: removing the legacy read guard restores the production 409; reversing it loses a canonical name; weakening strict subject enforcement admits a prohibited native-name preview. [Baseline](evidence/ASMA-8114/legacy-consultation-read/baseline.txt.json) and [restoration](evidence/ASMA-8114/legacy-consultation-read/restored.txt.json) each passed all five focused tests. [Before](evidence/ASMA-8114/legacy-consultation-read/source-entries-before.json) and [after](evidence/ASMA-8114/legacy-consultation-read/source-entries-after.json) inventories are byte-identical across all 2,759 entries. Source commit `51cfef9caaa1e2b12674ad932d0adfb0658d4777`, tree `c63a37e6d0b0db8e6cfb495d044e02d33f62a698`, archive `a4a661119a5f5e5c6fa211a75397cd72c5b5f5a7e45bc4d84816b0fc96f7cebe` bind this qualification only.

The first [full-gate attempt](evidence/ASMA-8114/legacy-consultation-read/full-gate-sandbox-dns-failure.txt.json) and [exit-1 receipt](evidence/ASMA-8114/legacy-consultation-read/full-gate-sandbox-dns-failure-receipt.json) remain immutable. Sandbox DNS refused `github.com` during the Swagger UI dependency build; that run earns no credit. The same-source network-enabled full archive gate is running and earns no completion credit. The API-contract correction and its subsequent frozen/API qualification remain pending. Merge, build/deployment, the three actual supported historical GET readbacks, broader ASMA-8114 operational gaps and task/workflow/epic acceptance remain separate. No SQL subject backfill, topic correction, native retirement or historical-result rewrite is part of this remedy. ASMA-8015 credential-source edits remain held under their separate approval boundary.
