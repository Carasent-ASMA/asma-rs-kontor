# Seat takeover qualification

Load when authoring, reviewing or qualifying `seat-takeover`; routine takeover
uses the procedure in [SKILL.md](../SKILL.md).

## Scenario preparation

| ID | Short request (literal) | Prepared expected result | Negative / adjacent refusal |
| --- | --- | --- | --- |
| SCN-005 | "Take over `<exact native id>`" | Re-read seat, occupant, generation, supervisor, cwd, checkout, obligations; resume a usable occupant or admit one replacement with preserved phase/evidence/counters when unusable or explicitly authorized | Healthy alone does not refuse an explicitly authorized replacement |
| SCN-006 | Ambiguous title / known predecessor ID | Report candidates, no mutation; predecessor ID → recorded successor | Picking the newest title is forbidden |
| SCN-007 | Partially failed or replayed takeover | Reconcile correlation; preflight failure keeps predecessor; post-retirement failure → unoccupied recovery; no duplicate successor | Concurrent writers blocked; a mismatched successor must be retired before another recovery admission |
| SCN-008 | Watchdog sees idle / wait / hold / permission / pause | Reconcile same occupant; preserve an active legitimate wait; resume only after resolution and authorization | Unauthorized resume or replacement blocked |
| SCN-009 | Fresh seat vs compaction vs unusability | Restore required procedure revision; compaction preserves identity; pressure alone does not authorize takeover | Replacement without retirement evidence blocked |
| SCN-011 | Same takeover inputs on both surfaces | Same allowed next phase and evidence rules; mechanism gaps explicit | Procedure alone does not prove runtime parity |

## Additional forward-test cases

- Explicitly requested replacement of a healthy occupant preserves work and
  resolves the requested admissible route without requiring an invented failure.
- On Paseo-direct, an already-open receiver with suitable placement and authority
  can become the recorded owner after verified predecessor archive; native
  adoption is optional and no third session is created. On Kontor-governed work,
  missing required adoption/binding keeps the predecessor intact.
- An ad-hoc brainstorm without Git marks absent workflow/repository/branch/HEAD
  as not applicable, keeps actual cwd and permission provenance, and invents no
  tracker key or policy pin.
- The successor keeps the exact original active title; the predecessor gets one
  `STALE • ` prefix under selected naming policy. Unsupported archived retitle
  records `rename_pending`; neither title is evidence of retirement.
- The initial prompt permits bootstrap only. Wrong placement or permissions
  blocks task effects until corrected and read back.
- Paseo-direct supervisor replacement preserves healthy children before archive
  and verifies notifications, a subscription or finite observation. Logical
  mapping alone does not complete reporting; missing callback transfer selects
  the fallback. Governed replacement still verifies delivery of child
  notifications under its selected contract.

## Direct recovery decisions

Assess these requests with the skill and raw session/checkpoint observations.
Do not mutate live sessions to qualify a documentation change. Record the chosen
next action and its evidence; wording checks are not behavioral qualification.

| Case | Observations | Required decision |
| --- | --- | --- |
| Existing receiver | User says "you take over"; receiver placement/authority match; no native adopt API | Prepare brief and worker/reporting routes, archive and verify predecessor, record this receiver as sole owner; continue without creating another session |
| Receiver owns another slot | Idle AUD session is explicitly named as the SWE replacement | Preserve AUD obligations and record that slot vacant before SWE activation; this builder session cannot independently audit the work |
| Running managed child | Child shares parent's workspace, no open tab; native callback still targets predecessor | Detach through supported CLI/SDK before parent archive, verify child survives; subscribe or observe finitely without a probe prompt |
| Provider-native worker | Background work has no independent managed Paseo ID | Account for and finish/checkpoint/recover that work before retiring its runtime; do not claim detach protects it |
| Failed retirement | Archive response failed during close, although `archivedAt` is set | No successor writes for this task/checkout, including another file; reconcile actual runtime and preserve evidence; unrelated work uses a separate checkout where applicable |
| Replay/race | Checkpoint already names an active successor, or another coordinator is retiring the old session | Return existing successor or reconcile in-flight operation; never admit a duplicate |
| Lost callback/restart | Worker remains healthy, old notification registration unavailable | Restore read-only subscription or finite observation with an owner, deadline and terminal condition; no worker restart or routing-only prompt |
| Admission fails | Old session successfully retired; receiver cwd or permissions mismatch | Unoccupied/recovery state; bootstrap only until corrected and verified; no silent old-session revival |
| Off-fleet resume | Idle seat's effective model differs from its selected role binding | Preserve work; block task effects and correct through supported lifecycle; do not copy the drift into a successor |
| Recovery without fleet | Authentication failed; no selected fleet or no eligible route; runtime offers a default | Locate/create and select the fleet or record route exhaustion before retirement; no default model or duplicate writer |
| Governed control | Selected runtime requires revisioned adoption and native binding, operation unavailable | Preserve governed contract and report that scoped capability gap; direct fallback cannot satisfy it |

Native mechanism evidence and limitations live in
[Paseo-direct recovery](paseo-direct-recovery.md#native-evidence-and-capability-limits).

## Capability ownership

Governed runtime proof belongs to the project plan's topology, retirement,
adoption and correlation owner, the takeover owner, the loading foundation and
paired acceptance, under the owning plan. Skill prose and simulated decisions do
not qualify these capabilities or activate them. The direct procedural fallback
uses observed supported operations and a recorded owner; it does not claim
those governed capabilities or wait for their implementation.
