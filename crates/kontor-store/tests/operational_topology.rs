//! OP-01 generic topology, seat, adaptive-state and export round trip.

use kontor_core::id::{
    AggregateRevision, ExternalId, ExternalName, MiniProjectId, ProjectId, RoleCode, RoleSlotId,
    SeatBindingId, Timestamp, TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::repository::{
    AdaptiveAdmissionAdvance, MiniProjectTopologySnapshot, NewAdaptiveAdmissionState,
    NewMiniProject, NewProject, NewSeatBinding, NewSessionTopologyNode, ProjectRepository,
    ProjectTopologyDefault, RepositoryError, TopologyRepository,
};
use kontor_core::spec::{CatalogRoleRef, TopologySnapshot};
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
        .publish_topology_spec(project_id, &topology, created_at)
        .expect("the topology is published");
    store
        .publish_role_catalog(&catalog, created_at)
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
            created_at,
        })
        .expect("the epic node is created below the project root");
    let lsa_id = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: lsa_id,
            project_id,
            mini_project_id: Some(mini_project_id),
            topology: snapshot.clone(),
            kind: TopologyKindKey::parse("LSA").expect("the LSA kind"),
            parent_id: Some(epic_id),
            created_at,
        })
        .expect("the LSA node is created");
    let lsa = catalog
        .role(&RoleCode::parse("LSA").expect("the LSA role code"))
        .expect("the catalog has LSA");
    store
        .create_seat_binding(&NewSeatBinding {
            id: SeatBindingId::generate(),
            project_id,
            topology_node_id: lsa_id,
            role_slot_id: RoleSlotId::parse("epic.lsa").expect("a role slot"),
            role: CatalogRoleRef {
                catalog_id: catalog.catalog_id,
                catalog_revision: catalog.version,
                role_code: lsa.role_code.clone(),
                standard_title: lsa.standard_title.clone(),
                custom_display_name: Some(name("Kontor LSA")),
            },
            task_id: None,
            team_run_id: None,
            created_at,
        })
        .expect("the typed seat binding is created");

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
            .list_seat_bindings(project_id, lsa_id)
            .expect("bindings are readable")
            .len(),
        1
    );
    assert_eq!(
        reopened
            .get_adaptive_admission_state(project_id, mini_project_id)
            .expect("state is readable")
            .expect("state exists")
            .clean_observation_streak,
        1
    );
    let export = export_realm(&reopened, at("2026-08-16T02:00:00Z")).expect("the Realm exports");
    assert_eq!(export.records.topology_specs.len(), 1);
    assert_eq!(export.records.topology_nodes.len(), 3);
    assert_eq!(export.records.seat_bindings.len(), 1);
    assert_eq!(export.records.adaptive_admission_state.len(), 1);
}
