# Kontor — Documentation Index

> Canonical implementation-documentation root for the Kontor Rust control plane. Last updated: 2026-10-17 (routing and statuses checked against `master` at `0971674d`, PR #300, 2026-10-11).

## Overview

This documentation covers implementation-local knowledge for the `asma-rs-kontor` workspace. Platform-wide Kontor product principles, architecture, decisions and implementation plans remain in the [AI orchestration documentation](../../../_docs/ai-orchestration/index.md); this router contains audits, contracts and other documents whose scope is the Kontor repository implementation itself.

**Start here for *why*:** [Kontor governing principles](../../../_docs/ai-orchestration/architecture/2026-08-26-11-30-architecture-kontor-governing-principles.md) — Autonomy and Delivery Quality, and the fourteen principles every rule in this repository serves. They define thin as a narrow authority boundary, require a rich mission-control projection, prefer reuse over duplication and make human escalation a prepared last resort. The repository-local restatement, readable without the parent checkout, is [`ARCHITECTURE.md`](../ARCHITECTURE.md).

**Merged is not deployed.** Statuses below say *merged* for source on `master`; qualification and deployment are claimed only where a dated report records them.

## Documents by Category

### Reference (`reference/`)

| Document | Summary | Status | When to Load |
| --- | --- | --- | --- |
| [Recommended teams and seats](reference/2026-09-04-23-35-reference-recommended-teams-and-seats.md) | Role responsibilities, fleet-design recommendations and explicit implementation limits | Recommendation; implementation limits re-checked at `0971674d` | Designing teams or evaluating models for roles |
| [Durable desks in kontord](reference/2026-10-10-23-55-reference-kontord-durable-desks.md) | ASMA-8450: epic-independent `DESK • ADAM` / `DESK • PR REVIEW` projects, their supported operations and limits, explicit successor selection and rollback | Merged (PR #295, `733807cc`); atomic fresh ensure and refusal exits in PR #299 (`813135fd`), desk-directory seat MCP in PR #300 (`0971674d`) | Ensuring, materializing or reading back a desk; selecting or rolling back a desk-capable Team Definition |
| [Durable desk seats in kontord](reference/2026-10-11-02-20-reference-kontord-durable-desk-seats.md) | ASMA-8451: dated `paseo_direct` / `daemon` seat declarations (`LSA • GENERAL`), the seat report, missing-only reconciliation with its replay fence, support limits and the operator runbook | Merged (PR #296, `430975a3`) | Declaring a desk seat; reading or reconciling a desk's seats; deciding whether Kontor may restart one |
| [Desk Adams in kontord](reference/2026-10-11-05-36-reference-kontord-desk-adams.md) | ASMA-8452: epic and ad-hoc Adam subjects in the `ADAM` desk workspace, role-led titles, cross-vendor LSA→TPM handoff, ad-hoc origin binding on `epics:apply`, desk-project placement with no epic label, no-ECP delivery and the support limits | Merged (PR #297, `b3db4adf`) | Placing, handing off or binding a desk Adam; declaring `desk_leadership` |
| [PR reviews in kontord](reference/2026-10-11-11-35-reference-kontord-pr-reviews.md) | ASMA-8453: one Adam tab (`TPM • <key> • <repo>#<n>`) and one review room (`RSW • …`, `REV A` / `REV B` / `JUDGE`) per pull request on the PR desk, identified by repository and number and authorized by the confirmed Jira key's publication binding; open, close with desk and sibling isolation, readback, and the support limits | Merged (PR #298, `e376f405`) | Opening, closing or reading a PR review; declaring `pr_review` |

### Plans (`plans/`)

| Document | Summary | Status | When to Load |
|----------|---------|--------|-------------|
| [`2026-09-01-21-45-plan-configuration-driven-native-naming-live-migration.md`](plans/2026-09-01-21-45-plan-configuration-driven-native-naming-live-migration.md) | Implements ASMA-8062: immutable pinned Team Definition naming/hierarchy, deterministic ASW/CSW topics and seats, identity-preserving migration, exact-master deployment and live readback. | 📚 KBI migration verified 2026-09-02; historical execution plan | Auditing the migration receipt or planning a separate epic upgrade |
| [`2026-09-25-06-50-plan-kontor-fast-verification.md`](plans/2026-09-25-06-50-plan-kontor-fast-verification.md) | Implements ASMA-8258: SQLite build flags (memstatus off, optimised), desktop crate out of the default gate, nextest lanes with per-test timings, a shared on-disk migrated template, one integration binary per crate, injectable timeouts, machine hygiene for parallel seats, and a decision-gated move of the full gate to one run per candidate commit. Measured 2026-09-25: `schema_v1` 198 s → 7.7 s from the two flags alone. | 🔵 Planned — Paseo direct, one team, Kontor bypassed by operator decision. None of its deliverables (FV-01…FV-10), including the SQLite build flags and the nextest lanes, is on `master` at `0971674d`; PR #269 (`65fe8af8`: test lanes, stable lock check, migrated store template) predates the plan and is its baseline | Changing how Kontor tests are built, selected, run or timed; editing the store/daemon test harness; revisiting the local-verification policy |
| [`2026-09-01-21-45-open-questions-configuration-driven-native-naming.md`](plans/2026-09-01-21-45-open-questions-configuration-driven-native-naming.md) | Four original naming questions and their dispositions. | 📚 Historical — closed | Tracing the approved aggregate boundary and naming agreement |

### Architecture (`architecture/`)

| Document | Summary | Status | When to Load |
|----------|---------|--------|-------------|
| [`2026-09-01-13-25-architecture-kontor-local-verification-policy.md`](architecture/2026-09-01-13-25-architecture-kontor-local-verification-policy.md) | Explicitly disables GitHub Actions CI for Kontor and makes recorded, exact-commit local gates the merge-verification policy; reversal requires a new explicit decision. The one workflow on `master`, the publication branch and title check, runs no build, test or lint gate. | 🟢 Approved | Configuring Kontor CI, deciding merge evidence, or considering GitHub Actions for this repository |
| [`2026-09-24-20-45-architecture-live-fleet-routing.md`](architecture/2026-09-24-20-45-architecture-live-fleet-routing.md) | ASMA-8255: delivery, Committee and Advisor placement read one owner-only `<state-root>/fleet.yml` at placement time instead of the frozen template route; last-valid snapshot, vendor-keyed reviewer independence, file receipts. Supersedes template-snapshot immutability for routing only. | 🟢 Approved; implemented in PR #271 (`7ffd650d`); extended by the activated fleet policy of PR #292 (`4097afb8`, ASMA-8280) | Changing how a seat chooses its model route; editing `fleet.yml` or a published fleet policy; reviewing why a seat did not use its template route |

### Audits (`audits/`)

| Document | Summary | Status | When to Load |
|----------|---------|--------|-------------|
| [`2026-08-20-18-53-audit-kontor-rust-workspace-code-quality.md`](audits/2026-08-20-18-53-audit-kontor-rust-workspace-code-quality.md) | In-depth snapshot audit of the 240k-line Kontor Rust workspace: source/test composition, file and function size, DRY/YAGNI/SOLID, MCP-tool legitimacy, safety, dependencies, CI gaps, and prioritized remediation. Its 127-tool / 16-worker counts are explicitly historical; current inventory is routed to the repository contract. | 🟤 Point-in-time snapshot | Assessing the audited commit's maintainability or its original findings; use current source for today's counts |
| [`2026-09-01-22-19-audit-asma-8062-team-definition-v77.md`](audits/2026-09-01-22-19-audit-asma-8062-team-definition-v77.md) | Point-in-time v77 independent audit; later KBI migration resolution is in the governing receipt. | 📚 Historical audit | Reviewing that candidate and its follow-up evidence |

### Reports (`reports/`)

Reports are dated, point-in-time records; their bodies are not rewritten. The Status column gives the report's own state and then what later merged. Raw evidence the reports link lives under `reports/evidence/<Jira key>/`; that tree is historical, being removed under ASMA-8511, and takes no new files.

| Document | Summary | Status | When to Load |
| --- | --- | --- | --- |
| [ASMA-8454 desk MCP repair rework](reports/2026-10-11-22-03-report-asma-8454-desk-mcp-rework.md) | AUD B1 retained-directory containment, healthy native read-only replay, 509 focused tests and 23/23 mutants | Report: frozen rework candidate. Later merged in PR #300 (`0971674d`); new-head QA/AUD and completion-committee outcomes not recorded here | Reviewing the merged hosted desk MCP composition or preparing its released rehearsal |
| [ASMA-8454 hosted desk MCP repair](reports/2026-10-11-21-25-report-asma-8454-desk-seat-mcp-repair.md) | Enabled Claude composition in verified desk directories; atomic publication, rollback/recovery and exact-source mutation evidence. | Historical carrier — AUD B1 rejected at `255d8c93`; superseded by the rework record above | Tracing the original repair source, tests and mutations behind PR #300 |
| [ASMA-8454 durable desk qualification](reports/2026-10-11-16-00-report-asma-8454-desk-qualification.md) | Integrated four-structure acceptance, two authorized repairs, restore/replay, pinned compatibility, whole-change mutations and coordinator release steps. | Report: frozen source candidate. Later merged in PR #299 (`813135fd`); Independent Review verdict not recorded here | Reviewing the ASMA-8454 source qualification or preparing its released rehearsal |
| [ASMA-8114 legacy consultation read](reports/2026-09-26-18-22-report-asma-8114-legacy-consultation-read-kontor-operational-gap.md) | Legacy Committee/Advisor GETs returned 409 on a missing subject; the read DTO now omits the unprovable name while native effects keep strict subject enforcement. | Report: integrated source qualified. Later merged in PR #279 (`173d399b`); the three historical GET readbacks are not recorded here | Recovering reads of the settled legacy consultations or auditing the correction |
| [ASMA-8015 stopped-realm Jira credential](reports/2026-09-26-17-20-report-asma-8015-stopped-realm-jira-credential-kontor-operational-gap.md) | `install-jira-credential` for a stopped realm: stdin-only document, realm/alias-scoped Keychain address, verified compensation, mutation and audit history. | Report: corrected source qualified, live delivery pending. Later merged in PR #278 (`f95e2065`); credential enrollment not recorded here | Installing or auditing native Jira connector credentials |
| [ASMA-8196 abandoned bridge lineage](reports/2026-09-26-11-08-report-asma-8196-abandoned-bridge-lineage-kontor-operational-gap.md) | An abandoned never-bound replacement bridge made a cancelled predecessor look current; selector, census and TeamRun hydration now follow the preserved ancestry. | Report: deployed lineage repair verified; census reconciliation and workflow acceptance pending. Fix merged in PR #276 (`e29b895d`), evidence in PR #277 (`eef31733`) | Auditing replacement-chain selection or a naming census |
| [ASMA-8121 export event-kind gap](reports/2026-09-26-08-28-report-asma-8121-export-event-kind-kontor-operational-gap.md) | Deployed export failure on command-intent fields, exact snapshot/restore identity proof and ASMA-8049 MUT-003 qualification. | Report: export gap resolved; ASMA-8049 census and closeout pending. Fix merged in PR #274 (`dddb7287`), continuity proof in PR #275 (`f4d90f78`) | Qualifying backup/export continuity or ASMA-8049 closeout |
| [ASMA-8196 prepared-intent recovery](reports/2026-09-26-05-06-report-asma-8196-prepared-intent-recovery.md) | Real crash-boundary regression, same-native rematerialization, frozen-authority refusal, independent QA finding and preserved worktree-recovery evidence. | Report: source qualified, merge/deployment pending. Later merged in PR #273 (`4c1c52cb`); live persona/attachment readback and epic acceptance not recorded here. PRs #284 (`d5a4cdff`) and #286 (`6e0b3557`) later added the read-only `kontor_epic_core_team_get` and `kontor_epic_core_team_seat_occupancies_get` | Auditing the ASMA-8196 recovery or the safe-carry incident; reading a seat's persona and occupancy chain |
| [ASMA-8255 implementer handoff](reports/2026-09-25-06-44-report-asma-8255-implementer-handoff-to-verification.md) | LF-01…LF-08 live `fleet.yml` routing delivered for verification: commits, mutation pass, gate results and the lead's TASK-036…039 steps. | Report: ready for verification, nothing merged. Later merged in PR #271 (`7ffd650d`); live `fleet.yml` install and drills D-1…D-8 not recorded here | Auditing what the ASMA-8255 implementer delivered; see the [live fleet routing decision](architecture/2026-09-24-20-45-architecture-live-fleet-routing.md) |

### Operations (`operations/`)

| Document | Summary | Status | When to Load |
| --- | --- | --- | --- |
| [Durable desk qualification runbook](operations/2026-10-11-15-28-runbook-durable-desk-qualification.md) | Disposable integrated restore/replay, declaration comparison, mutations, CLI refusal exit codes and coordinator release/readback steps. | Merged (PR #299, `813135fd`); release steps 3–8 not recorded here | Reproducing ASMA-8454 or preparing its evidence-gated release |

### Contracts (root)

| Document | Summary | Status | When to Load |
|----------|---------|--------|-------------|
| [ASMA-7869 committee remediation gate](ASMA-7869-committee-remediation-gate.md) | Architecture boundary for non-destructive Committee failure remediation and clean re-review: how a failed decision is ingested as durable evidence, why advancing the same run to an internal second round poisons it, how a legacy advanced run is reconstructed without rewriting it, and what independently authorized remediation must produce before a separate clean re-review may be consumed. Explicitly not a waiver, a verdict-fabrication path, an immutable-row repair procedure, or permission to reuse a poisoned run. | 🟢 Boundary in force | Changing Committee round handling, remediation authority or completion re-review; recovering a run that advanced prematurely; reviewing anything that touches immutable findings |

## Repository-level documents outside this router

| Document | Scope |
|----------|-------|
| [`../README.md`](../README.md) | What Kontor is and is not, what is built and what is deliberately not, build and run |
| [`../ARCHITECTURE.md`](../ARCHITECTURE.md) | Principles, authority boundaries, consistency model, completion machinery, tool registry, security and extension rules |
| [`../CHANGELOG.md`](../CHANGELOG.md) | Dated and versioned record of notable changes |
| [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | Branch, design-rule, test-evidence and pull-request procedure, including the required local gates |
| [`../docs/RECOMMENDED-TEAMS-AND-SEATS.md`](../docs/RECOMMENDED-TEAMS-AND-SEATS.md) | The original 2026-08-29 recommended-teams text, on `master` since `fd204794` (2026-08-29). The [reference copy](reference/2026-09-04-23-35-reference-recommended-teams-and-seats.md) is the 2026-09-04 recovery of the same document with an explicit implementation boundary; the two copies are maintained separately and differ |
| [`../docs/CONFIGURATION.md`](../docs/CONFIGURATION.md) | Where configuration lives, experience-memory transport, Jira-derived names and reconciliation, seat supervision and permission posture, live `fleet.yml` and activated fleet policy, quota signals, seat MCP surface and serve profiles, GitHub App publication identity |
| [`../docs/CONSULTATION_LIFECYCLE.md`](../docs/CONSULTATION_LIFECYCLE.md) | Consultation seat permissions and guard, idle-session release, governed planning pairs |
| [`../docs/NATIVE_NAMING.md`](../docs/NATIVE_NAMING.md) | Current pinned-definition naming contract, branch and worktree naming, desk, desk-seat, desk-Adam and PR-review titles, and legacy-import correction |
| [Canonical implementation inventory](https://github.com/Carasent-ASMA/asma-modules/blob/master/_docs/ai-orchestration/reference/2026-09-05-11-36-reference-kontor-implementation-inventory.md) | Current revision-stamped schema, registry, merge and deployment distinctions |
| [`../RECOVERY.md`](../RECOVERY.md) | Offline `kontor-daemon` runbook: snapshot, restore, export, import, credential rotation and stopped-realm Jira credential install (`install-jira-credential`, PR #278); upgrade and binary rollback (snapshot and restore accept only the running build's exact schema, so a rollback stops the daemon, puts the old binary back, moves the migrated `kontor.db` and its `-wal`/`-shm` aside and restores the pre-upgrade snapshot with the old build); and pointers to live control-plane recovery, which a serving daemon exposes as tools rather than subcommands |
| [`../SECURITY.md`](../SECURITY.md) | Reporting and supported-version policy |
| [`../docs/QUOTA-FALLBACK-PLAN.md`](../docs/QUOTA-FALLBACK-PLAN.md) | Provider quota routing behind the rung walk; the document carries its own dated status |
| [`../docs/evidence/`](../docs/evidence/README.md) | Per-ticket delivery evidence (`ASMA-*`, `KON-*`, `KBI-*`, `KCADR`): release notes, QA, review, mutation and closeout records. Historical, being removed under ASMA-8511; add no new files. The KON-MVP-18 e2e pilot inside `cargo test --workspace` still writes `docs/evidence/KON-MVP-18/<run-id>/` on every run; delete it and never commit it until ASMA-8511 moves that output |

## Folder Legend

| Folder | Contents |
|--------|----------|
| `architecture/` | Living repository-local design and engineering-policy decisions |
| `audits/` | Point-in-time code-quality, completeness, security, and compliance audits of the Kontor implementation |
| `operations/` | Living runbooks for operating or qualifying a Kontor capability |
| `plans/` | Active implementation plans; compacted into history at closeout |
| `reference/` | Living contracts for a merged capability: supported operations, refusals, support limits and rollback |
| `reports/` | Dated, point-in-time delivery and qualification reports; raw evidence under `reports/evidence/<Jira key>/`, historical, being removed under ASMA-8511, add no new files |

## Related authoritative documentation

- [AI Orchestration documentation index](../../../_docs/ai-orchestration/index.md) — product principles, architecture, plans, operational reports and historical decisions governing Kontor.
- [Kontor governing principles](../../../_docs/ai-orchestration/architecture/2026-08-26-11-30-architecture-kontor-governing-principles.md) — Autonomy and Delivery Quality, reuse boundaries, mission-control UI and last-resort human escalation; read before proposing any Kontor change.
- [Kontor control-plane architecture](../../../_docs/ai-orchestration/architecture/2026-08-08-20-12-architecture-asma-kontor-control-plane.md) — the authoritative platform-wide *how*.
- [Kontor codes and terms](../../../_docs/ai-orchestration/reference/2026-08-16-13-02-reference-kontor-codes-and-terms.md) — canonical Jira identities, epic backlog namespaces, derived item codes and topology naming revisions.
- [`ARCHITECTURE.md`](../ARCHITECTURE.md) — repository-local architecture overview.
- [`README.md`](../README.md) — build, usage, and current repository overview.
