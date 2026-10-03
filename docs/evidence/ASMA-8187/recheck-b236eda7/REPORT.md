# ASMA-8187 — broad recheck at an exact head, from a clean tree

Every row ran against the source tree exactly as `b236eda7` holds it,
serially, on one machine, with the raw log committed beside this report.
The table is generated from those logs by `harness/mkreport.py`; the
tallies are summed from the `test result:` lines the runs printed.

## Results

| Step | Command | Wall clock | Witness | Dirty before / after | Verdict |
| `01-fmt` | `cargo fmt --all -- --check` | 3s | no diff | 0 / 0 | **PASS** |
| `02-compile-default` | `cargo check --workspace --all-targets --locked` | 1s | Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.96s | 0 / 0 | **PASS** |
| `03-compile-feature-edge` | `cargo check -p kontor-store --all-targets --features fault-injection --locked` | 1s | Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.26s | 0 / 0 | **PASS** |
| `04-api` | `cargo test -p kontor-api -- --test-threads=1` | 2s | 37 passed, 0 failed, 4 binaries | 0 / 0 | **PASS** |
| `05-mcp-parity` | `cargo test -p kontor-mcp -- --test-threads=1` | 1s | 74 passed, 0 failed, 4 binaries | 0 / 0 | **PASS** |
| `06-daemon` | `cargo test -p kontor-daemon -- --test-threads=1` | 303s | 685 passed, 0 failed, 10 binaries | 0 / 0 | **PASS** |
| `07-store` | `cargo test -p kontor-store -- --test-threads=1` | 490s | 624 passed, 0 failed, 36 binaries | 0 / 0 | **PASS** |

```text
head   : b236eda75b6d95f96442ad37224030d7de753195
tree   : 498939c3f301ea39c7e7664c197b54459369f9f9
parent : 2f9e20d6e078ed5a4b63eabf57f355b05e964f01  (single parent, no merge)
TOTALS 1420 passed, 0 failed, 54 binaries
```

The daemon suite is 685 where the previous round was 682: the three new
regressions that prove the barrier's bounds are bounds. All five
coherence tests run inside the ordinary `cargo test` invocation.

## Mutants at this same head

| Mutant | Exit | Restoration |
|---|---|---|
| `M-P22b1-receipt-not-unique-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-P22b2-completion-instant-not-frozen-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-P23-upgrade-assumes-coherent-bindings-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-P24-readback-unbound-from-transition-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-P25-unchanged-answers-before-proof-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-CTL-control-pair-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-COH-split-lock-boundary-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-COH-TEAM-hybrid-generation-and-persona-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-HANG-SETTLE-ALWAYS-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-HANG-NO-TIMEOUT-FLAG-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-HANG-PAUSE-UNBOUNDED-b236eda7` | 101 — killed | byte-identical to the named snapshot |
| `M-HANG-WAIT-ASSERTS-b236eda7` | 101 — killed | byte-identical to the named snapshot |

Twelve mutants, twelve kills, every target restored byte-identically.
The four `M-HANG-*` mutants are new and attack the bounds the barrier
regressions depend on. `M-HANG-PAUSE-UNBOUNDED` was expected to time out
rather than die; it was killed in 5.01s instead, because the harness no
longer depends on the code under test to terminate. The expectation and
the result are both recorded in the mutants' MANIFEST.

## Cleanliness, and the gap this replaces

Each step records `git status --porcelain=v1` for the whole repository,
verbatim, both before and after it ran, plus an independent
`git diff --quiet HEAD -- crates/ Cargo.toml Cargo.lock` and the blob id of
`applications.rs` as HEAD holds it against the working tree. The logs are
staged outside the repository while the run is in progress, so the cleanliness
they record is the real thing and not an allowance carved out for the harness's
own output.

Every step in the table shows zero dirty paths before and after.

The previous round's receipts recorded a bare count — "1 path", "2 paths" —
without naming anything, and the judgement that those paths were evidence-only
lived in the report rather than in the evidence. That gap is **not**
reconstructed here: no classification is backfilled onto the old logs, and the
old report's claim stands as it was made, unsupported by its own receipts.

One honest detail about this round. The first execution of this same harness
showed four dirty paths from step 03 onward — the four `m_hang_*.py` mutation
scripts, which were being written while it ran. Those receipts named them, by
path, which is what the remedy asked for. They were then committed and the
whole table re-run from a genuinely clean tree, which is what the table above
reports. The earlier execution is not presented as this one.

## What a green table here does and does not mean

Proven at this head: the workspace compiles on the default feature set and on
the `fault-injection` edge; every suite in `kontor-api`, `kontor-mcp`,
`kontor-daemon` and `kontor-store` passes serially, including
`hosted_seat_autonomy`, the `schema_v1` ladder through `0122`, the
backup/export round trip, the OpenAPI contract and MCP seat parity; the two
projection-coherence regressions hold under a deterministic barrier; and the
three new bound regressions hold, so a stuck worker fails the suite instead of
hanging it.

Not proven here, and not claimed: any live succession, any deployed behaviour,
any credential enrollment, any Kontor gate, and independent verification. Every
test in this lane runs against the fake runtime and synthetic fixtures. The
earlier rejected verification at `7864a3ca` remains immutable FAIL history and
nothing here converts it. No push, merge, publication, Jira transition, gate
verdict or closure action was taken.

## Findings and dissent carried forward, not resolved

* The LSA's finding that `8910ebc6` overwrote twelve immutable carriers stands.
  QA's historical-preservation PASS also stands. Neither withdraws the other;
  the remediation acted on the LSA's finding because a reader reads the working
  tree.
* The malformed `M-CTL` originals remain malformed, labelled, and byte-identical
  to what they always were.
* The first `M-COH` run remains INCONCLUSIVE — a forty-eight minute hang that
  proved nothing, and the reason this round exists.
* The `M-P22b2` raw receipt remains lost and disclosed, not reconstructed.
* `M-HANG-PAUSE-UNBOUNDED` is a timeout by construction and is not counted as a
  kill.
