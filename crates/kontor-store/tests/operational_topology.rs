//! OP-01 generic topology, seat, adaptive-state and export round trip.

use kontor_core::id::{
    AggregateRevision, ExternalId, ExternalName, MiniProjectId, ProjectId, RoleCode, RoleSlotId,
    SeatBindingId, SpecVersion, Timestamp, TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::naming::{NativeNameSegment, NativeNameTemplate, NativeNameToken};
use kontor_core::repository::{
    AdaptiveAdmissionAdvance, MiniProjectTopologySnapshot, NewAdaptiveAdmissionState,
    NewMiniProject, NewProject, NewSeatBinding, NewSessionTopologyNode, ProjectRepository,
    ProjectTopologyDefault, RepositoryError, TopologyRepository,
};
use kontor_core::spec::{
    CatalogRoleRef, Shareability, ShareabilityClass, ShareabilityClassifier,
    ShareabilityProvenance, ShareabilityTier, TopologySnapshot,
};
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use kontor_store::backup::export_realm;
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

/// The stamp ordinary work produces: nobody was asked.
fn default_stamp() -> Shareability {
    Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("tier B classifies")
}

#[test]
fn operational_state_survives_restart_and_typed_export() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the store opens");
    let project_id = ProjectId::generate();
    let mini_project_id = MiniProjectId::generate();
    let created_at = at("2026-08-16T01:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Operational project"),
            root_path: name("/tmp/operational-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_mini_project(&NewMiniProject {
            id: mini_project_id,
            project_id,
            name: name("Operational epic"),
            created_at,
        })
        .expect("the MiniProject is created");

    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let topology = domain.topology_specs.first().expect("a topology").clone();
    let catalog = domain
        .role_catalogs
        .first()
        .expect("a role catalog")
        .clone();
    let canonical_hash = store
        .publish_topology_spec(project_id, &topology, &default_stamp(), created_at)
        .expect("the topology is published");
    store
        .publish_role_catalog(&catalog, &default_stamp(), created_at)
        .expect("the catalog is published");
    let snapshot = TopologySnapshot {
        spec_id: topology.spec_id,
        version: topology.version,
        canonical_hash,
    };
    store
        .set_project_topology_default(&ProjectTopologyDefault {
            project_id,
            topology: snapshot.clone(),
            selected_at: created_at,
        })
        .expect("the project default is selected");
    store
        .pin_mini_project_topology(&MiniProjectTopologySnapshot {
            project_id,
            mini_project_id,
            topology: snapshot.clone(),
            pinned_at: created_at,
        })
        .expect("the MiniProject snapshot is pinned");

    let root_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: root_id,
            project_id,
            mini_project_id: None,
            topology: snapshot.clone(),
            kind: TopologyKindKey::parse("PSW").expect("the root kind"),
            parent_id: None,
            task_id: None,
            created_at,
        })
        .expect("the project root is created");
    let epic_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: epic_id,
            project_id,
            mini_project_id: Some(mini_project_id),
            topology: snapshot.clone(),
            kind: TopologyKindKey::parse("ESW").expect("the epic kind"),
            parent_id: Some(root_id),
            task_id: None,
            created_at,
        })
        .expect("the epic node is created below the project root");
    // One ECP under the ESW, hosting the epic's control seats directly
    // (OP-REQ-040). LSA and TPM are role codes on SeatBindings, never nodes.
    let ecp_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: ecp_id,
            project_id,
            mini_project_id: Some(mini_project_id),
            topology: snapshot.clone(),
            kind: TopologyKindKey::parse("ECP").expect("the control-plane kind"),
            parent_id: Some(epic_id),
            task_id: None,
            created_at,
        })
        .expect("the ECP node is created below the epic");
    for (code, slot, label) in [
        ("LSA", "epic.lsa", "Kontor LSA"),
        ("TPM", "epic.tpm", "Kontor TPM"),
    ] {
        let entry = catalog
            .role(&RoleCode::parse(code).expect("a standard role code"))
            .expect("the catalog has the role");
        store
            .create_seat_binding(&NewSeatBinding {
                id: SeatBindingId::generate(),
                project_id,
                topology_node_id: ecp_id,
                role_slot_id: RoleSlotId::parse(slot).expect("a role slot"),
                role: CatalogRoleRef {
                    catalog_id: catalog.catalog_id,
                    catalog_revision: catalog.version,
                    role_code: entry.role_code.clone(),
                    standard_title: entry.standard_title.clone(),
                    custom_display_name: Some(name(label)),
                },
                task_id: None,
                team_run_id: None,
                attach_deadline: at("2026-08-16T01:10:00Z"),
                parent_seat_binding_id: None,
                created_at,
            })
            .expect("the typed seat binding is created");
    }

    let observation = ExternalId::parse("clean-observation-1").expect("an observation id");
    store
        .create_adaptive_admission_state(&NewAdaptiveAdmissionState {
            project_id,
            mini_project_id,
            current_window: 4,
            clean_observation_streak: 0,
            last_observation_id: None,
            created_at,
        })
        .expect("adaptive state is created");
    store
        .advance_adaptive_admission_state(&AdaptiveAdmissionAdvance {
            project_id,
            mini_project_id,
            current_window: 4,
            clean_observation_streak: 1,
            last_observation_id: Some(observation.clone()),
            expected_revision: AggregateRevision::INITIAL,
            updated_at: at("2026-08-16T01:01:00Z"),
        })
        .expect("the first observation advances state");
    assert!(matches!(
        store.advance_adaptive_admission_state(&AdaptiveAdmissionAdvance {
            project_id,
            mini_project_id,
            current_window: 4,
            clean_observation_streak: 1,
            last_observation_id: Some(observation),
            expected_revision: AggregateRevision::INITIAL.next().expect("revision two"),
            updated_at: at("2026-08-16T01:02:00Z"),
        }),
        Err(RepositoryError::Conflict { .. })
    ));

    drop(store);
    let reopened = SqliteStore::open(&database).expect("the store reopens");
    assert_eq!(
        reopened
            .list_topology_nodes(project_id, Some(mini_project_id))
            .expect("nodes are readable")
            .len(),
        2
    );
    assert_eq!(
        reopened
            .list_seat_bindings(project_id, ecp_id)
            .expect("bindings are readable")
            .len(),
        2,
        "one ECP hosts both control seats rather than one workspace each"
    );
    assert_eq!(
        reopened
            .get_adaptive_admission_state(project_id, mini_project_id)
            .expect("state is readable")
            .expect("state exists")
            .clean_observation_streak,
        1
    );
    assert_eq!(
        reopened
            .get_topology_spec_shareability(project_id, topology.spec_id, topology.version)
            .expect("the classification is readable")
            .expect("the revision is classified"),
        default_stamp(),
        "a published specification keeps its write-time stamp across restart"
    );

    let export = export_realm(&reopened, at("2026-08-16T02:00:00Z")).expect("the Realm exports");
    assert_eq!(export.records.topology_specs.len(), 1);
    assert_eq!(export.records.topology_nodes.len(), 3);
    assert_eq!(export.records.seat_bindings.len(), 2);
    assert_eq!(export.records.adaptive_admission_state.len(), 1);

    let exported = export
        .records
        .topology_specs
        .first()
        .expect("the exported specification");
    assert_eq!(exported.shareability_class, "project_shared");
    assert_eq!(exported.shareability_classifier, None);
    assert_eq!(exported.shareability_provenance, "type_default");
    let exported_catalog = export
        .records
        .role_catalog_revisions
        .first()
        .expect("the exported catalog");
    assert_eq!(exported_catalog.shareability_class, "project_shared");
    assert_eq!(exported_catalog.shareability_provenance, "type_default");
}

