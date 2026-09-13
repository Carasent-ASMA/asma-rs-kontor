Artifact: gate-repair-report-round-8

# ASMA-8116 — repair of F-8116-V17

Verifier receipt: `01a09883-d858-7221-be31-b5dc2e7caf43`
Rejected candidate: `3ee6e76`
Verifier report: `HIGH-VERIFICATION-ROUND-8.md`, commit `a93c734`, preserved verbatim
(SHA-256 `6233e0a91dd2ec59055f213e61b0a1b98c8f3d1c2bbf5f2f41736ff467019df7`, verified)

## The defect

The alias read proved immutable issue `901`. The write-boundary read of the
canonical key could answer `999` while every protected mutable field stayed
byte-identical — so the observation digest compared equal, and Kontor posted
`/transitions` before anything later blocked. A digest answers "has anything a
human could move changed?"; it cannot answer "is this the same issue", because
two issues can present identical protected fields.

## The repair

**Identity is carried and compared separately from the digest.**

1. `ExpectedObservation` gains `issue_id: Option<ExternalId>` — the immutable
   issue the plan was *proved* against. It sits beside the digest, never inside
   it, so the `observation_hash` contract is untouched and old evidence still
   compares equal.
2. All three write-request builders populate it from the observation the plan
   was computed from, which is the initially proved identity.
3. `JiraConnector::validate_expected` compares it against the identity the
   write-boundary read just returned, **before** the digest check and before any
   effect, refusing with a typed `JiraError::refused`. No new
   `StatusConflictKind` variant was added: widening that published closed enum
   is not in this repair's bounds.

**The binding advance is deferred behind that agreement.** `decide_jira_identity`
takes `advance_now`; the epic path passes `false` and the rename is committed by
`commit_same_issue_rename` only once the dry run has validated. A rebind
therefore leaves the ledger untouched — the proof precedes the advance, not just
the effect. The task path is unchanged (`true`): it does not re-read at a write
boundary in the same pass, and its effects are covered by the same expectation.

No extra read, no numeric inference, no change to legacy `JiraItemCode`, tokens
or naming. The current-key selector and the canonical-key hash fix from earlier
rounds are retained.

## Regression

`a_write_boundary_rebind_is_refused_before_any_effect_or_binding_advance` drives
the exact two-read rebind: the alias read answers `901`, the write-boundary read
answers `999`, and both carry identical key, status, assignee, update token and
body so the digest agrees throughout. It asserts the pass actually reached the
write boundary (≥2 issue reads — otherwise it would prove nothing), that no
transition was posted, that `renamed` is 0, that the binding still reads
`ASMA-1`, and that no rename occurrence was spent.

## Mutations — three seeded, three killed, restored

| # | Mutation | Result |
| --- | --- | --- |
| M17a | Remove the immutable-ID comparison at the write boundary | **killed** |
| M17b | Misroute it — compare the proved id against itself | **killed** |
| M17c | Advance the binding before the boundary agrees (`advance_now: true`) | **killed** |

Restoration verified by inspection of each mutated site.

## The one focused test that had to change, and why

`the_resident_reconciler_follows_a_same_issue_rename_through_the_connector`
failed after the repair with `renamed: 0`. That is the new invariant working:
its fixture stopped before the dry run, so under "advance only after
write-boundary agreement" the binding correctly did not move.

**No assertion was weakened.** The fixture was instead brought to the boundary
its name already claims, so the rename is genuinely followed end to end:

- an explicit legacy backlog code is assigned (a code, never a Jira key), which
  placement needs before it derives an item code;
- the shared responder now offers the route the *governing* workflow targets per
  subject kind — `10236` for epics, `10231` for tasks — instead of `10231` for
  both, which was a plan conflict for an epic and stopped the pass early;
- the responder's body became opt-in (`readable_body`). A case about the write
  boundary needs a readable body, because an unreadable one raises a content
  conflict first; the content-conflict case
  (`a_same_issue_rename_reports_its_content_conflict_against_the_current_key`)
  opts out and keeps its original meaning.

## Gates — isolated `CARGO_TARGET_DIR` throughout

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy` (4 crates, `-D warnings`) | clean |
| `cargo test -p kontor-store --test jira_materialization` | ok, 30/30 |
| `cargo test -p kontor-store --test schema_v1` | ok, 58/58 on re-run; one earlier run 57/58 — see below |
| `cargo test -p kontor-jira` | ok, 9 + 19 |
| `cargo test -p kontor-api` | ok, 23 + 5 + OpenAPI 3 |
| `cargo test -p kontor-daemon --lib` | ok, 81/81 |
| `cargo test -p kontor-daemon --test loopback_api jira` | ok, 9 (1 predeclared ignored) |
| reconciler / rename / rebind targets (7) | ok |

`schema_v1` failed 57/58 on one run and passed 58/58 on the next. That is
**OQ-002**'s ledgered intermittent concurrent-first-open failure, whose
attribution remains unsettled; it is not claimed as settled here either way.

## Remaining inherited failure

`an_epic_placeholder_body_is_typed_reported_and_repairable` still fails with
`503 unavailable — the configured native Jira connector could not answer`. It is
the inherited **ASMA-8123** placeholder-body failure: it failed identically at
`3ee6e76`, at `b8e9549` and at `560db2c`, including with this round's and the
previous round's connector changes reverted in the same clean target. It is
outside F-8116-V17's scope and is recorded, not fixed.

## Preserved

Every earlier repair, legacy `JiraItemCode`/token/hash behaviour, ASMA-8117
ownership, OQ-002/OQ-004/OQ-005 and migration `0095` ordering. No audit, merge,
deploy, Jira or topology action.
