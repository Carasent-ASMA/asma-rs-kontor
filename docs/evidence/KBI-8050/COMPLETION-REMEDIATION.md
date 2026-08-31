# KBI-8050 completion remediation

Date: 2026-08-31  
Jira epic/task: ASMA-8049 / ASMA-8050

## Round-one findings addressed

The first independent Committee verdict was `non_compliant`. Delivery had the
identity model and topology naming, but its operational proof exposed two
control-plane gaps:

1. a failed Jira create batch could only be bypassed by a later link batch,
   leaving recovery identity and retry behavior insufficiently strict;
2. a Committee native permission could be answered only through direct Paseo,
   outside Kontor's typed, durable authority surface.

## Remediation shipped

- Schema v74 adds an immutable recovery ledger against the original pending
  create batch. The daemon discovers that batch during an explicit link apply,
  requires an exact item set, persists the recovery before connector reads,
  maps results by ordinal, and confirms the original items and batch.
- Recovery readback requires exact Jira key, project, issue type, parent,
  summary, description and marker. Ordinary link mode remains intentionally
  non-owning for summary/description/marker.
- Jira epic confirmation refuses a different already-confirmed full key.
- Schema v75 adds a durable Committee permission-response effect ledger scoped
  to the exact project, Committee run, logical seat, occupancy generation,
  native filler and runtime request. Only runtime acknowledgement confirms it.
- HTTP, OpenAPI, generated TypeScript, CLI and MCP now expose exact permission
  inspect/respond operations. The narrow `leadership` serve profile admits them
  for persistent LSA/TPM seats without exposing unrelated operator tools.
- Paseo validates runtime generation plus exact Committee-run and SeatBinding
  labels before inspection or response and rereads the request as resolved.
- Non-Git leadership/consultation MCP composition resolves its exclusion target
  before writing, so a refused preflight leaves no partial configuration.

## Replay and uncertainty policy

A confirmed permission response replays from schema v75 without another runtime
call, even after the Committee becomes terminal. A dispatch whose acknowledgement
is unknown remains `dispatching` and fails closed; Kontor never guesses that the
requested decision was applied and never sends it a second time.

The Jira recovery retains the original create receipt, batch, items, requested
keys, ordinals and markers. Replay resolves the same batch and recovery rows;
it does not create a replacement batch or issue.

## Pre-merge verification

The remediation candidate passed the complete repository gate set on
2026-08-31:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- the Rust workspace suites, including 236 daemon loopback cases, all 55
  migration/schema cases, and the contract/e2e tail;
- `cargo audit` and `cargo deny check` under the repository's configured
  advisory, license, ban and source policies;
- frozen pnpm install, generated OpenAPI/TypeScript parity, workspace
  typecheck, and 296/296 console tests;
- `pnpm audit --prod`, with no known production vulnerability; and
- the two deliberate mutation kills recorded in `MUTATION.md`.

The contract canaries read back 150 mapped operations, 149 advertised MCP
tools, and 151 documented OpenAPI operations. Both new Committee permission
routes are explicitly operator-scoped.
