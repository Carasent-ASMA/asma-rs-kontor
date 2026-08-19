# KON-OP-04 implementation record

Date: 2026-08-17
Status: composed behind the OP-03 boundary
Scope: Project Core Team, Quick sessions, QSW-to-ESW promotion, epic-roster upgrade

Companion to `ARCHITECTURE.md`. That document decides; this one records what was
built against it, and the two places the build could not follow it literally.

## What is composed

All ten successor contracts named in the architecture's table now answer from
durable state rather than a typed `Unavailable`:

| Contract | Backed by |
| --- | --- |
| `GET /core-team` | `core_team_revisions` (schema v30) |
| `POST /core-team:preview` / `:apply` | pure resolve + `apply_core_team` receipt |
| `POST /epics/{id}/core-team/seats:materialize` | `epic_rosters` + OP-02 chain |
| `GET /quick-roles` | derived from the current Core Team |
| `POST /quick-sessions:ensure` | `quick_sessions` (schema v31) |
| `POST /quick-sessions/{id}/promotion:preview` / `:apply` | `quick_session_promotions` |
| `POST /epics/{id}/roster:upgrade-preview` / `:upgrade-apply` | `epic_rosters` |

Five command kinds were added: `apply_core_team`, `ensure_quick_session`,
`promote_quick_session`, `materialize_core_team`, `upgrade_epic_roster`.

## Corrections the architecture required

- **The closed Core Team seat DTO.** `CoreTeamPreviewRequest` and
  `CoreTeamApplyRequest` carried `Vec<RoleSelectionDto>`, which cannot express
  `presence` or `ad_hoc_allowed`. They now carry `CoreTeamSeatSelectionDto`, and
  the OpenAPI/registry artefacts were regenerated in the same change.

- **`EpicPresence` moved to `kontor-core::spec`.** The wire and the domain
  resolve one spelling rather than two that could drift.

- **Previews commit nothing.** Promotion and roster previews mint no id, store
  no draft and write no row; the apply recomputes the plan, compares the digest
  and freezes ids inside the transaction that records them. This replaces the
  pre-existing `OperationalWorkflow` behaviour, whose `preview_promotion` stored
  a draft and generated `MiniProjectId`/`TopologyNodeId`/`SeatBindingId` at
  preview time — a promotion that could not survive a restart between preview
  and apply.

- **Epics are addressed by `MiniProjectId`.** `epic_rosters` is keyed by the
  epic, matching the `/epics/{epic_id}/...` routes. The prior model keyed an
  epic's roster by the Quick session it came from, which left an epic created
  any other way unable to answer for its own seats.

- **Roster upgrade honours the named target.** The prior model ignored the
  caller's `target` and required exactly `current + 1`.

## Two places this build could not follow the architecture literally

### 1. The handoff is not a `HandoffCapsule`

The architecture says promotion builds one immutable `HandoffCapsule`. It cannot,
in this build, without fabricating provenance:

- `HandoffCapsule` requires `source_run_id: AgentRunId`, `context_pack_id`,
  `context_pack_hash` and a `WorkspaceRef`.
