---
title: ASMA-8196 abandoned replacement bridge selection
type: report
status: "🟡 Deployed lineage repair verified; census reconciliation and workflow acceptance pending"
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

## Additional unbound-fork boundary

The initial `eb41560f` archive has four genuine compiled behavioral kills and
green baseline/restored suites, preserved in its
[mutation receipt](evidence/ASMA-8196/replacement-bridges/initial-eb41560f-mutations/results.json).
The [initial driver failure](evidence/ASMA-8196/replacement-bridges/initial-eb41560f-mutations/initial-driver-failure.txt)
is a formatting-sensitive locator error after the first kill. Its source was
restored exactly; the corrected driver reused only verified exact-pin actual
logs and the identical patch. No infrastructure failure receives kill credit.

One additional [regression](evidence/ASMA-8196/replacement-bridges/unbound-fork-red.txt)
then fails on that source: a meaningful current leaf without a native hides a
fork from the store while public preview refuses it. The corrected census now
derives all meaningful current runs before selecting their runtime bindings,
and refuses multiple leaves on an active exact slot, including unbound leaves.
The [expanded store suite](evidence/ASMA-8196/replacement-bridges/store-expanded-green.txt)
passes 24 contracts; [final fork refusal assertions](evidence/ASMA-8196/replacement-bridges/store-fork-refusal-green.txt)
pass both cases and verify a semantic ambiguity error, not a generic failure.
The [expanded preview suite](evidence/ASMA-8196/replacement-bridges/daemon-expanded-green.txt)
passes all three tests with both bound/bound and bound/unbound forks. The
[additional checkpoint receipt](evidence/ASMA-8196/replacement-bridges/unbound-fork-checkpoint.json)
binds the source and actual outcomes. Earlier focused results and mutations
retain their original subjects; no mutation or full-gate credit transfers from
the initial store implementation to this changed source.

## Independent integration audit and corrective hydration

The [actual 3b9 source audit](evidence/ASMA-8196/replacement-bridges/actual-lsa-3b9-audit.txt)
supports both selectors but finds a P1 in canonical TeamRun hydration: retaining
only a meaningful run's immediate abandoned parent drops an earlier abandoned
bridge. Preview/migration can therefore accept two bridges while replacement
or team certification rejects the same immutable chain. The historical audit
retains that defect and its two P2 findings without later approval credit.

The canonical hydrator now retains the complete ancestor closure of meaningful
runs, before applying its existing role/team, occupancy, parent, cycle, fork and
successor-depth checks. Unreferenced trailing abandoned attempts remain omitted
and consume no successor depth. No durable parent or identity is changed.
The [core red log](evidence/ASMA-8196/replacement-bridges/hydration-red-teams.txt)
reproduces the missing-parent refusal; the [daemon red log](evidence/ASMA-8196/replacement-bridges/hydration-red-daemon.txt)
also reproduces replacement failure and the misleading settlement action.
After correction, the [core focused log](evidence/ASMA-8196/replacement-bridges/hydration-green-teams.txt)
passes two tests, including malformed-chain cases and actual depth enforcement;
the [daemon focused log](evidence/ASMA-8196/replacement-bridges/hydration-green-daemon.txt)
passes four tests. Its replacement regression actually launches the requested
other slot while retaining the two-bridge role's exact run/parent/binding rows.
The deliberately absent current native in that other role remains a fixture,
not live restoration evidence.
The [full team package](evidence/ASMA-8196/replacement-bridges/hydration-teams-all-green.txt)
also passes all 49 lifecycle contracts. The
[focused correction receipt](evidence/ASMA-8196/replacement-bridges/hydration-focused-checkpoint.json)
records the exact source hashes and separate red/green subjects.

Cyclic, absent-leaf and ambiguous-leaf refusals now advise reconciliation of the
recorded lineage while preserving identities. A fork regression verifies the
public action, rather than directing an operator to runtime settlement.

The [3b9 mutation receipt](evidence/ASMA-8196/replacement-bridges/historical-3b9-mutations/results.json)
records four actual compiled behavioral kills and restored three/24 greens on
that earlier source. It does not qualify this hydration correction. Its full
archive gate likewise remains an earlier-source run and gives no new-head credit.

