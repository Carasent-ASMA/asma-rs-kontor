# Tickets desk

How `DESK • TICKETS` holds small non-epic work under the Kontor workflow. The
[Paseo-direct native placement reference](paseo-direct-native-placement.md#desks-and-lead-workspaces-2026-10-24)
owns desk placement, names and lead workspaces; this procedure owns routing, the
lead's tracker duty, the wall protocol and staffing. Names come from the
project's naming card and its desk layout amendment.

## Contents

- [Routing](#routing)
- [Tracker duty](#tracker-duty)
- [Wall protocol](#wall-protocol)
- [Team staffing](#team-staffing)
- [Testing and mutation](#testing-and-mutation)

## Routing

- `DESK • TICKETS` is the desk for small non-epic work. The retired QSW desk is
  not a routing target, and QSW remains only a legacy governed term.
- An accepted ticket gets exactly one `TSW • <ticket key> • <topic>` workspace
  in the desk.
- A non-epic issue whose parent epic is active (its tracker status category is
  not Done) belongs to that epic and is delivered in a `TSW` of the epic's `ESW`.
- `DESK • TICKETS` takes an issue only when it has no parent epic or its parent
  epic is Done.
- A ticket on `DESK • TICKETS` follows the same `TSW` workflow and
  trusted-delivery gates as an epic `TSW`: its team's checks, tests and mutation
  pass ([testing and mutation](#testing-and-mutation)), and the PR check.

## Tracker duty

The ticket lead owns its issues' tracker state.

- A lead may create `{{ project.tracker.project_key }}` issues when the
  work needs them, following the
  [task content contract](../../tracker-linked-creation/SKILL.md#task-content-contract).
- It must move the issues it owns through their workflow states as work
  progresses, through the supported tracker writer.
- A state change follows observed progress, never a plan alone.
- A lead does not move another lead's issues.

## Wall protocol

A ticket lead stops that ticket's delivery and asks the operator when it
observes any of these triggers:

1. acceptance needs a change to a module, public contract, schema or shared
   library that the ticket's Delivery context does not name;
2. an accepted decision record or architecture rule would need a deviation;
3. the same blocking review finding returns after two fix rounds;
4. the work splits into two or more independently acceptable outcomes.

The lead then:

- comments on the tracker issue with the trigger, evidence, options and its
  recommendation;
- records a hold with reason, owner, lifts-when and a review date at most 30 days
  ahead;
- asks the operator one question with these options:
  - **A — widen this ticket and refactor now:** rewrite the issue's scope and
    continue in the same `TSW` through the same gates.
  - **B — split into new tickets:** create the new issues, link them, narrow
    the current one and continue it.
  - **C — plan an epic:** hand off to a new LSA in the desk's `LSA` workspace.
    That LSA plans and creates the epic and re-parents the ticket, and the
    ticket continues in a `TSW` of the new `ESW` ([routing](#routing)). The old
    `TSW` retires after the handoff readback.

Other tickets continue. The operator's answer is recorded verbatim on the
issue, and the lead continues without further prompts. Without an answer by
the review date, the lead raises attention again; the ticket is never dropped.

## Team staffing

Choose the team per ticket by its
[triage tier](../../ticket-triage-tiers/SKILL.md).

- **Tickets basic:** `SWE` and `QA` seats in the ticket's `TSW`, then the PR
  check on the pull request. Used for Chore and simple Standard tickets.
- **Advanced:** the project's existing standard, high-stakes or UX team, as the
  ticket needs. Any high-stakes trigger always selects the high-stakes team.
- Tickets basic is declared like the existing teams: a team id with role-slot
  model bindings in `{{ project.fleet }}` and an entry wherever Kontor names its
  teams. It appears in Kontor's Teams view.
- The lead moves a ticket to a heavier team when evidence shows it needs one.

## Testing and mutation

Testing and a mutation pass are mandatory for every task and every ticket,
whatever its size or team, including documentation and configuration work.

- Code gets tests plus seeded mutants per the
  [mutation-testing](../../mutation-testing/SKILL.md) skill.
- Documentation and configuration get the checks that guard them (links,
  mirror, schema, route resolution, attribution) plus seeded defects those
  checks must catch.
- A task or ticket with a surviving mutant is not done.
