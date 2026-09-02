//! Team Definition ownership and exact native-name rendering contracts.

use kontor_core::id::{
    ExternalName, RoleCode, RoleSlotId, SCHEMA_VERSION, SpecVersion, TeamDefinitionId,
    TopologyKindKey, TopologySpecId,
};
use kontor_core::naming::{
    NameSeparator, NativeNameSegment, NativeNameTemplate, NativeNameToken, NativeNameValues,
};
use kontor_core::spec::{
    NodeProjectionCapability, TeamContainerDefinition, TeamDefinitionSeatSlot,
    TeamDefinitionSnapshot, TeamDefinitionSpec, TopologySnapshot,
};

fn name(value: &str) -> ExternalName {
    ExternalName::parse(value).expect("a valid external name")
}

fn kind(value: &str) -> TopologyKindKey {
    TopologyKindKey::parse(value).expect("a valid topology kind")
}

fn token(value: NativeNameToken) -> NativeNameSegment {
    NativeNameSegment::Token(value)
}

fn template(tokens: Vec<NativeNameToken>) -> NativeNameTemplate {
    NativeNameTemplate::from_segments(tokens.into_iter().map(token).collect())
        .expect("a valid typed template")
}

fn definition() -> TeamDefinitionSpec {
    let container = |code: &str,
                     parent: Option<&str>,
                     tokens: Vec<NativeNameToken>,
                     seat_tokens: Option<Vec<NativeNameToken>>,
                     read_only: bool,
                     slots: Vec<TeamDefinitionSeatSlot>| {
        TeamContainerDefinition {
            kind: kind(code),
            parent: parent.map(kind),
            prefix: name(code),
            projection_capabilities: vec![if code == "ESW" {
                NodeProjectionCapability::NativeRoot
            } else {
                NodeProjectionCapability::NativeChild
            }],
            read_only,
            name_template: template(tokens),
            seat_name_template: seat_tokens.map(template),
            slots,
            team_slots: Vec::new(),
        }
    };
    TeamDefinitionSpec {
        schema_version: SCHEMA_VERSION,
        definition_id: TeamDefinitionId::parse("01936f5a-2000-7000-8000-000000000001")
            .expect("a definition id"),
        version: SpecVersion::FIRST,
        name: name("ASMA Operational Team Definition"),
        topology: TopologySnapshot {
            spec_id: TopologySpecId::parse("01936f5a-1000-7000-8000-000000000001")
                .expect("a topology id"),
            version: SpecVersion::parse(4).expect("topology v4"),
            canonical_hash: kontor_core::id::ContentHash::of(b"topology-v4"),
        },
        separator: NameSeparator::parse(" • ").expect("the exact bullet separator"),
        containers: vec![
            container(
                "ESW",
                None,
                vec![NativeNameToken::Prefix, NativeNameToken::EpicItemCode],
                None,
                false,
                vec![],
            ),
            container(
                "ECP",
                Some("ESW"),
                vec![NativeNameToken::Prefix, NativeNameToken::EpicItemCode],
                Some(vec![NativeNameToken::RoleCode]),
                false,
                vec![],
            ),
            container(
                "TSW",
                Some("ESW"),
                vec![NativeNameToken::Prefix, NativeNameToken::TaskItemCode],
                Some(vec![NativeNameToken::RoleCode]),
                false,
                vec![],
            ),
            container(
                "ASW",
                Some("ESW"),
                vec![
                    NativeNameToken::Prefix,
                    NativeNameToken::ScopeItemCode,
                    NativeNameToken::Topic,
                ],
                Some(vec![NativeNameToken::RoleCode]),
                true,
                vec![
                    TeamDefinitionSeatSlot {
                        slot_id: RoleSlotId::parse("software-architect").expect("a slot"),
                        role_code: Some(RoleCode::parse("SA").expect("a role code")),
                        display_name: None,
                        capability_profile: name("independent-advisor"),
                    },
                    TeamDefinitionSeatSlot {
                        slot_id: RoleSlotId::parse("auditor").expect("a slot"),
                        role_code: Some(RoleCode::parse("AUD").expect("a role code")),
                        display_name: None,
                        capability_profile: name("independent-advisor"),
                    },
                ],
            ),
            container(
                "CSW",
                Some("ESW"),
                vec![
                    NativeNameToken::Prefix,
                    NativeNameToken::ScopeItemCode,
                    NativeNameToken::Topic,
                ],
                Some(vec![NativeNameToken::SlotDisplayName]),
                true,
                vec![
                    TeamDefinitionSeatSlot {
                        slot_id: RoleSlotId::parse("reviewer-a").expect("a slot"),
                        role_code: None,
                        display_name: Some(name("SEAT A")),
                        capability_profile: name("independent-reviewer"),
                    },
                    TeamDefinitionSeatSlot {
                        slot_id: RoleSlotId::parse("reviewer-b").expect("a slot"),
                        role_code: None,
                        display_name: Some(name("SEAT B")),
                        capability_profile: name("independent-reviewer"),
                    },
                    TeamDefinitionSeatSlot {
                        slot_id: RoleSlotId::parse("judge").expect("a slot"),
                        role_code: None,
                        display_name: Some(name("JUDGE")),
                        capability_profile: name("committee-judge"),
                    },
                ],
            ),
        ],
    }
}

