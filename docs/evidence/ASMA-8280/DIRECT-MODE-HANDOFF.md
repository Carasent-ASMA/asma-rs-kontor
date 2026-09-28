# ASMA-8280 direct-mode handoff — gap matrix, MUT-001 site and launch specification

- Date: 2026-09-28
- Task: ASMA-8280 / TASK-002
- Author: implement seat `462cd93d-30ec-4ed0-9841-3687612da5e0` (parent TPM `3c52abda-4fd0-4af6-b086-476bc51eb305`)
- Workspace: `wks_a284b5d96a0ea32b`
- Branch: `feat/ASMA-8280-share-activated-yaml-policy-across-orchestration-modes` (local, not pushed)
- Reconciled from HEAD `68de02fcb3458516dba3de4e0449ded2ee4cceb5`. Frozen and not rewritten: implementation `1168c17a`, `2a96a650`, `ec68ec5a`; verification `0eee8f63`; bounded audit `68de02fc`.

This record is implementation evidence and a handoff. It is not verification, does not
close TASK-002, and claims no mutation acceptance.

## Goal under reconciliation

> Both orchestration modes resolve all required leadership, delivery and consultation
> slots from the same activated project YAML revision through reused deterministic
> validation and resolution, with matching provenance, explicit eligibility and no live
> effect from unactivated edits.

## Gap matrix

`✓` satisfied by committed source and tests; `◐` partly; `✗` absent.

| Goal element | Governed (Kontor) | Direct (Paseo-direct) | Evidence / remaining work |
| --- | --- | --- | --- |
| One authoring source (`config/orchestration/`) | ✗ | ✗ | No project YAML exists in `asma-modules`. Owner: the `asma-modules` integration writer (TPM). Kontor accepts any v1/v2 document through the registered operations. |
| Registered publication and activation | ✓ | n/a | `/v1/fleet/policy{,:preview,:publish,:activate}`, MCP/CLI `kontor_fleet_policy_*` (`2a96a650`). |
| Immutable history, owner-only pointer, fail-closed reading | ✓ | ◐ | Daemon `FleetSource` (`1168c17a`). The content decision is now shared: `FleetSnapshot::activated` (this slice). A direct reader still performs its own file I/O; see blocker B-1. |
| Reused deterministic parser, validator and resolver | ✓ | ◐ | `kontor-fleet` is the single implementation. The direct side is library-complete (this slice) and has no executable consumer (B-1). |
| Leadership slots (LSA, TPM) | ◐ | ✗ | Governed: the three call sites admit only a caller route the bound chain offers (`ec68ec5a`); the policy does not yet choose the leadership route. Direct: needs a roster source (B-2) and the consumer (B-1). |
| Delivery slots | ✓ | ✗ | Governed: `declared_delivery_rungs` walks the bound chain under quota evidence and records `fleet-decisions/<team_run_id>.jsonl`. Direct: consumer (B-1). |
| Consultation slots (Advisor, Committee) | ✓ | ◐ | Governed: freeze, recovery and native-less reroute read the verified policy. Direct: per-slot `select` exists; joint Committee vendor-distinct allocation is still daemon-only (`select_committee_allocation`), gap G-5. |
| Policy-chosen route | ✓ delivery/consultation, ✗ leadership | ◐ | `FleetResolution::select` returns the first route the stated eligibility admits (this slice). |
| Explicit eligibility | ◐ | ✓ library | `Eligibility` (unavailable accounts, excluded vendors) and `ExclusionReason` for every passed route (this slice). Governed placement derives eligibility from quota evidence, not from this type (G-4). |
| Matching provenance | ✓ records | ✓ library | Delivery `FleetDecision`, leadership `LeadershipDecision`, consultation admission `profile_hash`; library `FleetSelection`. Field mapping in the table under "Launch specification". |
| Policy hash on the consultation launch request | ✓ | ✗ | `ConsultationLaunchRequest.route_provenance` (`crates/kontor-runtime/src/adapter.rs:459`) carries `source = fleet_configuration` and `evidence_hash = fleet.hash()`. It is set at the Advisor freeze (`crates/kontor-daemon/src/applications.rs:12708`) and Committee seat recovery (`:27446`). Committee admission freezes `source` and `profile_hash`, which `consultation_route_provenance` (`:407`) re-creates for each `launch_consultation`. Direct: no consumer (B-1). |
| Policy hash on the consultation native outcome and readback | ✗ | ✗ | `ConsultationLaunchOutcome` (`adapter.rs:495`) carries identity, provider session, time and `created`, and no provenance. The Paseo adapter writes `evidence_hash` to a native label only for an OpenCode route (`label::OPERATOR_ACCEPTED_FALLBACK`, `crates/kontor-runtime-paseo/src/adapter.rs:5360`); every other provider gets `read_only = true` and no hash. Launch readback checks route and permission mode, not the hash (`:5518`). |
| Policy hash on the hosted-leadership launch | ✗ | ✗ | `HostedSeatLaunchRequest` (`adapter.rs:548`) has no provenance field, and its outcome is the same `ConsultationLaunchOutcome`. Correlation is only by `fleet-decisions/leadership/<seat_binding_id>.jsonl` (seat binding plus occupancy generation). |
| Policy hash on the delivery launch | ✗ | ✗ | `LaunchParts` (`crates/kontor-runtime/src/request.rs:269`) has no provenance field. Correlation is only by `fleet-decisions/<team_run_id>.jsonl` (`agent_run_id`, `fleet_hash`). |
| No live effect from unactivated edits | ✓ | ✓ library | Slice-one and slice-two loopbacks; this slice's cross-mode test covers `fleet.yml`, an unactivated publication and a rewritten artifact. |
| Real provider or Paseo launch | ✗ | ✗ | Later supervised effect; specification below. |

