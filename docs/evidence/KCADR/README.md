# ASMA-8098 compatibility recovery evidence

Status: updated implementation is in progress. The archived release qualification below belongs to historical candidate `4c51059e07a9517d57cb9bd2665dc537ff681a2c`; it does not qualify the newer source. Final integrated release checks, required Kontor role gates, disposable live qualification and fleet deployment remain pending. This is implementation evidence, not a deployment receipt.

## Scope and identities

- Epic: ASMA-8098 / `01a06e13-878a-7aa3-8a08-bfad07cc8c4c` / backlog code `KCADR`.
- Compatibility: ASMA-8099 / `01a06e13-878b-77e1-885e-7187ffc97b6d`.
- Documentation: ASMA-8100 / `01a06e13-878c-7080-9801-d984bfe0eb04`.
- Realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`, project `01a0064a-e056-7603-9968-ef64fdaacb75`.
- Exact bindings and unwaivable gates: [admission readback](admission-readback.json).

The later user approval delegates PR #167 source completion and merge to one receiving session and authorizes independent reviewers; the parent session owns documentation, live qualification, deployment and closure. This supersedes the earlier no-delegation default. Both tasks are blocked at revision 2 to prevent additional automatic admission while release prerequisites remain open; see [the durable receipts and readback](release-block-readback.json). This change does not claim the fleet-verifier, fleet-spec-auditor, inspector or architect verdicts that the selected profiles require.

## Implementation

- Optional server permissions preserve old-daemon compatibility when omitted. Explicit permissions restrict the final composed capability surface, including MCP retitling fallback. Denied preparation and retitling leave runtime mutation counts unchanged.
- The WebSocket hello negotiates timeline replacement invalidation. Replacements are correlated by native agent ID and retained independently of the bounded content queue. A replacement requires canonical cursor-free refetch; old rows are discarded.
- New current Claude catalog choices use `claude-fable-5-1`. Frozen `claude-fable-5` routes still pass registry recovery for all three Claude account aliases. Existing sessions, definitions and historical evidence are not rewritten.
- Governance paragraphs and the team-design blueprint were recovered with current implementation boundaries, read-only consultation responsibilities and template-owned committee composition.

Source commits: `7381029` (adapter), `bb4ac51` (model catalog), `536b932` (repository guidance). Candidate `4c51059e07a9517d57cb9bd2665dc537ff681a2c` integrates naming owner PR #166 at `ac7e788243050714d9722733cc6548d781e387d9`. Root documentation is in [asma-modules PR #2981](https://github.com/Carasent-ASMA/asma-modules/pull/2981); implementation is in [Kontor PR #167](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/167). The branch has since integrated OP-21/OP-22 and changed runtime, admission and Jira mapping source. Those inputs differ from the historical qualified archive.

## Current recovery candidate

- `76a8db559bb20f87554b8b7d2d865d47b89645d9` adds task workflow specification v3 for frozen `docs@1`, without replacing the code/high mappings or changing task profile pins. The real mapping regression failed before correction; the full Jira suite then passed (1 unit + 14 contract tests). See [red](docs-workflow-red.log) and [green](docs-workflow-green.log).
- `1d430977a7fb59877af3cd1f53718668e2587b0d` preserves a partially admitted epic's frozen Team Definition and permits a structurally legal historical project-root boundary within the same topology lineage. The focused admission regression and all 8 store topology contracts passed; foreign project, terminal root, unrelated lineage and within-epic snapshot mismatches remain refused. See [admission](admission-green.log) and [root boundary](root-boundary-green.log).
- `bbc299d` enforces current session permission declarations on bound operations, and restores pending permission state during canonical timeline refetch. The full adapter suite passed (108 unit + 190 contract tests, 6 live tests excluded); two additional deliberate defects were killed. See [adapter](adapter-review-green.log), [mutations](review-mutants.json), and the [independent repository review](independent-paseo-review.json). That approval is scoped to permission, timeline and model source; it does not stand in for required Kontor workflow verdicts.
- Native cleanup uses the existing seat-retire and topology archive operations. An idle hosted native must be archived/read back before its logical seat is released. A retired local child requires exact binding, parent host and directory checks, no reported native work, and fresh absence after archive. A shared native lifecycle guard excludes concurrent placement, messages and migration. Lost acknowledgements and exact-key retries preserve the original identities.
- The existing topology inspection and mutation projections now return stored node/seat `revision` values. Qualification reads `GET /v1/projects/{project_id}/topology:inspect`, then uses the exact returned revision in each retirement/archive request.
- `ae373f5ab7455b3ad6e65530230cc7beb4c9f57e` makes first-proposal memory history readable before approval. The regression reproduced the null-to-boolean SQL decode failure; all 10 memory tests pass after the null-safe read predicate. Immutable revision contents and approval rules are unchanged. See [red](memory-history-red.log) and [green](memory-history-green.log).
- Final integrated release qualification and deployment remain pending. A stopped exploratory daemon run is not qualification of the new candidate.

The cleanup, admission, docs mapping and memory changes received [independent repository review](independent-cleanup-review.json). The [focused daemon regression](cleanup-daemon-focused.log) exercises public revision readback, concurrent message/migration refusal, native retirement, lost acknowledgements and exact receipt replay. [API contracts](cleanup-api-contract.log) were regenerated and [console type checking](cleanup-console-typecheck.log) passed. These checks do not replace the pending integrated archive gate.

## Native cleanup qualification limits

Cleanup refuses native roots, all Git-worktree kinds, explicitly Paseo-owned trees and configured adopted identities. Paseo 0.7.2 skips teardown commands and directory deletion for the permitted nonowned local kinds, and workspace archive preserves the parent project. The stored binding does not distinguish every historical creation from same-path recovery/adoption; qualification therefore retains the actual creation and readback receipts for its explicitly disposable local fixture.

The adapter refuses every terminal returned by the native terminal inventory, running/unknown script or setup state, unarchived sessions, malformed directory responses and incomplete pagination. Upstream 0.7.2 terminal listing masks internal listing errors as an empty list. The protocol therefore cannot certify terminal collector health; this limitation is not claimed as solved enforcement. The disposable fixture contains no externally created terminals or scripts. Its isolated native project/server teardown is a separate test-infrastructure action, not evidence that Kontor archived a native root.

## Historical qualification of `4c51059`

| Check | Result | Evidence |
| --- | --- | --- |
| Initial permission regressions against old behavior | 2 failed before correction | [Red run](permission-red.log) |
| Restored full adapter suite | 107 unit + 186 contract passed; 6 opt-in live tests excluded | [Adapter run](adapter-tests.log) |
| Frozen and current model routes | 1 focused registry recovery test passed | [Model recovery](model-compatibility-tests.log) |
| Current model catalog | 1 focused HTTP catalog test passed | [Model catalog](model-catalog-tests.log) |
| Deliberate defects | 3/3 killed by assertions, not compilation failures | [Mutation results](mutation-results.json) |
| Live Paseo 0.7.2 read-only suite | 5 passed before and after integration; same daemon identity and project count | [Initial readback](live-readonly.log), [archived candidate readback](live-integrated-readonly.log) |
| Documentation navigation | 82 local links resolve in the catalog layout | [Link check](document-links.json) |
| Optimized release binaries | Daemon, CLI and MCP build succeeded; all three help smoke checks passed; not installed | [Build and hashes](release-build.json), [smoke checks](release-binary-smoke.json) |
| Full archived release check | Passed: 2,259 Rust tests, 300 console tests, fmt, strict Clippy, reproducible lockfile, dependency/license checks and audits | [Qualification](qualification.json), [complete log](release-gates.log) |

Mutation checks remove the post-composition permission restriction, drop replacement routing, and drop replacement delivery. Each mutation was applied separately and restored in a finally block. The restored adapter suite passed afterwards. These targeted checks establish sensitivity to the repaired defects; they are not a claim of exhaustive mutation coverage.

## Recovery provenance and disposition

[Provenance](recovery-provenance.json) records the source checkout, source HEAD and SHA-256 values. [Recovered permission patches](recovered-permission-patches.json) records the bounded starting patches; the implementation strengthens them with final MCP restrictions, durable invalidation and behavior tests.

The original documentation branch remains available to its cleanup owner. Its useful governance paragraphs and `docs/RECOMMENDED-TEAMS-AND-SEATS.md` have been recovered into the current source tree. The old whole-branch diff must not be reapplied: that would restore outdated naming/tool counts or overwrite newer fixes. No worktree is deleted by this initiative.

Publication observation for the cleanup owner: on the clean, unpublished documentation branch, `asma git commit --push --create-pr --force` reported no pending pushes and attempted a PR before the remote branch existed. GitHub rejected the missing head. The subsequent ordinary commit-plus-push flow published the branch and created PR #2981. Confirm the remote branch/PR directly before treating a local checkpoint as recovered; correction of that ASMA CLI path is outside this compatibility change.

The old reservation migration and completion-recovery patch remain excluded. Succession, consultation recovery, naming migration and existing epic reconciliation remain with EPIC SPECIAL NAMING. Legacy AgentsRoom v4 model manifests remain historical/import evidence; new model availability is expressed in the current Kontor catalog without rewriting immutable run pins.

## Release and closeout checkpoint

No installed Kontor binary was changed by this source lane. Only historical candidate `4c51059` has the full archive qualification recorded here; the final integrated candidate still requires its own release check. Independent gates, disposable Kontor-to-Paseo mutation qualification, cleanup/readback and fleet deployment must still be evidenced before closure. The user requested serialized deployment after the naming release.

Admission has partially committed the graph but fails while governing the epic: the topology parent is terminal, outside scope or an illegal kind. Native Jira creation returned 503; a recorded ASMA-only fallback created the exact three issues above. Kontor stored their verified links. Initial materialization was refused after Jira automation moved them to DRAFT. The compatibility task has since been reconciled through Kontor to On hold with a converged fresh readback. Documentation reconciliation was blocked because the served build had no unique Jira workflow specification for its frozen docs profile; the additive v3 correction is committed and awaits installation through the supported control plane; see [the current reconciliation receipts](jira-reconciliation.json). The served CLI/MCP catalogs expose no standalone evidence-ingestion command. Do not invent a passed gate, create replacement topology or bypass the workflow to close these tasks.

The durable gap report and exact idempotency keys are in the asma-modules document `_docs/ai-orchestration/reports/2026-09-04-23-21-report-compatibility-kontor-operational-gap.md`. Resume the same identities after the committed admission and connector/workflow corrections are qualified and deployed, attach these artifacts through the supported recovered surface, and complete the real gates and release qualification.
