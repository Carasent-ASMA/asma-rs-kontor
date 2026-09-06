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
