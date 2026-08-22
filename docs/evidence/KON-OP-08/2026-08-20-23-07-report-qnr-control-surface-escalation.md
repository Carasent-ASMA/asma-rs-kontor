# QNR control-surface escalation — replay, projection and TPM routing

> **Date:** 2026-08-20 23:07 CEST
> **Status:** 🔴 Defect verified; repair routed
> **Author:** Inspector · KON-OP-08
> **Category:** report
> **Scope:** ASMA-7877 / KON-OP-08 bounded escalation from ASMA-7675 / QNR-P1
> **Summary:** Independently reproduces cursor-free session timeline failures on exact active QNR seats, pins the epoch-origin/tail mismatch and message-projection lag with two red tests, and records the unreachable task-scoped TPM-seat invariant. The report authorizes no QNR identity, topology, AgentsRoom or gate mutation.

---

## When to Load

**Load this document when:**

- correcting `session-timeline-get` for active Paseo sessions whose tail begins
  after native sequence 1;
- making a successful `session-message-send` reduce exact AgentRun and TeamRun
  observations;
- repairing task-scoped persistent TPM-seat materialization or messaging;
- resuming QNR-P1 orchestration after the control surface is deployed.

**Do NOT load for:** QNR product-code implementation, replacing QNR runs, arming
GATE-RE-PORT, launching ASMA-7929, or approving PR 64.

---

## Bounded verdict

The escalation is **verified**. Three adjacent control-surface defects block
safe ASMA-7675 orchestration:

1. a cursor-free Paseo history request returns the newest tail page while the
   public API validates it as an epoch-origin page;
2. a successful session message does not persist a fresh post-delivery runtime
   observation, so AgentRun and TeamRun projections can remain
   `waiting_input` while the same native seat is running;
3. task-scoped persistent TPM SeatBindings are neither Delivery Team AgentRuns
   nor materialized hosted seats reachable through the topology-message route.

No QNR state was corrected in this inspection. The smallest repair and exact
red regressions are routed to preserved OP-08 builder AgentRun
`01a01eb0-453d-7c21-ac60-bee1d8cf8d73`.

## Authoritative identity and live reproduction

| Item | Value |
| --- | --- |
| Realm | `01a00649-9ee6-73e0-ba1b-6a6c35cfd065` |
| Project | `01a0064a-e056-7603-9968-ef64fdaacb75` |
| QNR epic | `01a019c0-eee7-72a1-a8a7-7fff1ddce8f3` / `ASMA-7675` |
| Authoritative cursor | `380` |
| OP-08 task | `01a0074f-672e-79a3-9876-d0e1bf585d4e`, revision 2 |
| OP-08 TeamRun | `01a0195b-7280-7500-81cf-c28023f8cbf8` |
| OP-08 builder | `01a01eb0-453d-7c21-ac60-bee1d8cf8d73` |
| Candidate checkout | `4e3e8fd774c04c6bd95bcd99f66b143df615b142` |

Supported `run-get` readback at cursor 380 preserved the exact native
identities:

| QNR task / role | AgentRun | Runtime binding | Native id | Projection |
| --- | --- | --- | --- | --- |
| ASMA-7679 builder | `01a01ce6-5991-7921-95af-c2b8b5c0fb2f` | `01a01eac-7c50-7561-b2ea-c4cd8bed0bf8` | `51ea9b29-a3cd-4beb-b6af-da9eec7a10eb` | `waiting_input`, stale, last cursor 368 |
| ASMA-7932 builder | `01a01eae-1ed6-7521-89a7-e43157b1798a` | `01a01eae-1ed6-7521-89a7-e44b956345f2` | `42ca852b-89ef-4b9a-bf08-fe328d51e8cb` | `waiting_input`, stale, last cursor 374 |
| OP-08 builder | `01a01eb0-453d-7c21-ac60-bee1d8cf8d73` | `01a01eb0-453d-7c21-ac60-bee1d8cf8d72` | `1743804b-f9b9-4167-98ad-e39b3f402a01` | `running`, stale, last cursor 377 |

Cursor-free `session-timeline-get --limit 100` returned HTTP 409
`timeline_refetch_required` for all five independently read seats:

- ASMA-7679 builder `01a01ce6-5991-7921-95af-c2b8b5c0fb2f`;
- ASMA-7679 architect `01a01b25-c44e-7883-8502-9f4780fd1c90`;
- ASMA-7932 builder `01a01eae-1ed6-7521-89a7-e43157b1798a`;
- ASMA-7932 architect `01a01b25-c46f-7fc0-b138-bc35453fab71`;
- OP-08 builder `01a01eb0-453d-7c21-ac60-bee1d8cf8d73`.

Each refusal returned no newest or oldest retained cursor and prescribed
reading the timeline again from the runtime. Repeating the prescribed
cursor-free operation therefore repeats the same refusal and cannot produce a
stream anchor.

The handed message acknowledgements remain the exact QNR incident examples:

