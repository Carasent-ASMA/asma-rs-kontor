# KON-OP-08 LSA schema and owned-gap checkpoint

> **Date:** 2026-08-19 19:05 CEST  
> **Status:** 🟢 Approved  
> **Author:** LSA / Architect · KON-OP-08  
> **Category:** report  
> **Scope:** ASMA-7877 / KON-OP-08 architecture resume through OP-09 PR 44  
> **Summary:** Records the current OP-09 integration baseline, immutable OP-12
> dependency, OP-14/OP-08 migration reservation, three newly evidenced OP-08
> gaps, and the exact resume checkpoint for the existing builder seat. No
> production migration was renumbered in this checkpoint.

---

## When to load

**Load this document when:**

- resuming the existing OP-08 builder after the 2026-08-19 LSA correction;
- integrating OP-14 / ASMA-7941 and renumbering project-subject authority;
- implementing project discovery, Jira spec pinning/readback or dynamic task
  materialization/admission replay; or
- auditing why OP-08 did not take migration `0042` after OP-12 merged.

**Do not load for:** QNR work, unrelated Kontor epics or direct Paseo recovery.

---

## Immutable repository position

The ASMA Git fetch/readback established:

| Ref | Exact commit |
| --- | --- |
| pre-amendment local OP-08 implementation head | `0f62c51f1a56fe149832b9dcf6b5201e4bc852d2` |
| fetched `origin/master` after OP-09 PR 44 | `4eb60a5b791db6bc299a7431678705728fa2de83` |
| exact OP-09 parent / OP-12 PR 45 merge | `2c4e5e495ae7ad826e389cadb30adba3f615f3ac` |

The local branch reflog confirms the three commits the LSA ordered preserved:

1. `785e458284ab6c6ef27298a9ee6f317254d5f975`
2. `54050674c17ce0a20520fa93a1d06157c0450d3a`
3. `0f62c51f1a56fe149832b9dcf6b5201e4bc852d2`

OP-12's merged tree declares `SCHEMA_VERSION = 41` and includes
`0041_open_questions.sql`. The current OP-08 tree also declares schema 41 but
uses that generation for `0041_project_subject_authority.sql`. A merge before
OP-14 would therefore require one of four dishonest states: steal OP-14's
reserved `0042`, claim OP-08's `0043` before its predecessor exists, drop
OP-12's immutable migration, or leave the authority code without its fresh-DB
schema.

OP-09 advances master from that exact OP-12 parent to
`4eb60a5b791db6bc299a7431678705728fa2de83`. Its scope is console/evidence only
and reports no API-contract or server-migration change. It is therefore the
current integration baseline for this architecture and all non-conflicting
OP-08 work, without changing the three-generation reservation below.

The LSA correction resolves this without rewriting history:

```text
OP-12 merged master                    -> 0041
OP-14 / ASMA-7941 exact future merge  -> 0042
OP-08 after that exact merge          -> 0043_project_subject_authority
```

Accordingly, this checkpoint fetches and verifies OP-09 and its exact OP-12
parent but does not create a partial schema merge commit. Architecture-only and
other non-conflicting work continue from the OP-09 baseline. After OP-14
merges, the builder must integrate the exact then-current master, which must
include OP-09, and move the local authority migration directly to `0043`,
updating every pin in one coherent checkpoint. No `0042` placeholder or
temporary weakened test is allowed.

## Owned gap evidence

### Gap 1 — project discovery

Evidence:

- `tests/contract/fixtures/v1-operation-inventory.txt` has
  `POST /v1/projects:ensure` but neither project read route.
- `kontor-mcp::REGISTRY` consequently has no project list/get commands.
- A client must already know a project id before it can use any project-scoped
  read, which makes the documented MCP-only narrative incomplete.

Owned outcome: add `GET /v1/projects` and `GET
/v1/projects/{project_id}`, stable list/empty-list/not-found contracts and exact
OpenAPI/registry/CLI/MCP parity.

### Gap 2 — Jira spec ownership and honest declared hops

Evidence:

- connector field/workflow routes only list the bundled catalog and its
  `installed` boolean;
- `Services::jira_specs` selects the first bundled pair rather than an
  auditable project pin;
- live Jira offered no direct `DRAFT` (`10237`) to `In Development` (`10214`)
  transition, only the declared intermediate `READY FOR DEVELOPMENT` (`10213`);
  and
- false-success receipt `01a01af4-5198-7b63-a7cd-7dfb8c61fb20` reported
  convergence after the first effect without a fresh observation proving the
  final milestone.

Owned outcome: project-scoped field/workflow install receipts, one compatible
pair pin, exact pin readback, and a reconcile response that reports the first
declared hop as progress until a fresh Jira read proves the final milestone.

The error decision in `0f62c51` remains binding: deterministic Jira
domain/spec/selection refusals are non-retryable `unsupported_capability`,
typed state conflicts are `revision_conflict`, and only a genuinely unavailable
external boundary is retryable.

### Gap 3 — dynamic task materialization and admission replay

Evidence:

- an applied task persists worktree/ticket/module data but is not installed as
  a dynamic runtime scope;
- `PaseoExecutionScope::task_scope` reads a startup `task_scopes` map from
  `runtimes.json`, so a task applied after startup is unknown without config
  editing and restart;
- the adapter already has strict create/adopt, workspace readback and
  one-session-per-run primitives; and
- `Services::live_seat` can select the first child of any historical TeamRun
  without first excluding parked/abandoned terminal runs.

Owned outcome: persist a runtime-neutral task scope during apply, materialize
the exact ticket workspace through the adapter with create/adopt/readback,
then make scheduler replay yield one active TeamRun and exactly one current
attached seat per declared slot. A parked/abandoned prior run is never
re-parented or adopted.

## Required tracer regression

The public-interface tracer is:

```text
daemon starts without a task in runtimes.json
  -> epics:apply persists the new task scope
  -> task materialization creates/adopts and binds the exact native workspace
  -> scheduler plan/start admits the declared team
  -> replay start returns the same active team and seats
```

It must prove:

- one active TeamRun;
- one current AgentRun and one attached SeatBinding per declared slot;
- one exact workspace binding at the applied canonical worktree;
- no config edit, daemon restart or direct Paseo operation;
- an existing parked/abandoned run remains terminal and unattached; and
- replay creates no additional workspace, TeamRun, run or seat.

The declared-hop tracer separately proves:

```text
DRAFT -> fresh READY FOR DEVELOPMENT readback (progressed)
      -> fresh In Development readback (converged)
```

No fresh readback means no convergence receipt.

## Builder resume checkpoint

Use the existing OP-08 builder run and seat. Do not create, replace or rebind
any architect, builder, inspector, tester or verifier seat.

The builder may proceed with project reads, connector-pin contracts,
declared-hop readback and dynamic-scope/materialization tests that do not claim
a new store generation. The store/OpenAPI migration lane stays blocked until
OP-14 / ASMA-7941 merges. At that point:

1. fetch and record the exact new `origin/master` immediately before the
   integration commit;
2. prove it descends from OP-09, contains OP-12 `0041` and contains OP-14
   `0042`;
3. integrate it while preserving `785e458`, `5405067` and `0f62c51`;
4. renumber local authority directly to `0043` with all lineage/pins; and
5. run fresh-v42, populated-v42, restart/replay and full store/OpenAPI parity
   gates before committing or pushing.

No read or mutation of QNR epic
`01a019c0-eee7-72a1-a8a7-7fff1ddce8f3` occurred in this checkpoint.
