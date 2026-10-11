//! PR review placement: the optional Team Definition field that places one
//! pull request's Adam and review room on a PR desk (ASMA-8453).
//!
//! The Adam sits directly below the desk's workspace and borrows it; the room
//! is a native workspace directly below the desk. Both are named by the pull
//! request's confirmed Jira key and `<repo>#<n>`, so two pull requests never
//! share a title. Every rule here is a published-document rule: nothing is
//! decided from a kind's spelling.

use kontor_core::backlog_identity::ConfirmedJiraKey;
use kontor_core::id::{ExternalId, RoleSlotId, SCHEMA_VERSION, TopologyKindKey};
use kontor_core::naming::NativeNameValues;
use kontor_core::pull_request::{ForgeRepository, PullRequestRef};
use kontor_core::spec::{ProjectSessionTopologySpec, TeamDefinitionSpec};
use serde_json::{Value, json};

fn token(value: &str) -> Value {
    json!({"kind": "token", "value": value})
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

/// A project root, an epic with a ticket, and one PR desk whose workspace
/// hosts each pull request's Adam and whose project holds each review room.
fn topology() -> Value {
    let mut root = node_kind("PSW", &[], &["native_root"], false);
    root["cardinality"] = json!({"minimum": 1, "maximum": 1});
    json!({
        "schema_version": SCHEMA_VERSION,
        "spec_id": "01936f5a-1000-7000-8000-000000000001",
        "version": 8,
        "name": "PR review topology",
        "root_kind": "PSW",
        "node_kinds": [
            root,
            node_kind("ESW", &["PSW"], &["native_root"], false),
            node_kind("TSW", &["ESW"], &["native_child", "session_host"], true),
            node_kind("DESK", &["PSW"], &["native_root"], false),
            node_kind("DWS", &["DESK"], &["native_child", "session_host"], true),
            node_kind("PRA", &["DWS"], &["parent_session_host"], true),
            node_kind("RSW", &["DESK"], &["native_child", "session_host"], true),
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

fn label_slot(slot: &str, label: &str) -> Value {
    json!({"slot_id": slot, "display_name": label, "capability_profile": "independent-reviewer"})
}

/// The definition the topology above validates, reviewing on `pr-review`.
fn definition() -> Value {
    let mut esw = container("ESW", None, &["native_root"]);
    esw["name_template"] = json!({"segments": [token("PREFIX"), token("EPIC_JIRA_KEY")]});
    let mut tsw = container("TSW", Some("ESW"), &["native_child", "session_host"]);
    tsw["name_template"] = json!({"segments": [token("PREFIX"), token("TASK_JIRA_KEY")]});
    tsw["seat_name_template"] = json!({"segments": [token("ROLE_CODE")]});
    let mut desk = container("DESK", None, &["native_root"]);
    desk["name_template"] = json!({"segments": [token("PREFIX"), token("DESK_NAME")]});
    let mut workspace = container("DWS", Some("DESK"), &["native_child", "session_host"]);
    workspace["prefix"] = json!("ADAM");
    let mut adam = container("PRA", Some("DWS"), &["parent_session_host"]);
    adam["seat_name_template"] = json!({"segments": [
        token("ROLE_CODE"), token("SCOPE_JIRA_KEY"), token("PULL_REQUEST"),
    ]});
    adam["slots"] = json!([{"slot_id": "tpm", "role_code": "TPM",
                            "capability_profile": "pr-coordination"}]);
    let mut room = container("RSW", Some("DESK"), &["native_child", "session_host"]);
    room["name_template"] = json!({"segments": [
        token("PREFIX"), token("SCOPE_JIRA_KEY"), token("PULL_REQUEST"),
    ]});
    room["seat_name_template"] = json!({"segments": [token("SLOT_DISPLAY_NAME")]});
    room["slots"] = json!([
        label_slot("rev-a", "REV A"),
        label_slot("rev-b", "REV B"),
        {"slot_id": "judge", "display_name": "JUDGE", "capability_profile": "committee-judge"},
    ]);
    let topology = parse_topology(topology());
    json!({
        "schema_version": SCHEMA_VERSION,
        "definition_id": "01936f5a-2000-7000-8000-000000000001",
        "version": 9,
        "name": "PR review Team Definition",
        "topology": {
            "spec_id": topology.spec_id,
            "version": topology.version,
            "canonical_hash": topology.canonicalize().expect("canonical").hash(),
        },
        "separator": " • ",
        "containers": [esw, tsw, desk, workspace, adam, room],
        "desks": [{"desk_key": "pr-review", "display_name": "PR REVIEW", "kind": "DESK",
                   "workspace_kind": "DWS"}],
        "pr_review": {
            "desk_key": "pr-review",
            "adam_kind": "PRA",
            "room_kind": "RSW",
            "room_roles": [
                {"slot_id": "rev-a", "role_code": "AUD"},
                {"slot_id": "rev-b", "role_code": "AUD"},
                {"slot_id": "judge", "role_code": "AUD"},
            ],
        },
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

fn container_index(document: &Value, kind: &str) -> usize {
    document["containers"]
        .as_array()
        .expect("containers")
        .iter()
        .position(|container| container["kind"] == kind)
        .expect("the kind is declared")
}

fn pull_request(repository: &str, number: u64) -> PullRequestRef {
    PullRequestRef::new(
        ForgeRepository::parse(repository).expect("a repository"),
        number,
    )
    .expect("a pull request")
}

fn key(text: &str) -> ConfirmedJiraKey {
    ConfirmedJiraKey::parse(&ExternalId::parse(text).expect("an id")).expect("a confirmed key")
}

#[test]
fn a_pr_review_definition_validates_against_its_topology() {
    let topology = parse_topology(topology());
    topology.validate().expect("the topology is legal");
    let definition = parse_definition(definition());
    definition
        .validate_against(&topology)
        .expect("the definition composes with its topology");
    let review = definition.pr_review.as_ref().expect("declared");
    assert_eq!(review.desk_key.as_str(), "pr-review");
    for slot in ["rev-a", "rev-b", "judge"] {
        let slot = RoleSlotId::parse(slot).expect("a slot");
        assert_eq!(
            definition
                .pr_review_role(&slot)
                .map(kontor_core::id::RoleCode::as_str),
            Some("AUD")
        );
    }
    assert!(
        definition
            .pr_review_role(&RoleSlotId::parse("tpm").expect("a slot"))
            .is_none(),
        "the Adam's seat is role-named and needs no declared role"
    );
}

#[test]
fn a_definition_without_pr_review_keeps_its_exact_bytes() {
    let mut document = definition();
    let object = document.as_object_mut().expect("an object");
    object.remove("pr_review");
    for kind in ["PRA", "RSW"] {
        let index = container_index(&Value::Object(object.clone()), kind);
        object["containers"]
            .as_array_mut()
            .expect("containers")
            .remove(index);
    }
    let parsed = parse_definition(document.clone());
    assert!(parsed.pr_review.is_none());
    let serialized = serde_json::to_value(&parsed).expect("serializes");
    assert!(
        serialized.get("pr_review").is_none(),
        "an undeclared PR review is absent from the canonical bytes"
    );
    assert_eq!(
        parsed.canonicalize().expect("canonical").hash(),
        kontor_core::id::CanonicalDocument::from_value(&document)
            .expect("canonical")
            .hash(),
        "the hash is the hash of the document as written"
    );
}

#[test]
fn titles_render_the_key_and_pull_request_and_never_collapse() {
    let definition = parse_definition(definition());
    let room = definition
        .container(&TopologyKindKey::parse("RSW").expect("a kind"))
        .expect("the room");
    let adam = definition
        .container(&TopologyKindKey::parse("PRA").expect("a kind"))
        .expect("the Adam");
    let render_room = |key: &ConfirmedJiraKey, pull_request: &PullRequestRef| {
        room.name_template
            .render(
                &definition.separator,
                &NativeNameValues::new()
                    .with_prefix(room.prefix.as_str())
                    .with_scope_jira_key(key)
                    .with_pull_request(pull_request),
            )
            .expect("renders")
            .as_str()
            .to_owned()
    };
    let render_adam = |key: &ConfirmedJiraKey, pull_request: &PullRequestRef| {
        adam.seat_name_template
            .as_ref()
            .expect("a seat template")
            .render(
                &definition.separator,
                &NativeNameValues::new()
                    .with_role_code("TPM")
                    .with_scope_jira_key(key)
                    .with_pull_request(pull_request),
            )
            .expect("renders")
            .as_str()
            .to_owned()
    };
    let ticket = key("ASMA-8453");
    let first = pull_request("Carasent-ASMA/asma-rs-kontor", 297);
    let second = pull_request("Carasent-ASMA/asma-rs-kontor", 298);
    let elsewhere = pull_request("Carasent-ASMA/asma-modules", 297);
    assert_eq!(
        render_room(&ticket, &first),
        "RSW • ASMA-8453 • asma-rs-kontor#297"
    );
    assert_eq!(
        render_adam(&ticket, &first),
        "TPM • ASMA-8453 • asma-rs-kontor#297"
    );
    // Two pull requests of one key, and one number in two repositories.
    assert_ne!(render_room(&ticket, &first), render_room(&ticket, &second));
    assert_ne!(
        render_room(&ticket, &first),
        render_room(&ticket, &elsewhere)
    );
    assert_ne!(render_adam(&ticket, &first), render_adam(&ticket, &second));
    assert_ne!(
        render_adam(&ticket, &first),
        render_adam(&ticket, &elsewhere)
    );
    // A pull request has no value to fall back on.
    assert!(
        room.name_template
            .render(
                &definition.separator,
                &NativeNameValues::new()
                    .with_prefix("RSW")
                    .with_scope_jira_key(&ticket),
            )
            .is_err()
    );
}

#[test]
fn the_declaration_must_name_a_declared_desk_and_two_kinds_of_its_own() {
    let mut document = definition();
    document["pr_review"]["desk_key"] = json!("adam");
    assert!(refusal(document).contains("does not declare"));

    let mut document = definition();
    document["pr_review"]["room_kind"] = json!("PRA");
    assert!(refusal(document).contains("two kinds of their own"));

    let mut document = definition();
    document["pr_review"]["adam_kind"] = json!("DWS");
    assert!(refusal(document).contains("two kinds of their own"));
}

#[test]
fn the_adam_is_a_borrowing_subject_below_the_workspace_with_one_titled_seat() {
    let adam = container_index(&definition(), "PRA");

    let mut document = definition();
    document["containers"][adam]["parent"] = json!("DESK");
    assert!(refusal(document).contains("parent_session_host directly below"));

    // A kind with a container of its own keeps bare role titles, so the
    // generic seat rule refuses its subject-scoped title first.
    let mut document = definition();
    document["containers"][adam]["projection_capabilities"] =
        json!(["native_child", "session_host"]);
    assert!(refusal(document).contains("exactly ROLE_CODE or SLOT_DISPLAY_NAME"));

    // Without the pull request its title collapses two PRs of one key.
    let mut document = definition();
    document["containers"][adam]["seat_name_template"] =
        json!({"segments": [token("ROLE_CODE"), token("SCOPE_JIRA_KEY")]});
    assert!(refusal(document).contains("one role-named seat"));

    // Without the key it is not the ticket's.
    let mut document = definition();
    document["containers"][adam]["seat_name_template"] =
        json!({"segments": [token("ROLE_CODE"), token("PULL_REQUEST")]});
    assert!(refusal(document).contains("one role-named seat"));

    let mut document = definition();
    document["containers"][adam]["slots"] = json!([
        {"slot_id": "tpm", "role_code": "TPM", "capability_profile": "pr-coordination"},
        {"slot_id": "lsa", "role_code": "LSA", "capability_profile": "pr-coordination"},
    ]);
    assert!(refusal(document).contains("one role-named seat"));
}

#[test]
fn the_room_is_a_native_workspace_below_the_desk_named_by_its_pull_request() {
    let room = container_index(&definition(), "RSW");

    let mut document = definition();
    document["containers"][room]["parent"] = json!("DWS");
    assert!(refusal(document).contains("native_child session_host directly below the desk"));

    let mut document = definition();
    document["containers"][room]["projection_capabilities"] = json!(["native_child"]);
    let error = refusal(document);
    assert!(
        error.contains("native_child session_host directly below the desk"),
        "{error}"
    );

    let mut document = definition();
    document["containers"][room]["name_template"] =
        json!({"segments": [token("PREFIX"), token("SCOPE_JIRA_KEY")]});
    assert!(refusal(document).contains("named by PREFIX"));

    let mut document = definition();
    document["containers"][room]["name_template"] = json!({"segments": [
        token("PREFIX"), token("SCOPE_JIRA_KEY"), token("PULL_REQUEST"), token("TOPIC"),
    ]});
    assert!(refusal(document).contains("named by PREFIX"));

    let mut document = definition();
    document["containers"][room]["seat_name_template"] = json!({"segments": [token("ROLE_CODE")]});
    document["containers"][room]["slots"] = json!([
        {"slot_id": "rev-a", "role_code": "AUD", "capability_profile": "independent-reviewer"},
    ]);
    document["pr_review"]["room_roles"] = json!([{"slot_id": "rev-a", "role_code": "AUD"}]);
    assert!(refusal(document).contains("named by PREFIX"));
}

#[test]
fn every_room_slot_has_exactly_one_declared_role() {
    let mut document = definition();
    document["pr_review"]["room_roles"]
        .as_array_mut()
        .expect("roles")
        .pop();
    assert!(refusal(document).contains("every room slot exactly once"));

    let mut document = definition();
    document["pr_review"]["room_roles"]
        .as_array_mut()
        .expect("roles")
        .push(json!({"slot_id": "rev-a", "role_code": "SA"}));
    assert!(refusal(document).contains("every room slot exactly once"));

    let mut document = definition();
    document["pr_review"]["room_roles"]
        .as_array_mut()
        .expect("roles")
        .push(json!({"slot_id": "rev-c", "role_code": "AUD"}));
    assert!(refusal(document).contains("every room slot exactly once"));
}

#[test]
fn the_adam_slot_is_never_a_room_slot() {
    // Routes are keyed by slot, so a shared id could not name each seat once.
    let mut document = definition();
    let adam = container_index(&document, "PRA");
    document["containers"][adam]["slots"][0]["slot_id"] = json!("judge");
    assert!(refusal(document).contains("must differ from every room slot"));
}

#[test]
fn no_other_container_may_render_a_pull_request_or_a_scope_key_seat() {
    let tsw = container_index(&definition(), "TSW");
    let mut document = definition();
    document["containers"][tsw]["name_template"] = json!({"segments": [
        token("PREFIX"), token("TASK_JIRA_KEY"), token("PULL_REQUEST"),
    ]});
    assert!(refusal(document).contains("only a PR review Adam or room"));

    // Without any declaration, nothing may render one at all.
    let mut document = definition();
    document
        .as_object_mut()
        .expect("an object")
        .remove("pr_review");
    assert!(refusal(document).contains("only a PR review Adam or room"));

    // The room's seats are its labels; only the Adam's seat names a key.
    let room = container_index(&definition(), "RSW");
    let mut document = definition();
    document["containers"][room]["seat_name_template"] =
        json!({"segments": [token("SLOT_DISPLAY_NAME"), token("SCOPE_JIRA_KEY")]});
    refusal(document);
}

#[test]
fn the_topology_must_permit_both_edges() {
    let topology = {
        let mut document = topology();
        document["node_kinds"][6]["allowed_parents"] = json!(["DWS"]);
        parse_topology(document)
    };
    let mut document = definition();
    document["topology"]["canonical_hash"] =
        json!(topology.canonicalize().expect("canonical").hash());
    let error = parse_definition(document)
        .validate_against(&topology)
        .expect_err("the topology refuses the room below the desk");
    assert!(error.to_string().contains("parent"), "{error}");
}
