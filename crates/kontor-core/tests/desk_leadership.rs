//! Desk-led epic leadership: the parent-hosted subject projection and the
//! optional Team Definition field that selects it (ASMA-8452).
//!
//! A leadership subject owns no native container. It sits directly below a
//! desk's workspace, borrows that workspace for its seats, and names the work
//! those seats do in a role-led title. Every rule here is a published-document
//! rule: nothing is decided from a kind's spelling.

use kontor_core::backlog_identity::ConfirmedJiraKey;
use kontor_core::id::{ExternalId, SCHEMA_VERSION};
use kontor_core::naming::NativeNameValues;
use kontor_core::spec::{NodeProjectionCapability, ProjectSessionTopologySpec, TeamDefinitionSpec};
use serde_json::{Value, json};

fn token(value: &str) -> Value {
    json!({"kind": "token", "value": value})
}

fn literal(value: &str) -> Value {
    json!({"kind": "literal", "value": value})
}

fn help(name: &str) -> Value {
    json!({"full_name": name, "meaning": "A kind.", "category": "session_topology",
           "lifecycle": "current"})
}

fn node_kind(kind: &str, parents: &[&str], capabilities: &[&str], seat: bool) -> Value {
    let mut declared = json!({
        "kind": kind,
        "allowed_parents": parents,
        "cardinality": {"minimum": 0},
        "projection_capabilities": capabilities,
        "read_only": false,
        "name_template": {"segments": [token("AREA_CODE")]},
        "code_help": help(kind),
    });
    if seat {
        declared["seat_name_template"] = json!({"segments": [token("AREA_CODE")]});
    }
    declared
}

/// A project root, an epic with a ticket, and one desk whose workspace hosts
/// an epic and an ad-hoc leadership subject.
fn topology() -> Value {
    let mut root = node_kind("PSW", &[], &["native_root"], false);
    root["cardinality"] = json!({"minimum": 1, "maximum": 1});
    json!({
        "schema_version": SCHEMA_VERSION,
        "spec_id": "01936f5a-1000-7000-8000-000000000001",
        "version": 7,
        "name": "Desk-led topology",
        "root_kind": "PSW",
        "node_kinds": [
            root,
            node_kind("ESW", &["PSW"], &["native_root"], false),
            node_kind("TSW", &["ESW"], &["native_child", "session_host"], true),
            node_kind("DESK", &["PSW"], &["native_root"], false),
            node_kind("DWS", &["DESK"], &["native_child", "session_host"], true),
            node_kind("ADE", &["DWS"], &["parent_session_host"], true),
            node_kind("ADH", &["DWS"], &["parent_session_host"], true),
        ],
    })
}

fn parse_topology(document: Value) -> ProjectSessionTopologySpec {
    serde_json::from_value(document).expect("a topology document")
}

fn container(kind: &str, parent: Option<&str>, capabilities: &[&str]) -> Value {
    json!({
        "kind": kind,
        "parent": parent,
        "prefix": kind,
        "projection_capabilities": capabilities,
        "read_only": false,
        "name_template": {"segments": [token("PREFIX")]},
    })
}

fn role_slot(slot: &str, role: &str) -> Value {
    json!({"slot_id": slot, "role_code": role, "capability_profile": "epic-leadership"})
}

