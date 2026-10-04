# ASMA-8202 deployment receipt

Date: 2026-09-17

## Candidate and qualification

The feature work was rebased by fast-forwarding its original `f78d041` base to
current `origin/master` `1e3f35af3de1086ddc5dead52ab94b1b28950d42` before
the final qualification and release build. The production/test diff is fixed by
SHA-256 `baf24eb35b4a3d10f7f7275404be2517e208f78fd9b65ddfbaf4de206147431e`.

Passed after that update:

- `cargo test -p kontor-store --test scheduler_admission`: 32 passed.
- `cargo test -p kontor-daemon --test loopback_api a_partially_seated_candidate_claims_progress_and_an_unattached_one_does_not -- --exact`: passed.
- `cargo clippy -p kontor-store -p kontor-daemon --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.
- Both selector mutations recorded in [MUTATION.md](MUTATION.md) were killed.

The whole-tree `cargo fmt --all -- --check` is not green on current master: it
reports pre-existing formatting drift in `crates/kontor-api/src/applications.rs`
and unrelated regions of `crates/kontor-daemon/tests/loopback_api.rs`. The
ASMA-8202 files/regions were formatted before the master update; the deployment
did not rewrite those unrelated master lines.

## Installed artifacts

| Artifact | Built and installed SHA-256 |
| --- | --- |
| `kontor` | `9bcbbb6815ff2a4c5798bd506f14c10fdb9602cc8cb8300af8f5416071060afe` |
| `kontor-daemon` | `3aa693116a78c87cd28af4c34fe35fc1da36e5a693476177422b801392133bc7` |
| `kontor-mcp` | `ec6073958067dd70c540e3c681d58e11612f13d20be109ad9b1e15b4c0c649bd` |

LaunchAgent `com.asma.kontor.daemon` restarted the same state root and realm:

- PID `1330` → `2989`.
- Realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065` preserved.
- Database schema remained 98; `PRAGMA integrity_check` returned `ok` and
  `PRAGMA foreign_key_check` returned zero rows.

The verified snapshot, old binaries, installed hashes, configurations and
machine-readable receipt are retained at:

`/Users/igor/.local/state/kontor/asma/deploy-backups/20260917T040259Z-asma-8202-1e3f35a/`

## Live recovery proof

Before restart, the only row matching the new recovery predicate was this
task's admitted root:

- Task `01a0ac9d-a966-7f03-9ead-2bd049094c7f`: `ready@1`.
- TeamRun `01a0ad5a-7497-7e43-8c79-3a009be5238e`: queued.
- AgentRun `01a0ad5a-7498-7170-9d63-e010e1ff2aad`: queued,
  `run_requested`, observed `unknown`, no runtime binding.

The resident reconciler attached it immediately after startup reconciliation.
Independent API readback at cursor 3631 reported the task `in_progress@2`; the
same TeamRun is `running`, and the same AgentRun is attached and confirmed with
native id `572d39a4-3a4b-46e4-8ec0-f0343be41f39`.

Identity comparisons in the deployment gate proved:

- exactly one TeamRun still belongs to the task;
- the existing four logical seat ids and role slots are byte-for-byte unchanged;
- project `01a0064a-e056-7603-9968-ef64fdaacb75` and TSW node
  `01a0ad5a-75d3-7fd0-a133-375833efe26d` are unchanged;
- native workspace `wks_479843d94033d377` and its canonical worktree path are
  unchanged;
- the unconfirmed selector no longer matches the attached root.

The first promotion attempt also recovered the same native id, but its gate read
the task before the recovery method completed its final task-state transition.
That deliberately strict assertion triggered automatic binary/database rollback.
Readback then proved the original queued/unbound state and old binary hashes were
restored. The gate was corrected to wait for both attachment and `in_progress`;
the successful receipt above is the second attempt. The failed attempt is
retained separately at
`/Users/igor/.local/state/kontor/asma/deploy-backups/20260917T040109Z-asma-8202-1e3f35a/`.
