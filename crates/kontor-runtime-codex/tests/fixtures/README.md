# Codex adapter fixtures

`account-a/` and `account-b/` are two **approved `CODEX_HOME` directories**, as an
operator would lay them out.

Each holds:

- `kontor-profile.json` — the non-secret identity marker the adapter reads. It
  carries a schema version, the account-profile id and the expected non-secret
  provider identity, and nothing else: `CodexHomeMarker` denies unknown fields,
  so a marker that grew a credential field would fail to parse rather than be
  held in memory.
- `auth.json` — a stand-in for the credential file Codex reads and **Kontor never
  does**. Its `value` is a canary: the suite plants it in the fixture transport's
  watch list, so any dispatch that carried it — in argv, in the working
  directory, or in an environment value — is reported as a leak. It must never
  appear, because nothing in this adapter opens this file.

`worktree/` is the verified task worktree `prepare_workspace` binds. It exists on
disk because that verification is a real `is_dir` check rather than a claim.
