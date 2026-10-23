# Changelog

All notable changes to Kontor are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and releases use [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added durable desks. A Team Definition may declare an epic-independent desk,
  a project-level native project and its one workspace, recorded in an
  immutable schema-v131 `desks` table. `topology:ensure`, `:materialize` and
  `:drift` accept a desk scope, and node retire/archive routes and epic
  closeout never move a desk node. A desk holds three kinds of seat:
  - Declared durable seats (schema v132; `kontor_desk_seat_declare`,
    `kontor_desk_seats_list` and `kontor_desk_seats_reconcile`). Reconcile
    launches only a selected, daemon-owned seat that a census proved missing.
  - Scoped epic and ad-hoc Adam seats placed without a new ECP (schema v133).
    They are titled `LSA • <epic key> • <topic>` and `LSA • ADHOC • <topic>`.
    `kontor_desk_adam_ad_hoc_open` opens an ad-hoc seat;
    `kontor_desk_adam_handoff` fences the planning LSA and launches the
    coordinating TPM on another vendor.
  - One Adam tab and review room per pull request (schema v134), titled
    `TPM • <key> • <repo>#<n>` and `RSW • <key> • <repo>#<n>` through the new
    `PULL_REQUEST` naming token. Driven by `kontor_desk_pr_reviews_list`,
    `kontor_desk_pr_review_open` and `kontor_desk_pr_review_close`.

  Operational topology v6 and Team Definition v7 ship as additive successors,
  and the default declares no desk. Export generations 14–17 carry the new
  records (ASMA-8450, ASMA-8451, ASMA-8452, ASMA-8453).
- Added a Tokenomics view to the console, reachable from the connection
  screen, the navigation rail and `?view=tokenomics`. It loads local report
  data through the development server's `TOKENOMICS_DATA_ORIGIN` proxy and
  bundles no report data (ASMA-8353).
- Added the shared orchestration policy (ASMA-8278, ASMA-8279–ASMA-8284):
  - Versioned fleet policies and orchestration bundles with
    preview/publish/activate lifecycles (schemas v125–v126). They are served
    through the `kontor_fleet_policy_*` and `kontor_fleet_bundle_*` tools.
    Placement selection is the CLI-local `fleet-policy-resolve`, which has no
    `/v1` route.
  - The `planning_pair@1` protocol: two read-only advisory members and no
    Judge (schemas v127–v128; `kontor_planning_pair_*` tools).
  - Bounded attestation verification over immutable authority-key and
    prepared-token records (schemas v129–v130).

  The work adds the `kontor-fleet` and `kontor-fleet-activation` crates.
- Added experience memory and analogue recall (ASMA-8155, ASMA-8156,
  ASMA-8158, ASMA-8159):
  - Experiences are classified and proposed through `kontor_experience_classify`
    and `kontor_experience_propose`.
  - A recall is previewed, frozen and read back through
    `kontor_memory_recall_preview`, `_freeze` and `_get`. It holds at most 8
    experiences within 32768 bytes.
  - A rebuildable memory projection with receipts (schemas v123–v124) is served
    by `kontor_memory_projection_preview`, `_rebuild` and `_get`.
  - The optional Cognee adapter (`kontor-memory-cognee`, `memory-cognee.json`)
    is disabled by default. `enabled: true` is refused at startup with
    `cognee_unavailable`.
- Added the Core Team route succession ledger (schemas v120–v122, export
  generation 13). A route change claims its succession exclusively before it
  retires and launches. A succession lost in flight completes on replay
  instead of re-planning, and launch-intent installation and the seat
  observation are recorded separately, so a replay reconciles the effects
  still pending (ASMA-8187).
- Added the roster occupancy reads `kontor_epic_core_team_get` and
  `kontor_epic_core_team_seat_occupancies_get` (ASMA-8196).
- Added `kontor-daemon install-jira-credential --alias`, which installs a Jira
  credential into a stopped realm. It reads bounded stdin, writes a
  Realm/root-scoped Keychain address, and verifies compensation if the write
  fails (ASMA-8015).
- Added live `fleet.yml` model routing. Every delivery, Committee and Advisor
  placement reads the owner-only `<state_root>/fleet.yml` at placement time, so
  an edit changes the next placement without a rebuild or restart. An invalid
  edit keeps the last valid snapshot, and deleting the file restores template
  routing. Committee reviewers stay on different vendors. Routes the fleet
  lists are accepted as catalogued. See `config/examples/fleet.yml`
  (ASMA-8255).
- Added Admin recovery off a disabled provider. Seat succession may retire a
  seat that already ran when an explicit Admin `model_route` names a different
  provider. Seat fill accepts one catalogued Admin `model_route` for an owed
  slot whose frozen chain cannot be placed. Consultations may use the exact
  `cursor/gpt-5.6-sol/xhigh` route under an operator-accepted recovery profile.
- Added exact delivery observation (ASMA-8203):
  - `kontor_turn_observe` reads the exact current turn without settling it.
  - `kontor_session_message_proof_get` and `kontor_session_message_reconcile`
    run a resumable, read-only proof over canonical history for a message
    whose delivery is uncertain (schema v118). The proof never resends, and an
    incomplete scan is refused as `proof_scan_incomplete`.
- Added completion recovery surfaces (schemas v111–v114; ASMA-7869):
  - `kontor_artifact_record` registers verified Git artifacts against exact
    settled producer turns.
  - `kontor_open_question_record` and `kontor_open_questions_list` expose the
    OpenQuestion ledger.
  - A judge sees same-round peer findings only after every required reviewer
    has submitted.
  - An absent child container is recovered under its proved parent.
- Added epic root archive after a proven closeout. The final native project
  root is removed only when completion is Done, every child is archived, and
  no workspace or unarchived session remains. Native absence is verified after
  removal (ASMA-8204).
- Added supersession of a never-bound prepared Core Team launch intent through
  `kontor_core_team_launch_intent_supersede` (Admin; schema v109, export
  generation 11; ASMA-8187). Hosted TPM lineage recovery now runs through the
  supported Core Team route preview/apply (ASMA-8098).
- Added TeamRun slot repair:
  - `kontor_team_run_seat_fill` seats an admitted slot that is owed a handoff
    but has no AgentRun.
  - `kontor_team_run_admission_adopt` adopts a queued admission atomically
    through an immutable adoption ledger (schema v110, export generation 12;
    ASMA-8234, ASMA-8188).
- Added kickoff holds. `epics:apply` accepts an optional `initial_hold` that
  is persisted before the epic becomes governable (ASMA-8090). A hold may
  declare `lift_condition: kickoff_ready`, which lifts it once the epic and
  every task have confirmed Jira bindings and every task has a declared
  worktree; the default remains `manual` (schema v101; ASMA-8194).
- Added `kontor_workflow_phase_recover` (Admin), which catches a stalled
  workflow up to the phase its own recorded evidence proves (ASMA-8205,
  ASMA-8199).
- Added an operator-only task worktree-claim preview/apply repair. It binds an
  exact task revision and old claim to the sole project/Jira/module-derived ASMA
  catalog target and branch, records an immutable schema-v108 before/after
  receipt, and leaves the task aggregate, Jira binding, workflow, gates and
  dependency graph unchanged (ASMA-8120).
- Added v2 team-template revisions that declare the fleet autonomy floor per
  slot (ASMA-8192).
- Added operator-only turn-correlation challenge preview/apply
  (`kontor_turn_correlation_challenge_preview`/`_apply`, schema v99). A turn
  whose runtime message id was lost settles against a server-owned challenge
  instead (ASMA-8119).
- Added the `EPIC_JIRA_KEY`, `TASK_JIRA_KEY` and `SCOPE_JIRA_KEY` native-name
  tokens, so names render from confirmed Jira keys. Schema v96 records the
  consultation subject (ASMA-8117).
- Added Jira description projection (ASMA-8123):
  - Kontor reads issue bodies back and reports typed conflicts:
    `missing_external_body`, `placeholder_body_only`,
    `diverged_from_projection`, `human_authored_divergence` and
    `unreadable_external_body`.
  - Descriptions are written through `kontor_epic_description_preview`/`_apply`
    and `kontor_task_description_preview`/`_apply` (schema v94).
  - Replacing a human-authored body requires `replace_human_authored`, and a
    write whose confirmation was lost settles as
    `recovered_lost_confirmation`.
  - Links are written as ADF links, and blank lines are encoded the way Jira
    stores them.
- Added rejected gate-verdict recovery through `kontor_gate_rejection_recover`
  (Admin; schema v90). Present null bindings are refused, conflict detail
  never discloses another task, and a fenced workflow catches up before
  delivery waits (ASMA-8110).
- Added an identity-preserving preview/apply correction for malformed legacy
  Committee topics and their bound CSW titles
  (`kontor_committee_topic_correction_preview`/`_apply`), including
  consultations that have already settled (schema v92; ASMA-8114).
- Added the `.githooks/pre-push` publication branch check, enabled with
  `git config core.hooksPath .githooks` (ASMA-8101).
- Added the shared publication-branch grammar (`kontor_core::branch`) ported
  from the ASMA CLI — `<type>/<PROJECT>-<number>-<slug>` with stable refusal
  codes — and used it to derive a task's canonical `.worktrees/` placement from
  its confirmed Jira key when an epic apply declares none (ASMA-8101).
- Added publication attestation (ASMA-8101): `publication:preview` judges a
  branch, commit and pull-request title against the confirmed Kontor/Jira
  binding of the epic or task the branch key names; `publication:attest`
  records the accepted or refused decision idempotently (schema v89) and
  `publication/{attestation_id}` reads it back. Exposed as the
  `kontor_publication_preview`, `kontor_publication_attest` and
  `kontor_publication_get` tools and CLI commands. The ASMA CLI calls the
  attestation before every push and pull-request creation or reuse.
- Added the GitHub App publication gateway (`config/github-app.json`): a
  poller that judges every open pull request of the governed repositories and
  posts the `asma/publication-identity` check run, and `publication:merge`
  (`kontor_publication_merge`), a squash merge through the App identity bound to
  the judged head commit that refuses a moved head. Inert until the App is
  installed and the document exists.
- Added the `asma/publication-branch-title` pull-request check
  (`.github/workflows/publication-branch-title.yml`). It checks branch and
  title shape and that both carry the same key; it does not check the Jira
  binding. The checker is fetched from the immutable `github.workflow_sha`
  (ASMA-8114, ASMA-8101).
- Added immutable project-scoped epic backlog codes, deterministic
  collision-safe allocation, manual overrides and schema-v72 legacy evidence
  migration.
- Added the typed `ITEM_CODE` native-name token and Operational topology v4 for
  `ESW · KOP-8001`, `ECP · KOP-8001` and `TSW · KOP-7869` rendering from
  confirmed Jira identities.
- Added durable, exact-seat Committee permission inspection and response through
  the HTTP, CLI and MCP surfaces, including a schema-v75 effect ledger and the
  narrow leadership serve-profile capabilities needed by persistent LSA/TPM
  seats.

### Changed

- The publication check, pre-push hook and fast/heavy test lanes read the
  project key, base branch and check name from the selected project
  configuration (`KONTOR_CONFIG_DIR` / `KONTOR_CONFIG_FILE`). The pre-push hook
  falls back to `.github/kontor-project.yml` in a clean agent shell, and the
  workflow emits the configured check name through the Checks API.
  `KONTOR_ALLOW_AGENT_PUSH` replaces `ASMA_ALLOW_AGENT_PUSH`.
- CLI refusals now exit with their class instead of 1:
  - 3 (refused): `placement_blocked`, `ensure_mismatch` and
    `handoff_unsettled`.
  - 4 (conflict): `delivery_unconfirmed`.
  - 5 (unavailable): `proof_scan_incomplete` and `provider_usage_throttled`.
  - 6 (absent): `rename_pending`.

  (ASMA-8454)
- New admission now requires a current quota observation within the evidence
  window. An absent or stale observation is treated as unavailable rather than
  admissible, and the refusal says to probe the account (ASMA-8120).
- Raised the compiled capacity defaults: global 16→24, project 8→14, mission
  7→12, runtime 8→14, adaptive ceiling 7→12. Startup now loads and validates
  the stored capacity configuration, refuses to start when that configuration
  is invalid, and validates the seed only when no stored row exists. The
  capacity read reports `stored_ceilings` and `restart_required`
  (ASMA-8191, ASMA-8190, ASMA-8200).
- The model catalog recognizes more routes:
  - Claude Fable 5.1.
  - `deepseek/deepseek-flash` as the OpenCode default; `deepseek-v4-flash`
    remains admissible (ASMA-8154).
  - The approved fallback routes: Codex `gpt-5.6-luna`, Cursor `auto-smart`,
    OpenRouter `z-ai/glm-5.3-flash`, and
    `nvidia/nemotron-3-ultra-550b-a55b:free` at medium and high effort only
    (ASMA-8237).
  - Cursor `gpt-5.6-sol` at low through max.

  Recognition does not change headroom, eligibility or template authority.
- Raised the supported Paseo baseline from 0.3.1 to 0.8.0. Builds below the
  floor are observed but never driven, and the contract fixtures are
  re-baselined to 0.8.0.
- Refusals name what they refused:
  - Identifier refusals carry the rejecting type and the structural path.
  - Refusal diagnostics name the exact rule and subject (ASMA-8234).
  - An invalid `Idempotency-Key` header is refused as that header.

  The seat handoff, session and Committee permission routes accept the
  standard idempotency-key vocabulary instead of requiring a UUIDv7
  (ASMA-8190, ASMA-8191).
- Scheduler placement fails closed. The Jira bindings, the pinned Team
  Definition revision, the declared worktree and the exact ESW/ECP/TSW
  bindings are attested before any TeamRun, AgentRun, seat, launch intent or
  lease is committed (ASMA-8115).
- Every epic-apply response and replay reports the ECP native binding and the
  leadership-seat state, and names the most-blocking completion call
  (ASMA-8199).
- Confirmed Jira keys resolve through the immutable Jira issue id (schema
  v95). A same-issue key rename reconciles in place, and one issue can no
  longer be confirmed onto two subjects (ASMA-8116).
- Consultation callers now supply semantic topics only. Kontor rejects
  repeated Jira/item codes, container prefixes and configured separators,
  returns the complete Team Definition-rendered `container_name`, and enforces
  one durable semantic Advisor/Committee identity across different retry keys
  (schema v91). A topic carrying the epic's derived item code is refused
  whether or not the pinned template renders that code, and a concurrent
  duplicate is refused with a typed error naming the surviving run
  (ASMA-8114).
- Publication policy revision 3 required every branch and pull-request title
  to carry the same confirmed Jira key, with child work published on the
  child's branch. Revision 4 (under Fixed) is the current policy (ASMA-8114).
- Accepted the ASMA CLI's nested catalog-module worktree layout
  (`.worktrees/<jira-key>/<module>`) after exact slug, Git common-directory
  and actual branch readback. Epic apply accepts such a task worktree when its
  slug is a confirmed epic or task key, and a task's catalog checkout binds
  slug and branch to that task's key (ASMA-8114, ASMA-8116).
- Epic apply refuses a managed `.worktrees/` path whose branch is outside the
  publication grammar or bound to a foreign key; the Paseo adapter refuses to
  mint any branch that is not canonical and bound to the epic or task key;
  consultation checkouts are named `chore/<EPIC-KEY>-consultation-<node id>`
  (existing `consultation-<node id>` checkouts are kept); an imported epic with
  no execution scope now runs under its confirmed Jira epic key (ASMA-8101).
- Extended epic preview/apply/readback and the generated OpenAPI/TypeScript
  contracts with the Kontor-owned epic backlog namespace while preserving full
  Jira issue keys as the binding authority.
- Advanced the immutable Operational topology opt-in to revision 4; revision 1
  remains unchanged.
- Daemon black-box tests now replay the 96 schema generations once per test
  process and clone the migrated database into each world, instead of
  replaying them for every world: `loopback_api` finishes in about 40s instead
  of about 17 minutes, with the same pass/fail set. The Realm identity is
  shared by the clones because the schema refuses to re-identify a copied row,
  so the tests that compare identities start from `World::open_created_realm()`
  and pay the real migration (ASMA-7869).
- Added contributor test lanes. `scripts/test-lanes.py` selects the fast,
  heavy or full lane from `scripts/test-timings.json`; exit code 3 means the
  heavy lane is required. The `verify-tree` lock check proves `--locked`
  satisfiability instead of regenerating the lock. The store suite clones a
  migrated template, so the `kontor-store` crate runs in about 12 minutes
  instead of about 44.
- The dev profile builds with `debug = "line-tables-only"`.

### Fixed

- Fixed personal Recently viewed for sessions opened through Kontor on the
  selected host and this device: most recent first; A → B → A gives A, B.
  Prompts and worker activity do not count as visits, and legacy saved history
  is preserved without invented opening times. Source:
  [3b16f79e](https://github.com/Carasent-ASMA/asma-rs-kontor/commit/3b16f79ec2cf1f81cd01e5fef5c63e87fc1ff2e3).
  As of 2026-10-23, installed/readback **PASS**, recorded in
  `2026-10-23-personal-mru-install-3b16f79e-r1/RECEIPT.json`
  (SHA256 `656b5e840cea40f8ef5b0f8a3d9f9373100a6c9a42f7a6f89fb1276328ff4830`).
  Physical acceptance remains pending. Cumulative prototype and TEST delivery
  are separate and unscheduled (ASMA-8541).

- Desk seat MCP configuration is published, journaled, into the non-Git desk
  directory. A fresh desk reservation commits atomically with its two
  topology nodes (ASMA-8454).
- Legacy consultation archival reads omit a name they cannot prove instead of
  failing with 409 (ASMA-8113).
- Recovered interrupted bound-seat installation and attachment, and preserved
  the ancestry of an abandoned replacement bridge (ASMA-8190).
- Realm export preserves command intents (ASMA-8121).
- Publication attestations refuse update and delete (schema v119; ASMA-8102).
- Seats that ran a live turn on Paseo 0.9 are readable again. Projected
  timeline pages are expanded into native sequences with content-free
  `kontor_elided` stand-ins, so messaging, observation, settlement and recovery
  no longer fail with `timeline_refetch_required`. Already-canonical 0.8 pages
  are unchanged.
- A done task's settled-turn artifact claims count as ticket evidence.
- Hosted leadership sessions are created with their scoped MCP configuration
  and the six leadership tools. Existing sessions need an explicit recovery
  (ASMA-8203).
- Unblocked epic closeout recovery (ASMA-8188):
  - Reviewers read only the Committee's exact subject evidence through
    `kontor_committee_artifact_get`.
  - Message issuance boundaries (schema v117) require continuous history
    coverage before acknowledgement.
  - Oversized native history pages shrink instead of reporting the runtime
    unreachable.
  - The immutable hosted role persona is retained separately from native
    readback (schema v116).
  - Census, workspace-kind and successor-reservation repairs.
- Drained native migrations safely (ASMA-8120):
  - Historical local command effects are certified atomically (schema v115).
  - Native lifecycle changes queue fairly behind in-flight operations.
  - Delivery uncertainty is restored from durable issuance.
  - Failed idle seats can retire.
- Current memory reads select the approved current revision before decoding,
  instead of hydrating every historical revision under the store lock
  (ASMA-8101).
- Persistent LSA sessions receive their configured role persona at seat
  creation (ASMA-8196).
- Repaired Core Team container recovery and canonical turn evidence
  (ASMA-8101):
  - Recovery keeps the latest inspected container proof and rechecks
    placement before retirement.
  - Taskless consultation worktrees are valid.
  - Canonical history frames up to 8 MiB are accepted instead of silently
    dropped.
- Task contexts resolve again once approved memory outgrows the canonical
  ceiling: memory is narrowed to the longest deterministic prefix that fits,
  and every omission is reported. Bound container preparation re-proves
  placement by exact native id instead of trusting cached state (ASMA-8234).
- An inspected launch placement rehydrates after restart, so an admitted run
  launches without a duplicate workspace or admission (ASMA-8120).
- Leadership seat autonomy resolves through the same floor as delivery seats
  and is frozen per occupancy generation. A hosted launch intent is recorded
  before its native effect (schemas v102–v103; ASMA-8193).
- A delivered message reads as delivered. Settlement proves that the message
  and its response are one turn, and timeline epochs survive restart (schemas
  v100, v104–v105). Unproven delivery is refused as `delivery_unconfirmed`
  (503) (ASMA-8203).
- Confirmed Jira identities are recovered, newest bindings first (ASMA-8049).
- Publication policy revision 4 closes two fail-open holes found by ASMA-8102
  verification. A publication naming a repository outside its binding is
  refused with `repository_mismatch`, and the authorized set is now derived
  from the durable Kontor ProjectId the forge mapping governs plus the bound
  task's module: the project's root repository always, that task's module
  repository when it owns one, and nothing else. It is never derived from a
  root path, the request, or the GitHub App's optional configuration, so every
  other project authorizes nothing and an unknown module widens nothing. A
  pull request with a null title also skipped title validation entirely and was
  accepted, so it could publish under no confirmed key; a title is now required
  whenever `pull_request` is present, while a push carrying no pull request
  still needs none (ASMA-8102).
- Recovered admitted runs whose runtime attachment never confirmed. They are
  reported as `runtime_attachment_unconfirmed` and replayed under their
  original identities, and the resident retry rotates fairly through the
  admission scan (ASMA-8202).
- A rejection fence resolves its role through the pinned Team Definition
  (ASMA-8189).
- Completion adopts the current TPM after the pinned TPM seat was replaced.
  Legacy native ticket closures are certified through receipt-backed closure
  certificates with typed provenance (schemas v97–v98; ASMA-7869).
- Corrected noncanonical legacy epic codes atomically. A partially seated
  TeamRun resumes only by adopting its exact downstream native, and an
  ambiguous census is refused (ASMA-7675).
- Repaired the consultation lifecycle (schema v93; ASMA-8111):
  - Reviews complete without a plan-approval stop. Claude members run in
    default mode behind a fail-closed tool guard, and Codex members run in a
    read-only sandbox.
  - Released sessions are recorded in a ledger.
  - Interrupted launches and recoveries resume.
  - The Kontor MCP server reaches consultation seats over stdio, with
    `KONTOR_AUTH` forwarded by name.
  - Committee completion selects an exact result through a typed `verdict`.
- Task scopes may carry only a Jira key. Task progress is derived from
  attached seats after a partial admission (ASMA-8116).
- An Admin may replace a seat that owes exactly one undelivered targetless
  handoff (ASMA-8112).
- Canonicalized every newly created Paseo epic project before binding it:
  native-root creation now requires project-rename support, verifies the
  registered directory by exact project ID, applies the Team Definition's ESW
  title, and refuses the binding when the unchanged ID, root and exact title do
  not read back (ASMA-8102).
- Each runtime message proof and each terminal response position settles at
  most one turn, so a concurrent caller with a new key gets a typed conflict
  and produces no handoff. A rejected gate routes the workflow atomically to
  its pinned `rejection_target` (ASMA-8100).
- Repaired seat recovery (ASMA-8099):
  - An Admin can reroute a delivery seat that was never bound.
  - Gate evaluator roles map to their frozen slots.
  - The newest recovered successor replays.
  - Turn proof starts at the claimed message window.
  - Jira reconciliation resumes after a conflict is resolved.
- Completed the configuration-driven naming migration (ASMA-8090):
  - Ordinary gate verdicts come from the live evaluator seat and its pinned
    account.
  - Archived seats and terminal legacy teams count as history, not as
    migration blockers.
  - A legacy ESW root reads back.
  - An exact archived Advisor retires and correlates with its predecessor
    workspace.
- Enforced the Paseo compatibility contracts (ASMA-8098):
  - Session permissions constrain composed capabilities and bound operations,
    including the MCP fallback.
  - Exact-agent timeline replacement survives event-queue overflow through a
    cursor-free canonical refetch.
  - Gate evaluations commit atomically with their workflow results, so an
    exact retry keeps the original verdict.
  - Jira materialization reports identity and creation-metadata mismatches
    as such, not as status conflicts.
- Limited Team Definition migration previews and exact persistence censuses to
  active topology nodes and seats, leaving retired or archived native history
  untouched instead of requiring an unavailable historical workspace retitle.
- Fenced seat release and every topology lifecycle transition atomically with
  an in-flight Team Definition migration, and limited TeamRun slot preflight
  and parent resolution to exact active topology so historical predecessors
  cannot block or race a live migration.
- Made Jira materialization retries safely adopt an exact existing task binding
  after a lost connector response without creating a duplicate link, and
  prevented already-confirmed materialization items from being rebound.
- Kept pre-schema-v72 epics readable and operable while they await an explicit
  backlog-code assignment; topology v4 still refuses to materialize without it.
- Required standalone native-container retitles to preserve the exact native
  identity and read back the requested title before recording a success receipt.
- Recover failed Jira create materialization in place against the original
  schema-v74 batch and immutable marker ledger. Recovery requires an exact Jira
  project/type/parent/summary/description/marker match, maps by ordinal instead
  of response position, and cannot rebind an already-confirmed epic.
- Compose seat MCP configuration atomically even when an ECP is not a Git
  checkout, leaving no partial `.mcp.json` or harness directory on preflight
  refusal.

## [0.2.1] - 2026-08-30

### Fixed

- Recovered stale or missing hosted Core Team natives on their governed model
  route while preserving the stable logical seat and fenced native history.
- Extended durable global Committee rounds through the scheduler's full
  positive round domain so a needs-human recovery can enter round three.
- Persisted exact-occupancy Completion-wake delivery, canonical timeline
  acknowledgement, restart reconciliation, and newest-projection handover to
  a replacement TPM without duplicate native effects.
- Refreshed the console's development-only OpenAPI dependency chain to consume
  the patched `js-yaml` release.

## [0.2.0] - 2026-08-26

### Added

- Durable, local-first orchestration state for projects, tasks, teams, seats,
  gates, evidence, approved memory, runtime bindings and external projections.
- Deterministic admission, guardrails, completion profiles, independent review,
  bounded remediation and receipt-backed recovery.
- A production Paseo runtime adapter, native Jira and provider-quota connectors,
  a generated CLI/MCP capability surface, and responsive operator console.
- Governing documentation for Kontor's two product values: Autonomy and Delivery
  Quality.

### Changed

- Synchronized architecture, configuration, recovery and quota-routing
  documentation with schema v66 and the current 146-tool registry.
- Clarified that admission is default-allow and authorization only narrows work.
- Distinguished shipped account-before-rung resolution from the unbuilt
  launch-time `Wait` / `NeedsHuman` actuation and mid-run succession paths.
- Marked calendar exposure, post-delivery profile packs, automatic stale-evidence
  rejection and the supervision engine as unfinished capabilities.

[Unreleased]: https://github.com/Carasent-ASMA/asma-rs-kontor/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/Carasent-ASMA/asma-rs-kontor/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/Carasent-ASMA/asma-rs-kontor/releases/tag/v0.2.0
