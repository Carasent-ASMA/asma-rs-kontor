# ASMA-8102 / PUB-01 publication-validator mutation ledger

Date: 2026-09-19
Base: `cc33a870bf320df4c77299d92258de85d9ee8233`, tree
`ae4a277f4178dd9a85bc59f03db0f3ef6f8b60af`.

`plan/infrastructure-publication-identity-enforcement-1.md` row TASK-001
requires a seeded and killed validator mutant. `docs/evidence/ASMA-8115/MUTATION.md`
already carries inherited PUB-01 mutants, but its latest re-verification runs on
base `d5fef521` — before `bdf088b1`, the commit that added ASMA-8102's two
corrections. Neither of those corrections had a mutant. This ledger closes that
for the mandatory-pull-request-title rule.

## Mutant

The rule under test is the one the high-verification gate rejected PUB-01 for:
a pull request whose title was null skipped title validation entirely and was
accepted, so it could publish under no confirmed Jira key.

File: `crates/kontor-core/src/publication.rs`, in `evaluate`.

```diff
-        (Some(_), None) => reasons.push(PublicationRefusal::TitleKeyMissing),
+        (Some(_), None) => {}
```

This is load-bearing rather than cosmetic: it reinstates the exact defect, and
it is the only arm standing between a titleless pull request and acceptance.
The branch-only arm `(None, None) => {}` is untouched, so a test that passed
merely because pushes carry no title would not turn red here.

## Digests

| State | SHA-256 of `publication.rs` | Git blob |
| --- | --- | --- |
| Pristine | `238c83a67ddad309f6ff084f3a622b944d04c73473dce45b657267c298e7827a` | `9b874983b13ab5d32534f0b8b5d589afa10c63ce` |
| Mutated | `384c077c3f359e7ffea8e356a038906e6bda100d67a957048d3dfe91cc15c9dc` | `086b7a040ebd4300c66642396ae821536e1899c6` |
| Restored | `238c83a67ddad309f6ff084f3a622b944d04c73473dce45b657267c298e7827a` | `9b874983b13ab5d32534f0b8b5d589afa10c63ce` |

`cmp` reported the restored file byte-identical to the pristine copy and
`git status --porcelain` reported no change. No mutant remains in the tree.

## Reproduction

```sh
git worktree add --detach /tmp/asma-8102-mutation cc33a870
cd /tmp/asma-8102-mutation
cp crates/kontor-core/src/publication.rs /tmp/publication.rs.pristine

# seed: replace the (Some(_), None) arm body with {}
cargo test -p kontor-core --test publication_identity a_pull_request_must_carry_a_title
cargo test -p kontor-daemon --test loopback_api a_pull_request_without_a_title_is_refused

cp /tmp/publication.rs.pristine crates/kontor-core/src/publication.rs
cmp /tmp/publication.rs.pristine crates/kontor-core/src/publication.rs
cargo test -p kontor-core --test publication_identity a_pull_request_must_carry_a_title
cargo test -p kontor-daemon --test loopback_api a_pull_request_without_a_title_is_refused
```

## Result

| Killing test | Layer | Red | Green |
| --- | --- | --- | --- |
| `a_pull_request_must_carry_a_title` | domain | `FAILED. 0 passed; 1 failed` | `ok. 1 passed` |
| `a_pull_request_without_a_title_is_refused` | daemon loopback API | `FAILED. 0 passed; 1 failed` | `ok. 1 passed` |

Each red is the assertion detecting the mutation, not a malformed invocation:

- Domain, `publication_identity.rs:309` — `left: []`, `right: ["pr_title_key_missing"]`,
  message *"a pull request without a title names no key"*.
- Loopback, `loopback_api.rs:55701` — the API answered
  `"pull_request": 2985, "title": null, "accepted": true, "reasons": []`, which
  is the rejected finding reproduced end to end through the served route.

The whole PUB-01 domain suite was green after restore: `13 passed; 0 failed`.

## Residual, recorded rather than closed

- **The project-scoped repository guard has no mutant.** `is_governed_publication_project`
  and the root-plus-task-module narrowing in `kontor-daemon` are the other half
  of ASMA-8102 and are still uncovered by any mutation. The ASMA-8115 ledger's
  "treat every repository as approved" mutant kills only the base's
  `repository_mismatch` check, not the scoping added in `bdf088b1`. This pass
  was bounded to one mutant; that one is the next to seed.
- **Domain-level `binding_ambiguous` test: non-blocking.** TASK-001 lists an
  "ambiguity" test without naming a layer, and
  `a_branch_key_resolving_to_an_epic_and_task_is_refused_as_ambiguous` covers it
  in `crates/kontor-daemon/tests/loopback_api.rs`. The plan does not require the
  domain seam, so none was added.

## Deferred, outside PUB-01 scope

- **`publication:merge` still authorizes from optional GitHub App configuration**
  (`crates/kontor-daemon/src/github_publication.rs`), not from the durable
  ProjectId derivation that now governs preview and attest. PUB-02 owns that
  surface. Until it converges, one feature has two authorization sources.
- **The governed project is a compiled constant** (`PUBLICATION_GOVERNED_PROJECT`).
  Portable across machines and realm replicas, but still code rather than data;
  a per-project governed-repository column would need a migration.
