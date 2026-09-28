# ASMA-8280 direct-mode handoff — gap matrix, MUT-001 site and launch specification

- Date: 2026-09-28
- Task: ASMA-8280 / TASK-002
- Author: implement seat `462cd93d-30ec-4ed0-9841-3687612da5e0` (parent TPM `3c52abda-4fd0-4af6-b086-476bc51eb305`)
- Workspace: `wks_a284b5d96a0ea32b`
- Branch: `feat/ASMA-8280-share-activated-yaml-policy-across-orchestration-modes` (local, not pushed)
- Reconciled from HEAD `68de02fcb3458516dba3de4e0449ded2ee4cceb5`. Frozen and not rewritten: implementation `1168c17a`, `2a96a650`, `ec68ec5a`; verification `0eee8f63`; bounded audit `68de02fc`.
- Slice four (this revision) is built on accepted `a3bd2f99a5ffa4dcc8bd936550b01b27865f6ba8` and implements the LSA disposition of B-1 and B-2 (plan commit `671d49ca`, section "LSA disposition — direct resolution and roster (2026-09-28)") and G-4. Ancestors `1168c17a`, `2a96a650`, `ec68ec5a`, `0eee8f63`, `68de02fc`, `77722be0`, `b00a3328`, `09e7f5f0` and `a3bd2f99` are unchanged.

This record is implementation evidence and a handoff. It is not verification, does not
close TASK-002, and claims no mutation acceptance.

## Slice four — what is now implemented