#[test]
fn recommended_team_definition_renders_the_exact_container_and_local_seat_matrix() {
    let definition = definition();
    definition.validate().expect("the definition is valid");

    let render = |code: &str, values: NativeNameValues| {
        definition
            .container(&kind(code))
            .expect("the container is configured")
            .name_template
            .render(&definition.separator, &values)
            .expect("the configured values render")
    };
    assert_eq!(
        render(
            "ESW",
            NativeNameValues::new()
                .with_prefix("ESW")
                .with_epic_item_code("KBI-8049")
        )
        .as_str(),
        "ESW • KBI-8049"
    );
    assert_eq!(
        render(
            "TSW",
            NativeNameValues::new()
                .with_prefix("TSW")
                .with_task_item_code("KBI-8062")
        )
        .as_str(),
        "TSW • KBI-8062"
    );
    assert_eq!(
        render(
            "ASW",
            NativeNameValues::new()
                .with_prefix("ASW")
                .with_scope_item_code("KBI-8049")
                .with_topic("Jira recovery")
        )
        .as_str(),
        "ASW • KBI-8049 • Jira recovery"
    );
    assert_eq!(
        render(
            "CSW",
            NativeNameValues::new()
                .with_prefix("CSW")
                .with_scope_item_code("KBI-8062")
                .with_topic("Naming contract")
        )
        .as_str(),
        "CSW • KBI-8062 • Naming contract"
    );

    let ecp = definition.container(&kind("ECP")).expect("ECP");
    assert_eq!(
        ecp.seat_name_template
            .as_ref()
            .expect("seat template")
            .render(
                &definition.separator,
                &NativeNameValues::new().with_role_code("LSA")
            )
            .expect("role code renders")
            .as_str(),
        "LSA"
    );
    let csw = definition.container(&kind("CSW")).expect("CSW");
    assert_eq!(
        csw.seat_name_template
            .as_ref()
            .expect("seat template")
            .render(
                &definition.separator,
                &NativeNameValues::new().with_slot_display_name("SEAT A")
            )
            .expect("slot display name renders")
            .as_str(),
        "SEAT A"
    );
}

#[test]
fn a_missing_topic_or_local_seat_value_fails_closed() {
    let definition = definition();
    let asw = definition.container(&kind("ASW")).expect("ASW");
    assert!(
        asw.name_template
            .render(
                &definition.separator,
                &NativeNameValues::new()
                    .with_prefix("ASW")
                    .with_scope_item_code("KBI-8049")
            )
            .is_err(),
        "the renderer never invents a topic"
    );
    let csw = definition.container(&kind("CSW")).expect("CSW");
    assert!(
        csw.seat_name_template
            .as_ref()
            .expect("seat template")
            .render(&definition.separator, &NativeNameValues::new())
            .is_err(),
        "the renderer never substitutes a role code for a configured slot label"
    );
}

#[test]
fn the_snapshot_binds_the_exact_definition_bytes() {
    let definition = definition();
    let snapshot = TeamDefinitionSnapshot::from_revision(&definition).expect("a snapshot");
    assert_eq!(snapshot.definition_id, definition.definition_id);
    assert_eq!(snapshot.version, definition.version);
    assert_eq!(
        snapshot.canonical_hash,
        definition
            .canonicalize()
            .expect("canonical bytes")
            .hash()
            .clone()
    );
}

#[test]
fn team_definitions_reject_every_legacy_naming_source() {
    for forbidden in [
        NativeNameToken::AreaCode,
        NativeNameToken::JiraCode,
        NativeNameToken::KontorBacklogCode,
        NativeNameToken::ItemCode,
        NativeNameToken::AiShortName,
    ] {
        let mut definition = definition();
        definition.containers[0].name_template = template(vec![forbidden]);
        let error = definition
            .validate()
            .expect_err("legacy topology tokens must remain read-only compatibility data");
        assert!(
            error.to_string().contains("may use only PREFIX"),
            "unexpected refusal for {forbidden:?}: {error}"
        );
    }
}

