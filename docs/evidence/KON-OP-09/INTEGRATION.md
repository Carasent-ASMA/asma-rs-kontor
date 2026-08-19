# KON-OP-09 current-master integration

Date: 2026-08-19
Task: `KON-OP-09` / Jira `ASMA-7878`
Scope: merging current `origin/master` into the accepted OP-09 console revision
Seat: builder (`code` work profile, implementation phase)

Answers the integration refusal recorded in `RELEASE-NOTES.md`. The merge is one
commit: the generated-artifact conflict and the production behavior the merged
contract requires belong to the same revision, so no intermediate commit leaves
the tree failing to compile.

## Generated artifact

`origin/master` and this branch both changed `apps/console/src/api/schema.d.ts`.
It is generated, so it was regenerated from the merged authoritative
`crates/kontor-api/contract/openapi.json` rather than resolved by hand.
`verify:api` reports the committed file byte-identical to generator output.

## Contract changes and the behavior each required

| Contract | Console behavior |
| --- | --- |
| `InvokeConsultationRequest.caller_seat_binding_id` now required | The Advisor and Committee forms gained a required **Calling seat** select, populated from the seats the topology projection returned for the epic. |
| Consultation receipt is now optional | `Receipt` accepts an absent receipt and renders nothing rather than claiming one. The run's own typed state and id still render. |
| `CompletionStateDto.phase` is a tagged object | The phase renders through `CodeHelp` on `phase.phase`, with the round appended for the variants that carry one. |
| `outstanding` removed, `blockers` added | The panel lists each typed blocker: its controlled code through `CodeHelp`, then that variant's own fields as label/value. No variant is interpreted or collapsed. |
| `RemediateCompletionRequest.action` replaces `reason` | The free-form reason input is gone. The operator selects the acting authority and fills exactly that variant's fields; `remediationAction` builds the closed tagged object or refuses to submit. |

### The one decision worth confirming

`caller_seat_binding_id` is described as *"exact active epic seat whose role is
authorized by the pinned policy"*. The console cannot know which seat its human
operator is acting as, and deciding authorization in the browser is precisely
what the architecture forbids.

The seat is therefore **selected from the server's own topology projection**,
never typed, and every projected seat is offered rather than filtered by a
client-side guess at which role the pinned policy authorizes — an unauthorized
choice is refused by the daemon and shown as a refusal. When the topology read
fails there is no seat to name, so invocation is disabled with that stated
reason instead of falling back to an invented id.

If the intended behavior is narrower — one particular role, or a seat the
console should derive rather than offer — that is a contract reading the
inspector should correct here.

## Coverage

Four tests added (290 → 294), each verified against the defect it pins:

| Test | Pins |
| --- | --- |
| `names the calling seat from the topology projection when invoking a consultation` | the selected seat binding reaches the request |
| `refuses to offer a consultation caller when the topology projected no seat` | invocation disabled, reason stated |
| `sends the tagged remediation authority the operator selected` | each variant sends its own fields, and switching authority drops the other variant's |
| `renders the typed completion phase and its server blockers` | tagged phase and typed blockers render |

Hard-coding the caller seat turns the first red; making `remediationAction`
ignore the selected authority turns the third red. Both mutations were reverted.

Fixtures in `ProjectView.test.tsx` and `e2e/project.spec.ts` were updated to the
merged completion shape.

## Not done here

The panel still renders neither `rounds`, `closeout`, `wakes` nor the
`needs_human` payload the merged `CompletionStateDto` now carries. Those are new
surface rather than integration repair, and the completion panel's
incompleteness is already recorded in `RELEASE-NOTES.md`. This change replaces
`outstanding` with its direct successor and stops there.

The unrelated `evidence/ASMA-7854-PLAYWRIGHT-*.png`, `apps/console/test-results/`
and `docs/evidence/KON-MVP-18/run-*` paths in this worktree are untouched and
uncommitted.
