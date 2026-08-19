# KON-OP-09 / ASMA-7878 — release notes

> **Date:** 2026-08-19 12:13 CEST
> **Status:** 🟢 Approved
> **Author:** Architect · OP-09 successor seat
> **Category:** report
> **Scope:** `KON-OP-09` Operational diagnostic UI/UX release unit
> **Summary:** Release evidence for the accepted OP-09 current-`master`
> integration and its renewed independent code-review and QA gates.

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
this verdict for `architect` and requires the `release-notes` artifact. The
current-`master` integration has now completed that required new production,
review and QA cycle.

The accepted current production revision is
`a24758714244762170da17e7604718086aac4a8b`. The two later tester commits are
QA-only and do not alter production code:

| Purpose | Revision | Durable result |
| --- | --- | --- |
| Accepted current-master integration | `a24758714244762170da17e7604718086aac4a8b` | resolves the generated contract and production behavior incompatibilities |
| Focused QA behavior test and report | `0d1a9e66aebf8862e9a80ce10b3532a5a033014e` | adds one receipt-absence assertion and round-3 QA evidence |
| QA receipt readback | `df6fa688d7e70ca28a97b14fa07c8180cffeb3d0` | records the renewed durable Kontor QA receipt |

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
| `a24758714244762170da17e7604718086aac4a8b` | integrates current master and clears its typed-contract failures |
| `0d1a9e66aebf8862e9a80ce10b3532a5a033014e` | adds the focused QA-only receipt-absence assertion |
| `df6fa688d7e70ca28a97b14fa07c8180cffeb3d0` | records renewed QA receipt `01a019ce-5478-7592-b8d1-bc63e56f0a3c` |

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
| `code-review-gate` | accepted production head `a247587`; receipt `01a019bd-f9b8-71e0-9542-080700d325e9` | renewed pass |
| `qa-gate` | `docs/evidence/KON-OP-09/QA.md` through `df6fa68`; receipt `01a019ce-5478-7592-b8d1-bc63e56f0a3c` | renewed pass |
| `release-gate` | this `release-notes` artifact | eligible for architect pass after green PR readback |

The accepted QA run reports:

| Check | Result |
| --- | --- |
| Generated API drift | pass |
| Console type check | pass |
| Console component/contract tests | 16 files, 295 passed |
| Browser QA | 4 passed: Project Operations and Teams at desktop and phone widths |
| Rust format and clippy | pass |
| Full workspace verification | pass, including `pilot` and `pilot_live` |

The committed visual evidence has these SHA-256 digests:

- `evidence/ASMA-7878-PROJECT-DESKTOP.png`:
  `de5a98d8bb5506d12413a7700f09967ebb7131726dd9bc009962d2ebb3239ba6`
- `evidence/ASMA-7878-PROJECT-PHONE.png`:
  `46dc03a0a2f6f27babbe71284ae14fdf1e311590fde46888e214abece663abf2`

## Data, compatibility and rollout

This is a console-only release. It adds no database migration, Rust service,
server route or public contract; its TypeScript schema is regenerated from the
merged authoritative contract. The current TeamRun, workspaces, seats, native
sessions and bindings are preserved. OP-09 creates no topology and performs no
direct Jira or runtime mutation.

The release branch is `feat/ASMA-7878-kontor-diagnostic-ui`; PR 44 targets the
`asma-rs-kontor` default branch. Current-master integration is complete, GitHub
reports the PR mergeable, and its Rust and Console checks are green. Jira status
follows the authoritative Kontor task state through typed reconciliation after
the release gate and merge are durable.

## Current-master integration resolution

The prior release rejection receipt `01a0198a-92ab-72b1-b241-030f81104e72`
correctly held the gate when current `master` first exposed contract failures.
The accepted integration `a24758714244762170da17e7604718086aac4a8b`
regenerated `apps/console/src/api/schema.d.ts` from authoritative
`openapi.json`, then implemented every required typed behavior:

- Advisor and Committee invocation selects the exact caller seat binding from
  the server projection and lets the daemon enforce policy.
- An absent consultation receipt renders no fabricated confirmation.
- Completion renders its tagged phase and typed blockers.
- Completion remediation sends the selected closed tagged action instead of a
  free-form reason.

Independent review then passed at that exact production head under receipt
`01a019bd-f9b8-71e0-9542-080700d325e9`. Independent QA passed under receipt
`01a019ce-5478-7592-b8d1-bc63e56f0a3c` after generated drift, type check,
295 component tests, four browser flows, formatting, clippy and the workspace
suite all passed. The old rejection is therefore resolved, not erased.

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
- The merged Completion projection also carries rounds, closeout, wakes and a
  `needs_human` payload that this integration does not yet render. The accepted
  integration replaces the removed `outstanding` field with typed blockers and
  does not claim a broader completion-console expansion.

These limitations are visible in `REVIEW.md`; none weakens the accepted
idempotency, independent-projection, generated-contract, authority-boundary or
desktop/phone evidence used by the prerequisite gates.
