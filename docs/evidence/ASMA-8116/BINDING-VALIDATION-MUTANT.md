# ASMA-8116 binding-validation mutant

- **Mutation:** disabled the shared `cross_subject` guard in
  `SqliteStore::confirm_jira_materialization_item` while leaving all other code
  unchanged.
- **Command:** `cargo test -p kontor-store --test jira_materialization
  activation_requires_every_confirmed_binding_and_survives_readback -- --exact`
- **Observed kill:** the test failed at the assertion requiring a typed conflict
  when an epic attempted to confirm the Jira key already confirmed for a task.
- **Restoration:** restored the guard; the same focused test passes.
