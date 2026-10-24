//! Synthetic inputs exercising the production read-only preview core.
//!
//! No selected-folder parser, authentication, activation, store or runtime is
//! exercised here. The proposal in `_docs/plans/2026-10-23-15-45-proposal-composition-snapshot.md`
//! separates these reusable component checks from the dependent service contract.

use kontor_core::id::{
    BoundedText, CanonicalDocument, ContentHash, ExternalId, ExternalName, IdempotencyKey,
    RoleCode, RoleSlotId, SCHEMA_VERSION, parse_utc_timestamp,
};
use kontor_core::naming::{
    NameSeparator, NativeNameSegment, NativeNameTemplate, NativeNameToken, NativeNameValues,
};
use kontor_daemon::composition_snapshot::{
    CompositionInput, CompositionSnapshot, CompositionSubject, preview_composition,
};
use kontor_fleet::{Eligibility, FleetSnapshot};
use kontor_profiles::pack::{
    PackCategoryKey, ProfilePackSpec, RolePrompt, parse_pack, resolve_profile,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// These existing neutral fixtures are compile-time test inputs, never folders
// accepted as runtime configuration. No WP08 candidate source is included.
const PROFILE: &str = include_str!("../../kontor-profiles/tests/fixtures/custom-pack-a.json");
const FLEET: &str = include_str!("../../../config/examples/orchestration/fleet.yml");
const FLEET_BINDING: &str = "team/01936f5a-0000-7000-8000-000000000001/implement";

fn fixture() -> (CompositionInput, ProfilePackSpec, FleetSnapshot) {
    let request = CompositionInput {
        subject: CompositionSubject {
            project: ExternalId::parse("synthetic-project-a").unwrap(),
            workspace: ExternalId::parse("synthetic-workspace-a").unwrap(),
            parent: Some(ExternalId::parse("synthetic-parent-a").unwrap()),
            slot: RoleSlotId::parse("synthetic-slot-a").unwrap(),
        },
        category: PackCategoryKey::parse("alpha-cat").unwrap(),
        fleet_binding: FLEET_BINDING.to_owned(),
        template: NativeNameTemplate::from_segments(vec![
            NativeNameSegment::Token(NativeNameToken::RoleCode),
            NativeNameSegment::Token(NativeNameToken::Topic),
        ])
        .unwrap(),
        separator: NameSeparator::parse(" · ").unwrap(),
        values: NativeNameValues::new()
            .with_role_code("RESEARCH")
            .with_topic("Synthetic topic"),
        persona: RolePrompt {
            role_code: RoleCode::parse("RESEARCH").unwrap(),
            prompt: BoundedText::parse("Synthetic persona for contract probes only.").unwrap(),
        },
        eligibility: Eligibility::default(),
        resolved_at: parse_utc_timestamp("2026-01-01T00:00:00Z").unwrap(),
        source_pins: Default::default(),
        policy_pins: Default::default(),
    };
    (
        request,
        parse_pack(PROFILE).unwrap(),
        FleetSnapshot::parse_policy(FLEET).unwrap(),
    )
}

fn body(snapshot: &CompositionSnapshot) -> Value {
    snapshot.document().deserialize().unwrap()
}

#[test]
fn preview_is_repeatable_and_never_claims_actuation_or_effective_loading() {
    let (request, profiles, fleet) = fixture();
    let first = preview_composition(&request, &profiles, &fleet).unwrap();
    let second = preview_composition(&request, &profiles, &fleet).unwrap();
    assert_eq!(first.document(), second.document());
    let value = body(&first);
    assert_eq!(value["naming"]["title"], "RESEARCH · Synthetic topic");
    assert_eq!(value["native_actuation_authorized"], false);
    assert_eq!(value["effective_loading"], "not_observed");
    assert_eq!(value["coverage"], "profile_fleet_naming_projection");
    assert_eq!(value["profile"]["skills"][0]["skill"], "alpha-s1");
    assert_eq!(value["attachments"]["persona"]["state"], "planned");
    assert_eq!(value["attachments"]["skills"]["state"], "planned");
    assert_eq!(
        value["attachments"]["skills"]["references"],
        value["profile"]["skills"]
    );
    assert!(value["attachments"]["persona"].get("prompt").is_none());
    assert!(!value["fleet"]["selected"].is_null());
}

#[test]
fn supplied_source_and_policy_pins_are_exactly_bound_but_never_activate_a_preview() {
    let (mut input, profiles, fleet) = fixture();
    let absent = preview_composition(&input, &profiles, &fleet).unwrap();
    assert_eq!(body(&absent)["source_pins"], json!({}));
    assert_eq!(body(&absent)["policy_pins"], json!({}));
    input.source_pins.insert(
        ExternalId::parse("synthetic-source").unwrap(),
        ContentHash::of(b"synthetic-source-bytes"),
    );
    let source = preview_composition(&input, &profiles, &fleet).unwrap();
    input.policy_pins.insert(
        ExternalId::parse("synthetic-policy").unwrap(),
        ContentHash::of(b"synthetic-policy-revision"),
    );
    let policy = preview_composition(&input, &profiles, &fleet).unwrap();
    assert_ne!(absent.hash(), source.hash());
    assert_ne!(source.hash(), policy.hash());
    let value = body(&policy);
    assert_eq!(
        value["source_pins"],
        serde_json::to_value(&input.source_pins).unwrap()
    );
    assert_eq!(
        value["policy_pins"],
        serde_json::to_value(&input.policy_pins).unwrap()
    );
    assert!(
        value["unverified"]
            .as_array()
            .unwrap()
            .contains(&json!("supplied_pins_and_activation"))
    );
    assert_eq!(value["native_actuation_authorized"], false);
    assert_eq!(value["effective_loading"], "not_observed");
    input
        .policy_pins
        .values_mut()
        .for_each(|pin| *pin = ContentHash::of(b"changed-policy"));
    assert_ne!(
        policy.hash(),
        preview_composition(&input, &profiles, &fleet)
            .unwrap()
            .hash()
    );
}

#[test]
fn evaluation_time_and_component_changes_cannot_rewrite_an_existing_snapshot() {
    let (mut input, profiles, fleet) = fixture();
    let original = preview_composition(&input, &profiles, &fleet).unwrap();
    let original_document = original.document().clone();
    input.resolved_at = parse_utc_timestamp("2026-01-01T00:00:01Z").unwrap();
    let later = preview_composition(&input, &profiles, &fleet).unwrap();
    assert_ne!(original.hash(), later.hash());
    assert_eq!(original.document(), &original_document);
    for flag in [
        "selected_folder_capture",
        "native_identity",
        "topology_roster_role_join",
        "persona_catalog_binding",
        "host_route_qualification",
        "skill_content",
        "caller_authority",
    ] {
        assert!(
            body(&later)["unverified"]
                .as_array()
                .unwrap()
                .contains(&json!(flag)),
            "{flag}"
        );
    }
}

#[test]
fn exact_native_context_changes_the_hash_even_when_the_title_is_identical() {
    let (request, profiles, fleet) = fixture();
    let original = preview_composition(&request, &profiles, &fleet).unwrap();
    let mut changed = [
        request.clone(),
        request.clone(),
        request.clone(),
        request.clone(),
    ];
    changed[0].subject.project = ExternalId::parse("synthetic-project-b").unwrap();
    changed[1].subject.workspace = ExternalId::parse("synthetic-workspace-b").unwrap();
    changed[2].subject.parent = None;
    changed[3].subject.slot = RoleSlotId::parse("synthetic-slot-b").unwrap();
    for input in &changed {
        let other = preview_composition(input, &profiles, &fleet).unwrap();
        assert_eq!(
            body(&original)["naming"]["title"],
            body(&other)["naming"]["title"]
        );
        assert_ne!(original.hash(), other.hash());
    }
}

#[test]
fn profile_owner_refuses_missing_skill_revisions_and_tampered_bundles() {
    let (request, mut profiles, fleet) = fixture();
    let mut resolved = resolve_profile(&profiles, &request.category, request.resolved_at).unwrap();
    resolved.skills.clear();
    assert!(resolved.verify().is_err());
    profiles.skills.clear();
    assert!(preview_composition(&request, &profiles, &fleet).is_err());
}

#[test]
fn profile_and_persona_edits_change_the_snapshot_without_changing_the_prior_value() {
    let (mut request, mut profiles, fleet) = fixture();
    let original = preview_composition(&request, &profiles, &fleet).unwrap();
    let original_hash = original.hash().clone();
    profiles.skills[0].label = ExternalName::parse("Revised synthetic skill").unwrap();
    let revised = preview_composition(&request, &profiles, &fleet).unwrap();
    assert_ne!(original_hash, *revised.hash());
    request.persona.prompt = BoundedText::parse("Revised synthetic persona.").unwrap();
    let revised_persona = preview_composition(&request, &profiles, &fleet).unwrap();
    assert_ne!(revised.hash(), revised_persona.hash());
    assert_eq!(*original.hash(), original_hash);
}

#[test]
fn fleet_owner_refuses_content_hash_mismatch_and_preserves_no_route() {
    let (mut request, profiles, fleet) = fixture();
    assert!(FleetSnapshot::published(&ContentHash::of(b"different policy"), FLEET).is_err());
    let available = preview_composition(&request, &profiles, &fleet).unwrap();
    request.eligibility.unavailable_accounts = fleet
        .resolve(FLEET_BINDING)
        .unwrap()
        .routes
        .iter()
        .map(|route| route.rung.provider.0.clone())
        .collect();
    let blocked = preview_composition(&request, &profiles, &fleet).unwrap();
    assert!(body(&blocked)["fleet"]["selected"].is_null());
    assert_eq!(body(&blocked)["native_actuation_authorized"], false);
    assert_ne!(available.hash(), blocked.hash());
}

#[test]
fn absent_profile_binding_or_name_token_is_not_guessed() {
    let (request, profiles, fleet) = fixture();
    let mut changed = request.clone();
    changed.category = PackCategoryKey::parse("unconfigured-category").unwrap();
    assert!(preview_composition(&changed, &profiles, &fleet).is_err());
    changed = request.clone();
    changed.fleet_binding = "unconfigured-binding".to_owned();
    assert!(preview_composition(&changed, &profiles, &fleet).is_err());
    changed = request;
    changed.values = NativeNameValues::new().with_role_code("RESEARCH");
    assert!(preview_composition(&changed, &profiles, &fleet).is_err());
}

/// Proposed body only. Transport identity and server-held grant bindings are
/// deliberately absent. Deserialization here is not endpoint authentication.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CreateSeatBodyProbe {
    grant_reference: ExternalId,
    idempotency_key: IdempotencyKey,
    expected_composition_hash: ContentHash,
    expected_subject_revision: ExternalId,
    slot: RoleSlotId,
}

