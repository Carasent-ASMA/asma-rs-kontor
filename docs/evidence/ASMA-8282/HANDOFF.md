# ASMA-8282 handoff — `planning_pair@1`

- Task: ASMA-8282 / TASK-004
- Workspace: TSW `wks_8062e92dee85f5b3`
- Branch: `feat/ASMA-8282-qualify-shared-advisor-and-committee-protocols`, local, not pushed, no pull request
- Orchestration: paseo-direct (plan `2026-09-27-20-14-plan-shared-orchestration-workflow.md`). No Kontor step, receipt or obligation is proposed here.
- Slice one (2026-09-29): the domain and the shared reader. It was fast-forwarded from `173d399bfd44ec598332b4fe1cdf882af024fe5c` to the published `739debaacfb0ace894b36114aaa1eef6835cdfc1` (`origin/feat/ASMA-8278-shared-orchestration-workflow`) before any feature edit, and committed as `d876055ba73f6d56577a60782888cfe4a0209df8` (tree `0b5b2cce8fbb9bdfaf70daf4e6265f48ef6139c3`).
- Slice two (2026-09-30): the direct CLI `planning_pair@1` mode, committed as `d9dce9c8ecb8f529de4e3967e87ab25f78af112a` (tree `5deed40edebddcce49d6496f61b8cb3a744f92ed`). It is built on the accepted `d876055b` (tree `0b5b2cce`) and was dispatched by TPM generation 2 `7fc26706-dce2-4a02-be87-e0f933b25fd4` (predecessor `3c52abda-4fd0-4af6-b086-476bc51eb305`, archived). Every ancestor, `d876055b` included, is unchanged.
- Slice three (2026-10-02): immutable profile identity, the run's durable record and MUT-003, with the reserved governed decisions reported rather than adopted. It is built on the accepted `d9dce9c8` (tree `5deed40e`), was dispatched by TPM successor `47cb9a74-a798-4f40-9b54-2a818afe0a42`, and was committed as `3b05abce237593b88d4bb83fc41440967479dd8e` (tree `7c92ee34dd825b1feaa7f46583174eb0ea2258d6`). Verify passed and audit 6f failed it: a sealed finding was readable through the public `PlanningPairRun::record()`.
- Audit remediation (2026-10-02, this revision): it closes every public read path to a sealed finding or answer, and adds nothing else. It is built on `3b05abce` (tree `7c92ee34`) and dispatched by the same TPM. The writing seat is Paseo agent `45d0acc7-0773-4f43-858f-c6d6c8eecf78`, Claude session `3538e45e-113f-432b-9d1e-57e0d1b4af96`, in project `prj_e9f8052597f78919` and TSW `wks_8062e92dee85f5b3`. Every ancestor, `3b05abce` included, is unchanged.
- Slice four (2026-10-02): the additive governed slice under the D-1 to D-3 disposition. It is built on `832e60cc` (tree `678333e4`) after native verify `b887` and audit `6f` passed on that exact head. D-1 is checkpoint `8a27a851` (tree `d73c5f51`); D-2 and D-3 are the next checkpoint. The writing seat is the same Claude session `3538e45e-113f-432b-9d1e-57e0d1b4af96` in project `prj_e9f8052597f78919` and TSW `wks_8062e92dee85f5b3`. Every ancestor is unchanged. See "Slice four" below.
- Audit 6f rework (2026-10-02): the independent 6f audit failed `4922a9a2` (tree `fcfcbdd5`), because a member role was resolved from the build's first catalog rather than the epic's selection. The rework is built on `4922a9a2` and dispatched by TPM, from the same writing session. It is bounded to the catalog authority fix, the selected-container safeguards and a concurrent-resume regression. See "Audit 6f rework" below.
- Audit 6f turn-5 rework (2026-10-02): the independent 6f audit failed `420f82f5` (tree `2f49d3cb`). Two same-key requests could both answer `created` after the compare-and-swap. The rework is built on `420f82f5`, dispatched by TPM from the same writing session, and bounded to atomic receipt classification, its deterministic regression and mutation evidence, and the handoff. See "Audit 6f turn-5 rework" below.
- D-3 member surface (2026-10-02): TPM dispatch from the accepted `b3457c31` (tree `169542e6`), after verify and audit passed. It covers the member guard, the serve profile, the typed member launch context and its readback, and route-specific capability, with mock and disposable-fixture tests only. See "D-3 member surface" below.
- D-3 authority repair (2026-10-02): verify b887 on `1b1d5597` passed the composition and confirmed an authority hole, and the LSA granted no waiver. The repair: a member binds only when every mandatory member-surface field matches; every real Paseo route is refused before freeze; and Claude composition is proved on fixtures only. See "D-3 authority repair" below.
- Frontier B (2026-10-02): the opt-in `planning_pair_caller` source profile, built on `8b6ee064` (tree `aef842f6`) after verify and audit 6f passed with no findings. It covers the registry profile and explicit selection, plus tests and docs. No composition and no daemon source change. See "Frontier B" below.
- Frontier A (2026-10-02): the same-native member reconcile seam, at source level only. It is built on `e7a9404a` (tree `62377b57`) after verify and audit passed. It covers an opt-in runtime port that is typed unsupported on every real runtime, a hypothetical fake, the second-seat-only and both-routes fixtures, and docs. The daemon recovery operation's authority is returned to the LSA. See "Frontier A" below.
- Frontier A operation (2026-10-02): the frozen caller's same-native member recovery, under the LSA decision. It is built on `75f253cb` (tree `d9e728c7`) after its bounded verify and audit passed. It covers the operation, the forward migration 0123 with the pair's known-native claim, tests and docs, at source and fixture level only. See "Frontier A: the frozen caller's same-native member recovery" below.
- The untracked directory `docs/evidence/KON-MVP-18/run-4d1b209d3fa9ea8e/` is disclosed e2e test evidence from slice one's workspace run. It is not part of any commit and is preserved untouched. At slice four it holds 53 files, which hash to `4ff4c1bc5cbe7c7ce505442d50827f91d64b0bb308cd18183a39d42339e49e87`: each file's SHA-256 in sorted path order, hashed again. Its files were last written at 2026-10-02 00:25 CEST. That run was not this writing session's (its only workspace run was on 2026-09-29), and slice four did not touch them. The untracked `.agents/`, `.asma/`, `.cursor/`, `AGENTS.md` and `CLAUDE.md` are adapter installations owned by others, and are likewise untouched and uncommitted.

This record is implementation evidence and a handoff. It is not verification. It
does not close TASK-004 or TASK-002, and it claims no mutation acceptance.

## Consumed, not reopened

- The qualified identity source `e29b895d0f045c499144870a2fa610fc642b6cc0` and the deployed recovery source `173d399bfd44ec598332b4fe1cdf882af024fe5c` are ancestors of this slice and are not modified. The consultation identity (`ConsultationIdentity`, `ConsultationFamily`) and consultation recovery code paths are untouched. The only change to `crates/kontor-core/src/consultation.rs` makes its private `has_duplicate` helper `pub(crate)`, so the planning pair's list rules reuse it.
- ASMA-8113 stays open.
- OG-01 through OG-05 and ASMA-8101 contract items 11–12 are out of this slice.

## Slice one — decisions (Igor, 2026-09-29)

| Question | Decision |
| --- | --- |
| Surfaces | Domain (`kontor-core`) and the shared reader (`kontor-fleet-activation`) only. No CLI, registry, daemon, store, API, OpenAPI or migration change. |
| Member binding keys | The caller names one existing binding key per member (`team/…`, `committee/…`, `advisor/…` or `leadership/…`), exactly as joint `--allocation` slots do. There is no new key family and no parser or policy-schema change. |
| Handoff | This new file. `docs/evidence/ASMA-8280/DIRECT-MODE-HANDOFF.md` is unchanged. |
| Final run | `cargo test --workspace --no-fail-fast` once on the final tree. |

## Slice one — what is implemented

| Element | Source | What it does |
| --- | --- | --- |
| Explicit protocol selection | `crates/kontor-core/src/planning_pair.rs`: `ConsultationProtocol` (`advisor`, `planning_pair@1`, `independent_review`), `select_protocol` | The caller names the protocol. `select_protocol(None, …)` is refused (`NO_PROTOCOL`), and a protocol that is not available is refused (`UNAVAILABLE`). The answer is never a different protocol, so a planning pair never stands in for an Independent Review, and an Advisor never stands in for either. |
| No gate from advice | `ConsultationProtocol::is_formal_review`, `require_formal_review` | Only `independent_review` passes. An Advisor or a planning pair is refused with `MissingAuthority { subject: "FormalReviewGate", rule: NOT_FORMAL }`, even after a unanimous `accepted` disposition. The existing completion gate (`kontor-scheduler` `CompletionObservation::VerdictRecorded`) is unchanged. It accepts only a `CommitteeVerdict` and a `CommitteeRunId`, and a planning pair has neither. |
| Document | `PlanningPairSpec`, `PlanningPairMemberSpec` | `protocol` must be `planning_pair@1` (`NOT_PLANNING_PAIR`). `members` is exactly `seat-a` then `seat-b` (`MEMBERS`), titled `SEAT A` / `SEAT B` (`PlanningPairSlot::label`). Each member has only `specialty`, `behavior` and the existing pin-only `ConsultationContextPolicy`. There is no `judge`, `aggregation`, `quorum`, `diversity`, `round_limit` or clarification-count field, and `deny_unknown_fields` refuses each one. `validate` / `canonicalize` follow the Advisor and Committee rules for callers, scopes and budget. The document has no id, revision or registry yet (see "Remaining gaps"). |
| Protocol bounds | `FINDINGS_ROUNDS = 1`, `MAX_CLARIFICATION_ROUNDS = 1` | These are fixed by the protocol version, not by document data. A successor that changes a bound is a new protocol version. |
| Frozen members | `PlanningPairMember`, `PlanningPairMembers::freeze` | Exactly two members, `seat-a` then `seat-b`, each with the caller-named `binding_key`, the chosen `route` and the actual `vendor`. `freeze` refuses an empty or `unknown` vendor (`VENDOR_UNKNOWN`) and two members on the same vendor (`SAME_VENDOR`), whatever their account aliases or harnesses. The members carry the `placement_hash` of the receipt they were frozen from. Nothing can change a member once frozen: there is no setter, no replacement and no third slot. |
| One independent findings round | `PlanningPairRun::admit`, `record_finding`, `findings`, `state` | Only `Member(slot)` records that slot's finding (`MEMBER_ONLY`), and only once (`FINDING_IMMUTABLE`). `findings()` is `None` until both findings are durable, so neither the caller nor the other member can read a partial round. The run is deliberately not `Serialize`, so no rendering can leak one finding early. Each contribution's `document_hash` is canonical and bound to the document hash, the placement hash, the round and the slot. |
| At most one caller-requested clarification | `request_clarification`, `record_answer`, `clarification` | Only the caller asks (`CALLER_ONLY`), only after both findings are recorded (`FINDINGS_INCOMPLETE`), and only once (`EXTRA_CLARIFICATION`, both while the round is open and after it is answered). The question addresses one or both members, each once (`ADDRESSEES`). Only an addressed member answers (`NOT_ADDRESSED`), once (`ANSWER_IMMUTABLE`), and never without a question (`NO_CLARIFICATION`). Answers are released only when every addressed member has answered. |
| Caller disposition, dissent retained | `PlanningPairDisposition`, `MemberDisposition`, `record_disposition`, `retained_dissent` | The caller records one decision, which reuses the Advisor's `AdviceDisposition` vocabulary. It waits for every requested answer (`ANSWERS_INCOMPLETE`). It must list `seat-a` then `seat-b`, each citing its exact finding hash and its exact answer hash, or none if it gave none. Omitting, reordering, duplicating or rewriting a member is refused (`DISSENT_LOST`). `superseded` is refused because this is the only decision (`FIRST_DISPOSITION`). The disposition has no verdict, gate, aggregate or settlement field. After it, every operation is `Terminal`. `retained_dissent()` returns every finding and answer of a member the caller rejected or only partly accepted, verbatim. Both findings stay readable. |
| No Judge, quorum, conjunctive verdict or settlement | whole module | `PlanningPairState` is `awaiting_findings`, `findings_released`, `awaiting_answers`, `answers_released` and `disposed`. No state is settled, and nothing computes an outcome from the findings. |
| Placement from the activated snapshot | `crates/kontor-fleet-activation/src/lib.rs`: `PlanningPairRequest`, `PlanningPairMemberRequest`, `PlanningPairPlacement`, `Activated::place_planning_pair`, `place_planning_pair` | The request is `{members: [{slot, binding_key, unavailable_accounts?, excluded_vendors?}]}` and has no diversity or role field, so neither can be waived or set to Judge. PP-01 refuses anything but `seat-a` then `seat-b`. The request becomes a `JointAllocationRequest` with both members as `reviewer` under `distinct_vendor_per_reviewer`, and runs through the existing `Activated::allocate`: one `load`, each key resolved by `Activated::resolve`, and `kontor_fleet::allocate` choosing. There is no second parser, resolver or allocator. The independence key is the policy's model vendor, so aliases on one maker, or a Cursor route to an Anthropic model beside Claude, count as one vendor, and an `unknown` maker cannot be placed. |
| Receipts | `PlanningPairPlacement { protocol, selection, placement_hash, members? }` | `selection` is the existing `JointSelection`, verbatim: `ActivationProvenance` (policy hash and schema, and under a v2 activation the bundle and roster hashes), and each member's binding, chain, policy exclusions, eligibility, considered candidates and choice. `placement_hash` is the canonical hash of `{schema_version: 1, protocol: "planning_pair@1", selection}`. `members` is present only when both were placed. A blocked allocation is the defined block result, and nothing is frozen from it. PP-02 covers the unreachable case where the receipt is not canonical or the placed members do not freeze. |

## Slice one — tests

`crates/kontor-core/tests/planning_pair.rs` (18) and
`crates/kontor-fleet-activation/src/tests.rs` (4 new, plus PP-01 in
`every_rule_string_is_stable`). Every negative test asserts its exact rule.

| Required negative | Tests |
| --- | --- |
| Same vendor | `members_on_one_actual_vendor_are_refused` (two aliases; Cursor and Claude on one maker), `a_member_without_a_known_vendor_is_refused`; `a_planning_pair_on_one_actual_vendor_is_blocked` (one Anthropic-only chain; two OpenAI accounts; Cursor→Anthropic beside Claude), `a_planning_pair_member_with_an_unknown_vendor_is_not_placed` |
| Missing member | `a_pair_missing_a_member_is_refused` (one, none, reversed, repeated and three members, in the document and at freeze), `a_findings_round_missing_a_member_releases_nothing`; `a_planning_pair_missing_or_repeating_a_member_is_refused` (PP-01, and D-03 for an unresolvable member binding) |
| Mutation attempt | `a_member_grant_cannot_name_an_authority` (`capabilities`, `allowed_operations`, `read_only`, `gate_waiver` refused on the member and on its grant), `a_recorded_finding_or_answer_cannot_be_rewritten`, `a_member_cannot_speak_for_the_other_seat_or_the_caller`, `a_disposed_pair_is_immutable` |
| Extra clarification | `a_second_clarification_round_is_refused`, `only_an_addressed_member_answers_and_only_when_the_caller_asked` |
| Advice presented as a gate | `advice_cannot_satisfy_a_formal_review_gate`, `a_disposition_cannot_carry_a_verdict`, `protocol_selection_is_explicit_and_never_substituted`, `a_planning_pair_has_no_judge_quorum_or_verdict_to_declare` |
| Lost dissent | `a_disposition_that_omits_or_reorders_a_member_is_refused`, `a_disposition_citing_a_rewritten_finding_or_answer_is_refused`, `dissent_survives_the_callers_decision` |

Positive tests: `one_findings_round_one_clarification_then_the_callers_disposition`, and
`a_planning_pair_is_one_joint_allocation_on_two_actual_vendors`. That test asserts that the
placement's `selection` equals `Activated::allocate` for the same two slots, that both
entry points agree, and that seat B's receipt records `vendor_held` with
`conflicts_with: seat-a`.

### Local mutant checks (preparation only)

On the uncommitted tree, each mutant below was applied alone, its crate's test target
was run, and the source was restored and touched. All 13 were killed:

| Mutant | Killed by |
| --- | --- |
| `freeze` same-vendor check dropped | `members_on_one_actual_vendor_are_refused` |
| `freeze` admits vendor `unknown` | `a_member_without_a_known_vendor_is_refused` |
| `findings_complete` always passes | `a_findings_round_missing_a_member_releases_nothing` |
| second clarification admitted | `a_second_clarification_round_is_refused` |
| finding rewrite admitted | `a_recorded_finding_or_answer_cannot_be_rewritten` |
| member authority skipped | `a_member_cannot_speak_for_the_other_seat_or_the_caller` |
| caller authority skipped | `a_member_cannot_speak_for_the_other_seat_or_the_caller` |
| planning pair passes `require_formal_review` | `advice_cannot_satisfy_a_formal_review_gate` |
| disposition hashes not compared | `a_disposition_citing_a_rewritten_finding_or_answer_is_refused` |
| disposition may omit a member | `a_disposition_that_omits_or_reorders_a_member_is_refused` |
| `select_protocol` substitutes the first available protocol | `protocol_selection_is_explicit_and_never_substituted` |
| placement diversity set to `none` | the three placement tests that need distinct vendors |
| placement member order unchecked | `a_planning_pair_missing_or_repeating_a_member_is_refused` |

These checks only show that the named killers exist. They are not the supported
mutation evidence. MUT-003 ("missing finding or same-vendor collision passes a gate")
still needs the supported run on the reviewed candidate. Its sites in this slice are
`PlanningPairMembers::freeze`, `Activated::place_planning_pair` (the diversity),
`PlanningPairRun::findings_complete` and `ConsultationProtocol::require_formal_review`.

### Slice one gates

- `cargo fmt -p kontor-core -p kontor-fleet-activation -- --check`: clean. The format was applied per crate, and only the new code changed.
- `cargo clippy -p kontor-core -p kontor-fleet -p kontor-fleet-activation --all-targets -- -D warnings`: clean.
- `cargo test -p kontor-core --test planning_pair`: 18 passed. `cargo test -p kontor-fleet-activation`: 18 passed.
- `cargo test --workspace --no-fail-fast` on tree `0b5b2cce`: exit 0, with 156 targets, 3043 passed, 0 failed and 9 ignored (recorded in the body of `d876055b`).

## Slice two — the direct CLI `planning_pair@1` mode

### Choices made inside the dispatch, for review

The dispatch did not specify these. Each one is reversible, and none is presented as a substitute decision.

1. **Mode selector.** The existing operation takes one more optional argument, `planning_pair` (flag `--planning-pair`), and its presence selects the mode. The protocol version is not a request field: the answer names it (`protocol: "planning_pair@1"`), and a successor would be a new, explicitly selected mode.
2. **Existing refusals stay byte-identical.** `binding_key` with `allocation`, and no mode at all, are still J-01 with the same rule text and action. PP-03 covers only the combinations that include `planning_pair`. As a result, a request that names no mode is still told about only the two older modes; the operation's schema and help describe all three.
3. **PP-04 is a rule of its own** rather than reusing J-02, whose text speaks of a joint allocation and its slots.
4. **The block result's key is `placement`**, beside the existing `selection` and `allocation`.
5. **Rule texts.** PP-03 and PP-04 live in `kontor_fleet_activation::rule`, beside J-01 and J-02, which are also CLI-only.

