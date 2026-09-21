# ASMA-8203 — independent review of release 118 at `44663e10`

Read-only inspection of the deployed source, root's qualification receipts and
the prior ASMA-8203 findings. **No production effects, no message replay, no
gate, Jira, topology, schedule or watchdog action.** Nothing in this document is
my implementation: the release, its deployment and its tests are root's, and are
attributed as such throughout.

## Subject

| | |
|---|---|
| Deployed source | `44663e10e063ad3d11c0c84ca6a87d680d75e835` — "ASMA-8203 Recover uncertain delivery and scoped leadership tools (#264)" |
| Qualified source | `1b524140e83fc9ce48bdf9cab46d8553ecba5c05` — "fix(ASMA-8234): inject scoped leadership MCP for hosted seats" |
| Tree (both) | `c5ea3516f75d2144e2c33224d016d877f602207c` |
| Deployment bundle | `deployments/ASMA-8188-20260921T100129Z-44663e10/deployment.json` |
| Qualification | `repair-worktrees/20260921/release118-qualification/qualification.json` |
| Live schema | 118, `live: true`, reconciliation open, scheduling open |

### The two commit hashes are not a discrepancy

`qualification.json` records a different `source_commit` from `deployment.json`,
which reads alarmingly at a glance. It is not a gap: both commits resolve to the
**identical tree** `c5ea3516…`, which is exactly the tree hash the qualification
recorded, and `git diff 1b524140 44663e10` is empty. `44663e10` is the PR #264
merge carrying `1b524140`'s content. **The qualification covers the deployed
bytes.** I checked this first because accepting it on the strength of the label
alone would have been the easy mistake.

## What root implemented (attribution: root)

Migrations present at `44663e10`: `0115_atomic_local_command_results.sql`,
`0116_hosted_seat_role_personas.sql`,
`0117_runtime_message_issuance_boundaries.sql`,
`0118_runtime_message_delivery_proofs.sql`; `SCHEMA_VERSION = 118`.

`0118` adds bounded, resumable evidence for legacy sends that have no pre-send
boundary. Its own header states the governing property — *"No native dispatch is
authorized by either table. The historical issuance, observed upper bound, epoch
and anchor remain immutable; each page is CAS"* — and it enforces immutability in
the schema rather than in prose, with forward-only and no-delete triggers on both
the proof table and its steps.

The refusal wording confirms zero-resend on unproven history:

- `canonical history reconciliation reached its safety limit before proving whether the message landed; do not resend`
- `the correlation challenge may have landed but exact canonical content does not confirm it; do not resend`

## Acceptance mapping against the ASMA-8203 audit scope

| Audit requirement | State at `44663e10` | Evidence |
|---|---|---|
| Durable issuance before send | Present | `record_message_issuance` across 7 files; issuance written pre-effect |
| Epoch persistence before effect | Present | capture routed through `tail_window_recovering_epoch_once(adapter, identity, binding, 1, 1)` |
| Bounded first-issuance floor | Present, **ungated** | `state.rs`: "Every send gets its boundary, including the first"; `note_issuance_boundary` called before the `Replayed` check |
| Suffix strictly after the floor | Present | `adapter.rs`: `if floor.is_some_and(\|floor\| event.position.sequence <= floor.sequence) { continue; }` |
| Schema 118 resumable legacy proof | Present | `0118`, CAS pages, forward-only/no-delete triggers |
| Missing native history refuses, zero resend | Present | 30 `DeliveryConfirmationUnknown` sites; refusal strings above |
| Post-delivery-failure + restart duplicates (the rejected P1) | Covered | direct **and** derived restart regressions pass |

### Prior findings, preserved

The old audit `artifact-asma-8203-high-audit-report-24290153` rejected on
post-delivery commit failure plus restart producing a duplicate. That finding
stands as historically valid; it is corrected in this release, not withdrawn.

Three defects I reported during implementation are carried into the deployed
source, in one case with my comment text intact:

1. **First-send boundary never registered.** Registration was gated on
   `Replayed`, so a first send into a long session still reconciled against whole
   history. Fixed and ungated.
2. **Pre-effect epoch durability.** Boundary capture now goes through the
   durability barrier, so no floor names an epoch that exists only in one
   process.
3. **Occurrences at or below the floor counted.** The page reaching the floor
   overshoots it; such occurrences belong to an earlier issuance. Filtered.

Root additionally **strengthened** the floor path beyond what I submitted, with a
page-join and intra-page contiguity check (`joins` / `contiguous`) that I had not
written.

## My previously open red item does not reproduce here

I last reported `native_child_archive_requires_retirement_and_recovers_a_lost_acknowledgement`
timing out at 15s on **my** branch (`48b4a896`), caused by the pre-send round
trip my boundary capture added, and I did not diagnose it.

At `44663e10` that test **passes**: `final-daemon.log` records
`native_child_archive_requires_retirement_and_recovers_a_lost_acknowledgement ... ok`.
The test itself is unchanged (still a 15s timeout) and the capture call is the
same shape, so the difference lies in the rest of root's delta — which includes
their own changes to the retirement path in the same file. I did not isolate
which change resolved it, and I am not claiming to have; I am recording that the
failure I reported is **not present in the deployed source**.

