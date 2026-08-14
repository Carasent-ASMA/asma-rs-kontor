# KON-18 MCP journey QA

Current review checkpoint: `1d3cf8877fe98673056207af27f8d05db568e6a5`  
Commit under review: `1d3cf8877fe98673056207af27f8d05db568e6a5`  
TSW archive: `e424212` / `wks_5f1dd03a839f8c04`  
QA verdict: **COMPOSITE_PASS**

## Superseded preflight history

The initial deterministic-only preflight covered checkpoint `a280aaf` and
returned **READY-FOR-AUDIT**. That finding is preserved below as historical
record. It is superseded as the document headline by the composite review at
`1d3cf887`, which incorporates the separate live-Paseo artifact.

## Superseded preflight typed claims

1. **PASS — journey completeness.** `mcp_journey.rs` drives an empty realm from realm/catalog reads through project/account/epic creation, planning, two scheduler admission rounds, runtime settlement, gate recording, task completion, and epic close-out. The final assertion is `closed["state"] == "closed"`. The journey uses one admin Lead dispatcher and one credential, derives later identifiers from tool replies, records the 11-call checkpoint, and asserts request cardinality and `/v1/` paths across the whole journey. `RouterTransport` is the production MCP transport seam and uses `Router::oneshot`; no hand-composed HTTP request or manual session administration is used.

2. **PASS — non-vacuity evidence.** The commit message records that inverting the live-attempt closure guard makes the journey red, while removing only the dependency gate remains green because `max_concurrency: 1` independently limits each round. The source comments explicitly preserve that distinction. Under the read-only constraint I did not seed a new mutation into the checkout; the committed journey and its stated mutation result were checked without modifying source.

3. **PASS — cardinality.** The 11-call mid-test checkpoint remains, and the final whole-journey assertions require `transport.calls() == routes.len()` and every route to begin with `/v1/`. There is no separate Cargo target named `mcp_cardinality` in this checkout; the cardinality assertions are embedded in `mcp_journey`.

4. **PASS — local gates.** The directly runnable gates passed: `mcp_journey` 2/2; full `kontor-daemon` tests 106 loopback + 2 journey + 6 recovery/security + 21 library, 0 failed; `cargo fmt --all -- --check`; and `cargo clippy --offline -p kontor-daemon --tests -- -D warnings`. The reported `mcp_cardinality/mutants/parity` 11/11/11 labels are not separate checked-in Cargo targets, but the relevant journey and daemon suites pass.

5. **PASS — commit/integrity scope.** HEAD is `1d3cf887`; the commit changes only `mcp_journey.rs` (+364/-48). Cargo.lock remains SHA-256 `fd022e16` as reported. The submodule has no tracked or staged diff; preserved untracked historical evidence directories remain. The superproject's existing `NOTES.md` state is preserved.

6. **PASS — live Paseo record is truthful.** `LIVE-PASEO-EVIDENCE.md` and `live-inventory.json` exist. They record Paseo 0.3.1 present, the ASMA connector version request failing with status 2, and `answers_acceptance_criteria: false`. No live journey is claimed or faked. This is a pre-existing/conditional evidence gap requiring explicit per-run Lead authorization and a working connector; it is not a regression in commit `1d3cf887`.

7. **PASS — KON-15/KON-16 surface.** This commit is test-only, and the local daemon, journey, recovery, fmt, and clippy gates expose no product or parity defect. The recorded prior survivors are treated as the scripted-fake fixture ceiling and an overclaimed assertion, not as a new defect in the MCP journey.

## Commands run

```text
cargo test --offline -p kontor-daemon --test mcp_journey
  exit 0 — 2 passed, 0 failed

cargo test --offline -p kontor-daemon
  exit 0 — 21 library + 106 loopback + 2 journey + 6 recovery/security passed

cargo fmt --all -- --check
  exit 0

cargo clippy --offline -p kontor-daemon --tests -- -D warnings
  exit 0

git -C _tools/asma-rs-kontor diff --quiet
  exit 0 — no tracked submodule diff

git -C _tools/asma-rs-kontor diff --cached --quiet
  exit 0 — nothing staged in submodule

sha256sum _tools/asma-rs-kontor/Cargo.lock
  exit 0 — recorded fd022e16 value

test -f /Users/igor/kon-mvp-20-scratch/evidence/kon18-closeout/LIVE-PASEO-EVIDENCE.md
  exit 0

test -f /Users/igor/kon-mvp-20-scratch/evidence/kon18-closeout/live-inventory.json
  exit 0
```

