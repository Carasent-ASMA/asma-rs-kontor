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

## OQ-002 — attribution of the `schema_v1` concurrent-open failure — open

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
- **Required to settle:** run the full `schema_v1` target on the integrated
  tree with the migration reverted, and compare against the same target with it
  applied. This must be settled before final gates; the assumption is not
  evidence.

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
