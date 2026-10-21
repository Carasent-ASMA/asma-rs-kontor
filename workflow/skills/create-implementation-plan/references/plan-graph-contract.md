# Plan dependency graph contract — version 1

Applies to all current, unfinished implementation plans. Completed plans,
historical records and redirects are excluded from the migration. This is the
planning domain's file contract for both orchestration surfaces. It does not
change a live plan pin, workflow selection or admission authority.

## Author one source

Inside `### 2.5 Dependency Graph and Waves`, place exactly one fenced block
whose language is `plan-graph`. Its body is strict JSON with these fields:

| Object | Required fields | Meaning |
| --- | --- | --- |
| Root | `schema_version`, `epic`, `tasks`, `external_fences` | Version is integer `1`; tasks is nonempty; fences may be empty. |
| Epic | `jira`, `kontor_id` | Confirmed identifiers, or explicit JSON `null` before binding. |
| Task | `id`, `title`, `phase`, `jira`, `kontor_id`, `depends_on` | Stable local code, single-line title and phase label; bindings as above; array of sibling local prerequisite codes. |
| External fence | `task`, `prerequisite`, `owner`, `lift_condition` | Local dependent task, named external prerequisite, responsible owner and observable lift condition; all nonempty strings. |

All fields are required; unknown fields are rejected. Duplicate JSON keys are
errors. No comments, trailing commas, YAML aliases, NaN, inferred edges or
alternative schema. Local codes follow `TASK-001`, `KN-01`, etc.;
`<tracker-key>` belongs in `jira`, never in `id` or `depends_on`. Every delivery
task appears exactly once in the §2 Task tables and once in the graph. Task
tables have `Task` as their first column and a `Closure condition` column; each
Task cell contains only its local code, optionally in Markdown code/bold
formatting. Every row has exactly the header's cell count and a nonempty closure
condition. Escape literal pipes in cell content as `\|` so they cannot shift
other columns.

`depends_on: []` is an explicit root. `A.depends_on: [B]` means **B → A**.
The validator rejects self-edges, duplicate edges, unknown siblings and cycles.
The longest predecessor path determines a node's zero-based wave, including
joins. No authored wave, readiness, progress or runtime status field is accepted.
External fences do not create sibling edges or confer scheduler readiness.
Unknown owner/lift conditions must be resolved before the graph can validate.

Use the [plan template](../../../templates/plan.md) as the complete example.
Tracker keys must be confirmed keys that match
`{{ project.tracker.key_pattern }}`; the validator checks shape/uniqueness, not
remote existence. Draft `null` bindings are honest and accepted for structural
review. Launch-ready validation requires a tracker key on the epic and every
task, and Kontor IDs when frontmatter selects `orchestration: kontor`. It does
not select the surface, replace the paired governance markers or query `kontord`.

## Generate and validate

Use the project's canonical CLI commands from the owning checkout. They call
the same validator used by the commit hook, CI and graph readers; there is no
second validation implementation.

The CLI resolves that validator from the caller's checkout, not from the plan's
path or the CLI installation. Before validating across worktrees, verify the
selected validator matches the revision agreed for the migration or handoff. If
the candidate checkout carries an older validator, run from the checkout
containing the agreed revision and pass absolute candidate paths. Do not switch
branches or overwrite another owner's checkout to do this.

```sh
{{ project.cli.command }} plan sync path/to/plan.md
{{ project.cli.command }} plan validate path/to/plan.md
{{ project.cli.command }} plan validate --for-launch path/to/plan.md
{{ project.cli.command }} plan export path/to/plan.md
```

If the project CLI does not expose plan verbs, substitute the project's
supported plan validator and record the exact command names.

`sync` replaces only the block between `<!-- plan-graph:derived:start -->` and
`<!-- plan-graph:derived:end -->`. The template supplies those markers. It
validates graph structure before writing; it never fills bindings or changes
dependencies. `render` prints this view without writing. The generated view
contains wave rows, explicit directed edges and the task/tracker/Kontor binding
projection. Keep scheduling facts and join reasons in prose after it.
`validate` fails if the projection drifts, is missing, duplicated or outside
§2.5.

## Mandatory model validation loop and gates

