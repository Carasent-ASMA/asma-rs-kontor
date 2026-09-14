# ASMA-8119 operational gaps

This report is additive. It records every bounded fallback or refusal observed
during the ASMA-8049 recovery and does not replace the preserved task,
TeamRun, AgentRun, SeatBinding, topology, or native-runtime evidence.

## OG-8119-01 — pull requests 215 and 216 bypassed Kontor publication

- Intended Kontor commands: immediately re-read the Kontor publication subject
  and GitHub pull request, then use `kontor_publication_preview`,
  `kontor_publication_attest`, and `kontor_publication_merge` against the exact
  reviewed head.
- Failure class: operator governance bypass. No Kontor transport failure was
  proved before either direct GitHub merge, so the fallback was not justified by
  the control-plane fallback rule.
- Scope: realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`, project
  `01a0064a-e056-7603-9968-ef64fdaacb75`, epic
  `01a0539a-51c9-7301-9bd7-26c09167b23e` (`ASMA-8049`).
- Exact fallback and effect: PR #215 was reviewed at head
  `04941eb9c260c76dd1573b09d88e564d29bc4728` and direct `gh` operations marked
  it ready and squash-merged it as
  `6355b85027caea030d037bed4cf538c20ddea0e4`. PR #216 was reviewed at exact
  head `972b5d9404af7bce95fbbbecc82a700665e5d5c0`; direct
  `gh pr merge 216 --squash` produced exact merge
  `f78d041e80042417e0d9a059449eb85737571797`.
- Current checkpoint: ASMA-8119 was created by the repository-supported
  `asma worktree add ASMA-8119 --mod _tools/asma-rs-kontor` mechanism from exact
  `f78d041e80042417e0d9a059449eb85737571797`. No deployment, Jira write,
  topology write, or runtime mutation was performed by these publication
  fallbacks.
- Control: before every later publication write, re-read both Kontor and GitHub
  immediately and use Kontor preview/attest/merge when supported. Direct GitHub
  merge is permitted only after a newly observed Kontor transport failure is
  recorded here with its exact effect.
- Owner: ASMA-8119.
- Status: bypass recorded; prevention control active for all subsequent pull
  requests in this recovery.

## OG-8119-02 — canonical ASMA-8118 timeline read returned unavailable

- Intended Kontor command: read the canonical timeline for AgentRun
  `01a09509-5f19-7060-84cf-62d994bc6217` to reconcile settlement key
  `01a09f94-5878-7a76-a67b-dc20a422c69b`.
- Failure class: Kontor runtime-adapter transport/reconciliation refusal.
- Exact refusal: `kontor_session_timeline_get` returned HTTP 503 with code
  `unavailable`, stating that the session runtime could not be reached and that
  nothing changed.
- Scope: task `01a07722-c3e9-72d0-b372-9353c3d98797` (`ASMA-8118`), TeamRun
  `01a09509-5f17-71c2-99d4-841b5e2fb3a0`, AgentRun
  `01a09509-5f19-7060-84cf-62d994bc6217`, active topology SeatBinding
  `01a09509-b790-7b82-a67c-e5d7733e1fff`, AgentRun runtime binding
  `01a09509-5f19-7060-84cf-62e282ee0305` at generation 1, native agent
  `ccc353b6-d28f-4715-9bfb-981afccb6aad`, workspace
  `wks_5ec1eb84e5d2782c`.
- Superseded observation: an earlier bounded note interpreted a partial direct
  Paseo projection as canonical epoch 7 message sequence 178 and response
  sequence 385. That interpretation is not authoritative correlation evidence
  and must not be used to settle a turn.
- Authoritative complete readback: the Paseo 0.8.0 canonical timeline is epoch
  2, sequences 1 through 385, with `next=null`. Its user messages are sequences
  1 and 144. The exact approved strings are `message_id="null for every
  event"` and `native_event_id="null for every event"`; therefore neither
  historical user message is uniquely or server-verifiably correlatable to the
  historical dispatch record. The approved blocker is
  `runtime_proof_unavailable`, `settlement_attempted=false`, and the timeline
  identifies Paseo `0.8.0`.
