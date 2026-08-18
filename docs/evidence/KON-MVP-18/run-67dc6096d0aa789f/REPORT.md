# KON-MVP-18 pilot evidence — ACCEPT

Run `run-67dc6096d0aa789f` · commit `9cf14733ce88ae750a6b0f30b3069be9a89c4b25`

| pass | fail | blocked | missing |
| --- | --- | --- | --- |
| 42 | 0 | 0 | 0 |

## Cases

| case | outcome | criterion | evidence |
| --- | --- | --- | --- |
| `project.profiles` | pass | five tasks resolve five pinned work-profile snapshots | `snapshots/profiles/pilot-code.json`<br>`snapshots/profiles/pilot-ux.json`<br>`snapshots/profiles/pilot-research.json`<br>`snapshots/profiles/pilot-docs.json`<br>`snapshots/profiles/pilot-incident.json` |
| `project.custom-profile` | pass | the incident profile runs as fixture data with no core or client branch | `runtime/custom-profile-scan.json` |
| `project.worktrees` | pass | safe parallel tasks hold distinct verified worktrees | `snapshots/scheduling-safe-batch.json` |
| `project.collision-contender` | pass | the non-isolated contender never launches | `runtime/collision-refusal.json` |
| `project.two-accounts` | pass | two account profiles run concurrently and are attributed correctly | `runtime/accounts.json` |
| `project.account-secrecy` | pass | no credential or token canary reaches any persisted or logged artifact | `runtime/account-secrecy.json`<br>`runtime/accounts.json` |
| `project.cross-engine` | pass | a sealed handoff capsule links a successor on the other runtime | `snapshots/handoff-capsule.json`<br>`receipts/handoff-acknowledgement.json` |
| `project.workspace-identity` | pass | predecessor and successor share the exact verified task workspace | `snapshots/handoff-workspace.json`<br>`snapshots/handoff-capsule.json`<br>`receipts/handoff-acknowledgement.json` |
| `negative.collision` | pass | two armed tasks sharing a module without isolation refuse, first lease unchanged | `runtime/collision-refusal.json`<br>`snapshots/scheduling-safe-batch.json` |
| `negative.rejection-loop` | pass | the second rejection by one reviewer parks and launches no third run | `snapshots/gate-rejection-loop.json`<br>`receipts/gate-park.json` |
| `negative.rejection-reset` | pass | a pass resets only that reviewer and gate stream | `snapshots/gate-rejection-reset.json` |
| `negative.degraded-verdict` | pass | a degraded binding cannot write a gate verdict, gate and task stay open | `receipts/degraded-verdict-refusal.json` |
| `negative.ambiguous-command` | pass | a lost acknowledgement reconciles by id: one effect, original receipt | `runtime/ambiguous-command.json` |
| `negative.event-disorder` | pass | duplicates no-op, older events cannot regress, gaps block dispatch | `runtime/event-disorder.json` |
| `negative.restart` | pass | durable intent, binding and cursor reload; a generation change stays unreconciled | `runtime/restart.json` |
| `negative.worktree-park` | pass | a wrong or ambiguous worktree parks rather than proceeds | `snapshots/worktree-park.json` |
| `negative.lost-contact` | pass | stream closure and process disappearance are lost-contact, never terminal | `runtime/lost-contact.json` |
| `negative.adoption-inbox` | pass | a foreign native session is offered for adoption, never auto-bound | `runtime/adoption-inbox.json` |
| `session.history-parity` | pass | desktop and phone load identical cursor-paginated history | `session/history-desktop.json`<br>`session/history-phone.json` |
| `session.live-parity` | pass | both clients subscribe strictly after the runtime cursor and agree frame for frame | `session/live-frames.json` |
| `session.message-idempotency` | pass | the same follow-up message id twice yields one effect and one receipt | `session/idempotency.json` |
| `session.permission-idempotency` | pass | the same permission response id twice yields one effect and one receipt | `session/permission-idempotency.json` |
| `session.refetch` | pass | an epoch change and a sequence gap force refetch without mutating lifecycle | `session/refetch.json` |
| `session.no-direct-runtime` | pass | no client path reaches Paseo, AO or a runtime endpoint | `session/network-ledger.json` |
| `session.no-transcript-persistence` | pass | transcript and token canaries are absent from SQLite, export and logs | `privacy-scan.json` |
| `domain.intake-dedup` | pass | a replayed source event returns the original receipt and creates no second graph | `receipts/intake-dedup.json` |
| `domain.intake-decisions` | pass | approve, terminal reject and bounded auto-arm each admit or refuse as declared | `receipts/intake-decisions.json` |
| `domain.persona-self-approval` | pass | a persona actor cannot approve the gate it is under test for | `receipts/persona-self-approval.json` |
| `domain.profile-durability` | pass | pinned revision, phase and gate history and artifacts survive restart | `snapshots/profile-durability.json` |
| `domain.jira-asma` | pass | the ASMA workflow confirms principal and assignee by refetch before development | `jira/asma-plan.json` |
| `domain.jira-qa-distinct` | pass | internal QA readiness never projects as the external active QA status | `jira/alternate-plan.json` |
| `domain.jira-alternate` | pass | a workflow with different status names produces identical core behaviour | `jira/alternate-plan.json` |
| `domain.jira-hold-close-reopen` | pass | hold, close and reopen are deterministic and never guess a multi-hop path | `jira/hold-close-reopen.json` |
| `domain.jira-ownership` | pass | a different existing owner and every terminal assignee are preserved | `jira/ownership.json` |
| `domain.privacy-zones` | pass | Zone C stays private, owned fields project once, no outbound comment exists | `jira/privacy-zones.json` |
| `domain.inbound-comment` | pass | one inbound comment mirrors exactly once with external provenance | `jira/inbound-comment.json` |
| `domain.calendar-unrestricted` | pass | an unconfigured project is unrestricted but still needs arming | `calendar/unrestricted.json` |
| `domain.calendar-configured` | pass | closed windows, drain, holidays and override expiry admit as declared | `calendar/configured.json` |
| `domain.calendar-client-clock` | pass | no client clock influences admission | `calendar/client-clock.json` |
| `domain.ux-gate-order` | pass | the UX task cannot close before functionality QA, design QA and final audit | `snapshots/ux-closure.json` |
| `surface.parity` | pass | API, CLI and MCP report matching ids, revisions and cursors | `session/surface-parity.json` |
| `cleanup.processes` | pass | every spawned process and native session is closed or retained with a reason | `cleanup.json` |

