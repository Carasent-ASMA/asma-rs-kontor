//! Durable desks in a Team Definition (ASMA-8450).
//!
//! A desk successor is derived here from the bundled defaults rather than read
//! from a bundled revision, so these tests pin the *rules* — what a desk
//! declaration may say, what it renders, and what it must leave untouched —
//! independently of which lineage version a successor is eventually shipped at.

use kontor_core::id::{DeskKey, TopologyKindKey};
use kontor_core::naming::NativeNameValues;
use kontor_core::spec::{ProjectSessionTopologySpec, TeamDefinitionSpec};

const DESK_KINDS: &str = r#"[
  {"kind": "DESK", "allowed_parents": ["PSW"], "cardinality": {"minimum": 0},
   "projection_capabilities": ["native_root"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "Durable Desk", "meaning": "A desk.",
                 "category": "session_topology", "lifecycle": "current"}},
  {"kind": "DWS", "allowed_parents": ["DESK"], "cardinality": {"minimum": 1, "maximum": 1},
   "projection_capabilities": ["native_child", "session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "seat_name_template": {"segments": [{"kind": "token", "value": "AREA_CODE"}]},
   "code_help": {"full_name": "Desk Workspace", "meaning": "A desk's workspace.",
                 "category": "session_topology", "lifecycle": "current"}}
]"#;

const DESK_CONTAINERS: &str = r#"[
  {"kind": "DESK", "parent": null, "prefix": "DESK", "projection_capabilities": ["native_root"],
   "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"},
                                  {"kind": "token", "value": "DESK_NAME"}]}},
  {"kind": "DWS", "parent": "DESK", "prefix": "ADAM",
   "projection_capabilities": ["native_child", "session_host"], "read_only": false,
   "name_template": {"segments": [{"kind": "token", "value": "PREFIX"}]}}
]"#;

const DESKS: &str = r#"[
  {"desk_key": "adam", "display_name": "ADAM", "kind": "DESK", "workspace_kind": "DWS"},
  {"desk_key": "pr-review", "display_name": "PR REVIEW", "kind": "DESK", "workspace_kind": "DWS"}
]"#;

/// The bundled default Team Definition and the topology it composes against.
fn defaults() -> (TeamDefinitionSpec, ProjectSessionTopologySpec) {
    let domain = kontor_profiles::bundled_operational_domain().expect("the bundled domain");
    let definition = domain
        .team_definitions
        .first()
        .cloned()
        .expect("a default Team Definition");
    let topology = domain
        .topology_specs
        .iter()
        .find(|topology| {
            topology.spec_id == definition.topology.spec_id
                && topology.version == definition.topology.version
        })
        .cloned()
        .expect("the default's topology is bundled");
    (definition, topology)
}

/// The default topology plus the two desk kinds, as its next revision.
fn desk_topology() -> ProjectSessionTopologySpec {
    let (_, topology) = defaults();
    let mut value = serde_json::to_value(&topology).expect("the topology serializes");
    let kinds: Vec<serde_json::Value> = serde_json::from_str(DESK_KINDS).expect("desk kinds");
    value["node_kinds"]
        .as_array_mut()
        .expect("node kinds")
        .extend(kinds);
    value["version"] = serde_json::json!(topology.version.get() + 1);
    serde_json::from_value(value).expect("a desk topology deserializes")
}

/// The default definition plus both desks, composed against `topology`.
fn desk_definition_value(topology: &ProjectSessionTopologySpec) -> serde_json::Value {
    let (definition, _) = defaults();
    let mut value = serde_json::to_value(&definition).expect("the definition serializes");
    let containers: Vec<serde_json::Value> =
        serde_json::from_str(DESK_CONTAINERS).expect("desk containers");
    value["containers"]
        .as_array_mut()
        .expect("containers")
        .extend(containers);
    value["desks"] = serde_json::from_str(DESKS).expect("desks");
    value["version"] = serde_json::json!(definition.version.get() + 1);
    value["topology"] = serde_json::json!({
        "spec_id": topology.spec_id.to_string(),
        "version": topology.version.get(),
        "canonical_hash": topology.canonicalize().expect("canonical").hash().as_str(),
    });
    value
}

