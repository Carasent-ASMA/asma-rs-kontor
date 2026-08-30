//! Specification, identity and canonical-document verification.
//!
//! The mutants this suite exists to kill:
//!
//! * branching on a particular profile, phase or gate id;
//! * accepting a cyclic, unreachable or dead-ended phase graph;
//! * accepting a gate with no evaluator authority, or one whose waiver authority
//!   is the same as its evaluator authority;
//! * accepting a persona that can approve its own scenario, or one pointing at
//!   production or real identity;
//! * accepting an unbounded trigger or an auto-arm without a capability;
//! * letting a duplicate source event create a second work graph;
//! * accepting a document whose canonical bytes or digest changed.

use std::collections::BTreeSet;

use kontor_core::DomainError;
use kontor_core::calendar::{
    CalendarProfileSpec, CalendarResolution, EffectiveCalendarState, HolidayMergePolicy,
    IanaTimeZone, Weekday, WeeklyWindow, WorkCalendarAssignment, resolve_effective_state,
    validate_windows,
};
use kontor_core::compaction::CompactionStatus;
use kontor_core::id::{
    AccountProfileId, ArtifactKey, CanonicalDocument, CommandReceiptId, ContentHash, CurrencyCode,
    ExternalName, GateKey, IdempotencyKey, IntakeReceiptId, ModuleKey, Money, PhaseKey, ProjectId,
    RoleKey, SchemaVersion, SkillKey, SourceEventId, SpecVersion, TaskId, Timestamp,
    WorkProfileKey, parse_utc_timestamp, validate_module_key, validate_open_key,
};
use kontor_core::id::{AggregateRevision, CalendarProfileId, SCHEMA_VERSION, WorkCalendarId};
use kontor_core::spec::ProjectSessionTopologySpec;
use kontor_core::spec::{
    ApprovalReceipt, AutoArmPolicy, BudgetBounds, ContextCapabilityResult, ContextClamp,
    ContextEnforcement, ContextPolicyInputs, ContextPolicySnapshot, ContextPolicySource,
    ContextWindowBounds, ContextWindowClass, ContextWindowPolicy, DedupExpression,
    EffectiveContextPolicy, EffortLevel, EnvironmentKind, IntakeReceipt, IntakeResult, JsonPointer,
    ModelChainPolicy, ModelRef, ModelRung, PersonaScenarioSnapshot, PersonaScenarioSpec, PhaseEdge,
    ProposedWorkGraph, ProviderQuotaKind, ProviderQuotaSource, ProviderRef, RequestedContextPolicy,
    ResolvedWorkProfileSnapshot, RoleContextSeed, Shareability, ShareabilityClass,
    ShareabilityClassifier, ShareabilityProvenance, ShareabilityTier, TeamContextPolicySeed,
    TriggerSpec, WorkProfileSpec, resolve_context_window,
};
use proptest::prelude::*;

const ARBITRARY_PROFILE: &str = include_str!("fixtures/work_profile_arbitrary.json");
const PERSONA: &str = include_str!("fixtures/persona_scenario.json");
const TRIGGER: &str = include_str!("fixtures/trigger.json");

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("fixture timestamp is canonical UTC")
}

fn profile() -> WorkProfileSpec {
    serde_json::from_str(ARBITRARY_PROFILE).expect("the arbitrary profile fixture parses")
}

fn persona() -> PersonaScenarioSpec {
    serde_json::from_str(PERSONA).expect("the persona fixture parses")
}

fn trigger() -> TriggerSpec {
    serde_json::from_str(TRIGGER).expect("the trigger fixture parses")
}

fn phase(id: &str) -> PhaseKey {
    PhaseKey::parse(id).expect("valid phase key")
}

fn native_topology(value: serde_json::Value) -> Result<ProjectSessionTopologySpec, String> {
    serde_json::from_value::<ProjectSessionTopologySpec>(value)
        .map_err(|error| error.to_string())
        .and_then(|spec| {
            spec.validate()
                .map(|()| spec)
                .map_err(|error| error.to_string())
        })
}

fn minimal_native_topology() -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "spec_id": "01936f5a-1000-7000-8000-000000000001",
        "version": 1,
        "name": "Naming contract",
        "name_separator": " • ",
        "root_kind": "PSW",
        "node_kinds": [{
            "kind": "PSW",
            "allowed_parents": [],
            "cardinality": {"minimum": 1, "maximum": 1},
            "projection_capabilities": ["native_root", "session_host"],
            "read_only": false,
            "name_template": {"segments": [{"kind": "literal", "value": "Project Session Workspace"}]},
            "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
            "code_help": {
                "full_name": "Project Session Workspace",
                "meaning": "One naming validation root.",
                "category": "session_topology",
                "lifecycle": "current"
            }
        }],
        "historical_codes": []
    })
}

#[test]
fn topology_naming_requires_typed_container_and_hosted_seat_templates() {
    let valid = minimal_native_topology();
    native_topology(valid.clone()).expect("the complete typed document validates");

    let mut missing_container = valid.clone();
    missing_container["node_kinds"][0]
        .as_object_mut()
        .expect("a node object")
        .remove("name_template");
    assert!(native_topology(missing_container).is_err());

    let mut missing_seat = valid;
    missing_seat["node_kinds"][0]
        .as_object_mut()
        .expect("a node object")
        .remove("seat_name_template");
    assert!(native_topology(missing_seat).is_err());
}

#[test]
fn topology_naming_rejects_legacy_unknown_empty_duplicate_and_punctuated_templates() {
    let mut legacy = minimal_native_topology();
    legacy["node_kinds"][0]["name_template"] = serde_json::json!("Project Session Workspace");
    assert!(
        native_topology(legacy).is_err(),
        "legacy strings are read but not valid for publish"
    );

    let mut unknown = minimal_native_topology();
    unknown["node_kinds"][0]["name_template"] = serde_json::json!({
        "segments": [{"kind": "token", "value": "SOMETHING_INFERRED"}]
    });
    assert!(native_topology(unknown).is_err());

    let mut empty = minimal_native_topology();
    empty["node_kinds"][0]["name_template"] = serde_json::json!({"segments": []});
    assert!(native_topology(empty).is_err());

    let mut duplicate = minimal_native_topology();
    duplicate["node_kinds"][0]["name_template"] = serde_json::json!({
        "segments": [
            {"kind": "token", "value": "AREA_CODE"},
            {"kind": "token", "value": "AREA_CODE"}
        ]
    });
    assert!(native_topology(duplicate).is_err());

    for punctuation in ["legacy · literal", "embedded • literal"] {
        let mut punctuated = minimal_native_topology();
        punctuated["node_kinds"][0]["name_template"] = serde_json::json!({
            "segments": [{"kind": "literal", "value": punctuation}]
        });
        assert!(native_topology(punctuated).is_err());
    }
}

#[test]
fn topology_naming_accepts_versioned_bullets_while_rejecting_invalid_separators() {
    for separator in [" • ", " · "] {
        let mut document = minimal_native_topology();
        document["name_separator"] = serde_json::json!(separator);
        assert!(
            native_topology(document).is_ok(),
            "`{}` must remain a valid versioned separator",
            separator.escape_debug()
        );
    }

    for separator in ["", "   ", "\n"] {
        let mut document = minimal_native_topology();
        document["name_separator"] = serde_json::json!(separator);
        assert!(
            native_topology(document).is_err(),
            "`{}` must be refused",
            separator.escape_debug()
        );
    }
}

// ---------------------------------------------------------------------------
// Open keys and external names
// ---------------------------------------------------------------------------

#[test]
fn internal_keys_follow_one_conservative_rule() {
    for accepted in [
        "a",
        "q7.delivery",
        "ux-ui-layout",
        "code",
        "0abc",
        "a.b_c-d",
        &"x".repeat(128),
    ] {
        validate_open_key("test", accepted).unwrap_or_else(|_| panic!("`{accepted}` is legal"));
    }
    for rejected in [
        "",
        " leading",
        "trailing ",
        "Upper",
        ".dot-first",
        "-dash-first",
        "_underscore-first",
        "with space",
        "with/slash",
        "with\\backslash",
        "with\ttab",
        "with\nnewline",
        "ünicode",
        &"x".repeat(129),
    ] {
        assert!(
            validate_open_key("test", rejected).is_err(),
            "`{}` must be rejected",
            rejected.escape_debug()
        );
    }
}

#[test]
fn module_keys_round_trip_canonical_repository_paths() {
    for accepted in [
        "editor/asma-bunjs-editor",
        "_tools/asma-rs-kontor",
        "_tools/cdc_cli",
        "shared/asma-ui-core",
        "code",
        "q7.delivery",
        "a/b/c/d",
        "0abc/1def",
        // The whole-key limit is inclusive, and `tasks.module_key` is
        // `CHECK (length(module_key) BETWEEN 1 AND 128)`: a path of exactly 128
        // characters is the longest module the store can hold, so it must parse.
        &format!("{}/{}", "a".repeat(63), "b".repeat(64)),
    ] {
        let key =
            ModuleKey::parse(accepted).unwrap_or_else(|_| panic!("`{accepted}` is a legal module"));
        // Byte-for-byte: a module key that normalized its own spelling would
        // hand the collision lock a different name than the `mod:` tag carries.
        assert_eq!(key.as_str(), accepted, "`{accepted}` must round-trip");
        assert_eq!(
            key.to_string(),
            accepted,
            "`{accepted}` must display as itself"
        );
        let json = serde_json::to_string(&key).expect("a module key serializes");
        assert_eq!(json, format!("\"{accepted}\""));
        let back: ModuleKey = serde_json::from_str(&json).expect("a module key deserializes");
        assert_eq!(back, key);
    }
}

