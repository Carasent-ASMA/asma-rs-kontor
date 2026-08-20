//! The seeded ASMA Operational topology and role vocabulary.
//!
//! The mutants this suite exists to kill:
//!
//! * reinstating `LSA` or `TPM` as topology-node kinds instead of role codes;
//! * letting an ESW carry zero or several Epic Control Planes;
//! * giving one control role its own workspace again;
//! * shipping a code with no server-owned full name or meaning, which forces a
//!   client to invent one;
//! * declaring a compatibility or retired code as a usable node kind;
//! * letting `SA` stand in for the mandatory `LSA` slot.

use kontor_core::id::{RoleCode, SpecVersion, TopologyKindKey};
use kontor_core::naming::{NativeNameSegment, NativeNameToken};
use kontor_core::spec::{
    CodeCategory, CodeLifecycle, NodeCardinality, NodeProjectionCapability,
    ProjectSessionTopologySpec, TopologyNodeKindSpec,
};
use kontor_profiles::bundled_operational_domain;

fn topology() -> ProjectSessionTopologySpec {
    bundled_operational_domain()
        .expect("the bundled domain validates")
        .topology_specs
        .first()
        .expect("a seeded topology")
        .clone()
}

fn kind(text: &str) -> TopologyKindKey {
    TopologyKindKey::parse(text).expect("a valid kind key")
}

fn declared(spec: &ProjectSessionTopologySpec, text: &str) -> TopologyNodeKindSpec {
    spec.node_kinds
        .iter()
        .find(|entry| entry.kind == kind(text))
        .unwrap_or_else(|| panic!("{text} is declared"))
        .clone()
}

fn token(value: NativeNameToken) -> NativeNameSegment {
    NativeNameSegment::Token(value)
}

#[test]
fn the_operational_default_is_exactly_the_approved_kind_vocabulary() {
    let spec = topology();
    let codes: Vec<String> = spec
        .node_kinds
        .iter()
        .map(|entry| entry.kind.to_string())
        .collect();
    assert_eq!(
        codes,
        ["PSW", "QSW", "ESW", "ECP", "TSW", "ASW", "CSW"],
        "the seeded vocabulary is the approved dictionary, in its documented order"
    );
}

#[test]
fn one_epic_control_plane_sits_under_each_epic_workspace() {
    let spec = topology();
    let ecp = declared(&spec, "ECP");
    assert_eq!(ecp.allowed_parents, vec![kind("ESW")]);
    assert_eq!(
        ecp.cardinality,
        NodeCardinality {
            minimum: 1,
            maximum: Some(1)
        },
        "exactly one ECP per ESW -- not optional, not several"
    );
    assert!(
        ecp.projection_capabilities
            .contains(&NodeProjectionCapability::SessionHost),
        "the ECP hosts the control seats itself"
    );
    assert!(
        ecp.projection_capabilities
            .contains(&NodeProjectionCapability::NativeChild),
        "the ECP is one ordinary workspace under the epic project"
    );
    assert!(
        !ecp.projection_capabilities
            .contains(&NodeProjectionCapability::NativeRoot),
        "the default claims no nested Paseo project"
    );
    assert!(!ecp.read_only);
}

