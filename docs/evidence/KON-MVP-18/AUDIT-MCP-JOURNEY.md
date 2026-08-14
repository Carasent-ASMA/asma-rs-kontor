# KON-18 final MCP journey audit

Date: 2026-08-14  
Ticket: KON-MVP-18 / ASMA-7762  
Committed checkpoint: `1d3cf8877fe98673056207af27f8d05db568e6a5`  
Live evidence: `live-20260814T193824Z`  
TSW: `wks_5f1dd03a839f8c04`  
Archive: `e424212`

## Verdict

**AUDITED_TRUE — both KON-20 corrective close-out requirements are satisfied.**

The live bundle remains honestly `NON_COMPLIANT` at 38 pass / 0 fail / 4
blocked. That is not contradicted by this audit: the committed MCP journey is
the required empty-realm-to-closed proof, while the separate live Paseo bundle
records real runtime evidence and preserves its own harness-coverage blocks.

## Checklist

### 1. Committed MCP-only empty-realm journey — PASS / CLOSED

`crates/kontor-daemon/tests/mcp_journey.rs:289-694` drives one empty realm
through catalog discovery, project/account/epic creation, planning, two
scheduler rounds, runtime settlement, gate recording, task completion, and
`close_epic`, asserting `closed["state"] == "closed"`.

The test uses one admin `Dispatcher`, one `RouterTransport`, and one realm
credential. All instructions to Kontor are dispatched through the MCP tool
surface; the transport drives the production router, authentication,
authority, schema, application, and store paths. The native-finish helper is
explicitly the outside runtime reaching terminal state, not manual session
administration or a second Kontor ingress.

Cardinality is non-vacuous: the test asserts the 11-call checkpoint and, after
completion, `transport.calls() == routes.len()` plus `/v1/` for every route.
Later identifiers, tasks, gates, artifacts, and revisions are read from tool
responses rather than seeded assumptions.

The commit message records the non-vacuity mutation: inverting the live-attempt
guard in `certify_team_closure` makes the journey red. It also honestly notes
that removing only the dependency gate is masked by the independent
`max_concurrency: 1` limit; that limitation is not claimed as coverage of the
dependency guard.

### 2. Separate real-Paseo evidence — PASS / CLOSED

Independently checked the live manifest: all 46 listed artifact hashes match
disk and the sorted NUL-join root recomputes to
`ef2106e655384c7981ac232e726af165586094d48e0e77727ab009a36f592833`.
The live bundle does not contain a `MANIFEST.sha256`; therefore direct
`shasum -c` is not applicable, but the manifest's 46 per-artifact hashes and
root formula are independently verified.

`phase0/inventory.json` records real Paseo/CLI 0.3.1, a reachable daemon at
`127.0.0.1:6767`, and the live Paseo lane. `lifecycle/settle.json` records
successful `kontor_turn_settle` for both bound seats. Restart evidence keeps
the native session and binding identity, replays the same message at the same
epoch/sequence with exactly one timeline occurrence, and accepts a fresh key.

Cleanup records the Kontor daemon stopped, all three MCP children closed, one
created workspace archived with zero remaining, and two native agents archived.
The disposable Paseo project is explicitly retained because Paseo 0.3.1 has no
project-removal operation.

### 3. Honest live NON_COMPLIANT result — PASS / CLOSED

The bundle verdict remains `NON_COMPLIANT`, with 38 pass, 0 fail, 4 blocked,
and no relabelling. The two unbound seats return `role_slot_unbound`; the live
run did not invoke `kontor_role_slot_waive`. This is a harness coverage
omission, not a new product defect. The waiver contract is already covered by
the audited `AUDIT-RERUN-A280AAF.md` and
`LEAD-DECISION-MUT-006-SURVIVOR.md`, so the live refusal and the waiver
contract are consistent.

### 4. BLK-005 event-gap — PASS / CLOSED

The sealed deterministic evidence records MUT-008 as `KILLED`: both the
contract runtime-adapter and pilot oracles exit 101 under the sequence-gap
mutation. The live bundle correctly leaves its own event-gap case blocked
because it did not produce deterministic mutation results; the prior sealed
oracle is the required coverage and no scope cut is hidden.

### 5. Gates and scope — PASS

The QA composite records journey 2/2, cardinality/mutants/parity 11/11/11,
full daemon 0 failed, fmt clean, and clippy clean. The committed test-only
change is exactly the MCP journey test. Cargo.lock remains SHA-256
`fd022e16848992060cac6657706b48e6787575f9f768d886e32a7a4255a59453`.

The committed submodule is at `1d3cf887` with no tracked or staged source diff.
Historical untracked `run-*` directories are preserved. The supplied
`QA-MCP-JOURNEY.md` is also present as an untracked evidence document; this
does not alter the committed tree or the journey proof. This audit report is
the requested additional evidence write.

## Final statement

**AUDITED_TRUE.** The committed MCP-only journey and the separate real-Paseo
artifact satisfy the two KON-20 corrective close-out requirements. The live
bundle's `NON_COMPLIANT` verdict remains valid and transparent on its four
declared blocks; it does not represent a contradiction or hidden product
defect. No second live run, new seat, code edit, commit, push, ticket, or board
mutation was performed by this audit.

## Audit confirmation — 2026-08-14

**CONFIRMED.** The QA correction is internally consistent. Its header and
composite review both identify commit `1d3cf8877fe98673056207af27f8d05db568e6a5`,
and the headline is unambiguous: `COMPOSITE_PASS`. The earlier `a280aaf`
`READY-FOR-AUDIT` preflight remains present under an explicit superseded-history
section rather than being deleted.

The composite body still matches the audited claims: the deterministic MCP
journey reaches closed, the separate live Paseo artifact is identified, the
live result remains `NON_COMPLIANT` at 38/0/4, the omitted waiver invocation is
the harness-coverage close blocker, and BLK-005 is covered by sealed MUT-008
deterministic evidence. No code, Cargo.lock, audit content before this note, or
foreign `run-*` evidence directory was changed by the correction review. The
underlying **AUDITED_TRUE** verdict stands unchanged.
