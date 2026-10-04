Artifact: high-scope-record

# ASMA-8280 high-scope record: shared activated orchestration policy seam

- Date: 2026-09-27
- Task: ASMA-8280
- Branch: `feat/ASMA-8280-share-activated-yaml-policy-across-orchestration-modes`
- Source baseline: `173d399bfd44ec598332b4fe1cdf882af024fe5c`
- Workspace: `wks_a284b5d96a0ea32b`
- Native parent: TPM `3c52abda-4fd0-4af6-b086-476bc51eb305`

## Scope decision

This record freezes the seam for sharing one activated YAML orchestration policy across direct and Kontor-governed orchestration modes. This scope pass changes documentation only. It does not implement policy activation, publication, materialization, routing, or state-root projection.

The future implementation must reuse the ASMA-8255 fleet parser, validator, route flattener, and resolver as one shared implementation. It must not create a second policy interpreter for direct mode or for leadership seats. The existing immutable publication, naming, and identity contracts remain authoritative.

The ASMA-8255 operator exception for Cursor-backed `gpt-5.6-sol` is historical compiled compatibility only. ASMA-8280 must not widen its providers, models, effort levels, eligible slots, activation behavior, or availability, and must not encode that exception as a generally authorable YAML rule.

## Graph and source inventory

The repository code graph was inventoried first from the immutable local codebase-memory graph indexed at `2026-09-26T06:00:07Z`, then checked against the source at the baseline above. The graph identifies `FleetSource::current`, `FleetSnapshot::routes_for`, `FleetSource::record_decision`, and `FleetSource::last_vendor` as the current resolver/publication consumers. It also exposes three leadership paths which still accept or compare explicit routes:

- `Applications::core_team_route_plan`
- `Applications::materialize_core_team`
- `Applications::supersede_core_team_launch_intent`

The intended dependency direction is:

```text
versioned authoring YAML (future)
        |
        v
one shared parser + validator + deterministic resolver
        |                                  |
        |                                  +--> direct-mode local resolution
        v
existing canonical publication boundary
        |
        v
activated content hash + owner-only generated runtime projection (future)
        |
        +--> delivery bindings
        +--> committee/advisor bindings
        +--> leadership bindings (proof required below)
```

Neither the authoring tree nor either future output in this diagram is created by this record.

### Modules to reuse

| Concern | Existing source of truth | Reuse boundary |
| --- | --- | --- |
| Fleet policy schema and deterministic resolution | `crates/kontor-daemon/src/fleet.rs` | Reuse `FleetDocument`, `FleetSnapshot`, model-major/account-order flattening, rule validation, `routes_for`, vendor and independence lookup, and binding-key helpers. Extract the pure layer to a dependency-safe shared library if direct mode cannot depend on the daemon; do not copy it. Preserve the daemon-facing behavior and API. |
| Fleet runtime projection and evidence | `crates/kontor-daemon/src/fleet.rs` | Preserve `FleetSource`, last-valid/error state, history, decision receipts, and `last_vendor` behavior for the generated runtime projection. Authoring YAML is not itself a state-root runtime file. |
| Candidate validation and immutable publication | `crates/kontor-daemon/src/applications.rs` | Reuse the existing judge/hash/publish pattern represented by `judge_candidate`, `candidate_hash`, `judge_team_definition_candidate`, `publish_topology_spec`, `publish_team_definition`, `publish_bundled_topology_revisions`, and `publish_bundled_team_definitions`. Publication must validate the exact bytes later published. |
| Canonical hashes and stable identifiers | `crates/kontor-core/src/id.rs` | Continue to use `CanonicalDocument`, `ContentHash`, `RoleCode`, `RoleSlotId`, `TeamDefinitionId`, and their existing normalization and hashing rules. Do not introduce display-name identity. |
| Published orchestration specifications | `crates/kontor-core/src/spec.rs` | Preserve `ModelRung`, `ModelChainPolicy`, `ProjectSessionTopologySpec`/`TopologySnapshot`, `TeamDefinitionSpec`/`TeamDefinitionSnapshot`, `TeamDefinitionSeatSlot`, and `WorkProfileSpec`. Existing snapshots remain immutable and pin exact canonical bytes. |
| Native hierarchy and rendered names | `crates/kontor-core/src/naming.rs` and `crates/kontor-core/src/spec.rs` | `NativeNameTemplate`, `NativeNameValues`, and the pinned Team Definition remain the only naming authority. Missing tokens fail closed; names are never inferred from policy keys or caller prose. |
| Stable seat occupancy | `crates/kontor-core/src/state.rs` | Preserve `SeatBindingId`, topology node, `RoleSlotId`, frozen role snapshot, parent seat binding, revision, generation, and lifecycle evidence. A policy selection changes route choice, not seat identity. |
| Leadership roster | `crates/kontor-teams/src/operational.rs` and `crates/kontor-teams/src/spec.rs` | Reuse `CoreTeamSeatSelection`, `CoreTeamSeat`, `CoreTeamRevision`, `RoleSlotSpec`, and `TeamTemplateSpec`. Mandatory LSA and TPM membership and the frozen roster remain authoritative. |
| Profile-pack composition | `crates/kontor-profiles/src/pack.rs` | Reuse `ProfilePackSpec` and `OperationalDomainPack` publication and resolution rather than creating a policy-owned shadow copy of topology, Team Definition, or roles. |
| Consultation identity | `crates/kontor-core/src/consultation.rs` | Preserve existing advisor and committee identities and their current fleet binding contracts. |
| Leadership lineage | `crates/kontor-core/src/tpm_lineage.rs` and `Applications::hosted_seat_lineage` | Preserve generation, occupancy, and native-parent lineage across placement. |
| Provider launch/readback | `crates/kontor-runtime/src/adapter.rs` and `crates/kontor-runtime-paseo/src/client.rs` | Use the existing adapter boundary and readback proof. Policy resolution must not bypass provider launch validation or runtime evidence. |
| Durable revisions, pins, bindings, and receipts | `crates/kontor-core/src/repository.rs` and `crates/kontor-store` | Publish and pin through existing repositories and transactions. No in-place mutation of an existing revision or hash is permitted. |