### What is implemented

| Element | Source | What it does |
| --- | --- | --- |
| Request mode | `crates/kontor-mcp/src/registry.rs`: the `planning_pair` argument of `kontor_fleet_policy_resolve`, and `FLEET_PLANNING_PAIR` / `FLEET_PLANNING_PAIR_MEMBER` | This is a declared, closed nested schema: `{members: [{slot: seat-a\|seat-b, binding_key, unavailable_accounts?, excluded_vendors?}]}`. It has no diversity, role or Judge field. The operation is otherwise unchanged: same name, `Execution::Local(LocalOperation::FleetPolicyResolve)`, `OpKind::Read`, operator tier (admin inherits it, and an observer is refused), no route and no new handler. There is no second operation or control plane. The descriptions of `binding_key`, `allocation` and the operation now name the third mode. |
| Mode exclusivity | `crates/kontor-cli/src/local.rs` `fleet_policy_resolve`; PP-03 and PP-04 | `planning_pair` with `binding_key`, `allocation` or both is PP-03, and `planning_pair` with top-level eligibility is PP-04. Both are `invalid_request`, exit 2, dispatched false. The planning pair branch is checked first, and the single and joint code paths after it are unchanged. |
| Handler | `local.rs` `fleet_policy_planning_pair` | It deserializes the shared `PlanningPairRequest` and calls `kontor_fleet_activation::place_planning_pair`: one `load`, then `Activated::place_planning_pair`, then `Activated::allocate`, then `kontor_fleet::allocate`. There is no second parser, resolver or allocator. It prints `{tool, status: 200, body: PlanningPairPlacement}` verbatim. With no complete placement it prints `status: 409`, `placement_blocked`, with the same document under `placement` and no `members`, and exits 1. Reader refusals go through the existing `refuse`: PP-01 and PP-02 are `invalid_request` (exit 2), and D-03 and the other absent-artifact rules are `not_found` (exit 6). |
| Placement evidence, not a gate | the answer body | The body has exactly `protocol`, `selection`, `placement_hash` and `members`, and no verdict field. The CLI writes, records and settles nothing. Independent Review remains the only formal gate. |
| Documentation | `docs/CONFIGURATION.md`, "Direct-mode resolution without a daemon" | Documents the mode, its refusals and its answer. `docs/evidence/ASMA-8280/DIRECT-MODE-HANDOFF.md` is unchanged. |

No daemon, store, API, OpenAPI, console, migration or root/ECP file changed.

### Tests

`crates/kontor-cli/tests/local_resolve.rs` has 4 new tests (9 in total). The registry
test `every_operation_is_exactly_one_route_or_exactly_one_local_handler` now declares
`planning_pair` optional and checks its closed schema: `slot` is `["seat-a", "seat-b"]`,
`slot` and `binding_key` are required, and there is no `diversity`, `role` or `judge`.
`every_rule_string_is_stable` pins PP-03.

| Test | What it proves |
| --- | --- |
| `a_planning_pair_is_one_placement_through_the_shared_reader` | The body equals `kontor_fleet_activation::place_planning_pair` verbatim, and its `selection` equals the `--allocation` answer for the same two reviewer slots, so no second allocator exists. SEAT A takes `codex-work`/openai and SEAT B takes `claude-personal`/anthropic, and SEAT B's receipt records `vendor_held` with `conflicts_with: seat-a`. `members.placement_hash` equals `placement_hash`, and the provenance names the bundle. The body has exactly four keys and none is a verdict. Admin equals operator, and an observer is refused before anything is read. |
| `a_planning_pair_on_one_actual_vendor_is_blocked_whole` | With Anthropic excluded, or the Claude account unavailable, both members can reach only OpenAI. The answer is exit 1, `status: 409`, `placement_blocked`, `blocked: no_distinct_reviewer_vendors`, with no member frozen and no slot selected, and it equals the shared reader's block result. |
| `a_planning_pair_names_seat_a_then_seat_b_and_resolvable_bindings` | Reversed, single, repeated, three and empty member lists are each PP-01 (exit 2). A member binding the activation does not bind is D-03 (`not_found`, exit 6). |
| `the_three_request_forms_are_mutually_exclusive` | `planning_pair` with `binding_key`, with `allocation`, and with both is PP-03. `binding_key` with `allocation`, and no mode, stay J-01. Top-level `--unavailable-accounts` or `--excluded-vendors` with `planning_pair` is PP-04. The declared schema refuses a member `role`, a `judge` slot, a pair-level `diversity`, a member without `binding_key`, and a pair without `members`, before anything is read. |

### Slice two local mutant checks (preparation only)

On the uncommitted tree, each mutant was applied alone, the named target was run, and
the source was restored and touched. All 7 were killed:

| Mutant | Killed by |
| --- | --- |
| PP-03 exclusivity check dropped | `the_three_request_forms_are_mutually_exclusive` |
| PP-04 top-level eligibility admitted | `the_three_request_forms_are_mutually_exclusive` |
| blocked placement printed as `status: 200` | `a_planning_pair_on_one_actual_vendor_is_blocked_whole` |
| planning pair branch skipped (falls through to J-01) | all four new tests |
| shared reader's diversity set to `none` | `a_planning_pair_is_one_placement_through_the_shared_reader`, `a_planning_pair_on_one_actual_vendor_is_blocked_whole` |
| registry `slot` admits `judge` | `the_three_request_forms_are_mutually_exclusive` |
| registry member admits a `role` field | `every_operation_is_exactly_one_route_or_exactly_one_local_handler` |

These are not the supported mutation evidence, and MUT-003 is still unperformed.

### Slice two gates

- `cargo fmt -p kontor-cli -p kontor-mcp -p kontor-fleet-activation -- --check`: clean. The format was applied per crate, and only the new test code changed.
- `cargo clippy -p kontor-cli -p kontor-mcp -p kontor-fleet-activation --all-targets -- -D warnings`: clean.
- On the final code: `cargo test --no-fail-fast -p kontor-cli` gave 33 passed (21 binary unit, 1 `fleet_bundle`, 9 `local_resolve`, 1 `memory_parity`, 1 `version`). `-p kontor-mcp --lib` gave 67 passed, and `-p kontor-fleet-activation` gave 18. `-p kontor-core --test planning_pair` gave 18. `-p kontor-tests-contract --test mcp_parity --test mcp_cardinality --test mcp_mutants` gave 12 + 12 + 12 passed. No test failed.

## Slice three — immutable identity, durable record and MUT-003

### Reserved decisions, reported to TPM and LSA (not adopted)

> **Disposed since (2026-10-02).** The LSA (`b50b37c0-1977-4f73-a221-3c95c367dfbc`) disposed D-1 (option A), D-2 (the registered operation table) and D-3's source contract for bounded implementation. The TPM records that disposition as the sole plan writer. Actual selected capability, release and live qualification stay fenced, and the ASMA-8113 owner's compatibility review is an acceptance fence on the changed identity vocabulary. The report below is kept as written at `3b05abce`. The audit remediation adds no storage, operation or identity change. The additive slice follows only after verify and audit pass on the repaired candidate.

The dispatch asked for governed persistence, identity and operations "reusing existing
consultation identity, uniqueness, store and registered application operations". The
existing conventions do not establish where a planning pair sits in them. Each item below
is a decision this seat must not take. Nothing in this slice depends on any of them.

**D-1 — Publication and identity boundary.** It blocks persistence, the run id, the
semantic identity and every governed operation. The existing consultation surface is
closed to two families:

- **Database:** `consultation_profile_revisions.family` (migration 0034), `consultation_runs.family` (last rebuilt in 0070) and `consultation_topic_corrections.family` (0091) are each `CHECK (family IN ('advisor', 'committee'))`. Migration 0034 says the family is closed in SQL "so a caller that reached the database directly still cannot invent a third consultation family".
- **Run state:** `consultation_runs.state` is closed to `materializing`, `running`, `awaiting_judge`, `settled` and `needs_human`, and `settled_at` is tied to `settled`. A planning pair has no settlement and ends `disposed`.
- **Command kinds:** the closed kind list (last rebuilt in 0108) names `apply_advisor_profile`, `apply_committee_template`, `invoke_advisor_run`, `invoke_committee_run`, `record_committee_findings`, `settle_advisor_run`, `settle_committee_run`, `recover_consultation_seat` and `reroute_unmaterialized_consultation_seat`, and nothing for a planning pair.
- **Rust:** `ConsultationFamily` and `ConsultationRunId` have two variants each. 82 lines under `crates/*/src` name a variant (counted by grep; inline test modules included), 63 of them in `crates/kontor-daemon/src/applications.rs`. The rest are in the core repository port, the store, the runtime adapter and the fake runtime.
- **Native correlation:** the Paseo label `kontor.consultation_run` is `<family>/<run uuid>`, and launch, recovery and readback correlation all match it.
- **Identity:** `ConsultationIdentity::hash` (ASMA-8113, consumed at `e29b895d`) hashes `family.as_str()`.

The options:

- **A. A third family, `planning_pair`, through the same tables, identity function, labels and command kinds.** This needs one migration that widens the three family CHECKs, either the run-state vocabulary (`disposed` and the planning-pair states) or a planning-pair state table of its own, and the command kinds. It also needs `ConsultationFamily::PlanningPair`, `ConsultationRunId::PlanningPair(<PlanningPairRunId>)` and every match site. It reuses the semantic-identity uniqueness index `(project_id, semantic_identity_hash)`, topic validation, naming, seat bindings and consultation recovery. Existing Advisor and Committee identity hashes would not change, because the family is a hashed string input. It needs the ASMA-8113 owner to accept widening the identity vocabulary. **This seat recommends A.**
- **B. A Committee-family template with a protocol discriminator.** This is rejected by the contract: a planning pair would become addressable as a Committee, which is the alias with Independent Review this task forbids.
- **C. Dedicated planning-pair tables and a separate identity.** This leaves ASMA-8113 untouched, but duplicates the uniqueness, naming, seat and recovery conventions the dispatch says to reuse.

The decision owner is the LSA, consulting the ASMA-8113 owner. If it is recorded as an ADR, acceptance is Igor's (AGENTS.md, "Decision records").

**D-2 — Governed operation contract.** This depends on D-1. No registered operation accepts a planning pair. The decision must fix:

- the route family and names, for example profile preview, apply and list; run invoke and get; member finding and answer recording; and the caller's clarification and disposition;
- the tiers: Admin publication, as for Advisor and Committee profiles; Operator invocation and caller acts; seat-scoped member writes;
- the idempotency command kinds and their fingerprints;
- the receipt and outcome shapes.

Mirroring `kontor_advisor_*` and `kontor_committee_*` is the obvious starting shape, but the clarification and disposition steps have no existing counterpart, so they are a contract decision, not a copy.

**D-3 — Member placement and launch.** This depends on D-1 and D-2 and is outside this seat's authority. A governed finding or answer comes from a member seat, authenticated by a seat-scoped credential on its SeatBinding inside a CSW-like container. Creating those seats is TPM-owned placement, and native workspace and seat creation, which this dispatch forbids inventing. Until it exists, a governed planning pair cannot record a member's finding with authenticated provenance.

### What is implemented (decision-neutral)

Everything below is what options A and C would both persist. No table, migration, run id,
family variant, command kind, route, MCP tool or OpenAPI change is added. The direct CLI,
daemon, store, API and every other crate are unchanged.

| Element | Source | What it does |
| --- | --- | --- |
| Profile identity | `crates/kontor-core/src/id.rs` `PlanningPairProfileId`; `planning_pair.rs` `PlanningPairSpec.profile_id` and `.version`, `PlanningPairSpec::pin`, `PlanningPairPin` | A UUID v7 entity id, as for `AdvisorProfileId`, and its own type, so it can never be read as a Committee template id. A revision is `(profile_id, version, definition_hash)`, the Advisor and Committee convention. Version one is the floor because `SpecVersion` cannot hold 0, so the document needs no check of its own. The canonical hash covers the id and version, so another revision or another document never hashes equal. |
| Run pin | `PlanningPairRun::pin`; `spec_hash()` is now the pin's `definition_hash` | A run records exactly the revision it was convened under. Contribution hashes still bind the definition hash, which now covers the id and version. |
| Durable record | `PlanningPairRecord`, `RecordedContribution`, `RecordedClarification`; `PlanningPairRun::record` (public at `3b05abce`; module-private since the audit remediation), `PlanningPairRun::restore`; `PlanningPairRecord::canonicalize`, `PlanningPairRecord::from_stored` | The record is a closed canonical document (`deny_unknown_fields` throughout) of the pin, the placement hash, both members, the question, the findings in slot order, at most one clarification with its answers, and the disposition. It has no field for a Judge, verdict, aggregate, settlement or second round. `from_stored` holds stored bytes to their canonical form and address (`CanonicalDocument::from_stored`). `restore` trusts nothing: it refuses another protocol (`NOT_PLANNING_PAIR`) or another revision (`RECORD_PIN`), freezes the members again, admits the run again, and replays every finding, the clarification, each answer and the disposition through the live transitions. So any rule above fails on its own rule. Each contribution must hash to its recorded address (`RECORD_HASH`), and the record must be the one the replayed run writes (`RECORD_NOT_CANONICAL`). |
| Sealed findings stay sealed | `PlanningPairRun` is still not `Serialize` | **Defect at `3b05abce` (audit 6f): this claim was false.** The public `PlanningPairRun::record()` returned the sealed finding, as did the derived `Debug`. See "Audit remediation" below. As written then: the record necessarily holds a finding while the round is sealed. It is a store's form, not a release, and a restored run releases findings only through `findings()`, exactly as the live run does. |
| Members | `PlanningPairMember` now derives `Deserialize` with `deny_unknown_fields` | Deserializing one admits nothing; only `PlanningPairMembers::freeze` does. |

### Tests

`crates/kontor-core/tests/planning_pair.rs` has 5 new tests (23 in total). The fixture
now uses one fixed profile id, so every `spec()` is the same revision.

| Test | What it proves |
| --- | --- |
| `a_document_revision_is_identified_by_its_id_version_and_hash` | The pin is the fixed id, version one and the canonical hash, and the run holds it. Version two and another id each change the pin and the hash. Version `0` and a non-UUID id cannot be parsed. |
| `a_run_survives_the_store_at_every_point_in_its_life` | Canonical bytes are read back and restored equal at every state: admitted, one sealed finding, findings released, awaiting an answer, answered, disposed. A restored one-finding run still releases nothing. A restored disposed run is `Terminal`, as the live one is. |
| `a_stored_run_under_another_revision_or_protocol_fails_closed` | Another version or another id is `RECORD_PIN`. `protocol: independent_review` or `advisor` is `NOT_PLANNING_PAIR`. |
| `a_stored_run_that_breaks_a_rule_fails_closed_on_that_rule` | Nine tampered records each fail on their own rule: a rewritten finding (`RECORD_HASH`), seat B moved onto seat A's vendor (`SAME_VENDOR`), seat B dropped (`MEMBERS`), seat A's finding twice (`FINDING_IMMUTABLE`), seat B's finding missing (`FINDINGS_INCOMPLETE`), an answer from an unaddressed member (`NOT_ADDRESSED`), the requested answer missing (`ANSWERS_INCOMPLETE`), seat B's dissent dropped from the disposition (`DISSENT_LOST`), and the findings out of slot order (`RECORD_NOT_CANONICAL`). The untouched record restores. |
| `a_stored_record_has_no_room_for_a_judge_a_verdict_or_a_second_round` | `judge`, `verdict`, `aggregate`, `settled`, `clarifications` and `round` are refused, and so is a stored member gaining `capabilities`. Re-indented bytes, and bytes under another address, are refused by `from_stored`. |

### MUT-003 (seeded and run on this slice's candidate source)

MUT-003 is "missing finding or same-vendor collision passes a gate". It was run on this
slice's final source before the TPM-routed verify and audit, following the ASMA-8117
MUT-002 convention:

1. one mutant at a time;
2. the file's SHA-256 captured before seeding;
3. one command with `--exact`, whose `--list` enumerates exactly the named tests;
4. the result observed red with the mutant in place;
5. the file restored from an untouched copy, confirmed by its SHA-256, and touched;
6. the same command rerun green.

Sites a–e are the planning pair's. Sites f and g are the formal Independent Review
evaluator's, the only gate there is.

| Id | Site (file:line, unmutated SHA-256) | Mutant | Command filter (`--exact`) and `--list` count | Observed red | Restored green |
| --- | --- | --- | --- | --- | --- |
| MUT-003-a | `crates/kontor-core/src/planning_pair.rs:406`, `PlanningPairMembers::freeze`, `c2b607e4b8adbe506fec9479191225d8eb9a1ba950f148adc2f7711914df233d` | the same-vendor check becomes `if false && …` | `-p kontor-core --test planning_pair -- members_on_one_actual_vendor_are_refused a_stored_run_that_breaks_a_rule_fails_closed_on_that_rule`, 2 listed | 0 passed, 2 failed: `left: Ok(PlanningPairMembers { … })`, `right: Err(Invalid { subject: "PlanningPairMembers", rule: "the two members reach the same actual vendor, …" })`; and the stored case "seat B moved onto seat A's vendor" | 2 passed; SHA-256 equal |
| MUT-003-b | same file `:844`, `PlanningPairRun::findings_complete`, same SHA-256 | the guard becomes `if false` | `-- a_findings_round_missing_a_member_releases_nothing a_stored_run_that_breaks_a_rule_fails_closed_on_that_rule`, 2 listed | 0 passed, 2 failed: `left: Ok(())`, `right: Err(MissingEvidence { …, rule: "both members' findings must be recorded before the caller acts on either" })`; and the stored case "seat B's finding missing" | 2 passed; SHA-256 equal |
| MUT-003-c | same file `:129`, `ConsultationProtocol::is_formal_review`, same SHA-256 | `matches!(self, Self::IndependentReview)` becomes `!matches!(self, Self::Advisor)` | `-- advice_cannot_satisfy_a_formal_review_gate`, 1 listed | 0 passed, 1 failed: `assertion failed: !protocol.is_formal_review()` | 1 passed; SHA-256 equal |
| MUT-003-d | `crates/kontor-fleet-activation/src/lib.rs:890`, `Activated::place_planning_pair`, `2f5c53dda01c849bed10883aa8b93c1dfb7a6ec125c09b0059a80ef111bb311a` (equal to the `d9dce9c8` blob) | diversity becomes `AllocationDiversity::None` | `-p kontor-fleet-activation --lib -- tests::a_planning_pair_on_one_actual_vendor_is_blocked tests::a_planning_pair_is_one_joint_allocation_on_two_actual_vendors`, 2 listed | 0 passed, 2 failed: both panicked with `Invalid { rule: "a planning pair's placement could not be frozen as one canonical receipt with two distinct actual vendors" }`. With diversity waived, the allocator put both members on one vendor, and the domain `freeze` still refused to freeze them; that second layer is why the answer became PP-02 rather than a same-vendor pair | 2 passed; SHA-256 equal |
| MUT-003-e | `crates/kontor-cli/src/local.rs:233`, `fleet_policy_planning_pair`, `7ae9557a3ba29c9ce975657f5dba2f740ab888233ad3c563e08f946fbf870582` (equal to the `d9dce9c8` blob) | `placement.is_complete()` becomes `true` | `-p kontor-cli --test local_resolve -- a_planning_pair_on_one_actual_vendor_is_blocked_whole`, 1 listed | 0 passed, 1 failed: exit `0` where `1` is required, with a body whose `selection.blocked` is `no_distinct_reviewer_vendors` printed as success | 1 passed; SHA-256 equal |
| MUT-003-f | `crates/kontor-core/src/consultation.rs:860`, `conjunctive_outcome`, `5f9f90d148edc14e6a9c493dadf48283a87748443630869b33607786a40d6235` (equal to the `d9dce9c8` blob) | a missing required finding `continue`s instead of `return None` | `-p kontor-core --test consultation_specs -- a_missing_finding_blocks_settlement_rather_than_passing`, 1 listed | 0 passed, 1 failed: "an absent finding is not agreement", `left: Some(Compliant)`, `right: None` | 1 passed; SHA-256 equal |
| MUT-003-g | same file `:774`, `CommitteeTemplateSpec::validate`, same SHA-256 | the `distinct_provider_per_slot` check becomes `if false && …` | `-- reviewers_sharing_a_primary_provider_are_refused reviewers_colliding_only_on_a_fallback_rung_are_refused`, 2 listed | 0 passed, 2 failed (each at its `assert!(template.validate().is_err())`) | 2 passed; SHA-256 equal |

