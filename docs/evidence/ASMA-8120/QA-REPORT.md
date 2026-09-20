# ASMA-8120 QA report

Date: 2026-09-20
Artifact: `qa-report`
Verdict: **BLOCKED — source repair passes; rollout completion is not evidenced**
Candidate: `19521d12164a8513c48328581bd7d5517cffee5d`
Candidate tree: `8428ae1cc3f7f26cf79d11890d34f9948ab72727`
Base: `1e11e4abf80d8751e90fdee77670c785d45e5f1e`

## Outcome

The source change in `77148a4c` is correct and independently green. It adds the
missing `correct_task_worktree -> task / witness / no desired state`
declaration to the test-owned command matrix and a focused semantic guard. It
does not change production behavior.

ASMA-8120 cannot receive a task-level PASS. The handed-off high-change record
is explicitly partial, twelve eligible epics remain unmigrated, the required
restart proof is absent, and OQ-8120-04 and OQ-8120-05 remain open. QA did not
choose a disposition for either question or perform any rollout mutation.

## Independent source verification

All checks ran against the clean candidate before this report was added.

| Check | Result |
|---|---|
| `git diff --check 1e11e4ab..19521d12` | pass |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --locked -p kontor-core --tests -- -D warnings` | pass |
| `cargo test --locked -p kontor-core --test domain_state --no-fail-fast` | pass, 38/38 |
| `cargo test --locked -p kontor-core --no-fail-fast` | pass, 324/324 across 17 targets |

The two test commands used a fresh target directory. The focused guard passed,
as did the exhaustive declaration/matrix equality check.

The causal baseline was also reproduced from an isolated archive of
`1e11e4ab` with a separate fresh target directory:

```text
cargo test --locked -p kontor-core --test domain_state \
  every_command_kind_declares_its_legal_targets_revision_rule_and_desired_state \
  -- --exact

FAILED: correct_task_worktree must not be able to target a task
exit 101
```

That failure disappears at `77148a4c`; no production source differs between
the two commits. This proves the added declaration closes the pre-existing
test/production disagreement rather than masking it with a production change.

## Task-level blockers

### Q-8120-QA-01 — twelve eligible epics remain unmigrated

The scope identifies thirteen eligible confirmed-and-pinned epics. ASMA-8049
is the only recorded successful migration. The high-change record previews the
remaining twelve, then records that the only two apply attempts were inert.
Therefore twelve, not eleven, remain unmigrated.

Five of the twelve refuse at preview and need separately owned repairs:
ASMA-7869, ASMA-8108, ASMA-8111, ASMA-8101 and ASMA-8098. Seven preview clean,
but none has a successful apply receipt: ASMA-8109, ASMA-8113, ASMA-8155,
ASMA-8208, ASMA-8186, ASMA-8188 and ASMA-8190.

### Q-8120-QA-02 — the handoff record is internally inconsistent

The durable records need correction before another caller relies on them:

- `HIGH-CHANGE-RECORD.md` labels the current state as stopped at the canary and
  says step 5 has no receipt, while its resume section records a successful
  canary receipt and independent readback.
- The same record says "remaining eleven" after listing twelve remaining
  epics and explicitly proving both attempted applies had no effect.
- OQ-8120-04 says "the seven migrations recorded" have before/after identities,
  but the high-change record contains seven clean previews, zero successful
  migrations among them, and only two inert apply attempts.
- OQ-8120-05 repeats the eleven-epic count even though its own next paragraph
  says seven of twelve preview clean.
- The seven preview hashes described as ready for exact reuse are stored only
  as eight-character prefixes. A future caller must obtain the complete hashes
  from durable receipts or perform fresh previews; the prefixes are not apply
  inputs.

These are evidence defects, not authority to edit the rollout state. The two
underlying ambiguities are already recorded as OQ-8120-04 and OQ-8120-05, so QA
did not add a competing open-question entry.

### Q-8120-QA-03 — required closure evidence is absent

The high-scope record requires sequential apply/readback for the remaining
eligible fleet, full before/after/restart identity tuples, archived/retired
non-effect evidence, and a supported daemon restart followed by persistence
checks. The handoff expressly records that no daemon restart occurred and does
not contain those completion receipts. A partial record cannot satisfy that
contract.

## Required disposition before PASS

1. Resolve OQ-8120-04 and OQ-8120-05 through their recorded authority path.
2. Correct the high-change and open-question records so counts, outcomes and
   reusable inputs agree with the receipts.
3. Complete or explicitly re-scope the twelve unmigrated eligible epics through
   supported preview/apply/readback paths; do not infer the five missing inputs.
4. Record the required restart-persistence, exact identity and database
   integrity evidence.
5. Present the resulting clean candidate and complete evidence to QA again.

## QA boundary

QA changed no production or test source, invoked no live Kontor mutation, and
did not restart the daemon. The only QA-authored repository change is this
report.
