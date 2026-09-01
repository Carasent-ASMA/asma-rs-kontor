//! Exact Team Definition rendering contracts beyond the shipped fixture:
//! every recommended container and seat row, byte-exact separator behavior,
//! and the fail-closed ambiguity refusals the naming contract requires.

use kontor_core::id::{
    ExternalName, RoleCode, RoleSlotId, SCHEMA_VERSION, SpecVersion, TeamDefinitionId,
    TopologyKindKey, TopologySpecId,
};
use kontor_core::naming::{
    NameSeparator, NativeNameSegment, NativeNameTemplate, NativeNameToken, NativeNameValues,
};
use kontor_core::spec::{
    NodeProjectionCapability, TeamContainerDefinition, TeamDefinitionSeatSlot, TeamDefinitionSpec,
    TopologySnapshot,
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

fn role_coded_slot(id: &str, code: &str) -> TeamDefinitionSeatSlot {
    TeamDefinitionSeatSlot {
        slot_id: RoleSlotId::parse(id).expect("a slot"),
        role_code: Some(RoleCode::parse(code).expect("a role code")),
        display_name: None,
        capability_profile: name("independent-advisor"),
    }
}

fn display_named_slot(id: &str, label: &str) -> TeamDefinitionSeatSlot {
    TeamDefinitionSeatSlot {
        slot_id: RoleSlotId::parse(id).expect("a slot"),
        role_code: None,
        display_name: Some(name(label)),
        capability_profile: name("independent-reviewer"),
    }
}

/// The recommended ASMA definition: the contract table of
/// `docs/NATIVE_NAMING.md`, expressed as one validated revision.
fn recommended() -> TeamDefinitionSpec {
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
        definition_id: TeamDefinitionId::parse("01936f5a-2000-7000-8000-000000000009")
            .expect("a definition id"),
        version: SpecVersion::FIRST,
        name: name("Recommended ASMA Team Definition"),
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
                    display_named_slot("reviewer-a", "SEAT A"),
                    display_named_slot("reviewer-b", "SEAT B"),
                    display_named_slot("judge", "JUDGE"),
                ],
            ),
        ],
    }
}

fn render_container(
    definition: &TeamDefinitionSpec,
    code: &str,
    values: &NativeNameValues,
) -> ExternalName {
    definition
        .container(&kind(code))
        .expect("the container is configured")
        .name_template
        .render(&definition.separator, values)
        .expect("the configured values render")
}

fn render_seat(
    definition: &TeamDefinitionSpec,
    code: &str,
    values: &NativeNameValues,
) -> ExternalName {
    definition
        .container(&kind(code))
        .expect("the container is configured")
        .seat_name_template
        .as_ref()
        .expect("the seat template is configured")
        .render(&definition.separator, values)
        .expect("the configured seat values render")
}

#[test]
fn every_recommended_container_row_renders_the_exact_contract_bytes() {
    let definition = recommended();
    definition.validate().expect("the definition is valid");

    let epic = |prefix: &str| {
        NativeNameValues::new()
            .with_prefix(prefix)
            .with_epic_item_code("KBI-8049")
    };
    let task = |prefix: &str| {
        NativeNameValues::new()
            .with_prefix(prefix)
            .with_task_item_code("KBI-8062")
    };
    let scope = |prefix: &str, item: &str, topic: &str| {
        NativeNameValues::new()
            .with_prefix(prefix)
            .with_scope_item_code(item)
            .with_topic(topic)
    };

    assert_eq!(
        render_container(&definition, "ESW", &epic("ESW")).as_str(),
        "ESW • KBI-8049"
    );
    assert_eq!(
        render_container(&definition, "ECP", &epic("ECP")).as_str(),
        "ECP • KBI-8049"
    );
    assert_eq!(
        render_container(&definition, "TSW", &task("TSW")).as_str(),
        "TSW • KBI-8062"
    );
    assert_eq!(
        render_container(
            &definition,
            "ASW",
            &scope("ASW", "KBI-8049", "Jira recovery")
        )
        .as_str(),
        "ASW • KBI-8049 • Jira recovery",
        "an epic-scoped subject carries the epic item code"
    );
    assert_eq!(
        render_container(
            &definition,
            "ASW",
            &scope("ASW", "KBI-8062", "Jira recovery")
        )
        .as_str(),
        "ASW • KBI-8062 • Jira recovery",
        "a task-scoped subject carries the task item code"
    );
    assert_eq!(
        render_container(
            &definition,
            "CSW",
            &scope("CSW", "KBI-8062", "Naming contract")
        )
        .as_str(),
        "CSW • KBI-8062 • Naming contract"
    );
    assert_eq!(
        render_container(
            &definition,
            "CSW",
            &scope("CSW", "KBI-8049", "Release readiness")
        )
        .as_str(),
        "CSW • KBI-8049 • Release readiness",
        "an epic-global CSW keeps the epic item code and stays a Committee workspace"
    );
}

