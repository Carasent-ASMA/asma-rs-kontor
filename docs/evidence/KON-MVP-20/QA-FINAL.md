# KON-MVP-20 / ASMA-7764 — corrective QA disposition

**READY-FOR-MERGE-AND-FINAL-COMMITTEE**

The Lead-integrated corrective candidate at committed source anchor
`97791ab1aff72d2dfbaeffaa72b2b631705f4356` addresses the prior typed
`NON_COMPLIANT` findings:

- the daemon no longer composes the production AO family; an AO configuration
  fails closed with a typed unsupported-family error;
- every new Paseo launch carries the frozen role slot's explicit provider,
  model, and optional effort, and the launch is refused if native readback does
  not match;
- the runtime requires the pinned Paseo protocol baseline independently of
  provider/model route selection;
- the corrected archive and phone screenshot hashes are recorded, and the
  invalid untracked Seat B write remains incident evidence only;
- the combined mutation result is 31/31 killed, and the committed-source
  archive verifier, Rust gates, dependency checks, console gates, build, API
  verification, and Playwright 2/2 all pass.

This document does not fabricate the remaining live C4 observation or a final
committee verdict. After this candidate is merged, the existing Orchestrator
must obtain a live provider/model/effort handoff through the existing seats and
reconvene the same read-only committee. Only its typed `COMPLIANT` verdict
authorizes the six Jira terminal transitions. The Orchestrator then closes and
reads back `ASMA-7751`, `ASMA-7759`, `ASMA-7760`, `ASMA-7762`, `ASMA-7821`, and
`ASMA-7854`, reconciles board/Jira/git, and closes the epic. `ASMA-7756` remains
the accepted deferral. Operational Teams, Advisors, configurable Committees,
and Completion Profiles remain a separate Operational epic.