- `agent_runs.team_run_id` is `NOT NULL`, and the same architecture forbids a
  Quick session from creating a TeamRun ("A Quick session does not create a
  MiniProject, TeamRun or epic phase").
- The promotion request is bodyless, so the caller cannot supply them either.

Every route to a `HandoffCapsule` therefore ends in inventing an `AgentRunId`
that references no run. Instead, promotion delivers a typed handoff document
persisted on `quick_session_promotions`, bound to the exact frozen LSA
`SeatBindingId`, with its canonical hash. Every *behavioural* requirement the
architecture states about the handoff is kept: immutable, exact bytes and digest,
delivered to the exact frozen LSA seat, and promotion cannot report success
before delivery is recorded.

**Open for the architect:** whether to give Quick sessions a run identity (so a
real capsule becomes constructible), or to accept a promotion-specific handoff
document as the portable representation for session-to-seat transfer.

### 2. A base with no readback is not treated as a missing binding

The architecture says a missing or mismatched PSW binding is `placement_blocked`.
Mismatch is enforced in `promotable`, which runs at promotion preview and at the
first apply of a promotion. It does not run at ensure, because at ensure there is
no prior observation to disagree with — the session is being placed for the first
time, and what it observes is what gets recorded. Nor does it run on a *resumed*
promotion, which skips `promotable` entirely: the effects still outstanding at
that point sit under the ESW rather than the PSW, so the base is no longer the
thing being placed against. That is deliberate rather than an accident of control
flow, and it is stated here because the control flow alone does not say so.

Absence is not enforced anywhere, because a base is only read back once something
has been placed under it — so refusing on absence would refuse the first Quick
session in every realm, permanently.

What is refused is a base that does not *exist*: without an adopted base there is
nothing to place under, and the only way forward would be to invent a native
project, which is the fallback this path must never take. The observed native id
is recorded when placement happens, and a later disagreement refuses.

## Ordering of durable keys against their effects

Both multi-effect commands write the row that reconciles them before those
effects, and the review of `090b61f` found both getting it wrong. Recorded here
because the ordering is the whole mechanism and is not visible from the happy
path:

- `promotion:apply` writes the promotion row **and** the frozen roster in one
  transaction (`begin_promotion`), before the MiniProject, the nodes, the seats
  or the handoff. The resume path reads the roster before anything else, so a
  roster written later left any failure in between recorded as promoted and
  impossible to resume — permanently, since the row is keyed by its source and
  nothing deletes it.
- `quick-sessions:ensure` writes the `quick_sessions` row first, carrying the
  node and seat ids, then reconciles each of those by id. Written last, a
  failure in between left an orphaned node and an unattached seat binding that
  nothing could attribute — the artefact the OP-REQ-039 phantom was made of.

Both are covered by tests that construct the interrupted state rather than the
successful one, and both were confirmed to fail against the previous ordering.

## Proofs

In `crates/kontor-daemon/tests/loopback_api.rs`, driven through the public API:

- a roster is previewed, applied and read back; the preview publishes nothing;
- mandatory `LSA`/`TPM` are inserted, stay `required` and stay distinct seats;
  `SA` never satisfies `LSA`;
- stated presence and `ad_hoc_allowed` survive the round trip rather than being
  inferred;
- a raw role string, a caller-authored `standard_title` and an omitted presence
  are all refused by the closed DTO;
- an unconfigured project answers `not_found` for both `core-team` and
  `quick-roles` rather than an empty roster or an empty picker;
- quick roles are exactly the `ad_hoc_allowed` entries;
- a Quick session opens once and a lost acknowledgement returns the same session,
  node and seat;
- a quick-ineligible and an unknown role both refuse before any effect;
- a promotion creates one epic, one control plane and each required/default seat,
  leaves on-demand roles absent, and a repeated apply returns the same epic;
- a later project Core Team edit leaves a promoted epic's frozen roster
  unchanged, and only the explicit upgrade moves it;
- a promotion interrupted immediately after authorization resumes to completion
  against the epic it already froze, and seats its LSA;
- an ensure interrupted after its row was written completes the node and seat
  the row claims, rather than returning a session with no workspace;
- authorizing a promotion records the epic and its roster together, records
  neither when the roster cannot be written, and a second command for one
  session is a typed conflict rather than a backend error
  (`crates/kontor-store/tests/operational_promotion.rs`).

Mutation-checked: hard-coding `presence` to `Required` and answering with an
empty roster each killed two tests.

## Not in scope

Jira create/link and ASMA activation (OP-07), Advisor/Committee behaviour
(OP-05), Completion compilation (OP-06), final cross-feature assembly (OP-08).
`OperationalWorkflow` in `kontor-teams` retains its own unit tests; the daemon
composes the pure domain pieces and the store rather than that in-memory
aggregate, because production replay authority is the command receipt plus the
repository transaction.
