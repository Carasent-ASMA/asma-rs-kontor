# ASMA-8110 high-verification report: remediation re-verification

Date: 2026-09-06
Artifact: `high-verification-report`
Task: `ASMA-8110` / `01a07391-328e-74a3-a808-e7b5775c8438`
Phase: `high-verification`
TeamRun: `01a07398-b8d2-7363-8dcc-e92c061deffa`
Candidate: `5743d7ac014166ba3045803a68c211534ce98ea0`
Handoff evidence: `e47128f47ab2806fd0b791c19b55f64d881fefa8`
Status: **rejected — further implementation changes required**

## Verdict

The remediation closes the original route-time TeamRun defect and its named
happy-path regressions pass, but candidate `5743d7a` still does not satisfy the
amended high-verification contract.

The authoritative archive command fails at online lockfile regeneration on the
exact frozen tree. Two temporary adversarial tests also prove that a malformed
`"result": null` binding is still downgraded to legacy and that the new
post-transaction diagnostic decoration masks a cross-task source-receipt
refusal as an already-consumed route. The required concurrent test does not
actually synchronize both requests before either route commits, and the
amended scope's deployment instructions still name schema/migration 89 while
the candidate and deployed realm are at 90.

No production source was changed during verification. No daemon was started or
stopped, no deployment or recovery was invoked, and no task, workflow, Jira,
runtime, topology, TeamRun, AgentRun, seat, or native-session state was mutated.

## Candidate and handoff boundary

- Implement turn ordinal 3 is durably settled with `high-change`, evidence hash
  `9b987aced783c5386feb0ebb69cb79a4cbade88ade59257a4a7effa03fc85097`,
  and terminal timeline position `4:2796`.
- The handoff freezes code commit
  `5743d7ac014166ba3045803a68c211534ce98ea0`; evidence commit `e47128f` changes
  only `HIGH-CHANGE-RECORD.md` on top of it.
- A scratch repository created from `git archive 5743d7a` had tree hash
  `03390e559e8a5d3715992d337765f9d5bd56bf61`, exactly equal to
  `5743d7a^{tree}`. All candidate results below ran from that tree.
- After the handoff, the shared branch was rebased again to `ed15e21`. Its tree
  is byte-identical to current `origin/master` (`bf71bc2`) and differs from the
  frozen handoff only in unrelated ASMA-8102 files. It was not substituted for
  the exact candidate during verification.

The previous rejection is preserved in Git at
`3800cb27654c16dc075b4edf8955fd6b34b404d1`; this file supersedes its working
tree report for the remediation verification while retaining that immutable
history.

## Findings

### F-8110-R1 — high: the authoritative archive gate is still red

`python3 scripts/verify-tree.py --mode archive` was run with registry access
from the scratch Git repository whose committed tree exactly equals
`5743d7a^{tree}`. It exited 1 during its first gate:

```text
Cargo.lock regeneration differs byte-for-byte from the committed lockfile
```

Current online resolution changes only:

```diff
-libflate 2.3.1
+libflate 2.3.2

-ureq 3.4.0
+ureq 3.4.1

-ureq-proto 0.6.1
+ureq-proto 0.6.2
```

The implementation receipt's earlier online equality is not reproducible at
verification time. The authoritative command therefore reached none of format,
clippy, workspace tests, audit, deny, pnpm install, typecheck, Vitest, or the
production dependency audit.

The implementation also reported an inherited workspace failure. An
independent exact test reproduces it:
`no_tool_names_a_store_a_database_or_a_migration` rejects the three ASMA-8101
publication arguments named `repository`. Git history confirms those arguments
already exist at candidate base `e4bb5fb`, and the ASMA-8110 delta adds no such
argument. The baseline attribution is sound, but the amended scope explicitly
requires complete successful archive output; an inherited failure is not a
passing gate and still prevents all later stages from carrying evidence.

Required correction: refresh or durably pin the lock according to project
policy, resolve or formally disposition the publication-vocabulary baseline,
and run the complete authoritative command successfully on a new exact SHA.

### F-8110-R2 — high: `result: null` is treated as an absent legacy binding