#[test]
fn create_body_shape_cannot_supply_authority_or_launch_overrides() {
    let (request, profiles, fleet) = fixture();
    let snapshot = preview_composition(&request, &profiles, &fleet).unwrap();
    let value = serde_json::to_value(CreateSeatBodyProbe {
        grant_reference: ExternalId::parse("synthetic-grant-reference").unwrap(),
        idempotency_key: IdempotencyKey::parse("synthetic-create-key").unwrap(),
        expected_composition_hash: snapshot.hash().clone(),
        expected_subject_revision: ExternalId::parse("synthetic-observation-a").unwrap(),
        slot: request.subject.slot,
    })
    .unwrap();
    assert!(serde_json::from_value::<CreateSeatBodyProbe>(value.clone()).is_ok());
    for field in value.as_object().unwrap().keys() {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<CreateSeatBodyProbe>(missing).is_err());
    }
    for field in [
        "issuer",
        "principal",
        "verifier",
        "realm",
        "host",
        "operation_id",
        "project",
        "workspace",
        "role",
        "caller",
        "parent",
        "pins",
        "expiry",
        "revocation_generation",
        "delegation",
        "launch",
        "persona",
        "skills",
    ] {
        let mut override_body = value.clone();
        override_body[field] = json!("synthetic-override");
        assert!(
            serde_json::from_value::<CreateSeatBodyProbe>(override_body).is_err(),
            "{field}"
        );
    }
}