fn desk_definition() -> (TeamDefinitionSpec, ProjectSessionTopologySpec) {
    let topology = desk_topology();
    let definition =
        serde_json::from_value(desk_definition_value(&topology)).expect("a desk definition");
    (definition, topology)
}

/// The refusal one mutated desk definition earns, from validation alone.
fn refusal(mutate: impl FnOnce(&mut serde_json::Value)) -> String {
    let topology = desk_topology();
    let mut value = desk_definition_value(&topology);
    mutate(&mut value);
    let definition: TeamDefinitionSpec =
        serde_json::from_value(value).expect("the mutated definition still deserializes");
    definition
        .validate_against(&topology)
        .expect_err("the mutated definition must be refused")
        .to_string()
}

fn render(definition: &TeamDefinitionSpec, kind: &str, desk: &str) -> String {
    let container = definition
        .container(&TopologyKindKey::parse(kind).expect("a kind"))
        .expect("the container is configured");
    let declaration = definition
        .desk(&DeskKey::parse(desk).expect("a desk key"))
        .expect("the desk is declared");
    container
        .name_template
        .render(
            &definition.separator,
            &NativeNameValues::new()
                .with_prefix(container.prefix.as_str())
                .with_desk_name(declaration.display_name.as_str()),
        )
        .expect("a desk name renders")
        .as_str()
        .to_owned()
}

#[test]
fn a_desk_successor_composes_against_its_exact_topology_and_renders_the_adopted_names() {
    let (definition, topology) = desk_definition();
    definition
        .validate_against(&topology)
        .expect("the desk successor composes");
    assert_eq!(render(&definition, "DESK", "adam"), "DESK • ADAM");
    assert_eq!(render(&definition, "DESK", "pr-review"), "DESK • PR REVIEW");
    assert_eq!(render(&definition, "DWS", "adam"), "ADAM");
    assert_eq!(render(&definition, "DWS", "pr-review"), "ADAM");
}

#[test]
fn a_desk_successor_cites_a_topology_by_its_exact_hash() {
    let topology = desk_topology();
    let mut value = desk_definition_value(&topology);
    // The previous revision's hash, with this revision's version: a forged
    // citation of the bytes the desk kinds are declared in.
    let (_, previous) = defaults();
    value["topology"]["canonical_hash"] =
        serde_json::json!(previous.canonicalize().expect("canonical").hash().as_str());
    let definition: TeamDefinitionSpec = serde_json::from_value(value).expect("deserializes");
    assert!(definition.validate_against(&topology).is_err());
    // And the desk kinds exist only from the successor on.
    let (desks, _) = desk_definition();
    let mut against_previous = serde_json::to_value(&desks).expect("serializes");
    against_previous["topology"] = serde_json::json!({
        "spec_id": previous.spec_id.to_string(),
        "version": previous.version.get(),
        "canonical_hash": previous.canonicalize().expect("canonical").hash().as_str(),
    });
    let against_previous: TeamDefinitionSpec =
        serde_json::from_value(against_previous).expect("deserializes");
    assert!(against_previous.validate_against(&previous).is_err());
}

