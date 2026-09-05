# ASMA-8110 high-change record: gate-verdict consumption and recovery

Date: 2026-09-06
Artifact: `high-change-record`
Task: `ASMA-8110` / `01a07391-328e-74a3-a808-e7b5775c8438`
Phase: `high-change`
TeamRun: `01a07398-b8d2-7363-8dcc-e92c061deffa`
Status: in progress

## Open questions

### OQ-8110-01 — the fence's qualifying role is named inconsistently by scope

Subject: which role's settled turn releases the post-rejection freshness fence.
Attaches to: `ASMA-8110`, and specifically
[`HIGH-SCOPE-RECORD.md`](HIGH-SCOPE-RECORD.md) §4 "Append-only route and
fresh-authoring fence".

Why the state is ambiguous. §4 requires the releasing turn to be "from the
pinned handoff role that owns the edge out of the rejection target". In the
frozen `docs@1` profile the rejection target is `authoring`, and the edge out of
`authoring` is `authoring -> technical-review` with `handoff_role: inspector`.
So read literally, the releasing role is the **inspector** — the reviewer whose
rejection raised the fence. That directly contradicts three other statements in
the same contract: §4's own "a reviewer/auditor turn ... must not release the
fence", mutation case MUT-8110-09, and qualification step 10, which releases the
fence by handing "the preserved **authoring** seat a new bounded turn".

The contradiction is not a reading error. `handoff_role` on `from -> to` names
the role work is handed **to** — `kontor-profiles/src/pack.rs` refuses one that
is "a role the pinned team supplies no slot for", because that role is the one
that must pick the work up. The role that *authors* a phase is therefore named
by the edge leading **into** it, not out of it.

Options seen:

1. Take the edge **into** the rejection target (`edge.to == target`). Names the
   author. For `docs@1`'s `authoring` this is the entry phase, so no inbound edge
   exists and no role is named; the fence then rests on freshness, active
   TeamRun and required artifacts alone.
2. Take the edge **out of** the rejection target verbatim. Names the inspector,
   and would let the rejecting reviewer's own next turn release the fence —
   failing MUT-8110-09 and contradicting qualification step 10.
3. Drop the role condition entirely and fence on artifacts, freshness and run.

Chosen, pending scope's ruling: **option 1**. It is the only reading under which
every acceptance criterion and all ten mutation cases can hold at once, and it
degrades to option 3 exactly where the profile names nobody. The required
artifacts of the rejection-target phase (`draft` for `authoring`) do the
discriminating work in the `docs@1` case either way: an inspector turn carries
`technical-review-notes`, not `draft`, so it cannot release the fence under
option 1 or option 3.

This choice is confined to one expression in
`crates/kontor-daemon/src/applications.rs::rejection_fence_holds` and is a
one-line change if scope rules otherwise. No stored data depends on it: the
route row records no role.

