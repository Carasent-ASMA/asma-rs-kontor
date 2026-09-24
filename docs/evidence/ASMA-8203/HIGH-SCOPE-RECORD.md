Artifact: `high-scope-record`

# ASMA-8203 high-scope record: runtime message proof on delivery turns

Date: 2026-09-17
Task: Jira `ASMA-8203` / Kontor `01a0ac9d-a96d-75b2-9803-169d4f2e531f`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-scope`
TeamRun: `01a0acf9-6636-7040-8b15-d9e31793c685`
Baseline: clean `f78d041`

## Outcome

Treat ASMA-8203 as regression and mutation hardening of the existing delivery
proof path. Do not add another message-echo mechanism, proof extractor, API,
schema, or settlement state machine unless a required mutant survives and
demonstrates a production defect.

The behavior named by the ticket already exists at the frozen baseline:

1. A Paseo delivery launch sends the runtime binding UUID as
   `clientMessageId`; follow-up sends carry their durable Kontor `MessageId`.
2. Paseo canonical timeline normalization maps only `clientMessageId` to
   `EventSubject::Message`. The provider's `messageId` is deliberately not
   treated as Kontor identity.
3. Follow-up delivery returns `MessageAck` with the exact canonical user-message
   position. Initial-turn reconciliation already scans canonical history for the
   same client message id.
4. `Services::prove_current_turn` freshly attests and inspects the exact binding,
   requires `waiting_input`, reads from immediately before the claimed message,
   and accepts only one exact message-id/position match and one later terminal
   response in the same epoch.
5. `turn-settle` rejects absent proof before a runtime effect, rejects stale or
   fabricated windows, and persists the complete tuple under the store's
   append-only proof constraint.

The original operational report behind this ticket is not evidence of an echo
defect. Approved memory item
`operational-gap-delivery-turn-message-id-never-echoed-20260916` revision 3
closes OG-055 as a false positive: only the first bounded timeline page had
been read, and the attempted self-settlement occurred while the seat was still
running and before its response existed as the terminal event.

## Live proof already present

The protected realm contains current-runtime role-turn receipts produced by the
supported settlement surface. These are evidence that the deployed path carries
the exact tuple; they are not inferred from transcript text.

| Turn | Kontor message id | Message position | Response position |
| --- | --- | --- | --- |
| ASMA-8192 implement turn 1, `01a0ac92-f555-7892-82cc-0d48eb1ccabe` | `01a0ac66-1de8-71a5-8fab-4c3c28042609` | `1:971` | `1:1300` |
| ASMA-8192 verify turn 1, `01a0ac9f-69b6-7312-b115-54d649430d56` | `d4f8bc97-6636-7b6f-927b-d4e9e2e2b4b6` | `2:11` | `2:224` |
| ASMA-8192 audit turn 1, `01a0acfd-71ee-75e0-839a-4581d9988456` | `56a3df04-8c2e-7290-940c-cc684cd01a74` | `3:314` | `3:380` |

The first receipt records `high-change` and `high-scope-record` with evidence
hash `1a338b20984416bee60b9fa4c491826744bac3163b151590e7d69faae59c0a6b`.
Later turns prove the path is not a one-off tied to one timeline epoch.

## Open-question ledger

### OQ-8203-01 — post-response settlement ownership — RESOLVED

- **Subject:** whether "delivery turns carry proof" requires a delivery seat to
  settle itself before returning its response.
- **Attaches to:** Jira ASMA-8203 and
  `POST /v1/projects/{project_id}/agent-runs/{agent_run_id}/turns:settle`,
  specifically `TurnRuntimeProofRequest`.
- **Why the state was ambiguous:** the ticket requires a supported settlement
  with message, request, and response positions but does not name the caller.
  A responding seat cannot observe the canonical position of the response it
  has not returned yet. The protected realm proves that a post-turn control
  caller can obtain and submit the exact tuple successfully.
- **Options seen:** (a) keep post-turn settlement and harden its explicit
  evidence contract; (b) add a read-only proof-discovery operation but retain a
  separate settlement caller; (c) add durable pending-settlement intent so a
  seat stages artifacts before responding and the scheduler finalizes later.
- **Disposition:** (a) for ASMA-8203. It is the behavior the Jira acceptance text
  names and the live receipts prove. Option (b) duplicates canonical-history
  scanning without changing settlement authority. Option (c) is a new
  asynchronous command, persistence, replay, revision, and failure-recovery
  contract absent from this ticket. If autonomous self-settlement is required,
  admit it separately with those semantics and acceptance cases rather than
  hiding it inside a Paseo echo change.

No product or architecture ambiguity remains open in this scope record.

## Implementation boundary

Implementation owns only the proof gaps in the committed regression and
mutation evidence:

1. Add a daemon loopback matrix proving that an otherwise well-formed request
   is refused when it supplies a different valid message id, a wrong
   user-message position, or a non-terminal/wrong response position. Each
   refusal must assert the stable error code/rule and that no role turn,
   artifact evidence, dispatch, or other write was recorded.
2. Retain the existing missing-proof and stale-proof checks. Extend them only
   where needed to assert the same zero-write boundary; do not duplicate their
   setup into a new framework.
3. Add or strengthen one Paseo adapter contract regression that follows the
   delivery value end to end: the launch/send request's Kontor id is
   `clientMessageId`, canonical history normalizes that exact id onto the user
   message at its exact position, and the provider `messageId` cannot stand in
   for it. Reuse the recorded fixture and current normalization path.
4. Run the targeted mutants below one at a time in a disposable checkout,
   restore each immediately, and record the red assertion plus restored green
   result. Production code changes are allowed only if a mutant survives due to
   a real behavior gap, not merely to make this ticket non-empty.

### Targeted mutation contract

| Mutant | Deliberate defect | Required killer |
| --- | --- | --- |
| M1 | Normalize Paseo provider `messageId` instead of `clientMessageId` | Paseo identity regression |
| M2 | Stop requiring `EventSubject::Message(message_id)` at the claimed request position | forged-message-id loopback case |
| M3 | Stop requiring the claimed response to be the last non-state/non-log turn event | forged-terminal-position loopback case |
| M4 | Permit `runtime_proof: None` to reach settlement | existing missing-proof loopback case |

The restored tree must contain none of the mutants. The mutation record belongs
under `docs/evidence/ASMA-8203/` and is part of `high-change` evidence.

## Explicit non-scope

- Do not alter Paseo's request wire solely to add an echo it already sends and
  receives.
- Do not add a second proof scanner beside `prove_current_turn`.
- Do not infer the current turn from the latest arbitrary message or response.
- Do not weaken fresh binding attestation, `waiting_input`, same-epoch ordering,
  exact message identity, response terminality, or single-use settlement.
- Do not add a migration or new persistence row for a test-only hardening task.
- Do not rebuild or redeploy an unchanged production binary. If implementation
  finds and fixes a production defect, the normal build, deploy, and live
  readback requirements become mandatory and must name that delta.
- Do not use transcript prose as runtime evidence or repeat direct Paseo reads
  when the supported Kontor surface already exposes canonical history.

## Scope verification

| Check | Result |
| --- | --- |
| `cargo test -p kontor-runtime-paseo only_the_client_message_id_addresses_a_kontor_message` | PASS, 1/1 |
| `cargo test -p kontor-daemon --test loopback_api settling_a_bounded_turn_leaves_the_seat_live_and_the_run_open -- --exact` | PASS, 1/1 |
| `cargo test -p kontor-daemon --test loopback_api settling_a_bounded_turn_reads_only_the_claimed_current_window -- --exact` | PASS, 1/1 |
| Protected `role_turns` readback for ASMA-8192 | PASS; multiple exact `current_runtime` proof tuples persist across epochs |

These checks establish the baseline; they do not claim the new negative matrix
or mutation pass has been performed.

## Handoff

The implementation seat should preserve this branch and worktree, add the
smallest focused regressions above, run M1-M4, and produce `high-change` with
the exact commands and mutation receipts. A no-production-code result is the
expected outcome and is acceptable. After its response is terminal, settle the
ASMA-8203 implementation turn through the same supported Kontor surface; that
receipt is the task-local live proof. Do not make the implementation seat guess
its own not-yet-existing response position.