The [whitespace preservation receipt](evidence/ASMA-8196/replacement-bridges/historical-whitespace-preservation.json)
retains original log bytes/hashes and exact encoded mutant-patch bytes. Five
display logs remove only extra blank final lines; the historical patch's
significant blank context is base64 encoded instead of silently changed. The
original 3b9 manifest and original mutation receipt remain separately preserved.
No output, outcome or historical member finding was edited or reclassified.

## Exact 6b source audit and mutation qualification

The [actual corrective source audit](evidence/ASMA-8196/replacement-bridges/actual-lsa-6b-audit.txt)
finds no remaining P0/P1/P2 in source `6b3654bca823ac85ed7cc0bb1d6965bc730c4743`,
tree `daaf541fbc0e66d5e960a6a5e1909e348528df3f`. Its
[publication receipt](evidence/ASMA-8196/replacement-bridges/lsa-6b-publication-receipt.json)
preserves the complete actual conditional verdict, including its qualification
requirements. The earlier 3b9 P1 and P2 findings remain unchanged as history.

The exact-source [six-mutant receipt](evidence/ASMA-8196/replacement-bridges/qualified-6b3654bc-mutations/results.json)
records six compiled behavioral failures, all exit 101: daemon bridge ancestry,
daemon trailing abandonment, store bridge ancestry, store unbound-fork refusal,
core transitive bridge retention, and core trailing abandonment. Baseline and
restored runs each pass 49 lifecycle plus three context contracts, four daemon
tests and 24 store contracts, all exit 0. Restored source hashes independently
match the immutable candidate and primary source. This run completed at
`2026-09-26T10:02:57.723400Z`.

All twelve raw logs are unchanged. Six actual mutant patches are encoded with
their decoded hashes to preserve significant whitespace; the untouched original
execution receipt is retained beside the current publication receipt. Source
isolation uses a disposable archive while mutation builds share
`_tools/asma-rs-kontor/target`; the final green suites re-establish that cache.
These results supply source mutation qualification, not deployment or native
continuity evidence. The exact-6b full archive gate subsequently completed exit 0.

The [exact-6b qualification receipt](evidence/ASMA-8196/replacement-bridges/qualified-6b3654bc-source-qualification.json)
binds the source/tree, all changed implementation/test hashes and
[raw full-gate log](evidence/ASMA-8196/replacement-bridges/qualified-6b3654bc-full-gate.txt.json).
It reports 2,915 Rust tests passed across 149 binaries, zero failures, nine
ignored, and 305 console tests passed. Formatting, strict workspace Clippy,
locked workspace tests, audit, deny, frozen install, typecheck and production
audit all passed. The run ended `2026-09-26T10:19:47.371779Z` with actual exit 0.
It used an isolated build target after the earlier gate completed; every check
executed anew, independently of the mutation cache.

The [historical 3b9 qualification receipt](evidence/ASMA-8196/replacement-bridges/historical-3b9eb3a5-source-qualification.json)
and [raw log](evidence/ASMA-8196/replacement-bridges/historical-3b9eb3a5-full-gate.txt.json)
retain that earlier source's 2,912 Rust and 305 console passes. Its independent
audit found the hydration P1 despite those passes. It supplies no qualification
credit to the changed 6b implementation or its new regressions.

## Qualification audit and byte-preserving publication correction

The [actual qualification audit](evidence/ASMA-8196/replacement-bridges/actual-lsa-a488-audit.txt)
and [identity receipt](evidence/ASMA-8196/replacement-bridges/lsa-a488-publication-receipt.json)
confirm source qualification is complete and preserve the publication-only P2.
Both full-gate logs are now base64 JSON carriers: decoding `rawBase64` reproduces
the complete original bytes, including diagnostic whitespace, at the unchanged
decoded hashes. No output or qualification result was normalized or reclassified.
The [original exact-6b receipt](evidence/ASMA-8196/replacement-bridges/original-qualified-6b3654bc-source-qualification.json)
and [original historical-3b receipt](evidence/ASMA-8196/replacement-bridges/original-historical-3b9eb3a5-source-qualification.json)
retain the original raw-log locators and hashes as history. Current receipts name
the encoded carriers and record both carrier and decoded-log hashes. This change
supplies no new execution, deployment, census or workflow acceptance.

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