#[test]
fn operational_v1_owns_the_exact_native_name_matrix_and_separator_bytes() {
    let spec = topology();
    assert_eq!(
        spec.spec_id.to_string(),
        "01936f5a-1000-7000-8000-000000000001"
    );
    assert_eq!(spec.version, SpecVersion::parse(1).expect("v1"));
    assert_eq!(spec.name_separator.as_str().as_bytes(), " • ".as_bytes());

    let scoped = [
        token(NativeNameToken::AreaCode),
        token(NativeNameToken::JiraCode),
        token(NativeNameToken::KontorBacklogCode),
    ];
    for area in ["ESW", "ECP", "TSW"] {
        assert_eq!(
            declared(&spec, area).name_template.segments(),
            Some(scoped.as_slice()),
            "{area} uses the same specification-owned scope template"
        );
    }
    assert_eq!(
        declared(&spec, "ECP")
            .seat_name_template
            .as_ref()
            .and_then(|template| template.segments()),
        Some(scoped.as_slice())
    );
    let ticket_seat = [
        token(NativeNameToken::AreaCode),
        token(NativeNameToken::KontorBacklogCode),
    ];
    assert_eq!(
        declared(&spec, "TSW")
            .seat_name_template
            .as_ref()
            .and_then(|template| template.segments()),
        Some(ticket_seat.as_slice())
    );

    for (area, literal) in [
        ("PSW", "Project Session Workspace"),
        ("QSW", "Quick Session Workspace"),
        ("ASW", "Advisor Session Workspace"),
        ("CSW", "Committee Session Workspace"),
    ] {
        let expected = [NativeNameSegment::Literal(
            kontor_core::id::ExternalName::parse(literal).expect("a fixture literal"),
        )];
        assert_eq!(
            declared(&spec, area).name_template.segments(),
            Some(expected.as_slice())
        );
    }
    for area in ["QSW", "ASW", "CSW"] {
        let role_only = [token(NativeNameToken::AreaCode)];
        assert_eq!(
            declared(&spec, area)
                .seat_name_template
                .as_ref()
                .and_then(|template| template.segments()),
            Some(role_only.as_slice())
        );
    }

    let serialized = serde_json::to_string(&spec).expect("the seed serializes");
    assert!(
        !serialized.contains('\u{00b7}'),
        "the v1 seed contains no U+00B7 fallback punctuation"
    );
}

#[test]
fn control_roles_are_seat_bindings_and_never_topology_kinds() {
    let spec = topology();
    for role in ["LSA", "TPM", "SA", "SEAT"] {
        assert!(
            spec.node_kinds.iter().all(|entry| entry.kind != kind(role)),
            "{role} is a role code or a binding, never a node kind"
        );
    }
    let catalog = bundled_operational_domain()
        .expect("the bundled domain validates")
        .role_catalogs
        .first()
        .expect("a seeded catalog")
        .clone();
    for role in ["LSA", "TPM", "SA"] {
        assert!(
            catalog
                .role(&RoleCode::parse(role).expect("a valid role code"))
                .is_some(),
            "{role} is a standard role code"
        );
    }
}

#[test]
fn the_lead_architect_slot_cannot_be_filled_by_a_plain_architect() {
    let catalog = bundled_operational_domain()
        .expect("the bundled domain validates")
        .role_catalogs
        .first()
        .expect("a seeded catalog")
        .clone();
    let lead = catalog
        .role(&RoleCode::parse("LSA").expect("a valid role code"))
        .expect("LSA exists");
    let plain = catalog
        .role(&RoleCode::parse("SA").expect("a valid role code"))
        .expect("SA exists");
    assert_ne!(lead.role_code, plain.role_code);
    assert_ne!(lead.standard_title, plain.standard_title);
    assert_eq!(lead.standard_title.as_str(), "Lead Software Architect");
    assert_eq!(plain.standard_title.as_str(), "Software Architect");
    assert!(
        plain.responsibility_summary.as_str().contains("LSA"),
        "SA's own meaning says it cannot satisfy the LSA slot"
    );
}

#[test]
fn every_seeded_code_carries_server_owned_help() {
    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let spec = domain.topology_specs.first().expect("a seeded topology");
    for entry in &spec.node_kinds {
        let help = &entry.code_help;
        assert!(!help.full_name.as_str().trim().is_empty());
        assert!(!help.meaning.as_str().trim().is_empty());
        assert_ne!(
            help.full_name.as_str(),
            entry.kind.as_str(),
            "{} needs a real expansion, not its own code echoed back",
            entry.kind
        );
        assert_eq!(help.category, CodeCategory::SessionTopology);
        assert_eq!(help.lifecycle, CodeLifecycle::Current);
    }
    for role in &domain
        .role_catalogs
        .first()
        .expect("a seeded catalog")
        .roles
    {
        assert!(!role.standard_title.as_str().trim().is_empty());
        assert!(!role.responsibility_summary.as_str().trim().is_empty());
        assert_ne!(
            role.responsibility_summary.as_str(),
            role.standard_title.as_str(),
            "{} needs a meaning, not its title repeated",
            role.role_code
        );
        assert_eq!(role.lifecycle, CodeLifecycle::Current);
    }
}