All 7 were killed. After the pass, `git status` showed only this slice's three edited files.
These rows are historical: they ran on `planning_pair.rs` blob `c2b607e4…` and the
slice-three tests. Rows a–c were run again after the audit remediation changed both files
(see that section). Rows d–g stand as recorded, because their files and killer tests are
unchanged.
This is executed MUT-003 evidence, not its acceptance: the TPM routes verify and audit on
the committed head. If the reviewed head changes any site above, rerun that site.

### Slice three gates

- `cargo fmt -p kontor-core -- --check`: clean. The format was applied per crate, and only this slice's files changed.
- `cargo clippy -p kontor-core -p kontor-fleet-activation -p kontor-cli --all-targets -- -D warnings`: clean.
- On the final code: `cargo test --no-fail-fast -p kontor-core` gave 346 passed across 18 targets (23 in `planning_pair`). `-p kontor-fleet-activation` gave 18 passed. `-p kontor-cli --test local_resolve` gave 9 passed. No test failed. Registry and store parity were not rerun, because neither the registry nor the store changed.

## Audit remediation — sealed contributions have no public read path

### The finding

Audit 6f failed `3b05abce`. After seat A alone, `pair.record().findings[0].advice` returned
seat A's sealed finding while `findings()` was `None`. "For a store only" was a comment,
not a boundary. Looking for equivalent paths from a `&PlanningPairRun` turned up three
more:

- **`Debug`:** the derived `Debug` printed every recorded contribution.
- **`Clone`:** the actor is a value, so in-process authority is nominal. Anyone holding a
  run could clone it, record a fake seat B on the copy, and call `findings()` to release
  seat A's real finding without touching the live run.
- **`PartialEq`:** a restored run built from a guessed record, compared with the live run,
  would confirm the guess.

### The repair

The smallest repair that is actually enforced: a run now emits nothing sealed, and its
durable form flows only into the module. No storage, operation, identity or crate outside
`kontor-core`'s planning pair changes.

| Element | Source (`crates/kontor-core/src/planning_pair.rs`) | What it does |
| --- | --- | --- |
| No record accessor | `fn record` is module-private | It survives only inside `PlanningPairRun::restore`, for the `RECORD_NOT_CANONICAL` comparison. |
| No derived escape | `PlanningPairRun` no longer derives `Clone`, `PartialEq`, `Eq` or `Debug` | A hand-written `Debug` renders the pin, members, question, state, `findings()`, `clarification()` and the disposition, through `finish_non_exhaustive`. A sealed contribution appears neither by content, nor by address, nor by count. |
| Explicit storage seam | `PlanningPairRecord::admitted(&run)`; the record's documentation | This is the only bridge from a run to a record, and it carries the header (pin, placement, both members, question) and no contribution. The persistence owner then appends each transition's own input under the address the transition returned: the member's submitted slot and advice as a `RecordedContribution` after `record_finding` or `record_answer`, in slot order, and the caller's own `ClarificationRequest` and `PlanningPairDisposition` once those succeed. This matches the disposition's "findings and answers independently durable". `restore` is unchanged and remains the only way back, so durable round trips, hashes and every rule still hold. |
| Proof of absence | compile-fail doctests on `PlanningPairRun` | A compiling control (`state()` and `findings()` from a `&PlanningPairRun`) sits beside four `compile_fail` snippets, each differing from it by one call: `run.clone()` into an owned run, `run == other`, `serde_json::to_value(run)` and `run.record()`. |

What still holds by design: in-process, `PlanningPairActor` is a value, so a holder of
`&mut PlanningPairRun` can still pass `Member(SeatB)` and record a real seat-B finding.
That is a write, not a read. It is the authority the governed service must authenticate,
per D-2 ("derive principal, member and active generation from existing authentication").
The domain cannot authenticate a caller, and this repair does not claim it does.

### Tests

`crates/kontor-core/tests/planning_pair.rs` now has 25 tests (2 new), and its record tests
go through the seam. A test `Journal` is the persistence owner: it writes `admitted(&run)`,
then appends each transition's own input under the address it returned, in slot order.
Nothing is read back out of the run.

| Test | What it proves |
| --- | --- |
| `a_sealed_finding_has_no_public_read_path_until_release` (new) | After seat A alone: the state is `awaiting_findings`; `findings()`, `clarification()` and `disposition()` are `None`; `retained_dissent()` is empty. Neither `{:?}` nor `{:#?}` contains the advice or its address. `admitted(&run)` carries no contribution and its canonical bytes contain neither. A clarification or a disposition is refused with `FINDINGS_INCOMPLETE`, and nothing leaks afterwards. Seat B's finding then releases seat A's verbatim, under the address seat A was given. |
| `a_sealed_answer_has_no_public_read_path_until_every_addressed_member_answers` (new) | The same for answers. With both members addressed and only seat A answered, nothing renders or bridges the answer, while the released findings still render. Seat B's answer releases seat A's verbatim. |
| `a_run_survives_the_store_at_every_point_in_its_life` (rewritten) | The run is restored from canonical stored bytes at every state and held to every public observation of the live run, its rendering included, since the run has no equality of its own. The restored one-finding run stays sealed, and completing it releases seat A's original finding under its original address, so the sealed finding was kept, not dropped. A restored clarified run refuses a second clarification (`EXTRA_CLARIFICATION`). A restored disposed run keeps seat B's dissent (finding and answer), is refused by `require_formal_review` (`NOT_FORMAL`), and is `Terminal`. |
| `a_stored_run_under_another_revision_or_protocol_fails_closed`, `a_stored_run_that_breaks_a_rule_fails_closed_on_that_rule`, `a_stored_record_has_no_room_for_a_judge_a_verdict_or_a_second_round` | Unchanged assertions on a record assembled through the seam: `RECORD_PIN`, `NOT_PLANNING_PAIR`, the nine tampered records each on its own rule (`RECORD_HASH` among them), and no room for a Judge, verdict, aggregate, settlement or second round. Refusals are compared by `.err()`, because the run has no equality. |

### Repair checks (seeded and run on the remediation source)

The convention is the MUT-003 one: one mutant at a time, the file's SHA-256 captured before
seeding and confirmed after restoring, a filtered command whose `--list` count is given,
red with the mutant in place, and green after restoring. The source is `planning_pair.rs`,
blob `e6222fedeefd821673a6e8fa7db0fb470b848a5987142e9ee337727abc50eaff`. The tests are
blob `9b21d4d1cf29dc5c2bce90f860cc4179839825fc4ad2acdd46d7e721088705db`. These checks are
not MUT-003.

| Id | Mutant | Command (`--list` count) | Red | Green |
| --- | --- | --- | --- | --- |
| REPAIR-1 | `#[derive(Clone)]` on the run | `cargo test -p kontor-core --doc -- planning_pair::PlanningPairRun` (5) | 3 passed, 1 failed: the `clone` doctest, "Test compiled successfully, but it's marked `compile_fail`" | 4 passed |
| REPAIR-2 | `#[derive(PartialEq)]` on the run | same (5) | 3 passed, 1 failed: the `==` doctest, compiled successfully | 4 passed |
| REPAIR-3 | `#[derive(Serialize)]` on the run and its private clarification | same (5) | 3 passed, 1 failed: the `serde_json::to_value` doctest, compiled successfully. A first form that derived it on the run alone did not compile, because the clarification is not `Serialize`; it was discarded as a type-checker kill, not counted | 4 passed |
| REPAIR-4 | `fn record` made `pub` again | same (5) | 3 passed, 1 failed: the `run.record()` doctest, compiled successfully | 4 passed |
| REPAIR-5 | `Debug` renders `&self.findings` (raw) | `cargo test -p kontor-core --test planning_pair -- a_sealed_finding_has_no_public_read_path_until_release --exact` (1) | 0 passed, 1 failed at the rendering assertion | 1 passed |
| REPAIR-6 | `Debug` renders the raw clarification answers | `-- a_sealed_answer_has_no_public_read_path_until_every_addressed_member_answers --exact` (1) | 0 passed, 1 failed at the rendering assertion | 1 passed |
| REPAIR-7 | `admitted` copies the recorded findings | `-- a_sealed_finding_has_no_public_read_path_until_release --exact` (1) | 0 passed, 1 failed at "the header carries no contribution" | 1 passed |

### MUT-003 re-run (sites a–c)

The remediation changed `planning_pair.rs` and its tests, so sites a–c were re-seeded with
the same mutants and the same `--exact` filters as the historical rows. The source is blob
`e6222fed…` above, restored and confirmed by SHA-256 after each run.

| Id | Site | Command filter (`--list` count) | Red | Green |
| --- | --- | --- | --- | --- |
| MUT-003-a (re-run) | `PlanningPairMembers::freeze`, line 414: the same-vendor check becomes `if false && …` | `-- members_on_one_actual_vendor_are_refused a_stored_run_that_breaks_a_rule_fails_closed_on_that_rule` (2) | 0 passed, 2 failed: `left: Ok(PlanningPairMembers { … })` where `SAME_VENDOR` is required | 2 passed |
| MUT-003-b (re-run) | `PlanningPairRun::findings_complete`, line 891: the guard becomes `if false` | `-- a_findings_round_missing_a_member_releases_nothing a_stored_run_that_breaks_a_rule_fails_closed_on_that_rule` (2) | 0 passed, 2 failed: `left: Ok(())` where `FINDINGS_INCOMPLETE` is required | 2 passed |
| MUT-003-c (re-run) | `ConsultationProtocol::is_formal_review`, line 137: becomes `!matches!(self, Self::Advisor)` | `-- advice_cannot_satisfy_a_formal_review_gate` (1) | 0 passed, 1 failed: `assertion failed: !protocol.is_formal_review()` | 1 passed |

Rows d–g are retained, not re-run. Their sites and killer tests are byte-identical to
`3b05abce`:

- `crates/kontor-fleet-activation/src/lib.rs` `2f5c53dd…`, with `src/tests.rs` `a508b9d6…`;
- `crates/kontor-cli/src/local.rs` `7ae9557a…`, with `tests/local_resolve.rs` `92ccfb83…`;
- `crates/kontor-core/src/consultation.rs` `5f9f90d1…`, with `tests/consultation_specs.rs` `efe9eb3c…`.

All of these MUT-003 rows were seeded and run by this implement seat. None is an
independent reproduction, and acceptance remains the TPM-routed verify and audit.

### Remediation gates

- `cargo fmt -p kontor-core -- --check`: clean. The format was applied per crate, and only the remediation's test file changed.
- `cargo clippy -p kontor-core --all-targets -- -D warnings`: clean.
- On the final code: `cargo test --no-fail-fast -p kontor-core` gave 353 passed across 19 targets (25 in `planning_pair`, plus 5 planning-pair doctests: the control and 4 `compile_fail`). `-p kontor-fleet-activation` gave 18 passed and `-p kontor-cli --test local_resolve` gave 9; both consume the changed types. No test failed. No unchanged broad suite was rerun.

## Slice four — the governed planning pair (D-1 to D-3, additive)

The contract is `/tmp/asma-orchestration-kickoff-20260927/task004-d1-d3-disposition-20261002.md`
(SHA-256 `032496b8235b535e5779e22970391d989a3906e6bf6386e806390706b40fb25c`), read in full.
It authorizes the owning domain's additive persistence, its registered operations and its
source-authentication seam. It authorizes no platform ADR and no native effect. This slice
is built on `832e60cc` (tree `678333e4`) after its verify and audit passed. The ASMA-8113
compatibility reviewer is currently unassigned (Igor is the recorded owner), so the changed
identity vocabulary's acceptance, integration and deployment stay fenced until a reviewer
is assigned and gives an exact verdict. Nothing here claims that acceptance.

There are two local checkpoints:

- `8a27a8515788af53f5f0202e7d67536b4641c1f4` (tree `d73c5f51ac7a958825f56e61690c53b83e2a97af`, parent `832e60cc`) is D-1: the family, the run id, persistence and the identity proof.
- The checkpoint this section was committed with is D-2 and D-3: the registered operations, source authentication and runtime ports. Its id is in the final report, because a commit cannot name itself.

### D-1 — family, identity and persistence (`8a27a851`)

| Element | Source | What it does |
| --- | --- | --- |
| Closed family and typed run id | `kontor-core` `id.rs` `PlanningPairRunId`; `consultation.rs` `ConsultationRunId::PlanningPair`, `ConsultationFamily::PlanningPair` (`planning_pair`), `ConsultationRunState::Disposed` | A third closed variant everywhere. Every exhaustive daemon site names it. The Committee- and Advisor-only paths refuse it or treat it as unreachable, and the completion verdict scan skips it, so advice is never a verdict candidate. |
| Shared identity, golden bytes unchanged | `consultation_semantic_identity_of_kind` reuses `ConsultationIdentity::hash`; `tests/consultation_identity_golden.rs` | Advisor/epic, Advisor/task, Committee/task and Committee re-review digests are pinned as captured at `832e60cc`, and they still hold. A planning pair identity differs only by family, and still ignores retry mechanics. The schema version, the old endpoints and the historical migrations are untouched. |
| Forward-only migration | `crates/kontor-store/migrations/0122_planning_pair_family.sql` (schema 122) | Rebuilds `consultation_profile_revisions` and `consultation_runs` with the family closed to three names. Run state is family-conditioned: Advisor and Committee keep exactly the v70 vocabulary, and a planning pair is only `materializing`, `running`, `needs_human` or `disposed`, never `awaiting_judge` or `settled`. A planning pair also requires `result` and `settled_at` to be NULL, its semantic identity, topic and subject to be present, and `round = 1`. The v96 indexes and triggers are recreated verbatim, `disposed` is terminal, and `command_receipts` gains the six kinds. |
| Payload store keyed by the run | `planning_pair_placements`, `planning_pair_record_revisions`, `planning_pair_contributions` | These are foreign-keyed to the one run row, so there is no second identity registry. Each is immutable and permanent. A record revision must equal its run's revision, and none may follow a disposed record. A contribution is `(run, round, slot)` and cites the record revision that carried it. `result` is never written for a planning pair. |
| Repository port | `kontor-core` `repository.rs` `StoredPlanningPair{Placement,Record,Contribution}`; store `create_planning_pair_run`, `planning_pair_placement`, `latest_planning_pair_record`, `planning_pair_contributions`, `append_planning_pair_record` | Freezing writes the run, node, seats, placement and first record in one transaction, through the same validation and insert as every consultation run. Each append is one compare-and-swap on the run revision, refused once the run is disposed, with the record and its contribution beside it. |

### D-2 — registered operations and source authentication

| Tool | Route (`/v1/projects/{project_id}` …) | Tier | Command kind |
| --- | --- | --- | --- |
| `kontor_planning_pair_profiles_list` | `GET /planning-pair-profiles` | Observer | — |
| `kontor_planning_pair_profile_preview` | `POST /planning-pair-profiles:preview` | Admin | — |
| `kontor_planning_pair_profile_apply` | `POST /planning-pair-profiles:apply` | Admin | `apply_planning_pair_profile` |
| `kontor_planning_pair_run_invoke` | `POST /epics/{epic_id}/planning-pair-runs:invoke` | Operator, exact caller | `invoke_planning_pair_run` |
| `kontor_planning_pair_run_get` | `GET /planning-pair-runs/{planning_pair_run_id}` | Observer, plus visibility | — |
| `kontor_planning_pair_findings_record` | `POST …/findings:record` | Operator, plus member credential | `record_planning_pair_finding` |
| `kontor_planning_pair_clarification_request` | `POST …/clarification:request` | Operator, plus frozen caller | `request_planning_pair_clarification` |
| `kontor_planning_pair_answer_record` | `POST …/answers:record` | Operator, plus addressed member | `record_planning_pair_answer` |
| `kontor_planning_pair_disposition_record` | `POST …/disposition:record` | Operator, plus frozen caller | `record_planning_pair_disposition` |

The sources are `crates/kontor-api/src/planning_pair.rs` (DTOs and handlers), `lib.rs` (routes), `applications.rs` (port), `openapi.rs` with the regenerated `contract/openapi.json`, `apps/console/src/api/schema.d.ts`, and `crates/kontor-daemon/src/applications/planning_pair.rs` (the operations). In MCP, `registry.rs` has the nine rows and `ArgType::PlanningPairRunId`, and `dispatch.rs` the parse.

- **Profiles** go through the shared consultation publication path: an exact preview hash, the expected catalog revision, the canonical id, version and hash, and the idempotency key.
- **Authority is authentication.** The registry tier is a floor. Every write takes the actor from the scoped seat credential (`kontor-seat-v2`: seat binding and occupancy generation), never from the body.
  - **The caller** is the exact frozen caller seat at its current hosted generation, on an active node of the epic. Its role is held to the pinned document's caller roles and scopes.
  - **A member** is the exact frozen member seat at its current occupancy generation, and a write also needs an observed native identity. The slot comes from that seat alone.
  - **Ambient credentials** reach only the catalog and the observer projection. An Admin or Operator credential cannot invoke, contribute, ask or decide.
- **Order** is: authenticate, build the intent, replay, then check the revision.
  - The intent includes the actor's binding and generation.
  - Same key and same intent replays the original receipt (`unchanged`). Same key and another payload is `idempotency_conflict`.
  - A retired generation is refused before its replay is looked up, so it never regains authority.
- **Writes** carry the expected run revision (CAS) and a bounded payload. A clarification names one or both slots, once. The disposition cites both exact finding hashes and every given answer's hash, and dissent is retained. Nothing carries a verdict, settlement, Judge or aggregate.
- **Visibility.**
  - The caller sees findings, the clarification, the disposition and retained dissent only once the domain releases them.
  - A member sees only its own words, sealed or not.
  - An observer sees no contribution.
- **Closed member serve profile.** `planning_pair_member` serves exactly `kontor_planning_pair_run_get`, `kontor_planning_pair_findings_record` and `kontor_planning_pair_answer_record`. No other profile is widened.

### D-3 — source seams, placement and runtime ports

