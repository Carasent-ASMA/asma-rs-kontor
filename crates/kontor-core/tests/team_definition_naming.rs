//! Team Definition ownership and exact native-name rendering contracts.

use kontor_core::id::{
    ExternalName, RoleSlotId, SCHEMA_VERSION, SpecVersion, TeamDefinitionId, TopologyKindKey,
    TopologySpecId,
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
                vec![],
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
                        display_name: name("SEAT A"),
                        capability_profile: name("independent-reviewer"),
                    },
                    TeamDefinitionSeatSlot {
                        slot_id: RoleSlotId::parse("reviewer-b").expect("a slot"),
                        display_name: name("SEAT B"),
                        capability_profile: name("independent-reviewer"),
                    },
                    TeamDefinitionSeatSlot {
                        slot_id: RoleSlotId::parse("judge").expect("a slot"),
                        display_name: name("JUDGE"),
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