## Detail

### `project.profiles`

five tasks resolved five distinct content-hashed bundles and each re-derived its own digest: pilot-code=code v1, pilot-ux=ux-ui-layout v1, pilot-research=research v1, pilot-docs=docs v1, pilot-incident=incident-response-v1 v1

### `project.custom-profile`

`incident-response-v1` resolved and executed as fixture data; none of its 14 pilot-specific ids appears as a literal in any shipped crate or client source file

### `project.worktrees`

five pilot tasks on five distinct verified worktrees were admitted in one ready batch: overlapping in time is not contention when the trees differ

### `project.collision-contender`

the contender shares `pilot.code` with an in-flight task and claims no tree, so the pass admitted nothing and refused it as `module_in_flight`

### `project.two-accounts`

two profiles storing nothing but `config_home` aliases resolved through separate approved homes into separate child environments — neither carrying the other's material — and both held a live seat on one runtime at the same time, each attributed to its own account-profile id. A runtime that cannot prove a per-run account environment refused the same pin with `runtime cannot prove a per-run account environment`

### `project.account-secrecy`

neither account's planted credential material nor either approved home's canonical path appears in the redacted policy, resolver, resolved-environment, profile, binding or adapter-call renderings, nor anywhere under either bundle root — while a control string this section deliberately wrote *was* found by the same scan and the same renderings still name the variable and the profile ids, so the absence is a result rather than an empty search