The amended scope allows legacy comparison only when a result binding is truly
absent and requires malformed or partial bindings to fail closed
(`HIGH-SCOPE-RECORD.md:169-173,292-297,436-438`). The implementation comment
likewise says a stored payload with no `result` at all is legacy. The code does
more: `bound_gate_record_result` returns `None` when the member is either absent
**or null**:

```rust
if payload.get("result").is_none_or(serde_json::Value::is_null) {
    return Ok(None);
}
```

(`crates/kontor-store/src/repository.rs:11732-11737`.)

A temporary QA case rewrote a valid gate-recording payload to contain
`"result": null`, recomputed its payload hash, and otherwise reused the
candidate's invalid-binding test. The expected refusal failed: the endpoint
returned HTTP 200 with `applied: "created"` and wrote the recovery route. The
probe was then removed and the scratch tree returned to the exact candidate.

This is the same integrity failure class as original F-8110-03: a malformed
present result can authorize the caller-selected legacy comparison path.

Required correction: only an absent member may return `None`; a present null or
non-object value must flow through strict parsing and refuse. Retain the new
null case beside the hash, partial-object, mismatched-binding and absent-member
cases.

### F-8110-R3 — high: route decoration masks an invalid source receipt

On **every** error from `recover_gate_rejection_with_intent`, the service now
looks up a route first by source receipt and then by caller-selected evaluation.
If either exists it discards the store error and returns
`already_routed_refusal` (`crates/kontor-daemon/src/applications.rs:25031-25072`).
The code does not prove that the discarded error was the route-uniqueness
conflict.

A temporary QA case first created a valid route, then submitted a fresh key and
a valid `RecordGateVerdict` receipt whose immutable target belonged to another
task while retaining the already-routed gate/sequence. The store correctly
rejected the cross-task source, but the service replaced that refusal with:

```text
409 revision_conflict
at: command-receipts/{the-valid-route-receipt}
```

The probe expected the scope's cross-identity refusal with no unrelated receipt
disclosure and failed. It was then removed and the exact candidate tree was
restored.

This violates `HIGH-SCOPE-RECORD.md:180-185`, which distinguishes wrong receipt
target/intent from an already-consumed source and requires cross-identity
requests to retain their typed invalid/not-found response.

Required correction: decorate only the repository's exact already-routed
conflict. Preserve every other error unchanged. Add post-route cases for a
cross-task target, wrong canonical gate/intent, non-verdict receipt and invalid
exact binding so caller-selected evaluation identity cannot mask source
validation.

### F-8110-R4 — medium: the required concurrency interleaving is not proved

`concurrent_fresh_recovery_keys_name_the_single_original_route_receipt` starts
two futures with `tokio::join!`, but it has no barrier between route pre-check
and route transaction. The process-wide store mutex allows the winner to finish
before the loser performs its pre-check. The implementation record confirms
that this is what occurred and that the original mutation survived.

The added second half is useful: it sequentially presents a different valid
legacy receipt for the same routed evaluation and reaches the post-transaction
decoration path. It is not the amended scope's required synchronization of two
fresh keys before either commit (`HIGH-SCOPE-RECORD.md:298-302`). It also did not
catch F-8110-R3 because the alternate receipt is a valid source.

Required correction: add a deterministic test barrier or a store-level
equivalent that proves both pre-checks observe no route before either route
transaction commits, while retaining the evaluation-identity collision test.

### F-8110-R5 — medium: authoritative deployment instructions still name v89

The implementation correctly renumbered the route migration to
`0090_gate_rejection_routes.sql` and set `SCHEMA_VERSION` to 90 after current
master claimed v89 for publication attestations. The high-change record explains
that integration decision. The amended high-scope record was not corrected:

- `HIGH-SCOPE-RECORD.md:190` still requires migration 0089;
- `HIGH-SCOPE-RECORD.md:406-408` still instructs delivery to require schema 89
  and migration 0089 exactly once.

The same-realm read-only census now observes schema 90 and the route table, so
that protected delivery check cannot succeed as written. Correct the scope's
file and schema references before any later delivery/readback relies on them.