- Additive identity correction: revision 21 preserves the historically
  mislabeled addendum `readback.seat_binding_id` as the runtime binding
  `01a09509-5f19-7060-84cf-62e282ee0305` and adds no
  `readback.runtime_binding_id`. Its authoritative
  `asma_8118_binding_identity_correction_20260914.exact_identity` distinguishes
  topology SeatBinding `01a09509-b790-7b82-a67c-e5d7733e1fff` from runtime
  binding `01a09509-5f19-7060-84cf-62e282ee0305`, generation 1, AgentRun
  `01a09509-5f19-7060-84cf-62d994bc6217` revision 6. The historical addendum
  retains report SHA
  `3f667be8feac65ef1e8331fa872966cf6868d173e8405921b931749168df1ee8`;
  the root and identity-correction SHA are
  `0ad932926ae6813bd134468b53986c61339bf45de41b9aec237441edf512009c`.
- Bounded effect: historical backfill is refused. No settlement was attempted,
  no message was sent, and no runtime or control-plane state changed. The
  source-only candidate can establish only a new server-generated,
  identity-bound post-fix challenge and exact terminal response; it does not
  invent proof for either historical message.
- Current checkpoint: the original task, TeamRun, AgentRun, topology
  SeatBinding, runtime binding, workspace, native agent, message, and
  settlement key remain authoritative.
- Owner: ASMA-8119 source correction under the ASMA-8049 recovery.
- Status: the source-only adapter/control-surface candidate is regression-tested.
  It remains open pending governed publication and a later ASMA-8120 certified
  deployment/runtime verification; no live challenge or settlement success is
  claimed.

## OG-8119-03 — hosted-seat title conflict is wider than the host workspace

- Intended Kontor command: claim the unowned native into the Core Team SWE seat
  in its existing ECP.
- Failure class: adapter conflict-scope refusal.
- Exact refusal: hosted-seat claim scans the whole Paseo project, so a delivery
  seat titled `SWE` in a sibling TSW is treated as a conflict even though it is
  outside the claim's host workspace.
- Effect: the correct unowned native cannot be claimed. No replacement seat,
  agent, workspace, or topology node has been created.
- Current checkpoint: foreign-ownership, claimant-identity, and exact-host
  checks remain required; only title-conflict scope is under correction.
- Owner: ASMA-8119 source correction under the ASMA-8049 recovery.
- Status: the workspace-scoped source correction and focused Paseo contract
  regression are complete locally. Governed publication and later ASMA-8120
  deployment/runtime verification remain; no live claim is asserted.

## OG-8119-04 — live ASMA-8119 binding cannot recover its module cwd

- Intended Kontor command: preview and apply container recovery for the existing
  ASMA-8119 TSW while retaining its exact workspace and native identities, then
  read back the exact corrected cwd.
- Failure class: container-recovery contract refusal for a still-live binding.
- Scope: task `01a07722-c3ea-7300-9215-6f55309848f2` (`ASMA-8119`), TeamRun
  `01a09f49-0bbd-7402-a0c8-4882dbdfedc3`, scope AgentRun
  `01a09f49-0bbd-7402-a0c8-4893914653c8`, implementation AgentRun
  `01a09f49-7cd9-7132-a069-2211565e935f`, scope SeatBinding
  `01a09f49-5916-7750-9da0-11a9b1d839da`, implementation SeatBinding
  `01a09f49-5943-7d90-8764-56c91ab50069`, topology node
  `01a09f49-0bc3-7e83-a339-5b2590f92c24`, native workspace
  `wks_a177fd8f877b54d5`.
