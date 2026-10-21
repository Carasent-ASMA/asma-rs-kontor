# Agent dispatch qualification

Load when authoring, reviewing or qualifying `agent-dispatch`; routine dispatch
uses the procedure in [SKILL.md](../SKILL.md).

## Scenario preparation

Replay each case against a frozen procedure revision; new commits invalidate
affected acceptance evidence. Record short request, selected
procedure, scope/surface/workflow, expected transition, forbidden effects,
receiving-checkout paths and capability owner for runtime proof.

| ID | Short request (literal) | Prepared expected result | Negative / adjacent refusal |
| --- | --- | --- | --- |
| SCN-001 | "Start QA for `EXM-8279`" (duplicate or lost callback variant) | One declared logical seat; supported route; exact placement/parent inputs; replay reconciles without second occupant | Second launch while occupant active → reconcile, do not duplicate |
| SCN-010 | Adam plans task C while TPM dispatches A/B | One plan publisher; one admission writer; ready work only; same-module serialized unless isolated worktrees verified | Two supervisors admit same task → refuse second admission |
| SCN-011 | Same dispatch inputs in Paseo-direct vs Kontor-governed | Same allowed next phase and evidence obligations; mechanism differences explicit | — |
| SCN-012 | "Run Independent Review on ..." / advisor request | Caller vs placement preserved; only supported named protocol; advice ≠ formal gate | Planning pair selected where Independent Review required → block |
| SCN-013 | Wrong trigger, missing required reference, or loading optimization | Fail with actionable reason; no unrelated skill selected | Reduced context must not drop fresh/resume/post-compaction restoration in any claimed harness |

## Capability ownership

External proof owners are named by the project's plan: policy resolution,
topology/parent placement, the actual loading router and inventory, and paired
acceptance and cross-surface parity of paired runs. This skill names the
capability owner per scenario; it does not claim their implementation.
