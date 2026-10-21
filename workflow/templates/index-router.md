# {{ project.name }} documentation index

> Canonical documentation root for the project. Last updated:
> {{ document.last_updated }}. Every routed document has one row and one load
> condition; a document with no row is not routed.

## Start here

| Document | Summary | Status | When to load |
| --- | --- | --- | --- |
| [{{ document.title }}]({{ document.path }}) | {{ document.summary }} | {{ document.status }} | {{ document.load_when }} |
| [{{ document.title }}]({{ document.path }}) | {{ document.summary }} | {{ document.status }} | {{ document.load_when }} |

## Plans

| Document | Summary | Status | When to load |
| --- | --- | --- | --- |
| [{{ document.title }}]({{ document.path }}) | {{ document.summary }} | {{ document.status }} | {{ document.load_when }} |

## Analyses and reports

| Document | Summary | Status | When to load |
| --- | --- | --- | --- |
| [{{ document.title }}]({{ document.path }}) | {{ document.summary }} | {{ document.status }} | {{ document.load_when }} |

## Decisions

| Document | Summary | Status | When to load |
| --- | --- | --- | --- |
| [{{ document.title }}]({{ document.path }}) | {{ document.summary }} | {{ document.status }} | {{ document.load_when }} |

## Router rules

- One row per document; one owner and one status per row.
- The status matches the document's own front matter, not a copied date.
- A completed or superseded document stays routed with its final status and a
  link to its successor.
- Links are relative and stay inside the public tree; a private source is cited
  by short public text, never by a link.
