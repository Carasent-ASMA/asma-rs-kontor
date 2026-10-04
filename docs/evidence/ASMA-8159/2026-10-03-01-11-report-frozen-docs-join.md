# Frozen documentation join findings

> **Date:** 2026-10-03 01:11 Europe/Oslo
> **Status:** 🔴 Draft — integration findings for TPM routing
> **Category:** report
> **Scope:** ASMA-8159 read-only join of independently reviewed ASMA-8157 docs
> **Summary:** Compare frozen docs with accepted ASMA-8156/8158 source names and configuration. No docs-branch edit or independent review verdict is issued.

## When to load

Load to route the documentation join before final release or to reproduce the
exact operation/configuration comparison. Evidence is in
[docs-join.json](receipts/docs-join.json).

## Pins and method

Docs repository: asma-modules, frozen commit
9b5b538c269deca55058f3fdd9efc66ee8046ab9, worktree
/Users/igor/.paseo/worktrees/0vl4ss0m/docs-asma-8157-experience-memory-skill-and-runbook.
HEAD matched the frozen commit and status was clean. All comparisons read
immutable git-show blobs; this checkout was never modified.

Source: ASMA-8156 655e07ead8b87bebf3a5d43ddccc15c860114e05 and
ASMA-8158 / combined HEAD 831dd7c1dc95a57258c75f5896bff5b76f3830fa.
The user supplied QA+Audit PASS for both source pins, auditPassed=true for 8158,
and independent docs review PASS for 9b5b538c. These are carried review-status
pins, not fresh reviewer readbacks or acceptance of this integration package.

## Precise drift

| Finding | Frozen location | Comparison and owning disposition |
| --- | --- | --- |
| DOC-01 — final typed contract and names absent | .github/skills/experience-memory/SKILL.md:26–30, 151–153; runbook:5, 8, 37–49, 262–264 | Says encoding/names are provisional or await 8156. Neither main document names any of the eight accepted registry operations below. Pin accepted source/schema/fixture and replace only the now-resolved provisional statements through the ASMA-8157 docs owner. Wire-valid example/evidence validation is still not demonstrated by a prose worksheet. |
| DOC-02 — accepted transport configuration absent | Skill's capability/projection sections; runbook:1 and 5 | Neither names memory-cognee.json, enabled, credential_alias, dataset_prefix, timeout_ms, cognee_unavailable or the explicit DaemonConfig::with_memory_cognee composition gate. Add source-accurate configuration/reference and distinguish fixture composition, qualification, activation and ordinary-startup/public-rebuild unavailability. Route to docs owner; live enablement remains a later product frontier. |
| DOC-03 — loader note stale at frozen tip | runbook:266–270 | Still says the Claude catalog link and generated inventory refresh are needed. Frozen 9b5b538c actually contains .claude/skills/experience-memory and the refreshed .github/agent-policy-inventory.json; its commit changes those two paths. Route this resolved source-configuration note to docs/loader owners. Presence is not an installed harness/runtime attestation. |

These are omissions/stale references, not detected misspellings of a published
operation. No conflicting literal operation/configuration name was found:
the candidate intentionally avoided wire commands while the source was pending.
Conceptual approval, eligibility, provider-policy, ordering, byte-budget,
degradation, replay/purge and post-canary activation contracts align with the
accepted source. All four affected skill pairs (experience-memory,
mini-project-kickoff, epic-orchestration and remember) are byte-identical.

## Accepted source operation names

The registry is crates/kontor-mcp/src/registry.rs on the combined pin. Generated
CLI suffixes remove kontor_ and use hyphens. Actual help and parity checks
remain source evidence, never an installed-operation claim.

| MCP registry operation | HTTP method and path | Tier |
| --- | --- | --- |
| kontor_experience_propose | POST /v1/projects/{project_id}/memory/experiences:propose | Operator |
| kontor_experience_classify | GET /v1/projects/{project_id}/memory/experiences:classify | Observer |
| kontor_memory_recall_preview | POST /v1/projects/{project_id}/memory/recall:preview | Observer |
| kontor_memory_recall_freeze | POST /v1/projects/{project_id}/memory/recall:freeze | Operator |
| kontor_memory_recall_get | GET /v1/projects/{project_id}/memory/recall/{agent_run_id} | Observer |
| kontor_memory_projection_preview | GET /v1/projects/{project_id}/memory/projection:preview | Observer |
| kontor_memory_projection_get | GET /v1/projects/{project_id}/memory/projection | Observer |
| kontor_memory_projection_rebuild | POST /v1/projects/{project_id}/memory/projection:rebuild | Operator |

Existing Admin approval/tombstone/purge operations remain unchanged.
There is no separate public typed-document validation/preview or projection
activation operation in these eight additions. Recall preview previews recall,
not a proposed document. Do not invent one to finalize DOC-01. The reviewed core
fixture and server-produced OpenAPI define the encoding; runtime proposal and
evidence resolution still occur at their authoritative boundary.

## Accepted transport configuration

docs/CONFIGURATION.md:31–94 describes optional <state-root>/memory-cognee.json,
strict JSON ≤8 KiB, disabled by default:

| Key | Default / constraint |
| --- | --- |
| enabled | false; explicit transport opt-in |
| endpoint | null; self-hosted HTTP loopback or HTTPS, no userinfo/query/fragment |
| credential_alias | null; non-secret alias, not a resolved credential |
| dataset_prefix | kontor only in v1 |
| timeout_ms | 1500; range 1–1500 for entire semantic operation |

Default document projection_policy remains local_only. Client::qualify returns
add/cognify/canary evidence without activation. SqliteStore::activate_projection
requires those successes and canonical freshness/CAS. Ordinary startup refuses
enabled configuration without explicit transport; public rebuild remains
projection_unavailable. This is an acknowledged capability frontier carried
from the accepted source, not a newly discovered accepted-source defect.

## Unchanged boundaries

Runbook generation-12 cross-Realm export exclusions match current source.
Its warning that historical plan migration 0095 is occupied is correct.
Schema/number reconciliation with external ASMA-8187 remains in the separate
worksheet. No superproject pointer, policy/skill file, fleet selection, live
configuration, corpus, authority or orchestration surface was changed.