/// The definition the topology above validates, with its leadership on the
/// `adam` desk.
fn definition() -> Value {
    let mut esw = container("ESW", None, &["native_root"]);
    esw["name_template"] = json!({"segments": [token("PREFIX"), token("EPIC_JIRA_KEY")]});
    let mut tsw = container("TSW", Some("ESW"), &["native_child", "session_host"]);
    tsw["name_template"] = json!({"segments": [token("PREFIX"), token("TASK_JIRA_KEY")]});
    tsw["seat_name_template"] = json!({"segments": [token("ROLE_CODE")]});
    let mut desk = container("DESK", None, &["native_root"]);
    desk["name_template"] = json!({"segments": [token("PREFIX"), token("DESK_NAME")]});
    desk["prefix"] = json!("DESK");
    let mut workspace = container("DWS", Some("DESK"), &["native_child", "session_host"]);
    workspace["prefix"] = json!("ADAM");
    let mut epic = container("ADE", Some("DWS"), &["parent_session_host"]);
    epic["seat_name_template"] = json!({"segments": [
        token("ROLE_CODE"), token("EPIC_JIRA_KEY"), token("TOPIC"),
    ]});
    epic["slots"] = json!([role_slot("lsa", "LSA"), role_slot("tpm", "TPM")]);
    let mut ad_hoc = container("ADH", Some("DWS"), &["parent_session_host"]);
    ad_hoc["seat_name_template"] = json!({"segments": [
        token("ROLE_CODE"), literal("ADHOC"), token("TOPIC"),
    ]});
    ad_hoc["slots"] = json!([role_slot("lsa", "LSA")]);
    let topology = parse_topology(topology());
    json!({
        "schema_version": SCHEMA_VERSION,
        "definition_id": "01936f5a-2000-7000-8000-000000000001",
        "version": 8,
        "name": "Desk-led Team Definition",
        "topology": {
            "spec_id": topology.spec_id,
            "version": topology.version,
            "canonical_hash": topology.canonicalize().expect("canonical").hash(),
        },
        "separator": " • ",
        "containers": [esw, tsw, desk, workspace, epic, ad_hoc],
        "desks": [{"desk_key": "adam", "display_name": "ADAM", "kind": "DESK",
                   "workspace_kind": "DWS"}],
        "desk_leadership": {"desk_key": "adam", "epic_kind": "ADE", "ad_hoc_kind": "ADH"},
    })
}

fn parse_definition(document: Value) -> TeamDefinitionSpec {
    serde_json::from_value(document).expect("a definition document")
}

fn refusal(document: Value) -> String {
    parse_definition(document)
        .validate()
        .expect_err("the definition is refused")
        .to_string()
}

#[test]
fn a_desk_led_definition_validates_against_its_topology() {
    let topology = parse_topology(topology());
    topology.validate().expect("the topology is legal");
    let definition = parse_definition(definition());
    definition
        .validate_against(&topology)
        .expect("the definition composes with its topology");
    let leadership = definition
        .desk_leadership
        .as_ref()
        .expect("the leadership is declared");
    assert_eq!(leadership.desk_key.as_str(), "adam");
}

#[test]
fn a_parent_session_host_is_exclusive_and_needs_a_native_hosting_parent() {
    // Beside any native projection it would describe a second place for the
    // same seats; beside logical_only it would contradict itself.
    for extra in [
        "native_child",
        "session_host",
        "logical_only",
        "native_root",
    ] {
        let mut document = topology();
        document["node_kinds"][5]["projection_capabilities"] =
            json!(["parent_session_host", extra]);
        assert!(
            parse_topology(document).validate().is_err(),
            "parent_session_host beside {extra} is refused"
        );
    }
    // A parent that is not itself a native session host has no container to
    // lend: an epic project hosts no seats.
    let mut document = topology();
    document["node_kinds"][5]["allowed_parents"] = json!(["ESW"]);
    let error = parse_topology(document)
        .validate()
        .expect_err("a non-hosting parent is refused");
    assert!(
        error
            .to_string()
            .contains("only below native session hosts"),
        "{error}"
    );
    // One acceptable parent among unacceptable ones is still refused.
    let mut document = topology();
    document["node_kinds"][5]["allowed_parents"] = json!(["DWS", "ESW"]);
    assert!(parse_topology(document).validate().is_err());
    // A seat-hosting kind always declares how its seats are named.
    let mut document = topology();
    document["node_kinds"][5]
        .as_object_mut()
        .expect("a kind")
        .remove("seat_name_template");
    assert!(parse_topology(document).validate().is_err());
}

#[test]
fn hosting_is_answered_from_capabilities_alone() {
    assert!(NodeProjectionCapability::hosts_sessions(&[
        NodeProjectionCapability::ParentSessionHost
    ]));
    assert!(NodeProjectionCapability::hosts_in_parent(&[
        NodeProjectionCapability::ParentSessionHost
    ]));
    assert!(NodeProjectionCapability::hosts_sessions(&[
        NodeProjectionCapability::NativeChild,
        NodeProjectionCapability::SessionHost,
    ]));
    assert!(!NodeProjectionCapability::hosts_in_parent(&[
        NodeProjectionCapability::NativeChild,
        NodeProjectionCapability::SessionHost,
    ]));
    assert!(!NodeProjectionCapability::hosts_sessions(&[
        NodeProjectionCapability::NativeRoot
    ]));
}