#[test]
fn module_keys_contend_across_slash_and_dotted_holdout_spellings() {
    let slash = ModuleKey::parse("shared/asma-core-helpers").expect("canonical path");
    let dotted = ModuleKey::parse("shared.asma-core-helpers").expect("holdout spelling");
    assert_eq!(slash.contention_identity(), "shared.asma-core-helpers");
    assert_eq!(dotted.contention_identity(), "shared.asma-core-helpers");
    assert!(slash.contends_with(&dotted));
    assert!(dotted.contends_with(&slash));
    // Replacing every `.` with `/` would invent `shared/asma/core/helpers`.
    assert!(!slash.contends_with(&ModuleKey::parse("editor/asma-app-editor").expect("other")));
    assert_eq!(
        ModuleKey::parse("a/b/c")
            .expect("three segments")
            .contention_identity(),
        "a.b.c"
    );
}

#[test]
fn module_keys_refuse_every_non_canonical_path_spelling() {
    for rejected in [
        // A place named two ways is two locks on one place.
        "/editor/asma-bunjs-editor",
        "editor/asma-bunjs-editor/",
        "editor//asma-bunjs-editor",
        "./editor",
        "editor/./asma-bunjs-editor",
        "editor/../editor",
        "..",
        ".",
        "editor\\asma-bunjs-editor",
        "/",
        // Everything the shared open-key rule already refused stays refused.
        "",
        " leading/x",
        "trailing/x ",
        "Editor/asma-bunjs-editor",
        // Uppercase is non-canonical wherever it falls, not only in the position
        // the shared rule happens to check first.
        "editor/asmaBunjsEditor",
        "editor/asma bunjs editor",
        "-dash/x",
        ".hidden/x",
        "editor/\tasma",
        "editor/\nasma",
        "ünicode/x",
        &"x".repeat(129),
        &format!("editor/{}", "x".repeat(129)),
        // The limit is on the whole key, not on each segment: `tasks.module_key`
        // is `CHECK (length(module_key) BETWEEN 1 AND 128)`, so a per-segment
        // length check would let this through validation and fail the insert as
        // a backend error instead of a typed refusal.
        &format!("{}/{}", "a".repeat(64), "b".repeat(64)),
    ] {
        assert!(
            ModuleKey::parse(rejected).is_err(),
            "`{}` must be rejected",
            rejected.escape_debug()
        );
    }

    // The path rule keeps the secret screen its siblings have. A token is
    // lexically a fine path segment, so only the screen refuses it.
    let token = "ghp_0123456789abcdefghijklmnopqrstuvwxyz";
    for spelling in [token.to_owned(), format!("shared/{token}")] {
        assert!(
            matches!(
                ModuleKey::parse(&spelling),
                Err(DomainError::SensitiveMaterial { .. })
            ),
            "a credential must not pass as a module path"
        );
    }
}

#[test]
fn only_the_module_key_carries_a_path_separator() {
    // The relaxation is one key wide. A mutant that moved it into
    // `validate_open_key`, or that pointed a sibling key at the module rule,
    // dies on these.
    assert!(validate_open_key("test", "editor/asma-bunjs-editor").is_err());
    assert!(validate_open_key("test", "_tools").is_err());
    for parsed in [
        WorkProfileKey::parse("editor/asma-bunjs-editor").is_ok(),
        RoleKey::parse("editor/asma-bunjs-editor").is_ok(),
        SkillKey::parse("editor/asma-bunjs-editor").is_ok(),
        PhaseKey::parse("editor/asma-bunjs-editor").is_ok(),
        GateKey::parse("editor/asma-bunjs-editor").is_ok(),
        ArtifactKey::parse("editor/asma-bunjs-editor").is_ok(),
        WorkProfileKey::parse("_tools").is_ok(),
        RoleKey::parse("_tools").is_ok(),
    ] {
        assert!(!parsed, "only `ModuleKey` may spell a repository path");
    }
    // And the module rule is a *relaxation*, not a replacement: everything the
    // shared rule accepts as a single segment is still a legal module.
    for accepted in ["a", "q7.delivery", "ux-ui-layout", "0abc", "a.b_c-d"] {
        validate_module_key("test", accepted)
            .unwrap_or_else(|_| panic!("`{accepted}` is still a legal module"));
    }
}

#[test]
fn external_names_are_unicode_and_may_contain_spaces() {
    for accepted in ["In Development", "Til kontroll", "På vent", "QA Design"] {
        ExternalName::parse(accepted).unwrap_or_else(|_| panic!("`{accepted}` is a legal name"));
    }
    for rejected in ["", " leading", "trailing ", "with\u{0}null"] {
        assert!(
            ExternalName::parse(rejected).is_err(),
            "`{}` must be rejected",
            rejected.escape_debug()
        );
    }
}

#[test]
fn entity_ids_must_be_canonical_version_seven_uuids() {
    let generated = ProjectId::generate();
    let text = generated.to_string();
    assert_eq!(
        ProjectId::parse(&text).expect("a generated id round-trips"),
        generated
    );
    assert_eq!(generated.as_uuid().get_version_num(), 7);

    for rejected in [
        "",
        "not-a-uuid",
        // Version 4, not 7.
        "3f2504e0-4f89-41d3-9a0c-0305e82c3301",
        // Uppercase is not the canonical text form.
        &text.to_uppercase(),
        // Braced and simple spellings are not canonical either.
        &format!("{{{text}}}"),
        &text.replace('-', ""),
    ] {
        assert!(
            ProjectId::parse(rejected).is_err(),
            "`{rejected}` must be rejected"
        );
    }
}

#[test]
fn timestamps_must_already_be_canonical_utc() {
    parse_utc_timestamp("2026-08-09T10:00:00Z").expect("canonical UTC is accepted");
    for rejected in [
        "2026-08-09T12:00:00+02:00",
        "2026-08-09T10:00:00z",
        "2026-08-09 10:00:00Z",
        "2026-08-09T10:00:00.000Z",
        "not a time",
    ] {
        assert!(
            parse_utc_timestamp(rejected).is_err(),
            "`{rejected}` must be rejected rather than normalized"
        );
    }
}

// ---------------------------------------------------------------------------
// Canonical documents
// ---------------------------------------------------------------------------

#[test]
fn canonical_form_is_key_order_and_line_ending_independent() {
    let first = serde_json::json!({
        "schema_version": 1,
        "b": [3, 1, 2],
        "a": { "z": "one\r\ntwo", "y": true }
    });
    let second = serde_json::json!({
        "a": { "y": true, "z": "one\ntwo" },
        "schema_version": 1,
        "b": [3, 1, 2]
    });
    let first = CanonicalDocument::from_value(&first).expect("canonicalizes");
    let second = CanonicalDocument::from_value(&second).expect("canonicalizes");

    assert_eq!(first.json(), second.json());
    assert_eq!(first.hash(), second.hash());
    // Array order is content, not formatting.
    assert!(first.json().contains("[3,1,2]"));
    assert!(!first.json().contains('\r'));
}

#[test]
fn a_document_must_declare_a_schema_version_this_binary_understands() {
    assert!(CanonicalDocument::from_value(&serde_json::json!({ "a": 1 })).is_err());
    assert!(CanonicalDocument::from_value(&serde_json::json!(["a"])).is_err());
    assert!(CanonicalDocument::from_value(&serde_json::json!({ "schema_version": 0 })).is_err());
    assert!(CanonicalDocument::from_value(&serde_json::json!({ "schema_version": 2 })).is_err());
    let document = CanonicalDocument::from_value(&serde_json::json!({ "schema_version": 1 }))
        .expect("schema v1 is accepted");
    assert_eq!(document.schema_version(), SchemaVersion::parse(1).unwrap());
}

#[test]
fn a_stored_document_is_re_verified_against_its_bytes_and_digest() {
    let document = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1, "a": 1, "b": 2
    }))
    .expect("canonicalizes");

    CanonicalDocument::from_stored(document.json(), document.hash())
        .expect("the stored form re-admits");

    // Same content, non-canonical byte order.
    assert!(
        CanonicalDocument::from_stored(r#"{"b":2,"a":1,"schema_version":1}"#, document.hash())
            .is_err(),
        "non-canonical bytes must be refused"
    );
    // Canonical bytes, wrong digest.
    let wrong = ContentHash::of(b"something else");
    assert!(
        CanonicalDocument::from_stored(document.json(), &wrong).is_err(),
        "a mismatched digest must be refused"
    );
}

#[test]
fn documents_that_carry_credentials_or_identity_are_refused_before_sql() {
    let secret = "ghp_thisisnotarealtokenvalue0000000000";
    let value = serde_json::json!({
        "schema_version": 1,
        "connection": { "api_key": secret }
    });
    let error = CanonicalDocument::from_value(&value).expect_err("a credential must be refused");
    match &error {
        DomainError::SensitiveMaterial { path } => {
            assert_eq!(path, "connection.api_key");
        }
        other => panic!("expected sensitive-material rejection, got {other:?}"),
    }
    // The value itself must never reach an error message or an assertion.
    assert!(
        !error.to_string().contains(secret),
        "the rejected value must not be echoed"
    );

    let pem = serde_json::json!({
        "schema_version": 1,
        "attachment": "-----BEGIN RSA PRIVATE KEY-----\nabc\n"
    });
    assert!(CanonicalDocument::from_value(&pem).is_err());

    let identity = serde_json::json!({ "schema_version": 1, "personnummer": "01019012345" });
    assert!(CanonicalDocument::from_value(&identity).is_err());

    // Legitimate names that merely contain a forbidden word are unaffected.
    let budget = serde_json::json!({
        "schema_version": 1, "token_budget": 100, "max_tokens": 5, "cookie_policy": "none"
    });
    CanonicalDocument::from_value(&budget).expect("bounded budgets are not credentials");
}