### TEST-002 coverage

| TEST-002 clause | Status | Where |
| --- | --- | --- |
| Deterministic same-policy resolution | ✓ | `kontor-fleet` `equal_policy_bytes_*`; daemon `a_direct_reader_of_the_activation_chooses_what_placement_chooses` |
| All seat classes | ✓ library, ◐ runtime | LSA, TPM, delivery, committee and advisor keys in the cross-mode test; governed loopbacks from slices one and two |
| Source hashes | ◐ | Policy content hash throughout. There is no authoring-bundle or source-file hash, because no `config/orchestration/` bundle exists. |
| Activation boundary | ✓ | `only_the_activated_bytes_are_admitted`, daemon activation tests, loopbacks |
| Missing or invalid input | ✓ | Rule tests V-01..V-33, L-01..L-03, P-07, A-01..A-09, block result |
| Old-pin compatibility | ✓ | v1 `fleet.yml` reader unchanged (V-33 applies only to publication); `an_older_roster_revision_keeps_its_own_leadership_binding` |
| No secret material | ✓ | V-33 `a_published_policy_carries_no_credential_path_or_email`; the selection receipt passes `CanonicalDocument` sensitive-material checks |

## Blockers needing a decision

**B-1 — direct-mode transport.** The decision says Paseo-direct "reads the activated
snapshot locally through `asma` and the same library". `asma` is Python and `kontor-fleet`
is Rust. The `kontor` CLI is purely a daemon client, generated from the registry. The
constraints then exclude every obvious path:

- a daemon-backed registered read breaks CON-002 ("Default Paseo-direct needs no Kontor read");
- a standalone binary is a new command outside the registry;
- a Python reimplementation is a second parser.

Options, in order of recommendation:

1. **Registry-declared, CLI-local read (recommended).** Add one registry tool,
   `kontor_fleet_policy_resolve`, whose kind is local. It is listed in `CLI_ONLY` so MCP
   does not advertise it. `kontor` executes it in-process, with no daemon and no
   credentials: it reads `fleet-activation.json` and the named artifact under the
   daemon's file rules, admits them through `FleetSnapshot::activated`, and returns
   `FleetSelection` JSON. This amends the decision that activation I/O lives only in
   `kontor-daemon`: the read-side guards (`read_guarded`, `Guard`, `FleetActivation`,
   `verify_activation`) move to a small daemon-free crate that both the daemon and
   `kontor-cli` link.
