# Common evidence envelope

One shared copy for [seat-handoff](../SKILL.md#common-evidence-envelope) and
[seat-takeover](../../seat-takeover/SKILL.md#common-takeover-evidence-fields).
Every normal handoff, rework, remediation and takeover records these fields.

Record **references and hashes only**. Never copy fleet tables, model rosters
or full policy bodies into the package or brief.

| Field | Requirement |
| --- | --- |
| Procedure revision | Exact git SHA (or pinned revision id) of the skill file used: `seat-handoff` for a transition, `seat-takeover` for a takeover |
| Orchestration surface | `paseo-direct` or `kontor-governed`, per the [surface rule](../../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule) |
| Selected workflow | Pinned workflow/plan revision id or commit SHA the transition or takeover runs under |
| Policy provenance | Selected policy source references: path + content hash (for example the activated orchestration bundle hash or an explicit last-valid snapshot id recorded in plan evidence) |
| Fleet selection and readback | Exact fleet location/hash and selection evidence, role/slot binding, resolved provider/account route/model/effort and observed runtime values; apply [selected fleet enforcement](../../epic-orchestration/references/fleet-selection.md) before accepting the receiving seat |
| Source identity (handoff) | Source logical seat, native occupant id, scope tracker key |
| Receiver identity (handoff) | Target logical seat, native occupant id (or declared slot awaiting occupant), scope tracker key |
| Receiving checkout | **Absolute** filesystem path to the repository root the receiver must use |
| Receiving `cwd` | **Absolute** workspace/seat working directory read back from placement (not the launcher's assumed tree) |
| Branch | Branch name bound to the task or contribution, matching `{{ project.branch.pattern }}` |
| `HEAD` | Commit SHA for that checkout: at handoff time; for a takeover, at resolve, brief and readback as applicable |
| Artifact SHA | Applicable artifact under review: commit, published revision or declared deliverable id (takeover: when one exists) |

A takeover records predecessor and successor identities in its own sequence,
not in the two handoff identity rows.