#[test]
fn canonicalization_is_bounded() {
    let mut deep = serde_json::json!({ "schema_version": 1 });
    for _ in 0..64 {
        deep = serde_json::json!({ "schema_version": 1, "next": deep });
    }
    assert!(
        CanonicalDocument::from_value(&deep).is_err(),
        "an over-nested document must be refused"
    );
}

// ---------------------------------------------------------------------------
// Work profiles
// ---------------------------------------------------------------------------

#[test]
fn the_arbitrary_fixture_profile_validates_and_hashes_stably() {
    let spec = profile();
    spec.validate().expect("the fixture profile is valid");
    let first = spec.canonicalize().expect("canonicalizes");
    let second = spec.canonicalize().expect("canonicalizes");
    assert_eq!(first.hash(), second.hash());

    let snapshot =
        ResolvedWorkProfileSnapshot::resolve(&spec, at("2026-08-09T09:00:00Z")).expect("resolves");
    snapshot.verify().expect("the snapshot matches its digest");

    // The snapshot round-trips through JSON unchanged.
    let json = serde_json::to_string(&snapshot).expect("serializes");
    let restored: ResolvedWorkProfileSnapshot = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(restored, snapshot);
    restored
        .verify()
        .expect("the restored snapshot still verifies");
}

#[test]
fn renaming_every_profile_and_phase_id_changes_nothing_but_the_hash() {
    let original = profile();
    let mut renamed = original.clone();
    renamed.id = WorkProfileKey::parse("ux-ui-layout").expect("a seed-shaped name is just data");

    let rename = |key: &PhaseKey| phase(&format!("zz-{}", key.as_str().replace('.', "-")));
    for spec in &mut renamed.phases {
        spec.id = rename(&spec.id);
        spec.rejection_route = spec.rejection_route.as_ref().map(rename);
    }
    for edge in &mut renamed.edges {
        edge.from = rename(&edge.from);
        edge.to = rename(&edge.to);
    }
    for artifact in &mut renamed.artifacts {
        artifact.producer_phase = rename(&artifact.producer_phase);
    }
    for gate in &mut renamed.gates {
        gate.phase = rename(&gate.phase);
        gate.rejection_target = rename(&gate.rejection_target);
    }
    renamed.entry_phase = rename(&original.entry_phase);
    renamed.terminal_phases = original.terminal_phases.iter().map(rename).collect();

    renamed
        .validate()
        .expect("validation is structural, not name-based");
    assert_ne!(
        renamed.canonicalize().expect("canonicalizes").hash(),
        original.canonicalize().expect("canonicalizes").hash(),
        "different content must hash differently"
    );
}

#[test]
fn an_invalid_phase_graph_is_refused() {
    // A self edge.
    let mut spec = profile();
    spec.edges.push(PhaseEdge {
        from: phase("q7.shape"),
        to: phase("q7.shape"),
        handoff_role: None,
    });
    assert!(spec.validate().is_err(), "a self edge must be refused");

    // A duplicate edge.
    let mut spec = profile();
    spec.edges.push(PhaseEdge {
        from: phase("q7.capture"),
        to: phase("q7.shape"),
        handoff_role: None,
    });
    assert!(spec.validate().is_err(), "a duplicate edge must be refused");

    // A cycle in the forward graph.
    let mut spec = profile();
    spec.edges.push(PhaseEdge {
        from: phase("q7.attest"),
        to: phase("q7.shape"),
        handoff_role: None,
    });
    assert!(spec.validate().is_err(), "a cycle must be refused");

    // An unreachable phase.
    let mut spec = profile();
    spec.edges.retain(|edge| edge.from != phase("q7.capture"));
    assert!(
        spec.validate().is_err(),
        "an unreachable phase must be refused"
    );

    // No terminal declaration at all.
    let mut spec = profile();
    spec.terminal_phases.clear();
    assert!(
        spec.validate().is_err(),
        "a missing terminal must be refused"
    );

    // A sink that is not declared terminal.
    let mut spec = profile();
    spec.phases.push(kontor_core::spec::PhaseSpec {
        id: phase("q7.dangling"),
        label: ExternalName::parse("Dangling").expect("valid label"),
        required_artifacts: Vec::new(),
        gates: Vec::new(),
        rejection_route: None,
    });
    spec.edges.push(PhaseEdge {
        from: phase("q7.shape"),
        to: phase("q7.dangling"),
        handoff_role: None,
    });
    assert!(
        spec.validate().is_err(),
        "an undeclared sink must be refused"
    );

    // An entry phase with an incoming edge.
    let mut spec = profile();
    spec.edges.push(PhaseEdge {
        from: phase("q7.shape"),
        to: phase("q7.capture"),
        handoff_role: None,
    });
    assert!(
        spec.validate().is_err(),
        "an entry with a predecessor must be refused"
    );

    // A duplicate phase id.
    let mut spec = profile();
    let duplicate = spec.phases[0].clone();
    spec.phases.push(duplicate);
    assert!(
        spec.validate().is_err(),
        "a duplicate phase must be refused"
    );
}

#[test]
fn a_rejection_route_must_target_a_strict_ancestor() {
    // Its own phase.
    let mut spec = profile();
    spec.gates[0].rejection_target = phase("q7.attest");
    assert!(spec.validate().is_err(), "self rejection must be refused");

    // A descendant rather than an ancestor.
    let mut spec = profile();
    spec.gates[0].rejection_target = phase("q7.settle");
    assert!(
        spec.validate().is_err(),
        "a non-ancestor rejection must be refused"
    );

    // A phase rejection route with the same rule.
    let mut spec = profile();
    spec.phases[1].rejection_route = Some(phase("q7.settle"));
    assert!(spec.validate().is_err());

    // A rejection route is not a forward edge, so it never makes the DAG cyclic.
    profile()
        .validate()
        .expect("the fixture rejects backwards and still validates");
}

#[test]
fn a_gate_must_have_authority_and_a_distinct_waiver_authority() {
    let mut spec = profile();
    spec.gates[0].evaluator_roles.clear();
    let error = spec
        .validate()
        .expect_err("an authority-free gate must be refused");
    assert!(matches!(error, DomainError::MissingAuthority { .. }));

    let mut spec = profile();
    spec.gates[0].waiver_roles = spec.gates[0].evaluator_roles.clone();
    let error = spec.validate().expect_err("self-waiver must be refused");
    assert!(matches!(error, DomainError::MissingAuthority { .. }));

    let mut spec = profile();
    spec.gates[0].waiver_allowed = true;
    spec.gates[0].waiver_roles.clear();
    assert!(
        spec.validate().is_err(),
        "a waivable gate needs an authority"
    );

    let mut spec = profile();
    spec.gates[0].evaluator_roles = vec![RoleKey::parse("role.unknown").expect("valid role")];
    assert!(
        spec.validate().is_err(),
        "an undeclared role must be refused"
    );
}

#[test]
fn a_required_artifact_must_have_a_producing_evidence_contract() {
    let mut spec = profile();
    spec.phases[0].required_artifacts =
        vec![ArtifactKey::parse("artifact.absent").expect("valid key")];
    assert!(
        spec.validate().is_err(),
        "a required artifact with no contract must be refused"
    );

    let mut spec = profile();
    spec.artifacts[0].evidence_required = false;
    let error = spec
        .validate()
        .expect_err("a required artifact needs an evidence contract");
    assert!(matches!(error, DomainError::MissingEvidence { .. }));

    let mut spec = profile();
    spec.gates[0].required_evidence = vec![ArtifactKey::parse("artifact.absent").unwrap()];
    assert!(spec.validate().is_err());
}

#[test]
fn budget_bounds_are_never_open_ended() {
    let mut spec = profile();
    spec.budget_defaults.max_tokens = 0;
    assert!(spec.validate().is_err(), "a zero bound is not a bound");
}

#[test]
fn unconstrained_budget_is_positive_and_distinct_from_a_stated_ceiling() {
    let unbounded = BudgetBounds::unconstrained();
    unbounded
        .validate()
        .expect("the omitted-arm sentinel must pass positive-value validation");
    assert!(unbounded.is_unconstrained());
    let stated = BudgetBounds {
        max_tokens: 1_000,
        max_commands: 10,
        max_duration_seconds: 60,
        max_cost: Money {
            minor_units: 1,
            currency: CurrencyCode::parse("NOK").unwrap(),
        },
    };
    assert!(!stated.is_unconstrained());
    // Currencies differ (NOK vs XXX), so `within` is false; auto-arm skips it
    // when the grant is unconstrained rather than treating that as BudgetExceeded.
    assert!(!stated.within(&unbounded));
}

// ---------------------------------------------------------------------------
// Property: any well-formed chain of arbitrary names validates
// ---------------------------------------------------------------------------

/// Build a linear profile of `length` phases with entirely arbitrary ids.
fn linear_profile(prefix: &str, length: usize) -> WorkProfileSpec {
    let mut spec = profile();
    let ids: Vec<PhaseKey> = (0..length)
        .map(|index| phase(&format!("{prefix}.p{index}")))
        .collect();

    spec.phases = ids
        .iter()
        .map(|id| kontor_core::spec::PhaseSpec {
            id: id.clone(),
            label: ExternalName::parse("Phase").expect("valid label"),
            required_artifacts: Vec::new(),
            gates: Vec::new(),
            rejection_route: None,
        })
        .collect();
    spec.edges = ids
        .windows(2)
        .map(|pair| PhaseEdge {
            from: pair[0].clone(),
            to: pair[1].clone(),
            handoff_role: None,
        })
        .collect();
    spec.entry_phase = ids[0].clone();
    spec.terminal_phases = vec![ids[length - 1].clone()];
    spec.artifacts.clear();
    spec.gates.clear();
    spec
}