- **Seats.** The existing SeatBindings, generation fencing and the semantic consultation container are reused. The members are slots `seat-a` and `seat-b` under logical role `planning_pair_member`. They are not reviewers or a Judge. The native correlation renders `planning_pair/<run uuid>` through the existing Paseo label.
- **Names and slots, explicit only.**
  - The published document names its `container_kind`.
  - The epic's pinned Team Definition must declare that kind read-only, with exactly two display-named slots: `seat-a` titled `SEAT A` and `seat-b` titled `SEAT B`, each with capability profile `planning_pair_member`.
  - A display-named slot carries no role code, so each member's registered role is the document's own explicit `members[].role_code`. **This field was added to `PlanningPairMemberSpec` in this slice** and is part of the document hash.
  - Nothing is defaulted. The bundled realm declares no such container, so it cannot invoke.
- **Placement** is the shared allocator on one activated snapshot (`Fleet::planning_pair_placement` → `kontor_fleet_activation::place_planning_pair`), under the caller's explicit binding keys and eligibility. The members land on distinct actual vendors or nothing is frozen. The run's placement document must hash to the shared reader's `placement_hash`.
- **Runtime port.** `RuntimeAdapter::validate_planning_pair_member_surface` is new and refuses by default as `UnsupportedCapability { Launch }`.
  - The daemon asks it before anything is frozen, and again before any container is prepared.
  - The fake runtime composes the surface for tests, and `withholding_planning_pair_members()` withdraws it.
  - Paseo, AO and Codex inherit the refusal, and Paseo's `launch_consultation` also refuses a planning pair run outright.
  - The result is the exact capability gap, `unsupported_capability`, with no native effect. It is not a fake pass.
- **Direct mode is unchanged.** `kontor-cli` and its `planning_pair@1` mode have no daemon dependency, no second parser or allocator and no control plane.

### Decisions taken inside the contract, for review

1. `container_kind` is part of the published document (D-3, "explicitly selected … Team Definition/profile"), validated against the epic's pin.
2. `members[].role_code` is part of the published document. The Team Definition's display-named slot cannot carry one (`TeamDefinitionSpec::validate`: "exactly one role code or display name"). The only other source, the seeded delivery role binding, would be an invented default.
3. The domain's own transition refusals keep the existing mapping:
   - a second clarification, an unaddressed answer or a second finding is `invalid_request` (400, subject `PlanningPairRun`);
   - any write to a disposed pair is `revision_conflict`, "the aggregate is terminal and immutable" (409).
4. A planning pair's members carry no seat recovery yet (see the gaps).
5. Receipts follow the established synchronous-command policy, which the Committee operations also use:
   - an applied command records its receipt, targeting the epic, with the run id inside the intent hash;
   - a refusal is returned in the existing error vocabulary and writes no receipt;
   - "capability unavailable" is the existing `unsupported_capability` code, and no new code is added.
6. The domain contract decision is recorded in `docs/CONSULTATION_LIFECYCLE.md` ("Governed planning pairs"). The plan record stays the TPM's, as sole plan writer.

### Tests

