---
name: document-organization
description: "Choose a documentation root, filename, metadata and router updates when creating or reorganizing documents. Not for reading docs, writing their body, multi-domain layout, data-model bundles, or specialized manuals and status reports."
---

# Document organization

Canonical source for documentation naming, folder structure and metadata
conventions across the project's documentation roots. Apply these rules when
creating, reviewing or reorganizing any documentation file.

This skill is the single authority for document filenames, top-level
documentation-root selection, category folders, metadata and router updates.
Document-type skills define document content only; they link these rules
instead of restating them.

The documentation roots are
`{{ project.documentation_roots }}`. Each root is one owning
project, system, domain or discipline. A root is not a global category folder:
do not create a second top-level plan or report folder outside the roots.

---

## 1. Document naming convention

### Format

```text
yyyy-mm-dd-hh-mm-<category-tag>-<descriptive-slug>.md
```

| Component | Rule | Example |
|-----------|------|---------|
| `yyyy-mm-dd-hh-mm-` | Creation timestamp, 24h, zero-padded. Sortable by `ls` and file tree. | `2026-05-25-17-37-` |
| `<category-tag>` | Short category identifier from the [category table](#document-categories) below. | `audit`, `analysis`, `plan` |
| `<descriptive-slug>` | Kebab-case, AI-friendly: describe what the document IS, not generic filler. Max about six words. | `example-system-topology-completeness` |

### Full example

```text
2026-05-25-17-37-audit-example-system-topology-completeness.md
2026-05-26-09-15-plan-example-migration-phase-2.md
2026-05-26-14-00-report-example-implementation-results.md
```

### Rules

- Always use the full timestamp prefix; never omit minutes or use a date-only
  prefix. The living by-name documents below are the one exception.
- Always include a category tag. If unsure, use `analysis`.
- The slug is AI-navigable: a reader should know the topic from the filename.
- No legacy names. Rename an existing document when touching it; do not keep
  old-style names.

### Living by-name documents (the exception)

Documents that are contracts referenced by name keep a stable by-name filename
because other documents and code cite the name itself:

- **Specs** — `spec-<product>-<slug>.md` in `specs/`.
- **Patterns** — `pattern-<slug>.md` in `patterns/`, when the project uses a
  living pattern family.

These are living documents: update them in place, never date-prefix them, and
never archive them to `history/`. All other naming rules still apply.

### Anti-patterns (do not use)

| Wrong | Why |
|-------|-----|
| `my-doc.md` | No date prefix, no category |
| `2026-05-25-doc.md` | Missing time, no category tag, vague slug |
| `readiness-investigation-2026-05-25.md` | Date buried in the middle, no category tag |
| `refactor-plan-v2-final.md` | No date, version in filename (use document status instead) |

---

## 2. Folder structure

### Choose the owning documentation root first

The configured documentation roots are a registry of owning scopes, not
category folders. Do not put an active plan in a repository-root `plan/` or a
global `plans/`. A plan that changes several repositories still belongs to the
root that owns the product or system outcome; repository breadth does not make
it global. Use an existing platform or programme root only when that domain is
genuinely the subject, never as a catch-all.

A single-scope documentation root follows this structure:

```text
<documentation-root>/
├── index.md                       # Master index: every doc with summary + when-to-load
│
├── audits/                        # Completeness, security, compliance, topology audits
├── analysis/                      # Gap analysis, investigations, research findings
├── architecture/                  # Architecture decisions, design docs, component diagrams
├── specs/                         # Living contracts, by-name (spec-<product>-<slug>.md)
├── patterns/                      # Living patterns, by-name (pattern-<slug>.md)
├── plans/                         # Implementation plans, migration plans, roadmaps
├── reports/                       # Result reports, status reports, post-implementation findings
├── spikes/                        # Time-boxed technical spike documents
├── operations/                    # Runbooks, troubleshooting guides, monitoring playbooks
├── history/                       # Compacted summaries of finished work (see §8)
│
└── _stats/                        # Auto-generated statistics (gitignored, never committed)
```

### Document categories

| Category tag | Folder | Use when the document is… |
|--------------|--------|----------------------------|
| `audit` | `audits/` | A completeness, security, compliance or process audit |
| `analysis` | `analysis/` | A gap analysis, investigation, research or technical deep-dive |
| `architecture` | `architecture/` | An architecture decision, design rationale or component spec |
| `spec` | `specs/` | A living specification or contract, by-name and never archived |
| `pattern` | `patterns/` | A living reusable interaction contract referenced by name |
| `plan` | `plans/` | An implementation plan, migration roadmap or phased rollout |
| `report` | `reports/` | A result report, status summary or post-implementation finding |
| `spike` | `spikes/` | A time-boxed technical spike with research questions and findings |
| `runbook` | `operations/` | A procedural runbook, troubleshooting guide or monitoring playbook |
| `history` | `history/` | A compacted summary of finished work (see §8) |

### Rules

- Every dated document goes inside its owning documentation root and then into
  that root's category folder.
- Plans follow the owning domain, not the repositories they edit. A plan that
  genuinely spans more than one bounded context uses the project's own
  cross-domain location, not a global folder.
- New categories are allowed when no existing folder fits; propose one and add
  it to this skill in the same change.
- Implementation-local documents stay with the implementation that owns them
  and follow the same structure.

---

## 3. Document metadata header

Every document starts with a metadata block immediately after the title:

```markdown
# <Title — human-readable, descriptive>

> **Date:** YYYY-MM-DD HH:MM
> **Status:** 🔴 Draft | 🟡 In Review | 🟢 Approved | ⚫ Superseded | 📚 Historical
> **Author:** <name or role; omit when the authoring is automated>
> **Category:** <category-tag>
> **Scope:** <which system, component or module this applies to>
> **Summary:** <1–3 sentences describing what this document contains and why.>

---

## When to Load

**Load this document when:**
- <Specific task or scenario>
- <Specific task or scenario>

**Do NOT load for:** <Anti-triggers; when this document would be noise.>

---
```

### Metadata field rules

| Field | Required | Notes |
|-------|----------|-------|
| `Date` | Yes | ISO format with HH:MM |
| `Status` | Yes | Emoji plus text: Draft, In Review, Approved, Superseded, Historical. A category-specific status vocabulary is fine (a plan may use Completed, In progress and so on). |
| `Author` | Recommended | A name or role. Omit when authoring is automated. |
| `Category` | Yes | Must match the category tag in the filename |
| `Scope` | Yes | System, component or module scope |
| `Summary` | Yes | 1–3 sentences; used to decide whether to read the body |
| `When to Load` | Yes | Specific trigger conditions |
| `Do NOT load for` | Recommended | Prevents loading irrelevant documents |

---

<a id="three-homes-for-information"></a>

## 3.1 Three homes for information

Every statement has exactly one home. Do not store it anywhere else.

| Kind | What it is | Home and rule |
| ---- | ---------- | ------------- |
| **Decision** | What we want | Git, with owner and date. It changes only by a newer decision. |
| **Fact** | What is true now (availability, quota, status) | Read live, or from one state store with a timestamp and expiry. Never store a fact in Markdown or YAML. |
| **History** | What happened | A dated report. Never use it as current truth. |

A document links to the source of a fact instead of copying its value, or shows
it as "as of <time>, from <command>". Every temporary block carries an end
condition a script can check: a date or a named check. A drift check compares
file claims with live readouts and reports stale lines; it never fixes them
silently.

This applies to all seats and documents. Seats also follow it in their messages.
Decision records follow the owning process in
[`decision-records`](../decision-records/SKILL.md).

---

## 4. Index file (`index.md`)

Every documentation root has an `index.md` that serves as its canonical
navigation hub. When a root uses a domain layer, every domain and shared
cross-domain area also has a router. Category folders do not need another index.
A router includes:

1. **Brief overview** — 2–4 sentences about what this documentation covers.
2. **Document table** — every document listed with filename, summary, status
   and when-to-load.
3. **Folder legend** — what each category folder contains.

### Index template

Start a new router from the [index template](assets/index-template.md).

---

## 5. AI context loading rules

### How the AI decides to load a document

When a task matches a document's **When to Load** triggers, the AI must:

1. **Check `index.md` first** — scan the document table for matching triggers.
2. **Read the metadata header** — verify the summary and status before reading
   the full body.
3. **Load the full document** only if the summary confirms relevance.
4. **Skip a Superseded document** — follow the replacement reference instead.

### Writing good "When to Load" triggers

Triggers are specific task scenarios, not vague topics:

| Good trigger | Bad trigger |
|--------------|-------------|
| "Adding a new table through the example migration tool" | "When working on the database" |
| "Debugging a foreign-table connection failure" | "Database issues" |
| "Planning the example runtime migration for db-per-tenant" | "Architecture changes" |

---

## 6. Migration path for existing documents

When touching an existing document that does not follow this convention:

1. **Rename** to `yyyy-mm-dd-hh-mm-<category>-<slug>.md` (use the original date
   if known, otherwise the file modification date).
2. **Add the metadata header** block.
3. **Move** it into the correct category folder.
4. **Update `index.md`** to list the renamed document.
5. **Update all cross-references** in other documents to point to the new path.

Do not keep old names for backward compatibility. Rename when touched. Living
by-name documents are already correct; never rename them to a timestamped form
and never compact them to `history/`.

---

## 7. Skill integration

The project's skill router loads this skill. Module or package adapters are
installed and checked by the project's own setup procedure. Do not add a second
loader or copy this skill into another location by hand.

In an instruction file, add a context trigger row:

```markdown
| Document creation or review | `workflow/skills/document-organization/SKILL.md` |
```

---

## 8. Document lifecycle and historical compaction

Every document has a useful life. When a document's live purpose is over, it
does not linger in the tree — verbose and stale material inflates every future
context load. What happens next depends on the document's class.

### 8.1 Two classes of document

| Class | Categories | End-of-life action |
|-------|------------|--------------------|
| **Point-in-time** — a snapshot of an intent, investigation or moment | `plan`, `analysis`, `spike`, `audit`, `report`, `prompt`, `onboarding`, `worknote` | **Compact to `history/`** (§8.3), then delete the verbose original; git keeps the full text. |
| **Living** — describes the current system or a standing decision | `architecture`, `spec`, `pattern`, `reference`, `glossary`, `runbook`, decision record | **Update in place**, or supersede. Never archive to `history/`. |

Rule of thumb: does the document describe what we intended to do at time T, or
how the system is and what we decided? The first ages into history; the second
is maintained or superseded.

### 8.2 When a point-in-time document becomes eligible

- **`plan`** — at either terminal state: fully complete (status Completed, no
  residual open tasks) or dropped (status Deprecated). A dropped plan is not
  "completed", but it is equally done-with; compact it and state why it was
  dropped and what replaced it. Not eligible: "core complete, residual open",
  In progress, Planned, On Hold.
- **`analysis` or `spike`** — once its decision is captured in a living
  document.
- **`audit` or `report`** — once its findings are remediated or absorbed.
- **General rule** — any point-in-time document that is superseded or abandoned
  is eligible the moment it is dropped; it is pure decision history.

### 8.3 What a historical summary keeps (and drops)

Keep only what helps future work:

- **What we set out to do** — one short paragraph.
- **What shipped** — the outcome, not the task-by-task journey.
- **Key decisions and deviations** — the locked decisions and where reality
  diverged, with the why.
- **Where it lives now** — durable pointers: code paths and the living document
  that is now the source of truth.
- **Lessons and gotchas** — anything that would bite again.
- **Supersession** — if the approach was replaced, say so and link the
  replacement.
- **Approval receipts stay receipts** — a dated sign-off is evidence for the
  scope it reviewed, not a standing rule. The living destination carries only
  the current rule.

Drop: task and phase tables, per-file checklists, line-number citations,
resolved risk registers and rejected alternatives unless a rejection is itself
a reusable lesson. Target a fraction of the original length; if the summary is
not much shorter, it has not been compacted.

### 8.4 Where history lives and how it is named

`history/` is a category folder inside the owning documentation root. The
filename is `yyyy-mm-dd-hh-mm-history-<slug>.md`, dated at compaction time.
Consolidate one historical document per initiative, not one per source file.
Use `Status: 📚 Historical` and add a `Compacted from:` line naming the retired
originals (git-recoverable).

### 8.5 The compaction procedure

1. Confirm every source document is eligible and that any standing contract or
   rule it still carries has been extracted into its living destination first.
   Never compact a document whose rule is not yet living somewhere: retirement
   would silently drop policy.
2. Write the consolidated summary into the correct `history/` folder.
3. **Redirect inbound links semantically** — an operational link targets the
   living replacement; a historical citation targets the history document.
   Repoint tracker or plan descriptions through the owning surface.
4. **Delete the verbose originals** — never keep old copies "for reference";
   git is the archive.
5. Update the root router: drop the retired rows and add a History section with
   the new document.

---

## 9. Open Knowledge Format alignment

Prose documents are not converted into Open Knowledge Format bundles. A data
model, schema, semantic layer, metrics or data-entity vocabulary uses the
project's knowledge-model convention. The rationale and mapping are in
[OKF alignment](references/okf-alignment.md).