proptest! {
    /// Any chain of arbitrary, legal ids validates, and its hash depends only on
    /// its content — never on which names happen to look like seed data.
    #[test]
    fn arbitrary_linear_profiles_validate(
        prefix in "[a-z][a-z0-9]{0,8}",
        length in 2usize..12,
    ) {
        let spec = linear_profile(&prefix, length);
        prop_assert!(spec.validate().is_ok());

        let document = spec.canonicalize().expect("canonicalizes");
        let restored: WorkProfileSpec =
            serde_json::from_str(document.json()).expect("round-trips");
        prop_assert_eq!(&restored, &spec);
        let recomputed = restored.canonicalize().expect("canonicalizes");
        prop_assert_eq!(recomputed.hash(), document.hash());
    }

    /// Closing the chain into a ring is always refused, at any length.
    #[test]
    fn any_cycle_is_refused(prefix in "[a-z][a-z0-9]{0,8}", length in 2usize..12) {
        let mut spec = linear_profile(&prefix, length);
        let first = spec.entry_phase.clone();
        let last = spec.terminal_phases[0].clone();
        spec.edges.push(PhaseEdge { from: last, to: first, handoff_role: None });
        prop_assert!(spec.validate().is_err());
    }
}

// ---------------------------------------------------------------------------
// Persona scenarios
// ---------------------------------------------------------------------------

#[test]
fn the_persona_fixture_validates_and_freezes() {
    let spec = persona();
    spec.validate().expect("the fixture persona is valid");
    let snapshot = PersonaScenarioSnapshot::freeze(&spec).expect("freezes");
    assert_eq!(
        &snapshot.definition_hash,
        spec.canonicalize().expect("canonicalizes").hash()
    );
}

#[test]
fn a_persona_can_never_approve_its_own_scenario() {
    let mut spec = persona();
    spec.evaluator_roles = vec![spec.actor_role.clone()];
    let error = spec
        .validate()
        .expect_err("an actor must not evaluate itself");
    assert!(matches!(error, DomainError::MissingAuthority { .. }));

    let mut spec = persona();
    spec.evaluator_roles.clear();
    let error = spec
        .validate()
        .expect_err("an independent evaluator is required");
    assert!(matches!(error, DomainError::MissingAuthority { .. }));
}

#[test]
fn a_persona_may_not_reference_production_or_real_identity() {
    let mut spec = persona();
    spec.environment.kind = EnvironmentKind::Production;
    assert!(spec.validate().is_err(), "production must be refused");

    let mut spec = persona();
    spec.identity.seeded = false;
    assert!(
        spec.validate().is_err(),
        "a non-seeded identity must be refused"
    );

    let mut spec = persona();
    spec.required_evidence.clear();
    assert!(spec.validate().is_err(), "evidence is mandatory");

    // A characteristics document carrying a credential cannot even be built.
    let credential = serde_json::json!({
        "schema_version": 1,
        "login": { "password": "hunter2" }
    });
    assert!(CanonicalDocument::from_value(&credential).is_err());
}

#[test]
fn persona_steps_must_be_consecutively_numbered() {
    let mut spec = persona();
    spec.steps[1].order = 5;
    assert!(spec.validate().is_err());

    let mut spec = persona();
    spec.steps.clear();
    assert!(spec.validate().is_err());
}

// ---------------------------------------------------------------------------
// Triggers and intake
// ---------------------------------------------------------------------------

#[test]
fn the_trigger_fixture_validates_and_matches_deterministically() {
    let spec = trigger();
    spec.validate().expect("the fixture trigger is valid");

    let envelope = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "kind": "request.created",
        "external_id": "ext-1",
        "body": { "note": "hello" }
    }))
    .expect("canonicalizes");
    assert!(spec.matches(&envelope).expect("filter evaluates"));

    let other = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "kind": "request.closed",
        "external_id": "ext-1"
    }))
    .expect("canonicalizes");
    assert!(!spec.matches(&other).expect("filter evaluates"));

    // The dedup key depends only on the pointed-at content.
    let same_content_new_id = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "kind": "request.created",
        "external_id": "ext-1",
        "body": { "note": "different" }
    }))
    .expect("canonicalizes");
    assert_eq!(
        spec.dedup.evaluate(&envelope).expect("dedup evaluates"),
        spec.dedup
            .evaluate(&same_content_new_id)
            .expect("dedup evaluates")
    );
    assert_ne!(
        spec.dedup.evaluate(&envelope).expect("dedup evaluates"),
        spec.dedup.evaluate(&other).expect("dedup evaluates")
    );
}

#[test]
fn a_dedup_expression_must_resolve_completely_or_not_at_all() {
    let spec = trigger();
    let missing = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "kind": "request.created"
    }))
    .expect("canonicalizes");
    assert!(
        spec.dedup.evaluate(&missing).is_err(),
        "a partially resolvable expression must not produce a weaker key"
    );

    let empty = DedupExpression {
        pointers: Vec::new(),
    };
    assert!(empty.evaluate(&missing).is_err());
    assert!(JsonPointer::parse("no-leading-slash").is_err());
}

#[test]
fn auto_arming_is_always_bounded() {
    let mut spec = trigger();
    spec.limits.max_concurrency = 0;
    assert!(spec.validate().is_err(), "zero concurrency is not a bound");

    let mut spec = trigger();
    spec.limits.budget.max_cost.minor_units = 0;
    assert!(spec.validate().is_err(), "a zero cost bound is not a bound");

    let mut spec = trigger();
    spec.approval = AutoArmPolicy::BoundedAutoArm {
        capability: kontor_core::spec::ExecutionCapability {
            granted_to: AccountProfileId::generate(),
            execution_authorization: kontor_core::id::ExecutionAuthorizationId::generate(),
        },
        max_concurrency: 0,
        budget: spec.limits.budget,
    };
    assert!(
        spec.validate().is_err(),
        "auto-arming with no concurrency bound must be refused"
    );

    // The bounded form still validates when every bound is present, and there is
    // no third, unbounded variant to reach.
    let mut spec = trigger();
    spec.approval = AutoArmPolicy::BoundedAutoArm {
        capability: kontor_core::spec::ExecutionCapability {
            granted_to: AccountProfileId::generate(),
            execution_authorization: kontor_core::id::ExecutionAuthorizationId::generate(),
        },
        max_concurrency: 1,
        budget: spec.limits.budget,
    };
    spec.validate().expect("a fully bounded auto-arm is legal");
    // Validating is not enough to be onboardable. `canonicalize` is the only
    // route to a digest, a receipt or a stored row, and it runs the shared
    // sensitive-material scanner over every key: a policy whose capability field
    // was spelled `authorization` — the HTTP header — validated here and was then
    // refused there, which made every bounded auto-arm policy unstorable.
    let document = spec
        .canonicalize()
        .expect("a fully bounded auto-arm can be canonicalized, hashed and stored");
    assert!(
        document.json().contains("execution_authorization"),
        "the capability names the authorization it is bounded by"
    );
}

fn receipt(result: IntakeResult) -> IntakeReceipt {
    IntakeReceipt {
        id: IntakeReceiptId::generate(),
        source_event_id: SourceEventId::generate(),
        source_event_hash: ContentHash::of(b"envelope"),
        trigger: kontor_core::id::TriggerKey::parse("trigger.inbound-request")
            .expect("valid trigger key"),
        trigger_version: SpecVersion::FIRST,
        result,
        approval: None,
        proposed: None,
        idempotency_key: IdempotencyKey::parse("intake-1").expect("valid key"),
        dedup_key: ContentHash::of(b"dedup"),
        duplicate_of: None,
        predecessor_receipt_id: None,
        decided_at: at("2026-08-09T10:00:00Z"),
    }
}

#[test]
fn an_intake_decision_must_be_internally_consistent() {
    receipt(IntakeResult::Proposed)
        .validate()
        .expect("a proposal needs nothing else");

    assert!(
        receipt(IntakeResult::Approved).validate().is_err(),
        "an approval without evidence must be refused"
    );
    let approved = IntakeReceipt {
        approval: Some(ApprovalReceipt {
            authority: AccountProfileId::generate(),
            receipt: CommandReceiptId::generate(),
            approved_at: at("2026-08-09T10:01:00Z"),
        }),
        proposed: Some(ProposedWorkGraph {
            project_id: ProjectId::generate(),
            mini_project_id: None,
            task_ids: vec![TaskId::generate()],
        }),
        ..receipt(IntakeResult::Approved)
    };
    approved.validate().expect("an evidenced approval is legal");

    assert!(
        receipt(IntakeResult::Duplicate).validate().is_err(),
        "a duplicate must point at the original"
    );
    let duplicate = IntakeReceipt {
        duplicate_of: Some(IntakeReceiptId::generate()),
        ..receipt(IntakeResult::Duplicate)
    };
    duplicate.validate().expect("a linked duplicate is legal");

    // A duplicate can never carry a work graph of its own.
    let forged = IntakeReceipt {
        proposed: Some(ProposedWorkGraph {
            project_id: ProjectId::generate(),
            mini_project_id: None,
            task_ids: vec![TaskId::generate()],
        }),
        ..duplicate
    };
    assert!(
        forged.validate().is_err(),
        "a duplicate must not create a second work graph"
    );

    let ignored_with_work = IntakeReceipt {
        proposed: Some(ProposedWorkGraph {
            project_id: ProjectId::generate(),
            mini_project_id: None,
            task_ids: Vec::new(),
        }),
        ..receipt(IntakeResult::Ignored)
    };
    assert!(ignored_with_work.validate().is_err());
}

