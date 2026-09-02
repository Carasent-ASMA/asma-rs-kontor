# ASMA-8062 Team Definition and schema-v77 audit

> **Date:** 2026-09-01 22:19 EEST
> **Status:** 🔴 Blocked for merge, migration, and deployment
> **Category:** correctness and security audit
> **Scope:** Team Definition core/JSON, the v77 schema and repository handoff,
> exact native naming, identity preservation, partial replay, and governing docs
> **Snapshot:** `HEAD` `353775ce92fb0c2a5196d602d3b15c3e117e59ff` plus the live handoff diff observed at 22:19 EEST. The tracked diff SHA-256 was `92ab4c84665743d3bc3494518f0e81a37663a48e925c836d8805c70a7bf7f99a`; untracked v77 SQL SHA-256 was `f3204ab537f91d517b3a00b24e0a3d0f12e81c4ab7b44c059de6171eac6f344b`.

No implementation or verification-owned file was edited by this audit.

## Verdict

The recommended JSON fixture renders the agreed UTF-8 names exactly and the
local renderer refuses missing topic/slot values. The persistence and migration
contract is not release-safe yet: it can publish a definition without proving
its referenced topology bytes/rules, confirm a migration without the desired
title having read back, address neither seat targets nor Kontor's full native
identity, cross project boundaries on a first pin, and strand a partial apply in
a terminal state that no same-key replay can resume.

**P0:** 0. No migration command is exposed by the audited tree, so the critical
repository flaws below are not presently reachable as a live command path.

**P1:** 9 release blockers. **P2:** 3 documentation/verification blockers.

## P1 findings

### P1-01 — Publication does not validate against the referenced topology

`TeamDefinitionSpec::canonicalize` performs only local validation
(`crates/kontor-core/src/spec.rs:998`). `publish_team_definition` then proves
only that `(project_id, spec_id, version)` exists
(`crates/kontor-store/src/repository.rs:14657`); it neither reads that topology
nor calls `TeamDefinitionSpec::validate_against`.

Consequently a definition with a forged topology hash, illegal kind/parent,
over-broad capability, or contradictory read-only policy can become immutable,
selected, and epic-pinned. This violates plan REQ-001/REQ-004 and the claim that
topology remains the legality validator.

**Gate:** publication tests must kill wrong-hash, absent-kind, illegal-parent,
capability-escalation, and read-only-policy mutants at the store boundary.

### P1-02 — Confirmation trusts a status label instead of exact title readback

`observe_team_definition_migration` checks only `native_id`, then stores caller
supplied `observed_title` and `state` independently
(`crates/kontor-store/src/repository.rs:15047`). Confirmation checks only that
the state is `renamed` or `unchanged`
(`crates/kontor-store/src/repository.rs:15115`). It never requires
`observed_title == desired_title` or even `observed_title.is_some()`.

A `Renamed`/`Unchanged` observation with a missing or wrong title therefore
moves the epic pin to bytes the native objects do not render.

**Gate:** encode and test the state/title invariant in both repository logic and
the schema: success states require the exact desired title; missing/different
titles remain pending or failed and cannot confirm.

### P1-03 — The target model cannot prove the required objects or identity

Each target stores only `topology_node_id`, one bare `native_id`, desired title,
observed title, and state
(`crates/kontor-store/migrations/0077_team_definition_naming.sql:159`). The
agreed migration covers containers **and seats** and reads back native id,
parent, kind, cwd, and title. Kontor's existing native identity is also a tuple
of runtime kind, host, generation, and native id, not the bare id.

The target key cannot represent multiple seats on one topology node, has no
seat-binding identity, omits host/generation/parent/kind/cwd, and has no foreign
key to an owned topology node. The verification fixture currently proves the
gap accidentally: it generates a nonexistent node/native pair and successfully
records and confirms it (`crates/kontor-store/tests/team_definition_persistence.rs:345`).

**Gate:** enumerate every container and seat target using its existing durable
binding and full native identity; persist the required readback tuple; refuse
missing, foreign, extra, or omitted targets before the first runtime effect.

### P1-04 — First-pin migration can cross project ownership boundaries

The v77 tables reference `projects(id)` and `mini_projects(id)` separately
instead of enforcing the `(project_id, mini_project_id)` relationship
(`crates/kontor-store/migrations/0077_team_definition_naming.sql:60` and `:96`).
`record_team_definition_migration` compares the current pin by the supplied
project/id pair but does not prove ownership
(`crates/kontor-store/src/repository.rs:14895`). For a foreign unpinned epic,
that lookup returns `None`, which equals `from: None`; confirmation then inserts
a snapshot pairing the foreign epic with the attacker's project.

