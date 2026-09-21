# ASMA-8102 high-verification report: publication identity corrections

Date: 2026-09-21

Artifact: `high-verification-report`

Phase: `high-verification`

Task: `ASMA-8102` / `01a0721b-ea31-7482-952f-f6f004019537`

Epic: `ASMA-8101` / `01a0721b-ea30-7fe3-88a5-4d33ca613414`

TeamRun: `01a075b2-ae1a-7c52-ba3a-b7a372069ea0`

Verifier AgentRun: `01a0c072-9408-7d32-a6f5-cdce6c764617`

Verifier native: `bb890453-0975-4086-8a4e-41c369050e68`

## Verdict

**PASS.** No blocking finding remains in the PUB-01 scope accepted by the
`high-scope-record`. The exact failures that caused gate sequence 1 to be
rejected are closed at the domain boundary, served route, mutation boundary,
current merged source, and installed live preview surface:

- a repository outside the resolved project/task binding is refused with
  `repository_mismatch`; and
- a publication carrying `pull_request` but no `title` is refused with
  `pr_title_key_missing`, while a push with neither remains permitted.

This report is an independent verifier artifact. It does not merge, deploy,
waive a gate, or broaden the approved CLI/Kontor scope to the deferred forge
controls.

## Source and evidence identity

The implementation artifact is
`docs/evidence/ASMA-8102/HIGH-CHANGE-RECORD.md` at commit
`8134327955da5c3df9d2a37141b2229a6d251e9b`. It qualifies corrective source
`bdf088b153652ed02ea03e0589392ceb62e23ced`, tree
`cdb2ef0677e3fd188ad1b005a5e290a68f3a5956`.

