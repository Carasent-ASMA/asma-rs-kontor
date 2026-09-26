# ASMA-8114 legacy consultation archival-read correction

> **Date:** 2026-09-26 18:22 Europe/Oslo
> **Status:** 🟠 Source candidate — five focused checks pass; exact qualification and deployment pending
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

The [executed focused runner](evidence/ASMA-8114/legacy-consultation-read/focused-runner.py) used disposable synthetic Realms and a private APFS-cloned cache. The [ten-artifact manifest](evidence/ASMA-8114/legacy-consultation-read/SHA256SUMS.json) binds exact receipts and lossless log carriers. Decode `rawBase64` strictly without normalization.

## Remaining qualification and delivery

Exact committed-source compiled behavioral mutations, the full archive gate and independent corrective source review remain pending. Running work receives no credit. Merge, build/deployment, the three actual supported historical GET readbacks, broader ASMA-8114 operational gaps and task/workflow/epic acceptance remain separate. No SQL subject backfill, topic correction, native retirement or historical-result rewrite is part of this remedy. ASMA-8015 credential-source edits remain held under their separate approval boundary.