Targets have the same problem: neither SQL nor repository code proves that a
target node belongs to the migration project and epic.

**Gate:** enforce composite ownership in SQL and in the transaction, then test
two projects with unpinned/pinned epics, foreign topology nodes, and foreign
native bindings.

### P1-05 — Terminal `failed` breaks same-key recovery after partial effects

`fail_team_definition_migration` is allowed from both `recorded` and `applying`,
makes the intent terminal, and removes the materialization fence
(`crates/kontor-store/src/repository.rs:15211`). After one native has already
been renamed, the epic still has the old pin while part of runtime has the new
title. New materialization may resume under that old pin. Re-recording the same
key returns the terminal intent, while observe/confirm refuse terminal intents,
so the approved same-key recovery path cannot resume.

**Gate:** settle OQ-A8062-02 below, then test a crash/failure after every target,
same-key replay, no recreation, no state regression, and continued fencing until
the runtime/pin relation is proven coherent.

### P1-06 — Reusing an idempotency key does not prove the same intent

The replay path looks up only `(project_id, idempotency_key)` and returns the
stored migration without comparing epic, from/to snapshots, or target set
(`crates/kontor-store/src/repository.rs:14895`). A key accidentally reused with
a different semantic request is reported as success rather than conflict.

**Gate:** define the semantic replay fingerprint (excluding only explicitly
non-semantic retry fields), persist/compare it, and test changed epic, target,
native identity, desired title, and destination revision.

### P1-07 — Project selection is not compare-and-swap

The accepted plan requires publish/select under compare-and-swap. The repository
port carries no expected prior selection, and the store performs an unconditional
upsert (`crates/kontor-core/src/repository.rs:3319` and
`crates/kontor-store/src/repository.rs:14762`). The live daemon handoff also
auto-selects the first bundled definition after observing no default. A
concurrent explicit selection can therefore be overwritten by stale bootstrap.

**Gate:** selection apply must bind the previewed current revision/absence and
candidate hash in one transaction; add the stale-preview race test.

### P1-08 — New Team Definitions admit the legacy naming vocabulary

The shared `NativeNameToken` still contains compatibility tokens
`AREA_CODE`, `JIRA_CODE`, `KONTOR_BACKLOG_CODE`, `ITEM_CODE`, and
`AI_SHORT_NAME` (`crates/kontor-core/src/naming.rs:136`).
`TeamDefinitionSpec::validate` validates templates but does not restrict them to
the agreed Team Definition vocabulary
(`PREFIX`, `EPIC_ITEM_CODE`, `TASK_ITEM_CODE`, `SCOPE_ITEM_CODE`, `TOPIC`,
`ROLE_CODE`, `SLOT_DISPLAY_NAME`). A new immutable definition can therefore
publish the exact legacy/Jira/title-derived naming sources the approved contract
forbids.

**Gate:** retain legacy tokens for historical topology reads, but reject them in
new Team Definition publication; add one refusal test per legacy token.

### P1-09 — Supported export/import omits the new authoritative state

The backup exporter is intentionally a typed allowlist, not a dynamic table
dump (`crates/kontor-store/src/backup/export.rs:1`). Its `ExportedRecords`
declaration includes topology specs/defaults/pins but none of
`team_definitions`, `project_team_definition_defaults`,
`mini_project_team_definition_snapshots`, migration intents, or migration
targets (`crates/kontor-store/src/backup/export.rs:573`). Import likewise has no
materialization path for them. The omission is not declared in
`redaction_summary`, so an export can appear complete while losing the new
naming authority and recovery state.

**Gate:** version the export schema, export/import the immutable definitions,
selections, pins, and resumable migration evidence in dependency order, and
prove byte/hash exactness plus a mid-migration round trip.

## P2 findings

### P2-01 — Nested JSON is not uniformly fail-closed

The aggregate, container, and slot structs deny unknown fields, but
`TopologySnapshot`, `NativeNameTemplate`, and `NativeNameSegment` do not. Unknown
nested JSON can deserialize and be discarded before canonicalization, so the
accepted document is not the literal request document.

**Gate:** add unknown-field tests at every nested level and reject rather than
silently normalize unrecognized input.

### P2-02 — Governing documentation describes incompatible contracts

