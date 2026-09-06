# ASMA-8114 high-scope record

## Outcome

Enforce one server-owned identity for every Advisor and Committee consultation,
then reconcile the duplicate and redundantly named KTHSR Committee workspaces
without replacing the surviving consultation or rewriting its findings.

## Reproduced failures

### Free-text consultation identity

The invocation API accepts a `topic` and stores it verbatim. Team Definition
rendering then expands `CSW • <scope item code> • <topic>`. Nothing currently
rejects a topic that begins with either the confirmed Jira key or the derived
Kontor item code. The caller therefore created these live titles:

- `CSW • KTHSR-8111 • ASMA-8111 targetless handoff recovery completion`
- `CSW • KTHSR-8111 • ASMA-8111 operational completion — pinned v1`

The first is a settled, distinct completion review. The second logical topic
exists twice because the retry used a new idempotency key. Invocation dedup is
currently keyed only by that caller key; no durable semantic identity prevents
two keys from freezing the same family, scope, template and topic.

### ASMA CLI worktree compatibility

`asma worktree add ASMA-8114 --mod _tools/asma-rs-kontor` correctly created:

- worktree: `.worktrees/asma-8114/asma-rs-kontor`
- branch: `feat/ASMA-8114-enforce-canonical-consultation-identity-and-reconcile-duplicate-csws`

Kontor's epic preview treated everything below `.worktrees/` as a branch encoded
in the path. It parsed `asma-8114/asma-rs-kontor` as the branch and returned
`branch_type_unknown`, although the checkout's actual branch is canonical and
bound to the confirmed Jira issue. The Paseo adapter already recognizes the
catalog module's separate Git identity, but its final check only searched for
the worktree slug as a substring of the branch.

### Task branch and pull-request identity

PRs #196 and #197 carried `ASMA-8102` titles on the reused branch
`feat/ASMA-8101-publication-identity-enforcement`. Publication policy revision
1 deliberately allowed a title to name any child of the resolved epic, even
when the head branch resolved to a different child task. The attestation was
therefore internally consistent with that permissive rule while violating the
one-Jira-task-per-branch convention.

Policy revision 3 accepts only a title beginning with the same confirmed Jira
key as its branch. Epic work uses the epic branch; a child task requires its own
Jira-keyed branch and checkout.

## Required contract

1. The server accepts a semantic topic, never a pre-rendered container name.
2. A topic must not contain the exact Jira key, derived Kontor item code, Team
   Definition prefix, or configured separator as caller-supplied name material.
3. Kontor derives one canonical identity from project, epic or task scope,
   family, pinned consultation revision, canonical topic and explicit re-review
   provenance.
4. A different idempotency key cannot freeze a second active consultation with
   that identity. The response identifies the existing run and instructs the
   caller to read or resume it.
5. The store enforces the same uniqueness transactionally so concurrent calls
   cannot pass an application-only check.
6. A materially different topic and an authorized re-review remain distinct.
7. The run projection includes the complete server-rendered container name.
8. Materialization succeeds only after exact native title, parent, kind, cwd and
   identity readback.
9. A supported preview/apply correction may remove a redundant legacy prefix,
   choose one survivor in a duplicate group, preserve immutable findings and
   receipts, and retire only the duplicate seats and container.
10. An ASMA CLI catalog worktree is accepted only when its two-segment path is
    bound to the task key and the runtime proves that the actual branch is
    canonical and carries that confirmed key. An absent catalog checkout is
    refused rather than synthesized under a guessed branch.
11. Every branch and pull-request title carry the same confirmed Jira key. Epic
    work uses the epic branch; a child task uses its own branch.

## Live reconciliation targets

KTHSR epic `01a07495-e5e0-7ef2-b285-878ee7fb2bd9`:

- settled completion run `01a0758b-ec8f-7271-a91f-ccd93c1b7202`; correct the
  topic in place and preserve all three findings and native identities;
- incomplete duplicate `01a075bc-50cf-7840-aff1-8181d4435b92`; preserve its
  historical invocation receipt and retire its seats and CSW after readback;
- progressing survivor `01a075bf-c12f-7140-8c19-380229a13ef2`; correct its
  topic in place and keep its recorded finding and current identities.

The desired visible titles are:

- `CSW • KTHSR-8111 • targetless handoff recovery completion`
- `CSW • KTHSR-8111 • operational completion — pinned v1`

Exactly one active CSW may remain for the second logical consultation.

## Validation

- Core branch identity tests for direct and catalog module worktree shapes.
- Daemon loopback tests for redundant-topic refusal and different-key semantic
  duplication, including a concurrent attempt.
- Store migration, roundtrip, backup and schema-v1 compatibility tests.
- Paseo checkout tests proving the actual branch and common Git identity.
- API, CLI and MCP parity regeneration and contract tests.
- Exact live preview/apply, daemon health, SQLite integrity and foreign-key
  readback, preserved project/epic/task/run/seat identities, and native titles.
- Mutation test that removes either topic-prefix rejection or semantic uniqueness
  and demonstrates a failing regression test.

### Mutation receipts

- Replacing publication policy revision 2's task-only title set with the former
  epic-wide set made
  `a_task_branch_binds_through_its_own_key` fail with exit 101: the observed
  reasons were empty instead of `pr_title_key_mismatch`.
- Removing `UNIQUE` from the consultation semantic-identity index made
  `a_fresh_invocation_key_cannot_freeze_the_same_semantic_consultation_twice`
  fail with exit 101 because the second run was inserted.
- Restoring both production implementations returned the complete publication
  contract (7/7) and the focused semantic-identity regression to green.