### `project.cross-engine`

the predecessor on runtime A sealed a `CrossEngineHandoff` capsule at `ac9415d049b6eb61`; the successor on runtime B acknowledged that exact digest, and the same acknowledgement refuses an edited capsule and refuses the run that produced it. Appending one risk changed the seal, and neither runtime will vouch for the other's binding — so the original binding stayed with A while the linkage travelled in the document

### `project.workspace-identity`

the capsule read back out of its own sealed bytes names `/w/pilot-handoff` on branch `feat/pilot-cross-engine` at baseline `45126a6`, the successor's onward capsule names the identical reference, and both runtimes prepared that same root for the same task — each under its own workspace binding, because a binding is a runtime's own attestation and does not travel

### `negative.collision`

two armed tasks over one module without distinct verified isolation refuse with `module_in_flight`; the holder's claim is untouched and no second candidate is admitted

### `negative.rejection-loop`

one reviewer rejected `design-review-gate` from two linked runs; the second rejection parked the task, closed its run as `parked` and opened a recovery episode in one committed unit, and the guarded third launch decided `park` / `second_rejection_parks` so the run count stayed at 2

### `negative.rejection-reset`

alpha's pass on the review gate cleared alpha's stream on that gate only: beta's count on the same gate and alpha's count on the code-review gate were untouched, a `started` verdict moved nothing, and alpha's next rejection of the reset gate counted one and did not park — which it would have, had the reset not happened

### `negative.degraded-verdict`

a rung-1 binding holding a role the gate authorizes was refused `block` / `verdict_rung_degraded` and wrote nothing: the gate stayed `not_ready` and the task stayed `in_progress`, both non-terminal. The identical request one rung higher was admitted `verdict_authority_held` and did write the verdict, so the refusal is about the rung

### `negative.ambiguous-command`

the send committed and then lost its acknowledgement; resubmitting the identical message id replayed the original receipt at 1:1 and left the session on one committed message across 3 adapter calls — while the same body under a fresh id committed a second, proving the id and not the content is what makes the effect exactly-once

### `negative.event-disorder`

one stream redelivered position 4 and then position 2 unchanged: both were dropped, the cursor never moved back and delivery was exactly [2, 3, 4, 5]. A second stream rewrote position 2 and was refused `the same position arrived with different content`; a third skipped to 5 and was refused `events are missing before this one`, and both refusals latched — the event that would have filled the hole was refused too. A candidate whose runtime still carries that open gap is refused `runtime_reconciliation_incomplete` by the ready pass, while the same candidate without it is admitted

### `negative.restart`

the runtime restarted from generation 1 into 2 between the message and its confirmation: the committed message, its ledger entry and the issued cursor all survived, while the old binding could neither send nor launch again. Reconciliation classified it `generation_changed` and proposed `orphaned` — an uncertainty, never a closure. Re-adopting the same native session converged it back to `keep` and left the run with exactly one native session, so nothing was launched twice

### `negative.worktree-park`

all seven worktree positions decided as declared: a moved tree and a tree nobody offered park as `worktree_moved`, an unclaimed one as `worktree_unclaimed`, two plausible trees as `worktree_ambiguous`, and only an exact pin or a single offered candidate passes — every refusal is `park`, never `block`

### `negative.lost-contact`

a live stream ended without the session reaching a terminal state, and a `succeeded` authoritative event carried over that closed channel still closed nothing — while the identical event over a reachable channel did close the run, so the refusal is about the broken channel and not about an assertion that never fires. A binding whose session discovery can no longer find is classified `missing_session` and proposed as `lost_contact`, which is uncertain and never terminal

### `negative.adoption-inbox`