## Existing binding contracts

ASMA-8255 fleet bindings currently cover these exact key families:

- `team/<team_definition_id>/<slot_id>`
- `committee/<committee_id>/<slot>`
- `advisor/<advisor_id>`

The current fleet schema deliberately rejects an invented `core/<role_code>` binding. ASMA-8255 deferred leadership integration because leadership routing is still supplied at the three explicit call sites named above. ASMA-8280 must not silently extend the old schema by treating a role code, title, or native display name as a new key.

For CoreTeam seats, the future binding must start from the pinned CoreTeam roster and its stable `RoleSlotId` plus frozen role snapshot. A `RoleCode` may participate only through the already-validated roster relationship; it cannot replace slot identity or authorize a caller-authored title. LSA and TPM must both be covered.

## Publication and activation boundary

The future authoring source belongs under the versioned `config/orchestration/` tree described by the shared-workflow plan. Its canonical published revision must be content-addressed. Runtime state-root files are generated owner-only projections produced by a supported publisher/materializer; they are not symlinks to checkout files and are not edited by hand.

Activation is a separate, explicit boundary from publication. An unactivated branch switch, dirty YAML edit, or merely published candidate must not alter either direct or governed placement. Both modes must resolve the same activated content hash from the same canonical policy bytes. Direct mode may resolve the activated bundle locally and must not require the daemon merely to interpret policy.

Missing or invalid selected configuration must not partially activate. Any last-valid behavior must be explicit, observable, and tied to an already activated revision. ASMA-8255's current missing-file and deletion behavior remains compatibility for legacy/unmigrated fleet operation; it is not implicit authority to turn a selected shared policy off or to fall back during partial activation.

Policy documents must not contain credentials, provider-home paths, provider-native account identifiers, or live quota state.

## Leadership-binding proof still required

No leadership-binding proof is claimed by this scope record. Implementation and verification must produce one end-to-end proof containing all of the following:

1. An activated policy revision is identified by its canonical content hash, and both direct and governed resolution report that same hash.
2. The leadership lookup begins with the exact pinned CoreTeam revision, stable `RoleSlotId`, and frozen role snapshot; it does not begin with display text or an unverified caller route.
3. The binding resolves LSA and TPM through the shared resolver, including the selected binding key, declared chain, selected rung, provider/model/effort, and policy provenance.
4. Equal inputs produce byte-equivalent resolution results in direct and governed modes, including ordering and error behavior.
5. A missing leadership binding, unknown slot, malformed chain, unavailable domain, or exhausted bound chain fails closed at the declared boundary. It does not fall through to a Team Definition, work profile, CLI default, historical operator exception, or caller-supplied route.
6. `core_team_route_plan`, `materialize_core_team`, and `supersede_core_team_launch_intent` all consume or compare the proved shared resolution. None remains an unproved alternate route-authority path.
7. The selected route is carried through launch and provider readback while preserving `TeamDefinitionId` and version, Team Definition hash, `RoleSlotId`, `SeatBindingId`, parent binding, generation, and native hierarchy/name evidence.
8. The activation revision, decision receipt, and launched/read-back route are durably correlated so an auditor can reconstruct which policy bytes placed the seat.
9. Editing an unactivated source document has no placement effect; activating a new valid hash changes only subsequent eligible placements according to the declared lifecycle contract.
10. A regression test proves the ASMA-8255 historical Cursor/operator exception has not gained a new slot, provider, model, effort, activation path, or YAML-authoring path.

If a new leadership binding namespace is required, it must be introduced as a reviewed, versioned schema successor and mapped to stable slot identity. Reusing the rejected `core/<role_code>` spelling by convention is not sufficient proof.

## Compatibility tests to keep

The following existing contracts are part of the compatibility floor. Names are recorded so future extraction or publication work cannot substitute broader integration coverage for the focused behavior already proven.

### Fleet parser, resolver, and state projection

Keep the focused tests in `crates/kontor-daemon/src/fleet.rs`, including:

- `the_shipped_example_parses`
- `each_rule_in_appendix_b_is_enforced`
- `malformed_yaml_and_unknown_fields_are_document_errors`
- `flattening_is_model_major_then_account`
- `an_unavailable_domain_or_account_is_skipped`
- `calibration_and_vision_rules_drop_models`
- `an_unbound_key_returns_none`
- `an_unknown_vendor_has_no_independence_key`
- `a_listed_route_must_match_the_models_efforts`
- `without_a_fleet_claude_and_codex_map_to_their_vendor`
- `a_missing_file_means_no_fleet`
- `an_edit_is_picked_up_without_restart`
- `an_invalid_edit_keeps_the_last_valid_snapshot`
- `a_group_writable_file_is_refused`
- `a_symlink_is_refused`
- `an_oversized_file_is_refused`
- `a_non_utf8_file_is_refused`
- `each_accepted_hash_is_written_to_history_once`
- `the_status_file_reports_the_last_error`
- `a_recorded_decision_is_the_last_vendor`
- `every_rule_string_is_verbatim_from_appendix_b`

Keep the fleet loopback behavior, including:

- `the_shipped_startup_path_composes_the_configured_fleet`
- `a_realm_with_no_fleet_still_starts_and_says_so`
- `a_misconfigured_fleet_refuses_the_start`
- `a_fleet_binding_routes_a_new_seat_without_restart`
- `an_edit_to_fleet_yml_changes_the_next_placement`
- `an_invalid_edit_keeps_the_previous_fleet`
- `deleting_fleet_yml_restores_template_routing`
- `a_quota_takeover_walks_sub_steps_before_the_next_domain`
- `every_admitted_fleet_placement_is_recorded`
- `a_fleet_bound_committee_seats_reviewers_on_different_vendors`
- `a_fleet_committee_route_launches_cursor_in_plan_mode`
- `a_fleet_bound_advisor_keeps_its_fleet_provenance_through_materialization`
- `a_fleet_bound_reviewer_that_loses_its_provider_recovers_on_the_fleet_chain`
- `without_fleet_yml_committee_allocation_is_unchanged`
- `a_verifier_skips_the_implementers_vendor`
- `independence_fails_closed_when_only_the_implementers_vendor_is_left`
- `an_implementer_placed_before_fleet_yml_does_not_block_its_verifier`

### Leadership and frozen roster

Keep the CoreTeam loopback contracts, including:

- `a_new_epic_reports_leadership_declared_but_not_materialized`
- `a_core_team_is_previewed_applied_and_read_back`
- `a_repeated_core_team_apply_publishes_one_revision`
- `a_core_team_refuses_a_weakened_mandatory_role_and_an_unknown_code`
- `a_core_team_refuses_a_raw_role_and_a_caller_authored_title`
- `quick_roles_are_the_ad_hoc_eligible_core_team_entries`
- `a_later_core_team_edit_leaves_a_promoted_epic_frozen`
- `a_legacy_epic_bootstraps_one_frozen_roster_and_one_leadership_pair`
- `core_team_materialization_proves_the_bound_ecp_before_writing_seats`
- `route_preview_refuses_exact_container_drift_with_node_and_membership_preserved`
- `route_apply_reproves_after_planning_before_retiring_the_live_predecessor`
- `route_apply_threads_the_proved_placement_into_the_retirement`
- `a_launch_intent_supersession_refuses_a_seat_from_another_epic`

### Naming, identity, publication, and old pins

Keep `crates/kontor-core/tests/team_definition_naming.rs`, especially:

- `recommended_team_definition_renders_the_exact_container_and_local_seat_matrix`
- `a_missing_topic_or_local_seat_value_fails_closed`
- `the_snapshot_binds_the_exact_definition_bytes`
- `team_definitions_reject_every_legacy_naming_source`
- `unknown_fields_fail_closed_at_every_nested_definition_level`
- `team_slots_register_delivery_roles_without_inferring_them`
- `alternative_templates_may_register_one_role_code_under_different_slot_ids`
- `a_team_slot_the_seat_template_cannot_render_is_refused`
- `a_duplicate_team_slot_id_is_refused`

Keep the exact native-name rendering and successor-revision contracts, including:

- `every_recommended_container_row_renders_the_exact_contract_bytes`
- `the_separator_is_exact_bullet_bytes_and_never_a_normalized_lookalike`
- `every_local_seat_row_renders_exactly_from_the_pinned_definition`
- `a_required_jira_key_with_no_confirmed_binding_refuses_instead_of_rendering`
- `the_old_token_definition_still_validates_and_renders_unchanged`
- `the_manifest_covers_exactly_the_four_live_pinned_source_revisions`
- `every_recorded_source_document_still_hashes_to_its_published_identity`
- `every_candidate_is_publishable_and_hashes_to_its_recorded_identity`
- `every_candidate_targets_an_unused_version_of_its_own_source_lineage`
- `a_candidate_differs_from_its_source_only_by_version_and_the_agreed_tokens`

Keep profile and publication behavior, including:

- `the_recommended_team_definition_owns_exact_native_names_and_local_seat_labels`
- `control_roles_are_seat_bindings_and_never_topology_kinds`
- `the_lead_architect_slot_cannot_be_filled_by_a_plain_architect`
- `a_vocabulary_is_drafted_validated_published_and_read_back`
- `a_published_specification_cannot_change_in_place`
- `publishing_under_a_stale_revision_writes_nothing`
- `publishing_a_document_the_validation_never_saw_is_refused`

Existing provider-adapter launch/readback and hosted-leadership composition suites also remain required. In particular, a new policy layer cannot replace the exact launch/readback proof with resolver-only assertions.

## Additional verification required with implementation

The compatibility floor above must be supplemented, not rewritten, with focused tests for:

- identical resolver output and diagnostics for direct and governed consumers of one activated revision;
- deterministic canonical bytes and content hash independent of checkout path or orchestration mode;
- publication without activation having no effect;
- atomic activation and rejection of missing, malformed, unknown-field, unsafe-permission, symlinked, oversized, and non-UTF-8 inputs without partial output;
- explicit selected-policy missing behavior and explicit last-valid behavior;
- both leadership slots and every supported delivery/committee/advisor binding class;
- chain exhaustion and unavailable-domain failure at the declared boundary;
- preservation of old published pins, native names, role-slot identity, parent lineage, and live seat occupancy;
- absence of credentials, provider homes, provider-native account IDs, and live quota in published policy artifacts;
- no widening of the historical operator exception.

## Out of scope for this record

- Creating `config/orchestration/` authoring files.
- Choosing or publishing a new schema version.
- Extracting or moving Rust modules.
- Adding a leadership binding namespace.
- Activating a policy revision.
- Writing a state-root projection.
- Materializing, replacing, or retiring any seat.
- Changing provider catalogs, route eligibility, quota behavior, or the historical operator exception.
- Running synchronization from this worktree.

The next implementation step may begin only after its change set names the shared library boundary and the reviewed leadership binding schema, and can point to the end-to-end binding proof defined above.