#[test]
fn every_gate_key_in_the_fixture_is_data_not_behaviour() {
    // The suite must never depend on a particular gate name: prove the profile
    // exposes exactly the gates it declares, whatever they are called.
    let spec = profile();
    let declared: BTreeSet<&GateKey> = spec.gates.iter().map(|gate| &gate.id).collect();
    for phase_spec in &spec.phases {
        for gate in &phase_spec.gates {
            assert!(declared.contains(gate));
            assert!(spec.gate(gate).is_some());
            assert_eq!(spec.gates_of(&phase_spec.id).len(), phase_spec.gates.len());
        }
    }
    assert!(spec.gate(&GateKey::parse("gate.absent").unwrap()).is_none());
}

#[test]
fn sensitive_material_is_rejected_from_every_persisted_string_category_without_echo() {
    // One canary per shape of credential material.
    let canaries = [
        "ghp_0123456789abcdefghijklmnopqrstuvwxyz",
        "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
        "-----BEGIN RSA PRIVATE KEY-----",
        "postgres://user:hunter2@db.internal:5432/kontor",
        "password=hunter2",
        "AKIAIOSFODNN7EXAMPLE0000",
    ];

    for canary in canaries {
        // Every persisted string category funnels through the same primitive.
        let rejections: Vec<Result<(), DomainError>> = vec![
            kontor_core::id::reject_sensitive_text("probe", canary),
            ExternalName::parse(canary).map(|_| ()),
            kontor_core::id::ExternalId::parse(&canary.replace(' ', "-")).map(|_| ()),
            kontor_core::id::BoundedText::parse(canary).map(|_| ()),
            IdempotencyKey::parse(&canary.replace(' ', "-")).map(|_| ()),
            CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "note": canary
            }))
            .map(|_| ()),
            // Nested inside an array inside an object.
            CanonicalDocument::from_value(&serde_json::json!({
                "schema_version": 1,
                "steps": [{ "instruction": canary }]
            }))
            .map(|_| ()),
        ];

        for (index, outcome) in rejections.iter().enumerate() {
            let error = outcome
                .as_ref()
                .err()
                .unwrap_or_else(|| panic!("category {index} accepted `{}`", canary.escape_debug()));
            // The value never appears in the rendered error.
            let rendered = error.to_string();
            let debug = format!("{error:?}");
            assert!(
                !rendered.contains(canary) && !debug.contains(canary),
                "category {index} echoed the canary"
            );
        }
    }

    // Ordinary values in the same categories are unaffected: the scan must not
    // be so broad that real data cannot be stored.
    for benign in [
        "In Development",
        "sk-skill",
        "Til kontroll",
        "basic hygiene",
        "token budget exceeded",
    ] {
        kontor_core::id::reject_sensitive_text("probe", benign)
            .unwrap_or_else(|_| panic!("`{benign}` is not a credential"));
        ExternalName::parse(benign).unwrap_or_else(|_| panic!("`{benign}` is a legal name"));
    }
    // Open keys go through the same rule and keep their own grammar.
    validate_open_key("test", "sk-skill").expect("a short sk- key is not a token");
    assert!(validate_open_key("test", "ghp-0123456789abcdefghijklmnopqrstuvwxyz").is_ok());
}

/// A credential prefix is the start of a token, never the middle of a word.
///
/// The regression: `sk-` matched inside `ta`sk-`scoped`, so an ordinary
/// hyphenated English sentence long enough to clear the tail bound was refused
/// as an OpenAI key — in a plain-prose brief, with nothing credential-shaped in
/// it anywhere. The scan has to stay narrow enough to store real text and wide
/// enough to catch a real key, and the boundary is what separates the two.
#[test]
fn a_credential_prefix_must_begin_at_a_token_boundary() {
    // Ordinary prose. Every one of these embeds a marker inside a word and is
    // long enough that only the boundary rule saves it.
    for benign in [
        "task-scoped placement is derived from the pinned topology revision",
        "risk-free rollback of the operational topology specification revision",
        "desk-side review of the epic control plane and its delivery seats",
        // Not `basic-…` or `bearer-…`: those are deliberate separator variants
        // of the HTTP auth schemes, they match at a real boundary, and
        // narrowing them would be a detection change rather than a bug fix.
        // Recorded as a residual in the OP-03 error-contract inventory.
        // `akia`/`asia` embedded mid-word. At the *start* of a word they are
        // indistinguishable from an AWS key by prefix alone and stay refused,
        // which is the conservative half of the same rule.
        "the makiavellian fantasia of a delivery vocabulary revision upgrade",
    ] {
        kontor_core::id::reject_sensitive_text("probe", benign)
            .unwrap_or_else(|_| panic!("`{benign}` is ordinary prose, not a credential"));
        kontor_core::id::BoundedText::parse(benign)
            .unwrap_or_else(|_| panic!("`{benign}` is storable text"));
    }

    // And the same markers at a real boundary are still refused, whatever
    // opens the token: nothing, whitespace, an assignment, a quote or a path.
    for canary in [
        "sk-0123456789abcdefghijklmnopqrstuv",
        "Bearer sk-0123456789abcdefghijklmnopqrstuv",
        "AWS_KEY=AKIAIOSFODNN7EXAMPLE0000",
        "\"glpat-0123456789abcdefghij\"",
        "/tmp/ghp_0123456789abcdefghijklmnopqrst",
        "value:xoxb-0123456789abcdefghij",
    ] {
        let refused = kontor_core::id::reject_sensitive_text("probe", canary)
            .expect_err("credential-shaped material at a boundary must be refused");
        assert!(
            matches!(refused, DomainError::SensitiveMaterial { .. }),
            "`{}` must be refused as sensitive material",
            canary.escape_debug()
        );
        // And never echoed, whatever opened the token.
        let rendered = format!("{refused} {refused:?}");
        assert!(
            !rendered.contains(canary),
            "the refusal echoed the canary: {rendered}"
        );
    }
}

#[test]
fn persona_actor_cannot_evaluate_or_waive_and_prohibited_actions_are_required() {
    let scenario = persona();
    scenario.validate().expect("the fixture scenario is valid");

    // Prohibited actions are mandatory and must each say something different.
    let mut empty = persona();
    empty.prohibited_actions.clear();
    assert!(
        empty.validate().is_err(),
        "a scenario with nothing prohibited constrains nothing"
    );
    let mut duplicated = persona();
    duplicated.prohibited_actions = vec![
        duplicated.prohibited_actions[0].clone(),
        duplicated.prohibited_actions[0].clone(),
    ];
    assert!(
        duplicated.validate().is_err(),
        "a duplicate prohibited action must be refused"
    );
    // Trimmed, bounded and sensitive-free are carried by the value type itself.
    assert!(ExternalName::parse(" untrimmed").is_err());
    assert!(ExternalName::parse("ghp_0123456789abcdefghijklmnopqrstuvwxyz").is_err());

    // Authority is resolved against the task's pinned profile, never asserted by
    // the scenario alone.
    let mut profile = profile();
    let gate = GateKey::parse("persona.gate").expect("a valid gate key");
    profile.gates[0].id = gate.clone();
    profile.phases[3].gates = vec![gate.clone()];
    profile.gates[0].evaluator_roles = vec![RoleKey::parse("reviewer.independent").unwrap()];
    profile.gates[0].waiver_roles = vec![RoleKey::parse("authority.waiver").unwrap()];
    let snapshot = ResolvedWorkProfileSnapshot::resolve(&profile, at("2026-08-09T09:00:00Z"))
        .expect("the profile resolves");

    let mut aligned = persona();
    aligned.gate_under_test = gate.clone();
    aligned.actor_role = RoleKey::parse("persona.actor").expect("a valid role key");
    aligned.evaluator_roles = vec![RoleKey::parse("reviewer.independent").unwrap()];
    PersonaScenarioSnapshot::freeze_onto_task(&aligned, &snapshot)
        .expect("an independent evaluator authorized by the gate is legal");

    // The persona may not evaluate its own gate...
    let mut self_evaluating = aligned.clone();
    self_evaluating.actor_role = RoleKey::parse("reviewer.independent").unwrap();
    self_evaluating.evaluator_roles = vec![RoleKey::parse("reviewer.independent").unwrap()];
    let error = PersonaScenarioSnapshot::freeze_onto_task(&self_evaluating, &snapshot)
        .expect_err("the actor must not evaluate its own gate");
    assert!(matches!(error, DomainError::MissingAuthority { .. }));

    // ...nor waive it.
    let mut self_waiving = aligned.clone();
    self_waiving.actor_role = RoleKey::parse("authority.waiver").unwrap();
    let error = PersonaScenarioSnapshot::freeze_onto_task(&self_waiving, &snapshot)
        .expect_err("the actor must not hold waiver authority over its own gate");
    assert!(matches!(error, DomainError::MissingAuthority { .. }));

    // An evaluator the gate does not authorize is refused.
    let mut unauthorized = aligned.clone();
    unauthorized.evaluator_roles = vec![RoleKey::parse("maker.primary").unwrap()];
    assert!(
        PersonaScenarioSnapshot::freeze_onto_task(&unauthorized, &snapshot).is_err(),
        "an evaluator must be authorized by the pinned gate"
    );

    // Evaluator and waiver authority must not overlap.
    let mut overlapping = aligned.clone();
    overlapping.evaluator_roles = vec![RoleKey::parse("authority.waiver").unwrap()];
    assert!(
        PersonaScenarioSnapshot::freeze_onto_task(&overlapping, &snapshot).is_err(),
        "one role must not both evaluate and waive the same gate"
    );

    // A gate the pinned profile does not declare cannot be exercised at all.
    let mut absent = aligned.clone();
    absent.gate_under_test = GateKey::parse("gate.absent").expect("a valid gate key");
    assert!(PersonaScenarioSnapshot::freeze_onto_task(&absent, &snapshot).is_err());

    // A standalone freeze validates the scenario but makes no authority claim:
    // the unauthorized-evaluator scenario is perfectly well-formed on its own
    // and only fails once a pinned gate exists to check it against. That is
    // exactly why attaching to a task needs the profile context.
    unauthorized
        .validate()
        .expect("the scenario is well-formed in isolation");
    PersonaScenarioSnapshot::freeze(&unauthorized)
        .expect("a standalone freeze carries no authority claim");
    assert!(
        PersonaScenarioSnapshot::freeze_onto_task(&unauthorized, &snapshot).is_err(),
        "the same scenario is refused once a pinned gate can judge it"
    );
}

