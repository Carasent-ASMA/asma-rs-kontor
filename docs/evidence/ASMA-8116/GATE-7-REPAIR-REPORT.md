Artifact: gate-repair-report-round-7

# ASMA-8116 — repair of F-8116-V15 and F-8116-V16

Rejected candidate: `b8e9549`
Verifier report: `HIGH-VERIFICATION-ROUND-7.md`, commit `e8b2e64`, preserved verbatim

## One cause, two findings

The previous round fixed the symptom in the wrong place. Evidence was hashed
under the key the request was *addressed to*, so an alias read of `ASMA-1`
produced evidence that could not validate a later read of `MOVED-9`. Rather than
correct the evidence, that round asked Jira a second time — which is V15 (an
extra identity read) and, because the binding was already committed by then,
V16 (proof taken after the advance).

**The repair is at the boundary that produced the evidence.**
`JiraConnector::live` now hashes its canonical document under the key it
*observed*, taken from the response's top level, instead of the key it was asked
under. A single alias read therefore returns evidence that is already canonical
for the current key.

Everything else follows from that:

- **No extra read (V15).** The daemon's re-observation is deleted. The one
  response that resolved the alias carried the canonical key and the immutable
  issue id, and its evidence is valid at the address the writes use.
- **Proof before advance (V16).** With the after-the-fact refresh gone, the
  same-issue comparison in `decide_jira_identity` is the only proof, and it runs
  *before* `reconcile_confirmed_jira_key` commits anything. A mismatch returns
  `Stop(DifferentIssue)` with the old binding untouched and no occurrence spent.

Preserved deliberately: **human-move protection** — every mutable field a caller
guards against (status, assignee, update token, body) is still inside the hashed
document, so only alias drift is removed; **revision/authority**, **immutable
UUID**, **external issue id**, **anti-rebind**; and **no numeric inference** —
nothing is derived from the shape of either key.

## Regressions

- **`an_epic_same_issue_rename_never_addresses_the_superseded_key`** — retains
  the non-vacuous write assertion (write list not empty, some write addresses
  `/rest/api/3/issue/MOVED-9/`, none names `ASMA-1`) and now also asserts the
  read budget *at the boundary it reaches*: a second pass with nothing to
  reconcile performs exactly as many issue reads as the renaming pass.
- **`an_epic_key_naming_another_immutable_issue_is_refused_before_the_binding_moves`**
  — replaces the insufficient refusal test. The same single response names a
  different immutable issue, and it asserts the old binding is unchanged, the
  contested key resolves to nothing, **no rename occurrence is spent**, and no
  Jira effect is emitted. The occurrence assertion is what makes it fail if the
  binding advanced before the proof completed.

## Mutations — three seeded, three killed, restored

| # | Mutation | Test | Result |
| --- | --- | --- | --- |
| M16a | Reinstate the extra identity observation after the rename | `an_epic_same_issue_rename_never_addresses_the_superseded_key` | **killed** |
| M16b | Neuter the same-issue comparison, so any response passes proof | `an_epic_key_naming_another_immutable_issue_is_refused_before_the_binding_moves` | **killed** |
| M16c | Hash evidence under the requested alias again | `an_epic_same_issue_rename_never_addresses_the_superseded_key` | **killed** |

M16a first survived when aimed at
`the_resident_reconciler_follows_a_same_issue_rename_through_the_connector`:
that fixture blocks before the mutated line, so it cannot observe the extra
read. It was re-aimed at the fixture that reaches the native write boundary,
where the read budget is actually exercised. The survival is recorded rather
than the mutant being quietly dropped.

## Gates — isolated `CARGO_TARGET_DIR` throughout

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy` (4 crates, `-D warnings`) | clean |
| `cargo test -p kontor-store --test jira_materialization` | ok, 30/30 |
| `cargo test -p kontor-jira` | ok, 9 + 19 |
| `cargo test -p kontor-api` | ok, 23 + 5 + OpenAPI 3 |
| `cargo test -p kontor-daemon --lib` | ok, 81/81 |
| `cargo test -p kontor-daemon --test loopback_api jira` | ok, 9 (1 predeclared ignored) |
| identity/rename targets (6) | ok |

## Pre-existing failure, re-confirmed not ours

`an_epic_placeholder_body_is_typed_reported_and_repairable` fails with
`503 unavailable — the configured native Jira connector could not answer`. It
fails identically **with this round's connector hash change reverted**, in the
same clean target, so it is not caused by this repair. It failed the same way at
`b8e9549` and `560db2c`. Recorded, unattributed, outside V15/V16 scope.

## Preserved

Every earlier repair, legacy `JiraItemCode`/token/hash behaviour, ASMA-8117
ownership, OQ-002/OQ-004/OQ-005 and migration `0095` ordering. No audit, merge,
deploy, Jira or topology action.
