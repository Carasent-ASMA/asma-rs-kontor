---
name: docs-navigation
description: "Find, navigate and assess existing documentation in the configured roots or a repository that links to them. Read-path only; for new document naming or placement use document-organization, and for specialized authoring use the owning skill."
---

# Docs navigation — the read path

The configured documentation roots are `{{ project.documentation_roots }}`.
They are the single source of truth for project knowledge. A repository that
sits beside a root may expose it through a committed relative link; if you are
inside such a repository, the linked documentation tree is the same tree, not a
copy. Never conclude "no docs here" without following that link.

## 1. Entry points, in order

1. **The root's `index.md`** — the project or domain router. Every root and
   every domain folder has one. It lists documents with summary, status and
   when-to-load. Start here, not with grep.
2. **The decision-record register** at
   `{{ project.decision_authorities.decision_records_root }}` — the read-router
   for decisions. Check it before any architectural read or change; match the
   register's Area column.
3. **The glossary** at `{{ project.glossary }}` — the one dictionary for codes,
   roles, records, evidence and status.
4. **The gap ledger** at `{{ project.gap_ledger }}` — known gaps and their
   owners, when the project keeps one.
5. **Living maps and routers** named by the project's own index, referenced by
   name rather than copied.

## 2. Decode a filename before opening it

```text
yyyy-mm-dd-hh-mm-<category>-<slug>.md
2026-05-25-17-37-audit-example-topology-completeness.md
```

- **Timestamp** is the creation date, not precedence. Decide ownership by scope
  and explicit status or supersession, never by which filename is newer.
- **Category tag** tells you the genre (see §3).
- **Exception:** living by-name documents — specs (`spec-<product>-<slug>.md`)
  and patterns (`pattern-<slug>.md`) — have no date prefix; they are contracts,
  always current. The rule is stated once in
  [`document-organization` §1](../document-organization/SKILL.md#1-document-naming-convention).

A project may add a domain and tier layer to its roots. A plan still follows
the owning domain, not the repository that implements it.

## 3. What each category means for a reader

| Folder | Read it as | Trust level |
|--------|-----------|-------------|
| `specs/` | Living contract — how it IS | Authoritative, always current |
| `architecture/` | Design rationale and data model | Authoritative unless Status says otherwise |
| decision-record root | Immutable project decisions | Never contradict an Accepted record |
| `reference/` | Domain or system reference material | Stable |
| `plans/` | Intent — how it WILL be | May be partially done; check Status |
| `analysis/`, `spikes/` | Options weighed at a point in time | Input to a decision, not the decision |
| `audits/`, `reports/` | Snapshot findings | True as of their date only |
| `operations/` | Runbooks, troubleshooting | Follow verbatim |
| `history/` | Compacted summary of finished work | The durable outcome; the verbose original is gone — do not go looking for it |

## 4. Trust the metadata header

Every document opens with `Status`, `Summary` and **When to Load / Do NOT load
for** blocks. Read those first and skip documents whose anti-triggers match your
task. Status meanings: Approved (trust), In Review or Draft (verify),
Superseded (find the successor), Historical (outcome only).

## 5. Resolution order for a question

1. Root `index.md` → follow the when-to-load column.
2. Governing decision record (the register's Area column).
3. Domain `specs/`, then `architecture/`.
4. `history/` for "why is it like this" on finished work.
5. Only then grep the documentation roots for keywords.

## Related skills (authoring, not reading)

- Creating, naming or placing a document:
  [`document-organization`](../document-organization/SKILL.md).
- Writing or changing a decision record:
  [`decision-records`](../decision-records/SKILL.md).
- Retiring a finished plan or analysis into `history/`:
  [`document-organization` §8](../document-organization/SKILL.md#8-document-lifecycle-and-historical-compaction).
