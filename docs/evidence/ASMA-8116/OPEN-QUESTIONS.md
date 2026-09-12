# ASMA-8116 open questions

## OQ-001 — immutable Jira issue identity for key-change reconciliation — resolved

- **Attaches to:** ASMA-8116 / TASK-001 and the confirmed binding records
  `jira_epic_bindings` and `jira_task_binding_confirmations`.
- **Former ambiguity:** the connector readback currently retains the Jira key
  and a content hash, while the hash itself includes the key. Jira's immutable
  issue ID is not retained. The stored evidence therefore cannot distinguish a
  key change on the same Jira issue from an attempted bind to a different
  issue.
- **Options observed:** extend connector readback and both binding ledgers with
  the immutable Jira issue ID; assign that contract to the later Jira
  reconciliation task; or infer sameness from mutable content.
- **Resolution:** the admitted ASMA-8116 contract adopts the first option. The
  Jira REST top-level `id` must be retained in connector readback and in both
  confirmation ledgers. A confirmed key may change only when fresh readback
  proves the same immutable issue ID. A different issue ID is a typed
  anti-rebind refusal. A migrated record without an immutable ID cannot
  authorize a key change; migration must not synthesize one, and the record
  remains fail-closed until supported Jira readback establishes the ID.
- **Rejected reductions:** do not defer immutable identity to a later task, do
  not infer identity from mutable content, and do not preserve the provisional
  blanket refusal of every confirmed key change.

## OQ-002 — attribution of the `schema_v1` concurrent-open failure — open (characterized, not settled)

- **Attaches to:** ASMA-8116 / the v94 (to become v95) immutable Jira
  issue-identity migration and
  `crates/kontor-store/tests/schema_v1.rs::a_concurrent_first_open_initializes_exactly_one_realm`.
- **Ambiguity:** after integrating `origin/master` and adding the immutable
  issue-identity migration, that test fails reproducibly with SQLite
  `DatabaseBusy` when the full `schema_v1` target runs (56 passed, 1 failed, on
  two consecutive runs). Run in isolation the same test passes in ~1.48s
  against a 30s busy timeout — a ~20x margin — which points at contention while
  57 file-backed tests run in parallel rather than at migration cost. The
  baseline run that would separate the two causes was started and interrupted,
  so the attribution is **not** evidenced.
- **Options observed:** (a) pre-existing contention exposed by the integrated
  tree, including ASMA-8111's v93 additions, and unrelated to this migration;
  (b) a genuine regression in which the added migration pushes concurrent first
  open past the busy timeout; (c) an environment-specific effect of this
  machine's load during the runs.
- **Assumption being carried:** (a)/(c) — proceeding with implementation on the
  basis that this is load contention, not a logic regression. The added
  migration performs two `ALTER TABLE ADD COLUMN`, two unique-index creations
  and two trigger creations, none of which plausibly consumes the ~28.5s of
  additional budget the failure would require.
- **Run ledger (2026-09-12, integrated onto `origin/master` e40b5f4, migration
  renumbered to 0095):**
  | Arm | Result |
  | --- | --- |
  | with `0095` (pre-integration tree) | FAILED, twice |
  | with `0095` (integrated tree, run 1) | FAILED |
  | without `0095` (integrated tree) | ok, 57/57 |
  | with `0095` (integrated tree, run 2) | ok, 57/57 |
- **What this shows:** the failure is **intermittent**, not deterministic. The
  paired comparison that was supposed to settle attribution produced a failure
  and then a pass on the *same* arm, so it separates nothing. An earlier reading
  of the first paired run as proof that the migration causes the failure was
  withdrawn on the next run.
- **Still unsettled.** The honest position is that
  `a_concurrent_first_open_initializes_exactly_one_realm` is load-sensitive under
  57 parallel file-backed tests, and that this migration *may* raise the failure
  probability marginally by lengthening first open — a ~1.5s migration against a
  30s busy timeout, which only matters if heavy parallel load compresses that
  margin. Neither claim is evidenced by single runs.
- **Required to settle:** repeated runs of the full `schema_v1` target on both
  arms — on the order of ten each, executed without other cargo work competing
  for the machine — and a comparison of failure *rates* rather than outcomes.
  Single-run evidence must not be used to either blame or clear this migration.

## OQ-003 — task-side key change collides with the v81 canonical-link immutability invariant — resolved

- **Attaches to:** ASMA-8116 / `HIGH-SCOPE-RECORD.md` clause 3 and remaining
  handoff item 3, and the v81 ledger
  `crates/kontor-store/migrations/0081_canonical_jira_task_link_ledger.sql`.
- **Ambiguity:** clause 3 requires transactional same-issue key reconciliation
  that preserves the Kontor subject UUID, and item 5 requires it to cover
  **both** the epic and task ledgers. The epic ledger permits this: an
  `UPDATE` of `jira_epic_bindings.external_issue_key` is unconstrained apart
  from its uniqueness. The task ledger does not. ASMA-8081 installed
  `canonical_jira_task_links_immutable`, a `BEFORE UPDATE` trigger with no
  column list and no `WHEN` clause, so it aborts *every* update to that table;
  `canonical_jira_task_links_permanent` likewise aborts every delete, and
  `PRIMARY KEY (project_id, task_id)` forbids a second row for the same task.
  A task's canonical Jira key therefore cannot change by any route without
  altering an invariant another ticket introduced deliberately.
