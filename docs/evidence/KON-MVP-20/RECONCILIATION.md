# KON-MVP-20 reconciliation snapshot

Snapshot time: 2026-08-14 Europe/Oslo. This is a read-only truth record; it made
no AgentsRoom, Jira, Paseo, git-remote, or process-state mutation.

## Git and archive truth

- Outer `HEAD` = `origin/master` =
  `f9e341440b140ec4b94fbfeadfe5f52fd8e0ea89`.
- Its gitlink = submodule `HEAD` = submodule `origin/master` =
  `5cc0e223e8f297f551bb521c580508395620d432` (PR #24 merge).
- Committed/worktree/regenerated `Cargo.lock` SHA-256 =
  `2e89a646b8a4340951a96f4a655adcfafa82922c9943751657929894624f8179`.
- Source archive SHA-256 =
  `3ae9c7ae345072e909abcbd6f7464af5b8cc06d80d1b7941802d9de207f9572a`;
  archive verifier passed.

## Twenty-five child dispositions

AgentsRoom truth is complete: KON-01 through KON-11, KON-13 through KON-19,
and KON-21 through KON-25 are `done` (23 children). KON-12 / ASMA-7756 is
truthfully `pending` as the accepted AO production-lane deferral, reactivated by
the first production AO caller; its fixture-level artifact remains committed.
KON-20 / ASMA-7764 is the only `in_progress` child while this evidence and the
mandatory final committee remain open.

The Jira sync dry-run inspected all 25 and made no writes. Nineteen rows are
already status-synchronized (including the deliberately unmapped KON-12). Six
terminal transitions remain unapplied:

| Jira | AgentsRoom intent | Observed Jira -> proposed |
|---|---|---|
| ASMA-7751 / KON-07 | done | In Development -> Closed |
| ASMA-7759 / KON-15 | done | In Development -> Closed |
| ASMA-7760 / KON-16 | done | In Development -> Closed |
| ASMA-7762 / KON-18 | done | In Development -> Closed; description mirror also differs |
| ASMA-7821 / KON-23 | done | Ready for Development -> Closed |
| ASMA-7854 / KON-25 | done | Ready for Development -> Closed |

ASMA-7764 also has a description-mirror proposal. Therefore the snapshot is
truthful but **Jira is not yet fully converged**. The orchestrator must apply
and read back these normal close-out transitions before the final committee can
return `COMPLIANT`. The dry-run log SHA-256 is
`291784b040bfcbe919ed337f4fc2bbf77f5abd4e302f543b30b19ede013ddec0`.

## Paseo/runtime/client truth

- Canonical TSW `wks_6acbb27ff012c12c` and final-compliance TSC
  `wks_6a954eb856ea6017` both read back under epic project
  `prj_bfad22b5eed95efc` with the canonical checkout path.
- Nine persistent seats are visible in this checkout: this Architect is the
  only running seat; QA, Audit, Implement, and committee seats are idle. None
  requires attention. The final committee was not convened by this run.
- Paseo reports zero terminals and zero pending permissions. Process inspection
  found no `cargo test`, Playwright, Vitest, Vite, kontord, MCP-journey, or
  mutation process after validation.
- Rust, API/client generation, 278 console tests, build, and desktop/phone
  Playwright all converge on the same archive.

## Preserved residue and incident acknowledgement

The requested untracked files were left byte-identical and uncommitted:

- `crates/kontor-runtime-ao/src/mux.rs` —
  `30c583123ef488a191354f50d7999bf99a100e3666f52c251f545e6c25ebad46`;
- `crates/kontor-runtime-ao/tests/mux_live.rs` —
  `c1de6cf7bee4fb0b190f9dd1b6419826c11d3270c4182e30f48e5e6b72c80495`;
- `crates/kontor-runtime/src/terminal.rs` —
  `21c2974b423f3605fa3797bc528f6ecbf1e86f026b8c35d4391c27c874d8cfa5`;
- foreign `docs/evidence/KON-MVP-18/run-bcb865f13ce774ed/` aggregate —
  `eefdf05f9e7931fe8b62870befd2a6d343690e427f9b4a2e931c4ec08b79081e`.

Incident input
`/Users/igor/kon-mvp-20-scratch/evidence/paseo-corrective/incidents/2026-08-14-destructive-reset-dirty-tsw.md`
hashes to `b809886960d48bd6512428dc66150f481c0b33cfa46cca44af4de9f2f7f1b1f9`.
It records that destructive reset was used against a
dirty TSW. The KON-09 four-file residue is fully superseded by this merged
archive (`a280aaf` is an ancestor; no unique product bytes were lost). One old,
unstaged KON-18 `NOTES` edit was unrecoverable; it was non-product and not
acceptance evidence, while the canonical committed notes remain intact. The
process violation stays visible to the final committee and is not erased by the
successful archive result.

## Gate disposition

Code/archive validation is ready: 27/27 mutants killed and every gate green.
Cross-system close-out is **not yet fully converged** solely because the six
read-only Jira status proposals above have not been applied. No survivor or
product defect is being waived.
