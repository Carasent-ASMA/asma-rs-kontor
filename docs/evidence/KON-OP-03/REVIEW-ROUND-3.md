# KON-OP-03 code-review gate — round 3 review notes

Reviewed: `99c1e19` (super `db25cdb`; gitlink `99c1e19` == submodule HEAD), the
three commits on top of `ff09f92`.
Round 1 receipt `01a00d02-…` · round 2 receipt `01a00d69-…`.

Verdict: **rejected** — receipt `01a00deb-0b84-79c3-9d5b-e68ba2581014`, sequence 3.

The remaining gap is one family, nine operations. Everything else round 2 raised
is genuinely closed.

## Mechanical checks — all green

| Check | Result |
| --- | --- |
| `cargo test --workspace` | green — 106 suites, 1354 passed, 0 failed, 8 ignored |
| loopback | green — 134 passed (+11) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean, 0 violations |

## What is now genuinely composed

**CP3 — complete.** `kontor-integrations-asma/src/fleet.rs` is deleted.
`AsmaExecutable` survives only in `process.rs` and `jira.rs`; the Jira
integration was never in this ticket's scope, and the handoff's requirement was
the *fleet* preflight/status/block edge, which is gone. `kontor-accounts` gains
`capacity.rs` and `admission.rs`, and the cooldown mechanic that the crate
previously deferred to `asma fleet` now lives there.

**CP4 — complete.** `DEFAULT_CAPACITY` is `mission_max_in_flight: 7` and
`adaptive.ceiling: 7`, with `lib.rs:795` asserting both and an invariant test
that the ceiling cannot exceed the mission ceiling. `admission_window()` reads
the persisted `AdaptiveAdmissionState` and rebuilds the window from the stored
`AdaptivePosition`, falling back to `AdaptiveWindow::start` only when nothing is
persisted — documented as the one case that legitimately starts fresh. State is
seeded on epic apply (`applications.rs:5065`), advanced on observation
(`:2486`), and the mission ceiling counts `active_team_runs`, not seats.

**CP2 semantic topology — complete and well done.** `resolve_scope` is the
semantic boundary in one place: kind, parent, epic and task are derived from
`self.domain.delivery` and the pinned specification, with no literal kind spelled
in the file. `ensure_scope_chain` looks up every level before creating it, so a
repeat ensure creates nothing.

**Test quality.** The eleven behavioural tests drive real write paths over HTTP
rather than asserting refusal shapes. `a_plan_admits_against_the_width_that_was_learned`
is the strongest: a seeded window admits four, two clean refreshes grow it to
five, and the next plan admits five. The commit message is candid that the first
version of this proof did *not* catch the seeded "fresh window in the snapshot"
mutant, and explains why a `capacity_get` assertion could not observe it. That is
the right way to report mutation testing.

**The Advisor/Committee residual is legitimately out of scope.** An
`AdvisorRunId` or `CommitteeRunId` only exists once OP-05's service opens the
run, so there is no aggregate to scope a topology node to. `resolve_scope`
refuses those two variants with a typed `unavailable` before any effect, which is
exactly what the handoff prescribes for a successor-owned family. No objection.

## Why the gate still rejects

Nine operations still answer `unavailable`, and they are not successor-ticket
contracts — they are the first clause of CP2 and the first table of the handoff's
uniform `/v1` contract:

| Operation | Handoff table |
| --- | --- |
| `draft_topology_spec` | Topology specification, catalog and reference |
| `validate_topology_spec` | ″ |
| `publish_topology_spec` | ″ |
| `topology_spec` | ″ |
| `role_catalog` | ″ |
| `role` | ″ |
| `code_help` | ″ |
| `preview_topology_upgrade` | Semantic topology |
| `apply_topology_upgrade` | ″ |

The handoff's own definition of what OP-03 must make Operational is explicit:

> "topology publication, catalog lookup, code help and semantic topology actions
> have no `/v1` application operations"

Four things. Semantic topology actions are now composed; **publication, catalog
lookup and code help are not**. CP2 names them alongside semantic topology —
"Topology specification/read/upgrade, role catalog/code help and semantic
topology operations" — and the successor-ticket table does not list them, so the
contract-only licence does not reach them.

The practical consequence is a coherence gap, not just a missing endpoint:
`ensure_scope_chain` calls `self.pinned_spec(project_id)?`, so the composed
semantic topology consumes a specification that has no `/v1` path to publish,
read or upgrade. The Admin tier's defining Operational power — deciding what
kinds may ever exist in a project — does not work, and a client still cannot read
the role catalog or code help, which the handoff calls out as precisely the
failure those projections exist to prevent.

## The 14 required negative proofs

Eleven hold, two hold partially, one cannot be exercised.

| # | Proof | State |
| --- | --- | --- |
| 1 | Route absent from `REGISTRY` and the probe list | holds — parity oracle |
| 2 | Observer mutation or wrong minimum tier | holds |
| 3 | Stale revision / replay creating a second effect | **partial** — proven for semantic-topology writes; specification publication refuses |
| 4 | Raw `role`, unknown role code, caller-supplied title | **partial** — raw role closed by `deny_unknown_fields`; unknown role code unprovable while the catalog refuses |
| 5 | Model-authored kind/parent/native id/`cwd`/threshold/pid/argv | holds — closed union plus `resolve_scope` |
| 6 | A published or epic-pinned specification changing in place | **does not hold** — publish and upgrade both refuse |
| 7 | Materialization outside OP-02's exact-id/capability path | holds |
| 8 | Refresh storing only derived state; override rewriting raw evidence | holds — two tests |
| 9 | Production `AsmaExecutable` / fleet store / AgentsRoom description | holds — `fleet.rs` deleted plus a no-executable test |
| 10 | Snapshot resetting the adaptive window to four | holds — mutation-verified |
| 11 | One clean observation growing the window; replay growing it again | holds — mutation-verified |
| 12 | Counting seats instead of TeamRuns; an eighth run; cancelling under pressure | holds |
| 13 | `epic-get` Team revision immediately and after restart | holds — pre-existing |
| 14 | Contract-only successor reporting success | holds |

## A correction I own

My round-2 "to clear this gate" list said "compose the semantic-topology
operations". It should have said the whole of CP2 — the specification, catalog
and code-help family as well, which my own findings section in that same document
listed as refusing. The builder appears to have worked the checklist I wrote. The
gate still turns on the task and the handoff rather than on my checklist, but the
under-specification was mine, not a builder omission.

## To clear this gate

Compose the nine operations above against the OP-01 `ProjectSessionTopologySpec`
and `RoleCatalogRevision` documents the handoff records as already existing, and
add the two proofs that then become exercisable: a published or epic-pinned
specification refusing to change in place, and an unknown role code refusing
rather than being guessed.

If the intent is instead that this family moves to a successor ticket, that is a
legitimate scope decision — but it is the orchestrator's to make, and it needs
the handoff amended, because as written CP2 places it here.
