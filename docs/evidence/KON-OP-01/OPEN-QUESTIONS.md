# KON-OP-01 / ASMA-7870 — open questions

Date: 2026-08-16
Raised by: OP-01 builder seat
Status: open — raised, not closed here

Recorded rather than silently decided. Each entry states what the builder brief
asked for, what the authoritative plan says, which one was followed, and who
closes it. None of these blocked delivery: the work proceeded on the named
authority of the plan, which the brief itself designates as the authoritative
spec.

---

## OQ-OP-01-1 — Should OP-01 seed `independent_review@1` and `operational_default@1`?

**Closes:** LSA (architectural — ownership of seeded specification data)

**Brief says:** "OP-01 also seeds … `independent_review@1` + `operational_default@1`
(one remediation round)".

**Plan says**, in the KON-OP-01 Implementation paragraph: "**Do not seed
`independent_review@1` or `operational_default@1`**, and do not modify the
Foundation three-seat fixture or any Foundation snapshot; those remain with
OP-05, OP-06 and their declared integration owners."

**Followed:** the plan. Neither was seeded.

**Why:** three independent signals agree against the brief. The plan prohibits it
in the ticket's own Implementation text; the ticket's declared `Owns` list
excludes the Committee and Completion boundaries these documents belong to; and
the brief's own `scope_guard: OP-01-generic-domain-store-only` describes a
generic domain-store scope that a seeded Committee template and Completion
Profile would exceed. §9 Migration also assigns the `independent_review@1`
linkage to Foundation-behaviour preservation, not to OP-01.

**If the LSA rules the other way:** both are additive seed documents plus their
fixture entries; OP-01 would reopen for a follow-up commit. Nothing delivered
here would be rewritten.

---

## OQ-OP-01-2 — Does OP-01 own anything in `kontor-teams`?

**Closes:** LSA (architectural — module ownership)

**Brief says:** the ownership files are "`crates/kontor-core/src/{id,spec,state,repository}.rs`,
**kontor-teams**, kontor-profiles, kontor-store migration + repository".

**Plan says**, in the same ticket's `Owns` field: "It does **not** own
`kontor-teams`, runtime adapters, application services or Foundation
fixtures/snapshots."

**Followed:** the plan. `kontor-teams` was not touched.

**Why:** the plan's `Owns` field is explicit and negative, and nothing this
ticket needed required a change there. OP-CON-003's one-in-flight-ticket-per-
module rule also makes an unnecessary `kontor-teams` edit a collision risk with
another Operational ticket.

**Impact if wrong:** none observed — no required OP-01 behaviour was blocked by
leaving `kontor-teams` alone.

---

## OQ-OP-01-3 — Regenerated Foundation evidence directory (observed drift)

**Closes:** TPM (process — evidence ownership)

The OP-01 acceptance requires that broad check commands leave files outside the
ownership list byte-for-byte unchanged, and that any drift is reported before
handoff. This is that report.

Running `cargo test --workspace` regenerates
`docs/evidence/KON-MVP-18/run-<id>/` — a Foundation-era (KON-MVP-18) pilot
evidence directory, emitted by `crates/kontor-daemon/tests/mcp_journey.rs`. This
run produced `run-b467fdd87cbf3251`, whose own verdict is ACCEPT with 42 pass /
0 fail.

**Left uncommitted.** It is KON-MVP-18's evidence, not OP-01's, and the plan
excludes Foundation fixtures and snapshots from this ticket's ownership. Two
sibling `run-*` directories are tracked in git, so the prevailing convention may
be to commit them; that convention question is the TPM's, not a change OP-01
should make unilaterally.

**No source file outside the OP-01 ownership list was modified.**

---

## OQ-OP-01-4 — OP-REQ-038 is cited but does not exist

**Closes:** TPM (process — requirement register)

The builder brief instructs that unresolved ambiguities be recorded "per
OP-REQ-038 (durable open question; you may raise; LSA closes architectural, TPM
closes process)". The plan's requirement list ends at **OP-REQ-037**; there is no
OP-REQ-038 in
`_docs/ai-orchestration/plans/2026-08-14-23-21-plan-kontor-operational-mvp.md`.

**Followed:** the procedure the brief describes, which is coherent on its own
terms — this document is the durable record, raised and not self-closed, with
architectural and process items attributed separately.

**Ask:** either add OP-REQ-038 to the plan with the wording the brief assumes, or
correct future briefs to cite OP-REQ-030, which already governs missing
capabilities discovered during delivery. Two requirement registers disagreeing
about their own contents is the kind of drift that gets worse quietly.