- ASMA-7679 message `01a02077-0000-7000-8000-00000000000b`, epoch 4,
  sequence 632;
- ASMA-7932 message `01a02077-0000-7000-8000-00000000000c`, epoch 3,
  sequence 433.

## Finding QNR-CS-001 — cursor-free history violates the epoch-origin contract

`PaseoAdapter::history` selects `PaseoDirection::Tail` when no cursor is
present (`crates/kontor-runtime-paseo/src/adapter.rs`, lines 5224-5294). That is
the newest bounded page and can begin at any high native sequence.

The public timeline route treats the same cursor-free page as a read from the
epoch origin by constructing `HistoryReader::start` before validation
(`crates/kontor-api/src/sessions.rs`, lines 221-272). Its refusal is correct: a
page beginning at 101, 433 or 632 cannot prove that sequences 1 through the
preceding position were not skipped. The mismatch is that the adapter supplied
a tail page to an origin validator.

The existing lost-acknowledgement scanner already demonstrates the safe wire
mechanism: start with the only cursor-free `tail`, follow `hasOlder` and the
runtime-issued `startCursor` using `before`, keep the raw epoch fixed, and stop
at the oldest canonical page. Public history does not use that mechanism.

### Exact red regression

A temporary test named
`timeline_cursor_free_read_walks_back_to_the_epoch_origin` supplied a canonical
tail window at sequences 101-200 with `hasOlder=true`, plus a `before` page at
1-100. It called `PaseoAdapter::history(cursor=None)` and then the same
`HistoryReader::start` used by the API.

Result:

```text
TimelineRefetchRequired { reason: SequenceGap }
test result: FAILED. 0 passed; 1 failed; 137 filtered out
```

Command:

```text
cargo test -p kontor-runtime-paseo --test contract \
  timeline_cursor_free_read_walks_back_to_the_epoch_origin -- --exact --nocapture
```

Required green behavior: two native reads (`tail`, then `before`), a returned
page containing sequences 1-100, and a continuation cursor whose next read uses
`after` without a gap or overlap. Do not weaken `HistoryReader` or accept a
tail-derived anchor as complete history.

## Finding QNR-CS-002 — message success does not reduce runtime state

The session message route calls `adapter.resume`, discards the returned
`ControlPlaneObservation`, sends the message and returns its acknowledgement
(`crates/kontor-api/src/sessions.rs`, lines 457-497). It never performs a fresh
post-send inspection and never calls the durable observation reducer.

The reducer already exists in the daemon application layer:
`persist_run_observation` records the exact runtime identity, native sequence,
observed state and contact, then reduces both the AgentRun and TeamRun in the
same store transaction (`crates/kontor-daemon/src/applications.rs`, lines
2496-2545). The generic session-message route bypasses it.

### Exact red regression

A temporary loopback test named
`a_message_resume_reduces_the_run_and_team_run_back_to_running` first persisted
`waiting_input`, sent one successful message into the same bound session, and
asserted that the exact run and owning TeamRun reduced from a fresh runtime
observation.

Result:

```text
assertion `left == right` failed
left: WaitingInput
right: Running
test result: FAILED. 0 passed; 1 failed; 193 filtered out
```

Command:

```text
cargo test -p kontor-daemon --test loopback_api \
  a_message_resume_reduces_the_run_and_team_run_back_to_running \
  -- --exact --nocapture
```

Required correction: after an acknowledged delivery or acknowledged replay,
inspect the exact issued binding and persist that runtime observation through
one shared reducer. Do not infer `running` from message acceptance. Preserve
the runtime's exact observed state, identity and native sequence, and prove the
AgentRun and TeamRun move together without changing either identity.

## Finding QNR-CS-003 — task-scoped TPM seats have no valid messaging route

`topology-inspect` at cursor 380 reports active TPM SeatBindings in active QNR
TSWs, including:

- ASMA-7679: `01a01ce8-89b7-7e11-a35a-9a079ec4bb7f`;
- ASMA-7930: `01a01ce8-cb83-78c3-bd3b-99eaa3087a63`;
- ASMA-7932: `01a01ce9-b080-72b2-9d25-bd2b7fde9dc6`.

The corresponding Delivery Team projections contain architect, builder,
tester, inspector and verifier AgentRuns, but no TPM AgentRun. Therefore
`session-message-send` has no TPM AgentRun to address.

The topology-message implementation rejects every task-bound or TeamRun-bound
SeatBinding before it looks for a hosted native route and says to use the
session surface (`crates/kontor-daemon/src/applications.rs`, lines
10583-10615). Native hosted-seat launch is used by Core Team materialization
(`applications.rs`, lines 10323-10348); no task-scoped topology materializer
calls `launch_hosted_seat`. A task-scoped TPM is consequently neither class of
messageable seat.

Required invariant and test:

> Every active persistent SeatBinding is either backed by exactly one hosted
> native identity addressable through `topology-seat-message-send`, or linked
> to exactly one AgentRun addressable through `session-message-send`; never
> neither, never both. Messaging must not materialize a missing identity as a
> side effect.