#[test]
fn historical_codes_are_explained_but_never_usable() {
    let spec = topology();
    let tsc = spec.code_help(&kind("TSC")).expect("TSC is explained");
    assert_eq!(tsc.lifecycle, CodeLifecycle::Compatibility);
    assert_eq!(tsc.full_name.as_str(), "Ticket Session Committee");

    let pase = spec.code_help(&kind("PASE")).expect("PASE is explained");
    assert_eq!(pase.lifecycle, CodeLifecycle::Retired);

    for historical in ["TSC", "PASE"] {
        assert!(
            spec.node_kinds
                .iter()
                .all(|entry| entry.kind != kind(historical)),
            "{historical} is explained, never declared as a usable kind"
        );
    }
}

#[test]
fn help_resolves_for_declared_kinds_and_stays_absent_for_unknown_ones() {
    let spec = topology();
    assert_eq!(
        spec.code_help(&kind("ECP"))
            .expect("ECP is declared")
            .full_name
            .as_str(),
        "Epic Control Plane"
    );
    assert!(
        spec.code_help(&kind("NOPE")).is_none(),
        "an unknown code stays visibly unknown rather than being guessed"
    );
}

#[test]
fn a_specification_cannot_declare_a_non_current_code_as_a_usable_kind() {
    let mut spec = topology();
    // Take the ECP entry and mark it compatibility-only while leaving it
    // declared: the vocabulary would then offer a kind nothing may create.
    let index = spec
        .node_kinds
        .iter()
        .position(|entry| entry.kind == kind("ECP"))
        .expect("ECP is declared");
    spec.node_kinds[index].code_help.lifecycle = CodeLifecycle::Compatibility;
    assert!(spec.validate().is_err());

    // The mirror: a current code hidden in the historical list.
    let mut spec = topology();
    spec.historical_codes[0].help.lifecycle = CodeLifecycle::Current;
    assert!(spec.validate().is_err());

    // And a code that is both declared and explained as historical.
    let mut spec = topology();
    spec.historical_codes[0].kind = kind("ECP");
    assert!(spec.validate().is_err());
}

// ---------------------------------------------------------------------------
// Open-question closers (OP-REQ-038)
// ---------------------------------------------------------------------------

#[test]
fn both_open_question_closers_are_declared_codes_of_the_pinned_catalog() {
    let pack = bundled_operational_domain().expect("the bundled domain validates");
    let catalog = pack
        .role_catalogs
        .first()
        .expect("a seeded role catalog")
        .clone();

    for code in [
        &pack.delivery.architecture_closer_code,
        &pack.delivery.process_closer_code,
    ] {
        assert!(
            catalog.role(code).is_some(),
            "the closer code {code} must exist in the pinned catalog"
        );
    }
    assert_ne!(
        pack.delivery.architecture_closer_code, pack.delivery.process_closer_code,
        "the split is only a split if the two closers are different roles"
    );
}

#[test]
fn a_closer_code_the_catalog_does_not_declare_is_refused() {
    // The whole point of declaring closers as data is that they are checked
    // against the catalog. A code nobody declared would authorize a seat that
    // cannot exist, and the refusal is what stops that shipping.
    let mut pack = bundled_operational_domain().expect("the bundled domain validates");
    pack.delivery.architecture_closer_code =
        RoleCode::parse("NOPE").expect("a lexically valid code");
    assert!(
        pack.validate().is_err(),
        "a closer the catalog does not declare must be refused"
    );
}
