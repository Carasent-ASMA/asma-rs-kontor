# ASMA-8200 high-audit report

Date: 2026-09-20
Artifact: `high-audit-report`
Task: Jira `ASMA-8200` / Kontor `01a0ac9d-a95c-7291-91e8-b472b7b331bc`
Epic: Jira `ASMA-8190` / Kontor `01a0abdd-b84e-7be2-a1f6-b9f34aae8c23`
Project: `01a0064a-e056-7603-9968-ef64fdaacb75`
Phase: `high-audit`
TeamRun: `01a0b033-78bd-7691-90e0-a8a34eb46030`
Audit AgentRun: `01a0c032-e6bf-7010-b3b8-4bfebd9893ca`
Corrective source: `e16cd98d819b9f34d99ad3930794bc2d3f939090`
Evidence head: `a231b32945bc590f4c112efa5e07e1c68aa77926`
PR 253 merge: `6fbf4fc2da871c6a3ccca79a5c133f8c53fd4101`
Deployed source: `9dae7f790552bc4746255cd01d8ad1160cab7211`

## Verdict

**PASS.** The corrected implementation satisfies the frozen scope, closes the
original startup-precedence defect, preserves the intended process-lifetime
capacity model, and is converged in the live realm. No open audit finding or
unevidenced assumption remains.

This report does not record the audit gate. The current audit turn must first be
settled and this exact blob registered as `high-audit-report`; the already
settled exact scope blob must likewise be registered as `high-scope-record`.
The later evaluator write must use the account pin returned by supported context,
not infer one from provider or model. Current `kontor_run_get` readback exposes
`account_profile_id: null`, so this report does not invent an evaluator account.

## Audited lineage and preserved history

The audit used the clean isolated checkout
`/private/tmp/kontor-8200-capacity-precedence` at evidence head `a231b329`, not
the stale original worktree head. The lineage is:

1. `d21dc2de` — original integrated implementation and producer evidence;
2. `e16cd98d` — correction selecting a durable policy before validating the
   fallback seed;
3. `37e9a014` — independent corrective verification report;
4. `a231b329` — documentation-only correction to the helper attribution;
5. `6fbf4fc2` — PR 253 merge containing the complete corrected branch; and
6. `9dae7f79` — deployed descendant containing PR 253.

The original independent **REJECT** at `b8303a02` remains immutable historical
evidence. It is not amended, replaced, or re-described as a pass. The later
report at `37e9a014` verifies the coordinator's correction and records the
separate PASS.

Producer provenance is also preserved. Registered evidence
`01a0c028-aab9-7021-bbe2-9ba3605c8cba` points to the exact
`HIGH-CHANGE-RECORD.md` blob at `d21dc2de`, SHA-256
`1b7beba232c0c5eace23cb32d9b78421ff1ee69793f9c8516de47dddeeaaa24a`.
Its `producer_account` is **NULL**, with attribution
`native_proved_unknown`; this audit does not assign one. Registered evidence
`01a0c029-5f6a-77a3-bbb8-e2cc66f399ae` points to the exact verification blob
at `37e9a014`, SHA-256
`2576981026194fa563df5c02065f2cc117efb59cc14a27cc5b372d940a81a151`,
under the verifier's original account pin. Verification gate receipt
`01a0c02d-40d4-7382-ae7a-80b0b788a200` records `passed` at sequence 1.

## Scope-to-source audit