The plan's author/model owns the loop: **run → read every diagnostic → fix →
rerun until the command exits 0 and reports PASS**. Validation is an action,
not an instruction left for the next person. Never skip a failed rule, weaken
validation, suppress an error or fabricate a binding/dependency to get PASS.
If a diagnostic needs a missing fact or an authorized decision, resolve it
through the owning workflow; until then, record the exact unresolved diagnostic
and keep the plan draft/not ready. Continue independent work where authorized.

1. **Before review/handoff:** save the plan, run the project's plan sync and
   validate operations, fixing all reported failures. Honest null bindings are
   allowed at this draft/review stage. Do not call this launch-ready.
2. **Before declaring ready for launch:** after confirmed tracker bindings (and
   Kontor IDs on governed work), run launch-ready validation. Any failure blocks
   the readiness claim and handoff as ready for launch.
3. **Before execution admission:** the receiving coordinator runs that same
   launch-ready validation against the authoritative saved revision before
   lifting the kickoff hold or dispatching the first delivery work. The author's
   earlier PASS is not a substitute for verifying the received content.
4. **After any plan edit:** the previous receipt no longer covers the file.
   Regenerate the view if needed and repeat the applicable validation loop
   before the next readiness claim/handoff or admission based on the changed
   revision. Preserve active pins and historical evidence; this is no
   instruction to stop already admitted work or rewrite another coordinator's
   plan.

Record the exact command, exit code, actual PASS, source SHA-256 and plan
path/revision in the handoff or checkpoint **outside the validated plan**.
Include the working directory and selected validator path/SHA-256 so the
receiver can distinguish validation of the same plan by different revisions.
Writing a receipt back into that file changes its hash and requires another run.
Multi-file commands qualify only when the whole command succeeds; an earlier
file's PASS cannot hide a later failure. Retain actionable diagnostics on failure.

These gates apply to all current implementation plans under the migration rules
below. Migration does not retroactively revoke historical admission or stop
already admitted work. A new launch or admission based on an amended plan needs
the applicable check on that exact revision; there is no legacy bypass.

The graph check does not establish closure quality, scope/risk, remote binding
existence, full template compliance or admission authority. Continue all owning
workflow checks and repair their findings too.

`export` emits deterministic JSON: source path/hash, graph hash, bound epic,
nodes with computed `wave`, edges (`from` prerequisite, `to` dependent), grouped
waves and external fences. It performs full structural validation first and
exits nonzero without graph output on failure. Consumers must not fall back to
ASCII/Mermaid or prose extraction when a canonical block is absent or invalid.
Show graph unavailable with the diagnostic. No `kontord` connection is needed
for these read-only facts.

## Enforcement and existing plans

The shared pre-commit hook checks staged changes; it reads index blobs, not
unstaged files. The plan dependency-graph check runs the same validator on the
PR revision range and runs its behavior tests. Neither installs packages nor
contacts the tracker or `kontord`. Repository branch-protection configuration
remains separate; adding a workflow does not itself make its status required.

Changed Markdown files are plan candidates by `plan/` or `plans/` path,
`plan-`/dated `-plan-` filename, or by `goal:` plus the Implementation Steps
heading. Skill templates are checked explicitly by the validator tests, not
classified as deliverable plans. New plan files must comply. Existing canonical
plans must continue to comply, including when their graph is removed.

The checker requires a canonical graph on every changed current plan, including
status/evidence edits. A pre-existing noncanonical record is exempt only when
both old and new frontmatter have one exact terminal `status`: Completed, Done,
Released, Cancelled/Canceled, Superseded, Archived or Historical
(case-insensitive, optionally quoted). It reports `HISTORICAL`, never PASS.
Missing, duplicate or qualified status text does not establish this exception.
Reactivating a record, new files and previously canonical plans require
validation. Merely changing an active plan's status to Done cannot bypass the
check. Untouched completed records need no rewrite to satisfy a changed-file
check.

Migration is deliberate: enumerate every current implementation plan and record
its inclusion or exclusion with status evidence. Active, blocked and unfinished
draft plans are included; completed plans, historical records and redirects stay
unchanged. The authorized migration writer enumerates every real task and explicit
dependency, resolves ambiguous joins with the owner, separates external fences,
generates the view and fixes all validator diagnostics. Preserve goal, acceptance,
task ownership, active pins and historical receipts. Use isolated copies and
reviewed deltas for concurrent owners; migration is not authority to rebind their
checkpoints or publish their plans. No legacy graph extractor is retained.
