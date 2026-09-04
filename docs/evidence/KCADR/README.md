# ASMA-8098 compatibility recovery evidence

Status: implementation and automated release qualification complete; independent gates, disposable live mutation qualification and deployment remain pending. This is implementation-agent evidence, not an independent gate verdict or deployment receipt.

## Scope and identities

- Epic: ASMA-8098 / `01a06e13-878a-7aa3-8a08-bfad07cc8c4c` / backlog code `KCADR`.
- Compatibility: ASMA-8099 / `01a06e13-878b-77e1-885e-7187ffc97b6d`.
- Documentation: ASMA-8100 / `01a06e13-878c-7080-9801-d984bfe0eb04`.
- Realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`, project `01a0064a-e056-7603-9968-ef64fdaacb75`.
- Exact bindings and unwaivable gates: [admission readback](admission-readback.json).

The user approved one execution lane with no delegated agents. Both tasks are blocked at revision 2 to prevent additional automatic admission while release prerequisites remain open; see [the durable receipts and readback](release-block-readback.json). This change does not claim the fleet-verifier, fleet-spec-auditor, inspector or architect verdicts that the selected profiles require.

## Implementation

- Optional server permissions preserve old-daemon compatibility when omitted. Explicit permissions restrict the final composed capability surface, including MCP retitling fallback. Denied preparation and retitling leave runtime mutation counts unchanged.
- The WebSocket hello negotiates timeline replacement invalidation. Replacements are correlated by native agent ID and retained independently of the bounded content queue. A replacement requires canonical cursor-free refetch; old rows are discarded.
- New current Claude catalog choices use `claude-fable-5-1`. Frozen `claude-fable-5` routes still pass registry recovery for all three Claude account aliases. Existing sessions, definitions and historical evidence are not rewritten.
- Governance paragraphs and the team-design blueprint were recovered with current implementation boundaries, read-only consultation responsibilities and template-owned committee composition.

Source commits: `7381029` (adapter), `bb4ac51` (model catalog), `536b932` (repository guidance). Candidate `4c51059e07a9517d57cb9bd2665dc537ff681a2c` integrates naming owner PR #166 at `ac7e788243050714d9722733cc6548d781e387d9`. Root documentation is in [asma-modules PR #2981](https://github.com/Carasent-ASMA/asma-modules/pull/2981); implementation is in [Kontor PR #167](https://github.com/Carasent-ASMA/asma-rs-kontor/pull/167). Later commits contain evidence only; the runtime/build/test inputs remain identical to the qualified archive.

## Verification

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

No installed Kontor binary was changed by this lane. Automated release checks are complete. Independent gates, disposable Kontor-to-Paseo mutation qualification, cleanup/readback and fleet deployment must still be evidenced before closure. The user requested serialized deployment after the naming release.

Admission has partially committed the graph but fails while governing the epic: the topology parent is terminal, outside scope or an illegal kind. Native Jira creation returned 503; a recorded ASMA-only fallback created the exact three issues above. Kontor stored their verified links. Initial materialization was refused after Jira automation moved them to DRAFT. The compatibility task has since been reconciled through Kontor to On hold with a converged fresh readback. Documentation reconciliation remains blocked because the build has no unique Jira workflow specification for its frozen docs profile; see [the current reconciliation receipts](jira-reconciliation.json). The served CLI/MCP catalogs expose no standalone evidence-ingestion command. Do not invent a passed gate, create replacement topology or bypass the workflow to close these tasks.

The durable gap report and exact idempotency keys are in the asma-modules document `_docs/ai-orchestration/reports/2026-09-04-23-21-report-compatibility-kontor-operational-gap.md`. Resume the same identities after the responsible owner repairs admission and connector/workflow reconciliation, attach these artifacts through the supported recovered surface, and complete the real gates and release qualification.
