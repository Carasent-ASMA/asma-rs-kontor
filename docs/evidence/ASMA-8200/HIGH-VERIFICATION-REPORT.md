# ASMA-8200 corrective high-verification report

Date: 2026-09-20
Artifact: `high-verification`
Task: Jira `ASMA-8200` / Kontor `01a0ac9d-a95c-7291-91e8-b472b7b331bc`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-verification`
TeamRun: `01a0b033-78bd-7691-90e0-a8a34eb46030`
Base: `0f6498246d8c873a37453ec35e4a1db7d6be1468`
Candidate: `e16cd98d819b9f34d99ad3930794bc2d3f939090`
Candidate tree: `efb5f2d0fe922aec0eefd5d60ea31118a92063cc`

## Verdict

**PASS for the ASMA-8200 source correction.** HV-8200-001 is closed on the
current merged integration base. A present valid durable capacity policy is now
selected before the seed is validated; an absent durable policy still requires
a valid seed; and a present unusable durable policy still fails closed as
`StartupError::StoredCapacity` regardless of whether the unused seed is valid.

One non-blocking P2 documentation defect remains: the Rustdoc on
`applications::stored_capacity` still claims that the configuration read
surface calls that helper. The corrective high-change record now describes the
two paths truthfully, and the stale source comment has no runtime effect, so it
does not reopen HV-8200-001 or withhold this source verdict. It should be
corrected before final evidence closeout.

This report authorizes no merge, deployment, daemon restart, capacity write,
topology mutation, Jira transition or watchdog action. Release and live
convergence proof remain with the root owner named by the handoff.

## Candidate boundary

The exact corrective commit was read and tested from isolated checkout
`/private/tmp/kontor-8200-capacity-precedence`, branch
`fix/ASMA-8200-authoritative-capacity-seed`. The checkout was clean before
verification. The original rejection at `b8303a02` remains historical evidence;
it was not amended or substituted for this report.

Relative to merged base `0f649824`, the commit changes only:

- `crates/kontor-daemon/src/lib.rs`; and
- `docs/evidence/ASMA-8200/HIGH-CHANGE-RECORD.md`.

The production change is one policy-ordering move: `seed.validate()` leaves
`start_with_supervision` and runs only in `capacity_in_force`'s absent-row
branch. The remainder of the Rust diff updates startup documentation and adds
or strengthens the real-start regressions. There is no schema, route, DTO, CLI,
MCP, capacity-value, topology, Jira, runtime or watchdog change.

## Verified decision table

| Durable row | Seed | Result |
| --- | --- | --- |
| absent | valid | seed selected; daemon starts |
| absent | invalid | `StartupError::Capacity`; no credentials; lock released |
| valid | valid | stored policy selected |
| valid | invalid | stored policy selected; seed is irrelevant |
| malformed | valid or invalid | `StartupError::StoredCapacity` |
| domain-invalid | valid or invalid | `StartupError::StoredCapacity` |

The authoritative-row regression writes a complete policy
`9/7/5/3/2/6`, adaptive `2/1/5/1`, through an isolated real `SqliteStore`, then
starts the same realm with a zero global seed. It proves the realm identity is
preserved and `Daemon::config().capacity` equals the durable policy exactly.

The absent-row regression proves the zero seed refuses before credential
generation, then immediately starts the same root with a corrected seed. That
second start proves the refused attempt released the state-root lock.

The stored-policy refusal regression exercises both a missing required field
and a zero ceiling against both a valid seed and an invalid seed. All four cases
return `StartupError::StoredCapacity`, so neither fallback nor seed-validation
ordering can mask an unusable durable policy.

## Independent mutation evidence

### V-MUT-8200-01 — reintroduce the original unconditional seed validation

The verifier moved `seed.validate()` above `get_capacity_configuration` and
removed it from the absent-row branch, recreating HV-8200-001 without changing
any other behavior.

Command:

```text
cargo test --locked -p kontor-daemon --lib \
  a_present_stored_capacity_is_authoritative_over_an_invalid_seed -- --nocapture
```

Result: **KILLED**. The retained test failed 0/1 with the exact historical
failure:

```text
a valid stored policy is authoritative over an unused invalid seed:
Capacity { source: Invalid {
  subject: "CapacityConfig",
  rule: "every ceiling must be positive"
} }
```

The mutant was removed and the exact test then passed 1/1.

### V-MUT-8200-02 — bypass absent-row seed validation

The verifier removed only the `seed.validate()` call from the absent-row branch.

Command:

```text
cargo test --locked -p kontor-daemon --lib \
  an_invalid_seed_without_stored_capacity_refuses_start_and_releases_the_root \
  -- --nocapture
```

Result: **KILLED**. The test failed 0/1 because `Daemon::start` returned a live
daemon configured with the zero ceiling instead of `StartupError::Capacity`.
The mutant was removed and the exact test then passed 1/1.

After both mutation runs, `git status --porcelain=v1` and `git diff --check`
were clean before the verification report was added. No mutant remains.

## Test record

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | pass |
| `git diff --check 0f649824..e16cd98d` | pass |
| `cargo check --locked -p kontor-daemon --all-targets` | pass |
| `cargo clippy --locked -p kontor-daemon --lib --tests -- -D warnings` | pass |
| `cargo test --locked -p kontor-daemon --lib` | pass, 86/86 |
| `cargo test --locked -p kontor-daemon --test loopback_api capacity` | pass, 6/6 |
| `cargo test --locked -p kontor-daemon --no-fail-fast` | pass, 545 passed, 0 failed, 1 intentional ignore |
| `cargo test --locked -p kontor-api --test openapi_contract` | pass, 3/3 |
| `cargo test --locked -p kontor-tests-contract --test mcp_parity` | pass, 12/12 |

The complete daemon run includes 420 passing loopback tests, two passing MCP
journey tests, and every daemon unit and integration target. The capacity
restart regression remains green and proves effective readback, revision and
realm identity stability, cleared `restart_required`, and scheduler-facing
mission ceiling 5 after reopen.

## Historical baseline questions on the actual base

The original report's baseline questions were observations about old candidate
base `2e4b9985`, not requirements to recreate that drift on current merged base
`0f649824`. They are resolved for this candidate:

- repository-wide rustfmt passes, so the old `kontor-teams` formatting hunk is
  absent;
- the checked-in OpenAPI document matches the generated document, 3/3; and
- the complete daemon suite is green, including the formerly failing MCP
  bootstrap journey.

No exception, assumption or artificial scope expansion is needed for those
historical results.

## Finding V2-8200-01 — P2 — stale helper Rustdoc

`crates/kontor-daemon/src/applications.rs:14681-14682` says:

```text
The startup loader and the read surface both come through here
```

That remains false. Startup calls `stored_capacity`; the configuration read
surface directly deserializes `StoredCeilings` and preserves its existing
conversion path. The corrective addendum in `HIGH-CHANGE-RECORD.md` now states
that fact accurately, so the handed evidence no longer makes the false claim.

Recommended correction: describe this helper as the startup loader's durable
document conversion and validation seam, without claiming the read surface
calls it. This is documentation-only and does not affect the verified policy
selection or failure behavior.

## Open questions

None. The exact candidate, base, mutation outcomes, historical baseline
disposition and remaining P2 documentation defect are all evidenced above.

## Handoff

HV-8200-001 is closed for source integration at `e16cd98d`. Root retains
authority for merge, release gates, deployment and the guarded live convergence
proof. A later evidence-only correction should remove V2-8200-01's stale
Rustdoc, without changing the verified runtime behavior or rerunning
implementation under a duplicate seat.
