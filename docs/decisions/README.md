# Kontor decisions

This directory records the decisions that govern the public Kontor repository
and the Kontor workflow pack. It is not the decision log for a project that
adopts the pack.

## What belongs here

Record a decision here when it changes one of these:

- the Kontor product boundary, authority model or consistency rules;
- the workflow method, roles, gates, evidence ladder or closeout rules;
- the public workflow pack's vocabulary, contract or publication boundary;
- a repository-wide engineering policy that a later change must follow.

Do **not** record here:

- a consuming project's product or architecture decisions — those stay in that
  project's own decision records;
- a live fact, a status, a plan or a task list — link to the live source
  instead;
- a conversation, preference or unapproved proposal;
- anything that is not class P (public). Never copy people, accounts, costs,
  credentials, host paths, private project data or private links.

## Who may accept

A decision is **Proposed** by anyone. It becomes **Accepted** only by the
authority that owns its scope. The record names that authority. A decision that
reserves a choice to the platform owner is not accepted by a model, a seat, a
committee or a tool result.

## File format

Use one file per decision, named:

```text
yyyy-mm-dd-hh-mm-decision-<slug>.md
```

The file carries:

```markdown
# <Short decision title>

> **Status:** Proposed | Accepted | Superseded | Deprecated | Rejected
> **Date:** yyyy-mm-dd hh:mm <time zone>
> **Authority:** <role or named authority that may accept it>
> **Scope:** <what this decision governs, and what it does not>
> **Supersedes:** <decision id, or "none">
> **Publication class:** P

## Context

What made the decision necessary. Link evidence by path or URL; do not restate
a live fact as prose.

## Decision

The choice, stated as a rule that a later change can follow or cite.

## Consequences

What becomes easier, harder, forbidden or required. Name the decisions and
documents that must change if this one is superseded.

## Alternatives considered

Each real alternative and why it was not chosen.

## Evidence

The exact revisions, commands, reports or readbacks that support the decision.
```

A decision id is qualified by its file: `<file-slug>#DEC-<nn>`. A later record
may supersede an earlier one; it never edits the earlier decision in place.

## Index

Add one row per accepted, superseded or rejected decision. Proposed decisions
may be listed as proposed, but they do not govern until accepted.

| Id | Decision | Status | Authority | Supersedes |
| --- | --- | --- | --- | --- |
| — | No Kontor decision records yet | — | — | — |