2. **Daemon-backed registered read.** Simplest, and reuses asma's `_run_kontor`. Requires
   a CON-002 amendment.
3. **Standalone read-only binary on `kontor-fleet`.** Requires accepting a new executable.

**B-2 — the pinned Core Team revision in direct mode.** A leadership key is
`leadership/<core-team-revision-hash>/<slot>` and needs the canonical roster bytes. A
Paseo-direct epic has no Kontor roster. Recommendation: use the bootstrap roster, which
is `CoreTeamRevision::resolve(SpecVersion::FIRST, <shipped catalog>, &[])` — the same one
`epic_bootstrap_roster` freezes for a project with no published Core Team. The shipped
catalog id is fixed (`01936f5a-1000-7000-8000-000000000002`), so that hash is
deterministic per catalog revision and identical in both modes. The alternative is an
explicit canonical roster document in the authoring bundle. Either way is an LSA
schema/source decision.

## Remaining gaps after the decisions

- G-3: provenance on the native launch, per surface:
  - Consultation: the request already carries the policy hash (`route_provenance.evidence_hash`). Absent: the binding key on the request, the hash on `ConsultationLaunchOutcome`, and a native label or readback of the hash for providers other than OpenCode.
  - Hosted leadership: absent on the request, the outcome and the readback.
  - Delivery: absent on the request, the outcome and the readback.
- G-4: governed leadership should choose its route through `select` when the caller names none, and governed eligibility should be expressible as an `Eligibility`.
- G-5: move Committee vendor-distinct allocation into `kontor-fleet`, so both modes allocate a Committee identically.
- G-6: create the `config/orchestration/` authoring bundle and record source-bundle hashes.

## asma-cli handoff (not written; another checkout)

- **Repository:** `_tools/asma-cli` (`/Users/igor/carasent/asma-modules/_tools/asma-cli`), catalog module `asma-cli`. It is changed only in its own isolated task worktree, created by the plan's owner, on the epic-keyed module integration branch. This seat did not write it.
- **Owner boundary:** a narrow TASK-002 consumer. No parser, no scheduler, no control plane, and no new `asma` command. It extends the existing `asma fleet preflight` report, which already decides launch readiness.
- **Operation contract (under B-1 option 1):** `kontor --state-root <root> fleet-policy-resolve` with `binding_key` (team, committee or advisor) or `leadership` (`{roster, role_slot_id}` under B-2), plus `eligibility` (`{unavailable_accounts: [...], excluded_vendors: [...]}`). It returns `FleetSelection`: `provenance{policy_hash, schema_version, binding_key, chain}`, `eligibility`, `selected{rung{provider, model, effort}, step, sub_step, vendor}` or `null`, `considered[{route, excluded?}]`, and `excluded_by_policy[{step, model, account?, reason}]`.
- **Candidate source:** `src/asma_cli/fleet_policy.py`, with `resolve_seat(state_root, seat, eligibility) -> dict` calling the existing `kontor_publication._run_kontor` helper. Unavailable accounts come from `fleet_availability` records; the excluded vendor comes from the implementer's previous receipt. The launch receipt stores `FleetSelection` verbatim beside the Paseo readback.

## MUT-001 — prepared, not performed

MUT-001 is "unactivated YAML changes next placement". The activation decision has two sites; mutate each separately:

