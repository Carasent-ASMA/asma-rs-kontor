# ASMA-8114 operational gaps

## OG-01 — consultation topic is treated as unchecked title material

- Intended Kontor command: invoke the KTHSR operational-completion Committee
  once and resume it through exact idempotency replay.
- Failure class: naming validation and semantic idempotency gap.
- Scope: realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`, project
  `01a0064a-e056-7603-9968-ef64fdaacb75`, epic
  `01a07495-e5e0-7ef2-b285-878ee7fb2bd9`.
- Effect: the Jira key was duplicated inside two CSW titles, and two distinct
  idempotency keys froze the same logical Committee topic.
- Current checkpoint: all three original runs, topology nodes, seats and
  findings are preserved. No direct Paseo rename, archive or database mutation
  has been used.
- Owner: ASMA-8114.
- Status: open until the merged runtime rejects the original bad request,
  refuses semantic duplicates, and completes supported live reconciliation.

## OG-02 — supported ASMA worktree cannot be attached to its Kontor task

- Intended Kontor command: reapply epic KDCN with Jira task `ASMA-8114`, short
  code `KDCN-8114`, execution scope `ASMA-8113`, and the worktree created by
  `asma worktree add ASMA-8114 --mod _tools/asma-rs-kontor`.
- Failure class: adapter contract disagreement.
- Exact refusal: `branch_type_unknown`; Kontor parsed the relative path
  `asma-8114/asma-rs-kontor` as if it were the Git branch.
- Effect: Jira materialization succeeded, but the task cannot yet store its
  worktree, short code or epic execution scope. The covering authorization
  remains revoked and scheduler admission remains blocked.
- Fallback: implementation proceeds only in the already-created, Jira-keyed,
  isolated checkout. The task is not armed and no duplicate topology is created.
- Owner: ASMA-8114.
- Status: open until the fixed deployment accepts the same declarative graph and
  reads back the real branch as
  `feat/ASMA-8114-enforce-canonical-consultation-identity-and-reconcile-duplicate-csws`.

## OG-03 — exact Jira materialization replay returned a plan mismatch

- Intended Kontor command: replay the original KDCN Jira materialization apply
  under its exact idempotency key.
- Failure class: durable materialization replay disagreement.
- Effect: Kontor returned HTTP 409 stating that the durable batch item set
  differs from the requested plan. Readback shows exactly one Jira epic
  (`ASMA-8113`) and one task (`ASMA-8114`); no duplicate Jira issue was created.
- Current checkpoint: the confirmed Jira bindings remain authoritative and the
  original materialization receipt is preserved.
- Owner: ASMA-8114 unless analysis proves a separate source fix is required.
- Status: open; add a regression test and either correct the replay comparison or
  record a precise non-overlapping follow-up before closeout.

