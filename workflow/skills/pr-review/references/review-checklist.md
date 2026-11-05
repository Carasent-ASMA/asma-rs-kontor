# Review checklist

The desk checks human judgment only. Tools already check the rest.

## Read first, in order

1. The repository's nearest `AGENTS.md` and `CLAUDE.md`. Module guidance adds to
   the root rules in its scope; it does not override root authority.
2. Any decision record that covers the area: open the register named in the
   facts and match its Area column.
3. The tracker text and the whole diff, not only the changed hunk.

Initial review covers the whole diff and affected callers/contracts. For an
eligible delta, review the correction, affected behavior and all unresolved
findings; reuse only valid initial evidence. A changed head/base, criteria or
policy invalidates the evidence it affects. Missing evidence stays pending.
Record all real findings; do not restart review for cosmetic preferences.

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
11. **Done means.** Decide PR acceptance against its declared whole/partial
    scope, separately from task/epic completion. Map every in-scope criterion
    to exact-head/base proof and the criteria revision. Report
    `pr_acceptance.verdict: yes` only when that scope is proven; otherwise
    `no`, with missing/failed evidence explicit. A ticket without formal
    criteria uses its usable summary/description; a keyless release uses its
    own description. No usable goal means pending, never an invented criterion.
    Partial delivery names what remains. A merged PR or green build never
    substitutes for required QA/AUD, deployed acceptance or a P10 completion
    committee. Only the existing acceptance owner may establish full ticket Done.

## Tools check this, not you

Do not write findings about: format, lint, types, import order, spelling,
commit format, CI result or coverage numbers. Report relevant tool failures
as unresolved gate evidence; do not duplicate them as cosmetic findings.
Select tests for changed/added behavior and concretely affected callers/contracts
under repository policy, not automatic full suites. Preserve applicable scoped
build/type/dependency/security and independent QA obligations.
