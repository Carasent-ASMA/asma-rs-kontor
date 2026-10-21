### Behaviour scenarios (opt-in trial)

A new Paseo-direct plan may declare `behaviour_scenarios: bdd` in its
frontmatter. It then carries §1.1: one Gherkin scenario per user- or
API-observable behaviour the plan changes, each proving a `REQ-` (and an `AC-`
with its spec path when a living spec exists), owned by exactly one task, at
one layer, with one home. Chore, docs and refactor plans do not select it.
Selecting it installs no runtime enforcement; every rule below is agent and
reviewer guidance. Kontor-governed execution applies the same text inside its
pinned gates and claims no new gate, field or lifecycle value.

**Approval and freeze.** Scenarios are approved with the plan by whoever
approves the plan today; the approver reads the Gherkin blocks in §1.1. From
approval the text is frozen: block, hash and home form the **Approved Scenario
Baseline**. Any later change to a hashed line is a **Scenario Amendment**
(below), never a silent edit, and an implementing agent never weakens a
scenario to reach green. A scenario that exposes missing meaning is recorded
in §7 as an open question and resolved before approval.

**Authoring.** Declarative steps at the domain level: no selectors, routes,
function names, SQL or internal identifiers. One observable behaviour per
scenario; a Scenario Outline holds data rows of one rule, and a row with its
own acceptance meaning becomes its own scenario. Where a group has an error
path, include the inverse allowed case. Each scenario is self-contained:
`Background:` and `Rule:` are not used and shared givens are repeated; the
only tag on a scenario is its identity tag and the `Feature:` line carries
none; no `#` comments or `*` steps inside a scenario. A UX working context may
draft scenarios, with the project's drafting source; the plan block is what is
approved, and `WORKING_ROLE` grants no approval.

**Identity.** Draft `SCN-<nnn>` (three digits, unique within the plan). At
binding the owning task's confirmed tracker key is prefixed,
`<tracker-key>-SCN-007`, and the number is retained: never renumbered, never
reused. Tag form `@<tracker-key>-SCN-007`; title form `(<tracker-key>-SCN-007)`
at the end of a runner test title. The key names the task that introduced the
scenario and never changes: a later task amends under the original id and mints
a new id only for a new behaviour. Regex: draft `^SCN-\d{3}$`, bound
`^[A-Z][A-Z0-9]+-\d+-SCN-\d{3}$`. Ids never appear in a closure condition or a
tracker Goal; they live in §1.1, §6 and the task's Verification and evidence.

**Home and layer.** An `e2e` scenario's home is a `.feature` file under the
E2E repository's Playwright test root, executed through playwright-bdd; follow
the project's Playwright instruction for the exact layout. Every other layer's
home is the ordinary runner test that carries the id in its title, with the
Gherkin block as `//` or `#` line comments directly above it (one Gherkin line
per comment line, no `///` or block comments); no `.feature` file exists below
E2E. One scenario has one home and one acceptance layer. A cross-repository
journey is one `e2e` scenario homed in the repository that drives the browser;
an independently meaningful service rule is its own scenario in its own
repository; any other test may cite `supports <id>` in a comment as evidence,
never as a home.

**Hash.** The hashed unit is the scenario body: the `Scenario:` or
`Scenario Outline:` line through the scenario's last line, including doc
strings, data tables and `Examples:` rows. Tags are not hashed: the identity
tag is the only permitted tag and its draft and bound forms differ by
construction; any other tag is a freeze violation checked by rule and by the
repository meta-test. Normalize, then digest: strip a leading `//` or `#`
comment marker from each line, drop lines starting with `@`, trim each line,
drop empty lines, collapse each run of spaces or tabs to one space, join with
`\n` without a trailing newline, SHA-256 in hex. Record `sha256:<hex>` in the
§1.1 row. The same procedure over the plan block and the home must agree at
task start; a mismatch stops the task. A hash proves equality with the
approved text, not approval: approval is the plan approval, and an expected
hash rewritten without an amendment is a review finding.

```sh
printf '%s' "$(sed -E 's@^[[:space:]]*(//|#)?[[:space:]]*@@; /^@/d; s@[[:space:]]+$@@; s@[[:space:]]+@ @g' scenario.txt | grep -v '^$')" | shasum -a 256
```

**Companion and mutation.** E2E scenarios are never mutated and carry no
`MUT-` row. The mutation pass over changed source stays unconditional. Each
`e2e` row names the lower-layer test or scenario that pass exercises for the
decision logic behind the journey, or records
`E2E-only: no changed decision logic`. The exemption releases the scenario
from a companion, never the diff from mutation.

**Amendments.** Record `AMEND-<bound id>-<n>` in §1.1 before the artifact
changes: reason, authority (the plan approver), evidence to revalidate, new
hash, commit. The artifact change and the record land together, and the
commit or PR names the amendment id. Affected §6 rows return to
`Not yet performed` until re-run. A later plan amending an earlier plan's
scenario records the amendment in its own §1.1 under the original id; a
retired scenario is an amendment with disposition `retired`.

**Evidence and close-out.** §6 carries the scenario evidence table. Before
compaction every scenario has its home path and baseline commit recorded, the
history document keeps ids, homes, amendments and outcome, and tracker plan
links are redirected by the project's document-lifecycle procedure.

**Pilot evaluation.** The pilot plan records, at each milestone and at
closeout, observations for `{{ project.decision_authorities.platform }}` with
no automatic threshold: scenarios changed at plan review; scenario-line changes
in home history without an amendment; amendments and their reasons; behavioural
RED observed per new behaviour; failures at zero retries; step definitions in
total, used once and unused; hash mismatches at transcription; defects found by
scenarios versus other tests; scenario edits forced by the next change after the
pilot.