// ---------------------------------------------------------------------------
// Calendar windows across time-zone transitions
// ---------------------------------------------------------------------------
//
// A weekly window is written in *local* wall-clock time, so the only questions
// worth testing are the ones a time zone can make ambiguous: a local hour that
// does not exist, a local hour that happens twice, two windows fighting over the
// same minute, and a shift that runs past midnight.

fn oslo() -> IanaTimeZone {
    IanaTimeZone::parse("Europe/Oslo").expect("a known time zone")
}

fn civil_time(text: &str) -> jiff::civil::Time {
    text.parse().expect("a civil time")
}

fn window(weekday: Weekday, start: &str, end: &str) -> WeeklyWindow {
    WeeklyWindow {
        weekday,
        start: civil_time(start),
        end: civil_time(end),
    }
}

fn calendar_profile(windows: Vec<WeeklyWindow>) -> CalendarProfileSpec {
    CalendarProfileSpec {
        schema_version: SCHEMA_VERSION,
        profile_id: CalendarProfileId::generate(),
        version: SpecVersion::FIRST,
        name: ExternalName::parse("Shift").expect("a valid name"),
        windows,
        holiday_merge: HolidayMergePolicy::Ignore,
        drain_lead_minutes: 0,
    }
}

fn assignment(profile: &CalendarProfileSpec) -> WorkCalendarAssignment {
    WorkCalendarAssignment {
        id: WorkCalendarId::generate(),
        project_id: ProjectId::generate(),
        profile_id: profile.profile_id,
        profile_version: profile.version,
        timezone: oslo(),
        window_override: None,
        active: true,
        created_at: at("2026-01-01T00:00:00Z"),
        retired_at: None,
    }
}

/// Resolve one instant against one window set.
fn state_at(profile: &CalendarProfileSpec, instant: &str) -> EffectiveCalendarState {
    let assignment = assignment(profile);
    resolve_effective_state(&CalendarResolution {
        assignment: Some(&assignment),
        profile: Some(profile),
        exceptions: &[],
        schedule_override: None,
        mini_project: None,
        task: None,
        now: at(instant),
    })
    .expect("the pinned profile resolves")
}

#[test]
fn a_daylight_saving_gap_never_opens_a_window_that_has_no_local_time() {
    // Europe/Oslo springs forward on 2026-03-29: local 02:00 through 02:59 does
    // not exist at all, and 00:59Z is still 01:59 CET while 01:00Z is already
    // 03:00 CEST.
    let skipped = calendar_profile(vec![window(Weekday::Sunday, "02:00:00", "03:00:00")]);
    for instant in [
        "2026-03-29T00:30:00Z",
        "2026-03-29T00:59:59Z",
        "2026-03-29T01:00:00Z",
        "2026-03-29T01:30:00Z",
    ] {
        assert_eq!(
            state_at(&skipped, instant),
            EffectiveCalendarState::Closed,
            "{instant} must not open a window over a local hour that never happens"
        );
    }

    // The hours either side of the gap still behave normally, so the window set
    // is not simply inert that day.
    let around = calendar_profile(vec![
        window(Weekday::Sunday, "01:00:00", "02:00:00"),
        window(Weekday::Sunday, "03:00:00", "04:00:00"),
    ]);
    assert_eq!(
        state_at(&around, "2026-03-29T00:30:00Z"),
        EffectiveCalendarState::Open,
        "01:30 CET is before the gap and is open"
    );
    assert_eq!(
        state_at(&around, "2026-03-29T01:30:00Z"),
        EffectiveCalendarState::Open,
        "03:30 CEST is after the gap and is open"
    );
}

#[test]
fn a_daylight_saving_fold_opens_the_repeated_local_hour_both_times() {
    // Europe/Oslo falls back on 2026-10-25: local 02:30 happens once at 00:30Z
    // in CEST and again at 01:30Z in CET. A window over that hour is genuinely
    // open for two hours of real time, and neither pass may be skipped or
    // double-counted into a different state.
    let folded = calendar_profile(vec![window(Weekday::Sunday, "02:00:00", "03:00:00")]);
    for instant in ["2026-10-25T00:30:00Z", "2026-10-25T01:30:00Z"] {
        assert_eq!(
            state_at(&folded, instant),
            EffectiveCalendarState::Open,
            "{instant} is inside the repeated local hour and must be open"
        );
    }
    // The instant after the second pass is outside the window again.
    assert_eq!(
        state_at(&folded, "2026-10-25T02:00:00Z"),
        EffectiveCalendarState::Closed,
        "03:00 CET is past the window end"
    );

    // Drain is measured in local wall-clock minutes, which is exactly how the
    // window is written: both passes drain identically.
    let mut draining = folded.clone();
    draining.drain_lead_minutes = 45;
    for instant in ["2026-10-25T00:30:00Z", "2026-10-25T01:30:00Z"] {
        assert_eq!(
            state_at(&draining, instant),
            EffectiveCalendarState::Draining,
            "{instant} is 30 local minutes from the window end"
        );
    }
}

#[test]
fn overlapping_windows_are_refused_and_touching_windows_are_not() {
    // Overlap is rejected rather than merged, so the stored definition and the
    // resolved behaviour can never diverge.
    assert!(
        validate_windows(&[
            window(Weekday::Monday, "08:00:00", "12:00:00"),
            window(Weekday::Monday, "11:00:00", "16:00:00"),
        ])
        .is_err(),
        "two windows may not claim the same local minute"
    );
    // Declaration order must not decide the verdict.
    assert!(
        validate_windows(&[
            window(Weekday::Monday, "11:00:00", "16:00:00"),
            window(Weekday::Monday, "08:00:00", "12:00:00"),
        ])
        .is_err(),
        "overlap is a property of the set, not of its order"
    );
    // A fully contained window is still an overlap.
    assert!(
        validate_windows(&[
            window(Weekday::Monday, "08:00:00", "16:00:00"),
            window(Weekday::Monday, "10:00:00", "11:00:00"),
        ])
        .is_err()
    );
    // Abutting windows do not overlap: `end` is exclusive.
    validate_windows(&[
        window(Weekday::Monday, "08:00:00", "12:00:00"),
        window(Weekday::Monday, "12:00:00", "16:00:00"),
    ])
    .expect("an exclusive end may equal the next start");
    // The same clock times on different weekdays are independent.
    validate_windows(&[
        window(Weekday::Monday, "08:00:00", "12:00:00"),
        window(Weekday::Tuesday, "08:00:00", "12:00:00"),
    ])
    .expect("weekdays do not overlap each other");
}

#[test]
fn an_overnight_shift_must_be_declared_as_two_windows_and_then_resolves() {
    // A wrapping window is unrepresentable: `22:00-06:00` would leave every
    // consumer guessing whether it means eight hours or sixteen.
    assert!(
        window(Weekday::Monday, "22:00:00", "06:00:00")
            .validate()
            .is_err(),
        "a window may not wrap past midnight"
    );
    assert!(
        window(Weekday::Monday, "08:00:00", "08:00:00")
            .validate()
            .is_err(),
        "an empty window is not a window"
    );

    // Declared as the two windows it actually is, the shift resolves on both
    // sides of local midnight. In January Oslo is CET (UTC+1), so 22:30Z on
    // Monday is 23:30 Monday local and 01:30Z is 02:30 Tuesday local.
    let overnight = calendar_profile(vec![
        window(Weekday::Monday, "22:00:00", "23:59:59"),
        window(Weekday::Tuesday, "00:00:00", "06:00:00"),
    ]);
    validate_windows(&overnight.windows).expect("the two-window form is valid");
    assert_eq!(
        state_at(&overnight, "2026-01-05T22:30:00Z"),
        EffectiveCalendarState::Open,
        "23:30 Monday local is inside the first half of the shift"
    );
    assert_eq!(
        state_at(&overnight, "2026-01-06T01:30:00Z"),
        EffectiveCalendarState::Open,
        "02:30 Tuesday local is inside the second half"
    );
    assert_eq!(
        state_at(&overnight, "2026-01-06T06:30:00Z"),
        EffectiveCalendarState::Closed,
        "07:30 Tuesday local is after the shift"
    );
    // The gap the exclusive end leaves is one second, not a whole evening.
    assert_eq!(
        state_at(&overnight, "2026-01-05T20:30:00Z"),
        EffectiveCalendarState::Closed,
        "21:30 Monday local is before the shift starts"
    );
}

// ---------------------------------------------------------------------------
// Context-window policy
// ---------------------------------------------------------------------------

fn policy(class: ContextWindowClass) -> ContextWindowPolicy {
    ContextWindowPolicy {
        class,
        ..ContextWindowPolicy::standard()
    }
}

#[test]
fn the_five_classes_map_to_exactly_the_approved_trigger_targets() {
    assert_eq!(ContextWindowClass::ALL.len(), 5);
    assert_eq!(ContextWindowClass::Lean.trigger_tokens(), Some(128_000));
    assert_eq!(ContextWindowClass::Standard.trigger_tokens(), Some(256_000));
    assert_eq!(ContextWindowClass::Deep.trigger_tokens(), Some(512_000));
    assert_eq!(ContextWindowClass::Extended.trigger_tokens(), Some(720_000));
    // `native` keeps the runtime's own default, so Kontor has no number to send.
    assert_eq!(ContextWindowClass::Native.trigger_tokens(), None);
}