a native session this control plane never launched was reported by discovery and classified `orphan` → `ProposeInboxEntry` while unlabelled and `adoptable` → `ProposeAdoption` once it carried an unbound run's label. Both propose no run state at all, and neither reached the adapter's `adopt`: the run held zero native sessions until an explicit adoption call bound one. The *durable* inbox is not wired — session adoption remains staged until one public command can atomically record the run, binding, and frozen capability snapshot

### `session.history-parity`

two clients differing only in `User-Agent` and page size loaded byte-identical canonical history: 20 items over 1 desktop page and 3 phone pages, the same epoch 1, the same anchor and the same stream digest `90748f05d1d350f9`. Deviation, stated plainly: there are no PNG screenshots and no viewport is driven here, because no browser harness exists in this tree — the console's own vitest suite exercises the viewport through `apps/console/src/test/viewport.ts`, and this driver proves the API-level invariant underneath it, which is that page size and client identity cannot change what the history *is*

### `session.live-parity`

both clients subscribed at the anchor the timeline returned (`0192f0c0-0000-7000-8000-`, sequence 20) and were delivered the same 3 frames: identical normalized kinds, identical `(epoch, sequence)` positions and identical payload digests, every one strictly after the anchor and contiguous from it — so neither client saw an item the other did not, and neither re-read an item its history already covered

### `session.message-idempotency`

two message ids were each posted twice through the daemon, plus one contradiction: 5 dispatches reached the runtime and exactly 2 messages were committed, each appearing once in the session's content. The repeat answered 200 with the byte-identical original acknowledgement — same epoch, same sequence, same `accepted_at` — rather than a fresh one; the same key with different content was refused 409 `idempotency_conflict`; and a send whose acknowledgement was lost after committing answered 503 `unavailable` and then replayed its original receipt on retry instead of sending a second message

### `session.permission-idempotency`

the same response id answered `pilot-permission-1` twice: 3 dispatches reached the runtime and exactly one `permission_resolved` event was appended to the session's content, with the pending set falling from 1 to 0. The repeat answered 200 with the byte-identical original acknowledgement rather than applying a second decision; the same id with the opposite decision was refused 409 `idempotency_conflict`; and an id this session's content never raised was refused 404 before dispatch

### `session.refetch`

an epoch change and a sequence gap were both injected and both forced `timeline_refetch_required`: a history cursor issued for this binding at a foreign epoch was refused 409 by `GET /timeline`, and a live stream whose content skipped a sequence delivered the one frame it could vouch for and then ended with an `event: error` frame carrying no id — status still 200, because the subscription was valid and the *content* is what broke. Canonical history then reloaded cleanly from the start: every item the first read validated came back in the same order with the same payload digests, and the pages that follow are the message and permission effects genuinely committed in between, not a renumbering. Through all of it the run's lifecycle, derived state, revision and `closed_at` are unchanged: a refetch is a fact about a timeline, never about a run

### `session.no-direct-runtime`

every one of the 9 URL-shaped literals in `apps/console/src` and `apps/desktop/src-tauri/src` is either a loopback daemon address or a reserved test-only host, and the full list is in the artifact so the judgement can be audited rather than believed. No source speaks `ws://` or `wss://`, constructs a `WebSocket` or an `EventSource`, or names a runtime plane; all 16 paths the typed client builds begin `/v1/`, and both of its `fetch` sites prefix the base URL the console was configured with. This is a structural proof, stated as one: it shows there is nowhere in client source to put a runtime endpoint or credential, which is a stronger claim than watching one run and seeing no such call

### `session.no-transcript-persistence`

a transcript canary was sent as a real session message, a token canary was authored by the runtime into the content the daemon relays, and a third canary was the launch prompt. Every file under the daemon's state root — the SQLite database, its `-wal` and `-shm` companions, the lock and the credential file — and both pilot bundle roots were then scanned as raw bytes: none of the three appears anywhere. The same scan for the pilot project id found it, so the empty result is a finding rather than a broken scanner. Two deviations, stated rather than glossed: the criterion also names export, and no backup or export surface exists anywhere in the daemon, store or API — an absent feature cannot carry a canary, so the criterion is answered on everything that does exist rather than blocked on something that does not; and the daemon writes no log file into its state root, emitting `tracing` to the process subscriber instead, so the log half is answered by the absence of a sink