#[test]
fn only_a_parent_hosted_subject_may_follow_its_role_with_a_subject() {
    // A delivery seat keeps the bare role its ticket workspace already scopes.
    let mut document = definition();
    document["containers"][1]["seat_name_template"] =
        json!({"segments": [token("ROLE_CODE"), token("TASK_JIRA_KEY")]});
    assert!(refusal(document).contains("exactly ROLE_CODE or SLOT_DISPLAY_NAME"));
    // A subject title still leads with the role.
    let mut document = definition();
    document["containers"][4]["seat_name_template"] =
        json!({"segments": [token("EPIC_JIRA_KEY"), token("ROLE_CODE")]});
    assert!(refusal(document).contains("exactly ROLE_CODE or SLOT_DISPLAY_NAME"));
    // And may name only the epic key, its topic and literals after it.
    let mut document = definition();
    document["containers"][4]["seat_name_template"] =
        json!({"segments": [token("ROLE_CODE"), token("TASK_JIRA_KEY")]});
    assert!(refusal(document).contains("exactly ROLE_CODE or SLOT_DISPLAY_NAME"));
}

#[test]
fn desk_leadership_is_refused_unless_it_names_two_subjects_below_a_declared_desk() {
    let mut undeclared = definition();
    undeclared["desk_leadership"]["desk_key"] = json!("pr-review");
    assert!(refusal(undeclared).contains("does not declare"));

    let mut same = definition();
    same["desk_leadership"]["ad_hoc_kind"] = json!("ADE");
    assert!(refusal(same).contains("distinct kinds"));

    // A subject placed anywhere but directly below the desk's workspace.
    let mut misplaced = definition();
    misplaced["desk_leadership"]["epic_kind"] = json!("TSW");
    assert!(refusal(misplaced).contains("directly below the desk's workspace"));

    // An ad-hoc subject exists before any epic, so it has no key to render.
    let mut ad_hoc_key = definition();
    ad_hoc_key["containers"][5]["seat_name_template"] =
        json!({"segments": [token("ROLE_CODE"), token("EPIC_JIRA_KEY"), token("TOPIC")]});
    assert!(refusal(ad_hoc_key).contains("no epic key to render"));

    let mut unknown = definition();
    unknown["desk_leadership"]["surprise"] = json!(true);
    assert!(serde_json::from_value::<TeamDefinitionSpec>(unknown).is_err());
}

#[test]
fn a_definition_without_desk_leadership_keeps_its_canonical_bytes() {
    let mut document = definition();
    document
        .as_object_mut()
        .expect("a definition")
        .remove("desk_leadership");
    let definition = parse_definition(document);
    assert!(definition.desk_leadership.is_none());
    let canonical = definition.canonicalize().expect("canonical");
    let text = canonical.json();
    assert!(
        !text.contains("desk_leadership"),
        "an absent policy adds no bytes: {text}"
    );
}

#[test]
fn role_led_subject_titles_render_the_exact_naming_card_bytes() {
    let definition = parse_definition(definition());
    let render = |kind: &str, values: NativeNameValues| {
        definition
            .containers
            .iter()
            .find(|container| container.kind.as_str() == kind)
            .and_then(|container| container.seat_name_template.as_ref())
            .expect("a seat template")
            .render(&definition.separator, &values)
            .expect("renders")
            .as_str()
            .to_owned()
    };
    let key = ConfirmedJiraKey::parse(&ExternalId::parse("ASMA-8449").expect("an external id"))
        .expect("a confirmed key");
    assert_eq!(
        render(
            "ADE",
            NativeNameValues::default()
                .with_role_code("LSA")
                .with_epic_jira_key(&key)
                .with_topic("kontord durable desks"),
        ),
        "LSA • ASMA-8449 • kontord durable desks"
    );
    assert_eq!(
        render(
            "ADE",
            NativeNameValues::default()
                .with_role_code("TPM")
                .with_epic_jira_key(&key)
                .with_topic("kontord durable desks"),
        ),
        "TPM • ASMA-8449 • kontord durable desks"
    );
    assert_eq!(
        render(
            "ADH",
            NativeNameValues::default()
                .with_role_code("LSA")
                .with_topic("release readiness"),
        ),
        "LSA • ADHOC • release readiness"
    );
    // A topic carrying the separator cannot forge a fourth segment.
    let forged = definition.containers[5]
        .seat_name_template
        .as_ref()
        .expect("a seat template")
        .render(
            &definition.separator,
            &NativeNameValues::default()
                .with_role_code("LSA")
                .with_topic("x • TPM"),
        );
    assert!(forged.is_err());
}
