# ASMA-8282 handoff — `planning_pair@1`, first slice

- Date: 2026-09-29
- Task: ASMA-8282 / TASK-004, first slice only
- Author: first writer in TSW `wks_8062e92dee85f5b3`
- Branch: `feat/ASMA-8282-qualify-shared-advisor-and-committee-protocols`, local, not pushed, no pull request
- Base: fast-forwarded from `173d399bfd44ec598332b4fe1cdf882af024fe5c` to the published `739debaacfb0ace894b36114aaa1eef6835cdfc1` (`origin/feat/ASMA-8278-shared-orchestration-workflow`) before any feature edit. Every ancestor is unchanged.
- Orchestration: paseo-direct (plan `2026-09-27-20-14-plan-shared-orchestration-workflow.md`). No Kontor step, receipt or obligation is proposed here.

This record is implementation evidence and a handoff. It is not verification. It
does not close TASK-004 or TASK-002, and it claims no mutation acceptance.

## Consumed, not reopened

- The qualified identity source `e29b895d0f045c499144870a2fa610fc642b6cc0` and the deployed recovery source `173d399bfd44ec598332b4fe1cdf882af024fe5c` are ancestors of this slice and are not modified. The consultation identity (`ConsultationIdentity`, `ConsultationFamily`) and consultation recovery code paths are untouched. The only change to `crates/kontor-core/src/consultation.rs` makes its private `has_duplicate` helper `pub(crate)`, so the planning pair's list rules reuse it.
- ASMA-8113 stays open.
- OG-01 through OG-05 and ASMA-8101 contract items 11–12 are out of this slice.

## Decisions for this slice (Igor, 2026-09-29)

| Question | Decision |
| --- | --- |
| Surfaces | Domain (`kontor-core`) and the shared reader (`kontor-fleet-activation`) only. No CLI, registry, daemon, store, API, OpenAPI or migration change. |
| Member binding keys | The caller names one existing binding key per member (`team/…`, `committee/…`, `advisor/…` or `leadership/…`), exactly as joint `--allocation` slots do. There is no new key family and no parser or policy-schema change. |
| Handoff | This new file. `docs/evidence/ASMA-8280/DIRECT-MODE-HANDOFF.md` is unchanged. |
| Final run | `cargo test --workspace --no-fail-fast` once on the final tree. |

## What is implemented

