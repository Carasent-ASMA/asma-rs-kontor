# Seat handoff qualification

Load when authoring, reviewing or qualifying `seat-handoff`; routine handoff
uses the procedure in [SKILL.md](../SKILL.md).

## Scenario preparation

| ID | Short request (literal) | Prepared expected result | Negative / adjacent refusal |
| --- | --- | --- | --- |
| SCN-002 | Peer question vs "SWE hand to QA" | Peer: no scope/gate change. Handoff: identities, artifact SHA, criteria, ack; QA starts only when prerequisites hold | QA starts on message without ack or wrong SHA |
| SCN-003 | QA rejects; second rejection | Same fix owner gets full evidence; counters/findings persist; only corrected revision re-reviewed | New session must not reset counters |
| SCN-004 | Completion committee rejects | Prior verdict immutable; Adam remediation + TPM dispatch; clean re-review on corrected evidence | Silent overwrite of failed review |
| SCN-011 | Same handoff inputs on both surfaces | Same allowed next phase, rejection route, evidence obligations | — |
| SCN-014 | Tickets Done, release evidence missing | Handoff/completion refuses closure under selected contract | Label-only "done" blocked |

## Capability ownership

External proof: the surface's durable transition (not a message alone), the
formal protocols (including qualification of the clean independent re-review in
remediation) and combined completion qualification. This skill names the
capability owner per scenario; it does not claim their implementation.
