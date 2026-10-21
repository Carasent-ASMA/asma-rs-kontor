# Comment and finding format

Apply the shared communication rules in
[`kontor-communication`](../../kontor-communication/SKILL.md) for the
workflow-wide language and brevity rules. This reference owns only PR-specific
labels, required evidence, attribution and posting mechanics.

## The desk note

The desk posts under a human forge account, so every review starts with this
one line:

> This review was written by the AI review desk. A human still makes the merge decision.

## Labels

| Label | Meaning | Blocks the pull request? |
| --- | --- | --- |
| `blocking` | A real defect or risk that must be fixed before merge. | Yes |
| `should fix` | A real problem the author should fix; a human may accept it. | No |
| `question` | A real doubt, but you cannot show a failing case. Ask, do not claim. | No |
| `pre-existing` | A problem that already exists on the base branch. | No, never |
| `nit` | A small style or taste point. Off by default; use only when asked. | No |

## Finding format

Each finding has four parts:

```text
`file:line` — label: one-sentence problem.
Why it fails: a concrete input, state or sequence that breaks.
Suggestion: a concrete fix, not "consider refactoring".
Proof: what you read or ran to be sure.
```

Rules:

- Give about five findings in full, blockers first. List every additional
  unresolved blocker briefly with its evidence link. Keep other findings in the
  linked record; never drop a blocker to meet a length target.
- No proof, no finding. If you cannot show a concrete failing case, make it a
  `question`.
- Do not repeat a point the forge's automated review already made on the pull
  request.
- One point per finding. Do not bundle.
- Point at the exact new line (`file:line`), not a range when one line is enough.
- Follow the shared communication skill's plain-language and requested-language
  rules.

## The fix PR and suggested changes

The desk keeps one active fix PR per pull request for the `blocking` and
`should fix` findings that have a concrete fix; a later round adds its fixes to
that same fix PR. The review links that fix PR, and a finding the fix PR covers
gets no `suggestion` block. Questions never get a fix; they stay review
comments.

A local fix that is not in a fix PR — the pull request comes from a fork, where
the desk cannot open one, the desk refused the round because the author decides
on the active fix first, or the SWE could not fix the finding — is a forge
suggested change: a `suggestion` block on the exact `file:line` of the finding.
The author applies it with one click; the applier is the committer, and the desk
is credited as co-author.

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
the facts say `still_needed: flagged`, then the link to the round's fix PR when
there is one, then the JUDGE-kept findings, `blocking` first. Questions and
`pre-existing` findings never block.

## The scheduled follow-up comments

The desk, not a seat, posts the silence notice and the scheduled follow-up
comments. They
are plain English for a non-native reader, end with the hidden mark
`<!-- pr-desk -->`, and each carries a hidden
`<!-- pr-desk follow-up <word> <head> -->` line so one effect posts once per head:

- **draft:** the check failed; the comment names each failed check and says to
  fix them and set the pull request ready again.
- **branch:** the branch is outside the project grammar
  (`{{ project.branch.pattern }}`); the comment says the desk does not merge it
  and a human merges it after the branch and title are fixed.
- **untested:** no test area covers the change, or a prepared dependency
  checkout is missing; the comment says a human must check and merge the pull
  request.
- **not-granted-pass / not-granted-fail:** the repository is not granted, or the
  pull request is outside its scope; the desk only tells the author it is ready
  to merge, or names each failed check.
- **criteria:** the JUDGE answered `unchecked` for the Done means; the comment
  asks once for acceptance criteria.

A seat never posts these comments; the watchdog does.
