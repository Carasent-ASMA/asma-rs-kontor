# ASMA-8234 slot-2 integration acceptance refresh

> **Date:** 2026-10-03 11:27 CEST
> **Status:** 🟢 Approved — acceptance extended
> **Author:** Codex AUD seat
> **Category:** report
> **Scope:** Narrow integration-delta review of merge `48d27e47059a2b283aa7f7f785e0e18d9a472387`
> **Summary:** Confirms that the additive slot-2 join preserves the accepted refusal-diagnostics change and the merged ASMA-8196 default content, with a conflict-free exact merge tree and green focused checks.

---

## When to Load

**Load this document when:**

- deciding whether ASMA-8234 acceptance extends to slot-2 integration candidate `48d27e47`;
- checking how the accepted loopback assertion coexists with the shared file's default-side additions;
- preserving the original ASMA-8234 findings and pins through integration.

**Do NOT load for:** push, PR, merge-to-default, deployment, Jira, admission, permission, or live-runtime claims.

---

## Verdict

`accept`

The existing ASMA-8234 acceptance extends to slot-2 integration candidate `48d27e47059a2b283aa7f7f785e0e18d9a472387`. There are no new findings.

## Exact pins preserved

| Item | Exact value |
| --- | --- |
| Original base | `f95e206563bca88b6871f48528623441e5e1a231` |
| Accepted source | `6d9f216fc58dd1602f3044f8758c9dac4c907ba8` |
| Historical source | `fdb6967d378cb6bb8475853973b3446b2c23dbf4` |
| Historical/current stable patch-id | `52a839339d23d400e8759021d56d8aa03436a927` |
| Verification evidence | `ded67076996f83c4f34632e9f69512a9e5a65b52` |
| Acceptance evidence | `06dc77903a9f9b905a90702a5ad404babfd736b0` |
| Merged default | `d5a4cdff630c4aaa8e20a4b34bde4d9e44e5893e` |
| Integration candidate | `48d27e47059a2b283aa7f7f785e0e18d9a472387` |
| Integration tree | `b50383e3b68d375e77580615e1d9511a9f6e77bc` |

The merge has the exact declared parents: lane `06dc7790` and default `d5a4cdff`. Re-running `git merge-tree --write-tree 06dc7790 d5a4cdff` exits 0 and produces `b50383e3b68d375e77580615e1d9511a9f6e77bc`, exactly the recorded merge tree. No merge-only resolution exists.

The chain `6d9f216f` → `ded67076` → `06dc7790` → `48d27e47` remains reachable; every adjacent `git merge-base --is-ancestor` check returns 0. Default `d5a4cdff` is also an ancestor of the integration candidate. No rebase or history rewrite occurred.

## Lane isolation and shared-file coexistence

The lane contribution isolated as `d5a4cdff..48d27e47` is exactly the accepted change:

| Path | Isolated delta | Stable patch-id at accepted source and merge |
| --- | --- | --- |
| `crates/kontor-api/src/error.rs` | 62 insertions, 15 deletions | `a3fe1908bc7530d7b6cdd4f3f8d014be46328154` |
| `crates/kontor-daemon/tests/loopback_api.rs` | one hunk; 8 insertions, 6 deletions | `afa91f2be1ada32931a8bb2ddc9c75e2ed0328d3` |

After removing only diff metadata and hunk-position headers, the accepted and integrated change payloads have identical SHA-256 values: `b95cf97cea297892dd92ac514816d646272f6a68d154f83e40c5b704755433bb` for `error.rs` and `939b0fd06d8ac25b49af75f635d39998e9c9616f81fd044d47420894592422cf` for `loopback_api.rs`. This checks the added and removed bytes independently of their shifted line numbers.

`error.rs` is blob `678ececedda4c9c9554728e2a3a8014ce241c6fc` at both accepted source `6d9f216f` and integration head `48d27e47`.

The single loopback hunk is byte-for-byte the accepted tightening of `a_workspace_refusal_is_reported_as_a_placement_fact`: it retains HTTP/code behavior and replaces the loose substring check with exact rule equality. The pinned string `the requested root is not the canonical task worktree of this plane` occurs exactly once in the final shared test file.

Default-side ASMA-8196 additions also survive intact. The loopback default-addition patch in `f95e2065..d5a4cdff` and the patch carried into `06dc7790..48d27e47` have the same stable patch-id, `802e5b9ef5cb3a5b306280e42ac8546185b762f5`, and the same 447/0 numstat. The relevant blobs demonstrate composition rather than overwrite:

- default loopback: `772fab8ac8df5dfe4e57b613b94eaba5b984578c`;
- lane loopback: `bb8e089743908ed53c89009e0ad0a84fdf8f89c9`;
- integrated loopback: `d728abca71692572302a270c87c4a2dba188d905`.

`Cargo.lock` is blob `6642ded3accde407157fd50dbcedd50e2b9be327` at both default and integration head and records `yoke-derive 0.8.4`.

## Exact checks

| Command | Result |
| --- | --- |
| `cargo test -p kontor-api --lib error` | PASS — 15 passed, 0 failed, 14 filtered; log SHA-256 `fcaf12ed656b9bb0a15c54a8c668253ee861636cb26f2a097022dbbe819401fa` |
| `cargo test -p kontor-daemon --test loopback_api a_workspace_refusal` | PASS — 1 passed, 0 failed, 484 filtered; log SHA-256 `dd3ac8420ceb2f1eb982939ac73f696d84c38518b2646673daf99877464013a7` |
| `cargo fmt --all -- --check` | PASS, exit 0, no output; log SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

## Preserved findings and boundaries

The earlier failed finding named **`redundant adoption branch`** remains failed under its original scope and verdict; this integration review neither repairs nor reopens it. The historical absence of verification/audit acceptance for the refusal-diagnostics residual remains a historical fact, while `ded67076` and `06dc7790` remain the exact later current-candidate evidence. All original verdicts and pins stay attached to their original scopes.

This review changed no source or existing evidence file. It performed no push, PR, merge, Jira action, deployment, admission, permission, or external gate action. The pre-existing adapter entries remain untouched and excluded from this evidence checkpoint.
