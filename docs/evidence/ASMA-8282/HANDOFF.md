# ASMA-8282 handoff — `planning_pair@1`

- Task: ASMA-8282 / TASK-004
- Workspace: TSW `wks_8062e92dee85f5b3`
- Branch: `feat/ASMA-8282-qualify-shared-advisor-and-committee-protocols`, local, not pushed, no pull request
- Orchestration: paseo-direct (plan `2026-09-27-20-14-plan-shared-orchestration-workflow.md`). No Kontor step, receipt or obligation is proposed here.
- Slice one (2026-09-29): the domain and the shared reader. It was fast-forwarded from `173d399bfd44ec598332b4fe1cdf882af024fe5c` to the published `739debaacfb0ace894b36114aaa1eef6835cdfc1` (`origin/feat/ASMA-8278-shared-orchestration-workflow`) before any feature edit, and committed as `d876055ba73f6d56577a60782888cfe4a0209df8` (tree `0b5b2cce8fbb9bdfaf70daf4e6265f48ef6139c3`).
- Slice two (2026-09-30, this revision): the direct CLI `planning_pair@1` mode. It is built on the accepted `d876055b` (tree `0b5b2cce`) and was dispatched by TPM generation 2 `7fc26706-dce2-4a02-be87-e0f933b25fd4` (predecessor `3c52abda-4fd0-4af6-b086-476bc51eb305`, archived). Every ancestor, `d876055b` included, is unchanged.
- The untracked directory `docs/evidence/KON-MVP-18/run-4d1b209d3fa9ea8e/` is disclosed e2e test evidence from slice one's workspace run. It is not part of either commit and is preserved untouched.

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

## Remaining gaps after slice two

- **Governed mode.** There are no daemon operations for a planning pair (invoke, findings, clarification, disposition, get). There are also no API route, MCP-routed tool, OpenAPI or console types, ASW/CSW placement, seat bindings or scoped credentials.
- **Persistence and identity.** There are no store tables or migration. `ConsultationFamily` and `ConsultationIdentity` are not extended, so a planning pair cannot be persisted, deduplicated or recovered yet. The ASMA-8113 identity work stays open and is consumed, not reopened.
- **TPM-owned placement.** The direct CLI now takes the caller-named keys and each member's eligibility, but no TPM operation, receipt or procedure wraps it. The asma-cli consumer (`_tools/asma-cli`, another checkout) is not written.
- **Runtime.** There is no per-member `FleetLaunchProvenance` mapping, native launch or readback of the frozen members. Read-only authority is unrepresentable in the document and the run, but is not yet read back from a launched harness. No real provider or Paseo launch was performed, and no daemon or CLI build was deployed.
- **Publication and qualification.** `PlanningPairSpec` has no profile id, revision, catalog, preset or registered publication, no ASW/CSW naming and no Team Definition slot entry. The protocol is not yet qualified in either mode.
- **MUT-003** ("missing finding or same-vendor collision passes a gate") is not performed. Its sites are `PlanningPairMembers::freeze`, `Activated::place_planning_pair` (the diversity), `PlanningPairRun::findings_complete`, `ConsultationProtocol::require_formal_review`, and, from slice two, the CLI's `placement.is_complete()` gate on `status: 200`. The supported run on the reviewed candidate remains required.
- **Independent Review stays separate.** The general Committee cardinality fixture (`cardinality_is_data_not_three`: two reviewers, no Judge) is not `independent_review@1` and is not treated as one here. Independent Review remains the only formal gate, and its template and Judge requirement are decided separately.
- TASK-004 and TASK-002 are not closed by either slice.