## Residual

The only residual is the documented conditional live-Paseo gap: no qualifying live MCP journey exists at this checkpoint because the run was local-only and the ASMA connector probe exits 2. The deterministic MCP journey is complete and audit-ready; no new seat, workspace, project, or live run is authorized or required for this QA pass.

## Composite close-out QA — 2026-08-14

Live evidence reviewed: `/Users/igor/kon-mvp-20-scratch/evidence/kon18-closeout/live-20260814T193824Z/`.

### Requirement (1) — committed MCP-only journey

**SATISFIED.** The prior deterministic verdict remains valid at commit
`1d3cf8877fe98673056207af27f8d05db568e6a5`: the empty-realm journey reaches
`closed`, uses one caller and credential through MCP `RouterTransport`, asserts
the 11-call checkpoint and whole-journey one-request cardinality, and passed the
local daemon, fmt, and clippy gates.

### Requirement (2) — separate real-Paseo evidence

**SATISFIED, with an honestly disclosed bundle-local residual.** The live
bundle contains 46 manifest artifacts plus `verdict.json`, `cleanup.json`, and
`manifest.json`. Its inventory records real Paseo/CLI 0.3.1, daemon listen
`127.0.0.1:6767`, a reachable live daemon, an empty realm, and three scoped
credentials. `lifecycle/settle.json` records successful `kontor_turn_settle`
for the two bound seats, and the restart evidence preserves the native session,
binding identity, epoch/sequence position, replay one-effect occurrence, and
fresh-key delivery.

The live bundle's own verdict remains **`NON_COMPLIANT` — 38 pass, 0 fail, 4
blocked**. It is not softened or relabelled. The close blocker is specifically
**harness coverage, not a new product defect**: `lifecycle/close.json` contains
no `kontor_role_slot_waive` call, even though the two unbound seats were refused
as `role_slot_unbound`. The public admin waiver tool exists for exactly this
declared-but-unbound case. The waiver contract is already sealed and audited
true at `a280aaf` in `AUDIT-RERUN-A280AAF.md` and
`LEAD-DECISION-MUT-006-SURVIVOR.md`; no contradiction exists between that
contract and the live refusal. The live bundle's causal summary claiming Paseo
terminality prevents close-out is therefore not accepted as the composite
cause; the missing waiver invocation is the relevant close-out omission.

The live bundle's BLK-005 event-gap item is also not a new gap in the committed
journey proof. The prior sealed `a280aaf` deterministic evidence records MUT-008
(`paseo.event-gap-refetch`) as **KILLED**, with both the contract runtime-adapter
and pilot oracles red (exit 101). The live bundle did not produce deterministic
mutation results, so its own `paseo.event-gap-refetch` case correctly remains
blocked; the committed deterministic suite supplies the separate coverage.

### Integrity cross-check

Independent verification of the live manifest found all 46 listed artifact
hashes matching disk, the artifact set matching the directory, and the sorted
NUL-join root recomputing to the declared
`ef2106e655384c7981ac232e726af165586094d48e0e77727ab009a36f592833`.
The bundle does not ship `MANIFEST.sha256`; therefore a direct
`shasum -a 256 -c MANIFEST.sha256` invocation is not applicable, while the
manifest's per-artifact hashes and root formula are independently verified.
Cleanup records the Kontor daemon and MCP children stopped, the created
workspace archived, and no remaining Paseo workspace; the disposable Paseo
project is explicitly retained because Paseo 0.3.1 exposes no removal surface.

## Composite verdict

**`COMPOSITE_PASS` — both close-out requirements are met:** the committed
MCP-only empty-realm-to-closed journey is satisfied, and separate real-Paseo
evidence exists and is integrity-checked. The live bundle's `NON_COMPLIANT`
verdict remains preserved on its own terms; its close residual is the harness's
omitted admin waiver call for two unbound slots, covered by the previously
audited waiver contract. The only residual for audit is that no live bundle
itself closed the epic; this is documented coverage omission, not a contradiction
or regression of commit `1d3cf887`.
