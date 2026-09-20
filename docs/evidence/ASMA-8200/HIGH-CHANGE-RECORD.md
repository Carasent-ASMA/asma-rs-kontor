# ASMA-8200 high-change record

Date: 2026-09-17
Artifact: `high-change`
Task: Jira `ASMA-8200` / Kontor `01a0ac9d-a95c-7291-91e8-b472b7b331bc`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-change`
TeamRun: `01a0b033-78bd-7691-90e0-a8a34eb46030`
AgentRun: `01a0b033-9bec-7b83-8eff-ce5d414b8434` (`implement`)
Scope: `HIGH-SCOPE-RECORD.md`, frozen — restart-time activation only.

Worktree `asma-modules/.worktrees/asma-8200/asma-rs-kontor`, branch
`feat/ASMA-8200-enforce-persisted-capacity-configuration-at-runtime`, based on
`2e4b9985fc1073dd25c22b78349d8f36d6b1d4ec`.

## Change

Three files, 380 insertions, 11 deletions. No schema, route, CLI, MCP, DTO or
hot-reload surface was added, and `DEFAULT_CAPACITY` is unchanged.

### 1. One startup loader at the composition root

`crates/kontor-daemon/src/lib.rs:358` adds `capacity_in_force`, the only reader
of the durable policy on the start path. `start_with_supervision` calls it at
`lib.rs:463`, immediately after `SqliteStore::open` has opened and migrated the
store and before anything is composed:

```rust
config.capacity = capacity_in_force(&store, config.capacity)?;
```

The result is written back into `DaemonConfig::capacity`, so `Services::new`,
the startup log line and `Daemon::config` all have one answer to what the
process admits under rather than three. `DaemonConfig::capacity` is documented
as the *seed* it now is.

Precedence is the one the scope fixed: a present stored row is authoritative,
an absent row leaves the seed in force. The read happens exactly once, because
the composed ceilings and every admission plan built from them are
process-lifetime state — re-reading later would split one process between two
policies, which is the reason the read contract reports `restart_required`
instead of promising a live reload.

The loader sits in `start_with_supervision`, the funnel *both* `Daemon::start`
and `Daemon::start_configured` delegate to, so the executable's path and the
embedding path activate identically. There is no second composition site:
`Services::new` has exactly one call site, `lib.rs:432`.

### 2. Fail-closed handling of a present, unusable policy

`StartupError::StoredCapacity` (`lib.rs:168`) refuses the start when a *present*
configuration will not read back as ceilings this build understands, or carries
a set `CapacityConfig::validate` refuses. A backend read failure maps to the
existing `StartupError::Store`. Absence is not an error.

The conversion is reused, not duplicated. `applications.rs:13165` exposes
`pub(crate) fn stored_capacity`, which wraps the existing private
`StoredCeilings` deserialization and the existing `capacity_config`
wire-to-domain conversion and validates the result. The startup loader and the
configuration read surface now go through the same function, so the realm cannot
compose one policy and report another. The store was not taught any scheduler
semantics.

### 3. Truthful `restart_required` on apply

`apply_capacity_configuration` (`applications.rs:20504`) previously hardcoded
`restart_required: true`. It now compares the document it just wrote against the
composed ceilings:

```rust
restart_required: written != ceilings_dto(self.capacity),
```

An apply of values already in force therefore no longer claims a restart is
owed — which, after activation, is the ordinary case. `ceilings` still echoes the
written document and `stored_ceilings` stays `None` on apply; neither contract
changed. `capacity-config-get` was not modified: it already reported the
effective policy in `ceilings`, a differing durable replacement in
`stored_ceilings`, and `restart_required` exactly when the two differ.

### 4. Regressions

`a_reopened_realm_admits_under_the_stored_capacity_and_the_scheduler_projects_it`
in `crates/kontor-daemon/tests/loopback_api.rs` covers the five required
properties in order: the seed stands with no stored row (`restart_required`
false, no `stored_ceilings`); an applied, complete, coherent policy differing
from the seed in every ceiling leaves the running process reporting the old
effective values and `restart_required: true`; the state root is stopped and
reopened through the shipped startup path; the realm identity and configuration
revision are unchanged, the effective document equals the stored document field
for field, `restart_required` is false and no differing `stored_ceilings`
remains; and `mission_ceiling` on `/v1/projects/{id}/capacity` — the scheduler's
own copy, not the configuration endpoint's echo — moves from 12 to 5. The test
closes by proving a re-apply of the ceilings now in force reports
`restart_required: false`.

`a_stored_capacity_the_composition_root_cannot_honour_refuses_the_start`
(`lib.rs:1422`, `1432`) covers the refusal for both failure modes — a document
that will not deserialize and one carrying a zero ceiling. The scope permitted
this only if the condition could be created without weakening the production
write boundary: it is created by writing the row directly through an isolated
`SqliteStore` in the test, and `apply_capacity_configuration` still validates
before it writes, so nothing crossing the public boundary can produce one.

The existing compare-and-swap/replay regression
`the_capacity_configuration_reports_the_operational_ceilings_and_guards_its_revision`
is unchanged and green.

## Mutation evidence

One mutant, seeded alone, in the production code at `lib.rs:463`:

```rust
-        config.capacity = capacity_in_force(&store, config.capacity)?;
+        let _ = capacity_in_force(&store, config.capacity)?;
```

This bypasses the persisted-policy override while deliberately leaving the
fail-closed refusal path intact, so the result attributes the kill to the
override itself and not to the refusal.

| Mutant | File:line | Suite | Result |
| --- | --- | --- | --- |
| Bypass the startup override | `crates/kontor-daemon/src/lib.rs:463` | `a_reopened_realm_admits_under_the_stored_capacity_and_the_scheduler_projects_it` | KILLED, twice over |

Run 1 — effective readback, at the post-restart configuration assertion:

```
assertion `left == right` failed: the reopened realm admits under exactly the
stored policy, field for field
  left:  {"global_max_in_flight":24,...,"mission_max_in_flight":12,
          "adaptive":{"initial":4,"floor":1,"ceiling":12,"growth_step":1}}
  right: {"global_max_in_flight":9,...,"mission_max_in_flight":5,
          "adaptive":{"initial":2,"floor":1,"ceiling":5,"growth_step":1}}