| Frozen requirement | Audited evidence | Result |
| --- | --- | --- |
| Read the singleton after store open/migration and before service composition | `start_with_supervision` opens `SqliteStore`, calls `capacity_in_force`, writes the result back to `config.capacity`, then passes that value to the sole `Services::new` call | pass |
| Present stored policy is authoritative; seed is only an absent-row fallback | `capacity_in_force` reads first and calls `seed.validate()` only in its `None` branch | pass |
| Present malformed or domain-invalid policy fails closed | Present rows use `applications::stored_capacity`; read errors map to `StartupError::Store`, conversion/validation errors to `StartupError::StoredCapacity` | pass |
| One process uses one immutable policy; no live reload | The durable row is read once at composition and the selected value is retained by `DaemonConfig` and `Services` | pass |
| No duplicate conversion or scheduler semantics in the store | Startup reuses `StoredCeilings`, `capacity_config`, and `stored_capacity`; the configuration read surface intentionally keeps its existing separate conversion | pass |
| `restart_required` is truthful | Readback compares stored and composed ceilings; apply compares the written document with `self.capacity` | pass |
| No new schema, route, CLI, MCP, DTO, capacity knob, or default change | The corrective production delta is confined to startup ordering and regressions; `DEFAULT_CAPACITY` and external surfaces are unchanged | pass |
| Required restart, refusal, CAS, scheduler, and mutation coverage | Corrective verification records 545 daemon tests passing, 3/3 OpenAPI, 12/12 MCP parity, and both precedence mutants killed | pass |

The P2 documentation finding from corrective verification is closed by
`a231b329`: `stored_capacity` now says startup uses the validated helper while
the read surface retains its separate conversion. No runtime behavior changed
after the verified `e16cd98d` tree.

## Live convergence audit

Deployment receipt
`/Users/igor/.local/state/kontor/asma/deployments/ASMA-7869-20260920T185808Z-9dae7f79/deployment.json`
was verified at `2026-09-20T18:59:21.759604Z`. It records source `9dae7f79`,
schema 114, one restart, a healthy open realm, zero foreign-key violations,
and the watchdog still stopped. Git ancestry proves `e16cd98d`, `a231b329`, and
PR 253 are all ancestors of that deployed source.

Fresh supported readback at snapshot cursor 4385 shows:

- realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065` is unchanged;
- configuration revision is 1;
- effective ceilings are exactly `20/13/12/4/4/13`, adaptive `4/1/12/1`,
  with no headroom;
- `restart_required` is `false` and no differing `stored_ceilings` is returned;
- project capacity reports scheduler mission ceiling 12; and
- task `ASMA-8200` remains `in_progress` at `high-audit`, with the verification
  gate passed and the audit gate not yet recorded.

The deployment's `identity-before.json` and `identity-after.json` are
byte-identical, both SHA-256
`26e2514b84710dd206378065d10810b3a9efb463c92766bb355ed0a42a5e0881`.
Its `completion-before.json` and `completion-after.json` are also byte-identical,
both SHA-256
`6e8bd3bb857458c874820ef519059b656c8fa7092392bc4efa1dd057726c68e5`.
The receipt records preserved counts for 18 projects, 136 tasks, 457 AgentRuns,
643 seat bindings, 385 runtime bindings, and 21 hosted topology seats. This is
the required proof that activation did not replace identities or mutate task,
run, seat, topology, gate, or Jira completion state.

## Audit checks and limits

| Check | Result |
| --- | --- |
| corrected source and docs read at `a231b329` | pass |
| `git diff --check 0f649824..a231b329` | pass |
| PR 253 contains `e16cd98d` and `a231b329` | pass |
| deployed `9dae7f79` contains PR 253 | pass |
| exact producer evidence and NULL account attribution read back | pass |
| live configuration and scheduler projection read back | pass |
| pre/post identity and completion snapshots compared byte-for-byte | pass |

No test was duplicated: the independent verifier's complete green suite and two
killed mutants directly cover the corrected logic, and this audit found no
unresolved concern requiring another execution. The audit changed no production
source, capacity, deployment, runtime, topology, Jira, watchdog, task, seat, or
gate state.

## Open questions

None.

## Handoff

Coordinator: settle this audit AgentRun's terminal turn, register this exact Git
blob as `high-audit-report`, and register the exact settled scope blob as
`high-scope-record`. After those durable producer records exist, return the
fresh workflow revision and supported audit-seat account pin so the same audit
seat can record `high-audit-gate: passed` with all four declared evidence keys.
