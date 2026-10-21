# Command reference and worked example

Load when you need the exact commands for a step of
[long-task-commit-checkpoints](../SKILL.md) or a full example. The rules,
including when to commit and what to stage, stay in the skill. The examples use
conventional checkpoint subjects with neutral scopes. The project CLI validates
the checkpoint subject against its own convention, which you read live from the
CLI; `{{ project.branch.commit_pattern }}` is the PR-title/squash-subject
grammar, not the checkpoint subject.

## Command reference

```sh
# 1. Ensure the tracker issue exists. Paseo-direct (default): create it directly.
{{ project.cli.command }} task create --type feature --summary "..." --description-file scope.md --parent <TRACKER-KEY>
#    Kontor-governed epic instead: read the task and its binding through `kontord`
#    and capture both ids, e.g. task UUID + <TRACKER-KEY>.
kontor task-get --project-id <project-id> --task-id <task-id>
kontor ticket-reconcile-plan --project-id <project-id> --task-id <task-id>

# 2. materialize the catalog module if needed, then branch before any commit
{{ project.cli.command }} module sync <module-path>
{{ project.cli.command }} git --repo <target-repository> checkout -b --from <TRACKER-KEY>

# 3. per-phase checkpoint (stage the coherent subset first)
git add -p                                        # or: git add <paths>
{{ project.cli.command }} git commit --auto       # generated, validated, no push
{{ project.cli.command }} git commit -m "feat(users-overview): Add CSV export"   # hand-written (still validated)

# 4. On the planned release branch only: push + open PR when none exists
{{ project.cli.command }} git commit --auto --push --create-pr
{{ project.cli.command }} git commit --push --create-pr --force   # nothing to commit, but still push + open PR

# later checkpoints once a PR exists (--create-pr harmless but unnecessary)
{{ project.cli.command }} git commit --auto --push

# helpful
{{ project.cli.command }} git status
{{ project.cli.command }} git commit --auto --dry-run   # show planned git commands only
```

If the project CLI does not expose the verb shown, use its declared verbs
(`{{ project.cli.verbs }}`) or the equivalent supported operation and record
the exact command in the PR. For a missing module verb, use the project's
documented checkout/materialization procedure; for a missing worktree verb, use
`git worktree add` under `{{ project.worktree_layout }}`.

---

## Worked example — one multi-phase task, full flow

Task in `users-overview`: *"Add CSV export to the users-overview widget."* Plan
agreed, no Kontor task or tracker binding exists yet.

```sh
# tracker + branch (once)
#   create/reuse <TRACKER-KEY> directly on Paseo; through `kontord` when governed
{{ project.cli.command }} git --repo <target-repository> checkout -b --from <TRACKER-KEY>

# phase 1 — separable prep (stage only the hook extraction)
git add src/widgets/users-overview/hooks/useUserListSelectors.ts src/widgets/users-overview/...
{{ project.cli.command }} git commit --auto
#   -> refactor(users-overview): Extract user list selectors into hook

# phase 2 — dependency (isolated)
git add package.json pnpm-lock.yaml
{{ project.cli.command }} git commit --auto
#   -> chore(deps): Add papaparse for CSV serialization

# phase 3 — feature slice, push + open PR (first pushed checkpoint)
git add src/widgets/users-overview/...
{{ project.cli.command }} git commit --auto --push --create-pr
#   -> feat(users-overview): Add CSV export for user list
#        - Serialize visible rows with the shared filter applied
#        - Wire export action into the table toolbar

# phase 4 — tests (PR already exists: push only)
git add src/widgets/users-overview/__tests__/...
{{ project.cli.command }} git commit --auto --push
#   -> test(users-overview): Cover CSV export empty and filtered cases

# phase 5 — docs, and move the tracker issue
git add docs/...
{{ project.cli.command }} git commit --auto --push
# move the exact tracker task through the selected orchestration surface in Step 1
#   -> docs(users-overview): Document CSV export usage
```

Why this is right: scope is `users-overview` (the widget) not the repository
root; prep and deps land isolated before behavior; the feature slice opens the
PR; tests and docs follow with plain `--push`; each subject stands alone, so the
`git log` tells the whole story — and the branch + PR + transitions keep it
linked to `<TRACKER-KEY>` end to end.