### `domain.intake-dedup`

a replayed source event — the same identity, and separately a different identity carrying the identical envelope — returned `IntakeOutcome::Duplicate` holding the ORIGINAL receipt id both times, while the `source_events` and `intake_receipts` row counts stayed at one apiece; the same identity with different canonical bytes was refused as a conflict rather than answered from the old decision; and a receipt that tried to be a duplicate *and* carry a work graph does not validate

### `domain.intake-decisions`

three proposals were produced by `kontor_intake::evaluate`, stored, then decided through `commit_intake_decision`: approval persisted one decision and one task lineage under a real `ApproveIntake` receipt; terminal rejection persisted one decision, a reason and no task; bounded auto-arm persisted one decision and one task lineage naming its execution authorization. Approval without work and rejection carrying work are refused by `NewIntakeDecisionRecord::validate`; zero concurrency and any zero budget bound are refused by `AutoArmPolicy::validate`, and an unbounded variant does not deserialize because it does not exist. All nine `TaskOrigin::admits` outcomes matched: approved and bounded-auto-armed admit, and proposed-without-authorization, rejected, ignored and duplicate each refuse `intake_receipt_not_approved` while a missing or mismatched lineage refuses on its own code

### `domain.persona-self-approval`

the pack's own persona `simulated-patient`, acting as `therapist-verifier` — a role `functionality-qa-gate` authorizes — at the verdict rung, was refused `block` / `persona_self_approval` on the gate it is under test for, and `persona_cannot_evaluate` on every other gate; no verdict row exists

### `domain.profile-durability`

after closing and reopening the database, the task's pinned profile revision and `definition_hash` re-derived intact, the advanced phase, both gate verdicts with their sequences, principals, cited evidence and linked authority evaluation, the 3 artifact-evidence rows and the guardrail evaluation all read back byte-identical

### `domain.jira-asma`

against the shipped ASMA workflow an unowned ticket produced an assignee-only plan — `transition: None`, `assignment_prerequisite: true`, `ReassignToPrincipal` to the authenticated account — so the status cannot reach `implementation_active` before ownership converges. An observation whose answer carried no `principal_account_id` was refused `ownership_unresolved` rather than guessed at, an apply that reported `applied` with no refetched observation was refused as a malformed response, and the apply that did carry one produced a receipt with both `confirmed_at` and a `refetched_observation_id`, whose confirmed assignee is the principal and is not null

### `domain.jira-qa-distinct`

in both fixtures the internal `qa_ready` milestone targets the same status as `implementation_active` and never the externally visible QA status: a ticket sitting there with QA merely ready plans nothing at all, while the identical ticket with QA actually running converges to the project's own QA status by the route that leads there — so internal readiness cannot tell a human that review has started

### `domain.jira-alternate`

the shipped ASMA workflow and a second project sharing none of its 9 status ids were driven through the same 11-row decision matrix — stale observation, unknown status, terminal without evidence, terminal preserved, unowned, foreign owner, ready QA, active QA, hold, close and a target with no live route. Every row produced the identical decision shape and every row's external target was the project's own, which is what it means for the evaluator to have no name branch

### `domain.jira-hold-close-reopen`

across both fixtures hold and close are single deterministic outcomes — blocked work converges to the hold status by the one route that reaches it, a succeeded run with every required gate satisfied converges to the closed status, an already-closed ticket is `NoOp` and a ticket closed externally without Kontor's own evidence is `external_terminal_before_internal_evidence`. No path is ever guessed: with only an indirect route offered the answer is `no_live_transition` rather than a two-hop plan, and with two routes to one destination it is `multiple_live_transitions` rather than a choice. Reopen is deterministic too, and the determinism is that it is *not* automated: both fixtures declare a reopen selector, no milestone rule targets it, `reconcile` never reads `spec.reopen`, and a ticket sitting at that status plans nothing. The criterion asks for determinism, not for automation, so this is recorded as a pass with the absence stated rather than as a failure of a claim nobody made