#[test]
fn a_human_override_is_stored_whole_and_read_back() {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    let created_at = at("2026-08-16T01:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Withheld project"),
            root_path: name("/tmp/withheld-project"),
            created_at,
        })
        .expect("the project is created");

    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let topology = domain.topology_specs.first().expect("a topology").clone();
    let withheld = Shareability::overridden_by(
        ShareabilityTier::ProjectKnowledge,
        ShareabilityClass::KontorLocal,
        name("Lead Software Architect"),
    )
    .expect("tier B accepts an override");
    store
        .publish_topology_spec(project_id, &topology, &withheld, created_at)
        .expect("the withheld topology is published");

    let stored = store
        .get_topology_spec_shareability(project_id, topology.spec_id, topology.version)
        .expect("the classification is readable")
        .expect("the revision is classified");
    assert_eq!(stored, withheld);
    assert_eq!(stored.class, ShareabilityClass::KontorLocal);
    assert_eq!(stored.provenance, ShareabilityProvenance::HumanOverride);
    assert_eq!(
        stored.classifier.identity().map(ExternalName::to_string),
        Some("Lead Software Architect".to_owned()),
        "an override names the human who made it"
    );
}

#[test]
fn a_published_classification_cannot_be_revised_after_the_fact() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the store opens");
    let project_id = ProjectId::generate();
    let created_at = at("2026-08-16T01:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Immutable project"),
            root_path: name("/tmp/immutable-project"),
            created_at,
        })
        .expect("the project is created");
    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let topology = domain.topology_specs.first().expect("a topology").clone();
    store
        .publish_topology_spec(project_id, &topology, &default_stamp(), created_at)
        .expect("the topology is published");

    let connection = rusqlite::Connection::open(&database).expect("a direct connection");
    let reclassified = connection.execute(
        "UPDATE topology_specs SET shareability_class = 'kontor_local'",
        [],
    );
    assert!(
        reclassified.is_err(),
        "no surface may reclassify a published revision"
    );
}