#[test]
fn undeclared_or_ill_formed_desks_are_refused_before_publication() {
    assert!(
        refusal(|value| value["desks"][1]["desk_key"] = serde_json::json!("adam"))
            .contains("duplicate desk key or desk name")
    );
    assert!(
        refusal(|value| value["desks"][1]["display_name"] = serde_json::json!("ADAM"))
            .contains("duplicate desk key or desk name")
    );
    assert!(
        refusal(|value| value["desks"][0]["display_name"] = serde_json::json!("A • B"))
            .contains("separator glyph")
    );
    // A desk must be a root container with its workspace directly below it.
    assert!(
        refusal(|value| value["desks"][0]["kind"] = serde_json::json!("DWS"))
            .contains("a root container and a workspace container below it")
    );
    assert!(
        refusal(|value| value["desks"][0]["workspace_kind"] = serde_json::json!("ECP"))
            .contains("a root container and a workspace container below it")
    );
    assert!(
        refusal(|value| value["desks"][0]["kind"] = serde_json::json!("NOPE"))
            .contains("a root container and a workspace container below it")
    );
    // A desk is named only by its own vocabulary: no epic or task token.
    assert!(
        refusal(|value| {
            let desk = value["containers"]
                .as_array_mut()
                .expect("containers")
                .iter_mut()
                .find(|container| container["kind"] == "DESK")
                .expect("the desk container");
            desk["name_template"]["segments"]
                .as_array_mut()
                .expect("segments")
                .push(serde_json::json!({"kind": "token", "value": "EPIC_JIRA_KEY"}));
        })
        .contains("may render only PREFIX, DESK_NAME and literals")
    );
    // And DESK_NAME is meaningless anywhere a desk is not.
    assert!(
        refusal(|value| {
            let epic = value["containers"]
                .as_array_mut()
                .expect("containers")
                .iter_mut()
                .find(|container| container["kind"] == "ESW")
                .expect("the epic container");
            epic["name_template"]["segments"]
                .as_array_mut()
                .expect("segments")
                .push(serde_json::json!({"kind": "token", "value": "DESK_NAME"}));
        })
        .contains("only a declared desk container may render DESK_NAME")
    );
    // A desk container no desk declares is refused rather than read as a
    // second project root.
    assert!(
        refusal(|value| value["desks"] = serde_json::json!([]))
            .contains("only a declared desk container may render DESK_NAME")
    );
}

#[test]
fn a_desk_the_topology_does_not_place_below_the_project_root_is_refused() {
    let mut topology = serde_json::to_value(desk_topology()).expect("serializes");
    let desk = topology["node_kinds"]
        .as_array_mut()
        .expect("node kinds")
        .iter_mut()
        .find(|kind| kind["kind"] == "DESK")
        .expect("the desk kind");
    desk["allowed_parents"] = serde_json::json!(["ESW"]);
    let topology: ProjectSessionTopologySpec =
        serde_json::from_value(topology).expect("a topology");
    let definition: TeamDefinitionSpec =
        serde_json::from_value(desk_definition_value(&topology)).expect("a definition");
    let refused = definition
        .validate_against(&topology)
        .expect_err("a desk below an epic is refused")
        .to_string();
    assert!(
        refused.contains("does not place below the project root"),
        "{refused}"
    );
}

#[test]
fn a_desk_declaration_admits_no_unknown_field() {
    let topology = desk_topology();
    let mut value = desk_definition_value(&topology);
    value["desks"][0]["epic_id"] = serde_json::json!("01936f5a-0000-7000-8000-000000000001");
    assert!(serde_json::from_value::<TeamDefinitionSpec>(value).is_err());
}

#[test]
fn a_definition_without_desks_keeps_its_exact_bytes_and_hash() {
    // The bundled default predates desks. Reading it through a type that now
    // knows about desks must not add a key to its canonical bytes, or every
    // existing pin would stop matching the revision it names.
    let (definition, topology) = defaults();
    assert!(definition.desks.is_empty());
    let canonical = definition.canonicalize().expect("canonical");
    assert!(!canonical.json().contains("\"desks\""));
    assert_eq!(
        canonical.hash().as_str(),
        "217747248d527556fa452a0b6380215a3699def6ba73cedbaabdc00da3b4da56",
        "the ASMA-8117 census recorded this hash for the live v1 revision",
    );
    definition
        .validate_against(&topology)
        .expect("the default still composes");
}