| Suite | Tests | What they prove |
| --- | --- | --- |
| `crates/kontor-daemon/tests/loopback/planning_pair.rs` (new, 7) | `a_planning_pair_releases_sealed_findings_to_its_caller_and_ends_in_a_disposition` | Through the registered routes, on a realm that publishes the PPW kind and container, selects them, activates one fleet policy and publishes the document: both members are placed on distinct vendors, launched and observed. Each sealed finding is invisible to the caller, the observer and the peer; release goes to the caller alone. There is one clarification to one member: the second is refused, and an unaddressed answer is refused. The disposition cites exact hashes and keeps seat B's rejected finding as dissent. The pair is then terminal: `disposed` with `result` and `settled_at` NULL, three contribution rows, and absent from the Committee and Advisor reads. |
| | `only_the_frozen_caller_and_members_hold_authority_and_a_retired_generation_never_replays` | Refused with 403, freezing nothing: ambient Admin and Operator invoking, a TPM seat invoking, ambient Admin and Operator contributing, the caller contributing, another pair's member contributing or reading, and a member asking or deciding. A never-issued member generation is `stale_binding`. The slot and stored row are the authenticated member's. A current-generation replay is `unchanged`. After the member's generation, and then the caller's, is retired, their replays, reads, asks and invocation replay are `stale_binding`. |
| | `planning_pair_commands_replay_exactly_and_refuse_reuse_staleness_and_duplicates` | The invocation replays, refuses another payload under its key, and refuses a second key on the same scope and topic (`consultation_semantic_duplicate`). The two members race on one revision: exactly one lands, and the other gets `revision_conflict` naming the current revision. The winner's key replays and refuses a rewrite. A second finding is refused, and the loser lands on re-read. |
| | `concurrent_invocations_of_one_key_or_one_topic_admit_exactly_one_pair` | One key raced against itself: one pair, `created` and `unchanged`. Two keys raced on one topic: exactly one pair, plus `idempotency_conflict`. |
| | `a_planning_pair_freezes_nothing_without_its_explicit_container_or_distinct_vendors` | A document selecting `CSW` (read-only, but not the pair's slots) or the absent `XPW`, and members the policy can only put on one vendor, are each `placement_blocked`. No run is frozen and no container is prepared or launched. |
| | `a_runtime_without_the_member_surface_is_a_capability_gap_with_no_effect` | With the surface withheld: `unsupported_capability`, no run frozen, no container prepared, no member launched. |
| | `a_reopened_realm_restores_a_sealed_planning_pair_and_continues_it` | A daemon reopened on the same state root restores the sealed pair from its records through the domain transitions, keeps it sealed, and releases it on the second finding. |
| `crates/kontor-store/tests/planning_pair_store.rs` (7, in `8a27a851`); `schema_v1.rs` `v122_preserves_every_advisor_and_committee_row_and_rule` | | Freezing, CAS, rewrite refusal, disposed is terminal, unknown family and foreign state refused in SQL, the shared semantic index, and the payload only for a planning pair. The migration carries every v121 Advisor and Committee row, rule and receipt through unchanged. |
| `crates/kontor-core/tests/consultation_identity_golden.rs` (2, in `8a27a851`) | | The golden Advisor and Committee bytes, and the planning pair's own family identity. |
| `crates/kontor-runtime-paseo/tests/contract.rs` | `a_planning_pair_member_launch_is_an_unsupported_capability_with_no_native_effect` | Paseo refuses both the surface check and the launch as unsupported, with no plane call and no agent created. |
| `crates/kontor-mcp` | `the_planning_pair_member_profile_is_the_exact_member_surface`, `the_planning_pair_member_profile_never_widens_observer_authority` | The closed member surface, and that an observer keeps only the read. |
| `tests/contract/mcp_parity.rs`, `mcp_cardinality.rs` | | The nine tiers, the canary moving 201→210 mapped, 200→209 advertised and 202→211 documented, and the run-id sampler. |

### Slice four gates and receipts

- `cargo fmt -p <crate>` for the eight touched crates. Only this slice's lines changed.
- `cargo clippy -p kontor-core -p kontor-store -p kontor-api -p kontor-daemon -p kontor-mcp -p kontor-runtime -p kontor-runtime-paseo -p kontor-tests-contract --all-targets -- -D warnings`: clean.
- `cargo check --workspace --all-targets`: clean, so AO, Codex and every other consumer compile.
- `cargo test --no-fail-fast -p kontor-core -p kontor-api -p kontor-runtime -p kontor-runtime-paseo -p kontor-mcp -p kontor-tests-contract -p kontor-daemon`: exit 0, 56 result lines, 1774 passed, 0 failed, 7 ignored. Within it, `loopback_api` had 504 passed and 1 ignored, and the Paseo `contract` 289 passed.
- `cargo test -p kontor-store --test planning_pair_store`: 7 passed.
- `kontor-cli` tests (33 passed) ran before the `role_code` field was added. The CLI does not consume that type, and the workspace check covers its build.
- Console: `pnpm --filter kontor-console verify:api` and `typecheck` both passed.
- The OpenAPI change is purely additive: 9 paths and 15 schemas added, and no existing path or schema changed.
- The `8a27a851` checkpoint's gates were green when it was committed: `kontor-core`, the store suites, daemon lib 149, `loopback_api` 497 (1 ignored) and `schema_v1` 67.

### Slice four mutation checks (seeded and run by this seat)

The convention is MUT-003's:
- One mutant at a time.
- The file's SHA-256 captured before seeding and confirmed equal after a byte-exact restore and `touch`.
- One `--exact` filter, with its `--list` count.
- Red with the mutant in place, and green after restoring.

The post-restore SHA-256 values are:
- `crates/kontor-daemon/src/applications/planning_pair.rs` `753f7a7e3b91024fb6d19c749da0b3058fa32bcfc3f9f57f5d566bd50f4a028b`;
- `crates/kontor-runtime-paseo/src/adapter.rs` `9730a456f14dc83a337750e3ebc61bab2e7f89185b4179c73a59038b3e6e840e`;
- `crates/kontor-store/migrations/0122_planning_pair_family.sql` `91231d38cb7f3799784a3805788b86cfa3683254202a9beb30fb150c2e8e0866`;
- `crates/kontor-core/src/planning_pair.rs` `2bc0b3aa18ffb09d52537aa3d0184988689a0bfc1483af113d1a34ec5ec52ee9`.

Each row was 0 passed and N failed red, N passed green, with SHA-256 equal.

| Id | Site and mutant | Killer (listed) | Red |
| --- | --- | --- | --- |
| PP-MUT-01 | `authenticated_member`: the generation fence becomes `if false && …` | authority test (1) | A never-issued generation-2 credential recorded seat A's finding (`created`) where `stale_binding` is required |
| PP-MUT-02 | `authenticated_caller`: the exact-caller check becomes `if false && …` | authority test (1) | A member asking reached the hosted-generation check, giving 409 `stale_binding` where 403 is required |
| PP-MUT-03 | invoke authenticates after the replay | authority test (1) | The retired caller generation replayed its invocation (`unchanged`) |
| PP-MUT-04 | a member write authenticates after the replay (slot looked up unfenced) | authority test (1) | The retired seat-B generation replayed its finding (`unchanged`) |
| PP-MUT-05 | released findings reach every viewer (`filter(\|_\| true)`) | journey (1) | The observer saw the released findings |
| PP-MUT-06 | a member's own filter admits every slot | journey (1) | Seat B read seat A's sealed finding |
| PP-MUT-07 | a member write skips the expected-revision check | idempotency test (1) | Both racing writes landed, `[200, 200]` |
| PP-MUT-08 | invoke skips the surface check before freeze | unsupported test (1) | A run was frozen on an unsupporting runtime (`1` where `0` is required) |
| PP-MUT-09 | the container becomes the delivery Committee kind instead of the document's `container_kind` | journey (1) | `placement_blocked` "the planning pair container must declare seat-a and seat-b exactly once each" |
| PP-MUT-10 | Paseo's planning pair refusal becomes `if false && …` | Paseo contract test (1) | The launch went on instead of `UnsupportedCapability { Launch }` |
| PP-MUT-11 | SQL: the planning pair state arm also admits `awaiting_judge` | `storage_refuses_an_unknown_family_and_a_state_outside_the_family` (1) | "awaiting_judge: SQL accepted 1 rows" |
| PP-MUT-12 | SQL: the Advisor/Committee arm also admits `disposed` | same (1) | "SQL accepted the write (1 rows) that `CHECK constraint failed` forbids". The Advisor/Committee `disposed` copy is the only probe the mutant opens |
| MUT-003-a (re-run 2) | `PlanningPairMembers::freeze` same-vendor check, `if false && …` | `members_on_one_actual_vendor_are_refused`, `a_stored_run_that_breaks_a_rule_fails_closed_on_that_rule` (2) | `left: Ok(PlanningPairMembers { … })` where `SAME_VENDOR` is required |
| MUT-003-b (re-run 2) | `findings_complete` guard becomes `if false` | `a_findings_round_missing_a_member_releases_nothing`, `a_stored_run_that_breaks_a_rule_fails_closed_on_that_rule` (2) | `left: Ok(())` where `FINDINGS_INCOMPLETE` is required |
| MUT-003-c (re-run 2) | `is_formal_review` becomes `!matches!(self, Self::Advisor)` | `advice_cannot_satisfy_a_formal_review_gate` (1) | `assertion failed: !protocol.is_formal_review()` |

MUT-003 a–c were re-run because this slice changed `planning_pair.rs` (`container_kind`,
`role_code`). Their historical rows, the remediation re-run and the `832e60cc` sealed
repair are kept as written. Rows d–g are not re-run: their sites and killers are unchanged.

Not run, and therefore not claimed:
- The daemon's read-only and slot-title checks on the selected container. No test publishes a Team Definition whose selected container is writable or whose member slots are mistitled, so these two checks have no killer yet.
- The completion scan's planning pair `continue`. That scan lists only `ConsultationFamily::Committee` runs, so the arm is unreachable.

These rows are this seat's evidence, not an independent reproduction. Acceptance remains
the TPM-routed verify and audit.

## Audit 6f rework — the epic's selected catalog, container safeguards and concurrent resume

The independent 6f audit **failed** `4922a9a21644d5254ae74addfcd857b4e46fc52e` (tree
`fcfcbdd5f7e5b97912800ad4195300869f2b0161`). That checkpoint and its slice-four rows
above (PP-MUT-01 to 12, MUT-003 a–c re-run 2) are kept as written: they are the record
of the failed candidate, not of this repair. TPM's rework dispatch owns only:
- the catalog authority fix;
- the selected-container safeguard tests and their mutations;
- a concurrent-resume regression.

### The finding

At `4922`, `freeze_planning_pair` (`planning_pair.rs:467`) resolved each member's role with
`catalog_role_for_code` (`applications.rs:41474`). That function reads
`self.domain.role_catalogs.first()`, the build's first catalog, not the catalog the epic
selected. The store then inserted the `CatalogRoleRef` without `validate_against`.

Two failures followed:
- a role that only the build's first catalog declares could freeze, even when the epic selected another revision;
- a role valid only in the selected revision was refused.

### Source authority (existing, unchanged)

The epic's catalog selection already exists and is immutable:
- **The roster.** `StoredEpicRoster` (`kontor-core` `repository.rs:927`) is frozen at promotion from the project's published Core Team revision. That revision resolved against one explicitly named catalog revision.
- **The pin.** `CoreTeamRevision::resolve` (`kontor-teams` `operational.rs:111`) records that revision's canonical hash as `catalog_hash`, and every roster seat names `(catalog_id, catalog_revision)`.
- **The bytes.** The catalog's persisted bytes sit in `role_catalog_revisions`, written by `publish_role_catalog` (`kontor-store` `repository.rs:9365`). `get_role_catalog` re-reads them under their stored definition hash.
- **The check.** `CatalogRoleRef::validate_against` (`kontor-core` `spec.rs:630`) proves a reference is an exact projection of one catalog.

Nothing new is selected, sorted, defaulted or retargeted. No pin is migrated. The Advisor and Committee path, `catalog_role_for_code` included, is unchanged.

### The repair

Four places now hold a member's role to the epic's selected catalog:
- **`Services::epic_role_catalog`** (`applications/planning_pair.rs`) resolves the catalog. It reads the epic's frozen roster and the one catalog revision every seat names, loads that revision's persisted bytes, and requires their canonical hash to equal the roster's `catalog_hash`. Each failure is `placement_blocked`, before anything is frozen:
  - no roster;
  - a roster naming two revisions;
  - an unpersisted revision;
  - a hash mismatch.
- **`member_catalog_role`** builds each member's role. It takes the document's explicit `role_code` from that catalog alone, requires its lifecycle to be `Current`, and proves the result with `validate_against` and code correspondence (`require_member_role`).
- **`materialize_planning_pair_members`** proves every frozen SeatBinding role again before any container is prepared or member launched. The check is `validate_against` the epic's catalog plus correspondence to the member's explicit code, so a resumed run with a drifted role never launches.
- **The store** (`create_planning_pair_run` → `planning_pair_member_roles_in`, `repository.rs:2449`) proves each member binding's role inside the freezing transaction. The epic must have a roster; the role's catalog revision must be persisted and hash to the roster pin; and the role must pass `validate_against`. This is the planning pair path only, and `insert_consultation_run_in` is unchanged.
- **Unchanged contract.** `role_code` is still required only in new immutable planning pair documents, per the LSA's approval. There is no title, delivery, caller, model or fleet default. Old document bytes, hashes, goldens and read/restore are unchanged, and there were no previously persisted pair revisions under schema 122.

Two smaller changes:
- **Split slot predicates.** The slot-title and capability-profile checks are now two predicates with their own rules: "a planning pair slot must be titled SEAT A or SEAT B" and "… must hold the planning_pair_member capability profile". Each can be tested and mutated alone.
- **Concurrent resume.** When two requests for one key interleave, `running_planning_pair` advances the run from its frozen revision. The run's compare-and-swap admits exactly one advance, and the loser answers with the winner's row rather than `revision_conflict`. A replay check before recording then reports the second request as `unchanged`. This repairs the planning pair invocation only. The Advisor and Committee invocations keep the same pattern and are not repaired here: that would be a broad, unrelated repair, out of this dispatch.
- **Test-only fake control.** `ScriptedFakeRuntime::holding_consultation_launches` returns a `ConsultationLaunchGate` that holds every consultation launch until released. It needs no executor, like `FakeNativePause`. It is not live qualification.

### Tests

| Suite | Test | What it proves |
| --- | --- | --- |
| loopback (now 13) | `a_member_role_only_the_epics_selected_catalog_declares_is_frozen_from_it` | The epic selects a persisted successor revision declaring `PPM`. Both member SeatBindings hold `PPM` from that revision, and `validate_against` it succeeds. |
| | `a_member_role_outside_the_epics_selected_catalog_freezes_nothing` | The selected revision drops `SA` and keeps `AUD` only for compatibility. `SA` (which the build's first catalog declares as current) is refused as absent, and `AUD` because it "cannot open new seats". No run is frozen, and nothing is prepared or launched. |
| | `a_mismatched_catalog_pin_or_member_role_fails_closed_before_any_native_effect` | A roster pin the persisted catalog does not hash to freezes nothing. A run frozen with a failed first launch, whose stored seat-A role is then rewritten to another exact catalog role (`AUD`), is refused on resume ("does not correspond") with no prepare or launch. |
| | `a_writable_selected_container_freezes_nothing` | `PWW` is valid but writable, and is refused: "a planning pair container must be read-only". |
| | `a_mistitled_member_seat_freezes_nothing` | `PMW` is read-only but seat B is titled `SEAT C`, and is refused: "… must be titled SEAT A or SEAT B". |
| | `a_resumed_invocation_interleaved_with_its_original_launches_nothing_twice` | Both same-key requests are held at the member launch, then released. Both answer 200 with one pair, `created` and `unchanged`, on the same node and container, with the same two natives. The run is `running`. |
| `planning_pair_store.rs` (now 8) | `a_member_role_outside_the_epics_selected_catalog_is_refused_in_storage` | Each writes no run: an unselected persisted revision ("names a role catalog the epic did not select"), a rewritten title, an undeclared code, and an epic with no roster. The fixture now freezes the epic's roster. |

**Regression red before its fix:** `a_resumed_invocation_interleaved_with_its_original_launches_nothing_twice` was run with the `4922` advance in place and failed. One of the two requests answered 409 `revision_conflict`, "a persistence rule refused the write against the presented state". The log is `/tmp/pp2-evidence/resume-regression-red-before-fix.log` and is not committed.

### Rework mutation checks (seeded and run by this seat)

The convention is unchanged. The final source SHA-256 values, confirmed equal after each restore, are:
- `crates/kontor-daemon/src/applications/planning_pair.rs` `794360c9bf8336077a49031052718f2ed787915c8695474b4f7c275f0ada51bb`;
- `crates/kontor-store/src/repository.rs` `6b55fbf9b2982ab3c15e3db63c7a8a994db012a34a9e5516f82df8a270209913`.

| Id | Site and mutant | Filter (listed) | Red, then green |
| --- | --- | --- | --- |
| PP-MUT-13 | Freeze resolves the member role with `catalog_role_for_code`, the `4922` defect restored | selected-only, outside (2) | 0/2. `PPM` was refused, "the seeded delivery binding names a role the catalog does not declare". Unselected `SA` froze in the daemon, and the store guard alone refused it (`revision_conflict`). 2/2 green |
| PP-MUT-14 | The persisted-bytes pin check becomes `if false && …` | mismatch (1) | 0/1. The daemon accepted the mismatched pin, and the store guard refused (`revision_conflict`). 1/1 |
| PP-MUT-15 | The frozen-role/explicit-code correspondence becomes `if false && …` | mismatch (1) | 0/1. The drifted run resumed, launched both members and reached `running`. 1/1 |
| PP-MUT-16 | The `Current` lifecycle check becomes `if false && …` | outside (1) | 0/1. The compatibility-only `AUD` froze and launched. 1/1 |
| PP-MUT-17 | Store: the in-transaction role guard is not called | store boundary (1) | 0/1. The unselected revision froze (`Ok(Frozen …)`). 1/1 |
| PP-MUT-18 | Store: the roster-pin comparison becomes `if false && …` | store boundary (1) | 0/1. The same. 1/1 |
| PP-MUT-19 | The read-only predicate becomes `if false && …` | writable, mistitled (2) | 1/2. **Only** the writable test failed: `PWW` froze and launched. The mistitled test passed. 2/2 |
| PP-MUT-20 | The slot-title predicate becomes `if false && …` | mistitled, writable (2) | 1/2. **Only** the mistitled test failed: `PMW` froze and launched. The writable test passed. 2/2 |
| PP-MUT-21 | The lost advance refuses instead of answering with the winner's row | resume (1) | 0/1. 409 `revision_conflict`. 1/1 |
| PP-MUT-23 | The resumed request skips the replay check before recording | resume (1) | 0/1. `["created", "created"]` where `["created", "unchanged"]` is required. 1/1 |

Disclosed, not counted:
- **A redundant guard.** A first draft of the resume fix also re-read the row before advancing. Run on that draft (daemon SHA-256 `a8d96921…`), the re-read mutant and the lost-advance mutant each survived, because each guard alone was sufficient. The draft was reduced to the single compare-and-swap guard above, and the daemon mutants were re-run at the final hash (the table).
- **Superseded draft runs.** PP-MUT-13 to 16, 19 and 20 were also killed on that draft. Those runs are superseded by the final-hash rows.
- **Not re-run.** The slice-four rows PP-MUT-01 to 12 were not re-run at the new hash. Their killer tests are green on the final source (the gates below).
- **The completion-scan arm is no positive proof.** The planning pair `continue` in the completion verdict scan is unreachable, because that scan lists only `ConsultationFamily::Committee` runs. No mutant can kill it, and none is claimed.

### Rework gates

- `cargo fmt -p <crate> -- --check` is clean for `kontor-daemon`, `kontor-store` and `kontor-runtime`. `cargo clippy` on those three, all targets, `-D warnings`, is clean. `cargo check --workspace --all-targets` is clean.
- `kontor-core`: `consultation_identity_golden` 2 and `planning_pair` 25. The core is unchanged; this is the old-golden gate.
- `kontor-store`: `planning_pair_store` 8, `schema_v1` 67 (migration compatibility), `repository_roundtrip` 89, `advisor_multi_seat_advice` 3 and `consultation_profiles` 9.
- `kontor-runtime`: 81 (lib 61, plus three targets with 20).
- `kontor-daemon`: lib 149. `loopback_api` filtered to `planning_pair advisor committee consultation`: 35 passed, among them the 13 planning pair tests covering auth, replay, sealed reads, member authority, vendors, catalog and resume.
- `kontor-mcp` lib 69. `mcp_parity`, `mcp_cardinality` and `mcp_mutants`: 36. The Paseo planning pair contract test: 1.
- Not one test failed. The broad 1774-test run was not repeated. Only the planning pair module, the store's planning pair insert, the fake runtime and tests changed.

## Audit 6f turn-5 rework — atomic receipt classification

The independent 6f audit (codex turn 5) **failed** `420f82f5c4417b0a24ce5a7c3aefb0c604411935`
(tree `2f49d3cb893407a6c01c605d42c550be84d4c51b`). It and the `4922` failure are kept as written:
- the `420f` rework rows above, PP-MUT-13 to 21 and 23, with the draft survivors disclosed there;
- the slice-four rows.

The audit otherwise accepted the catalog authority and the writable and mistitled guards. Their bytes and scope are unchanged here.

### The finding

The run's compare-and-swap, node and native effects were deduplicated, but the receipt was classified by check-then-act:
- After the CAS, both same-key workers could call `replayed()` (`planning_pair.rs:217` at `420f`) and both find no receipt.
- The first `record()` then inserted. The second returned the existing receipt (`applications.rs:6297`), and both answered `created`.
- `created` plus `unchanged` was therefore not guaranteed.

The `420f` regression ran on the default current-thread runtime and held only the member launch, so the first released request always ran through its receipt before the second resumed. The PP-MUT-23 kill was therefore schedule-only, and its claim was too strong. That row is kept, with this limitation.

### The repair

- **`Services::record_classified`** (`applications.rs`) is the existing `record` body with an outcome. It reads the key and inserts inside the one store critical section, the `with_store` closure that holds the store mutex, around `record_local_command_in_realm`, whose transaction is `insert_local_command`. An existing exact replay is `false`; the stored receipt carrying the id this call generated is `true`.
- **`record`** now delegates and drops the outcome. Its callers, receipts and errors are unchanged, including the same-key, different-payload `idempotency_conflict`.
- **The invocation tail** drops its own `replayed()` pre-check and answers `created` or `unchanged` from that outcome. Authentication, generation and the replay check at the top of the invocation are unchanged.
- **Advisor and Committee** behavior is unchanged.
- **Test hold.** A `#[doc(hidden)]` `Daemon::hold_planning_pair_invocation_receipts` returns a `PlanningPairReceiptHold`. It is executor-free and holds every planning pair invocation immediately before its receipt write, after the compare-and-swap. No composed daemon installs one, and with none installed the hold point returns at once. It follows the daemon's existing `#[doc(hidden)]` black-box seams, such as `start_with_usage_poller`.
- **Not used:** a test-only lock, a serialized runtime, or a repeated check before the write.

### Tests

| Test | What it proves |
| --- | --- |
| `two_requests_held_at_the_receipt_write_classify_one_created_and_one_unchanged` (new) | Two same-key requests are held after the CAS at the receipt write, so neither has seen a receipt. The test requires both to arrive before releasing. Then: one `created` and one `unchanged`; the same `receipt_id`; the same run, `topology_node_id` and `container_name`; the same two natives; one planning pair run; one stored receipt row for the key. |
| `a_mixed_or_unpersisted_roster_catalog_freezes_nothing` (new) | A roster whose seats name two revisions is refused ("names more than one role catalog revision"). A roster naming a revision this realm never persisted is refused ("is not persisted in this realm"). Neither freezes, prepares or launches anything, and the same roster, restored, invokes. |
| `a_resumed_invocation_interleaved_with_its_original_launches_nothing_twice` | Its doc comment now states its scope: the launch interleaving and the `running` CAS. It does not line the two requests up at the receipt write. |

### Red, mutants and green (seeded and run by this seat)

The final SHA-256 values, confirmed equal after each restore, are:
- `crates/kontor-daemon/src/applications/planning_pair.rs` `bc8d457e2c3c9b5138b310e9098f24d6034a36f0694466b5f4d596704836e037`;
- `crates/kontor-daemon/src/applications.rs` `3d7f2116803ba274d4f1ac2bd3e9459ecbdb03e14dde84939842f1c5e792f39f`.

| Id | Site and mutant | Filter (listed) | Red, then green |
| --- | --- | --- | --- |
| PP-RED-420F | The `420f` classification at its own site: `replayed()` pre-check, then `record()` answered `created`. Only the receipt hold is added before the write | new receipt test, old resume test (2) | 1/2. The new receipt test failed with `["created", "created"]` and the **same** `receipt_id`, exactly the audit's finding. The old resume test **passed** on that same `420f` form, confirming it could not see the defect. 2/2 green |
| PP-MUT-24 | `record_classified`: an existing exact replay is classified as this call's write | receipt (1) | 0/1. `["created", "created"]`. 1/1 |
| PP-MUT-25 | The invocation ignores the classification and always answers `created` | receipt (1) | 0/1. `["created", "created"]`. 1/1 |
| PP-MUT-26 | The roster's one-revision check becomes `if false && …` | mixed/unpersisted (1) | 0/1. The mixed roster froze and launched (`running`, `created`). 1/1 |
| PP-MUT-27 | An unpersisted selected revision falls back to the build's first catalog | mixed/unpersisted (1) | 0/1. The unpersisted roster froze and launched. 1/1 |

Limitations, disclosed:
- **Unreachable branch.** The insert branch's `receipt.id == generated` cannot see another writer within this process, because the store mutex serializes the read-and-insert closure. That comparison is the store's exact outcome, but no in-process schedule reaches its `false` arm, and no kill of it is claimed.
- **Unchanged mutant sites.** The `420f` mutant sites PP-MUT-13 to 16 and 19 to 21 are byte-identical in the new source. `git diff 420f82f5` touches only the hold type and the invocation tail, so those rows were not re-run.
- **Removed site.** The PP-MUT-23 site, the pre-record `replayed()`, no longer exists.
- **No positive proof from the completion scan.** Its planning pair arm stays unreachable (the scan lists only Committee runs) and is no qualification.
- **Out of scope.** The Advisor and Committee invocation tails keep their own pre-existing patterns.

### Turn-5 gates

- `cargo fmt -p kontor-daemon -- --check` and `cargo clippy -p kontor-daemon --all-targets -- -D warnings` are clean. `cargo check --workspace --all-targets` is clean.
- `cargo test --no-fail-fast -p kontor-daemon`: 712 passed, 0 failed, 1 ignored.
  - lib 149 and main 2;
  - `loopback_api` 512, including the 15 planning pair tests;
  - `account_pinning` 5, `container_recreation` 12, `mcp_journey` 2, `quota_observation` 21, `recovery_security` 6 and `succession_handoff` 3.
  - The whole daemon was run because the shared `record` now delegates. Every authority operation passes through it.
- Core: golden 2, `planning_pair` 25. Store: `planning_pair_store` 8, `schema_v1` 67.
- MCP: lib 69, plus `mcp_parity`, `mcp_cardinality` and `mcp_mutants` 36. Paseo planning pair contract: 1.
- The broad workspace run was not repeated.

## D-3 member surface (2026-10-02)

> **Superseded in part — see "D-3 authority repair" below.** Verify b887 passed this composition
> but confirmed an authority hole, and the LSA granted no waiver. Every real Paseo route,
> Claude included, is now refused before freeze, and a member binds only when every mandatory
> field matches. The route cells and qualification rule below are kept as written at `1b1d5597`.

TPM's D3-MEMBER-SURFACE dispatch builds on `b3457c31a47f1cde27d4511fbfeb632986118288` (tree
`169542e601644ea6d091f0e89a08e78c930bcacc`), after verify and audit passed on that exact head.
The LSA domain contract D-3 / ADR-0008 applies. It is a domain contract, not a platform
acceptance.

This slice owns the member's runtime guard, serve profile, authentication context and
provenance source, plus mock and disposable-fixture tests. It does not touch:
- member recovery, the caller's tool profile, TPM placement or the root CLI;
- any native create, daemon start, provider, credential, network or live effect.

Release 419/r3 is unchanged. The ASMA-8113 reviewer is still unassigned, so identity
integration, publication and deploy stay fenced. Every observation below comes from
the fake runtime or a recorded Paseo fixture. It is source-contract evidence and not
a live qualification.

### What is implemented

| Element | Source | What it does |
| --- | --- | --- |
| One closed member surface | `kontor-core` `planning_pair.rs` `MEMBER_SERVE_PROFILE`, `MEMBER_MCP_TOOLS` | The three tools — `kontor_planning_pair_run_get`, `…_findings_record`, `…_answer_record` — as the one list. The registry profile, the guard's allowlist and the Paseo creation `toolPolicy` are each generated from it, so they cannot drift. The existing consultation creation policy, 4 preapproved tools against a 5-tool profile, is unchanged. |
| Registry profile | `kontor-mcp` `registry.rs` | `planning_pair_member` is the core list. No other profile changed. |
| Guard | `kontor-mcp` `consultation_guard.rs`, `main.rs` | `--consultation-tool-guard` alone is the Advisor and Committee surface, unchanged, reason text included. `--serve-profile consultation` is that same surface. `--serve-profile planning_pair_member` permits `Read`, `Glob`, `Grep`, `ToolSearch` and exactly the three tools under the exact `mcp__kontor__` prefix. An unknown profile, a malformed or extra argument, or a non-UTF-8 argument denies every tool. So does a member profile the registry lacks or that is not exactly the core list. Every argument list that begins with the guard flag is the guard's and exits zero with a decision, never a parser error a hook runner might treat as non-blocking. Oversized or unparseable requests deny. |
| Runtime port | `kontor-runtime` `planning_pair.rs`, `adapter.rs` | `validate_planning_pair_member_surface(&[PlanningPairMemberRoute])` is asked about the two actual routes, seat A then seat B, and refuses by default. `ConsultationLaunchRequest.planning_pair` is a typed, non-secret `PlanningPairLaunchContext`: run, SeatBinding, slot, occupancy generation, document pin, topology, Team Definition and role-catalog pins, container node and cwd, frozen route and actual vendor, placement hash, and requested fleet provenance. `planning_pair_context()` refuses a pair launch without a context, an Advisor or Committee launch with one, and any context that disagrees with its request. `ConsultationLaunchOutcome.planning_pair` is a typed `PlanningPairMemberObservation`, in which an unreportable field is `Unsupported`. |
| Fake runtime | `kontor-runtime` `fake.rs` | It validates the family context and records each member context. It writes member provenance to its own label store and reads it back, on the `fake.runtime.labels` surface (source contract). `withholding_planning_pair_members_on(provider)` refuses routes per provider. `dropping_planning_pair_provenance_labels` simulates readback drift. |
| Paseo route policy | `kontor-runtime-paseo` `adapter.rs` `planning_pair_member_routes` | Only a Claude route is composable, and it needs seat MCP composed and the guard binary attested for the member profile. Every other provider is `PermissionModeUnsupported { provider }`, decided before any plane call, file or process. |
| Paseo member launch | `adapter.rs`, `seat_mcp.rs`, `client.rs`, `wire.rs` | The context and route are checked before the launch claim. Then: the cwd gets the member serve profile in `.mcp.json` and the member guard hook. The creation frame gets `planning_pair_member_agent_create` (Claude only, `default` mode, the contained tool restriction) and `with_planning_pair_member_mcp`, which preapproves the core list. Labels add `kontor.occupancy_generation`, `kontor.consultation_profile_hash`, `kontor.placement_hash` and `kontor.serve_profile` to the consultation and fleet labels. The credential is only in the frame's secret environment. Readback reuses the placement, route and label checks and `observed_fleet_provenance`. |
| Daemon | `applications/planning_pair.rs` | The surface is asked about both placed routes inside `freeze_planning_pair`, before anything is persisted, and again in `materialize_planning_pair_members` before the container is prepared. `planning_pair_launch_context` derives each member's context from durable state and refuses any disagreement before native effect: the document pin against the run, the route against the placement, the placement against the run context, the epic's Team Definition and topology pins, and the roster catalog. The generation is the seat's own, never a default and never from the secret. A member is bound, and so may contribute, only when its readback observed exactly its requested provenance and reported its member surface. Otherwise `unavailable` names the kept native session, the run stays materializing, and a replay meets that same session. |

### Route capability cells

| Route | Cell | Basis |
| --- | --- | --- |
| Claude (`claude`, account-qualified `claude-*`) | Composable, source contract only | Guard attested for the member profile; member serve profile in the cwd and the frame; creation restriction; `default` mode; readback. No live session. |
| Codex (`codex`, `codex-*`) | **Unsupported**: `PermissionModeUnsupported` → `unsupported_capability`, `providers/<provider>` | Read-only sandbox plus `never` approval is composable, but it is not a closed tool restriction. Paseo's `toolPolicy` only preapproves, and the provider home's own MCP servers are not excluded. |
| Cursor | **Unsupported** | `plan` is behavioral. Cursor's ACP permits shell writes. |
| OpenCode | **Unsupported** | `plan` and the historical fallbacks are behavioral, not containment. |
| Any other provider | **Unsupported** | No composed surface. |
| Seat MCP kill switch (`KONTOR_SEAT_MCP=off`) | Claude refused: `LaunchNotAdmitted` | No scoped member MCP. |
| A guard binary that does not enforce the member profile | Claude refused: `LaunchNotAdmitted` | A mixed or older installation. |

**Capability blocker, returned to TPM and LSA.** A Codex member needs a supported,
acknowledged, per-agent closed tool restriction. One example would be a Paseo
`toolPolicy` that denies every unlisted tool for Codex, acknowledged by
`toolPolicyApplied`, with the provider home's other MCP servers excluded. A Cursor or
OpenCode member needs an enforced non-mutating execution boundary. Neither exists in
the Paseo surface this repository records, so both routes are refused rather than
approximated. The rest of the slice went ahead.

### Readback cells (Paseo)

| Field | Cell |
| --- | --- |
| Project | Proved through the bound workspace; the agent snapshot has no project id. |
| Workspace and cwd | Observed. |
| Native session parent | Observed absent; any parent refuses. |
| Run, SeatBinding, slot, occupancy generation, document hash, placement hash, serve profile, fleet provenance | Observed as exact labels. A label states what Kontor wrote; it is not account ownership or enforcement. |
| Provider, model, effective effort, mode | Observed. |
| Tool restriction | **Unsupported**: no snapshot field reports it. |
| Credential or account authority | Not claimed. The route's vendor is not the credential's authority. |

### Tests

| Suite | Tests |
| --- | --- |
| `kontor-mcp` bin, `consultation_guard` | `…exactly_the_three_member_tools` (foreign server, near spellings, case, missing prefix, consultation and caller tools denied); `the_omitted_and_the_explicit_consultation_profile_are_the_one_legacy_surface`; `an_unknown_or_malformed_guard_profile_denies_every_tool`; `an_oversized_malformed_or_foreign_hook_request_is_denied`; `the_member_guard_is_generated_from_the_one_closed_member_surface` |
| `kontor-mcp` `tests/consultation_guard.rs` (new, shipped binary) | the member profile allows exactly the member surface; the legacy guard is unchanged and grants no member tool; a malformed guard argv denies and still exits zero |
| Paseo `contract.rs` (7 new) | a Claude member is composed, created and read back under its closed surface (cwd files, frame, labels, `toolPolicy` from the core list, observed provenance, unsupported restriction, no secret on any surface); non-Claude routes refused with zero effects; seat MCP and an attested guard are required; missing or conflicting context refused before any plane call (11 cases); drifted readback refused and never a second session; a lost create acknowledgement adopts the one session; a relaunch adopts its one session. The recorded transport is synchronous, so true simultaneity is shown at the daemon. |
| daemon loopback (3 new, 18 in the module) | an unsupported member route freezes nothing and names its provider; member launches carry the seat's current generation (2 after a fence) and the frozen pins; a member without its provenance readback stays unqualified, its credential cannot contribute (`stale_binding`), and the replay meets the same native |

The existing module tests stay green: concurrent resume, the receipt hold regression, sealed visibility, cross-member and stale generations before replay, catalog authority, and the writable and mistitled containers.

### D-3 gates

- `cargo fmt -p` for `kontor-core`, `kontor-mcp`, `kontor-runtime`, `kontor-runtime-paseo` and `kontor-daemon`. `cargo clippy` on those five, all targets, `-D warnings`: clean.
- `cargo test --no-fail-fast -p kontor-core -p kontor-mcp -p kontor-runtime -p kontor-runtime-paseo -p kontor-tests-contract -p kontor-daemon`: exit 0, 53 result lines, 1764 passed, 0 failed, 7 ignored. That includes:
  - `loopback_api` 515 (1 ignored);
  - the Paseo contract 295 and live 0 (6 ignored);
  - `consultation_identity_golden` 2, `consultation_specs` 26, core `planning_pair` 25;
  - `mcp_parity`, `mcp_cardinality` and `mcp_mutants` 36;
  - `runtime_adapter` 52.
- `cargo test -p kontor-store --test schema_v1 --test planning_pair_store`: 67 and 8.
- The daemon was run whole because the fake's consultation launch path is shared by Advisor and Committee.

### D-3 mutation checks (seeded and run by this seat)

Pre-mutation SHA-256 values, each confirmed equal after restore:
- `kontor-runtime-paseo/src/adapter.rs` `6e13db0819cd2c14cd68af258c21fe3ae371fa359e50158d80e65efe11a93b1f`
- `kontor-daemon/src/applications/planning_pair.rs` `32875830d901589850ece2ab1399a4c0e7c54f64ba13f215a4aa10515c49ad79`
- `kontor-mcp/src/consultation_guard.rs` `985f2db7aa47304ef41035cac7fc605cd44134c4d39c011b4c5966afb9db4124`
- `kontor-mcp/src/main.rs` `f5fdd68e00e307d95dc7d5a3f3442af8bc777a20a5eb92576d89d63d2731c530`
- `kontor-runtime/src/planning_pair.rs` `c2a4a92fa71b8c7c4c2049f0a37f37c6c4ef5f995b0dea222b891ebfccdb2844`

| Id | Mutant | Filter (listed) | Red, then green |
| --- | --- | --- | --- |
| D3-MUT-01 | Member→consultation substitution in the frame: `with_planning_pair_member_mcp` becomes `with_consultation_mcp` | composed, zero-effects (2) | 1/2. **Only** the composed test failed: the frame named the consultation profile. 2/2 |
| D3-MUT-02 | Member→consultation substitution in the cwd: `compose_planning_pair_member` becomes `compose_consultation` | composed, zero-effects (2) | 1/2. **Only** the composed test failed: `.mcp.json` named `consultation`. 2/2 |
| D3-MUT-03 | Guard bypass: a profile's tool list is ignored for `mcp__kontor__` tools | member guard, malformed-request (2) | 1/2. **Only** the member guard test failed: `mcp__kontor__kontor_gate_record` allowed. 2/2 |
| D3-MUT-04 | Guard bypass: only the exact legacy argv reaches the guard | malformed binary, legacy binary (2) | 1/2. **Only** the malformed test failed: the binary printed no decision (parser exit). 2/2 |
| D3-MUT-05 | Pre-freeze gate removed in the daemon | route-surface loopback (1) | 0/1. A pair was frozen (`1` where `0` is required). 1/1 |
| D3-MUT-06 | Pre-freeze gate removed at the runtime: a non-Claude route passes | zero-effects, composed (2) | 1/2. **Only** zero-effects failed: `codex: Ok(())`. 2/2 |
| D3-MUT-07 | Generation mismatch: the context states `1`, not the seat's own | context loopback (1) | 0/1. Generation 1 where 2 is required. 1/1 |
| D3-MUT-08 | Observed provenance copy: the member qualifies without a matching readback | readback loopback (1) | 0/1. The member was bound and the invocation answered 200. 1/1 |
| D3-MUT-09 | The runtime skips validating the context against its request | context Paseo (1) | 0/1. A mismatched context reached the plane (`Transport`). 1/1 |
| D3-MUT-10 | An Advisor or Committee launch carrying a member context is accepted | context Paseo (1) | 0/1. It reached the plane. 1/1 |

These rows are this seat's evidence, not an independent reproduction. Every earlier row is
unchanged, including the failed `4922` and `420f` records, PP-MUT-23 marked schedule-only,
and the draft survivors.

## D-3 authority repair (2026-10-02)

The b887 verify callback on `1b1d5597abbd863225be4e4ce851c4bf384e14da` (tree
`ebd00a0d00e269041048cbd013cd4b397bfd0be8`) found the composition **passed**, but confirmed an
**authority hole**. The daemon bound a member on a matching observed fleet provenance plus any
`planning_pair` observation, even though correlation, route or tool restrictions might be
`Unsupported`. The Paseo adapter also advertised Claude as launchable, although it acknowledges
no applied restriction.

The LSA granted no waiver. That verdict and its finding, the `1b1d` "D-3 member surface" section
above, and every earlier failure and mutation record (`4922`, `420f`, the D3-MUT rows) are kept
as written. Where they conflict, this section supersedes the `1b1d` route cells and its
qualification rule.

This repair owns only the source fail-closed member binding, the pre-freeze provider capability,
their tests and this handoff. It adds no recovery and no caller tools, and has no native, daemon,
provider, credential, network or deploy effect.

### The repair

- **Fail-closed binding** (`applications/planning_pair.rs` `planning_pair_member_qualifies`). A member is bound — so it holds current contribution authority and the pair can reach a qualified running receipt — only when all of these hold:
  - its launch reported a `PlanningPairMemberObservation`;
  - every mandatory field is `Matched` (`PlanningPairMemberObservation::unmatched_mandatory`): correlation, route and closed tool restriction;
  - its readback observed exactly its requested fleet provenance.

  Each failure has its own typed no-observation rule, under `unavailable` / `planning pair member readback`:
  - "…launch reported no member-surface observation";
  - "…did not observe its correlation";
  - "…did not observe its route";
  - "…did not observe its closed tool restriction";
  - "…did not confirm its frozen provenance…".

  The refusal names the kept native session (`native/<id>`) and says confirmation is unknown. Nothing is bound or receipted, and the pair stays materializing. A replay meets the same session, with no second create, replacement, retirement or archive.
- **Account authority is separate.** The new `account_authority` field is always `Unsupported` and is never a qualification field. An account-qualified provider label is not credential ownership.
- **Pre-freeze provider capability** (`kontor-runtime` `MemberSurfaceGap`, `RuntimeError::PlanningPairMemberSurfaceUnsupported { provider, gap }`, mapped to `unsupported_capability` at `providers/<provider>/<gap>`). In Paseo, every real route is refused by `validate_planning_pair_member_surface` and again by `launch_consultation`. The refusal comes after the context check and before the launch claim, any plane call, composed file, guard process or session:

  | Route | Gap |
  | --- | --- |
  | Claude | `restriction_unacknowledged` |
  | Codex | `closed_tools_unavailable` |
  | Cursor, OpenCode | `read_only_unenforced` |
  | Any other provider | `not_composed` |

  The adapter's member launch branches are removed. No acknowledgement flag, config readback, production switch, label copy or attested binary was invented or treated as qualification.
- **Claude composition is fixture-only.** It is kept and proved by direct construction on source fixtures: `SeatMcp::compose_planning_pair_member` and `verify_planning_pair_member_guard`, `PaseoRpc::planning_pair_member_agent_create` with `with_planning_pair_member_mcp`, and `wire::planning_pair_member_labels`. It is not an advertised runtime launch.
- **The fake is a hypothetical contract.** Its default member observation has every mandatory field `Matched` and account authority `Unsupported`. That is a hypothetical, source-contract runtime, not a statement about any real one. `observing_planning_pair_member_field_unsupported(field)` and `omitting_planning_pair_member_observation()` model a runtime that claimed the whole surface before freeze and then could not observe it after create.
- **No exact API was found.** The only applied flag on the recorded Paseo surface is `providerOptionsApplied`, an optional per-agent boolean. Its semantics are OpenCode provider options, not the exact session profile tools, guard hash, enforcement or ambient-MCP exclusion. Nothing was probed and no network was used.

### Tests

| Suite | Tests |
| --- | --- |
| daemon loopback (4 new, 22 in the module) | `a_member_with_an_unobserved_correlation_is_kept_unqualified`, `…_route_…`, `…_tool_restriction_…`, `a_member_without_a_member_surface_observation_is_kept_unqualified`. Each proves: the typed rule and the kept native named; confirmation unknown; no member bound; both member credentials `stale_binding`; the pair materializing; no receipt for the key; the replay on the same native; nothing retired or archived. The provenance-readback test is unchanged. The positive journey, sealed, cross-member, generation, receipt and resume tests stay green against the fully matched fake. |
| Paseo `contract.rs` | `every_real_planning_pair_member_route_is_refused_before_any_effect`: 8 routes, each with its exact gap from both the surface check and the launch. Seat MCP and a member-enforcing guard are configured, and still: no plane call, no composed file, and the guard never runs. `the_claude_member_composition_is_constructible_on_source_fixtures_only`: guard attestation, including an older binary refused; cwd files; the creation frame with `toolPolicy` from the one list; labels; the secret only in the frame's secret environment; non-Claude frames unconstructible. `a_member_launch_with_a_missing_or_conflicting_context_is_refused_before_any_plane_call`: 11 cases, then a valid context refused for its route. The `1b1d` adapter-launch member tests are removed, because that launch no longer exists. |

### Repair gates

- `cargo fmt -p` and `cargo clippy --all-targets -D warnings` for `kontor-runtime`, `kontor-runtime-paseo`, `kontor-daemon` and `kontor-api`: clean.
- `cargo test --no-fail-fast -p kontor-runtime -p kontor-runtime-paseo -p kontor-api -p kontor-mcp`: 619 passed, 0 failed, 6 ignored. Paseo contract 291; guard binary 3.
- `-p kontor-daemon --lib`: 149.
- `--test loopback_api -- planning_pair advisor committee consultation`: 44.
- `kontor-tests-contract` `runtime_adapter`, `mcp_parity`, `mcp_cardinality` and `mcp_mutants`: 88.
- `kontor-core` `consultation_identity_golden` and `planning_pair`: 27.
- Not one failure. The broad 1764-test run was not repeated: this repair touches only planning pair paths and the fake's planning pair branches.

### Repair mutation checks (seeded and run by this seat)

Pre-mutation SHA-256 values, each confirmed equal after restore:
- `kontor-runtime/src/planning_pair.rs` `6840287410ebe8efa2a26061e2a0d2b7ef992d763a0418c275565116de9f71f9`
- `kontor-daemon/src/applications/planning_pair.rs` `511c456ca25e9c3620adafa3446a4df232afd3fa33ebdb935e74b277e2601eac`
- `kontor-runtime-paseo/src/adapter.rs` `e6f29cca90cbd60c5821e7da8121913886857736c9aa411ea93cae7d8f87f8c1`

| Id | Mutant | Filter (listed) | Red, then green |
| --- | --- | --- | --- |
| R-MUT-01 | Drop the tool-restriction check from the mandatory fields | restriction, correlation, route (3) | 2/3. **Only** the restriction test failed: the member bound and the invocation answered 200. 3/3 |
| R-MUT-02 | Drop the route check | route, correlation, restriction (3) | 2/3. **Only** the route test failed. 3/3 |
| R-MUT-03 | Drop the correlation check | correlation, route, restriction (3) | 2/3. **Only** the correlation test failed. 3/3 |
| R-MUT-04 | A launch with no member-surface observation qualifies | no-observation, restriction (2) | 1/2. **Only** the no-observation test failed. 2/2 |
| R-MUT-05 | Drop the provenance check | provenance, restriction (2) | 1/2. **Only** the provenance test failed. 2/2 |
| R-MUT-06 | Paseo admits a Claude route at the surface check | every-route (1) | 0/1. `claude: Ok(())`. 1/1 |
| R-MUT-07 | Paseo's launch refusal is removed | every-route (1) | 0/1. A Claude member launch reached the plane (`Transport`). 1/1 |

## Frontier B: the opt-in `planning_pair_caller` source profile (2026-10-02)

TPM's next independent source slice builds on `8b6ee064fa84d02d4365e2d28819150907da3745` (tree
`aef842f666e3e94fd7dccf3937db87745e7728f4`), after verify and audit 6f passed with no findings.