## Merged deployment and fresh continuity — 2026-09-26

The [actual final publication audit](evidence/ASMA-8196/replacement-bridges/actual-lsa-09f-publication-audit.txt)
with its [identity receipt](evidence/ASMA-8196/replacement-bridges/lsa-09f-publication-receipt.json)
closes the publication-only P2 and supports merging exact head `09f02b24`.
The [merge readback](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/merge-receipt.json)
records PR #276 merged at `e29b895d0f045c499144870a2fa610fc642b6cc0`.
Its [release build receipt](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/build-receipt.json)
and [encoded raw build log](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/release-build-log.json)
record exit 0 from the exact merged-source archive, unchanged executable source
relative to qualified 6b, and the three candidate binary hashes.

The [guarded deployment plan](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/deployment-plan.json),
[actual apply script](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/deployment-apply.py),
and [deployment receipt](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/deployment-receipt.json)
record the controlled restart at `2026-09-26T10:45:33.519752Z`, consistent DB
backup, exact realm, schema 119, `quick_check=ok`, zero foreign-key violations,
unchanged runtime/fleet configuration hashes and preserved ASMA-8098 Done@14.
The [identity checkpoint](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/before-lineage-identity.json)
binds unchanged run/parent/runtime/native/seat identities and ASMA-8188's actual
verification-gate sequence 2 across the restart. No manual SQLite write occurred.

Five fresh read-only Paseo conformance tests passed, zero failed, and the sole
mutating test was filtered; [encoded raw output](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/postdeployment-read-only-live-log.json)
binds exact e29 source and the live 0.9.1 identity. These are new postdeployment
checks, not reused ASMA-8098 or earlier deployment test credit.

The [ASMA-8190 preview before settlement](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/ASMA-8190-preview-before-settle.json)
already returned HTTP 200. [Supported settlement](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/runtime-settle-current-verify.json)
then recorded the current verify run as `waiting_input` at cursor 4868 without
inventing a terminal result or closing its TeamRun. The fresh
[ASMA-8190 preview](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/ASMA-8190-preview.json)
selects only run `01a0cf81-50ec-7822-8f3a-bd60690a9bfe`, native
`a526c5cc-e399-43c9-8a5e-830e5834a27b`, SeatBinding
`01a0b033-1d67-7bc2-b5e4-e5dd3ad9c72b`, generation 1. The cancelled
`5750a5be-8399-4360-a9af-4f9594eac006` is absent; no proposed change exists.
The [current exact native readback](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/current-verify-native.json)
confirms the preserved native is idle, unarchived, in Plan mode.

All thirteen [fresh preview results](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/preview-summary.json)
return HTTP 200, totaling 99 current targets and zero `would_change`.
The [joined current census](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/joined-current-census.json)
and [58 exact-ID native readbacks](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/native-exact-readbacks.json)
find all requested natives and all 56 current project/workspace containers;
43 distinct current seat references have matching names and present parents.
Fifteen of the 43 current seat references are archived. The complete 58-ID
readback also includes 15 historical IDs; 23 of all 58 are archived. These are
naming/identity observations, not live-occupancy claims for archived sessions.
The historical 89 applied targets and later 88-target readback are separate dated
sets. Their before/apply/after/current temporal reconciliation is still pending.

## Remaining acceptance

Source correction, exact-source qualification, independent source/publication
review, PR merge, release build, controlled deployment, supported runtime
observation and thirteen fresh naming previews are complete. The bridge-selection
P1 is corrected end to end without rewriting recorded identities. The [independent deployed-checkpoint audit](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/actual-lsa-deployed-checkpoint.txt)
and its [identity receipt](evidence/ASMA-8196/replacement-bridges/deployed-e29b895d/lsa-deployed-checkpoint-publication-receipt.json)
confirm the bounded deployment and current naming claims. Its archived-count
precision note is clarified above. Historical before/apply/after/current census
reconciliation remains pending. Current run `waiting_input` is freshness evidence,
not ASMA-8196 delivery settlement. ASMA-8196 task/workflow gates, ASMA-8120/8121
acceptance, ASMA-8049/8190 epic acceptance and global Kontor trust remain uncredited.
