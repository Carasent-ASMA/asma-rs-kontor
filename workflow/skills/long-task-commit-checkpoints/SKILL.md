---
name: long-task-commit-checkpoints
description: Use when committing or publishing during a long or multi-phase job — create or reuse the tracker issue, branch from its key, cut checkpoint commits, stage per concern, push, open a draft or ready pull request, or land it. The mandated lifecycle, driven through the project CLI.
---

# Long-Task Commit Checkpoints

Use this skill for any **long or multi-phase job**. It defines the **mandated
lifecycle** and the checkpoint discipline that produces a history a future
reader understands **from the commits alone**.

**Hard rule: use the project CLI for checkout, commit, branch, push and PR;
plain `git` only to stage and read.** Use the project CLI's git operations
(`{{ project.cli.command }} git ...`). The CLI enforces the commit convention
and wires branch/PR mechanics — so it writes a correct commit message for you,
and your job is deciding **when** to commit and **what** goes in.

---

<a id="the-mandated-lifecycle"></a>
## The mandated lifecycle

Follow these steps in order for every multi-phase job.

### Step 1 — Ensure the tracker issue (and, when governed, the Kontor task) exists

As soon as the plan is agreed, resolve the orchestration surface per the
[transition rule](../epic-orchestration/references/orchestration-surface-transition.md#orchestration-surface-transition-rule).

- **Paseo-direct (default):** reuse the in-scope tracker epic or issue when one
  covers the work; otherwise create it through the project CLI's tracker
  operation (parented to the epic for a task under an epic), applying the
  [task content contract](../tracker-linked-creation/SKILL.md#task-content-contract);
  set or repair an existing body through the same operation. Capture the
  confirmed tracker key. Do not create a legacy backlog ticket.
- **Kontor-governed epic:** read the owning Kontor project/epic/task graph.
  Reuse an existing in-scope Kontor task and confirmed tracker binding, or
  create the task through [mini-project-kickoff](../mini-project-kickoff/SKILL.md) or `kontord` epic
  preview/apply and create-or-link the tracker through
  [tracker-linked-creation](../tracker-linked-creation/SKILL.md). Read the task back and capture both its `kontord`
  id and confirmed tracker key; do not call the tracker directly or use a
  direct tracker-sync command. If `kontord` is unavailable before the task
  exists, stop: a tracker-only issue would be an orphan outside its control
  plane. If only tracker materialization is broken after the Kontor task exists,
  follow that skill's bounded fallback and `operational_gap` evidence rules.

### Step 2 — Resolve the delivery branch before ANY commit

Before the first commit, resolve or create the planned working branch **from the
confirmed tracker issue** (epic, standalone task or planned contribution task as
applicable below):

```sh
{{ project.cli.command }} git --repo <target-repository> checkout -b --from <TRACKER-KEY>
```

- `--from <ISSUE-KEY>` derives the branch name and type from the tracker issue
  (branch type auto-detected; override with the project's declared branch
  types: `{{ project.branch.types }}`).
- Where a `kontord` realm is configured, commit/push and create-PR operations
  also ask `kontord` to **attest** the publication (branch key = confirmed
  epic/task tracker key, base `{{ project.forge.base_branch }}`, title leading
  with a key from the epic graph). For a Kontor-governed epic (automatic inside
  a `kontord`-launched seat) a refusal or an unreachable daemon stops the push,
  and the recorded bypass flag is the only exception; on the default
  Paseo-direct surface the attestation is advisory and only warns.
- Land a PR with the project CLI's merge-pr operation (squash only, bound to the
  attested head commit). Do not use the forge CLI directly.
- A merge needs `{{ project.merge_authority.role }}`'s permission under
  `{{ project.merge_authority.policy }}`; the PR desk merges only on that
  authority's command or on the PR author's configured merge order.
- The result always matches `{{ project.branch.pattern }}`, and that grammar is
  **enforced, not advised**: the CLI refuses to create, push, or open a PR from
  any other branch (reason codes such as `branch_key_missing`,
  `branch_key_not_canonical`), the CLI's worktree operation requires a tracker
  key, and `kontord` derives the same branch for a task worktree from the task's
  confirmed tracker key. Do not hand-name branches (`cat-11`, `kon-op-22`,
  `fix/KON-OP-22-…`) — a Kontor display code is not a tracker key.
- If the target catalog module is missing, materialize it first through the
  project's module operation.
- `--repo` is the safe default. Running **inside the target repository** infers
  the same context, but from the workspace root the bare form can infer the
  superproject — pass `--repo <context>` unless you are certain you are inside
  the child.
- Exactly one source is required: `--from <ISSUE-KEY>` (normal work) or the
  project's hotpatch form.
- Never start committing on `{{ project.forge.base_branch }}` — `checkout -b
  --from` is the entry gate to the work.
- Checkout isolation and collision safety follow
  [checkout ownership and reuse](../epic-orchestration/SKILL.md#checkout-ownership-and-reuse);
  do not reuse another task's checkout without its qualified handoff controls.

#### Milestone branches and publication

Resolve the plan's delivery strategy before creating a branch. The branch rules
for each strategy (epic, milestone, standalone, Kontor-governed, urgent fix)
and the fallback when milestone mechanics cannot be demonstrated are in
[delivery milestones and adoption](../create-implementation-plan/references/delivery-milestones-and-adoption.md#milestone-branches-and-publication).
The CLI mechanics stay here:

- `checkout -b --from <KEY> --title <description>` gives a distinct
  tracker-linked description. Before using it for a milestone, inspect
  `--dry-run` and verify the repository, resulting branch and starting HEAD.
- **Capability limit:** while the installed CLI documents create-PR and merge-pr
  only against `{{ project.forge.base_branch }}`, this skill adds no PR path to
  another base. Do not assume child PRs targeting an integration branch are
  supported.

### Step 3 — Work in phases; checkpoint each with the project CLI

Do the work in phases (see checkpoint discipline below). At each checkpoint,
stage the coherent subset and commit through the CLI. Let the CLI generate the
conventional message:

```sh
{{ project.cli.command }} git commit --auto
```

- `--auto` generates the conventional message through the project backend and
  validates it — you don't hand-craft the shape. Use `-m "..."` only if you
  must (still validated).
- **Mind what lands in the commit — this is the part you control.** By default
  the commit operation commits **staged changes only.** So stage the coherent
  subset first (`git add -p` / `git add <paths>`), then commit. See the
  inclusion model below.

### Step 4 — Publish checkpoints from the planned branch

<a id="pull-request-readiness"></a>

#### Pull request readiness

This rule applies to every team and seat using the pack, on Paseo-direct and
Kontor-governed work alike:

- Keep a PR **draft** while it is unfinished, review-only, explicitly held, or
  waiting for an implementation or integration prerequisite. A body warning
  such as "do not merge" never replaces draft status.
- Mark it ready only when the declared merge scope is complete, your own
  validation has passed, and no hold or merge prerequisite remains. Non-draft
  means you intend merge after the normal PR review, required CI and
  approvals; it does not claim those independent gates already passed, and
  pending review is no reason to keep finished work draft.
- The author or owning coordinator keeps the state right and records the
  remaining prerequisite and its owner in the PR. A closure-only gate is not
  turned into a dispatch or merge dependency.
- Create a draft with the project CLI's push/create-PR operation with its
  draft flag; convert an existing ready PR with the forge CLI's ready/undo
  operation. If neither is possible, push the checkpoint and do not open the PR.
- The project CLI's merge operation refuses a draft, and a ready PR whose title
  or body still carries a hold marker. The review desk skips drafts.

Open a PR only from the planned milestone integration branch or another
explicitly authorized release branch, after its scope is review-ready. Check
the plan's source/target mapping first. A contribution branch may push preserved
checkpoints through the approved workflow, but absence of a PR never authorizes
its own PR to the default branch or release-related tracker movement. Apply the
capability fallback in
[Milestone branches and publication](#milestone-branches-and-publication).

On the authorized release branch, when there is no PR yet:

```sh
{{ project.cli.command }} git commit --auto --push --create-pr
```

- `--create-pr` is **only valid together with `--push`**. It pushes, then opens
  a forge PR from the current branch to `{{ project.forge.base_branch }}`.
- **Nothing left to commit but you still want the first push + PR?** Add
  `--force`. It lets the PR/transition flow continue even when there are **no
  local changes or pending pushes**. `--force` **requires** `--create-pr`.
- **If a PR already exists**, `--create-pr` is **harmless but unnecessary** — a
  plain push with `--auto` is enough for later checkpoints. Passing
  `--create-pr` again will not create a duplicate.
- Keep tracker movement deliberate. On the Paseo-direct surface, the transition
  flag on the PR-creating commit or the project CLI's tracker transition
  operation once source-control evidence exists is the path; for a
  Kontor-governed epic use the linked Kontor task's
  `ticket-reconcile-plan/apply` and no direct tracker transition flag.
- Not every phase needs a push. Local checkpoints are fine mid-phase; push at
  meaningful, review-ready points.

---

## WHEN to cut a commit (checkpoint discipline)

This is the hard part — and the part the CLI can't do for you. The commands are
mechanical and `--auto` writes the message; **deciding the boundaries is the
skill.**

### The one test that decides everything

> **Can I describe the staged change with a single honest conventional subject line, and does the tree still make sense after it?**

- Combines independently reviewable concerns with no coherent purpose → split. A conjunction alone is not evidence that a change is mixed.
- A **fragment** that can't stand on its own (half a rename, a function no one calls yet, code that doesn't build) → **too small / premature** → keep working, don't commit yet.
- **One** honest subject describes it and the tree is coherent → **that is a commit.** Cut it.

A good commit is the **smallest set of changes that advances exactly one phase or concern and leaves the tree coherent** — "one reviewable idea," not a line count.

### Plan the commit breakdown up front

When the plan is agreed (Step 1), also map its phases to a **planned sequence of
commits**. Each phase that ends in a coherent state is a commit boundary. This
stops you from dribbling fragments or piling up a giant uncommitted blob.

### Cut a commit at these checkpoints

- **A phase / plan step is complete and coherent** — a vertical slice works, a migration is written, a refactor is finished, a bug is fixed.
- **Before switching concern or layer** — finished the data layer and about to start the UI? Commit the data layer first.
- **After separable prep, before the behavior built on it** — a rename, file move, formatting pass, scaffold, or dependency bump goes in its **own** commit **before** the meaningful change, isolating the noisy diff from the one that carries intent.
- **A logically-independent fix noticed along the way** — its own commit, not a rider on the feature.
- **Tests for a slice** — kept with the code they cover (same commit, or a following `test:` commit).

### Do NOT commit when

- The change **doesn't build / typecheck** or leaves a dangling reference. On shared branches, **every commit must be buildable** — reviewers and `git bisect` depend on it.
- You're a few lines into the **same concern** you'll continue next. Keep going.

### Do NOT let a commit grow too large — split when

- It takes **more than one honest sentence** to describe.
- It **mixes prep with behavior** (rename + logic change, reformat + fix).
- It **touches unrelated areas** that share no honest scope.

Staging is how you enforce these boundaries: because the commit operation
commits **staged changes only** by default, stage one concern at a time
(`git add -p` / `git add <paths>`) and commit it before moving on. See the
**"What lands in the commit"** section below for the full inclusion model.

---

## What lands in the commit — staging & `--include-*`

The CLI writes and validates the *message*; **you** control the *content*. Get the content right and the generated message is automatically honest.

Default behavior: **the commit operation commits staged changes only.**
Tracked-but-unstaged edits and untracked files are left out unless you opt them
in.

- **Default (recommended)** — stage the coherent subset per concern (`git add -p`
  to split a file that holds two concerns, or `git add <paths>`), then commit
  with `--auto`. This is what turns one working session into several clean,
  self-explanatory commits.
- **`--include-unstaged`** — also pulls in tracked files with **unstaged**
  modifications. Convenient when the whole working tree is one concern, but it
  bypasses per-concern staging — use deliberately.
- **`--include-untracked`** — also pulls in **new/untracked** files. Needed when
  a phase adds new files you haven't `git add`-ed yet.
- Combine both to commit everything in the working tree; prefer explicit
  staging when a session touched more than one concern.

Then just glance at the generated subject — it should read as the single change
you intended. If not, restage more tightly (or override with `-m`).

---

## Command reference

The commands for each step, and a worked multi-phase example, are in the
[command reference](references/command-reference.md).

<a id="worked-example--one-multi-phase-task-full-flow"></a>

---

## Guardrails (the mandate, in one place)

- **Project CLI for checkout, commit, branch, push and PR**; plain `git` only to stage and read.
- **Confirmed tracker identity first**: use the Paseo-direct or Kontor-governed path in Step 1; only the governed path requires a Kontor task.
- **Branch before committing**: `{{ project.cli.command }} git --repo <context> checkout -b --from <ISSUE-KEY>` — never commit straight on `{{ project.forge.base_branch }}`, and do not rely on cwd inference from the workspace root (it can branch the superproject).
- **Stage per concern**: the commit operation commits **staged changes only** by default; use `--include-unstaged` / `--include-untracked` deliberately, not reflexively.
- **PR only from an authorized release branch**: after checking the plan's mapping, use the push/create-PR operation when its PR is absent; `--force` supports that same authorized case with nothing left to commit. Contribution branches push checkpoints without `--create-pr`; PR absence is not authority.

## Quick checklist per commit

- [ ] Working on a `checkout -b --from <ISSUE-KEY>` branch (not `{{ project.forge.base_branch }}`)
- [ ] One coherent reviewable concern — one honest subject
- [ ] Tree is coherent and **builds** at this commit
- [ ] Prep (rename/move/format/deps) separated from behavior
- [ ] Staged the coherent subset (`git add -p` / paths) before committing
- [ ] Committed with `--auto` — glanced at the generated subject; it honestly names the one change
- [ ] Pushed with `--push` when appropriate; added `--create-pr` only on the plan-authorized release branch when its PR is absent (`--force` only for that same case with nothing to commit)