| Decision | Source | What it does |
| --- | --- | --- |
| B-1 shared reader | `crates/kontor-fleet-activation/` (new) | The daemon-free, read-only reader: `FleetActivation` (v1 and v2 records), `Guard` and `read_guarded` (regular file, not group/other-writable, owned by the state root's owner, size limit, same inode, UTF-8), `verify_policy`, `verify_roster`, `verify_manifest`, `verify_bundle`, `load`, `Activated::resolve`, `resolve`. Policy bytes are admitted only through `FleetSnapshot::activated`. It depends on `kontor-core`, `kontor-fleet`, `serde` and `serde_json` only, and writes, caches and falls back to nothing. |
| B-1 daemon delegation | `crates/kontor-daemon/src/fleet.rs` | `FleetSource::placement`/`policy`, `recorded_activation`, `verify_published` and the `fleet.yml` read all go through the shared reader and its guards. The daemon keeps only its `fleet.yml` rule texts (F-01..F-06), publication, activation, history writes, the atomic pointer replacement, legacy, last-valid, cache, status and decision logs. |
| B-1 local operation | `crates/kontor-mcp/src/registry.rs`, `dispatch.rs`, `capability.rs`; `crates/kontor-cli/src/main.rs`, `local.rs` | `ToolSpec.execution` is a first-class `Execution::Http { method, path }` or `Execution::Local(LocalOperation)`. `kontor_fleet_policy_resolve` is `Execution::Local(LocalOperation::FleetPolicyResolve)`, admin tier, read. MCP's `tools()` lists only routed operations, and `Dispatcher::call` refuses a local operation by class (`Denied::LocalOperation`, `not_found`) before a request exists. It is not in `CLI_ONLY`. The CLI branches on `tool.local()` before `connect()`: same gate, same `kontor_mcp::validate` schema checks, no base URL (refused if given), no credential. |
| B-2 artifacts | `crates/kontor-fleet-activation/src/lib.rs` | `core-team-history/<core-team-revision-hash>.json` (canonical `CoreTeamRevision`), `orchestration-history/<source-bundle-hash>.json` (canonical `BundleManifest`: `schema_version`, `resolver`, `sources`, `policy_hash`, `policy_schema_version`, `role_catalog{catalog_id, version, content_hash}`, `core_team_revision_hash`). A v2 record names `source_bundle_hash`, `policy_hash`, `policy_schema_version` and `core_team_revision_hash`. Verification order: pointer, manifest (A-11 agreement), policy, roster (C-08 canonical address, L-01/L-02 shape), catalog pin (M-10). |
| B-2 authoring schema and publisher | `crates/kontor-daemon/src/orchestration.rs` (new) | `orchestration.yml` (`schema_version: 1`, `fleet: fleet.yml`, `core_team: teams/core-team.yml`) and `teams/core-team.yml` (`schema_version`, `version`, `role_catalog` pin, ordered `seats[{role_slot_id, role_code, custom_display_name?, presence, ad_hoc_allowed}]`). `resolve_bundle` checks the pin against the realm catalog's canonical hash (O-04), resolves through `CoreTeamRevision::resolve` (O-05), and requires the declared seats to be the resolved seats exactly, in order, mandatory roles included (O-06). `propose_core_team` / `propose_orchestration` are the explicit authoring generator; nothing at runtime calls them. |
| B-2 materialization seams | `crates/kontor-daemon/src/fleet.rs` | `FleetSource::publish_bundle` writes policy, roster and manifest immutably (owner-only, temporary file plus rename, existing artifacts re-verified) and reads the bundle back through `verify_bundle`. `FleetSource::activate_bundle` verifies every named artifact, then replaces the one pointer atomically, fenced by `ActivationFence{policy_hash, source_bundle_hash}`; replaying the standing bundle writes nothing. The v1 `activate` is unchanged and still writes the exact four-field v1 record. |
| B-2 governed consumption | `crates/kontor-daemon/src/applications.rs` `Services::leadership_binding` | Under a v2 activation, a governed leadership launch (materialization, route correction, launch-intent supersession) byte-and-hash-confirms the epic's pinned canonical revision against the selected roster. On a mismatch it is refused `placement_blocked` before any seat or native effect; the pin is not retargeted. Leadership decisions and route evidence add `source_bundle_hash` only under a v2 activation, so v1 rows and preview hashes keep their shape. |
| G-4 | `crates/kontor-api/src/applications.rs` `CoreTeamSeatRouteRequest`, `FleetEligibilityRequest`; `Services::leadership_choice` | A materialization route names exactly one of `model_route` (admitted or refused, never replaced) and `eligibility`. With `eligibility`, the route is `FleetResolution::select` under that explicit `Eligibility`; the decision records it (`eligibility`) with the exact policy, roster, binding and position. No binding, or nothing eligible, refuses before any seat exists. OpenAPI and console types are regenerated. |

## Goal under reconciliation

> Both orchestration modes resolve all required leadership, delivery and consultation
> slots from the same activated project YAML revision through reused deterministic
> validation and resolution, with matching provenance, explicit eligibility and no live
> effect from unactivated edits.

## Gap matrix

`✓` satisfied by committed source and tests; `◐` partly; `✗` absent.

| Goal element | Governed (Kontor) | Direct (Paseo-direct) | Evidence / remaining work |
| --- | --- | --- | --- |
| One authoring source (`config/orchestration/`) | ◐ | ◐ | Slice four: the module schema (`orchestration.yml`, `teams/core-team.yml`), the publisher `resolve_bundle` and the generator `propose_core_team` exist. No root `config/orchestration/` files exist (G-6: ECP checkout only, after schema review), and bundle publication is not yet a registered operation (sub-slice S-1). |
| Registered publication and activation | ✓ v1 policy, ◐ v2 bundle | n/a | v1: `/v1/fleet/policy{,:preview,:publish,:activate}`, MCP/CLI `kontor_fleet_policy_*` (`2a96a650`). v2: `FleetSource::publish_bundle` / `activate_bundle` are daemon seams only (S-1). |
| Immutable history, owner-only pointer, fail-closed reading | ✓ | ✓ | One reader: `kontor-fleet-activation` (`read_guarded`, `load`), used by the daemon's placement path and by the CLI's local operation. |
| Reused deterministic parser, validator and resolver | ✓ | ✓ | `kontor-fleet` is the single implementation; the direct consumer is `kontor fleet-policy-resolve`, which links it through `kontor-fleet-activation`. |
| Leadership slots (LSA, TPM) | ✓ | ✓ v2 only | Governed: a caller route must be on the bound chain (`ec68ec5a`); with no caller route the policy chooses under explicit eligibility (G-4, materialization); under a v2 activation the pinned revision must be the selected roster byte for byte. Direct: `fleet-policy-resolve --binding-key leadership/<hash>/<slot>` resolves only under a v2 activation whose selected roster has that hash (D-01 under v1, D-02 for another revision). |
| Delivery slots | ✓ | ✓ | Governed: `declared_delivery_rungs` walks the bound chain under quota evidence and records `fleet-decisions/<team_run_id>.jsonl`. Direct: `fleet-policy-resolve --binding-key team/<template>/<slot>`. |
| Consultation slots (Advisor, Committee) | ✓ | ◐ | Governed: freeze, recovery and native-less reroute read the verified policy. Direct: per-seat resolution of `advisor/…` and `committee/…/<seat>` keys; joint Committee vendor-distinct allocation is still daemon-only (`select_committee_allocation`), gap G-5. |
| Policy-chosen route | ✓ | ✓ | `FleetResolution::select`: governed delivery and consultation, governed leadership materialization with no caller route (G-4), and every direct resolution. |
| Explicit eligibility | ◐ | ✓ | Direct: `--unavailable-accounts`, `--excluded-vendors`. Governed leadership materialization takes an explicit `eligibility`. Governed delivery and consultation still derive eligibility from quota evidence rather than stating an `Eligibility`. |
| Matching provenance | ✓ records | ✓ | Delivery `FleetDecision`, leadership `LeadershipDecision` (now also `source_bundle_hash` under v2 and `eligibility` for a policy choice), consultation admission `profile_hash`; direct `FleetSelection`. Field mapping in the table under "Launch specification". |
| Policy hash on the consultation launch request | ✓ | ✗ | `ConsultationLaunchRequest.route_provenance` (`crates/kontor-runtime/src/adapter.rs:459`) carries `source = fleet_configuration` and `evidence_hash = fleet.hash()`. It is set at the Advisor freeze (`crates/kontor-daemon/src/applications.rs:12708`) and Committee seat recovery (`:27446`). Committee admission freezes `source` and `profile_hash`, which `consultation_route_provenance` (`:407`) re-creates for each `launch_consultation`. Direct: the resolution now exists (`fleet-policy-resolve`), but no direct launch carries its hash yet; the receipt is the asma-cli sub-slice. |
| Policy hash on the consultation native outcome and readback | ◐ | ✗ | The outcome object carries no provenance: `ConsultationLaunchOutcome` (`adapter.rs:495`) holds only identity, provider session, time and `created`. OpenCode launch readback does check the hash. `consultation_labels` (`crates/kontor-runtime-paseo/src/adapter.rs:5327`) writes `route_provenance.evidence_hash` as `label::OPERATOR_ACCEPTED_FALLBACK` for an OpenCode route (`:5360`). The fresh native readback then calls `verify_agent_placement` (`:5517`, defined at `:2614`), which requires every wanted label through `matches_labels` (`crates/kontor-runtime-paseo/src/wire.rs:925`). An OpenCode seat whose native label lacks that hash is therefore refused. Every other provider gets `read_only = true` and no hash label, so its readback checks no hash. |
| Policy hash on the hosted-leadership launch | ✗ | ✗ | `HostedSeatLaunchRequest` (`adapter.rs:548`) has no provenance field, and its outcome is the same `ConsultationLaunchOutcome`. Correlation is only by `fleet-decisions/leadership/<seat_binding_id>.jsonl` (seat binding plus occupancy generation). |
| Policy hash on the delivery launch | ✗ | ✗ | `LaunchParts` (`crates/kontor-runtime/src/request.rs:269`) has no provenance field. Correlation is only by `fleet-decisions/<team_run_id>.jsonl` (`agent_run_id`, `fleet_hash`). |
| No live effect from unactivated edits | ✓ | ✓ | Slice-one and slice-two loopbacks and the slice-three cross-mode test; slice four: the CLI never reads `fleet.yml` (A-10 with only `fleet.yml` present), and a published but unactivated bundle selects nothing. |
| Real provider or Paseo launch | ✗ | ✗ | Later supervised effect; specification below. |

### TEST-002 coverage

| TEST-002 clause | Status | Where |
| --- | --- | --- |
| Deterministic same-policy resolution | ✓ | `kontor-fleet` `equal_policy_bytes_*`; daemon `a_direct_reader_of_the_activation_chooses_what_placement_chooses`, `an_aligned_bundle_is_published_then_activated_as_one_selection` (governed placement and `kontor_fleet_activation::resolve` agree for every seat class); CLI `an_aligned_bundle_resolves_every_seat_class_without_a_daemon` (the CLI prints the shared reader's selection verbatim) |
| All seat classes | ✓ library and CLI, ◐ runtime | LSA, TPM, delivery, committee and advisor keys through the shared reader and the CLI; governed loopbacks from slices one, two and four |
| Source hashes | ✓ module, ✗ root | `BundleManifest.sources` records the hash of `orchestration.yml`, `fleet.yml` and `teams/core-team.yml` (`the_explicit_proposal_resolves_to_the_exact_mandatory_revision`). No root bundle exists yet (G-6). |
| Activation boundary | ✓ | `only_the_activated_bytes_are_admitted`, daemon activation and bundle tests, `a_bundle_that_does_not_verify_is_never_activated_and_never_served`, loopbacks |
| Missing or invalid input | ✓, ◐ two guards | Rule tests V-01..V-33, L-01..L-03, P-07, A-07..A-11, M-07..M-10, C-07..C-08, D-01..D-03, O-01..O-06, block result; the symlink, writable, oversized and UTF-8 guards (x-01, x-02, x-04, x-06) of the pointer, manifest, policy and roster (`every_named_file_is_read_under_its_own_guard`); CLI `the_local_read_fails_closed_and_never_falls_back`, `the_local_read_keeps_its_tier_its_schema_and_takes_no_base_url`. The owner (x-03) and swapped-file (x-05) guards are implemented but untested for every family: they need a second uid or a race harness. |
| Old-pin compatibility | ✓ | v1 `fleet.yml` reader unchanged (V-33 applies only to publication); `an_older_roster_revision_keeps_its_own_leadership_binding`; v1 record shape `a_schema_version_1_record_keeps_its_exact_shape`; v1 leadership refused directly (D-01) while v1 delivery resolves; governed epics pinned elsewhere keep their pin (`an_aligned_activation_leads_only_the_epic_pinned_to_its_roster`) |
| No secret material | ✓ | V-33 `a_published_policy_carries_no_credential_path_or_email`; the selection receipt passes `CanonicalDocument` sensitive-material checks |

## B-1 and B-2 — decided by the LSA, implemented in slice four

The recommendations this section used to carry were decided on 2026-09-28 (plan
commit `671d49ca`). B-1 took option 1 with one correction: the local operation is a
first-class execution class of the registry, not a `CLI_ONLY` entry. B-2 took the
explicit selected roster, not the bootstrap roster: `CoreTeamRevision::resolve(
SpecVersion::FIRST, <catalog>, &[])` is only the authoring generator. The governed
`epic_bootstrap_roster` fallback for a project with no published Core Team is
unchanged; the decision forbids a *direct-mode* runtime fallback, and none exists.
"What is now implemented" above is the source map.

### Choices made inside the decisions, for review

These are implementation choices this seat made where the decision text was silent.
Each is reversible and none is presented as a substitute decision.

1. **Local operation tier.** `kontor_fleet_policy_resolve` is declared `admin`, like
   the whole `kontor_fleet_policy_*` family, so no existing tier is weakened. The CLI
   enforces it with the dispatcher's own gate before reading. `asma-cli`'s
   `_run_kontor` passes `KONTOR_TIER = "operator"`, so the consumer must call at
   admin or the LSA must lower this declared tier. **Needs an LSA answer before the
   asma-cli sub-slice.**
2. **Local answer shape.** Success is the envelope every command prints,
   `{tool, status: 200, body: FleetSelection}`, so `_run_kontor` reads it unchanged.
   Nothing eligible is `{status: 409, body: {code: "placement_blocked", message,
   selection}}` and exit 1, the same class the CLI already gives the daemon's
   `placement_blocked`. Verification refusals are the CLI's local refusal document
   (`dispatched: false`): A-08, A-10, M-07, C-07, D-01 and D-03 as `not_found` (exit
   6), every other rule as `invalid_request` (exit 2), an I/O failure as
   `unavailable` (exit 5).
3. **Activation fence.** The v2 `activate_bundle` fences on the standing record's
   `(policy_hash, source_bundle_hash)`. The v1 `activate` keeps its policy-hash fence,
   so a v1 activation can still replace a v2 record when the caller names the
   standing policy. That drops aligned leadership coverage, visibly (D-01).
4. **Governed consumption point.** The governed path byte-and-hash-confirms at the
   leadership launch (`leadership_binding`). Governed Core Team publication
   (`core-team-apply`) neither writes nor reads `core-team-history/`. Whether it
   should consume the bundle's roster artifact is left to review (S-3).
5. **Bundle consistency.** The publisher does not refuse a policy whose leadership
   keys name another revision or leave a roster slot unbound. The direct reader
   refuses those keys at resolution (D-02, D-03), and the governed path refuses the
   launch. A publication-time rule is a possible later V-rule.
6. **G-4 scope.** Only materialization can omit a route. Route correction and
   launch-intent supersession name a route by contract and still admit only
   on-chain routes.

## Remaining gaps after slice four

- **S-1: registered bundle operations.** Preview, publish, activate and read for an
  orchestration bundle through the route table, MCP registry and CLI, including
  realm-wide idempotency binding (a store migration, which is a rebase hazard next
  to `0120`). Until then the aligned activation exists only through the daemon seams,
  so a qualification realm cannot be put into v2 through a supported surface.
- **S-2: generator surface.** `propose_core_team` / `propose_orchestration` are
  library functions. Which surface writes the initial `core-team.yml` proposal
  (a registered or local operation, or an `asma` authoring step) is not decided.
- **S-3: governed publication.** See choice 4 above.
- **G-3: provenance on the native launch, per surface:**
  - Consultation: the request already carries the policy hash (`route_provenance.evidence_hash`). Absent: the binding key on the request, the hash on `ConsultationLaunchOutcome`, and a native label or readback of the hash for providers other than OpenCode.
  - Hosted leadership: absent on the request, the outcome and the readback.
  - Delivery: absent on the request, the outcome and the readback.
- **G-4, remaining half:** governed delivery and consultation eligibility is still
  derived from quota evidence rather than stated as an `Eligibility`.
- **G-5: Committee allocation (not attempted; returned as a sub-slice, not a scope
  reduction).** `select_committee_allocation` (`crates/kontor-daemon/src/applications.rs`)
  is a backtracking walk over daemon types (`CommitteeTemplateSpec`,
  `FrozenCommitteeRoute`, `independence_key`'s provider-family fallback). Moving it
  needs a pure allocator in `kontor-fleet` over flattened routes and vendors, and a
  registry decision on how the direct consumer asks for a joint allocation: a
  second local operation, or a multi-key mode of `fleet-policy-resolve`. B-1
  authorises exactly one local operation, so this is an LSA decision.
- **G-6: root `config/orchestration/` files.** Prepared only in the ECP checkout,
  only after this module schema is reviewed, and not activated by being written.
- **asma-cli consumer** (below), **MUT-001 acceptance** (below), **deployment** of a
  daemon and CLI build through the owning release workflow, and the **supervised
  qualification** launch (below). None was performed.

## asma-cli handoff (not written; another checkout)

- **Repository:** `_tools/asma-cli` (`/Users/igor/carasent/asma-modules/_tools/asma-cli`), catalog module `asma-cli`. It is changed only in its own isolated task worktree, created by the plan's owner, on the epic-keyed module integration branch, after this schema is reviewed. This seat did not write it.
- **Owner boundary:** direct-mode input shaping and receipt capture only. No parser, no resolver, no scheduler, no control plane, and no new `asma` command. It extends the existing `asma fleet preflight` report through the existing `kontor_publication._run_kontor`.
- **Operation contract (implemented):** `kontor --state-root <root> --tier admin fleet-policy-resolve --binding-key <key> [--unavailable-accounts '<JSON array>'] [--excluded-vendors '<JSON array>']`. `<key>` is `team/<template>/<slot>`, `committee/<template>/<seat>`, `advisor/<profile>` or `leadership/<core-team-revision-hash>/<role-slot-id>`, where the hash is the `core_team_revision_hash` of the v2 activation record. The answer is `{tool, status, body}`: `status: 200` with `body` = `FleetSelection` (`provenance{policy_hash, schema_version, binding_key, chain}`, `eligibility`, `selected{rung{provider, model, effort}, step, sub_step, vendor}`, `considered[{route, excluded?}]`, `excluded_by_policy[...]`), or `status: 409` `placement_blocked` with the `selection`, or a local refusal (exit codes under choice 2 above). Store `body` verbatim beside the Paseo readback; never launch on anything but `status: 200`.
- **Tier:** see choice 1 above. `_run_kontor` currently passes `operator`, which this operation refuses.
- **Candidate source:** `src/asma_cli/fleet_policy.py`, with `resolve_seat(state_root, key, eligibility) -> dict` calling `_run_kontor`. Unavailable accounts come from `fleet_availability` records; excluded vendors come from the implementer's previous receipt.

## MUT-001 — prepared, not performed

MUT-001 is "unactivated YAML changes next placement". The activation decision now has
four sites; mutate each separately:

1. `crates/kontor-fleet/src/lib.rs`, `FleetSnapshot::activated` / `FleetSnapshot::published`. Mutant: drop the content-hash equality (`if ContentHash::of(document.as_bytes()) != *hash { return Err(invalid(P07)); }` becomes unconditional acceptance). Expected killers: `kontor-fleet` `only_the_activated_bytes_are_admitted`; `kontor-fleet-activation` `a_rewritten_or_non_canonical_artifact_fails_closed`; daemon `a_direct_reader_of_the_activation_chooses_what_placement_chooses`, `a_tampered_published_policy_fails_closed_without_falling_back`; loopback `an_activated_policy_places_the_next_seat_and_fails_closed_when_unverifiable`.
2. `crates/kontor-daemon/src/fleet.rs`, `FleetSource::placement`: the branch that serves `legacy()` only when `fleet-activation.json` is absent. Mutant: serve `legacy()` unconditionally. Expected killers: `an_activated_policy_replaces_fleet_yml_for_placement`, `an_unactivated_edit_or_publication_has_no_live_effect`, `a_bundle_that_does_not_verify_is_never_activated_and_never_served`, loopback `an_activated_policy_places_the_next_seat_and_fails_closed_when_unverifiable`.
3. `crates/kontor-fleet-activation/src/lib.rs`, `load`: the manifest agreement (A-11) and `verify_contents`. Mutants: accept a disagreeing record; skip the roster or catalog-pin check. Expected killers: `a_record_that_disagrees_with_its_manifest_fails_closed`, `a_missing_named_artifact_fails_closed`, `a_manifest_pinning_another_role_catalog_fails_closed`; CLI `the_local_read_fails_closed_and_never_falls_back`.
4. `crates/kontor-daemon/src/applications.rs`, `Services::leadership_binding`: the selected-roster confirmation. Mutant: skip it. Expected killer: loopback `an_aligned_activation_leads_only_the_epic_pinned_to_its_roster`.

Local preparation checks, which only confirm that the named killers exist:

- Slice three, site 1 on the uncommitted tree (P-07 equality removed). Failed: `kontor-fleet` `only_the_activated_bytes_are_admitted`; daemon `a_tampered_published_policy_fails_closed_without_falling_back` and `a_direct_reader_of_the_activation_chooses_what_placement_chooses`; loopback `an_activated_policy_places_the_next_seat_and_fails_closed_when_unverifiable`, `leadership_materialization_is_resolved_through_the_activated_policy`, `a_committee_allocates_from_the_activated_policy_and_fails_closed`, `an_advisor_freezes_from_the_activated_policy_and_fails_closed`, `a_committee_seat_recovery_fails_closed_on_an_unverifiable_activation` and `a_native_less_consultation_reroute_fails_closed_on_an_unverifiable_activation`.
- Slice four, site 4 on the uncommitted tree (confirmation short-circuited with `&& false`). Failed: `an_aligned_activation_leads_only_the_epic_pinned_to_its_roster`, which got 200 where it requires 409. The source was restored and touched.

Neither check is the supported mutation evidence. Acceptance still needs the
supported mutation run on the reviewed candidate, and this record claims none.

## Launch specification (later supervised effect)

Preconditions: the source is reviewed; the daemon build is deployed through the owning release workflow to a **dedicated qualification realm**. That means its own state root and its own Paseo workspace — never the live `~/.local/state/kontor/asma` realm. No live policy, pin, provider home or credential is migrated; no receipt is fabricated.

1. `kontor --state-root <qual> --tier admin fleet-policy-preview --document "$(cat policy.yml)"`; then `fleet-policy-publish` and `fleet-policy-activate` (expected active hash omitted); then `fleet-policy-get`. Record `policy_hash` H. This is a v1 activation: it covers governed leadership, delivery and consultation and direct delivery and consultation, but not direct leadership (D-01). An aligned v2 activation — bundle hash, policy H and roster hash — needs S-1 before it can be made through a supported surface.
2. For each seat class, launch through its existing registered operation and read the effect back:

| Seat class | Operation (CLI) | Provenance to capture, all naming H |
| --- | --- | --- |
| LSA, TPM | `core-team-materialize` with routes on the bound chain; then `core-team-route-preview` / `core-team-route-apply` | `fleet-decisions/leadership/<seat_binding_id>.jsonl`: `operation, seat_binding_id, occupancy_generation, core_team_revision_hash, role_slot_id, binding_key, fleet_hash, policy_schema_version, chain, step, sub_step, provider, model, effort, vendor`; runtime readback of the native id, generation and observed route |
| Delivery (for example `implement`) | `team-run-seat-fill` | `fleet-decisions/<team_run_id>.jsonl`: `binding_key, role_slot, fleet_hash, step, sub_step, provider, model, effort, vendor, account_profile_id`; launched route readback |
| Advisor | `advisor-run-invoke` | run context `admission.source = fleet_configuration`, `admission.profile_hash = H`; seat route readback |
| Committee reviewers and judge | `committee-run-invoke`; `consultation-seat-recover` | admission `routes[].source`, `routes[].profile_hash = H`; distinct reviewer vendors; readback |
| Direct | `kontor --state-root <qual> --tier admin fleet-policy-resolve --binding-key <key>` (through the asma consumer once it exists), then Paseo `create_agent` with the selected provider, model and effort | `FleetSelection` with `provenance.policy_hash = H`; Paseo readback of provider, model and thinking. Direct leadership keys need a v2 activation, so they wait on S-1. |

3. Negative cases in the same realm: edit `fleet.yml` and publish an unactivated candidate, and confirm no route changes; rewrite the activated artifact, and confirm every class refuses naming P-07 with no native effect.
