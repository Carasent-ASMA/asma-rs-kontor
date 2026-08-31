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

## Live-database migration rehearsal

A consistent online backup of the serving schema-73 ASMA realm was started on
isolated loopback port 17717 through the remediation daemon's real startup
path. It migrated atomically to schema 75, retained realm
`01a00649-9ee6-73e0-ba1b-6a6c35cfd065`, project
`01a0064a-e056-7603-9968-ef64fdaacb75`, the original planned create batch and
the confirmed fallback batch, and created no recovery or permission rows.

`PRAGMA integrity_check` returned `ok`; `PRAGMA foreign_key_check` returned no
rows. After a clean stop, the migrated copy produced a verified 15,126,528-byte
snapshot and restarted idempotently at schema 75 with the same project
identity. The copy intentionally did not contact Jira or Paseo: exact Jira
recovery is exercised against the authoritative realm only after deployment.

## Live in-place recovery

Date: 2026-08-31T22:24:37Z — realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`,
project `01a0064a-e056-7603-9968-ef64fdaacb75`, epic
`01a0539a-51c9-7301-9bd7-26c09167b23e`, task
`01a0539a-51ca-7ae1-9ce8-e40965efe0f2`.

Under the deployed schema-75 build, one `jira_materialization_apply` in link
mode, replayed under the original idempotency key
`kbi-jira-recover-original-20260831-v1`, adopted the **original** create batch
`01a0539a-a6b8-75d0-b3d0-cf653d396d5b` and confirmed both of its original
items:

- ordinal 0, epic `ASMA-8049`, readback hash
  `de0cf915f646a6381e1b6c3185a3d47798e5ee14390f2f0dd52ae5b84c7be92f`;
- ordinal 1, task `ASMA-8050`, readback hash
  `86032edcc40bd67efc75efa94fa7aca7dfb4873824cbb7a1e52048cc6ff57f80`.

Both items keep `intent_kind = create`, so the confirmation lands on the failed
batch rather than on a replacement. Readback ran in strict recover mode:
project, issue kind, parent, summary, description and marker were each required
to match the original create plan.

Nothing was duplicated. No new batch was planned, no new Jira issue was
created, `jira_materialization_recoveries` still holds exactly two rows under
the single receipt `01a05688-1b6e-7792-8b62-ecf653732442`, `jira_epic_bindings`
still names `ASMA-8049`, and the task still carries exactly one `connector.jira`
link to `ASMA-8050`. The earlier fallback link batch
`01a0539c-839d-7003-8ef8-9ff438d276f2` remains confirmed at its original
instant. The recovery command receipt moved `intent_persisted` → `confirmed`
and activation recorded receipt `01a059ec-ac61-7f03-9058-a3c5f5d77aa8`.

Two preconditions and one refusal belong to the record:

- The two Jira descriptions were reconciled to the Kontor projection
  (`Kontor epic <epic id>: <epic name>` / `Kontor task <task id>: <task title>`)
  through a bounded Jira fallback, because Kontor exposes no
  content-reconciliation surface: `ticket_reconcile_plan` answers
  `unsupported_capability`. Jira history preserves the prior text. Recorded as
  Kontor memory `operational-gap-asma-8049-jira-content-reconciliation-20260831`
  and `operational-gap-asma-8049-recovery-replay-identity-20260831`.
- A replay under a *new* idempotency key is refused with `the pending create
  item already names another recovery`; recovery replays only under the
  original key. That is deliberate, but no Kontor read surface exposes that
  replay identity.
- The first recovery attempt, made before the descriptions were reconciled, was
  refused before any Jira write and left the batch `planned`.