1. `crates/kontor-fleet/src/lib.rs`, `FleetSnapshot::activated` / `FleetSnapshot::published`. Mutant: drop the content-hash equality (`if ContentHash::of(document.as_bytes()) != *hash { return Err(invalid(P07)); }` becomes unconditional acceptance). An edited or unactivated document is then admitted as the activated policy. Expected killers: `kontor-fleet` `only_the_activated_bytes_are_admitted`; daemon `a_direct_reader_of_the_activation_chooses_what_placement_chooses`, `a_tampered_published_policy_fails_closed_without_falling_back`; loopback `an_activated_policy_places_the_next_seat_and_fails_closed_when_unverifiable`.
2. `crates/kontor-daemon/src/fleet.rs`, `FleetSource::policy`: the branch that serves `legacy()` only when `fleet-activation.json` is absent. Mutant: serve `legacy()` unconditionally (the slice-one mutant). Expected killers: `an_activated_policy_replaces_fleet_yml_for_placement`, `an_unactivated_edit_or_publication_has_no_live_effect`, loopback `an_activated_policy_places_the_next_seat_and_fails_closed_when_unverifiable`.

A local preparation check of site 1 only, on the uncommitted slice-three tree, removed the
P-07 equality. The mutant failed:

- `kontor-fleet` `only_the_activated_bytes_are_admitted`;
- daemon `a_tampered_published_policy_fails_closed_without_falling_back` and `a_direct_reader_of_the_activation_chooses_what_placement_chooses`;
- loopback `an_activated_policy_places_the_next_seat_and_fails_closed_when_unverifiable`, `leadership_materialization_is_resolved_through_the_activated_policy`, `a_committee_allocates_from_the_activated_policy_and_fails_closed`, `an_advisor_freezes_from_the_activated_policy_and_fails_closed`, `a_committee_seat_recovery_fails_closed_on_an_unverifiable_activation` and `a_native_less_consultation_reroute_fails_closed_on_an_unverifiable_activation`.

That check only confirms the named killers exist. It is not the supported mutation
evidence. Acceptance still needs the supported mutation run on the reviewed candidate,
and this record claims none.

## Launch specification (later supervised effect)

Preconditions: the source is reviewed; the daemon build is deployed through the owning release workflow to a **dedicated qualification realm**. That means its own state root and its own Paseo workspace — never the live `~/.local/state/kontor/asma` realm. No live policy, pin, provider home or credential is migrated; no receipt is fabricated.

1. `kontor --state-root <qual> --tier admin fleet-policy-preview --document "$(cat policy.yml)"`; then `fleet-policy-publish` and `fleet-policy-activate` (expected active hash omitted); then `fleet-policy-get`. Record `policy_hash` H.
2. For each seat class, launch through its existing registered operation and read the effect back:

| Seat class | Operation (CLI) | Provenance to capture, all naming H |
| --- | --- | --- |
| LSA, TPM | `core-team-materialize` with routes on the bound chain; then `core-team-route-preview` / `core-team-route-apply` | `fleet-decisions/leadership/<seat_binding_id>.jsonl`: `operation, seat_binding_id, occupancy_generation, core_team_revision_hash, role_slot_id, binding_key, fleet_hash, policy_schema_version, chain, step, sub_step, provider, model, effort, vendor`; runtime readback of the native id, generation and observed route |
| Delivery (for example `implement`) | `team-run-seat-fill` | `fleet-decisions/<team_run_id>.jsonl`: `binding_key, role_slot, fleet_hash, step, sub_step, provider, model, effort, vendor, account_profile_id`; launched route readback |
| Advisor | `advisor-run-invoke` | run context `admission.source = fleet_configuration`, `admission.profile_hash = H`; seat route readback |
| Committee reviewers and judge | `committee-run-invoke`; `consultation-seat-recover` | admission `routes[].source`, `routes[].profile_hash = H`; distinct reviewer vendors; readback |
| Direct (after B-1) | asma consumer, then Paseo `create_agent` with the selected provider, model and effort | `FleetSelection` with `provenance.policy_hash = H`; Paseo readback of provider, model and thinking |

3. Negative cases in the same realm: edit `fleet.yml` and publish an unactivated candidate, and confirm no route changes; rewrite the activated artifact, and confirm every class refuses naming P-07 with no native effect.
