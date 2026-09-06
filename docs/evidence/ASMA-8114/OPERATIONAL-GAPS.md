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

## OG-04 — Kontor could not confirm a follow-up message position

- Intended Kontor command: send one follow-up to existing ASMA-8110 AgentRun
  `01a07398-d34a-7b31-a3a6-1547a37e71a6` after its archive suite exposed two
  fixture regressions.
- Failure class: session message delivery confirmation gap.
- Exact result: `kontor_session_message_send` returned HTTP 503 and instructed
  the caller to inspect canonical history; the subsequent timeline read did not
  contain a confirmed position for that message.
- Bounded fallback: `paseo send` delivered the same follow-up to the already
  bound native agent `b600c003-b029-47fc-b5cc-ed120be11a1e`. No new agent,
  workspace or identity was created, and no wider orchestration continued
  through Paseo.
- Current checkpoint: the same ASMA-8110 seat owns its worktree and correction;
  deployment and live state remain untouched.
- Owner: ASMA-8114 for the message-confirmation defect; ASMA-8110 retains its
  implementation scope.
- Status: open until the message-confirmation path is reproduced and assigned a
  non-overlapping correction or shown already corrected by the merged runtime.

## OG-05 — the forge did not require publication identity

- Intended Kontor command: attest every push and pull request, post the
  `asma/publication-identity` GitHub App check, and require it before merge.
- Failure class: deployment/configuration gap after source delivery.
- Exact readback: neither governed repository has
  `config/github-app.json`; recent pull-request heads carry no App check, and
  the active repository rulesets require a pull request but no status check.
- Effect: the ASMA CLI normally attests through Kontor, but direct GitHub use or
  its explicit `--without-attestation` exception can bypass that local gate.
- Correction in this change: the dependency-free
  `asma/publication-branch-title` Actions check independently refuses the exact
  observed defect: noncanonical branch, non-master base, missing title key, or
  different branch/title Jira keys. After merge the repository ruleset will
  require that check. The existing App path remains the stronger confirmation
  of the key against Kontor's durable Jira binding.
- Owner: ASMA-8114 for the Kontor repository. The superproject requires the
  same workflow/ruleset under its own Jira-bound module change.
- Status: open until both rulesets read back the required check; App installation
  remains a separately visible configuration prerequisite rather than a claim
  that the absent credential is live.
