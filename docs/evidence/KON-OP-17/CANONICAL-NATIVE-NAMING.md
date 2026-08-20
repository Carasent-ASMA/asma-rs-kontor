# KON-OP-17 / ASMA-7950 — canonical native naming and Core Team route correction

Date: 2026-08-20
Branch: `fix/ASMA-7950-kontor-canonical-native-naming`
Status: schema-46 repair deployed; Paseo 0.4.0 project-rename attestation follow-up locally verified
Schema: 46 (`0045` remains KON-OP-18-owned; this repair adds `0046`)

This receipt supersedes only the later operational conclusions in
`NATIVE-PROJECTION-RECOVERY.md`. That earlier document remains the evidence for
the schema-44 checkpoint and its bounded fallback. QNR ASMA-7675 was not
mutated while this repair was developed or verified.

## Closed contract gaps

1. `EpicTaskRequest.short_code` is now the single durable, explicit source for
   a backlog task's compact native display identity. Preview, apply, epic
   projection, backup/export and restore all carry it. Existing KON-OP tasks
   retain their historical `KON-OP-*` prefix through a deliberately narrow
   migration backfill. No description, Jira key, UUID, worktree slug or ordinal
   is used as a fallback.
2. A legacy task without a short code fails closed before native runtime
   contact. An operator migrates it through the ordinary `epics:preview` then
   `epics:apply` contract with an explicit per-task mapping. Existing epic,
   task, topology, TeamRun, AgentRun, SeatBinding and native ids are preserved.
3. Canonical names are derived only from durable contract fields:

   - ESW/Paseo project: `Epic · <Jira epic> · <short epic title>`;
   - ECP: `ECP · <Jira epic> · <short epic title>`;
   - TSW: `TSW · <Jira issue> · <task short code>`;
   - ticket seat: `<Role> · <task short code>` (plus stable slot suffix when
     required).

4. Native ESW repair uses Paseo `project.rename`, while ECP/TSW repair uses the
   workspace retitle operation. Both paths read back the same native identity;
   neither recreates topology or containers. Paseo 0.4.0 carries the correlated
   project-rename envelope but omits `projectRename` from `status/server_info`;
   Kontor therefore attests that one optional operation from the 0.4.0 release
   floor while keeping the general recorded protocol floor at 0.3.1. Older,
   pre-release and unparseable versions remain fail closed.
5. Delivery-session reconciliation repairs the canonical seat title and
   labels in place. `jira.epic` means the external key (for example
   `ASMA-7675`); the already-published `kontor.project_id` contract remains the
   internal Kontor epic/MiniProject id.
6. Persistent Core Team LSA/TPM seats may be hosted in the exact bound local
   ECP. Ticket roles remain forbidden in a root/plain local workspace.
7. Admin Core Team route preview/apply preserves the logical SeatBinding,
   requires exact native id and generation, archives the exact idle
   predecessor, records route history, and launches one successor on the
   explicitly requested authorized provider/model/effort route. Replay is
   idempotent and identity drift fails closed.
8. Paseo now advertises and implements the exact retirement capability used by
   the controlled route correction. AO continues to declare it unsupported.

## QNR operator boundary and required decision

The QNR epic remains parked. Its durable identities are unchanged:

- realm `01a00649-9ee6-73e0-ba1b-6a6c35cfd065`;
- project `01a0064a-e056-7603-9968-ef64fdaacb75`;
- epic `01a019c0-eee7-72a1-a8a7-7fff1ddce8f3` / Jira `ASMA-7675`;
- ESW `01a01b25-c342-77e3-9802-fc4ccae3e8f0` / native project
  `prj_85aa32f2c4c4217f`;
- ECP `01a01b25-c343-7443-a1b0-145ca3ef6de5` / native workspace
  `wks_6f8d97404c7a18da`.

Adam/Igor must provide the authoritative short-code mapping for the legacy QNR
tasks, including ASMA-7676, ASMA-7679, ASMA-7930 and ASMA-7932. Kontor will not
guess it. After deployment, the Adam successor must preview/read back the whole
existing epic declaration with that mapping, apply the exact preview, then
retitle and reconcile the existing identities in place. It must not recreate
the ESW, ECP, TSWs or sessions.

The QNR TPM route correction must likewise be previewed and applied against
logical SeatBinding `01a01bfa-b4f4-7510-ad8c-59b08dfd85f6`, using its live
native id/generation readback and the approved lightweight route
`opencode` + `deepseek/deepseek-v4-flash` at `high` (or a later explicitly
approved fallback). No Claude session may be started or resumed during the
provider outage.

## Regression and mutation evidence

Focused regressions prove explicit short-code migration, fail-closed legacy
materialization, canonical project/workspace/seat naming, native project rename
with identity preservation, external/internal label semantics, real Core Team
placement in a local ECP, exact route correction, and capability-gated native
retirement. Deliberate mutations M25–M30 and their killer tests are recorded in
`MUTATION.md`; no mutant remains.

The release candidate passed:

```text
cargo fmt --all -- --check                                  passed
git diff --check                                             passed
cargo clippy --workspace --all-targets -- -D warnings       passed
cargo test --workspace                                      passed
pnpm --dir apps/console verify:api                           passed
pnpm --dir apps/console typecheck                            passed
pnpm --dir apps/console test                                 passed (295 tests)
cargo audit && cargo deny check && pnpm audit --prod         passed
cargo build --release --workspace                           passed
```

Merged-source schema-46 hashes installed from PR #62:

```text
kontor-daemon fe22568ac81943517ae6342f71c18ad9b6193c413102249d9ea57d917c95b856
kontor        ed1a8ea40d49cbf507542987d76160a0e46740a4d498e48f1fd175069f0a4ff0
kontor-mcp    1c29165fe70fc21a699ecc835df6da46010bdf6449c97eec6d77d3838f0fff7b
```

## Release receipt

Canonical naming/session repair PR #62 passed all four required CI jobs and
merged as `ecfe7c5a05594e81398cc298dd1b14b2dec7a8cc` at
2026-08-20T16:19:59Z. Its exact merged tree was built and installed with the
hashes above. Launchd started PID `16870`, the database remained at schema 46,
the loopback API answered HTTP 200, startup crossed the reconciliation barrier,
and the post-boot log contained zero `refused to restore` lines.

The first live ESW retitle preview then exposed a distinct compatibility gap:
Kontor returned 422 before runtime contact because Paseo 0.4.0 did not advertise
the project-rename feature that its installed daemon bundle implements. The
follow-up branch `fix/ASMA-7950-paseo-040-project-rename-attestation` adds the
release-floor attestation and M30 regression without changing schema or any QNR
identity. Its full fmt, clippy and workspace test gates passed before release.