## Verified remediation behavior

Static review and unmodified focused tests support these corrections:

- every route now stores an immutable route-time `team_run_id`, and fence
  evaluation compares settled turns to that id instead of recomputing the
  latest TeamRun;
- TeamRun resolution occurs only in transactions that actually write a route,
  preserving the receiptless evaluation-append contract;
- valid exact bindings fail on bad hashes, partial objects and mismatched
  workflow/sequence bindings; a genuinely absent result member remains
  recoverable through full legacy intent/evaluation comparison;
- the official wrong-input, same-key, restart, route-uniqueness, later-TeamRun,
  stale-artifact, migration and snapshot cases pass;
- the recovery HTTP/MCP/CLI surface remains admin-only and mechanically aligned.

These passing cases do not cure the three gate/code failures above.

## Test record for exact candidate `5743d7a`

| Command or probe | Result |
|---|---|
| `python3 scripts/verify-tree.py --mode archive` | **failed**: current online lock regeneration differs (`libflate`, `ureq`, `ureq-proto`) |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed |
| `cargo test --locked -p kontor-daemon --test loopback_api rejection` | 6 passed |
| exact `concurrent_fresh_recovery_keys_name_the_single_original_route_receipt` | 1 passed |
| exact `a_phase_advancing_gate_replays_after_revision_change_and_restart` | 1 passed |
| `cargo test --locked -p kontor-store --test repository_roundtrip rejection` | 2 passed |
| exact route migration/snapshot test | 1 passed |
| `cargo test --locked -p kontor-store --test schema_v1` | 57 passed |
| `cargo test --locked -p kontor-core --test domain_state command_kind` | 1 passed |
| `cargo test --locked -p kontor-mcp` | 63 passed |
| `cargo test --locked -p kontor-cli` | 22 passed |
| `cargo test --locked -p kontor-tests-contract --test mcp_parity` | 12 passed |
| exact `no_tool_names_a_store_a_database_or_a_migration` | **failed**, matching the reported inherited ASMA-8101 diagnostic |
| temporary `result: null` invalid-binding probe | **failed**: recovery unexpectedly returned 200/created |
| temporary post-route cross-task source probe | **failed**: invalid source was rewritten to an already-routed 409 and disclosed the route receipt |

The archive command stopped at lock regeneration, so this turn does not claim a
full workspace, audit, deny, pnpm, typecheck, Vitest or production-audit result
for the candidate. The focused commands were run separately after that failure.

## Same-realm read-only census

The realm changed outside this verification turn after the prior report:

- schema version is 90; `task_gate_rejection_routes` is present;
- `PRAGMA integrity_check` is `ok`; `PRAGMA foreign_key_check` returns zero rows;
- one route exists, recovering ASMA-8110's prior `high-verification-gate`
  rejection from workflow revision 5 to `high-implementation@6`, bound to this
  TeamRun;
- ASMA-8110 is `ready` and its active workflow is at `high-implementation@6`;
- ASMA-8100 remains `done@5` with its active workflow at `final-review@5`;
- no route consumes protected ASMA-8100 receipt
  `01a07373-0b66-7b93-905f-c2a21bee494f`.

These are observations only. This turn did not cause any of those state changes.

## Open questions

None. The settled implement turn, frozen candidate, later rebase, integrated
tree and same-realm state are each evidenced directly. The amended scope
resolves OQ-8110-01 through OQ-8110-03; this verification makes no unsupported
choice among alternatives.

## Exit criteria for another verification turn

1. Correct F-8110-R2 and F-8110-R3 and retain adversarial regressions for both.
2. Satisfy the deterministic concurrency requirement in F-8110-R4.
3. Correct the authoritative v89/v90 scope and deployment references.
4. Produce a new exact candidate whose online lock regeneration is identical,
   whose workspace baseline is green or formally dispositioned by the scope
   owner, and whose complete archive gate reaches exit 0.
5. Re-run high verification from that exact committed tree. Do not mutate
   ASMA-8100 or treat its terminal historical receipt as consumed.
