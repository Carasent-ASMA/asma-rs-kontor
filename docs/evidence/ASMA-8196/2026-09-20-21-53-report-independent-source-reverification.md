# ASMA-8196 Independent Source Re-verification

> **Date:** 2026-09-20 21:53 CEST
> **Status:** 🟢 Approved — source-only verification
> **Author:** ASMA-8196 verification seat `01a0bff0-c58c-7122-a53b-6b55917c8dce`
> **Category:** report
> **Scope:** Exact source commit `6c88c09342484d5a11d1f17923ec7446cac1b48b`; schema 116 persona snapshot repair; no merge, deployment, or live readback
> **Summary:** Independent same-seat re-verification of the two findings that rejected candidate `07dd4c1b`. The immutable per-occupancy persona snapshot and successor role-prompt regression pass focused exact-head tests and independently rebuilt mutants. The verdict is `PASS_SOURCE_ONLY`; live schema-116 activation remains unproved.

---

## When to Load

**Load this document when:**

- deciding whether ASMA-8196 commit `6c88c093` repaired the two prior high-verification findings;
- auditing TEST-007's durable persona evidence or the successor-launch mutation evidence;
- distinguishing exact-head source evidence from prerebase broad-suite results or later live schema-116 evidence.

**Do NOT load for:** claiming schema 116 is deployed, proving a live Paseo native's applied system prompt, or proving the later ASMA-8198 handoff/settlement behavior.

---

## Verdict

`PASS_SOURCE_ONLY`

No P0 or P1 source finding remains in the requested re-verification scope. This verdict does not authorize or claim a merge, deployment, schema activation, live native readback, task-gate transition, or completion of the producer's runtime-message acknowledgement.

## Verified identity and inputs

- Candidate: `6c88c09342484d5a11d1f17923ec7446cac1b48b`
- Candidate parent: `84135efa6c6a830b7d8c00eacb75d4072fe0ad44`
- Parent meaning: the real schema-115 atomic-receipt integration
- Source branch observed clean: `fix/ASMA-8196-persona-readback`
- Producer worktree observed clean and left untouched: `/private/tmp/kontor-8196-repair`
- Isolated verification checkout: `/private/tmp/kontor-8196-qa-reverify`
- Producer artifact: `artifact-asma-8196-high-change-6c88c093`
- Approved producer revision: `01a0c052-7f30-7970-ac28-f657ebea7a57`
- Independent verifier AgentRun: `01a0bff0-c58c-7122-a53b-6b55917c8dcf`
- Independent verifier SeatBinding: `01a0bff0-c58c-7122-a53b-6b55917c8dce`
- Independent verifier native identity: `5750a5be-8399-4360-a9af-4f9594eac006`

The workspace-required codebase-memory playbook was loaded, but its graph tools were not exposed to this seat. Verification therefore used the exact parent-to-candidate diff, targeted source reads, and focused executable evidence. This limitation did not require an assumption about the changed paths.

## Finding P1-1 — durable TEST-007 persona evidence

Status: **repaired for source verification**.

The implementation now has these independently checked properties:

1. Schema 116 creates `hosted_seat_role_personas`, keyed by `(project_id, seat_binding_id, occupancy_generation)`.
2. UPDATE and DELETE triggers make a recorded occupancy snapshot immutable and permanent.
3. `RolePersonaSnapshot` retains the exact prompt bytes and their SHA-256 digest.
4. `RolePersonaDelivery::CreateOnlyNoReadback` serializes as `create_only_no_readback`.
5. Both initial and successor hosted-seat launch paths freeze and persist the snapshot before the native create call.
6. The same frozen value supplies `HostedSeatLaunchRequest.role_prompt`; persistence and delivery do not independently reconstruct the prompt.
7. Successors persist under their own occupancy generation, while the predecessor row remains unchanged.
8. `CoreTeamSeatPersonaDto` is separate from `CoreTeamNativeSeatDto`. The API therefore reports Kontor's frozen create-time delivery evidence without presenting it as runtime readback.

TEST-007's plan wording says “run snapshot.” A hosted leadership seat has no AgentRun. The producer artifact explicitly disclosed the seat-snapshot interpretation, and this re-verification evaluates the supported Core Team seat projection named by the repair request. No run provenance is inferred.

### Direct immutability probe

The schema-116 migration was loaded into an isolated SQLite database and seeded with one LSA row. The probe observed:

- inserted row: generation `1`, role `LSA`, delivery `create_only_no_readback`, schema `116`;
- UPDATE refused with SQLite exit `19`: `a launched occupancy's role persona is immutable`;
- DELETE refused with SQLite exit `19`: `a launched occupancy's role persona is not deletable`;
- final read remained `1|persona|create_only_no_readback`.

The temporary database was removed after the probe.

## Finding P1-2 — successor prompt mutant

Status: **repaired for source verification**.

The promotion regression now reroutes the LSA, which has a seeded persona, rather than relying only on a TPM successor whose persona is intentionally absent. It independently reads the successor's system prompt and initial handoff, asserts that the system prompt contains the LSA persona, and asserts that the two values are distinct.

## Exact-head focused checks

All commands below ran in the isolated checkout at exact source head `6c88c09342484d5a11d1f17923ec7446cac1b48b`.

| Check | Result |
| --- | --- |
| `cargo test -p kontor-store --test schema_v1` | PASS — 63 passed, 0 failed, 0 ignored |
| `cargo test -p kontor-api --test openapi_contract` | PASS — 3 passed, 0 failed, 0 ignored |
| `cargo test -p kontor-daemon --test loopback_api a_promotion_creates_one_epic_and_hands_the_work_to_its_lsa -- --exact --nocapture` | PASS — 1 passed, 0 failed, 429 filtered |
| Restored-source rerun of the same promotion test | PASS — 1 passed, 0 failed, 429 filtered |
| `cargo fmt --all -- --check` | PASS — clean |
| `git diff --check` | PASS — clean |
| Restored checkout before report authoring | exact head `6c88c093`; no source diff |

### Exact-head mutation evidence

Each mutation was applied alone with `apply_patch`, forced a `kontor-daemon` rebuild, was killed by the focused promotion regression, and was removed before the next mutation.

| Mutant | Result | Killing observation |
| --- | --- | --- |
| MUT-8196-3: successor launch sends `role_prompt: None` | **KILLED** | `loopback_api.rs:44934`: `the successor was launched under a persona` |
| MUT-8196-4: successor snapshot is recorded under `FIRST_HOSTED_OCCUPANCY` | **KILLED** | `loopback_api.rs:44973`: projection reported generation `1` instead of the successor generation |

After both mutations, the exact candidate source was restored, the focused promotion regression passed again, and `git diff --exit-code` confirmed no residual source mutation.

## Test-scope honesty

The exact-head verification evidence is limited to the focused checks listed above. It does not relabel the producer's earlier broad results as tests of `6c88c093`:

- `kontor-daemon` 542 passed / 0 failed was a prerebase result;
- the cross-crate run's 1349 passed / 3 schema-expectation failures was a prerebase result;
- those three expectation failures are resolved by the exact-head schema `63/63` check, but the whole broad cross-crate suite was not rerun here.

## Not proved here

- No merge or deployment was performed.
- No live schema-116 receipt exists in this report; the supplied live baseline remains schema 114.
- Paseo exposes no system-prompt readback, so `create_only_no_readback` is intentionally weaker than native confirmation.
- The implementer's current runtime-message acknowledgement is not reconstructed or fabricated.
- The ASMA-8198 live handoff, action, and turn-settlement proof remains outside this task.
- No Kontor gate or Jira workflow state was advanced by this verification.