#[test]
fn the_separator_is_exact_bullet_bytes_and_never_a_normalized_lookalike() {
    let definition = recommended();
    let epic = NativeNameValues::new()
        .with_prefix("ESW")
        .with_epic_item_code("KBI-8049");
    let rendered = render_container(&definition, "ESW", &epic);
    assert_eq!(
        rendered.as_str().as_bytes(),
        b"ESW \xe2\x80\xa2 KBI-8049",
        "the separator is exactly SPACE, U+2022 BULLET, SPACE"
    );

    // The separator is specification-owned data. A different configured
    // separator renders its own bytes; nothing normalizes one into the other.
    let mut middot = recommended();
    middot.separator = NameSeparator::parse(" · ").expect("the middle-dot separator");
    let rendered = render_container(&middot, "ESW", &epic);
    assert_eq!(rendered.as_str(), "ESW · KBI-8049");
    assert_ne!(rendered.as_str().as_bytes(), b"ESW \xe2\x80\xa2 KBI-8049");
}

#[test]
fn every_local_seat_row_renders_exactly_from_the_pinned_definition() {
    let definition = recommended();
    definition.validate().expect("the definition is valid");

    let role = |code: &str| NativeNameValues::new().with_role_code(code);
    assert_eq!(
        render_seat(&definition, "ECP", &role("LSA")).as_str(),
        "LSA"
    );
    assert_eq!(
        render_seat(&definition, "ECP", &role("TPM")).as_str(),
        "TPM"
    );
    assert_eq!(
        render_seat(&definition, "TSW", &role("AUD")).as_str(),
        "AUD",
        "a TSW seat is the exact registered delivery role code"
    );
    assert_eq!(
        render_seat(&definition, "ASW", &role("SA")).as_str(),
        "SA",
        "an ASW seat is the exact configured advisor role code"
    );

    // The committee labels are exactly the configured slot display names, in
    // the order the pinned definition declares them, with no invented suffix
    // and no scope or topic leaked into a seat title.
    let labelled = |label: &str| NativeNameValues::new().with_slot_display_name(label);
    assert_eq!(
        render_seat(&definition, "CSW", &labelled("SEAT A")).as_str(),
        "SEAT A"
    );
    assert_eq!(
        render_seat(&definition, "CSW", &labelled("SEAT B")).as_str(),
        "SEAT B"
    );
    assert_eq!(
        render_seat(&definition, "CSW", &labelled("JUDGE")).as_str(),
        "JUDGE"
    );
    let leaked = NativeNameValues::new()
        .with_slot_display_name("SEAT A")
        .with_prefix("CSW")
        .with_scope_item_code("KBI-8062")
        .with_topic("Naming contract");
    assert_eq!(
        render_seat(&definition, "CSW", &leaked).as_str(),
        "SEAT A",
        "a seat title never repeats the container's scope or topic"
    );
}

#[test]
fn a_seat_template_repeating_container_scope_is_refused() {
    let mut definition = recommended();
    let tsw = definition
        .containers
        .iter_mut()
        .find(|container| container.kind == kind("TSW"))
        .expect("the TSW container");
    tsw.seat_name_template = Some(template(vec![
        NativeNameToken::Prefix,
        NativeNameToken::TaskItemCode,
    ]));
    assert!(
        definition.validate().is_err(),
        "a seat name must never carry the container prefix or an item code"
    );
}

#[test]
fn duplicate_container_prefixes_are_ambiguous_and_refused() {
    let mut definition = recommended();
    let asw = definition
        .containers
        .iter_mut()
        .find(|container| container.kind == kind("ASW"))
        .expect("the ASW container");
    asw.prefix = name("CSW");
    assert!(
        definition.validate().is_err(),
        "two container kinds rendering from one prefix leave every name ambiguous"
    );
}

#[test]
fn a_topic_containing_the_separator_is_refused_as_ambiguous() {
    let definition = recommended();
    let ambiguous = NativeNameValues::new()
        .with_prefix("CSW")
        .with_scope_item_code("KBI-8062")
        .with_topic("Naming • contract");
    assert!(
        definition
            .container(&kind("CSW"))
            .expect("CSW")
            .name_template
            .render(&definition.separator, &ambiguous)
            .is_err(),
        "a topic that contains the separator cannot be told apart from its segments"
    );
}

#[test]
fn role_coded_and_display_named_seat_policies_are_mutually_exclusive() {
    // A display-labelled committee must not fall back to role codes.
    let mut role_code_csw = recommended();
    let csw = role_code_csw
        .containers
        .iter_mut()
        .find(|container| container.kind == kind("CSW"))
        .expect("the CSW container");
    csw.seat_name_template = Some(template(vec![NativeNameToken::RoleCode]));
    assert!(
        role_code_csw.validate().is_err(),
        "a committee pinned to SEAT A/SEAT B/JUDGE must not render role codes"
    );

    // And a role-coded container must not silently depend on display labels.
    let mut display_named_asw = recommended();
    let asw = display_named_asw
        .containers
        .iter_mut()
        .find(|container| container.kind == kind("ASW"))
        .expect("the ASW container");
    asw.seat_name_template = Some(template(vec![NativeNameToken::SlotDisplayName]));
    asw.slots = vec![role_coded_slot("software-architect", "SA")];
    assert!(
        display_named_asw.validate().is_err(),
        "SLOT_DISPLAY_NAME requires display-named local slots"
    );
}