| Element | Source | What it does |
| --- | --- | --- |
| Explicit protocol selection | `crates/kontor-core/src/planning_pair.rs`: `ConsultationProtocol` (`advisor`, `planning_pair@1`, `independent_review`), `select_protocol` | The caller names the protocol. `select_protocol(None, …)` is refused (`NO_PROTOCOL`), and a protocol that is not available is refused (`UNAVAILABLE`). The answer is never a different protocol, so a planning pair never stands in for an Independent Review, and an Advisor never stands in for either. |
| No gate from advice | `ConsultationProtocol::is_formal_review`, `require_formal_review` | Only `independent_review` passes. An Advisor or a planning pair is refused with `MissingAuthority { subject: "FormalReviewGate", rule: NOT_FORMAL }`, even after a unanimous `accepted` disposition. The existing completion gate (`kontor-scheduler` `CompletionObservation::VerdictRecorded`) is unchanged. It accepts only a `CommitteeVerdict` and a `CommitteeRunId`, and a planning pair has neither. |
| Document | `PlanningPairSpec`, `PlanningPairMemberSpec` | `protocol` must be `planning_pair@1` (`NOT_PLANNING_PAIR`). `members` is exactly `seat-a` then `seat-b` (`MEMBERS`), titled `SEAT A` / `SEAT B` (`PlanningPairSlot::label`). Each member has only `specialty`, `behavior` and the existing pin-only `ConsultationContextPolicy`. There is no `judge`, `aggregation`, `quorum`, `diversity`, `round_limit` or clarification-count field, and `deny_unknown_fields` refuses each one. `validate` / `canonicalize` follow the Advisor and Committee rules for callers, scopes and budget. The document has no id, revision or registry yet (see "Not in this slice"). |
| Protocol bounds | `FINDINGS_ROUNDS = 1`, `MAX_CLARIFICATION_ROUNDS = 1` | These are fixed by the protocol version, not by document data. A successor that changes a bound is a new protocol version. |
| Frozen members | `PlanningPairMember`, `PlanningPairMembers::freeze` | Exactly two members, `seat-a` then `seat-b`, each with the caller-named `binding_key`, the chosen `route` and the actual `vendor`. `freeze` refuses an empty or `unknown` vendor (`VENDOR_UNKNOWN`) and two members on the same vendor (`SAME_VENDOR`), whatever their account aliases or harnesses. The members carry the `placement_hash` of the receipt they were frozen from. Nothing can change a member once frozen: there is no setter, no replacement and no third slot. |
| One independent findings round | `PlanningPairRun::admit`, `record_finding`, `findings`, `state` | Only `Member(slot)` records that slot's finding (`MEMBER_ONLY`), and only once (`FINDING_IMMUTABLE`). `findings()` is `None` until both findings are durable, so neither the caller nor the other member can read a partial round. The run is deliberately not `Serialize`, so no rendering can leak one finding early. Each contribution's `document_hash` is canonical and bound to the document hash, the placement hash, the round and the slot. |
| At most one caller-requested clarification | `request_clarification`, `record_answer`, `clarification` | Only the caller asks (`CALLER_ONLY`), only after both findings are recorded (`FINDINGS_INCOMPLETE`), and only once (`EXTRA_CLARIFICATION`, both while the round is open and after it is answered). The question addresses one or both members, each once (`ADDRESSEES`). Only an addressed member answers (`NOT_ADDRESSED`), once (`ANSWER_IMMUTABLE`), and never without a question (`NO_CLARIFICATION`). Answers are released only when every addressed member has answered. |
| Caller disposition, dissent retained | `PlanningPairDisposition`, `MemberDisposition`, `record_disposition`, `retained_dissent` | The caller records one decision, which reuses the Advisor's `AdviceDisposition` vocabulary. It waits for every requested answer (`ANSWERS_INCOMPLETE`). It must list `seat-a` then `seat-b`, each citing its exact finding hash and its exact answer hash, or none if it gave none. Omitting, reordering, duplicating or rewriting a member is refused (`DISSENT_LOST`). `superseded` is refused because this is the only decision (`FIRST_DISPOSITION`). The disposition has no verdict, gate, aggregate or settlement field. After it, every operation is `Terminal`. `retained_dissent()` returns every finding and answer of a member the caller rejected or only partly accepted, verbatim. Both findings stay readable. |
| No Judge, quorum, conjunctive verdict or settlement | whole module | `PlanningPairState` is `awaiting_findings`, `findings_released`, `awaiting_answers`, `answers_released` and `disposed`. No state is settled, and nothing computes an outcome from the findings. |
| Placement from the activated snapshot | `crates/kontor-fleet-activation/src/lib.rs`: `PlanningPairRequest`, `PlanningPairMemberRequest`, `PlanningPairPlacement`, `Activated::place_planning_pair`, `place_planning_pair` | The request is `{members: [{slot, binding_key, unavailable_accounts?, excluded_vendors?}]}` and has no diversity or role field, so neither can be waived or set to Judge. PP-01 refuses anything but `seat-a` then `seat-b`. The request becomes a `JointAllocationRequest` with both members as `reviewer` under `distinct_vendor_per_reviewer`, and runs through the existing `Activated::allocate`: one `load`, each key resolved by `Activated::resolve`, and `kontor_fleet::allocate` choosing. There is no second parser, resolver or allocator. The independence key is the policy's model vendor, so aliases on one maker, or a Cursor route to an Anthropic model beside Claude, count as one vendor, and an `unknown` maker cannot be placed. |
| Receipts | `PlanningPairPlacement { protocol, selection, placement_hash, members? }` | `selection` is the existing `JointSelection`, verbatim: `ActivationProvenance` (policy hash and schema, and under a v2 activation the bundle and roster hashes), and each member's binding, chain, policy exclusions, eligibility, considered candidates and choice. `placement_hash` is the canonical hash of `{schema_version: 1, protocol: "planning_pair@1", selection}`. `members` is present only when both were placed. A blocked allocation is the defined block result, and nothing is frozen from it. PP-02 covers the unreachable case where the receipt is not canonical or the placed members do not freeze. |

## Tests

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

### Gates on this tree

- `cargo fmt -p kontor-core -p kontor-fleet-activation -- --check`: clean. The format was applied per crate, and only the new code changed.
- `cargo clippy -p kontor-core -p kontor-fleet -p kontor-fleet-activation --all-targets -- -D warnings`: clean.
- `cargo test -p kontor-core --test planning_pair`: 18 passed. `cargo test -p kontor-fleet-activation`: 18 passed.
- `cargo test --workspace --no-fail-fast` on the final tree: the result is recorded in the body of the commit that adds this file, because writing it here would change the tree it describes.

## Not in this slice

- **Governed mode.** There are no daemon operations for a planning pair (invoke, findings, clarification, disposition, get), and no store tables, migration, API route, MCP tool, OpenAPI or console types, ASW/CSW placement, seat bindings or scoped credentials. `ConsultationFamily` and `ConsultationIdentity` are not extended, so no planning pair can be persisted or deduplicated yet.
- **Direct CLI.** `kontor_fleet_policy_resolve` has no planning-pair mode. Today a direct consumer can call `--allocation` with two `reviewer` slots under `distinct_vendor_per_reviewer`. That returns the same `JointSelection`, but not the protocol, the `placement_hash` or the frozen members.
- **Publication and qualification.** `PlanningPairSpec` has no profile id, revision, catalog, preset or registered publication. There is no ASW/CSW naming for it, and no Team Definition slot entry yet.
- **Launch provenance.** No per-member `FleetLaunchProvenance` mapping or runtime readback exists yet. The receipt it would be mapped from is the `JointSelection` above.
- **Read-only authority on the actual harness.** Read-only is unrepresentable in the document and in the run, but it is not yet read back from a launched harness.
- **TPM-owned placement.** The caller-named keys and eligibility are the inputs a TPM would supply, but there is no TPM operation around them yet.
- **A Committee template with two reviewers and no Judge still validates** (the existing `cardinality_is_data_not_three`). It is a Committee, not a planning pair: it produces a conjunctive verdict. Whether formal Independent Review requires its Judge at gate time is not decided in this slice.
