# ASMA-8112 release notes

Date: 2026-09-06

Release-gate verdict: **PASS**

Canonical artifact: `release-notes`

## Release identity

- Release subject reviewed: `17050eea142a6ada96b259c3712921199f64f3d8`.
- Production implementation: `9854faad10ce191960801de9c2a2c0a096c2fd33`.
- Review remediation: `92eab949df25b64b470430f2b150a8173f3bc33f`.
- Independent review receipt: `docs/evidence/ASMA-8112/REVIEW-NOTES.md` — PASS.
- Independent QA receipt: `docs/evidence/ASMA-8112/QA-REPORT.md` — PASS.

The crate tree at the release subject is byte-identical to the independently
validated remediation commit. This release evidence changes documentation only.

## Behavior released

The never-bound arm of `Services::replace_seat` keeps the existing exact-target
handoff authority and adds one narrowly defined recovery authority. An Admin may
replace the exact operator-abandoned, never-bound predecessor when the complete
candidate set contains exactly one pending, undispatched handoff for the same
`TeamRun` and role slot and that handoff has `target_agent_run = NULL`.

The successor is created in the same role slot and `TeamRun`, names the abandoned
predecessor as `parent_agent_run_id`, receives the original handoff message, and
becomes the dispatch target only when delivery succeeds. Replaying the same
idempotency key returns the recorded successor unchanged and launches no second
seat.

Zero candidates, multiple candidates, one candidate naming another run, and the
mixed `[targeted elsewhere, targetless]` set still refuse with no replacement or
dispatch mutation. No API, MCP, schema, migration, or store implementation
changes ship; the dispatch target was already nullable.

## Preserved fences

The independent review confirmed that the new targetless authority remains
behind all existing replacement fences:

- the predecessor is the exact operator-abandoned, unbound run and binding
  generation is zero;
- the requested role equals the predecessor's role and candidates match both its
  `TeamRun` and role slot;
- predecessor and task revisions are current;
- the `TeamRun` is non-terminal and no team-definition migration is in flight;
- the never-bound route accepts neither provider-outage nor quota evidence and
  requires an explicit model route;
- the HTTP route remains explicitly Admin-authorized;
- ambiguous, missing, stale, mismatched, bound, non-abandoned, and wrong-role
  requests cannot reach the added targetless authorization;
- command replay remains idempotent, successor lineage remains linked to the
  immutable predecessor, and the original AgentRun and dispatch records remain
  append-only history.

## Validated totals

Validation was discharged by the committed independent review and QA receipts;
the release turn did not rerun tests.

At remediation commit `92eab94`, two clean independent full runs of
`cargo test -p kontor-daemon` agreed exactly:

| Target | Result |
| --- | --- |
| `unittests src/lib.rs` | 71 passed |
| `unittests src/main.rs` | 2 passed |
| `tests/account_pinning.rs` | 5 passed |
| `tests/loopback_api.rs` | 283 passed, 1 ignored (284 enumerated) |
| `tests/mcp_journey.rs` | 2 passed |
| `tests/quota_observation.rs` | 21 passed |
| `tests/recovery_security.rs` | 6 passed |
| `tests/succession_handoff.rs` | 3 passed |
| Doc-tests | 0 |
| **Total** | **393 passed, 0 failed, 1 ignored** |

The one ignored test is pre-existing and unrelated:
`a_configured_jira_boundary_distinguishes_historical_from_native_completion`.
Formatting was clean and Clippy completed with no diagnostics.

QA then ran the focused `never_bound` loopback family from a fresh target at
subject `44e52aa`: **8 passed, 0 failed, 0 ignored, 276 filtered**. Those tests
exercise the successful targetless replacement with direct store readback,
same-key replay/idempotence and lineage; pre-abandonment and missing-route
refusal; two-targetless, mistargeted, and mixed-candidate ambiguity refusal; and
the existing never-bound/session/waiver boundaries. The broader green suite
retains the stale revision, binding-generation, role, terminal-team,
team-migration, Admin-route, bound replacement, and immutable-history coverage.
Mutation checks additionally proved that the positive fails against the old
gate and that only the mixed-candidate test kills the rejected narrower count.

## ASMA-8110 recovery impact

ASMA-8110 itself is unchanged. Its Kontor task
`01a07391-328e-74a3-a808-e7b5775c8438` remains in `high-verification`; live
readback records verifier run `01a07398-e78b-7443-a3e8-47559029debd` in role slot
`verify`, with no binding, terminal outcome `abandoned`, and no successor
lineage. The pending handoff for that same `TeamRun` and slot was derived after
abandonment and therefore recorded no target—the precise ASMA-8112 state.

After this daemon change is deployed, an Admin can use the existing
`kontor_seat_replace` route against that exact predecessor with current revisions,
generation zero, and an explicit governed model route. The retained handoff then
binds and instructs a linked successor verifier, allowing ASMA-8110's existing
high-verification work to continue. This repairs orchestration state only; it
does not alter ASMA-8110 code, artifacts, requirements, gate verdicts, or task
history.

## Deployment, readback, and rollback

Deployment is intentionally not performed by this release turn.

1. Merge the ASMA-8112 commits and deploy the resulting `kontor-daemon` through
   the normal daemon release path. No database migration or data rewrite is
   required.
2. Before recovery, read back ASMA-8110's task and exact abandoned verifier;
   confirm the task and predecessor revisions, role slot `verify`, null binding,
   abandoned operator outcome, non-terminal team, and one undispatched targetless
   handoff for that `TeamRun`/slot.
3. Invoke the existing Admin-only seat replacement once with generation zero,
   the read revisions, and an explicit governed model route.
4. Read back one bound successor whose parent is the abandoned verifier, the
   original dispatch delivered and targeted to that successor, and the old run
   retained. Replay the exact key and require `applied = unchanged`, the same
   successor id and binding, and no additional slot member.
5. Resume ASMA-8110 verification only after that readback succeeds.

Rollback is binary-only because this release has no schema or data migration.
Restore the preceding daemon artifact and restart through the normal service
procedure; do not attempt targetless recovery while rolled back because the old
gate will refuse it. Existing AgentRun and dispatch rows remain valid and
untouched, so the safe recovery is to redeploy ASMA-8112 and repeat the bounded
readback with the original idempotency key. Prefer roll-forward if a replacement
was already accepted, since replay is designed to converge on the recorded
successor.

## Gate decision

**PASS.** The implementation is a single guarded authorization change, its
existing safety fences and append-only model are preserved, independent review
and QA passed at exact recorded totals, and deployment has an explicit bounded
readback and rollback path.