#[test]
fn an_unattributed_override_is_refused_by_the_schema() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the store opens");
    let project_id = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Unattributed project"),
            root_path: name("/tmp/unattributed-project"),
            created_at: at("2026-08-16T01:00:00Z"),
        })
        .expect("the project is created");

    let connection = rusqlite::Connection::open(&database).expect("a direct connection");
    let anonymous = connection.execute(
        "INSERT INTO topology_specs
             (project_id, spec_id, version, name, root_kind, definition, definition_hash,
              published_at, shareability_class, shareability_classifier, shareability_provenance)
         VALUES (?1, 'spec', 1, 'n', 'PSW', '{}',
                 '0000000000000000000000000000000000000000000000000000000000000000',
                 '2026-08-16T01:00:00Z', 'kontor_local', NULL, 'human_override')",
        rusqlite::params![project_id.to_string()],
    );
    assert!(
        anonymous.is_err(),
        "an override with no human identity is not a stamp anyone made"
    );
}

/// Publishing refuses a stamp the schema alone cannot catch.
///
/// The insert trigger only proves that an override names a human. A class the
/// tier's default rule would never have produced, wearing that rule's identity,
/// is well-formed as far as SQLite is concerned — withholding a tier-B document
/// is a human decision, and the domain check at the publish boundary is the
/// only thing that says so. The mutant this kills is dropping `validate_for`
/// from the publish path.
#[test]
fn publishing_refuses_a_class_nobody_chose() {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    let created_at = at("2026-08-16T01:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Forged project"),
            root_path: name("/tmp/forged-project"),
            created_at,
        })
        .expect("the project is created");

    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let topology = domain.topology_specs.first().expect("a topology").clone();
    let catalog = domain
        .role_catalogs
        .first()
        .expect("a role catalog")
        .clone();
    let forged = Shareability {
        class: ShareabilityClass::KontorLocal,
        classifier: ShareabilityClassifier::TypeDefaultRule,
        provenance: ShareabilityProvenance::TypeDefault,
    };

    assert!(
        store
            .publish_topology_spec(project_id, &topology, &forged, created_at)
            .is_err(),
        "a withheld specification must name the human who withheld it"
    );
    assert!(
        store
            .publish_role_catalog(&catalog, &forged, created_at)
            .is_err(),
        "a withheld catalog must name the human who withheld it"
    );
    assert_eq!(
        store
            .get_topology_spec_shareability(project_id, topology.spec_id, topology.version)
            .expect("the read succeeds"),
        None,
        "a refused publish leaves nothing behind"
    );
}