```

The mutated build also reported `"restart_required":true` with a differing
`stored_ceilings` after a full restart — the exact OG-054 drift shape.

Run 2 — same mutant, the test temporarily narrowed past the assertion above so
execution reaches the scheduler-facing projection:

```
assertion `left == right` failed: the scheduler counts against the reopened
ceiling
  left:  Number(12)
  right: 5
```

`"mission_ceiling":12` with `"adaptive_width":4` on
`/v1/projects/{id}/capacity` is the seed, not the stored policy. The projection
is therefore pinned independently of the configuration endpoint's echo.

The mutant and the temporary test narrowing were both reverted, the reversion
was verified by grep and `git status --porcelain`, and the suite reruns green.
Nothing from the mutation pass remains in the candidate.

## Checks run

| Check | Result |
| --- | --- |
| `cargo fmt -p kontor-daemon --check` | pass |
| `cargo fmt --all --check` | **fail — pre-existing, out of scope; see the open question** |
| `cargo check -p kontor-daemon --all-targets` | pass, no warnings |
| `cargo test -p kontor-daemon --lib` | 82 passed, 0 failed |
| new restart regression | pass |
| existing capacity-configuration regression | pass |
| `the_configured_capacity_and_not_a_compiled_one_decides_what_is_admitted` | pass |

Independent verification owns the broader suite and the contract parity reruns.

## Open questions

### OQ-8200-01 — the frozen base is not `fmt`-clean outside this task's boundary

**Subject.** `cargo fmt --all --check`, one of the scope's five minimum
implementation checks, fails at the frozen base commit
`2e4b9985fc1073dd25c22b78349d8f36d6b1d4ec` for a reason ASMA-8200 does not own.

**Attaches to.** This `high-change` record, and the "Minimum implementation
checks" paragraph of `HIGH-SCOPE-RECORD.md`.

**Why the state is ambiguous.** The single offending hunk is
`crates/kontor-teams/tests/team_contract.rs:467`, a `.unwrap_or_else(|| panic!(...))`
closure rustfmt joins onto one line. It is not caused by this change and is not
a local-toolchain artifact: `rust-toolchain.toml` pins channel `1.97.1`, and the
drift reproduces under that pinned `rustfmt 1.9.0-stable`. So the required check
cannot pass without editing a file in `kontor-teams`, which the scope's
implementation boundary does not grant, and the scope records no exception.

**Options seen.**

1. *(taken)* Leave `team_contract.rs` untouched, prove formatting in scope with
   `cargo fmt -p kontor-daemon --check`, and report `--all` red with its exact
   cause. Keeps the candidate diff exactly the ASMA-8200 change, which is what
   independent verification is being handed. Cost: one named minimum check is
   red at handoff.

2. Include the one-line reformat in this candidate. Turns the check green at the
   cost of putting an unrelated crate into the reviewed diff and into whatever
   convergence proof follows it.

3. Fix it separately, on its own task, and rebase. Cleanest, and the slowest;
   it blocks this handoff on work outside the epic.

**Chosen pending a decision.** Option 1. It is reversible in one command
(`cargo fmt --all`) if the verify seat or the scope owner prefers option 2, and
it is the only option that does not silently widen a frozen boundary.

## Handoff

The candidate is the working tree described above: three files under
`crates/kontor-daemon`, uncommitted, on the preserved TeamRun and worktree.

Not done, and deliberately so — the scope withholds all of it until independent
verification: no merge, no gate recording, no Jira transition, and **no
deployment**. Implementation boundary item 5 (the one guarded rebuild/redeploy
of this fleet, and the exact post-restart identity and capacity readback proving
the live target — revision 1 at `20/13/12/4/4/13`, adaptive `4/1/12/1`,
`restart_required` false, identities preserved) is therefore still open and
belongs after the verify slot clears this candidate.

Hand to the original `verify` slot with OQ-8200-01 unresolved and visible.