/// MUT-CTX-01. Swapping the role-slot and work-profile arms of the resolver
/// makes this fail: the recorded source becomes `work_profile`, and the
/// resolved class with it.
#[test]
fn precedence_is_override_then_role_slot_then_work_profile_then_seed_then_standard() {
    let over = policy(ContextWindowClass::Extended);
    let slot = policy(ContextWindowClass::Deep);
    let profile = policy(ContextWindowClass::Lean);
    let seed = policy(ContextWindowClass::Standard);

    // Every prefix of the precedence chain, one candidate removed at a time.
    let cases = [
        (
            ContextPolicyInputs {
                run_override: Some(&over),
                role_slot: Some(&slot),
                work_profile: Some(&profile),
                role_seed: Some(&seed),
            },
            ContextPolicySource::AuthorizedRunOverride,
            ContextWindowClass::Extended,
        ),
        (
            ContextPolicyInputs {
                run_override: None,
                role_slot: Some(&slot),
                work_profile: Some(&profile),
                role_seed: Some(&seed),
            },
            ContextPolicySource::RoleSlot,
            ContextWindowClass::Deep,
        ),
        (
            ContextPolicyInputs {
                run_override: None,
                role_slot: None,
                work_profile: Some(&profile),
                role_seed: Some(&seed),
            },
            ContextPolicySource::WorkProfile,
            ContextWindowClass::Lean,
        ),
        (
            ContextPolicyInputs {
                run_override: None,
                role_slot: None,
                work_profile: None,
                role_seed: Some(&seed),
            },
            ContextPolicySource::RoleSeed,
            ContextWindowClass::Standard,
        ),
        (
            ContextPolicyInputs::default(),
            ContextPolicySource::StandardFallback,
            ContextWindowClass::Standard,
        ),
    ];

    for (inputs, expected_source, expected_class) in cases {
        let resolved = resolve_context_window(&inputs).expect("the chain resolves");
        assert_eq!(resolved.source, expected_source);
        assert_eq!(resolved.policy.class, expected_class);
    }
}

/// MUT-CTX-02. Letting a seed or the fallback reach an explicit-only class
/// makes this fail.
#[test]
fn extended_and_native_are_unreachable_from_a_seed() {
    for class in [ContextWindowClass::Extended, ContextWindowClass::Native] {
        let ambitious = policy(class);
        assert!(class.requires_explicit_selection());

        let seeded = resolve_context_window(&ContextPolicyInputs {
            role_seed: Some(&ambitious),
            ..ContextPolicyInputs::default()
        })
        .expect_err("a seed may not select an explicit-only class");
        assert!(matches!(seeded, DomainError::MissingAuthority { .. }));

        // The same policy is perfectly legal from a deliberate declaration.
        for explicit in [
            ContextPolicySource::AuthorizedRunOverride,
            ContextPolicySource::RoleSlot,
            ContextPolicySource::WorkProfile,
        ] {
            ambitious
                .ensure_selectable_by(explicit)
                .expect("an explicit declaration may select it");
        }
    }
}

#[test]
fn a_policy_document_refuses_an_arbitrary_token_target() {
    // There is no spelling for a custom threshold: the class owns the number.
    let with_tokens = serde_json::json!({
        "class": "standard",
        "enforcement": "best_effort",
        "trigger_scope": "growth_after_prefix",
        "boundary_compaction": true,
        "summary_profile": "portable_handoff_v1",
        "trigger_tokens": 300_000,
    });
    assert!(serde_json::from_value::<ContextWindowPolicy>(with_tokens).is_err());

    let unknown_class = serde_json::json!({
        "class": "enormous",
        "enforcement": "best_effort",
        "trigger_scope": "growth_after_prefix",
        "boundary_compaction": true,
        "summary_profile": "portable_handoff_v1",
    });
    assert!(serde_json::from_value::<ContextWindowPolicy>(unknown_class).is_err());
}

#[test]
fn resolution_is_byte_identical_across_repeated_freezes() {
    let resolved = resolve_context_window(&ContextPolicyInputs::default()).expect("resolves");
    let requested = RequestedContextPolicy::of(&resolved, SCHEMA_VERSION);
    let bounds = ContextWindowBounds::unknown();

    let first = ContextPolicySnapshot::freeze(
        requested,
        EffectiveContextPolicy::derive(&requested, &bounds, true).expect("derives"),
        at("2026-08-13T09:43:00Z"),
    )
    .expect("freezes");
    let second = ContextPolicySnapshot::freeze(
        requested,
        EffectiveContextPolicy::derive(&requested, &bounds, true).expect("derives"),
        at("2026-08-13T09:43:00Z"),
    )
    .expect("freezes");

    assert_eq!(first.requested_hash, second.requested_hash);
    assert_eq!(first.effective_hash, second.effective_hash);
    first.verify().expect("a fresh snapshot verifies");
}

#[test]
fn an_unknown_ceiling_leaves_the_requested_trigger_standing() {
    let resolved = resolve_context_window(&ContextPolicyInputs::default()).expect("resolves");
    let requested = RequestedContextPolicy::of(&resolved, SCHEMA_VERSION);
    let effective =
        EffectiveContextPolicy::derive(&requested, &ContextWindowBounds::unknown(), true)
            .expect("derives");

    assert_eq!(effective.trigger_tokens, Some(256_000));
    assert_eq!(effective.clamp, ContextClamp::None);
    // Unknown stays unknown. Zero is never substituted for an undeclared bound.
    assert_eq!(effective.bounds.safe_ceiling_tokens, None);
    assert_eq!(effective.bounds.minimum_trigger_tokens, None);
}

#[test]
fn a_declared_ceiling_clamps_the_trigger_and_records_why() {
    let deep = policy(ContextWindowClass::Deep);
    let resolved = resolve_context_window(&ContextPolicyInputs {
        role_slot: Some(&deep),
        ..ContextPolicyInputs::default()
    })
    .expect("resolves");
    let requested = RequestedContextPolicy::of(&resolved, SCHEMA_VERSION);
    let effective = EffectiveContextPolicy::derive(
        &requested,
        &ContextWindowBounds {
            safe_ceiling_tokens: Some(200_000),
            minimum_trigger_tokens: None,
        },
        true,
    )
    .expect("derives");

    assert_eq!(requested.trigger_tokens, Some(512_000));
    assert_eq!(effective.trigger_tokens, Some(200_000));
    assert_eq!(effective.clamp, ContextClamp::ToSafeCeiling);
    // The class is what was asked for and stays auditable as such.
    assert_eq!(effective.policy.class, ContextWindowClass::Deep);
}

#[test]
fn required_enforcement_refuses_a_runtime_that_cannot_configure_it() {
    let strict = ContextWindowPolicy {
        enforcement: ContextEnforcement::Required,
        ..ContextWindowPolicy::standard()
    };
    let resolved = resolve_context_window(&ContextPolicyInputs {
        role_slot: Some(&strict),
        ..ContextPolicyInputs::default()
    })
    .expect("resolves");
    let requested = RequestedContextPolicy::of(&resolved, SCHEMA_VERSION);

    let refused =
        EffectiveContextPolicy::derive(&requested, &ContextWindowBounds::unknown(), false)
            .expect_err("required enforcement needs a capable runtime");
    assert!(matches!(refused, DomainError::MissingEvidence { .. }));

    // A required target below the runtime's smallest configurable trigger
    // cannot be honoured without silently widening the seat's window.
    let raised = EffectiveContextPolicy::derive(
        &requested,
        &ContextWindowBounds {
            safe_ceiling_tokens: None,
            minimum_trigger_tokens: Some(400_000),
        },
        true,
    )
    .expect_err("a required target under the runtime minimum is refused");
    assert!(matches!(raised, DomainError::Invalid { .. }));
}

#[test]
fn best_effort_on_an_incapable_runtime_is_visibly_not_enforced() {
    let resolved = resolve_context_window(&ContextPolicyInputs::default()).expect("resolves");
    let requested = RequestedContextPolicy::of(&resolved, SCHEMA_VERSION);
    let effective =
        EffectiveContextPolicy::derive(&requested, &ContextWindowBounds::unknown(), false)
            .expect("best effort continues");

    assert_eq!(effective.capability, ContextCapabilityResult::NotEnforced);
    // No trigger is in force, and nothing claims one is.
    assert_eq!(effective.trigger_tokens, None);

    let snapshot = ContextPolicySnapshot::freeze(requested, effective, at("2026-08-13T09:43:00Z"))
        .expect("freezes");
    assert!(snapshot.permits_reuse(), "not_enforced still permits reuse");

    // Pending is the one state that blocks reuse.
    let pending =
        ContextPolicySnapshot::freeze(requested, effective.pending(), at("2026-08-13T09:43:00Z"))
            .expect("freezes");
    assert!(!pending.permits_reuse());
}

/// MUT-007. The enum variant is not the contract — the *spelling* is, because
/// that is what reaches JSON, errors and logs, and what a reader takes as the
/// verdict. A runtime that declares no context control must render as
/// `not_enforced` and never as a word that reads like success.
///
/// Re-spelling `ContextCapabilityResult::NotEnforced` as `"configured"`, or
/// `CompactionStatus::NotEnforced` / `Unsupported` as `"confirmed"`, makes this
/// fail.
#[test]
fn an_unenforced_context_policy_renders_as_not_enforced_and_never_as_success() {
    let resolved = resolve_context_window(&ContextPolicyInputs::default()).expect("resolves");
    let requested = RequestedContextPolicy::of(&resolved, SCHEMA_VERSION);
    let effective =
        EffectiveContextPolicy::derive(&requested, &ContextWindowBounds::unknown(), false)
            .expect("best effort continues on a runtime that declares no context control");

    assert_eq!(effective.capability, ContextCapabilityResult::NotEnforced);
    assert_eq!(
        effective.capability.as_str(),
        "not_enforced",
        "the stable spelling of an unenforced policy"
    );
    assert_eq!(effective.capability.to_string(), "not_enforced");
    assert_eq!(
        serde_json::to_value(effective.capability).expect("the capability serializes"),
        serde_json::json!("not_enforced")
    );
    assert_ne!(
        effective.capability.as_str(),
        ContextCapabilityResult::Configured.as_str(),
        "a runtime that enforces nothing must never render as a configured one"
    );

    // The compaction half of the same fact tells the same truth: neither an
    // unenforced nor an unsupported runtime may render as a confirmed one.
    assert_eq!(CompactionStatus::NotEnforced.as_str(), "not_enforced");
    assert_eq!(CompactionStatus::Unsupported.as_str(), "unsupported");
    for honest in [CompactionStatus::NotEnforced, CompactionStatus::Unsupported] {
        assert_ne!(
            honest.as_str(),
            CompactionStatus::Confirmed.as_str(),
            "{honest} must not render as a confirmed compaction"
        );
    }
}