### `domain.jira-ownership`

in both fixtures a ticket already held by somebody else converged its status while planning no assignment at all — under `accept_external` the existing owner is kept, not taken over — and a terminal ticket planned nothing whether it was unassigned, held by the principal or held by a stranger, because the preserve branch fires before any assignment branch can reconsider it. Three delegated plans that would have touched terminal ownership — an explicit unassign, a `preserve` action smuggling an assignee value, and an assignee that is not the authenticated principal — were each refused while building the request, so none of them reached the boundary

### `domain.privacy-zones`

with the shipped specification re-owned into all four zones, a projection carrying a Zone C value is refused as `projects a private field outward` before anything is compiled, while the same field left absent is simply omitted. Of five projected fields exactly two reached the wire — the Kontor-owned one and the mirrored one — each exactly once; the externally owned field was skipped rather than pushed and its canary appears nowhere in the request; an inbound-only field written outward and the same field projected twice are both refused by name. The absent field contributed no id and no `null`, so absence is not a clear. And no outbound comment is representable: `CommentPolicy` has one variant and the serialized `JiraRequest` has no field whose name so much as contains `comment`

### `domain.inbound-comment`

one inbound comment was mirrored once: the first append returned `true`, the same comment seen again on a later poll returned `false` and inserted nothing, and the table holds two rows only because a genuine edit under the same external comment id is kept as a second revision that names the digest it supersedes. Its external provenance survived — the author's external account id read back from SQLite, the display name, the external created and updated instants and the body digest — while the body itself appears in no evidence artifact. A revision whose digest does not match its body is refused by `verify` and by the store

### `domain.calendar-unrestricted`

a project with no assignment resolves `unrestricted`, needs no timezone and admits armed work at this instant — while the same task with its authorization removed refuses `authorization_missing`, not `calendar_closed`

### `domain.calendar-configured`

a pinned Europe/Oslo calendar resolved open, draining, closed, holiday-closed, override-open and override-expired at six instants; the scheduler refused new work while draining (`calendar_draining`) and while closed (`calendar_closed`) without touching work already admitted

### `domain.calendar-client-clock`

one snapshot decided twice across real elapsed time produced byte-identical plans, and only changing the snapshot's declared instant changed the answer: admission reads the instant the control plane recorded, never a caller's clock

### `domain.ux-gate-order`

with every phase complete, every artifact produced and the team's closure certificate presented, `ux-ui-layout` refused to close while functionality QA, design QA or the final audit were merely `active` — one at a time and all three together — and refused a waiver of the QA gate because the pinned profile allows none; it closed only once all six gates read `passed`

### `surface.parity`

the same session was read over two surfaces against one running Realm. The HTTP route and the MCP tool `kontor_session_timeline_get` returned the byte-identical timeline document — same `agent_run_id`, same epoch, same item positions, same continuation cursor `next` and same anchor. `run_show` agreed across both on the run id, the revision, the binding id, the lifecycle and the snapshot cursor, compared field by field because the run document also carries a freshness judgement about *now*. The bounded live read agreed frame for frame, ids included. Both ran in-process against the same `axum::Router` over a `Transport` written here, because the shipped `HttpTransport` needs a socket and TST-001 forbids binding one

### `cleanup.processes`

3 child process(es) were spawned, every one reaped by `Child::wait` before its call returned; 1 resolved `asma` executable(s) were deliberately never spawned because the refusal happens in `build_write_request` before `exchange`; 6 temporary director(ies) were created and all of them were observed gone after their owner dropped; no native session was opened, and the ledger says why the list is empty rather than leaving it bare