The smallest correction is to materialize specification-authorized
task-scoped persistent seats through the hosted-seat path under their existing
SeatBinding, then permit the topology-message route for those non-TeamRun
seats. If the specification does not authorize a task-scoped persistent TPM,
the logical seat must not be published active. Do not create an AgentRun merely
to make the current row messageable.

## QNR preservation readback

The QNR epic remains at cursor 380 with the four existing TeamRuns only:

| TeamRun | Projection |
| --- | --- |
| `01a01b25-c333-7841-9be1-15b093d05561` | `running` |
| `01a01b25-c44e-7883-8502-9f30ecef92d7` | `waiting_input` |
| `01a01b25-c45b-7121-871e-36fb1d99329a` | `running` |
| `01a01b25-c46f-7fc0-b138-bc2d8e6ce8eb` | `waiting_input` |

`GATE-RE-PORT` task `01a01ad4-0d5a-7bf0-a414-5bb6191ea899` has zero
TeamRuns. The epic has zero ticket links for ASMA-7929. No QNR run, binding,
native identity, topology node, AgentsRoom record, gate or authorization was
mutated.

Both temporary tests were removed immediately after their single red run. The
source files were restored byte-for-byte:

| File | Blob before and after |
| --- | --- |
| `crates/kontor-runtime-paseo/tests/contract.rs` | `2503118fec33d350a093dbd6556ab27881ec6578` |
| `crates/kontor-daemon/tests/loopback_api.rs` | `ae0f8ae4f9af75a6f0c2c221139ccbc233a4743d` |

## Open-question ledger

### OQ-001 — historical message acknowledgement readback

- **Subject:** supported readback of the two exact historical message receipts.
- **Attached record:** this report; ASMA-7679/ASMA-7932 MessageIds above.
- **Why ambiguous:** the message route returns an acknowledgement but exposes no
  receipt-get surface; the canonical timeline that could prove each message is
  currently unavailable. Replaying a message requires its exact body and is an
  operator mutation, while direct SQLite inspection is not a supported Kontor
  surface.
- **Options observed:** add a read-only message-receipt surface; repair timeline
  replay and then verify the exact `clientMessageId`; or have the owning QNR
  record attach the original response document.
- **Disposition:** unresolved. The two receipt coordinates are handed evidence,
  not independently re-sent or inferred.

### OQ-002 — hosted native projection for task-scoped TPM seats

- **Subject:** supported readback of an exact native identity for the three
  active task-scoped TPM SeatBindings.
- **Attached record:** this report and the cursor-380 QNR topology projection.
- **Why ambiguous:** `topology-inspect` projects the logical SeatBinding but no
  hosted native seat for task nodes. `topology-seat-message-send` is effectful
  and rejects task-bound seats before native lookup; there is no read-only
  hosted-seat-get route.
- **Options observed:** project hosted native identity in `topology-inspect`;
  add an exact read-only hosted-seat surface; or add a no-effect route preview.
- **Disposition:** unresolved. No database inspection, test message or native
  creation was used. The structural no-route defect is independently proven by
  the public projections and implementation path.

## Builder handoff

The preserved OP-08 builder should implement the three invariants above,
commit the two named red tests (plus the task-scoped TPM routing test), run the
focused suites and mutation-delete each new guard. The handoff must return:

- exact commit/base/head;
- both formerly red tests green and the new TPM invariant test green;
- a live cursor-free timeline page and usable stream anchor for the same QNR
  builder identity;
- post-message AgentRun and TeamRun readback at one new cursor;
- unchanged QNR binding/native ids, four-TeamRun cardinality, zero
  GATE-RE-PORT runs and ASMA-7929 absence.

## Routing receipt

The supported Kontor handoff used AgentRun
`01a01eb0-453d-7c21-ac60-bee1d8cf8d73` and idempotency key
`01a02102-2c5a-76aa-af1e-40a814c3fe90`. The original request and one exact
same-key/same-body replay both returned HTTP 503 `unavailable` because the
session runtime could not be reached. No new message id was minted, and the
response reported no control-plane change. The unresolved native-delivery
ambiguity is recorded in the operational-gap report rather than inferred.

Under the temporary Kontor-first recovery rule, one bounded direct Paseo
fallback was recorded and then delivered at 2026-08-20 23:15 CEST to the same
native builder `1743804b-f9b9-4167-98ad-e39b3f402a01` in the same workspace
`wks_162d4d0509f255c1`. Native status readback preserved the Kontor AgentRun
label and session `01a01eb0-6296-72f1-ba3a-a63b5ff1e705`; the prompt call
returned `success: true`, `status: running`, with completion notification
registered. No agent, workspace, session, binding or run was created.

The durable fallback record is
`_docs/ai-orchestration/reports/2026-08-20-23-13-report-kontor-operational-gap-qnr-escalation-route.md`.
The repair remains pending until the preserved builder returns implementation
and live verification evidence.