#[test]
fn repinning_an_epic_migrates_compatible_nodes_without_changing_their_identities_or_revisions() {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the store opens");
    let project_id = ProjectId::generate();
    let mini_project_id = MiniProjectId::generate();
    let created_at = at("2026-08-20T20:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Topology upgrade project"),
            root_path: name("/tmp/topology-upgrade-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_mini_project(&NewMiniProject {
            id: mini_project_id,
            project_id,
            name: name("Topology upgrade epic"),
            created_at,
        })
        .expect("the epic is created");

    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let first = domain.topology_specs.first().expect("a topology").clone();
    let first_hash = store
        .publish_topology_spec(project_id, &first, &default_stamp(), created_at)
        .expect("version one is published");
    let first_snapshot = TopologySnapshot {
        spec_id: first.spec_id,
        version: first.version,
        canonical_hash: first_hash,
    };
    store
        .set_project_topology_default(&ProjectTopologyDefault {
            project_id,
            topology: first_snapshot.clone(),
            selected_at: created_at,
        })
        .expect("the project default is selected");
    store
        .pin_mini_project_topology(&MiniProjectTopologySnapshot {
            project_id,
            mini_project_id,
            topology: first_snapshot.clone(),
            pinned_at: created_at,
        })
        .expect("the epic is pinned");

    let root_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: root_id,
            project_id,
            mini_project_id: None,
            topology: first_snapshot.clone(),
            kind: TopologyKindKey::parse("PSW").expect("the root kind"),
            parent_id: None,
            task_id: None,
            created_at,
        })
        .expect("the project root is created");
    let epic_node_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: epic_node_id,
            project_id,
            mini_project_id: Some(mini_project_id),
            topology: first_snapshot.clone(),
            kind: TopologyKindKey::parse("ESW").expect("the epic kind"),
            parent_id: Some(root_id),
            task_id: None,
            created_at,
        })
        .expect("the epic node is created");
    let control_node_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: control_node_id,
            project_id,
            mini_project_id: Some(mini_project_id),
            topology: first_snapshot.clone(),
            kind: TopologyKindKey::parse("ECP").expect("the control kind"),
            parent_id: Some(epic_node_id),
            task_id: None,
            created_at,
        })
        .expect("the control node is created");
    let revisions_before = store
        .list_topology_nodes(project_id, Some(mini_project_id))
        .expect("the nodes read")
        .into_iter()
        .map(|node| (node.id, node.revision))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut second = first.clone();
    second.version = SpecVersion::parse(2).expect("version two");
    second
        .node_kinds
        .iter_mut()
        .find(|kind| kind.kind.as_str() == "ESW")
        .expect("the epic kind")
        .name_template = NativeNameTemplate::from_segments(vec![
        NativeNameSegment::Literal(name("Upgraded ESW")),
        NativeNameSegment::Token(NativeNameToken::JiraCode),
        NativeNameSegment::Token(NativeNameToken::KontorBacklogCode),
    ])
    .expect("the upgraded template is valid");
    let second_hash = store
        .publish_topology_spec(
            project_id,
            &second,
            &default_stamp(),
            at("2026-08-20T20:01:00Z"),
        )
        .expect("version two is published");
    let second_snapshot = TopologySnapshot {
        spec_id: second.spec_id,
        version: second.version,
        canonical_hash: second_hash,
    };
    store
        .repin_mini_project_topology(&MiniProjectTopologySnapshot {
            project_id,
            mini_project_id,
            topology: second_snapshot.clone(),
            pinned_at: at("2026-08-20T20:02:00Z"),
        })
        .expect("the compatible upgrade is atomic");

    let nodes = store
        .list_topology_nodes(project_id, Some(mini_project_id))
        .expect("the upgraded nodes read");
    assert_eq!(
        nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
        vec![epic_node_id, control_node_id],
        "the upgrade creates or replaces no node"
    );
    assert!(
        nodes.iter().all(|node| node.topology == second_snapshot),
        "every node in the epic cites the exact immutable target revision"
    );
    assert!(
        nodes
            .iter()
            .all(|node| { revisions_before.get(&node.id).copied() == Some(node.revision) }),
        "repinning changes no node revision"
    );
    assert_eq!(
        store
            .get_topology_node(project_id, root_id)
            .expect("the root reads")
            .expect("the root remains")
            .topology,
        first_snapshot,
        "the project root remains on the project default"
    );

    // Reproduce the legacy failure shape exactly: the epic pin is durable at
    // v2 while every existing epic node still cites v1. Startup convergence
    // must finish that same move without creating a successor or changing a
    // revision.
    let direct = rusqlite::Connection::open(&database).expect("a direct connection");
    direct
        .execute(
            "UPDATE topology_nodes
             SET spec_id = ?1, spec_version = ?2, spec_hash = ?3
             WHERE project_id = ?4 AND mini_project_id = ?5",
            rusqlite::params![
                first_snapshot.spec_id.to_string(),
                i64::from(first_snapshot.version.get()),
                first_snapshot.canonical_hash.as_str(),
                project_id.to_string(),
                mini_project_id.to_string(),
            ],
        )
        .expect("the legacy partial state is reproduced");
    drop(direct);

    let repaired = store
        .reconcile_mini_project_topology_nodes()
        .expect("startup convergence repairs the partial move");
    assert_eq!(repaired.len(), 1);
    assert_eq!(repaired[0].mini_project_id, mini_project_id);
    let converged = store
        .list_topology_nodes(project_id, Some(mini_project_id))
        .expect("the repaired nodes read");
    assert!(
        converged.iter().all(|node| node.topology == second_snapshot
            && revisions_before.get(&node.id).copied() == Some(node.revision)),
        "startup convergence changes only the stale topology stamps"
    );
}
