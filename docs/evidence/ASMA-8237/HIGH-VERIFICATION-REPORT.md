# ASMA-8237 high-verification report

Date: 2026-09-20
Artifact: `high-verification-report-asma-8237-20260920`
Task: Jira `ASMA-8237` / Kontor `01a0b77d-110f-7401-ab3d-ca36da7ab5f7`
Phase: `high-verification`
TeamRun: `01a0bfd6-cb03-7160-a381-3d561b48e2bd`
Verifier AgentRun: `01a0c00a-9f3f-7422-a7e9-c75c955c26f9`
Candidate evidence head: `1e229b33aad855c2b1ff36ae0a2c11e152eb63af`
Candidate tree: `13f51b1f9d2a44f1e7db6be92321547db1908444`
Source repair: `da22b32398330c252d1f0e5bd3ac7487689a4449`
Refusal regression: `6ab089ef87599c20d1fe08f390096e8d69c9d7e3`
Comparison base: `9f61c74ad4c0ca859e4e4f665cb9a72c37a30fd2`
Status: **PASS**

## Verdict

The bounded ASMA-8237 correction passes independent verification.

The enforced model-route predicate now recognizes the six advertised Luna,
Cursor Auto, GLM 5.3 Flash and Nemotron provider/model routes at exactly the
explicit effort sets approved in the governing scope record. Guessed aliases
and off-ceiling efforts remain refused with the governed request-level error.
The correction changes recognition only: account/headroom resolution, role and
template authority, provider availability, default-provider selection and the
published watchdog chain are not weakened or bypassed.

The result is **PASS** at candidate evidence head `1e229b33`. This report does
not claim automatic failover, watchdog execution, deployment or a recorded
gate. Those are separate effects and were not performed by this verification.

## Authoritative inputs and exact boundary

- Governing record:
  `high-scope-record-asma-8237-20260920` revision 1,
  revision ID `01a0bfde-c2fe-7051-a52b-8b6d5a080a40`, approval receipt
  `01a0bfde-d8c0-70d1-b2f3-078d038ddfec`. The verifier read the immutable
  revision directly through Kontor memory history rather than relying on the
  implement handoff's summary.
- Implement handoff:
  `docs/evidence/ASMA-8237/IMPLEMENT-TO-VERIFY-HANDOFF.md` at `1e229b33`.
- The active seat was independently read back as role `verify`, TeamRun
  `01a0bfd6-cb03-7160-a381-3d561b48e2bd`, and AgentRun
  `01a0c00a-9f3f-7422-a7e9-c75c955c26f9`; task revision 2 was in
  `high-verification`.
- The complete post-`9f61c74a` candidate delta is three files: 127 lines in
  `crates/kontor-daemon/src/applications.rs` plus the two implementation
  evidence documents. `git diff --check 9f61c74a..1e229b33` passes.
- The production correction itself changes only
  `model_route_is_catalogued`; the rest of `applications.rs` is test-only.
  There is no runtime, store, migration, policy, schedule or watchdog-config
  delta.
- At verification start, remote `master` was
  `0f6498246d8c873a37453ec35e4a1db7d6be1468` and the remote feature branch was
  still `9f61c74a`. Publication of the implementation and this verifier report
  is deliberately a later, separately read-back operation.

## Acceptance findings

### Exact governed recognition

The new match arms reproduce the approved record exactly:

| Provider/model route | Explicit efforts admitted |
| --- | --- |
| `codex`, `codex-work`, `codex-personal` / `gpt-5.6-luna` | `low`, `medium`, `high`, `xhigh`, `max` |
| `cursor/auto-smart` | `low`, `medium`, `high`, `xhigh` |
| `opencode/openrouter/z-ai/glm-5.3-flash` | `low`, `high`, `max` |
| `opencode/openrouter/nvidia/nemotron-3-ultra-550b-a55b:free` | `medium`, `high` |

The original catalog regression separately proves those same explicit lists
are advertised exactly once, the three guessed aliases are absent, context
limits are not invented, and one default remains per affected provider.

### Governed refusal evidence

`parse_runtime_model_route` constructs and validates a requested rung, then
consults `model_route_is_catalogued`. A miss returns the exact domain refusal:

`RuntimeModelRouteRequest: the model route is not in the governed catalog`

The new focused test exercises that request parser for the three guessed
aliases and carries an admitted `codex/gpt-5.6-sol` control. Independent
mutation confirms both halves matter: blanket acceptance breaks the refusal
assertion, while removing the admitted control route breaks the control.

### Separate gates preserved

Static review of the full delta and adjacent call sites confirms:

- `QuotaOutlook::effective_rungs`, account selection and the scheduler's
  headroom walk are unchanged;
- provider availability remains checked by the runtime adapter at launch and
  route-correction boundaries;
- initial Committee recovery still requires a pinned template slot and exact
  catalogued routes;
- consultation recovery still requires exact aliases, runtime validation and
  provider-family diversity;
- `default_model_for_provider` is unchanged and has no Cursor entry, so Cursor
  cannot become a cross-family fallback default;
- runtime `provider_fallbacks` still validate and pass through the same
  governed predicate.

No test asserts these gates away. The new positive test proves only that the
approved routes are nameable.

## Independent mutation record

Mutations ran one at a time in a detached verifier worktree at exact candidate
head `1e229b33`; the authoritative checkout was never edited.

| Mutant | Independent result |
| --- | --- |
| `_ => false` to `_ => true` | **killed**: guessed `opencode/glm-5.3-flash` was accepted |
| remove existing `codex/gpt-5.6-sol` recognition | **killed**: the admitted control was refused |
| remove `cursor/auto-smart` recognition | **killed**: approved `cursor/auto-smart@high` was refused |
| add invented Nemotron `max` effort | **killed**: the off-ceiling refusal returned `Ok` |

Mutation score: **4/4 killed (100%)**. Every mutation was reverted with an
inverse patch. `git diff --exit-code` then passed in the verifier worktree, and
both focused tests passed again. The verified source SHA-256 is
`36d32b7466af966f897f0e7a0295abf361626be858bfeafa49fbd3835beb7156`.

## Test record

| Command | Result |
| --- | --- |
| focused request-refusal unit test | 1 passed, 0 failed |
| focused approved-route/ceiling unit test | 1 passed, 0 failed |
| `cargo test --locked -p kontor-daemon --test loopback_api the_model_catalog_ -- --nocapture` | 2 passed, 0 failed, 343 filtered out |
| `cargo test --locked -p kontor-daemon --lib` | 83 passed, 0 failed |
| `cargo fmt -p kontor-daemon -- --check` | passed |
| `cargo clippy --locked -p kontor-daemon --lib --tests` | passed |
| `cargo test --locked -p kontor-daemon --no-fail-fast` at candidate | 9 inherited failures; all other targets/checks passed |
| same broad command in detached worktree at `9f61c74a` | identical 9 failures |

The candidate and exact pre-repair baseline both produced 336 passed / 8
failed / 1 ignored in `loopback_api`, with the same eight failing test names and
the same response classes. Both also produced the same sole `mcp_journey`
failure: `placement_blocked` because the epic has no active immutable backlog
code. The source correction therefore neither introduced nor concealed those
failures.

## Limits and open questions

- Automatic pre-start failover remains unevidenced and is not part of this
  verdict.
- The watchdog/schedule was not started, resumed or changed.
- No deployment, Jira mutation, topology change, seat replacement, task
  lifecycle transition or gate write occurred during verification.
- No unresolved acceptance ambiguity remains. The two recorded implementation
  questions are resolved by the request-level refusal regression and the
  bounded recognition repair; no new open-question ledger entry is required.

