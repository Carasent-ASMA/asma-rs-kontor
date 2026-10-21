# Open Knowledge Format (OKF) alignment

Load this when asking why the documentation conventions in
[document-organization](../SKILL.md) look the way they do, or how they relate
to OKF. Placement and naming rules stay in the skill.

The conventions are deliberately OKF-shaped, so the documentation corpus is
interoperable with the
[Open Knowledge Format](https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing)
without adopting it as a separate system. OKF is markdown files plus structured
front matter, a per-folder `index.md`, cross-links and a small set of queryable
fields with one required `type` discriminator. The conventions already do all
of that:

| OKF idea | Our equivalent |
|----------|----------------|
| Markdown and structured front matter | Metadata header (skill §3) |
| `type` (the one required field) | **`Category`** (skill §3): the type discriminator |
| Per-folder `index.md` | Index routers (skill §4) |
| `description` and queryable fields | `Summary`, `Scope`, `When to Load` (skill §3) |
| Cross-links between concepts | Relative `.md` links, enforced everywhere |

**The one rule to internalize:** every document carries a **`Category`**, the
single required discriminator that lets an agent classify any document from
front matter alone. It must match the filename's category tag (skill §1).

**What the conventions do not do:** they do not convert prose documents such as
decisions, specs or analyses into OKF bundles. OKF's `type` taxonomy and graph
model are designed for data and semantic assets — tables, datasets, metrics and
join paths — not prose decisions.