This checkout owns one distinct, opt-in caller source profile plus its tests and docs. It
adds no recovery, no placement, no direct CLI and no daemon source change. It has no native,
provider, credential, network, deploy or source-policy effect, and does not widen the
leadership profile or deploy any change to it.

### What is implemented

| Element | Source | What it does |
| --- | --- | --- |
| One closed caller list | `kontor-core` `planning_pair.rs` `CALLER_SERVE_PROFILE`, `CALLER_MCP_TOOLS` | Exactly the four registered caller tools, with names confirmed in the registry: `kontor_planning_pair_run_get`, `kontor_planning_pair_run_invoke`, `kontor_planning_pair_clarification_request` and `kontor_planning_pair_disposition_record`. No new route. |
| Registry profile | `kontor-mcp` `registry.rs` | `planning_pair_caller` is generated from that list; there is no second allowlist. Every entry is at or below the Operator floor; the read is Observer. No member, publication, gate, permission, delegation, Advisor or Committee tool. The leadership, worker, consultation and member profiles are unchanged. |
| Explicit selection | `kontor-mcp` `main.rs` `selected_profile` | The caller profile is served only when named exactly: `--serve-profile planning_pair_caller`. An omitted profile is the tier's whole surface, as before. A near spelling is refused, and the refusal lists the declared profiles. |
| Composition | — | **Not composed.** No hosted seat, pin, role, title or document selects it. The hosted leadership MCP (`with_leadership_mcp`) stays `leadership`. Composing the caller profile into a hosted seat would be a new explicit optional selection, and none exists. |
| Guard | `consultation_guard.rs` | The caller profile is not a guard profile. `--consultation-tool-guard --serve-profile planning_pair_caller` is malformed and denies every tool, so the guard still accepts exactly `consultation` and `planning_pair_member`. |
| Authority | daemon, unchanged | Serving the profile grants nothing. The caller is still the exact frozen caller seat at its current hosted generation, under the document's original allowed roles, and authentication still precedes replay. |

### Tests

| Suite | Tests |
| --- | --- |
| `kontor-mcp` registry | `the_planning_pair_caller_profile_is_the_exact_caller_surface`: the exact four tools from the one list; member, publication, gate, permission, Advisor, Committee, completion and session tools excluded; the member profile shares only the read. `no_other_profile_serves_a_planning_pair_caller_write`. |
| `kontor-mcp` server | `the_planning_pair_caller_profile_serves_its_four_tools_within_the_tier`: four at Operator, only the read at Observer. |
| `kontor-mcp` main | `the_caller_profile_is_selected_only_by_its_exact_name`: omitted selects nothing; exact name only; six near or foreign names refused. |
| `kontor-mcp` guard (unit and binary) | The caller profile denies every tool as a guard profile. |
| daemon loopback (new, 23 in the module) | `the_opt_in_caller_profile_drives_a_pair_through_its_four_tools_and_grants_nothing`, which runs the production `kontor-mcp` dispatcher over the realm's real router with each seat's own credential. It shows, in order: (1) exactly the four tools are served; (2) the invocation runs through them; (3) the member writes, the profile publication and the Committee settle are excluded by the profile, with no request made; (4) members contribute under their own profile, which serves no caller tool; (5) the caller's read stays sealed until both findings, and the caller is refused (403) under the member profile; (6) one clarification, the second refused; (7) a disposition that keeps seat B's dissent, with no verdict, settlement, result or Judge. Under the caller profile, refusals are: member credential 409 `stale_binding` (no hosted caller occupancy); TPM 403 (role); ambient Admin 403; ambient Operator 403. No pair is frozen by any of them. A retired caller generation's replay of its own invocation key is 409 `stale_binding`. |

The existing old-path tests stay green: the leadership profile and the hosted leadership composition (6 preapproved tools, `leadership`), registry parity and cardinality, identity goldens, atomic receipts, sealed reads, cross-member and stale-before-replay, and every real Paseo route refused before freeze.

### Frontier B gates

- `cargo fmt -p` and `cargo clippy --all-targets -D warnings` for `kontor-core`, `kontor-mcp` and `kontor-daemon`: clean.
- `kontor-core` golden and `planning_pair`: 27.
- `-p kontor-mcp` (lib, bin, guard binary, seats): 89.
- `mcp_parity`, `mcp_cardinality` and `mcp_mutants`: 36.
- Paseo contract, filtered to `hosted_leadership`, `member` and `planning_pair`: 5.
- Daemon `loopback_api planning_pair::`: 23. `mcp_journey`: 2.
- Not one failure. The broad 1764-test run was not repeated: no daemon, runtime or adapter source changed.

### Frontier B mutation checks (seeded and run by this seat)

Pre-mutation SHA-256 values, each confirmed equal after restore:
- `kontor-core/src/planning_pair.rs` `be05929e4044a023ffeeb4525080ade6f3853906c289bd9272c66f3c2c7cf399`
- `kontor-mcp/src/registry.rs` `fd676a04270b0dda66e0d474d04ccaec6f309a50cfbff455aac4224b0b1b684c`
- `kontor-mcp/src/main.rs` `101fb34c256a860ebc3fb495c79e146a0665a5897a384aabf57e83ece5115a90`
- `kontor-runtime-paseo/src/client.rs` `02beaa2f63b10e26a424f4a6e1628b95f77cae049208206a24331b02f2b6ead7`
- `kontor-mcp/src/consultation_guard.rs` `c809cbc51a16dcf5418be09bd9cf2d45e8490120558fa743bbbdf99ab4362e41`
- `kontor-daemon/src/applications/planning_pair.rs` `511c456ca25e9c3620adafa3446a4df232afd3fa33ebdb935e74b277e2601eac`

| Id | Mutant | Filter (listed) | Red, then green |
| --- | --- | --- | --- |
| C-MUT-01 | Caller profile widening: the closed caller list gains the member's finding write | caller registry, caller server (2) | 0/2: both caller-surface tests failed. 2/2 |
| C-MUT-02 | Leadership widening: the leadership profile gains the caller's invoke | no-other-profile, caller registry (2) | 1/2. **Only** no-other-profile failed: "`leadership` must not serve kontor_planning_pair_run_invoke". 2/2 |
| C-MUT-03 | Implicit selection: an omitted `--serve-profile` selects the caller profile | caller selection, declared resolve (2) | 1/2. **Only** caller selection failed: "an omitted profile selects nothing". 2/2 |
| C-MUT-04 | Implicit selection at composition: a hosted leadership seat is composed with the caller profile | hosted leadership launch (1) | 0/1: "codex must receive its own leadership MCP". 1/1 |
| C-MUT-05 | Guard union: the guard accepts the caller profile as a member guard | malformed guard, member guard (2) | 1/2. **Only** the malformed test failed: the caller-profile guard allowed `Read`. 2/2 |
| C-MUT-06 | Auth generation bypass: the caller's hosted generation is not compared | caller-profile E2E, authority (2) | 0/2: both failed. Under the caller profile, the retired generation's replay was answered. 2/2 |

