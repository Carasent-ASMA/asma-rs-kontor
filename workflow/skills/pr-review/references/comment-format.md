# Comment and finding format

Apply the shared communication rules in
[`kontor-communication`](../../kontor-communication/SKILL.md) for the
workflow-wide language and brevity rules. This reference owns only PR-specific
labels, required evidence, attribution and posting mechanics.

## The desk note

Every review identifies the desk and the configured merge behavior:

> This review was written by the AI review desk. A qualified PR may merge automatically under the configured policy.

## Labels

| Label | Meaning | Blocks the pull request? |
| --- | --- | --- |
| `blocking` | A real defect or risk that must be fixed before merge. | Yes |
| `should fix` | A confirmed actionable problem owned by the reviewing side. | Yes, until resolved |
| `question` | A real doubt, but you cannot show a failing case. Ask, do not claim. | No |
| `pre-existing` | A problem that already exists on the base branch. | No, never |
| `nit` | A small style or taste point. Off by default; use only when asked. | No |

## Finding format

Each finding has four parts:

```text
`<finding-id>` · `file:line` — label: one-sentence problem.
Why it fails: a concrete input, state or sequence that breaks.
Suggestion: a concrete fix, not "consider refactoring".
Proof: what you read or ran to be sure.
```

Rules:

- Record every real finding, blockers first, with a stable PR-local ID and
  evidence and a persisted stable `root_cause`. There is no quota. A concise
  display may link complete records; presentation limits never remove findings or their dispositions.
- No proof, no finding. If you cannot show a concrete failing case, make it a
  `question`.
- For `blocking` or `should fix`, explain the user impact or violated contract
  in the problem/proof fields. A preference, speculative improvement or an
  extra test without a concrete uncovered behavior is not a confirmed defect.
  Keep optional polish advisory; never relabel a proven defect to get a pass.
- Verify existing findings and link/reuse their IDs instead of posting
  duplicates. Do not omit an issue because another reviewer found it.
- The deciding seat groups sightings of the same proven root cause before
  requesting corrections. Keep one surviving ID and an explicit `duplicate_of`
  disposition for each other sighting, including differently worded reports.
  Preserve their evidence and keys; similar wording alone proves no duplicate.
- One point per finding. Do not bundle.
- Point at the exact new line (`file:line`), not a range when one line is enough.
- Follow the shared communication skill's plain-language and requested-language
  rules.

## Corrections and suggested changes

The reviewing side dispatches SWE to correct confirmed findings on the ORIGINAL
PR branch, followed by independent verification. Link correction commits and
evidence, never a new nested fix PR. A pushed fix is not yet a verified fix.
Keep every ID with its final outcome and reason: `verified` (with current
`fixed_sha`), `rejected`, `duplicate` (with `duplicate_of` naming the surviving
stable ID), `pre-existing` or `deferred`.
Confirmed actionable findings cannot be deferred into a passing PR.

Correction scope follows the proven defect. A small correction gets the
eligible delta verification of its behavior, affected callers and unresolved
findings; it does not restart unrelated checks or reopen settled preferences.
Security, accessibility and other explicit contracts remain acceptance criteria
even when the fix is small.

When direct correction is unsupported or opted out, a safe prepared patch may
be posted as a suggestion on its exact line. Preserve the unresolved finding;
a suggestion is not an applied or verified fix. Never resubmit an author-rejected,
changed or reverted correction automatically. Questions remain comments.

```suggestion
<the replacement lines for that file:line>
```

## The posted review

The PR Adam posts one forge review per round with the desk's `post-review`
operation:

- `Request changes` when at least one `blocking` finding remains, except on a
  pull request the desk account opened, where the forge may allow only a
  comment;
- `Comment` in every other case;
- never `Approve`.

The body is the desk note, then one "Still needed?" note with its reasons when
the facts say `still_needed: flagged`, then correction/evidence links and all
finding dispositions, unresolved actionable findings first. Questions and
unchanged pre-existing issues do not block by themselves. If the change worsens
or newly exposes an existing defect, review that effect as an in-scope finding.

## The scheduled follow-up comments

The desk, not a seat, posts the silence notice and the scheduled follow-up
comments. They
are plain English for a non-native reader, end with the hidden mark
`<!-- pr-desk -->`, and each carries a hidden
`<!-- pr-desk follow-up <word> <head> -->` line so one effect posts once per head:

- **draft:** the check failed; the comment names each failed check and says to
  fix them and set the pull request ready again.
- **branch:** the branch is outside the project grammar
  (`{{ project.branch.pattern }}`); the comment names the required branch/title
  correction and says nothing has merged.
- **untested:** no test area covers the change, or a prepared dependency
  checkout is missing; the comment names the missing evidence and leaves
  acceptance pending.
- **not-granted-pass / not-granted-fail:** the repository is not granted, or the
  pull request is outside its scope; the desk only tells the author it is ready
  to merge, or names each failed check.
- **criteria:** the deciding record lacks usable PR-acceptance criteria; ask
  once for the missing criteria, without claiming the ticket is Done.

A seat never posts these comments; the watchdog does.
