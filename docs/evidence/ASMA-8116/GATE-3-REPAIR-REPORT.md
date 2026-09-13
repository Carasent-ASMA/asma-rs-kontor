Artifact: gate-repair-report-round-3

# ASMA-8116 — repair of high-verification gate sequence 3

Gate receipt: `01a095d3-bf0d-77d1-a1c1-cbc66cb0e52f`
Verifier evidence hash: `4ffa5b98c788ac3d61f9d97e919e53934b2b8ed473b0eef8dbf1cd4ecc9fb12c`
Verifier report: `HIGH-VERIFICATION-ROUND-3.md` (commit `290e92f`, preserved)
Repaired candidate: `f183ec3` plus this round.

## F-8116-V10 — the task path continues under the current key

`IdentityDecision::Proceed` carried `current_key` and the task caller discarded
it, so the projection and delegation built *before* observation kept the old
key and every later effect addressed it.

The task plan state is now rebuilt from `current_key` after the decision. The
projection is constructed through a `project_under(key)` closure, so the
delegation used for planning — and the projection retained in the prepared
ticket — name the key Jira reports now. The observation is reused, so this
costs no request and the zero-extra-read invariant is unchanged.

Retained regression: `a_task_rename_between_preview_and_apply_emits_the_current_key`
reads once for the preview, renames the issue between the reads, and reads again
for the apply. It asserts that **no outbound write ever names the superseded
key**, that the stored binding followed the rename, and that applying a freshly
derived plan puts the transition on `/rest/api/3/issue/MOVED-1/transitions`.

Two honest notes on its shape. First, the apply of the *stale* plan is refused
rather than applied, because the approved `projection_hash` no longer matches
once the key moves — that is correct, and the assertion records it instead of
pretending the stale plan proceeds. Second, the plan DTO does not expose the
external key at all, so "the returned projection" is asserted where it is
observable: in the request the next effect is addressed to.

## F-8116-V11 — authority is occurrence-specific

Authority was a permanent edge between two key strings, so a historical A→B row
authorized A→B again whenever the key string A became current. Both ledgers now
carry a monotonic `rename_sequence`, authority records the
`from_rename_sequence` it was issued against, and the guards require that
sequence to equal the binding's current one. The sequence advances as part of
the same transaction, which spends the authority.

Retained cycle regressions, both ledgers:
`a_task_key_cycle_does_not_reactivate_the_authority_of_an_earlier_occurrence`
and `an_epic_key_cycle_does_not_reactivate_the_authority_of_an_earlier_occurrence`
drive A→B→C→A through supported reconciliation only, then prove a raw A→B is
refused while a freshly proven rename from the recurring key still succeeds.

## F-8116-V12 — the stop reason is typed

`Stop` erased why identity processing stopped. It now carries
`IdentityRefusal`, and the task route maps each to its own answer:

| Reason | Response |
| --- | --- |
| `DifferentIssue` | HTTP 409, code `stale_binding` — a permanent contradiction that must never invite a retry |
| `UnprovenIdentity` | code `placement_blocked` — durable fail-closed, distinct from a contradiction |
| `Transient` | code `unavailable` — the connector or repository failure that *should* be retried |

Routing the contradiction through the generic repository conflict was tried and
withdrawn: it flattened to `revision_conflict` with the rule "a persistence rule
refused the write", which tells a caller to re-read and try again — exactly the
wrong advice. The regression asserts status **and** structured code for the
contradiction, and asserts the unproven case gets a different code.

## F-8116-V13 — selectors, re-killed mutants, hygiene

`RenamedIssueJira` now serves the status and route of the workflow that actually
governs the subject: epics get DRAFT `10237` with a route to TO BE GROOMED
`10236`; tasks get In Development `10214` with a route to On hold `10231`, which
is what the bundled task workflow targets for a blocked task. The task fixture
creates its subject `blocked` so a transition is genuinely planned — without a
planned transition a no-effect assertion proves nothing.

M8 and M9 were re-seeded against these corrected fixtures and both **failed the
restored assertions**, i.e. were killed, while the restored code records zero
writes. The mutation account's trailing blank line is removed and
`git diff --check` against `290e92f` is clean.

## Preserved

Zero-extra-read, V8's terminality, all earlier closed findings, `JiraItemCode`
and legacy token/code semantics (`backlog_identity.rs` byte-identical to
`origin/master`). OQ-002, OQ-004 and the ASMA-8117 migration-number collision
keep their existing attribution; no number is guessed and ASMA-8117 is untouched.
`mcp_journey` still fails exactly as OQ-004 predicts.