#[test]
fn a_seed_table_refuses_a_duplicate_role_and_an_explicit_only_class() {
    let role = RoleKey::parse("architect").expect("a valid role key");
    let duplicated = TeamContextPolicySeed {
        work_profile: None,
        role_seeds: vec![
            RoleContextSeed {
                role: role.clone(),
                context_window: policy(ContextWindowClass::Deep),
            },
            RoleContextSeed {
                role: role.clone(),
                context_window: policy(ContextWindowClass::Lean),
            },
        ],
    };
    assert!(duplicated.validate().is_err());

    let ambitious = TeamContextPolicySeed {
        work_profile: None,
        role_seeds: vec![RoleContextSeed {
            role,
            context_window: policy(ContextWindowClass::Extended),
        }],
    };
    assert!(matches!(
        ambitious
            .validate()
            .expect_err("a seed may not select extended"),
        DomainError::MissingAuthority { .. }
    ));
}

#[test]
fn a_model_chain_is_closed_and_bounded() {
    let rung = ModelRung {
        provider: ProviderRef("codex".to_owned()),
        model: ModelRef("gpt-5.6-sol".to_owned()),
        effort: Some(EffortLevel::Xhigh),
    };
    assert!(
        ModelChainPolicy {
            rungs: vec![rung.clone()]
        }
        .validate()
        .is_ok()
    );
    assert!(ModelChainPolicy { rungs: vec![] }.validate().is_err());
    assert!(
        ModelChainPolicy {
            rungs: vec![rung; 5]
        }
        .validate()
        .is_err()
    );

    for forbidden in [
        "deepseek/deepseek-v4-pro",
        "deepseek-v4pro",
        "DeepSeek_V4_Pro-Plus",
    ] {
        let error = ModelChainPolicy {
            rungs: vec![ModelRung {
                provider: ProviderRef("opencode".to_owned()),
                model: ModelRef(forbidden.to_owned()),
                effort: Some(EffortLevel::Max),
            }],
        }
        .validate()
        .expect_err("the DeepSeek V4 Pro family is excluded in every spelling");
        assert!(matches!(
            error,
            DomainError::Invalid {
                subject: "ModelRung",
                ..
            }
        ));
    }
}

#[test]
fn only_an_exhausted_allowance_recovers_on_a_clock() {
    use kontor_core::repository::ProviderQuotaState;
    let at = |second: i64| Timestamp::from_second(second).expect("a valid instant");
    let state = |kind: ProviderQuotaKind, resets_at: Option<Timestamp>| ProviderQuotaState {
        project_id: ProjectId::generate(),
        account_profile_id: AccountProfileId::generate(),
        provider: "codex".to_owned(),
        state: kind,
        resets_at,
        windows: Vec::new(),
        credit: None,
        evidence_hash: ContentHash::of(b"evidence"),
        source: ProviderQuotaSource::RuntimeObservation,
        observed_at: at(1_000),
        revision: AggregateRevision::INITIAL,
        updated_at: at(1_000),
    };

    assert!(!state(ProviderQuotaKind::Available, None).blocks_at(at(1_000)));

    // An allowance blocks until its instant and then stops blocking on its own.
    // Waiting for a collector to rewrite the row would park work past the moment
    // it could have run.
    let exhausted = state(ProviderQuotaKind::Exhausted, Some(at(2_000)));
    assert!(exhausted.blocks_at(at(1_999)));
    assert!(!exhausted.blocks_at(at(2_000)));

    // A drained balance never expires on its own: it recovers when someone pays.
    // A timer here would be a retry loop against a dead key.
    assert!(state(ProviderQuotaKind::Drained, None).blocks_at(at(9_999_999)));

    // Unknown fails closed, the way account availability does.
    assert!(state(ProviderQuotaKind::Unknown, None).blocks_at(at(9_999_999)));

    // `cannot_report` is the opposite instruction to `unknown`, not a synonym
    // for it. Both describe an absence of numbers: `unknown` means *this reading
    // failed*, and `cannot_report` means *this provider has no such number to
    // give* — OpenRouter's `:free` routes under FND-005/DEC-001. Failing closed
    // on the second retires a provider permanently on the strength of a figure
    // it was never going to produce, so it is used reactively instead: run until
    // it refuses, then record the reset it states.
    assert!(
        !state(ProviderQuotaKind::CannotReport, None).blocks_at(at(9_999_999)),
        "a provider that cannot report headroom is used, not retired"
    );
    assert!(ProviderQuotaKind::CannotReport.is_usable());
    assert!(!ProviderQuotaKind::Unknown.is_usable());
}

// ---------------------------------------------------------------------------
// Write-time shareability classification
//
// The mutants this section exists to kill:
//
// * letting tier-A operational state be classified at all;
// * flipping either tier default, so ordinary work silently changes side;
// * recording a human override with nobody's name on it;
// * recording a non-default class as though the default rule had produced it.
// ---------------------------------------------------------------------------

#[test]
fn tier_a_operational_state_refuses_classification() {
    assert!(!ShareabilityTier::OperationalState.is_classifiable());
    assert!(ShareabilityTier::OperationalState.default_class().is_err());
    assert!(Shareability::default_for(ShareabilityTier::OperationalState).is_err());
    assert!(
        Shareability::overridden_by(
            ShareabilityTier::OperationalState,
            ShareabilityClass::ProjectShared,
            ExternalName::parse("An operator").expect("a valid name"),
        )
        .is_err(),
        "a human cannot promote operational state either"
    );
}

#[test]
fn each_classifiable_tier_has_a_default_so_work_never_stalls() {
    for (tier, expected) in [
        (
            ShareabilityTier::ProjectKnowledge,
            ShareabilityClass::ProjectShared,
        ),
        (
            ShareabilityTier::PersonalDraft,
            ShareabilityClass::KontorLocal,
        ),
    ] {
        assert!(tier.is_classifiable());
        let stamp = Shareability::default_for(tier).expect("a classifiable tier has a default");
        assert_eq!(stamp.class, expected);
        assert_eq!(stamp.provenance, ShareabilityProvenance::TypeDefault);
        assert_eq!(stamp.classifier, ShareabilityClassifier::TypeDefaultRule);
        assert!(stamp.classifier.identity().is_none());
        stamp
            .validate_for(tier)
            .expect("a freshly defaulted stamp validates");
    }
}

#[test]
fn an_override_is_attributable_and_a_default_is_not() {
    let human = ExternalName::parse("Lead Software Architect").expect("a valid name");
    let promoted = Shareability::overridden_by(
        ShareabilityTier::PersonalDraft,
        ShareabilityClass::ProjectShared,
        human.clone(),
    )
    .expect("a human may promote a draft");
    assert_eq!(promoted.provenance, ShareabilityProvenance::HumanOverride);
    assert_eq!(promoted.classifier.identity(), Some(&human));
    promoted
        .validate_for(ShareabilityTier::PersonalDraft)
        .expect("an attributed override validates");

    // A class nobody chose, wearing the default rule's name.
    let forged = Shareability {
        class: ShareabilityClass::KontorLocal,
        classifier: ShareabilityClassifier::TypeDefaultRule,
        provenance: ShareabilityProvenance::TypeDefault,
    };
    assert!(
        forged
            .validate_for(ShareabilityTier::ProjectKnowledge)
            .is_err(),
        "withholding tier-B knowledge is a human decision, not a default"
    );

    // An override with the default rule's identity, and its mirror image.
    let unattributed = Shareability {
        class: ShareabilityClass::KontorLocal,
        classifier: ShareabilityClassifier::TypeDefaultRule,
        provenance: ShareabilityProvenance::HumanOverride,
    };
    assert!(
        unattributed
            .validate_for(ShareabilityTier::ProjectKnowledge)
            .is_err()
    );
    let unclaimed = Shareability {
        class: ShareabilityClass::ProjectShared,
        classifier: ShareabilityClassifier::Human(human),
        provenance: ShareabilityProvenance::TypeDefault,
    };
    assert!(
        unclaimed
            .validate_for(ShareabilityTier::ProjectKnowledge)
            .is_err()
    );
}

#[test]
fn classification_spellings_are_stable_and_closed() {
    assert_eq!(ShareabilityClass::ProjectShared.as_str(), "project_shared");
    assert_eq!(ShareabilityClass::KontorLocal.as_str(), "kontor_local");
    assert_eq!(ShareabilityProvenance::TypeDefault.as_str(), "type_default");
    assert_eq!(
        ShareabilityProvenance::HumanOverride.as_str(),
        "human_override"
    );
    assert!(ShareabilityClass::parse("public").is_err());
    assert!(ShareabilityProvenance::parse("guessed").is_err());
    for class in ShareabilityClass::ALL {
        assert_eq!(
            ShareabilityClass::parse(class.as_str()).expect("a known value"),
            *class
        );
    }
}