- **Options observed:**
  1. **Narrow the v81 trigger** in this ticket's migration so it admits only a
     change of `external_issue_key`, and only on a row whose confirmation
     carries the same immutable issue id, while keeping `project_id`,
     `task_id` and `link_id` aborting as today. This reads the v81 invariant as
     "a canonical link's *subject identity* is immutable", which was previously
     indistinguishable from "its key is immutable" because before this ticket
     the key *was* the identity.
  2. **Leave v81 absolute** and treat `canonical_jira_task_links.external_issue_key`
     as historical, sourcing the current key from `jira_links` instead. This
     avoids touching another ticket's invariant but splits the authority for
     "the task's current Jira key" across two tables and contradicts the
     resolver, which reads the canonical ledger.
  3. **Implement key change for epics only** and defer the task ledger. This
     keeps v81 untouched but does not satisfy clause 3 or item 5 as admitted.
- **Resolution:** option 1. Narrowing the v81 trigger is within ASMA-8116's
  authorized scope and must not be reduced to epics only, nor satisfied by
  sourcing the current key from an unconfirmed `jira_links` row. A task key
  update is permitted only inside the same transaction, after fresh connector
  readback proves the stored immutable Jira issue ID is identical. The task
  UUID, link identity, confirmation evidence and revision are preserved.
  Deletes stay blocked, and every update that changes subject, project, link
  identity or the immutable issue ID stays blocked.
- **Required coverage:** positive same-issue rename, different-ID anti-rebind,
  unauthorized direct SQL update, replay, race, and restart/backup evidence.

## OQ-004 — placement requires a legacy code the Jira-key policy forbids minting — OPEN (operational gap)

- **Attaches to:** ASMA-8116 / `HIGH-SCOPE-RECORD.md` clause 9,
  `Services::item_code_for_subject` in `crates/kontor-daemon/src/applications.rs`,
  and the `NativeNameToken` contract in `crates/kontor-core/src/naming.rs`.
- **Symptom:** with clause 9 in force (apply allocates no backlog code when none
  is declared), an epic has no active code and the MCP bootstrap journey fails:
  `placement_blocked: the epic has no active immutable backlog code`
  (`an_empty_realm_is_bootstrapped_through_mcp_tools_alone`).

### Two approaches attempted and both withdrawn

1. **Reinstate allocation** in `apply_epic` so every epic keeps an active code.
   *Withdrawn:* violates the fence that new-policy Jira-key work has no
   namespace allocation requirement and that old short codes are read-only
   historical mappings.
2. **`JiraItemCode::from_confirmed_key`** — project a subject with no legacy
   code using its confirmed Jira key, carried verbatim through the existing
   `item_code` field. *Withdrawn:* assigning a Jira key into an old-code field
   changes `ITEM_CODE`/`JiraItemCode` semantics, which is explicitly forbidden.
   `JiraItemCode::derive` behaviour and hashes must stay exactly as they are.

Both were reverted. `crates/kontor-core/src/backlog_identity.rs` and its tests
are byte-identical to `origin/master` again, and `item_code_for_subject` is
restored verbatim.

### The approved direction, and why it is blocked here

The confirmed Jira key must become **its own typed public identity**, with
`task_short_code` / `item_code` / `backlog_code` required only by *legacy*
consumers, and new-key-only templates consuming `EPIC_JIRA_KEY`,
`TASK_JIRA_KEY` and `SCOPE_JIRA_KEY` directly from confirmed bindings.

The resolver output already exists: `ConfirmedJiraBinding` (subject, key,
readback hash, confirmation time, revision) is the typed identity, and the
binder call sites are *already* lazy — every one is guarded by
`template_uses_token(..)` / `template_uses_item_code(..)`, so nothing is
eagerly demanded by code. The bootstrap fails because the **seeded templates
themselves** name item-code tokens, and the Jira-key tokens they would need do
not exist.

Closing that requires three decisions this seat does not hold:

1. **Extending a published closed contract.** `NativeNameToken` is a
   `closed_enum!` whose spellings are serialized into stored Team Definition
   specs, and `spec.rs` validates templates against an explicit allow-list whose
   refusal text enumerates the permitted tokens. Adding three tokens changes
   that published contract and the stored specs validated against it.
2. **Which seeded templates become new-key-only.** The defaults are seeded in
   `crates/kontor-store/src/migrations.rs` (`token_template([...])`), so changing
   them is a migration — on a branch whose migration number is already contended
   (this work sits at `0095`, master having taken `0094`).
3. **What counts as a "legacy consumer"** at each call site, which decides where
   an item code stays required and where it becomes optional.

- **Not invented, not relaxed:** no mapping was fabricated, no short code minted,
  no numeric reverse inference added, and no policy loosened. The branch is left
  with the bootstrap failing rather than with an unauthorized reconciliation.
- **Required to settle:** the approved plan's specification for the three new
  tokens (names, allow-list change, migration for seeded templates) and the
  legacy-consumer boundary.