#[test]
fn synthetic_grant_binding_covers_one_operation_and_its_exact_context() {
    let (request, profiles, fleet) = fixture();
    let snapshot = preview_composition(&request, &profiles, &fleet).unwrap();
    // A proposed server-held record shape, not an issued or authenticated grant.
    // No attempt ID is included: retries belong to this one operation identity.
    let binding = json!({
        "schema_version": SCHEMA_VERSION,
        "contract": "grant-binding-shape-probe@1",
        "grant_id": "synthetic-grant-a",
        "operation_id": "synthetic-operation-a",
        "operation_kind": "create_one_seat",
        "issuer_principal": "synthetic-issuer-a",
        "delegation_revision": ContentHash::of(b"synthetic-delegation"),
        "transport_principal": "synthetic-adapter-a",
        "verifier": "synthetic-verifier-a",
        "realm": "synthetic-realm-a",
        "host": "synthetic-host-a",
        "registered_project": "synthetic-registration-a",
        "subject": &request.subject,
        "role": &request.persona.role_code,
        "caller": "synthetic-caller-a",
        "subject_revision": "synthetic-observation-a",
        "intent_hash": ContentHash::of(b"synthetic-intent-not-a-live-capture"),
        "composition_hash": snapshot.hash(),
        "pins": {
            "profile": CanonicalDocument::from_serializable(&profiles).unwrap().hash(),
            "fleet": fleet.hash(),
            "activation": ContentHash::of(b"synthetic-activation"),
        },
        "expires_at": parse_utc_timestamp("2026-01-01T00:01:00Z").unwrap(),
        "revocation_generation": 7,
    });
    let original = CanonicalDocument::from_value(&binding).unwrap();
    assert_eq!(binding["operation_kind"], "create_one_seat");
    assert!(binding.get("attempt_id").is_none());
    // Required binding inventory. This checks digest coverage only; mismatches
    // must still be refused by the future verifier against server-held state.
    for path in [
        "/grant_id",
        "/operation_id",
        "/operation_kind",
        "/issuer_principal",
        "/delegation_revision",
        "/transport_principal",
        "/verifier",
        "/realm",
        "/host",
        "/registered_project",
        "/subject/project",
        "/subject/workspace",
        "/subject/slot",
        "/subject/parent",
        "/role",
        "/caller",
        "/subject_revision",
        "/intent_hash",
        "/composition_hash",
        "/pins/profile",
        "/pins/fleet",
        "/pins/activation",
        "/expires_at",
        "/revocation_generation",
    ] {
        let mut incomplete = binding.clone();
        let field = incomplete
            .pointer_mut(path)
            .expect("required binding field");
        assert!(!field.is_null(), "{path}");
        *field = Value::Null;
        assert_ne!(
            original.hash(),
            CanonicalDocument::from_value(&incomplete).unwrap().hash(),
            "{path}"
        );
    }
    assert_eq!(original, CanonicalDocument::from_value(&binding).unwrap());
}
