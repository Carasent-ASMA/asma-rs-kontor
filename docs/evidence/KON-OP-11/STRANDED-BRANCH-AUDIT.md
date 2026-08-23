# KON-OP-11 stranded branch audit

Date: 2026-08-23
Compared against: `origin/master` at `68ce415`

## Result

No unique production behavior from a completed Operational ticket remains only
on an old remote branch.

| Branch or artifact | Disposition |
| --- | --- |
| `feat/ASMA-7869-operational-hardening-and-quota-routing-plan` | Startup serving was recovered in PR #74; quota routing was merged and extended in PRs #71, #82, #84, #88 and #89. |
| `feat/ASMA-7874-kontor-advisors-committees` | Advisor and Committee behavior is present on master through PRs #37-#39 and later consultation fixes. Its missing documents are intermediate/rejected checkpoint records, not current release evidence. |
| `feat/ASMA-7876-kontor-jira-policy-cutover` | The authority implementation and evidence landed in PR #99. The current ASMA-8015 branch supersedes the unimplemented Jira half. |
| `feat/ASMA-7880-mutation-security-closeout` | Rejected. Its evidence claims red or unrun gates passed and accepts missing native Jira ownership. No commit from it is merged. |
| `feat/ASMA-7882-claude-usage-polling` | The implementation landed through PR #89 and was subsequently extended on master. |
| `crates/kontor-teams/tests/committee_cardinality.rs` | Not rescued. Current `kontor-core/tests/consultation_specs.rs`, `kontor-profiles/tests/consultation_presets.rs` and `kontor-teams/tests/team_contract.rs` cover the same cardinality and bound properties at the current boundaries. |
| KON-OP-06 evidence | The three previously stranded documents landed in PR #98. |

The parked QNR-v2 branches are outside this merge. Their epic remains parked,
and no gitlink-only commit or non-production plan edit is represented here as
Operational delivery.

## Jira truth correction

ASMA-7880 was moved from Closed back to Draft because the old closeout did not
satisfy its acceptance contract. ASMA-7951 is the superseded predecessor of
ASMA-7952 and must be dispositioned as such rather than counted as unfinished
production code. The Operational epic remains open until ASMA-7880 has a valid
independent final verdict.