- Exact refusal: the TSW is rooted at
  `/Users/igor/carasent/asma-modules/.worktrees/feat/ASMA-8049-jira-key-integration`,
  where `_tools/asma-rs-kontor` is absent; current recovery refuses immediately
  because the persisted native container still exists.
- Current checkpoint: the dirty TSW and all original TeamRun, AgentRun,
  SeatBinding, topology, workspace, and native-agent identities are preserved.
  The supported isolated module worktree is separate; no checkout was copied
  into the TSW and no state was edited directly.
- Source conclusion: Paseo 0.8.0 exposes no identity-preserving workspace or
  agent cwd relocation primitive and no per-turn execution-root acknowledgment.
  Kontor cannot truthfully project a changed cwd from a separately stored path,
  and prepending a `cd` instruction to a prompt would be neither a native cwd
  mutation nor exact readback. Therefore this source candidate implements no
  execution-root row or adapter claim and invents no cwd proof.
- Exact missing control surface: a future
  `kontor_team_run_execution_root_preview` /
  `kontor_team_run_execution_root_apply` pair on the existing TeamRun route
  must validate, without writing during preview, one clean ASMA-managed catalog
  module worktree for project
  `01a0064a-e056-7603-9968-ef64fdaacb75`, task
  `01a07722-c3ea-7300-9215-6f55309848f2` (`ASMA-8119`), module
  `_tools/asma-rs-kontor`, TeamRun
  `01a09f49-0bbd-7402-a0c8-4882dbdfedc3`, and topology node
  `01a09f49-0bc3-7e83-a339-5b2590f92c24`. Apply must consume the exact preview
  hash and an idempotency key, store the execution root separately from native
  cwd, and obtain an upstream-supported same-native execution-root
  acknowledgment before reporting success.
- Required fences: fresh expected task revision (last supplied value 2), fresh
  expected TeamRun and topology-node revisions, every preserved AgentRun and
  topology SeatBinding ID plus its freshly read current revision, every
  AgentRun runtime-binding ID/generation/native ID, workspace
  `wks_a177fd8f877b54d5`, the unchanged native cwd, target canonical Git common
  directory/module/Jira-key/cleanliness proof, and one CAS covering the whole
  identity set. Mutable revisions are write-time CAS inputs only; they must not
  replace the stable IDs in the stored authority.
- Owner and revision fence: the Paseo upstream runtime/API owner must first
  provide the identity-preserving execution-root primitive and exact readback;
  the Kontor Core runtime-adapter/control-surface owner then implements and
  reviews the exact preview/apply pair. The restored ASMA-8049 LSA must re-read
  every revision and use Kontor publication preview/attestation for that
  reviewed source SHA.
  ASMA-8120 exclusively owns installing that exact attested revision and any
  deployment or migration effect.
- Status: closed for this candidate as impossible to implement truthfully on
  Paseo 0.8.0; the missing upstream primitive and typed Kontor operation are the
  explicit later checkpoint.

## OG-8119-05 — unclaimed recovery native crossed the read authority boundary

- Intended control: the restored LSA SeatBinding, not this source-recovery
  native, owns Kontor reads and later publication preview/attestation.
- Exact fallback: before that authority correction, this unclaimed native made
  three `kontor_memory_search` calls. All were read-only. The first exact query
  was refused as invalid; the second/listing read and the third bounded `8118`
  search returned approved document data.
- Effect: no Kontor write route was called; no credential was read or disclosed;
  no task, TeamRun, AgentRun, SeatBinding, topology, runtime, gate, deployment,
  Jira, or publication state changed. No inference is claimed beyond the exact
  returned document.
- Control: all Kontor access from this native stopped immediately after the
  authority correction. This worktree may produce a local commit only; the
  restored LSA must re-read the publication subject and use Kontor publication
  preview/attestation before any push or pull-request write.
- Owner: restored ASMA-8049 LSA for publication; ASMA-8120 for deployment.
- Status: fallback recorded; local source/test work only.
