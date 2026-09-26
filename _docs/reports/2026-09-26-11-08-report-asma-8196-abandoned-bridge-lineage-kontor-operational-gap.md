---
title: ASMA-8196 abandoned replacement bridge selection
type: report
status: "🟡 Source correction; qualification and deployment pending"
created: 2026-09-26
jira: ASMA-8196
---

# ASMA-8196 abandoned replacement bridge selection

## Observed defect and authority

The deployed `dddb7287` naming preview returned HTTP 409 for ASMA-8190 with
“a delivery role has ambiguous current replacement-chain leaves.” Twelve other
migrated epics returned HTTP 200 with no pending title changes. The all-thirteen
post-restart census remains incomplete.

The [actual independent diagnosis](evidence/ASMA-8196/replacement-bridges/actual-lsa-bridge-diagnosis.txt)
proves one immutable verify-role lineage on TeamRun
`01a0b032-fe68-70c2-8cd7-e5bcb76fb56d`: cancelled bound run
`01a0bff0-c58c-7122-a53b-6b55917c8dcf`, abandoned never-bound bridge
`01a0c631-ed6f-7502-a510-cd945b2c8de6`, and current bound run
`01a0cf81-50ec-7822-8f3a-bd60690a9bfe`. Logical SeatBinding
`01a0b033-1d67-7bc2-b5e4-e5dd3ad9c72b` names native
`a526c5cc-e399-43c9-8a5e-830e5834a27b`. The old native
`5750a5be-8399-4360-a9af-4f9594eac006` is archived history.
There is no competing current authority.

The daemon discarded certified abandoned/unbound rows before computing parent
relationships. The store migration census considered only a direct child.
Both therefore lost the structural bridge and treated the bound ancestor as
another current occupant. Runtime settlement alone cannot repair this graph
selection error.

## Bounded correction and regressions

The daemon now follows transitive parent relationships from meaningful runs
through abandoned structural bridges, then excludes abandoned rows from leaf
candidates. A trailing abandoned-only chain leaves its bound predecessor
current. A genuine fork still refuses; a cyclic chain refuses explicitly.
The store traverses descendants within the exact project/TeamRun/role and
requires one current native occupant per logical seat. Migration record and
confirmation use that same census.

The preserved [daemon red result](evidence/ASMA-8196/replacement-bridges/daemon-red.txt)
reproduces the actual 409 rule; trailing abandoned and fork cases already pass.
The [store red result](evidence/ASMA-8196/replacement-bridges/store-red.txt)
has two behavioral failures: bridge census includes two occupants and a genuine
fork is not refused. After correction the [store suite](evidence/ASMA-8196/replacement-bridges/store-green.txt)
passes 23 contracts and the [public-preview suite](evidence/ASMA-8196/replacement-bridges/daemon-green.txt)
passes three tests, covering one/two bridges, trailing abandoned attempts, and
a genuine fork. The positive preview asserts exact current AgentRun/native,
unchanged logical SeatBinding/generation, and absence of the predecessor native.
Store tests prove current-only migration record/observation/confirmation and
retain every immutable lineage row.

The preview fixture deliberately records an absent current native and expects
`rename_pending`; it provides selection evidence, not a launch or native
restoration receipt. Existing predecessor cancellation and bridge abandonment
use the real API. Store fixtures use production repository writers.
The [checkpoint receipt](evidence/ASMA-8196/replacement-bridges/focused-checkpoint.json)
and [hash manifest](evidence/ASMA-8196/replacement-bridges/SHA256SUMS.json)
bind the source and actual outcomes.

## Worktree recovery disposition

This Paseo-direct repair owns the existing ASMA-8190 module worktree. ASMA's
generic checkout resolver refused its external child-worktree context, and
the exact `asma worktree add ASMA-8190 --mod _tools/asma-rs-kontor` operation
refused the already-existing path. The ASMA branch-create fallback did create
`fix/ASMA-8190-replacement-bridge-lineage`. A bounded guarded
`git reset --keep dddb7287` on that new unpublished clean branch established
the deployed source base after a fast-forward merge refused the historical
squash divergence. No uncommitted work was present or lost. The former
`feat/ASMA-8190-autonomous-epic-delivery` branch still preserves
`97a9cb81c8f5342ee6b5df3370824f9fb7ac433a`.
Commits, publication and final PR merge use ASMA.

The primary module's supported `asma git ... checkout master --latest --safe-carry`
preserved unrelated untracked evidence and retained recovery stash
`5b7b8fd648732ee1aeaac59f25544b1aa08e8dec`; it was not popped or dropped.
Other worktrees and their changes remain untouched.

## Remaining acceptance

Current results qualify only the focused source correction. Behavioral mutants,
the exact-candidate full archive gate, independent source review, merge/build/
deployment, and the fresh all-thirteen joined census remain pending. No live
parents, run identities, native identities or SeatBindings were rewritten.
After qualification/deployment, use supported settlement for the current run's
fresh observation, require ASMA-8190 preview 200 with that exact current
run/native/binding, no old native and zero `would_change`, then repeat all
thirteen previews. ASMA-8196 task settlement, ASMA-8120/8121 workflow gates,
ASMA-8049/8190 epic acceptance and global Kontor trust remain uncredited.