### Limits kept accurate (from audit 6f on `8b6ee064`)

- **Second-seat readback failure is untested, and qualification is per member.** Every readback-refusal test fails the first member launched (seat A). If seat B's readback failed after seat A was bound, seat A would stay bound; the pair would stay materializing with no receipt, and seat B would be unbound. No contract that unbinds both members is invented here.
- **The route refusal reports the first blocker only.** The surface refusal names only the first refused route, in seat order. When both routes are refused, it carries seat A's provider and gap and omits seat B's. This is a diagnostic limit, not an admission: every route is still refused before freeze.
- **The fully matched fake remains the hypothetical positive** that every caller-side protocol test runs against. Every real Paseo route is still refused before freeze with no effect, and with no match there is no authority.

## Frontier A: same-native member reconciliation, source seam only (2026-10-02)

> **Superseded in part (2026-10-02).** The LSA decided the operation's authority: the exact
> frozen caller. That operation is now implemented; see "Frontier A: the frozen caller's
> same-native member recovery". This section is kept as written at `75f253cb`.

TPM's separate frontier A dispatch builds on `e7a9404aa871c3f8c1ed29616182909b9b518ff7` (tree
`62377b57cbf057674759e2987095e1199c2cf03f`), after verify and audit passed.

This checkout owns the bounded runtime seam, its fake and fixture tests, and the docs. It has
no native, session, reconcile, probe, provider, credential, network, daemon-start, deploy or
policy effect. The caller profile, the leadership composition, placement and the root CLI are
unchanged. **The daemon recovery operation is not implemented.** Its authority is a genuine
authority question, returned to the LSA below instead of invented. The independent parts
continued.

### Returned to the LSA: who may recover a planning pair member

The only existing recovery is `kontor_consultation_seat_recover`
(`POST …/committee-runs/{id}/seats/{seat}/recover`). It is Committee-only, at the Admin tier
with ambient Admin authority, and it **replaces** the native: it archives the predecessor and
launches a successor at the next generation. This frontier forbids exactly that. A same-native
operation needs an actor, and the contracts point two ways:

- **Deriving the existing recovery's tier gives Admin.** But D-2 says that, for a planning pair,
  ambient credentials reach only the catalog and the observer projection, and that every write
  takes its actor from a scoped seat credential. With Admin as the actor, "a retired credential
  is denied before replay" has no generation to apply to.
- **D-2's actor model gives the frozen caller,** at its current hosted generation, on an Operator
  floor, with a retired generation refused before replay. That would be a fifth caller act, which
  the four-tool caller profile does not serve. This candidate allows no caller-profile widening.

| Option | Actor | What it needs |
| --- | --- | --- |
| A (this seat's recommendation) | The exact frozen caller at its current hosted generation | A new `kontor_planning_pair_seat_recover` at the Operator floor, served by no profile in this candidate. The request does compare-and-swap on the expected revision, the expected native id and the member's expected occupancy generation. The receipt kind is its own, `recover_planning_pair_seat`, which needs a migration widening the closed command-kind CHECK. |
| B | Ambient Admin, as for `kontor_consultation_seat_recover` | An explicit amendment to D-2's ambient-credential rule. |
| C | No new operation | Only the caller's existing invoke resume reconciles a materializing pair's known native. That needs the unqualified native identity persisted durably, which is a new identity meaning. Recovery of a running pair stays unsupported. |

**A second fact is returned with it, and is not changed here.** Qualification is per member, so a
bound seat A can record a finding while seat B is unqualified and the pair is still
materializing. That finding stays sealed from the caller. No contract decides whether a bound
member may contribute before its pair runs. The fixture below pins the behaviour as it is.

### What is implemented

| Element | Source | What it does |
| --- | --- | --- |
| Reconcile request | `kontor-runtime` `planning_pair.rs` `PlanningPairMemberReconcileRequest` | The frozen `PlanningPairLaunchContext`, the exact known `NativeRuntimeIdentity` and an instant. It carries **no credential**: the same session keeps the environment it was created with. `validate()` refuses a context with no occupancy generation, no actual vendor, or a requested provenance on another vendor. `require_same_native(outcome)` refuses a created session (`LaunchNotAdmitted`) and any other session, including another runtime generation or host (`CorrelationFailed`). |
| Runtime port | `adapter.rs` `RuntimeAdapter::reconcile_planning_pair_member` | An additive, opt-in method. It answers the existing `ConsultationLaunchOutcome` for the same session, `created` false, with the member surface and provenance read back again. An absent session is `StaleBinding`; another session or a drifted label is `CorrelationFailed`; an unreadable field is `Unsupported`. The **default** is `UnsupportedCapability { Resume }` before any effect. Paseo, AO and Codex keep the default, so no real route reconciles. |
| Fake runtime (hypothetical) | `fake.rs` | It reconciles only the exact session it created, under exactly the frozen context it was created with: any other run, seat, slot, generation, pin, route, vendor or placement is `CorrelationFailed`. It reads the surface back at call time, not from the launch's cached answer, and never mints. `losing_consultation_native(seat)` makes a session absent. `observing_planning_pair_member_field_unsupported_in(slot, field)` fails one slot's readback. Member launch and reconcile share one readback helper. |
| Daemon | unchanged | No operation calls the seam. The Committee recover route still cannot reach a member (test below). |

### Tests

| Suite | Test | What it proves |
| --- | --- | --- |
| `kontor-runtime` unit (3, new) | `a_reconcile_request_names_one_exact_member` | Generation 0, no vendor (`""`, blank or `unknown`) and provenance on another vendor are each refused with their own rule. |
| | `a_reconcile_answer_is_the_same_native_and_never_a_created_one` | Same session passes. A create is refused, and so are another native id, another runtime generation and another host. |
| | `a_reconcile_request_carries_no_credential` | The request's debug form names no credential. |
| Paseo contract (1, new) | `paseo_reconciles_no_planning_pair_member_native_and_refuses_before_any_effect` | Claude, Codex and Cursor routes, with seat MCP and a member-enforcing guard configured, are each refused as `UnsupportedCapability { Resume }`. There is no plane call, no composed file and no guard run. |
| Daemon loopback (4, new; 27 in the module) | `a_second_seat_readback_failure_keeps_the_first_member_bound_and_the_pair_materializing` | Only seat B's route is unobserved. Seat A launched first and stays bound to the same native; seat B stays unbound, named as `native/…`. There is no receipt and the pair stays materializing. The replay relaunches only seat B, on its same native, and nothing is retired or archived. Seat B's finding is `stale_binding`. Seat A's finding is recorded (200, still materializing) and sealed from the caller. |
| | `a_pair_whose_two_routes_are_both_refused_names_only_the_first_blocker` | Seat B's route alone is refused as `providers/claude-personal/not_composed`. With both refused, only `providers/cursor/not_composed` is named, `claude-personal` is absent, and nothing is frozen or launched. |
| | `the_hypothetical_fake_reconciles_only_the_exact_known_member_native_in_place` | Daemon-derived contexts drive these cases. Both members reconcile as the same native, with `created` false, provenance observed and every field matched, and no credential in the request. Seven drifts are each `CorrelationFailed`: another member's native, generation, route, vendor, placement, document and slot. A field withheld after launch reads back `Unsupported`. A lost native is `StaleBinding`. A withheld route is refused before the runtime is asked. Only reconcile calls are made: no launch, retire or archive. |
| | `the_committee_seat_recovery_route_never_reaches_a_planning_pair_member` | An Admin's exact Committee recover request on a pair's seat is `not_found` ("no such consultation run exists in this project"). No native is retired or launched, and both seats are unchanged. |

Several dispatch cells need the recovery operation and wait for the LSA's answer: retired before
replay, sealed findings and dissent kept through recovery, a disposed pair immutable under
recovery, exact replay against a different intent, and a concurrent resume's atomic receipt
under recovery. The existing invoke tests for those properties are unchanged and green.

### Frontier A gates

- `cargo fmt -p` for `kontor-runtime`, `kontor-runtime-paseo` and `kontor-daemon`: clean. Only this slice's files changed.
- `cargo clippy --all-targets -D warnings` for `kontor-runtime`, `-paseo`, `-ao`, `-codex` and `kontor-daemon`: clean.
- `-p kontor-runtime`: 64 lib tests, plus the 5, 7 and 8 integration tests.
- Paseo contract: all 292.
- Daemon `loopback_api planning_pair::`: 27.
- The `consultation`, `committee` and `advisor` filters: 23, the new Committee-route test included. The `recover`, `reroute` and `release` filters: 40, run before that test was added.
- `-p kontor-mcp`: 89. Guard profiles, the caller's 4 tools and the member's 3 are unchanged.
- Contract tests: `runtime_adapter` 52, `mcp_parity` 12, `mcp_cardinality` 12, `mcp_mutants` 12.
- `kontor-core`: `consultation_identity_golden` 2 and `planning_pair` 25.
- Not one failure. The broad 1764-test run was not repeated: daemon source is unchanged, the runtime change is an additive default method, and the fake refactor is covered by every planning pair test.

### Frontier A mutation checks (seeded and run by this seat)

Pre-mutation SHA-256 values, each confirmed equal after restore:
- `kontor-runtime/src/planning_pair.rs` `0e8927bd8ddf600c29cb6ce25488f3ab3c2ac3297642be856d5da7145e76e759`
- `kontor-runtime/src/adapter.rs` `b644af3fb6d37fc00b84ef8e20551ac1f9b9aa9e2365f2bfe29abb8b19dcb991`
- `kontor-runtime/src/fake.rs` `812123222f4de012b6b9fd9d732cc63c6c42c243e633f38cd13da9824e7ab6c3`
- `kontor-daemon/src/applications/planning_pair.rs` `511c456ca25e9c3620adafa3446a4df232afd3fa33ebdb935e74b277e2601eac`

Each mutant was reseeded on its own from clean source. The labels say whether it hits
production code or the fake's fidelity; a fake-fidelity kill is test-infrastructure evidence,
not production evidence.

| Id | Label | Mutant | Listed | Red (passed of listed), then green |
| --- | --- | --- | --- | --- |
| FA-MUT-01 | production, generation | A reconcile with occupancy generation 0 is accepted. | 3 | 2/3; only `…names_one_exact_member` failed. 3/3 |
| FA-MUT-02 | production, provenance | An `unknown` vendor counts as an actual vendor. | 3 | 2/3; only the same test failed. 3/3 |
| FA-MUT-03 | production, provenance | Provenance requested on another vendor is accepted. | 3 | 2/3; only the same test failed. 3/3 |
| FA-MUT-04 | production, no create | A created session passes as the same native. | 3 | 2/3; only `…never_a_created_one` failed. 3/3 |
| FA-MUT-05 | production, same-native correlation | The same native is compared by native id only, ignoring runtime generation and host. | 3 | 2/3; only the same test failed, at runtime generation 4. 3/3 |
| FA-MUT-06 | production, typed unsupported | The default reconcile answers `CorrelationFailed`. | 2 | 1/2; only the Paseo reconcile test failed. 2/2 |
| FA-MUT-07 | production, per-member qualification | The daemon checks qualification for seat A only, so seat B binds on a failed readback. | 6 | 5/6. **Only** the second-seat fixture failed: the invoke answered 200. The four older unqualified-member tests and the provenance test stayed green, because they never fail seat B alone. 6/6 |
| FA-MUT-08 | fake fidelity, frozen route, generation and pins | The fake's reconcile ignores the frozen context. | 1 | 0/1: "another generation" answered `Ok`. 1/1 |
| FA-MUT-09 | fake fidelity, no create | The fake's reconcile reports a create. | 1 | 0/1: `require_same_native` refused it. 1/1 |
| FA-MUT-10 | fake fidelity, readback now | The fake returns the launch-time observation. | 1 | 0/1: `Matched` where `Unsupported` was expected. 1/1 |
| FA-MUT-11 | fake fidelity, per-slot readback | The slot-scoped unobserved field is ignored. | 2 | 0/2: the seam test and the second-seat fixture. 2/2 |

### Limits kept accurate

- **No recovery operation exists.** No daemon path reconciles, resumes or rebinds a member. The seam is tested against the fake alone, as a hypothetical. Every real route keeps the typed unsupported default, and every real Paseo member route is still refused before freeze.
- **Per-member qualification is not atomic.** This is now tested, not just stated: seat A stays bound when seat B's readback fails. Seat A can contribute, sealed, while the pair is materializing. No contract unbinds both members, and none was invented.
- **The route refusal names the first blocker only.** This is now tested: with both routes refused, only seat A's provider and gap appear. It is a diagnostic limit, not an admission.
- **The kept unqualified native is not persisted.** It is named only in the refusal (`at`) and in the runtime's own labels. That is why option C would need a new durable identity record.
- **Resume is declared, not exercised.** The port allows a runtime to resume the same session in place. The fake models no stopped session.
- This is not a live qualification. Recovery's native effects need separate authorization.

## Frontier A: the frozen caller's same-native member recovery (2026-10-02)

TPM's frontier A dispatch builds on `75f253cb74fabf7d8c383ced850ae9d49fb59e11` (tree
`d9e728c7e193561b73004fca80ab49af671f4f35`), after its bounded verify and audit passed. That
candidate did not meet the full goal: it returned the operation's authority to the LSA.

The LSA decided under domain D-1 to D-3 / ADR-0008. This is a domain decision, not a platform,
identity-owner or native-authority acceptance. This checkout owns the operation, a forward
migration, the pair's known-native claim, its gates, tests and docs. Every native effect is
source and fixture only: no native probe, resume or create; no provider, credential, network,
daemon start, home, deploy, policy, pin, or new seat or workspace.

### The LSA decision, as implemented

- **Operation.** `kontor_planning_pair_seat_recover` is
  `POST /v1/projects/{project_id}/planning-pair-runs/{planning_pair_run_id}/seats/{seat_binding_id}/recover`.
  It is an `OpKind::Write` on the Operator floor.
- **Caller.** Only the exact frozen caller, authenticated first:
  - at its current hosted generation;
  - under the pinned document's allowed roles and scopes;
  - through its scoped seat identity.

  Ambient Admin and Operator, either member, another seat (including one the document admits by
  role), another pair's member and a retired caller are refused before any replay or runtime call.
- **Served by no profile.** Neither the caller's exact four, the member's three, leadership,
  worker nor consultation serves it. It appears only in the unprofiled whole-tier listing, like
  every tool. A future selection is separate.
- **Committee unchanged.** The existing Committee Admin replacing recovery keeps its hashes,
  endpoint and behaviour.
- **Closed body.** It holds `expected_run_revision`, `expected_member_occupancy_generation`, the
  full `expected_native_identity` (runtime kind, host, runtime generation, native id) and
  `expected_provider_session_id`, the last exactly when one is recorded. These are
  compare-and-swap assertions only. The path names the run and member, the credential names the
  caller, and the session is the bound seat's or its immutable claim's, never the body's. A member
  with no known session is refused, with no discovery, create or fallback. There is no route,
  provider, profile, generation, credential, replacement, archive or new-session field.
- **Receipt.** The kind is `CommandKind::RecoverPlanningPairSeat` (`recover_planning_pair_seat`),
  added in the forward migration 0123. The fingerprint covers:
  - the operation;
  - the run, the member seat and its slot;
  - the frozen caller binding and its current generation;
  - the expected revision and member generation;
  - the full native and session references;
  - the hash of the exact frozen member context;
  - the placement hash.

  It holds no secret.
- **Replay.** Authentication first. An exact key and intent replays the original durable receipt
  with no runtime call, even after the run moved or was disposed. A different intent conflicts.
- **New key.** The run revision, member generation, native identity and provider session are each
  compared, and the pair must not be terminal. Then the real-route check refuses before any effect,
  and the opt-in seam reads the same session back. That readback carries no credential and makes no
  generation bump, launch, replacement, archive, reallocation, or pin, model or vendor change.
- **Qualifying readback.** Every mandatory correlation, route and closed-restriction field must be
  `Matched`, and the current fleet provenance observed must equal the requested one, not a cached
  value. One store transaction then does all of this: the run CAS, the bind (if unbound) to the
  same session, the claim (written only if none is kept), the state, and the receipt. The
  receipt's `applied` is `created` or `unchanged`. A concurrent request for the key answers the
  winner's receipt.
- **Run state.** A materializing pair stays materializing; its invocation replay then runs it. A
  `needs_human` pair whose two members are both qualified again returns to `running`. A disposed
  pair is immutable to any new key.
- **Adverse readback.** This covers a stopped session the runtime cannot resume in place, a lost
  session, a drifted label, a misreported create, another provider conversation, or an unobserved
  field. It is refused as typed `unavailable`, with no receipt. One CAS withdraws only this
  member's current qualification. A running pair goes to `needs_human`, and a materializing one
  stays. The claim, the peer, every finding and the dissent are kept. No contract unbinds both
  members.
- **Second decision.** A qualified seat A records its own first finding while seat B is
  unqualified and the pair materializing. This rests on the full qualification binding, exact
  generation authentication before replay, the run CAS, the frozen profile, placement and slot, and
  the domain's one-finding rule on a non-disposed pair.
  - The finding is sealed from the caller, the peer and an observer; only seat A reads it back.
  - Seat B is denied. Seat A cannot rewrite, ask or decide.
  - There is no seat B qualification, running state, invocation receipt or recovery receipt. Seat A
    has its own contribution receipt.
  - Once seat B is requalified, both findings release through the original domain, with one
    clarification, hashes and dissent, and no gate.
  - A later lost qualification erases no finding. A new write needs a renewed qualification, and no
    second round or generation is synthesized.

### The durable known-native claim (schema 123)

`planning_pair_member_natives` is a pair-only, run-keyed sidecar (FK to the run and its frozen
placement), keyed by run, SeatBinding and occupancy generation. It is not a registry, a semantic
identity or `StoredConsultationSeat.native_identity`. It keeps:
- the runtime, native and provider session references;
- the launch's frozen-context hash and the placement hash;
- the observed-at time;
- the typed readback-refusal evidence (`PlanningPairReadbackRefusal`, a closed vocabulary).

Its triggers admit a claim only for a current member seat, at its current generation, of an open
planning pair, on its own placement. A claim is then immutable and permanent.

- **Writers.** Only a trusted runtime outcome writes it: a member launch's readback, or a verified
  exact-session recovery readback when none is kept. A caller body, label or alias never does.
- **Conflicts.** A claim already kept is never replaced. The same session is a replay; any other
  observation is a conflict.
- **Types.** The known claim (`StoredPlanningPairKnownNative`, projected as
  `members[].known_native`) is distinct from the qualifying bind (`observed_binding`). No secret is
  in the row, the DTO or a debug form.
- **Launch.** A member launch keeps its claim first: in one transaction with the bind when it
  qualified, and before the typed `unavailable` when it did not.
- **Replay and restart.** These meet the same claim: the same rule and native, no runtime call, no
  second create.
- **Lost acknowledgement.** With no supported observed id, nothing is claimed or guessed.
- **Unsupported routes.** They refuse before any claim, configuration or native effect.

