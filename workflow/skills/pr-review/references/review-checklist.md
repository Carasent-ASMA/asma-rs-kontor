# Review checklist

The desk checks human judgment only. Tools already check the rest.

## Read first, in order

1. The repository's nearest `AGENTS.md` and `CLAUDE.md`. Module guidance adds to
   the root rules in its scope; it does not override root authority.
2. Any decision record that covers the area: open the register named in the
   facts and match its Area column.
3. The tracker text and the whole diff, not only the changed hunk.

If an answer is "I cannot tell", say so and ask. "Unsure" is not a pass.

## The 11 human-judgment checks

1. **Right thing.** Does the change do what the tracker ticket asks — no more,
   no less? Is something asked missing, or something not asked added?
2. **Logic.** Is the reasoning correct? Check empty inputs, null state, error
   paths and boundaries.
3. **Fit.** Does it follow the decision records and the patterns already in this
   repository? Is this already implemented somewhere (duplicate work)? When the
   repository owns an audit or consistency check for this area, run it or name
   why you could not.
4. **Simplicity.** Is this the smallest change that solves the problem? Any
   needless abstraction, dependency or configuration?
5. **Contracts and migrations.** Are API, database and event changes
   compatible? Can the migration run forward and back safely? Do old and new
   versions work together while both are live?
6. **Security and tenant data.** Are the authorization checks in place? Can one
   tenant see or change another tenant's data? Any secret, injection or money
   path involved?
7. **Test value.** Do the tests prove behavior that matters in production, not
   just that lines ran? Would a test fail if the change were wrong?
8. **Readability.** Will the next reader understand this without asking the
   author? Are the names honest?
9. **Production risk.** How can it fail in production? Check rollout,
   configuration, performance, logging and retries.
10. **Effects outside the diff.** Callers, other repositories, shared libraries,
    documentation, feature flags and downstream consumers.
11. **Done means.** Does the head meet the tracker ticket's Done means or
    acceptance criteria? A pull request that says it completes the ticket must
    meet all of them; one that delivers part of a ticket must meet the part its
    description names and break nothing else. A ticket without usable criteria
    uses its summary and description as the goal. Answer `unchecked` only when
    nothing usable is there. A keyless release pull request uses its own
    description as the goal. Without a `yes` for this head nothing merges.

## Tools check this, not you

Do not write findings about: format, lint, types, import order, spelling,
commit format, CI result or coverage numbers. If a tool reports a failure, say
that one fact; do not review it line by line.