`docs/NATIVE_NAMING.md` says conformance is pending and presents a nested,
camelCase/map-shaped semantic JSON example. The committed implementation emits a
flat snake_case/vector shape. `docs/CONFIGURATION.md` simultaneously describes
Team Definition ownership as current behavior, calls it a pending target, and
says the definition owns roles, skills, contexts, and handoffs that the current
`TeamDefinitionSpec` does not contain. See OQ-A8062-01 and OQ-A8062-03.

**Gate:** resolve the two ledger questions and make one document/schema snapshot
normative; label historical topology behavior separately.

### P2-03 — Verification does not cover the dangerous negatives or upgrade path

The exact renderer/profile tests and empty-database v77 migration test pass. The
new persistence suite exercises happy paths but currently has 12/14 passing; its
two topic tests fail while creating the epic topology node because the fixture
did not pin a mini-project topology. It does not cover P1-01 through P1-08.
There is no populated v76→v77 upgrade test, complete migration crash matrix, or
Team Definition export/import round trip.

**Gate:** add focused negative/mutation tests, repair the fixture, and run the
populated upgrade plus full archive gate after the active seats converge.

## Open-question ledger

### OQ-A8062-01 — Normative Team Definition JSON wire shape

**Subject:** exact external JSON field/collection shape.

**Attaches to:** `docs/NATIVE_NAMING.md` “Recommended definition shape”,
`TeamDefinitionSpec`, the operational-domain fixture, and future API/OpenAPI/MCP
schemas.

**Why ambiguous:** the approved document says its camelCase/nested/map field
names express the semantic contract and that the implementation audit must
decide schema evolution. The implementation chose flat snake_case fields and
ordered vectors, but no decision record declares that shape normative or the doc
example illustrative.

**Options seen:** (1) make the implemented snake_case/vector form normative;
(2) implement the documented camelCase/nested/map form; (3) version an explicit
wire DTO while keeping the Rust/storage shape internal.

### OQ-A8062-02 — Meaning of terminal `failed` after a runtime effect

**Subject:** whether an applying migration may ever become terminal before
runtime and pin agree.

**Attaches to:** `TeamDefinitionMigrationState::Failed`, v77's partial index,
open-question decision OQ-KBI-8062-003, and recovery operations.

**Why ambiguous:** the state docs permit abandonment and clear the fence, while
the approved decision requires same-key recovery after partial external effects.
Both cannot hold after any target has changed.

**Options seen:** (1) allow terminal failure only before the first effect; (2)
keep apply failures nonterminal/retryable; (3) add an explicit quarantined state
that remains fenced until an operator proves a coherent resolution.

### OQ-A8062-03 — Team Definition aggregate content

**Subject:** whether Team Definition owns only hierarchy/naming/slot display
policy or the full roles/skills/contexts/handoffs contract.

**Attaches to:** `docs/CONFIGURATION.md:133`, `docs/NATIVE_NAMING.md`, and
`TeamDefinitionSpec`.

**Why ambiguous:** the naming contract and current type describe an epic-wide
naming aggregate, while configuration docs assign it broader team-template
responsibilities. That changes identity, revision, validation, and migration
scope.

**Options seen:** (1) keep Team Definition naming-focused and correct the docs;
(2) move the broader team configuration into it; (3) keep separate immutable
references and document their composition.

## Required release gates

1. Close every P1 and the three open questions before merge or live schema/native mutation.
2. Prove store publication composes against the exact topology hash and legality rules.
3. Prove complete container **and seat** enumeration, full native identity, and exact parent/kind/cwd/title readback.
4. Run the crash/replay matrix at every pre-effect, partial-effect, readback, pin-commit, and lost-response boundary.
5. Run cross-project/foreign-node security tests and stale-selection CAS tests.
6. Add populated v76→v77, foreign-key-check, restart, supported snapshot, and portable export/import tests.
7. Restore green focused persistence tests, `cargo check -p kontor-daemon`, API/OpenAPI/MCP parity, and the exact committed archive verification gate.
8. Only then use merged exact-master binaries for backup, schema migration, live retitle, and unchanged-ID/title readback receipts.

## Checks observed at this snapshot

- PASS: `cargo test -p kontor-core --test team_definition_naming` — 3/3.
- PASS: `cargo test -p kontor-profiles --test operational_domain` — 13/13.
- PASS: empty-database current-schema test — 1/1.
- FAIL: `cargo test -p kontor-store --test team_definition_persistence` — 12/14; both failures stop at missing mini-project topology pin.
- FAIL: `cargo check -p kontor-daemon` — the live implementation handoff had an unmatched delimiter in `applications.rs`; rerun after seat convergence.