## Root's qualification runs (attribution: root)

| Log | Result |
|---|---|
| `final-daemon.log` | 556 passed, 0 failed (97 + 457/1 ignored + 2) |
| `final-paseo.log` | 405 passed, 0 failed |
| `final-schema118.log` | 64 passed, 0 failed |
| `final-store-recovery.log` | 34 passed, 0 failed |
| `final-mcp.log` | 23 passed, 0 failed |
| `final-cli.log` | 22 passed, 0 failed |
| `final-openapi-check.log` | 3 passed, 0 failed |
| `final-clippy.log`, `final-format.log`, `final-console-parity.log` | no test lines; check-style logs |

Status `passed`. The four ASMA-8203 regressions I authored appear in that daemon
run — `direct`/`derived_first_send_registers_its_boundary` and
`direct`/`derived_delivery_epoch_commit_failure_then_restart_does_not_resend` —
all `ok`, and the ceiling regression
`a_long_session_acknowledges_from_a_bounded_suffix` is `ok` in the Paseo run.

## Deployment integrity (attribution: root)

`quick_check: ok`, `foreign_key_violations: 0`, `identities_preserved: true`,
`signatures_valid: true`, `operator_restart_count: 1`, `watchdog: remains
stopped`.

## Declared limitations

1. **Root's own open item.** The bundle records
   `launch_anomaly: "Unexpected automatic launch count; see launchctl-after.txt;
   root-cause review required"`. This release is deployed and healthy with that
   review outstanding. It is root's finding, not mine, and it is unresolved.
2. ~~**I did not re-run the suites.**~~ **Lifted** — see *Independent execution
   at exact head* below. Figures for the P1 regressions, the loopback suite and
   the Paseo suite are now mine; the remaining per-suite figures above
   (schema118, store-recovery, mcp, cli, openapi) are still read from root's
   logs and not reproduced by me.
3. **Legacy acknowledgements remain UNKNOWN.** All Sep-20 issuances keep their
   exact original ids and epochs under
   `operational-gap-legacy-message-unknown-current-qualification-20260921`. I
   replayed nothing and sent nothing.
4. **Coverage I flagged earlier is now root's to confirm, and I did not verify
   it.** In my last implementation turn I recorded that suffix-duplicate,
   changed-epoch and partial-absence refusal tests were unwritten. Root reports
   nine legacy-proof mutants; I read the qualification logs, which report suite
   totals rather than per-mutant outcomes, so I can neither confirm nor dispute
   that those three specific cases are covered. Stated as unverified rather than
   assumed either way.
5. **The `native_child_archive` interaction is unexplained, not proven absent.**
   It passes here; I did not identify the mechanism, so I cannot rule out a
   timing-sensitive interaction resurfacing under different load.

## Independent execution at exact head

The audit finding `P1-POSTDELIVERY-FAILURE-RESTART-DUPLICATES`
(`artifact-asma-8203-high-audit-report-24290153`, gate receipt `01a0c028-1ab6`)
is corrected in the deployed source and needs no further implementation. What it
did still need was someone other than the author of the qualification log to run
it, so that is what this section is.

Executed against the deployed source, in a clean worktree at `44663e10`:

| Check | Result |
|---|---|
| `direct_delivery_epoch_commit_failure_then_restart_does_not_resend` | **ok** |
| `derived_delivery_epoch_commit_failure_then_restart_does_not_resend` | **ok** |
| `kontor-daemon --test loopback_api` | **457 passed, 0 failed, 1 ignored** |
| `kontor-runtime-paseo` | 4 result blocks, **0 failures** |

The loopback total reproduces root's `final-daemon.log` figure exactly.
Receipt: `/tmp/p1-head-verify.log`,
sha256 `5661c18e092eab350ae4a2623c0a018b5fc00f9f4248d09803a37a522310b0fc`.

### Mutation — the regression is not vacuous

A passing test proves nothing until the defect it names can make it fail, so the
correction was removed at head and the same two tests re-run.

**Mutant M-P1**: `note_replayed_issuance` no longer declares a replayed issuance
unknown — the adapter is never handed back the durable fact a fresh process
lost, which is precisely the audited defect.

Both tests failed, reproducing the finding in the numbers the audit describes:

```
direct  ... the replay reconciled the existing delivery instead of instructing the seat twice
          left: 2   right: 1
derived ... the replay reconciled the existing delivery instead of instructing the seat twice
          left: 3   right: 1
```

Two native effects on the direct path and three on the derived one, against one
expected. The source was then restored byte-identically (`git status` clean) and
re-verified green.

## Uncovered defects

**None found.** The one candidate — the qualified/deployed commit mismatch —
resolved to identical trees on inspection. I record it as checked-and-clear
rather than omitting it, because the two differing SHAs are exactly what a later
reader would trip over.
