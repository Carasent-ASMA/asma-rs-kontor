# Advisor verdict — Desk seat MCP composition (released HOLD, ASMA-8454)

- Advisor seat: LSA • ASMA-8449 • Desk seat MCP composition (native a95460a9-afcd-468a-8236-b80be398c42b)
- ASW: wks_a588fb37c1273a17 (ESW prj_dcbdc492d52f77d5); premium rung sol@xhigh; read-only
- Received 2026-10-11 (delivered to coordinator); verified both supplied SHAs (finding 05f6e4b3…, request/response c5b0e2db…)

## Disposition

HOLD the released acceptance; repair the supported hosted-directory composition path. Integration defect in the approved desk outcome, not evidence Claude is unsupported. No new Igor scope decision for the repair.

## Per-option verdict

1. Compose in the existing non-Git desk cwd — RECOMMENDED (explicit placement eligibility, bounded failure/recovery, shared-cwd serialization).
2. Creation-frame MCP only — credible alternative but not acceptance-ready (provider-side facts unproven); merely skipping compose is not a repair.
3. Put files elsewhere — conditional (only an already-supported explicit config-path mechanism; per-seat cwd/extra workspace/provider-home override/new harness mechanism = architecture boundary → Igor).
4. Initialize Git in the desk — reject as default (desk-shape change; Igor decision).
5. Restrict desk Adams to non-Claude — reject (scope reduction; Igor decision).

## Exact seam

Explicit owned-directory composition mode inside seat_mcp.rs, selected through compose_for_hosted_seat at adapter.rs:6162 after the :6075-6099 checks; eligibility from HostedSeatScope::Desk + proved directory placement (never inferred from names/paths). Keep the worktree requirement for delivery/consultation/planning-pair callers. Real Git cwd unchanged; genuinely owned non-Git cwd needs no exclude; Git operational errors and wrong placement remain refusals. The branch also serves durable desk seats, ad-hoc Adams, PR Adams and PR rooms. Reuse current JSON merge content and leadership frame; secrets only in the creation secret channel; two subjects sharing ADAM get authority from bindings, never shared-file credentials.

## Required write contract

- Serialize composition by canonical cwd (adapter.rs:6920 guard is per seat, insufficient).
- Read, validate, render BOTH JSON objects before modifying; preserve unrelated content; refuse malformed/escaping paths.
- Stage beside destinations; atomic replace per file; skip identical bytes; settings before .mcp.json (final publication point).
- On handled publication failure restore exact previous bytes/absence; delete only this attempt's temp files/empty dir; never delete shared files on later native-create failure.
- Preserve prepared intent; create only after complete publication/readback; rollback failure reports incomplete cleanup and blocks launch (no "nothing changed").
- Crash-safe pair atomicity cannot be promised by write ordering alone; use prepared-intent recovery plus, where needed, a daemon-owned publication record/originals to finish/restore an interrupted pair before further create/reload.

## Lifecycle coverage

Fresh launch, missing durable-seat replacement, cross-vendor handoff, route correction, ad-hoc launch, PR open. Recovery of exact native bypasses :6162; reload at :6227 must reconcile pending publication and ensure complete config. retitle_seat (:9174), claim_hosted_seat (:6941), restore_bindings (:6573) do not compose — re-test retained identity/scope/no file change; do not add composition to metadata-only claim/retitle; if restore leads to relaunch, that boundary rebuilds/validates composition. Keep the repair in the hosted-directory branch (global merge_json change would widen review).

## Igor gate

NO for this bounded repair and its regression/qualification work. YES for a Git desk, vendor exclusion, changed cwd/container shape, new credential authority mechanism, or a newly discovered cross-boundary architectural change. Build/merge/release gates stay as recorded.

## Targeted review

Configured scope/architecture + implementation reviewers, QA and AUD. Architecture: unchanged desk membership/subject authority/epic ledger. Implementation: directory eligibility, publication/rollback/recovery, cwd lock. QA: real filesystem composition with seat_mcp Some, failure injection, lifecycle/replay. AUD: no secrets on disk, no ambient widening, path containment, cross-subject isolation. Then reuse the completion CSW's two reviewers (contrasting vendors) + JUDGE on the new frozen candidate plus release evidence; second-rejection escalation and exact-head gates retained.

## Released rehearsal must re-prove

1. Freeze repaired source/build hashes, installed/executing daemon identity, companion MCP identity; retain the original HOLD package.
2. Released production adapter with composition ENABLED and the failing Claude route in an isolated genuine directory fixture; read back both JSON files and the frame's leadership server/tool policy/secret-channel separation. RecordedPaseo with seat_mcp None or a Codex fixture is insufficient.
3. Epic LSA + supported TPM placement in DESK ADAM/ADAM, no ECP, exact ids/session/title/kind/cwd, SeatBinding + occupancy generation; labels and epic ledger as approved.
4. All four structures incl. a Claude hosted seat in the PR path, two epic subjects sharing ADAM, two PRs for one key; separate subjects/credentials/evidence; unchanged topology.
5. Replay/restart/recovery convergence; inject invalid second JSON, staged/publication failures, rollback failure, interruption between publications; no native creation on incomplete composition; no duplicate on lost acknowledgement.
6. Normal Git composition/exclude idempotence unchanged; unrelated JSON preserved; account-qualified Claude routes; foreign/disabled provider behavior retained.
7. Backup/restore, selection/rollback, partial PR close/retry, owned fixture cleanup; compare old bytes/pins/native identities; no real durable seats; Reports only after full released evidence.

Confidence: high on cause and no-scope-change disposition; moderate on the publication implementation until reviewed and failure-tested. Counter-argument: the creation frame might make cwd files redundant — unproven; the owned-directory repair is the safer minimal disposition.
