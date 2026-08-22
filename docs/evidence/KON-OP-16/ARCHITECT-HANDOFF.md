# KON-OP-16 / ASMA-7943 — Architect handoff

Recovery architect. Codex weekly quota was exhausted until ~2026-08-23 09:35;
Claude hit individual spend / 5h. This seat implemented against current
`asma-rs-kontor` `origin/master` (`9bc8510`, OP-15 / ASMA-7942 PR #81). It did
not use Claude or Codex.

## Ruling (LSA, dotted vs slash, no migration, AC-7)

- New admissions use canonical `/` module paths (`shared/asma-core-helpers`,
  `editor/asma-bunjs-editor`, …).
- These four ACTIVE module leases keep their current dotted keys until they
  release. Do not rewrite those rows:
  - `shared.asma-core-helpers`
  - `editor.asma-bunjs-editor` (two worktrees)
  - `editor.asma-app-editor`
- They will not contend with a slash spelling of the same module as two locks.
  Identity matching (`replace('/', '.')` on both sides) makes them one lock.
- Worktree filesystem paths that contain `.` are not this problem. Identity
  matching applies only to `lease_kind = 'module'` (and NULL v1 module rows).

## What shipped

1. **Every changed module takes a lease.** `tasks.module_key` stays the primary.
   Additional modules live in `task_modules` (schema v52;
   `0052_task_modules_and_module_identity.sql`). v51 is a version-only
   reservation so OP-13 PR 84 can keep `0051_provider_quota_headroom.sql`.
   Apply field `EpicTaskRequest.modules` is additive; omission leaves existing
   extras alone. The extra set can be filled once, then is immutable like
   `module_key`. No backfill of live QNR tasks.
2. **Dotted holdouts are not stolen.** `ModuleKey::contention_identity` /
   `contends_with`, `ensure_place_free` / `expire_lapsed` identity SQL, and
   trigger `resource_leases_module_identity_exclusive` refuse a slash admission
   against a live dotted lease of the same module and leave the dotted row
   unchanged. A *lapsed* dotted lease is reclaimable by the slash key (reclaim
   lineage); the dotted `resource_key` is still not rewritten.
3. Distinct verified worktrees of the same identity remain allowed. Unisolated
   vs anything of the same identity is exclusive, as before.

## Tests that pin the holdout

- `module_keys_contend_across_slash_and_dotted_holdout_spellings`
- `a_live_dotted_holdout_refuses_a_slash_admission_and_keeps_its_row`
  (store pre-check + direct SQL trigger; dotted `resource_key` unchanged)
- `a_lapsed_dotted_holdout_is_reclaimable_by_the_canonical_slash_key`
- `every_changed_module_takes_a_lease_and_the_secondary_contends`
- planner + guardrail slash vs dotted `ModuleInFlight`

## Out of scope

- No rewrite of live `resource_leases.resource_key` rows.
- No QNR fixture / live epic mutation.
- Superproject gitlink must not advance.
