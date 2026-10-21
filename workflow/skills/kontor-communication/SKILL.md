---
name: kontor-communication
description: "Write workflow comments, notices, reviews, fix requests, status messages and handoffs in any destination, including requested Norwegian or bilingual text. Applies to every component on both surfaces; manuals, status documents and READMEs use their authoring skills."
---

# Kontor communication

One writing rule for every Kontor component, including coordinators, delivery
seats, reviewers, desks and integration adapters. Apply it to the tracker, the
forge, chat, dashboards and future connected project-management tools. A
destination does not need its own copy of this rule.

This skill governs communication. The owning workflow still decides authority,
acceptance, transitions and the supported writer; this skill grants none.
Component-specific formats and limits apply alongside this rule. Full manuals,
status documents and READMEs use the project's authoring skills.

## Write for the reader

- Use clear, simple English for a non-native reader. Prefer familiar words,
  short sentences and active verbs. Explain a necessary technical term once.
- Follow a request for Norwegian or bilingual English/Norwegian. For bilingual
  text, use short **EN** and **NO** blocks with the same facts, action and
  certainty. Keep identifiers, commands and evidence links unchanged; share a
  link list instead of copying logs into both languages.
- Full documents keep their authoring contract; a project may default one
  document family to Norwegian.
- Lead with the result, problem or decision. Include only the context needed
  to understand it, and a next action with its owner when action is needed.
  Do not force headings or empty template fields into a short message.
- Remove ceremony, repeated history, internal seat chatter, unsupported claims
  and lists of routine work. Do not praise the process or narrate every tool call.
- Mention only people who need to act. Do not repeat a mention for an unchanged
  state. Include a time zone when a deadline or event time matters; use
  `{{ project.time_zone }}` unless the destination states another.

## Post only useful changes

- Post when there is a new decision, relevant state change, actionable finding,
  question or resolution. An unchanged poll, retry or repeated finding needs
  no new comment. Follow an explicitly requested reporting cadence.
- Read the current state and existing discussion before writing. A successful
  request is not proof that every destination is synchronized. Name any pending,
  failed or unverified part; never report a planned action as completed.
- Check that the facts behind an update are still current. Superseded facts
  must not restore an obsolete status summary. Preserve authorized corrections
  and holds under the owning workflow; report a conflict instead of guessing.
- Use structured status/check fields for machine state. A comment explains the
  result; its wording is not a replacement state machine or proof of approval.
- When supported, update the component's own clearly identified current-status
  comment instead of appending another snapshot. Keep review, decision and
  approval history. Correct a historical claim explicitly; do not rewrite
  another person's words or erase a human decision.
- Reuse the owning workflow's effect identity and posting route. A retry must
  not duplicate a comment, mention or notification. Missing update/deduplication
  support is a capability limitation, not permission to flood a thread.
- Identify automated actions as automation using the destination's supported
  attribution. Using a human account never represents human approval.

## What each message needs

| Message | Keep |
| --- | --- |
| Status or completion | Verified result; remaining work or checks if any; evidence link; next owner only when needed. Distinguish merged, verified and ticket done. |
| Defect or fix request | Location, severity, concrete failing case/impact, requested correction and proof. One problem per finding; link detailed reproduction or logs. |
| Review summary | Verdict, every unresolved blocker in brief, and links to findings/evidence. Mention a fixed finding once with its verified fix. |
| Question or decision request | The missing decision, its consequence, the recommendation and who can answer. Ask once unless the facts change. |
| Sync failure or recovery | What succeeded, the intended state and pending destination, effect/evidence link, and retry/recovery owner. One update when the situation materially changes. |
| Handoff | Current result, exact artifact/evidence and the receiving owner's next action. Put the full history in the linked record. |

Aim for one short paragraph or up to five brief bullets. Routine updates
normally fit within 80 words; a finding normally fits within 120. These are
editing targets, not permission to hide evidence or omit blockers. If more is
needed, keep a short complete summary and link the durable detail. Do not split
one long update into a stream of comments. Preserve required domain fields and
all blocking findings, using brief lines and links when there are many.

## Examples

Use real keys and links when posting; the values below are illustrative.

**Status:** PR #123 merged. {{ project.tracker.project_key }}-1234 is awaiting
deployed verification. The delivery team will check the new version.
[Merge and test evidence](https://example.invalid/evidence).

**Fix request:** `filter.py:42` — blocking: an empty filter returns every tenant's
records. Reject a missing tenant ID before querying. The isolation test fails
with an empty ID; [reproduction](https://example.invalid/evidence).

**Sync failure:** PR #123 merged. {{ project.tracker.project_key }}-1234 should
show the configured state for awaiting verification; the tracker did not accept
the update. [Pending effect](https://example.invalid/evidence).
Recovery owner: the PR desk. Synchronization is pending.

**Bilingual, when requested:**

EN: PR #123 is merged. {{ project.tracker.project_key }}-1234 is awaiting verification in dev.

NO: PR #123 er flettet inn. {{ project.tracker.project_key }}-1234 venter på kontroll i dev-miljøet.

Before sending, check: is this new, accurate, needed by this audience and as
short as its purpose allows?

## Owners linked from here

- PR-specific labels, finding format and posting mechanics:
  [`pr-review/references/comment-format.md`](../pr-review/references/comment-format.md).
- Decisions, facts and history: [`document-organization`](../document-organization/SKILL.md#three-homes-for-information).
