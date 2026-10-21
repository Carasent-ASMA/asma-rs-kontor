---
name: ubiquitous-language
description: Extract a domain glossary from the current conversation, flag ambiguities and propose canonical terms. Use when the user wants to define domain terms, build a glossary, harden terminology, create a ubiquitous language, or mentions "domain model" or "DDD".
---

# Ubiquitous language

Extract and formalize domain terminology from the current conversation into a
consistent glossary. The project's glossary path is `{{ project.glossary }}`.
When that path is a glossary document, update it in place; otherwise write
`UBIQUITOUS_LANGUAGE.md` in the working directory and tell the operator where it
went.

## Process

1. **Scan the conversation** for domain-relevant nouns, verbs and concepts.
2. **Identify problems**:
   - the same word used for different concepts (ambiguity);
   - different words used for the same concept (synonyms);
   - vague or overloaded terms.
3. **Propose a canonical glossary** with opinionated term choices.
4. **Write the glossary** to the configured path using the format below.
5. **Output a summary** inline in the conversation.

## Output format

```md
# Ubiquitous Language

## Order lifecycle

| Term | Definition | Aliases to avoid |
|------|-----------|-----------------|
| **Order** | A customer's request to purchase one or more items | Purchase, transaction |
| **Invoice** | A request for payment sent to a customer after delivery | Bill, payment request |

## People

| Term | Definition | Aliases to avoid |
|------|-----------|-----------------|
| **Customer** | A person or organization that places orders | Client, buyer, account |
| **User** | An authentication identity in the system | Login, account |

## Relationships

- An **Invoice** belongs to exactly one **Customer**.
- An **Order** produces one or more **Invoices**.

## Example dialogue

> **Dev:** "When a **Customer** places an **Order**, do we create the **Invoice** immediately?"
> **Domain expert:** "No — an **Invoice** is only generated once a **Fulfillment** is confirmed. A single **Order** can produce multiple **Invoices** if items ship in separate **Shipments**."
> **Dev:** "So if a **Shipment** is cancelled before dispatch, no **Invoice** exists for it?"
> **Domain expert:** "Exactly. The **Invoice** lifecycle is tied to the **Fulfillment**, not the **Order**."

## Flagged ambiguities

- "account" was used to mean both **Customer** and **User** — these are distinct concepts: a **Customer** places orders, while a **User** is an authentication identity that may or may not represent a **Customer**.
```

## Rules

- **Be opinionated.** When multiple words exist for the same concept, pick the
  best one and list the others as aliases to avoid.
- **Flag conflicts explicitly.** If a term is used ambiguously in the
  conversation, call it out under "Flagged ambiguities" with a recommendation.
- **Keep definitions tight.** One sentence maximum. Define what it IS, not what
  it does.
- **Show relationships.** Use bold term names and express cardinality where
  obvious.
- **Only include domain terms.** Skip generic programming concepts such as
  array, function or endpoint unless they have domain-specific meaning.
- **Group terms** into multiple tables when natural clusters emerge (subdomain,
  lifecycle or actor). If all terms belong to one cohesive domain, one table is
  fine.
- **Write an example dialogue.** A short conversation, three to five exchanges,
  between a developer and a domain expert that shows how the terms interact and
  clarifies their boundaries.

## Re-running

When invoked again in the same conversation:

1. Read the existing glossary.
2. Incorporate new terms from the subsequent discussion.
3. Update definitions if understanding has evolved.
4. Mark changed entries with "(updated)" and new entries with "(new)".
5. Re-flag any new ambiguities.
6. Rewrite the example dialogue to incorporate the new terms.

## Post-output instruction

After writing the glossary, state:

> I've written or updated the glossary. From this point forward I will use these
> terms consistently. If I drift from this language or you notice a term that
> should be added, let me know.