#[test]
fn unknown_fields_fail_closed_at_every_nested_definition_level() {
    let document = serde_json::to_value(definition()).expect("serializable definition");
    for path in ["topology", "template", "segment"] {
        let mut changed = document.clone();
        match path {
            "topology" => {
                changed["topology"]["unknown"] = serde_json::json!(true);
            }
            "template" => {
                changed["containers"][0]["name_template"]["unknown"] = serde_json::json!(true);
            }
            "segment" => {
                changed["containers"][0]["name_template"]["segments"][0]["unknown"] =
                    serde_json::json!(true);
            }
            _ => unreachable!(),
        }
        serde_json::from_value::<TeamDefinitionSpec>(changed)
            .expect_err("unknown nested fields must never be discarded before hashing");
    }
}

#[test]
fn team_slots_register_delivery_roles_without_inferring_them() {
    let mut definition = definition();
    let tsw = definition
        .containers
        .iter_mut()
        .find(|container| container.kind.as_str() == "TSW")
        .expect("the TSW container");
    tsw.team_slots = vec![
        TeamDefinitionSeatSlot {
            slot_id: RoleSlotId::parse("scope").expect("a slot"),
            role_code: Some(RoleCode::parse("SA").expect("a role code")),
            display_name: None,
            capability_profile: name("delivery-standard"),
        },
        TeamDefinitionSeatSlot {
            slot_id: RoleSlotId::parse("audit").expect("a slot"),
            role_code: Some(RoleCode::parse("AUD").expect("a role code")),
            display_name: None,
            capability_profile: name("delivery-high"),
        },
    ];
    definition
        .validate()
        .expect("registered delivery slots are a valid configuration");
    assert_eq!(
        definition
            .team_slot(&kind("TSW"), &RoleSlotId::parse("scope").expect("a slot"))
            .and_then(|slot| slot.role_code.as_ref())
            .map(RoleCode::as_str),
        Some("SA")
    );
    // A slot nobody registered has no answer, and the caller must refuse rather
    // than derive one from the slot's spelling or its logical role.
    assert!(
        definition
            .team_slot(
                &kind("TSW"),
                &RoleSlotId::parse("researcher-a").expect("a slot")
            )
            .is_none()
    );
}

#[test]
fn alternative_templates_may_register_one_role_code_under_different_slot_ids() {
    let mut definition = definition();
    let tsw = definition
        .containers
        .iter_mut()
        .find(|container| container.kind.as_str() == "TSW")
        .expect("the TSW container");
    // One catalog serves several alternative delivery templates. `scope` and
    // `architect` are both `SA`, and no TeamRun declares both, so the catalog
    // holds them side by side. Two slots of one *run* rendering the same name
    // is a different question, and admission is what answers it.
    tsw.team_slots = vec![
        TeamDefinitionSeatSlot {
            slot_id: RoleSlotId::parse("scope").expect("a slot"),
            role_code: Some(RoleCode::parse("SA").expect("a role code")),
            display_name: None,
            capability_profile: name("delivery-standard"),
        },
        TeamDefinitionSeatSlot {
            slot_id: RoleSlotId::parse("architect").expect("a slot"),
            role_code: Some(RoleCode::parse("SA").expect("a role code")),
            display_name: None,
            capability_profile: name("delivery-standard"),
        },
    ];
    definition
        .validate()
        .expect("a catalog may register one role code under different slot ids");
}

#[test]
fn a_team_slot_the_seat_template_cannot_render_is_refused() {
    let mut definition = definition();
    let tsw = definition
        .containers
        .iter_mut()
        .find(|container| container.kind.as_str() == "TSW")
        .expect("the TSW container");
    // The TSW seat template renders ROLE_CODE, so a label-only team slot
    // promises a name the renderer cannot produce.
    tsw.team_slots = vec![TeamDefinitionSeatSlot {
        slot_id: RoleSlotId::parse("scope").expect("a slot"),
        role_code: None,
        display_name: Some(name("SCOPE")),
        capability_profile: name("delivery-standard"),
    }];
    assert!(definition.validate().is_err());
}

#[test]
fn a_duplicate_team_slot_id_is_refused() {
    let mut definition = definition();
    let tsw = definition
        .containers
        .iter_mut()
        .find(|container| container.kind.as_str() == "TSW")
        .expect("the TSW container");
    let slot = TeamDefinitionSeatSlot {
        slot_id: RoleSlotId::parse("scope").expect("a slot"),
        role_code: Some(RoleCode::parse("SA").expect("a role code")),
        display_name: None,
        capability_profile: name("delivery-standard"),
    };
    tsw.team_slots = vec![slot.clone(), slot];
    assert!(definition.validate().is_err());
}
