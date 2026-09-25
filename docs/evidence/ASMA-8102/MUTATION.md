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

- ~~The project-scoped repository guard has no mutant.~~ Closed below, in
  *Mutant 2*.
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


# Mutant 2 — the governed-project guard

Date: 2026-09-20
Base: this ledger's own parent, `4d19761277575e35b6e4d789a373a8bf6fa0936f`,
whose source tree is `cc33a870`'s unchanged.

Closes the residual Mutant 1 recorded: the project-scoping half of ASMA-8102.

## Mutant

The guard decides whether a Kontor project carries the governed ASMA forge
mapping at all. Disabling it restores the exact widening two review rounds
rejected: every project in the realm authorizing `Carasent-ASMA/asma-modules`
and `Carasent-ASMA/asma-rs-kontor` merely by holding a confirmed tracker key.

File: `crates/kontor-daemon/src/applications.rs`.

```diff
 fn is_governed_publication_project(project_id: ProjectId) -> bool {
-    ProjectId::parse(PUBLICATION_GOVERNED_PROJECT).is_ok_and(|governed| governed == project_id)
+    let _ = project_id;
+    true
 }
```

## Digests

| State | SHA-256 of `applications.rs` | Git blob |
| --- | --- | --- |
| Pristine | `1b5182ee0e1a4a7330ed65959823d04b92d4127ed84de876b8ec7b214c170833` | `163e3fd025511fda9ab09c6bc8e42eb110b2a579` |
| Mutated | `8d4a0e3abe495f3509b2ccc59cf9f3e3a783b8820fe840fc4ca1a88fa7b6e5fe` | `fc42a0b5757c0df6d69da03c86820d3632bbc6cb` |
| Restored | `1b5182ee0e1a4a7330ed65959823d04b92d4127ed84de876b8ec7b214c170833` | `163e3fd025511fda9ab09c6bc8e42eb110b2a579` |

`cmp` reported the restored file byte-identical to the pristine copy and
`git status --porcelain` reported no change. No mutant remains in the tree.

## Reproduction

```sh
cd /private/tmp/asma-8102-mutation   # already at 4d197612
cp crates/kontor-daemon/src/applications.rs /tmp/applications.rs.pristine

# seed: make is_governed_publication_project return true unconditionally
cargo test -p kontor-daemon --test loopback_api \
  another_project_does_not_inherit_the_governed_repositories
cargo test -p kontor-daemon --test loopback_api \
  a_module_less_task_is_not_authorized_for_a_module_repository
cargo test -p kontor-core --test publication_identity \
  a_repository_outside_the_binding_is_refused

cp /tmp/applications.rs.pristine crates/kontor-daemon/src/applications.rs
cmp /tmp/applications.rs.pristine crates/kontor-daemon/src/applications.rs
cargo test -p kontor-daemon --test loopback_api \
  another_project_does_not_inherit_the_governed_repositories
```

## Result

| Test | Layer | Under mutant | Restored |
| --- | --- | --- | --- |
| `another_project_does_not_inherit_the_governed_repositories` | daemon loopback route | `FAILED. 0 passed; 1 failed` | `ok. 1 passed` |
| `a_module_less_task_is_not_authorized_for_a_module_repository` | daemon loopback route | `ok. 1 passed` — does not kill | `ok. 1 passed` |
| `a_repository_outside_the_binding_is_refused` | domain | `ok. 1 passed` — does not kill | `ok. 1 passed` |

The killing regression is exactly one test, and it is the cross-project
isolation case. Its red is the assertion, not a malformed invocation:
`loopback_api.rs:55795` recorded the served route answering
`"project_id": "01a0bc5d-10fd-…"` (an ungoverned project),
`"repository": "Carasent-ASMA/asma-modules"`, `"accepted": true`,
`"reasons": []`, against an expected `["repository_mismatch"]`.

Two tests deliberately stay green, and each records something:

- `a_module_less_task_is_not_authorized_for_a_module_repository` covers the
  root-plus-module narrowing, not project scoping. With the guard disabled the
  derivation still runs, so a module-less task still receives only the root and
  still refuses `asma-rs-kontor`. It is not a killing test for this mutant and
  must not be cited as one.
- `a_repository_outside_the_binding_is_refused` is a domain test over
  `PublicationBinding.repositories` as given. The guard is a daemon-side
  derivation of that field, so no domain-level test can kill this mutant. The
  domain half of the repository rule is killed instead by the ASMA-8115 ledger's
  "treat every repository as approved" mutant.

Both suites were green after restore: loopback `393 passed; 0 failed`, PUB-01
domain `13 passed; 0 failed`.

A secondary signal worth recording: with the guard disabled, `rustc` emitted
`dead_code` for the now-unused `PUBLICATION_GOVERNED_PROJECT` constant, so this
mutant is additionally caught by the repository's `clippy -D warnings` gate.

## Acceptance status after this pass

TASK-001's "seed and kill a validator mutant" is satisfied for both ASMA-8102
corrections: Mutant 1 for the mandatory pull-request title, Mutant 2 for the
project-scoped repository guard. The deferred items recorded under Mutant 1 —
`publication:merge` authorizing from optional GitHub App configuration, and the
governed project being a compiled constant rather than a per-project column —
are unchanged and remain outside PUB-01.