### What is implemented

| Element | Source |
| --- | --- |
| Command kind, claim, refusal vocabulary, readback request | `kontor-core` `receipt.rs`, `repository.rs` (`StoredPlanningPairKnownNative`, `PlanningPairMemberReadback`), `planning_pair.rs` (`PlanningPairReadbackRefusal`) |
| Forward migration | `kontor-store/migrations/0123_planning_pair_member_natives.sql`: the claim table and triggers, and `command_receipts` rebuilt exactly as v122 with the one kind; `SCHEMA_VERSION` 123 |
| Store | `planning_pair_known_native`, `record_planning_pair_member_launch` (claim plus optional bind), `requalify_planning_pair_member` (receipt, member check, claim, bind, state and run CAS in one transaction), `disqualify_planning_pair_member` (member check, claim, unbind of this member only, state and run CAS) |
| API | `RecoverPlanningPairSeatRequest`, `PlanningPairNativeIdentityDto`, `PlanningPairSeatRecoveryDto`, `PlanningPairKnownNativeDto`, `members[].known_native`; the route, its port method, the router and OpenAPI; `contract/openapi.json` (+253) and `apps/console/src/api/schema.d.ts` (+179) regenerated, additions only |
| Daemon | `applications/planning_pair.rs` `recover_planning_pair_seat`; claim-aware member materialization; a typed `PlanningPairReadbackRefusal` qualification rule; `#[doc(hidden)]` `hold_planning_pair_recovery_writes` (no composed daemon installs one) |
| MCP | `kontor_planning_pair_seat_recover` with a closed object for the native identity; no profile change |
| Fake runtime (hypothetical) | `clearing_planning_pair_member_observation_faults`, `stopping_consultation_native` and `running_consultation_native_again` (a stopped exact session), `misreporting_planning_pair_reconcile_as_created`; a stopped session is reconciled as typed `StaleBinding` |

Paseo, AO and Codex keep the seam's typed unsupported default, so every real route is still
refused before freeze and before any recovery effect.

### Decisions taken inside the contract, for review

1. **Adverse versus no-observation.**
   - Readback adverse, which withdraws a current qualification: a runtime `StaleBinding` (absent or
     stopped), `CorrelationFailed`, `created: true`, another session, another provider
     conversation, an unobserved field or unconfirmed provenance.
   - No observation, which changes no state: an unsupported capability, a withheld route, or a
     transport error.
2. **Revision on invalidation.** Invalidation moves the run revision under compare-and-swap and
   writes no receipt, as for any refusal. A retry of that key therefore meets a revision conflict.
3. **Replay after a member fence.** The frozen member context in the fingerprint carries the
   member's current generation. After a member generation fence, an old key is an
   `idempotency_conflict`, not the old receipt.
4. **Claim guards.** A launch claim is guarded by the current generation, an open pair and its
   placement, not by the run revision: by the second decision, seat A's own finding may move the
   run while seat B launches. The recovery's own writes compare-and-swap on the revision.
5. **Who sees `known_native`.** Every viewer that sees `observed_binding` also sees `known_native`.
   It holds no secret.
6. **Error codes.** A member with no known session answers `unavailable`. A disposed pair answers
   the domain's terminal refusal (`revision_conflict`, "the aggregate is terminal and immutable").
7. **Changed existing behaviour.**
   - An invocation replay of an unqualified member no longer calls the runtime: it consumes the
     claim.
   - The first-launch refusal now advises `kontor_planning_pair_seat_recover`. Its `rule`, `at` and
     "confirmation unknown" are unchanged.
   - Frontier A's second-seat fixture now pins zero launches on replay, not one.
   - The canary counts moved deliberately: 211 mapped, 210 advertised, 212 documented.

### Tests

New daemon loopback tests (35 in the module):

| Test | Proves |
| --- | --- |
| `the_caller_requalifies_a_second_seat_on_its_same_native_and_the_replay_runs_the_pair` | Seat B's refusal-evidenced claim; the caller reads `known_native`; recovery `created`; the same native and generation; materializing stays; exactly one in-place readback, with no launch, retirement or archive; the claim unchanged; the exact replay `unchanged` with no runtime call; a different intent conflicts; one receipt; the invocation replay then runs with no launch |
| `only_the_frozen_caller_recovers_and_a_retired_caller_never_replays` | Ambient Admin and Operator, seat A, seat B, the TPM, the TPM admitted by role under a document that allows it, and another pair's member are each refused. The retired caller's replay of its own key is `stale_binding`; the successor generation's is `idempotency_conflict`. No refused request reaches the runtime, and there is one receipt |
| `a_recovery_asserts_the_exact_known_session_under_compare_and_swap` | Another native id, runtime kind, host, runtime generation, provider session or none, member generation, and a stale revision are each refused before the runtime, and a route field is refused by the closed body. A member with no known native (a launch refused with no session) is `unavailable` with nothing discovered or created |
| `a_durable_unqualified_member_survives_a_restart_without_a_second_create` | A restarted realm's invocation replay answers the same rule and native with no launch; the claim survives; the recovery then runs and the invocation runs |
| `an_adverse_readback_withdraws_only_that_members_qualification_and_never_replaces` | A stopped session and a lost one are refused; only that member is unbound; the claim, the peer and the finding are kept; running goes to `needs_human`; no receipt. The unqualified member cannot write. The same session running again requalifies, but one member alone stays `needs_human`. A lost session is never requalified or relaunched. No generation moves |
| `a_qualified_member_contributes_alone_sealed_and_the_pair_survives_requalification` | The second decision end to end, including sealing against caller, observer and peer reads; requalification; release; one clarification; loss, then `needs_human`, then requalification back to `running`; the answer; the disposition with dissent; the disposed pair immutable to a new key, while the exact old-key replay is `unchanged`; findings and dissent intact; exactly findings ×2 and one answer, all at generation 1 |
| `a_withheld_route_or_a_misreported_create_never_requalifies` | A misreported create is refused and binds nothing. A withheld route is `unsupported_capability` before any readback |
| `recoveries_invocations_and_contributions_classify_atomically_at_their_barriers` | Barriers placed after the readback and before the write. A recovery raced by a finding is a `revision_conflict` with no bind and no receipt. Two same-key recoveries give one `created`, one `unchanged`, one receipt and one revision. An invocation held after its CAS and before its receipt, with recoveries in between: the stale one conflicts, the current one is created, and both receipts are written once |

Other new or updated tests:
- **Store:**
  - `v123_keeps_every_receipt_and_admits_only_a_current_members_immutable_claim` covers the receipts copied unchanged, the new kind only, retired-generation, wrong-placement and disposed refusals, one claim per generation, and immutability and permanence.
  - The schema version and table list now include v123.
  - The v115 repair fixture now drops the new table, as 0122's tables are dropped.
- **MCP:** `the_member_recovery_is_an_operator_write_no_profile_serves` checks the tier, kind, route and closed argument list; that no profile serves it; and that the caller's and member's lists are exact.
- **Real adapters:** `ao_reconciles_no_planning_pair_member_and_makes_no_call` (every lane) and `codex_reconciles_no_planning_pair_member_and_makes_no_call` join the existing Paseo frontier A test.
- **Contract:** `mcp_parity` maps the tool's tier and its deliberate counts.

### Frontier A operation gates

- `cargo fmt -p` on the ten affected crates: only this slice's files changed.
- `cargo clippy --all-targets -D warnings` on those ten crates: clean.
- Test counts:
  - `kontor-core` 355;
  - `kontor-store` 612;
  - `kontor-runtime` 84;
  - `kontor-api` 35 (OpenAPI contract included);
  - `kontor-mcp` 90;
  - contract `mcp_parity`, `mcp_cardinality`, `mcp_mutants` and `runtime_adapter`: 88;
  - Paseo contract 292;
  - AO contract 62;
  - Codex contract 24;
  - daemon loopback `planning_pair::` 35;
  - daemon loopback consultation, committee, advisor, recover, reroute and release filters: 63;
  - `mcp_journey` 2.
- Not one failure. Old goldens, guard profiles, the caller's 4, the member's 3, the catalog migration, authorization and sealing, and the atomic receipts are covered by those suites. The broad 1764-test run was not repeated.

### Frontier A operation mutation checks (seeded and run by this seat)

Pre-mutation SHA-256 values, each confirmed equal after restore:
- `kontor-daemon/src/applications/planning_pair.rs` `1e74b50a673d21b29dadb5e0922237ad588902c2591a4bf31d4f78ca99f00abf`
- `kontor-store/src/repository.rs` `806de05b2c9625dceffb052844dca2e2a1defc951da229c1ada1c20c30df6d40`
- `kontor-store/migrations/0123_planning_pair_member_natives.sql` `588df92f0db4078d4a714ccf284b0ecf3b1b4df32a6f36e790e5cf008fe3f0cc`
- `kontor-mcp/src/registry.rs` `2aab2094359fa66abdbb5296c317d5b8371ddd2e3af9321753faa2971cded1b0`

Each mutant was reseeded on its own from clean source. Every one is a producer (production)
site; the earlier FA-MUT-08 to -11 fake-fidelity rows above stay as history.

**Where a result says "answered 200",** a request the test expects to be refused succeeded instead.

One store guard was removed before this batch so that the mutant could be meaningful. That
guard was the store's second check, under the same transaction, of the run revision and disposal
already enforced by the compare-and-swap `UPDATE`. Without that removal, FR-MUT-12 would have
survived as a redundant guard.

| Id | Label | Mutant | Listed | Red (passed of listed), then green |
| --- | --- | --- | --- | --- |
| FR-MUT-01 | production, exact caller | The frozen-caller identity is not compared | 1 | 0/1. It failed first at seat A, which then reached the hosted-generation check (`stale_binding`) instead of the exact-caller refusal. 1/1 |
| FR-MUT-02 | production, authentication before replay | The caller is authenticated after the replay lookup | 1 | 0/1: the retired caller's replay of its own key answered 200. 1/1 |
| FR-MUT-03 | production, member generation CAS | The member generation is not compared | 1 | 0/1: the other-member-generation case answered 200. 1/1 |
| FR-MUT-04 | production, full native identity | The session is compared by native id only | 1 | 0/1: another runtime kind answered 200. 1/1 |
| FR-MUT-05 | production, provider conversation | The provider conversation is not compared | 1 | 0/1: another provider conversation answered 200. 1/1 |
| FR-MUT-06 | production, claim consumed on replay | A replay ignores the kept claim and launches again | 5 | 3/5. **Only** the second-seat and restart fixtures failed ("no launch"). The three unqualified-member tests stayed green: they compare the rule and native, which a cached relaunch repeats. 5/5 |
| FR-MUT-07 | production, claim before refusal | Only a qualifying readback is kept as a claim | 2 | 0/2: no claim for seat B. 2/2 |
| FR-MUT-08 | production, current readback | A recovery binds without qualifying its readback | 2 | 1/2. **Only** the decision test failed: seat A's unobserved-correlation recovery answered 200. The adverse test's stopped and lost sessions are runtime refusals, not readbacks. 2/2 |
| FR-MUT-09 | production, same native, never a create | The daemon does not hold the readback to the same native | 1 | 0/1: a misreported create answered 200. 1/1 |
| FR-MUT-10 | production, adverse invalidation | An adverse readback keeps the member qualified | 2 | 0/2: "seat A lost its qualification". 2/2 |
| FR-MUT-11 | production, disposed immutable | A new recovery on a disposed pair is not refused as terminal | 1 | 0/1. Storage still refused, but not with the domain's terminal rule. 1/1 |
| FR-MUT-12 | production store, run CAS | The recovery write does not hold the run revision | 1 | 0/1: the recovery raced by a finding answered 200. 1/1 |
| FR-MUT-13 | production store, receipt classification | An exact replay is classified `created` | 2 | 1/2. **Only** the barrier test failed: `["created","created"]`. 2/2 |
| FR-MUT-14 | production store, no both-unbind | An adverse readback unbinds every member | 1 | 0/1: "seat B kept its own". 1/1 |
| FR-MUT-15 | production store, `needs_human` | A running pair keeps running when a member loses qualification | 2 | 0/2: `Running` where `NeedsHuman` was expected. 2/2 |
| FR-MUT-16 | production store, running needs both | A `needs_human` pair runs with one member qualified | 1 | 0/1: "one qualified member does not run the pair". 1/1 |
| FR-MUT-17 | production store, materializing stays | A recovery advances a materializing pair to running | 1 | 0/1: "materializing stays". 1/1 |
| FR-MUT-18 | production migration, current generation | A claim is admitted for a retired member generation | 1 | 0/1: "a retired generation: the claim was admitted". 1/1 |
| FR-MUT-19 | production migration, immutable claim | A kept claim can be rewritten | 1 | 0/1: "a kept claim never moves". 1/1 |
| FR-MUT-20 | production registry, profile widening | The leadership profile serves the member recovery | 1 | 0/1: "`leadership` must not serve the member recovery". 1/1 |

**Not seeded, because they are redundant guards and a kill would come from schedule or ordering:**
- The daemon's frozen-context-hash refusal, which the store's immutable claim comparison also enforces.
- The claim keeper's other-identity conflict: the member readback check and the replay's claim consumption already prevent a second session from reaching it.

### Limits kept accurate

- **Not live.** Every positive path runs on the hypothetical fake. Every real route keeps the typed unsupported seam and the pre-freeze route refusal. In-place resume of a stopped session is unsupported in this source: the fake reports it as `unavailable`, and nothing resumes. Recovery's native effects need separate authorization.
- **Two runtime readbacks under concurrency.** Two concurrent same-key recoveries both read the session back before one write wins, so the runtime sees two readbacks. Exactly one bind, one revision and one receipt result.
- **No receipt for an adverse recovery.** A refused recovery that withdrew a qualification records no receipt; the run revision and the seat show the change.
- **Per-member qualification is not atomic.** This is unchanged and tested. No contract unbinds both members. The first-blocker route diagnostic is unchanged.
- **Ancestors.** The Committee replacing recovery and every Advisor and Committee route, golden, receipt, authentication and generation rule are unchanged.

## Remaining capability gaps after slice four (current)

- **ASMA-8113 fence.** The reviewer is unassigned, so the widened identity vocabulary's acceptance, integration and deployment wait on an explicit assignment and an exact verdict.
- **Paseo member-surface capability (one row).**

  | Field | Value |
  | --- | --- |
  | Affected routes | Claude, Codex, Cursor and OpenCode members through Paseo |
  | Owner | The Paseo provider/runtime capability owner |
  | Next action | A supported, exact restriction readback for a created session: the applied closed tool set, the guard and the ambient-MCP exclusion. For Codex, a closed tool restriction that excludes the provider home's own servers; for Cursor and OpenCode, an enforced read-only boundary |
  | Today | Every route is refused before freeze with zero effects |

  This is not a Kontor orchestration repair, and no external-issue authority is claimed. TPM's plan row follows.
- **No live effect.** Nothing native has been created or qualified:
  - no native workspace or seat;
  - no credential propagation;
  - no topology or Team Definition publication on a live realm;
  - no live policy deploy, pin migration or native qualification;
  - no release.
  The bundled pack declares no planning pair container, so a governed pair needs an explicitly published topology kind, Team Definition container and document. TPM owns that placement, and the owning release workflow owns deployment.
- **Member seat recovery.**
  - **Same-native recovery is implemented at source:** `kontor_planning_pair_seat_recover`, as the exact frozen caller, on the member's immutable known-native claim (schema 123). See "Frontier A: the frozen caller's same-native member recovery".
  - **Real routes:** every one keeps the seam's typed unsupported default and the pre-freeze refusal, so nothing is qualified live.
  - **Not implemented:** the replacing recovery (provider loss, credential propagation), and in-place resume of a stopped session.
  - **Unchanged:** the Committee recover route, which still cannot reach a member.
  - **Failure mode:** a lost or unqualified member fails closed.
- **Caller serve profile.** An opt-in `planning_pair_caller` source profile now serves exactly the four caller tools when it is selected by name. No hosted seat is composed with it: no explicit optional selection seam exists, and none is inferred. The `leadership` profile is unchanged, so a hosted LSA still has no caller tools unless a client holding its scoped credential serves this profile.
- **Direct-mode consumer.** The asma-cli consumer (`_tools/asma-cli`, another checkout) is not written.
- **TPM-owned placement.** No TPM operation, receipt or procedure wraps placement.
- **Concurrent resume.** This is repaired for the planning pair:
  - the `running` advance is guarded by the run's CAS (the 6f rework, with a held launch gate);
  - the receipt is classified where it is written (turn 5, regression-tested with a hold after the CAS and before the write).
  The hold is a `#[doc(hidden)]` black-box seam in daemon code that no composed daemon installs. The Advisor and Committee invocations keep the earlier pattern. They are not repaired in this dispatch, which allows no broad unrelated repair.
- **Catalog authority.** A member's role now comes only from the epic's frozen-roster catalog. An epic promoted with no published Core Team still freezes the build's catalog, through the existing `epic_bootstrap_roster`. That is the epic's recorded selection, not a planning pair default.
- TASK-004 and TASK-002 are not closed by any slice.

## Remaining gaps after the audit remediation

_Historical: kept as written at `832e60cc`. The current list is under slice four above._

- **D-1, D-2 and D-3** were disposed by the LSA on 2026-10-02 (see the note under "Reserved decisions") for bounded implementation. The additive persistence, registered-operation and source-authentication slice follows only after verify and audit pass on this repair. The ASMA-8113 owner's compatibility review fences acceptance of the changed identity vocabulary. Live placement, release and qualification stay fenced under D-3.
- **Persistence and identity.** The durable record, its explicit input-sourced seam (`PlanningPairRecord::admitted` plus each transition's own input and returned address) and its fail-closed restore exist, but no store keeps them: there are no tables or migration. The run id type and the semantic identity (dedupe) hash wait on D-1. ASMA-8113 stays open and is consumed, not reopened.
- **Governed mode.** There are no daemon operations for a planning pair and no API route, MCP-routed tool, OpenAPI or console types, ASW/CSW placement, seat bindings or scoped credentials (D-2, D-3).
- **TPM-owned placement.** Only the direct CLI places a pair. No TPM operation, receipt or procedure wraps it, and the asma-cli consumer (`_tools/asma-cli`, another checkout) is not written.
- **Runtime.** There is no per-member `FleetLaunchProvenance` mapping, native launch or readback of the frozen members. Read-only authority is unrepresentable in the document, the run and the record, but is not yet read back from a launched harness. No real provider or Paseo launch was performed, and no daemon or CLI build was deployed.
- **Publication and qualification.** The document now has an immutable identity, but no registered publication (D-2), catalog, preset, ASW/CSW naming or Team Definition slot. The protocol is not yet qualified in either mode.
- **MUT-003** was executed by this seat: 7 of 7 killed at `3b05abce`, and sites a–c killed again on the remediation source. Its acceptance is the TPM-routed verify and audit of the committed head.
- **In-process authority is nominal.** A `PlanningPairActor` is a value. The governed service must authenticate the member and caller (D-2, D-3) before a transition runs.
- **Independent Review stays separate.** The general Committee cardinality fixture (`cardinality_is_data_not_three`: two reviewers, no Judge) is not `independent_review@1` and is not treated as one here. Independent Review remains the only formal gate, and its template and Judge requirement are decided separately.
- TASK-004 and TASK-002 are not closed by any slice.
