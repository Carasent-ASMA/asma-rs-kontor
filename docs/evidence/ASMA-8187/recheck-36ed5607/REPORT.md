# ASMA-8187 — broad recheck at an exact head

Every row below ran against the source tree exactly as `36ed5607` holds it, on
one machine, serially, with the raw log committed beside this report. Nothing
here is retyped: the table is generated from those logs, and the suite tallies
are summed from the `test result:` lines the runs actually printed.

This report exists because an audit found the previous round's broad rechecks
asserted without committed receipts. The remedy is not a better assertion; it
is the receipts.

## Results

| Step | Command | Wall clock | Witness | Verdict |
|---|---|---|---|---|
| `01-fmt` | `cargo fmt --all -- --check` | 3s | no diff | **PASS** |
| `02-compile-default` | `cargo check --workspace --all-targets --locked` | 55s | Finished `dev` profile [unoptimized + debuginfo] target(s) in 55.53s | **PASS** |
| `03-compile-feature-edge` | `cargo check -p kontor-store --all-targets --features fault-injection --locked` | 12s | Finished `dev` profile [unoptimized + debuginfo] target(s) in 12.78s | **PASS** |
| `04-api` | `cargo test -p kontor-api -- --test-threads=1` | 10s | 37 passed, 0 failed, across 4 binaries | **PASS** |
| `05-mcp-parity` | `cargo test -p kontor-mcp -- --test-threads=1` | 16s | 74 passed, 0 failed, across 4 binaries | **PASS** |
| `06-daemon` | `cargo test -p kontor-daemon -- --test-threads=1` | 310s | 682 passed, 0 failed, across 10 binaries | **PASS** |
| `07-store` | `cargo test -p kontor-store -- --test-threads=1` | 443s | 624 passed, 0 failed, across 36 binaries | **PASS** |

```text
head   : 36ed5607753bb749a3a3f12f785fe632c3092650
tree   : d802479a71bdefa6dc2f0bee4332ee9237fc546d
parent : 8910ebc603b5d013861d0793c6f8384f75eaca9d  (single parent, no merge)
source dirty at run time: none -- every compiled path is exactly as HEAD holds it
```

The 'dirty' counts inside each log are docs/evidence paths: the harness's own 
output directory, and MANIFEST.md edited between steps 05 and 06.

Across the four test steps: **1417 passed, 0 failed, 54 test binaries**,
every one run with `--test-threads=1`. The two projection-coherence
regressions ran inside `06-daemon` and both passed; the dead-code warning
for `epic_control_seat_binding` appears in none of the seven logs, because
the helper is gone.

## Mutants re-run at this same head

Eight, through the committed harness, each one bounded and each one
restoring its target byte-identically to the snapshot the log names.

| Mutant | Exit | Restoration |
|---|---|---|
| `M-P22b1-receipt-not-unique-36ed5607` | 101 — killed | byte-identical to the named snapshot |
| `M-P22b2-completion-instant-not-frozen-36ed5607` | 101 — killed | byte-identical to the named snapshot |
| `M-P23-upgrade-assumes-coherent-bindings-36ed5607` | 101 — killed | byte-identical to the named snapshot |
| `M-P24-readback-unbound-from-transition-36ed5607` | 101 — killed | byte-identical to the named snapshot |
| `M-P25-unchanged-answers-before-proof-36ed5607` | 101 — killed | byte-identical to the named snapshot |
| `M-CTL-control-pair-36ed5607` | 101 — killed | byte-identical to the named snapshot |
| `M-COH-split-lock-boundary-36ed5607` | 101 — killed | byte-identical to the named snapshot |
| `M-COH-TEAM-hybrid-generation-and-persona-36ed5607` | 101 — killed | byte-identical to the named snapshot |

`M-CTL-control-pair-36ed5607` is a pair, not a single verdict: the
stopped-schema test passes under the mutant (blind) and the current-realm
control fails (the half that bites). Its non-zero exit is that second half.

`M-COH-TEAM-hybrid-generation-and-persona-36ed5607` is the one that matters
for the core-team projection: the escape guard is deliberately silenced, so
the kill comes from the persona/native agreement assertion alone —
`occupancy_generation` `left: 2`, `right: 1` against a
`native_seat.generation: 1`. A seat that existed at no instant.

## What a green table here does and does not mean

Proven at this head: the workspace compiles on the default feature set and on
the `fault-injection` edge; every suite in `kontor-api`, `kontor-mcp`,
`kontor-daemon` and `kontor-store` passes serially, including
`hosted_seat_autonomy`, the `schema_v1` migration ladder through `0122`, the
backup/export round trip, the OpenAPI contract and the MCP seat parity; and the
two projection-coherence regressions hold under a deterministic barrier.

Not proven here, and not claimed: any live succession, any deployed behaviour,
any credential enrollment, any Kontor gate, and independent verification. Every
test in this lane runs against the fake runtime and synthetic fixtures. The
earlier rejected verification at `7864a3ca` remains immutable FAIL history and
nothing in this report converts it. No push, merge, publication, Jira
transition, gate verdict or closure action was taken.

## The disagreement this report does not resolve

An independent audit found that `8910ebc6` overwrote twelve immutable
historical carriers in the working tree. Independent QA judged historical
preservation a PASS, on the grounds that Git ancestry retains every byte.

Both readings are on the record and neither is withdrawn. The remediation acted
on the audit's finding — the twelve are restored, the refreshed runs moved to
distinct paths, both generations enumerated — because a reader reads the
working tree, and an evidence set whose current state contradicts its own
immutability claim is not repaired by the claim being technically recoverable
from history. QA's point stands on its own terms: nothing was destroyed.

Recorded as a disagreement rather than merged into a single verdict.
