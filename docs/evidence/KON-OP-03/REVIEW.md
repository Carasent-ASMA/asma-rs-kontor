# KON-OP-03 code-review gate — review notes

Commit reviewed: `44bceeb feat(asma-7872): Expose the first /v1 application operations`
(super `72f1afa`; gitlink `44bceeb` == submodule HEAD).
Handoff reviewed against: `docs/evidence/KON-OP-03/ARCHITECTURE.md` (`548c9af`).

Verdict: **rejected** — receipt `01a00d02-7675-7331-8802-c3d3f973c16d`, gate
`code-review-gate`, sequence 1. Rejection routes to `implementation`.

## Mechanical checks

| Check | Result |
| --- | --- |
| `cargo test --workspace` | green — 105 suites, 1323 passed, 0 failed, 8 ignored |
| `crates/kontor-daemon/tests/loopback_api.rs` | green — 118 passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean, exit 0 |
| `cargo fmt --all -- --check` | **red — 5 violations** |

The formatting violations are all on lines this commit introduced:
`crates/kontor-api/src/applications.rs:2819,2900,2921` and
`crates/kontor-daemon/tests/loopback_api.rs:10944,10953`. `cargo fmt --all` fixes
them; this alone is not why the gate rejects, but it means the commit was
published without the check.

## What is correct in the delivered slice

The seven operations are coherent with the handoff and the quality is high.

- Route, OpenAPI operation, `ToolSpec` method/path/tier and generated client
  agree for all seven; the tier assignments match the handoff table exactly
  (specification family Admin, catalog and code help Observer).
- Only `kontord` resolves policy. Handlers parse ids, check capability and
  delegate; no policy leaked into the MCP or CLI client.
- The four new registry argument kinds route through the domain's own parsers,
  and `parse_domain`'s `_ => Ok(())` wildcard is replaced by an exhaustive match.
  This is a real safety improvement: a future identifier kind added to the string
  group and forgotten is now a compile error rather than a silently free-text
  argument.
- Contract-only handlers refuse with typed `Unavailable` → 503 before any effect,
  and the black-box test asserts no `receipt_id` on the refusal.
- `the_specification_routes_check_authority_before_they_find_no_service` is a
  genuine negative proof: it distinguishes a tier check that runs from one that
  was never wired, by requiring `forbidden` *before* `unavailable`.
- `SemanticTopologyTargetDto` is a closed tagged union over ids Kontor already
  owns. No node kind, parent, native id or `cwd` is accepted anywhere on the
  model-facing request side.
- The `docs/evidence/KON-MVP-18/run-*` directories were correctly left untracked.

## Why the gate rejects

### 1. Scope — 7 of ~51 operations, and no complete checkpoint

The handoff enumerates the surface: 7 specification/catalog/reference + 8
semantic topology + ~27 successor-ticket contracts + 9 capacity = ~51. Seven
landed. Measured against the handoff's own four coherent builder checkpoints:

- **CP1** — incomplete. The shared DTOs landed, but CP1's second half is
  "contract-only successor routes fail closed", i.e. the ~27 successor
  operations. None landed. `/v1/commands/{kind}` removal did not happen.
- **CP2** — half. Specification/catalog/code help landed (contract-only);
  the 8 semantic-topology operations and the two upgrade operations did not.
- **CP3**, **CP4** — untouched.

Not one checkpoint is complete. Passing this gate advances the task to `qa` and
then `release` on a 14% surface.

### 2. The shared vocabulary is half-landed and currently unreachable

Seven newly added DTOs are orphaned in the generated contract — present in
`contract/openapi.json` and in the console's `schema.d.ts`, referenced by no
operation: `RoleSelectionDto`, `ResolvedRoleRefDto`, `SemanticTopologyTargetDto`,
`TopologyNodeDto`, `TopologySeatDto`, `DesiredBindingDto`, `ObservedBindingDto`.

The console therefore ships TypeScript types for payloads no endpoint can return.
That is acceptable only if the consuming operations land in the same phase.

### 3. An explicit handoff instruction is unimplemented

> "Change the existing Delivery Team draft slot DTO from raw role JSON to this
> selection; do not add a second Delivery Team endpoint."

`TeamDraftRequest.slots` is still `Vec<serde_json::Value>` ("Slot declarations in
editor wire form") at `crates/kontor-api/src/applications.rs:631`.
`RoleSelectionDto` was defined and then not connected to the one existing
consumer the handoff named. Consequently the required negative proof

> "raw `role`, unknown role code or caller-supplied standard title"

is **not** killed — `kontor_team_draft_save` still accepts caller-authored role
JSON, which is the exact shortcut `RoleSelectionDto` exists to close.

### 4. `/v1/commands/{kind}` still violates the stated constraint

The handoff is unambiguous: `NON_AGENT_ROUTES` "may contain only health, OpenAPI
and genuine process probes", and `/v1/commands/{kind}` must be removed from the
public router and OpenAPI. It remains in all three:
`crates/kontor-api/src/lib.rs:233`, `crates/kontor-mcp/src/registry.rs:2340`,
and the generated contract (73 paths, `/v1/commands/{kind}` among them).

The builder's analysis of *why* is correct and worth preserving: ~15 loopback
call sites drive the generic idempotency/auth/revision proofs through that route,
so removal requires porting those proofs onto concrete routes first. That is an
argument for landing the concrete routes — CP1 — not an argument for shipping the
violation.

## Pre-existing behaviour, correctly flagged, not charged to this commit

Authority is checked inside the handler body, so axum's `Json` extractor runs
first: an authenticated but under-authorized caller sending a malformed body
receives 422 rather than 403. This is uniform across the pre-existing routes and
the new ones follow the established shape. It leaks schema-validity to an
under-authorized caller — worth a follow-up (an extractor-order or
`FromRequestParts` capability guard would fix it fleet-wide), but it is not a
regression introduced here and is not a reason for this rejection.

## To clear this gate

1. `cargo fmt --all`.
2. Complete CP1: land the ~27 successor-ticket contract operations failing closed
   with typed `unavailable`, using the same DTO → trait → handler → route →
   OpenAPI → `ToolSpec` pattern already proven here.
3. Migrate `TeamDraftRequest.slots` to `RoleSelectionDto` and add the negative
   proof that a raw role / caller-supplied standard title is refused.
4. Remove `/v1/commands/{kind}` from the router, OpenAPI and `NON_AGENT_ROUTES`,
   porting the ~15 generic loopback proofs onto concrete routes.
5. Land CP2's remaining 8 semantic-topology operations, which is what makes the
   orphaned topology DTOs reachable.

CP3 and CP4 remain after that; whether they belong to this task or to a split
successor is the orchestrator's call, but they are inside the task's stated scope
as written.