The current `origin/master` does not carry `bdf088b1` as an ancestor because
the coherent candidate was squash-integrated by
`e3a0c6c866b8e5e8949ea36e2a223ba88feb7cf6` (PR #239). The behavior is not an
unmerged side-branch claim:

- `crates/kontor-core/src/publication.rs` is the exact same Git blob
  `9b874983b13ab5d32534f0b8b5d589afa10c63ce` at `bdf088b1`, `e3a0c6c8`, and
  current `origin/master`;
- `crates/kontor-core/tests/publication_identity.rs` is the exact same Git
  blob `b97e51ae2d88f30cf07d79341ec34d0e38eced4b` at `bdf088b1` and current
  `origin/master`; and
- the complete publication constants, repository derivation,
  `resolve_publication_binding`, and `judge_publication` section of
  `crates/kontor-daemon/src/applications.rs` has SHA-256
  `f63fddb0265ccf2cc755e8594efd9bf3145288aeb3bacf5dd95170bc08a3778b`
  at `bdf088b1`, `e3a0c6c8`, and current `origin/master`.

The merged mutation evidence is commit
`d19cdb3dc552c8d9a77e58ca4c89cfe81f669e1d` (PR #246). Its only changed path
is `docs/evidence/ASMA-8102/MUTATION.md`; the blob is
`422765a3cbf406923d32845e4e42adaf04d6a132`, with SHA-256
`f7909683bbbf5ea3fe9c19e64014e9c21685276f904936eb42d0c7e5a937be32`.

## Acceptance findings

| Requirement | Independent evidence | Verdict |
| --- | --- | --- |
| Versioned identity and stable typed refusals | `POLICY_REVISION == 4`; complete `publication_identity` suite | PASS |
| Repository is judged atomically with the binding | Domain negative matrix, full loopback suite, live wrong-owner preview | PASS |
| Repository authorization is project-specific | `another_project_does_not_inherit_the_governed_repositories`; governed-project mutant | PASS |
| Task repository scope excludes sibling and unknown modules | Module-less and task-narrowing domain/route cases | PASS |
| Pull requests require a bound title key | Domain test, route test, live null-title preview, title mutant | PASS |
| Pushes do not invent a title requirement | Domain and route positive controls | PASS |
| Ambiguous resolution fails closed | `a_branch_key_resolving_to_an_epic_and_task_is_refused_as_ambiguous` | PASS |
| Wrong-project resolution fails closed | `a_task_from_another_project_in_this_realm_does_not_resolve` plus the repository fence | PASS |
| Replay and changed-head idempotency conflict | `an_epic_branch_publication_is_attested_recorded_and_replayed` | PASS |
| Refused decisions remain durable evidence | `a_publication_into_an_unauthorized_repository_is_refused` | PASS |
| Stale current-head comparison is covered | Exact `github_publication` stale-head unit test | PASS, downstream PUB-02 consumer only |
| Preview/attest/read remain integrated | Complete daemon loopback suite | PASS |
| MCP contract states both conditional rules | Registry descriptions and installed CLI help readback | PASS |

Policy revision 4 preserves the later ASMA-8114 naming rule: branch and PR
title carry the same confirmed Jira key. That later, more specific accepted
scope supersedes the older plan sentence that permitted an epic branch to carry
a child-task title; this verification does not regress policy revision 3 to the
earlier epic-wide title set.

## Live reproduction of the rejected findings

At 2026-09-21T08:27:32Z–08:27:33Z, the installed daemon in realm
`01a00649-9ee6-73e0-ba1b-6a6c35cfd065` evaluated the current ASMA-8102 task
branch through `publication:preview`:

| Case | Result |
| --- | --- |
| `Carasent-ASMA/asma-rs-kontor`, PR 246, title `ASMA-8102 Qualify publication refusals` | `accepted: true`, no reasons, policy revision 4, exact task resolution |
| Same identity with repository `WrongOrg/wrong-repository` | `accepted: false`, `repository_mismatch`, policy revision 4 |
| Same governed repository and PR number with `title: null` | `accepted: false`, `pr_title_key_missing`, policy revision 4 |

These are read-only previews and created no attestation. They directly reverse
the two policy-revision-3 responses recorded by the rejected verifier.

## Independent mutation verification

Both mutants were reproduced in a disposable detached worktree at exact
`bdf088b1`; the authoritative checkout was never edited.

1. Mandatory-title mutant:

   ```diff
   -        (Some(_), None) => reasons.push(PublicationRefusal::TitleKeyMissing),
   +        (Some(_), None) => {}
   ```

   - domain result: exit 101; `a_pull_request_must_carry_a_title` observed
     `[]` instead of `["pr_title_key_missing"]`;
   - route result: exit 101; `a_pull_request_without_a_title_is_refused`
     observed `accepted: true`, `reasons: []`, and `title: null`.

2. Governed-project mutant:

   ```diff
   fn is_governed_publication_project(project_id: ProjectId) -> bool {
   -    ProjectId::parse(PUBLICATION_GOVERNED_PROJECT).is_ok_and(|governed| governed == project_id)
   +    let _ = project_id;
   +    true
   }
   ```

   - route result: exit 101;
     `another_project_does_not_inherit_the_governed_repositories` observed an
     ungoverned project accepted into `Carasent-ASMA/asma-modules` with no
     refusal reasons;
   - compiler secondary signal: the governed-project constant became unused.

After restoration, source matched the committed blobs exactly:

- `publication.rs`: blob `9b874983b13ab5d32534f0b8b5d589afa10c63ce`,
  SHA-256 `238c83a67ddad309f6ff084f3a622b944d04c73473dce45b657267c298e7827a`;
- `applications.rs`: blob `d0cec625f40587b1ee7efd12402e442061db4e50`,
  SHA-256 `fdbb02c2b211e49cf634a5c0f9e1efd2fe119143602c27e486361aede6e10a46`.

The exact domain title test and governed-project route test then passed. The
disposable worktree was clean and removed.

## Executed checks

All checks ran against the clean source checkout, with locked dependencies.

```text
cargo test -p kontor-core --locked --test publication_identity
  13 passed, 0 failed

cargo test -p kontor-daemon --locked --test loopback_api
  392 passed, 0 failed, 1 intentionally ignored

cargo test -p kontor-daemon --locked \
  github_publication::tests::a_current_github_head_refuses_a_truly_stale_publication_sha \
  -- --exact
  1 passed, 0 failed

cargo fmt --all -- --check
  exit 0

cargo clippy -p kontor-core -p kontor-daemon -p kontor-mcp \
  --all-targets --locked -- -D warnings
  exit 0

git diff --check bdf088b1^ bdf088b1
  exit 0
```

The ignored loopback test is
`a_configured_jira_boundary_distinguishes_historical_from_native_completion`,
which declares itself superseded by native connector contract tests.

## Boundaries

- This PASS covers PUB-01 and the two rejected-gate corrections. It does not
  reactivate deferred GitHub App/ruleset enforcement or claim direct raw
  Git/GitHub bypass prevention.
- The stale-head proof is cited only as TASK-001 coverage exercised through
  the downstream PUB-02 consumer; no App activation claim is made.
- The compiled governed `ProjectId` remains a deliberate current mapping. A
  data-driven per-project repository configuration would be separate scope.
- No source behavior, Jira item, topology, authorization, workspace, seat,
  run, deployment, or lifecycle state was changed by verification.
