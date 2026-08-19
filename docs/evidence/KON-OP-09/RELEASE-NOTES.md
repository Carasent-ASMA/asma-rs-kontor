# KON-OP-09 / ASMA-7878 — release notes

> **Date:** 2026-08-19 12:13 CEST
> **Status:** 🟢 Approved
> **Author:** Architect · OP-09 successor seat
> **Category:** report
> **Scope:** `KON-OP-09` Operational diagnostic UI/UX release unit
> **Summary:** Release evidence for the accepted OP-09 console revision, its
> independent code-review and QA gates, and the bounded limitations carried
> into the integrated OP-10 proof.

---

## When to Load

**Load this document when:**

- evaluating or replaying the `KON-OP-09` `release-gate`;
- reconciling `ASMA-7878` with the Kontor task projection; or
- consuming OP-09 as an OP-10 integration dependency.

**Do NOT load for:** redesigning the Operational console or reopening findings
already closed by the accepted remediation.

---

## Release verdict

**PASS for `release-gate` evaluation.** The frozen `code@1` profile reserves
this verdict for `architect` and requires the `release-notes` artifact.

The accepted production revision is
`ffeffc3f23844228437cf3f27ece216e10489da2`. The two later commits are
evidence-only and do not alter production code:

| Purpose | Revision | Durable result |
| --- | --- | --- |
| Accepted production remediation | `ffeffc3f23844228437cf3f27ece216e10489da2` | closes idempotency replay and independent-projection findings |
| Independent round-3 code review | `4f3242b9526a3512d2b83453958d16cba2fa624f` | passed |
| Independent round-2 QA | `3f4ec94454e5182476e1c3acba04077feae475bf` | passed; `qa-report@ffeffc3:3f4ec94` |

The accepted branch range is rooted at seed
`3d2dfca293f1cb252c8a14d92f9f07cb26ddb324`. Its release history preserves the
earlier gate rounds rather than rewriting them:

| Revision | Disposition |
| --- | --- |
| `4211bb0b13a2dd252fd850b4aa3184697afe95a1` | initial OP-09 head; code review rejected two blockers |
| `47948b6882b87f7d26270a5e569815e1018253c9` | clears the two code-review blockers |
| `7cf08a4f2ec99027041f47b32c158f2887e6df55` | round-2 code review passed |
| `efd4670e83ae3ce4fb5fdedda5771506086bc492` | QA rejection retained as historical evidence |
| `ffeffc3f23844228437cf3f27ece216e10489da2` | clears both QA acceptance findings |
| `4f3242b9526a3512d2b83453958d16cba2fa624f` | round-3 code review passed |
| `3f4ec94454e5182476e1c3acba04077feae475bf` | round-2 QA passed |

## Released behavior

- The existing console gains a Project Operations surface for capacity,
  logical/native topology, Project Core Team, Quick sessions and promotion,
  Advisors, Committees, Completion Profiles and current epic completion.
- Delivery Teams retain their existing route and identity while receiving the
  clarified user-facing label. Core Team, Delivery Teams and consultation
  definitions remain separate concepts and lifecycles.
- The browser remains a thin client of the authenticated `/v1` contract. Wire
  types alias generated OpenAPI schemas, controlled codes use server-owned help,
  and no console source connects directly to Paseo, Jira or AgentsRoom.
- Promotion preview uses the contract's body-less `POST`, with a transport test
  that pins both path and verb.
- Console mutations hold one idempotency key per unchanged request intent,
  rotate the key when the intent changes and release it after a confirmed
  receipt.
- Core Team and Completion projections render independently from their catalog
  siblings, so one refused read no longer erases successful server evidence.
- Desktop and phone layouts expose the same seven sections. Code help is
  available through pointer hover, keyboard focus and click/touch, and unknown
  codes remain visibly unknown.

## Durable gate evidence

Kontor project `01a0064a-e056-7603-9968-ef64fdaacb75`, task
`01a0074f-6731-7873-87ac-dd424fe61623`, TeamRun
`01a010ba-4b34-7b72-8533-b735eb8b6627`:

| Gate | Evidence | Result |
| --- | --- | --- |
| `code-review-gate` | `docs/evidence/KON-OP-09/REVIEW.md` at `4f3242b` | passed at accepted production head `ffeffc3` |
| `qa-gate` | `docs/evidence/KON-OP-09/QA.md` at `3f4ec94`; receipt `01a01978-638c-77e3-b190-4e1decaa582f`, sequence `2` | passed |
| `release-gate` | this `release-notes` artifact | recommended passed by the authorized architect successor |

The accepted QA run reports:

| Check | Result |
| --- | --- |
| Generated API drift | pass |
| Console type check | pass |
| Console component/contract tests | 16 files, 290 passed |
| Browser QA | 4 passed: Project Operations and Delivery Teams at desktop and phone widths |
| Rust format and clippy | pass |
| Full workspace verification | pass, including `pilot` and `pilot_live` |

The committed visual evidence has these SHA-256 digests:

- `evidence/ASMA-7878-PROJECT-DESKTOP.png`:
  `2905a8fc4fc257af080f550e49ded0a9e6cf1b9a3d41cb8a48cbf0b92e56d418`
- `evidence/ASMA-7878-PROJECT-PHONE.png`:
  `bbaba81a138e013efc60bf35b1cd29df6105c07298c1d29a09980fa13f53b0b1`

## Data, compatibility and rollout

This is a console-only release. It adds no database migration, Rust service,
server route, public contract or generated schema. The current TeamRun,
workspaces, seats, native sessions and bindings are preserved. OP-09 creates no
topology and performs no direct Jira or runtime mutation.

The release branch is `feat/ASMA-7878-kontor-diagnostic-ui`; its target is the
`asma-rs-kontor` default branch through the repository's reviewed pull-request
flow. Jira status follows the authoritative Kontor task state through typed
ticket reconciliation after the release gate is durable.

## Deliberate limitations

- The `release()` half of the idempotency-key hook is not mutation-pinned by a
  focused component test. Independent review verified the implementation and
  classified this as a non-blocking coverage gap.
- The console does not yet expose Core Team seat materialization or the
  Advisor/Committee settle and finding-record operations. OP-10 owns the
  integrated live proof and must not infer these operations from display state.
- The horizontally scrolling topology table does not yet expose an explicit
  focusable labelled scroll region. The responsive browser evidence is green,
  but this keyboard affordance remains a recorded non-blocking follow-up.

These limitations are visible in `REVIEW.md`; none weakens the accepted
idempotency, independent-projection, generated-contract, authority-boundary or
desktop/phone evidence used by the prerequisite gates.
