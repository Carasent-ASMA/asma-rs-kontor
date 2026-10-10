//! Durable desks in the store (ASMA-8450): the desk row, the two unscoped nodes
//! that hang off an older project root, and the export generation that carries
//! them.
//!
//! The desk-capable topology and Team Definition are derived here from the
//! bundled defaults, so these tests pin the persistence rules independently of
//! the lineage version a successor is shipped at.

mod support;

use kontor_core::id::{
    ContentHash, DeskKey, ExternalName, MiniProjectId, ProjectId, Timestamp, TopologyKindKey,
    TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::repository::{
    MiniProjectTopologySnapshot, NewMiniProject, NewProject, NewSessionTopologyNode,
    ProjectRepository, RepositoryError, StoredDesk, TeamDefinitionRepository, TopologyRepository,
};
use kontor_core::spec::{
    ProjectSessionTopologySpec, Shareability, ShareabilityTier, TeamDefinitionSnapshot,
    TeamDefinitionSpec, TopologySnapshot,
};
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use kontor_store::backup::{KontorExportV1, create_snapshot, export_realm, restore_snapshot};

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

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn kind(text: &str) -> TopologyKindKey {
    TopologyKindKey::parse(text).expect("a kind")
}

fn desk_key(text: &str) -> DeskKey {
    DeskKey::parse(text).expect("a desk key")
}

fn default_stamp() -> Shareability {
    Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("tier B classifies")
}

fn snapshot_of(topology: &ProjectSessionTopologySpec) -> TopologySnapshot {
    TopologySnapshot {
        spec_id: topology.spec_id,
        version: topology.version,
        canonical_hash: topology.canonicalize().expect("canonical").hash().clone(),
    }
}

/// The bundled default topology at `version`, with the desk kinds or without.
fn topology_revision(version: u32, with_desks: bool) -> ProjectSessionTopologySpec {
    let domain = bundled_operational_domain().expect("the bundled domain");
    let definition = domain.team_definitions.first().expect("a definition");
    let base = domain
        .topology_specs
        .iter()
        .find(|topology| {
            topology.spec_id == definition.topology.spec_id
                && topology.version == definition.topology.version
        })
        .expect("the default's topology");
    let mut value = serde_json::to_value(base).expect("serializes");
    if with_desks {
        let kinds: Vec<serde_json::Value> = serde_json::from_str(DESK_KINDS).expect("kinds");
        value["node_kinds"]
            .as_array_mut()
            .expect("node kinds")
            .extend(kinds);
    }
    value["version"] = serde_json::json!(version);
    serde_json::from_value(value).expect("a topology revision")
}

struct Fixture {
    home: tempfile::TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    /// The bundled default, which the project root is placed under.
    base: TopologySnapshot,
    /// Its desk-capable successor.
    desks: TopologySnapshot,
    definition: TeamDefinitionSpec,
    root: TopologyNodeId,
    created_at: Timestamp,
}

impl Fixture {
    fn build() -> Self {
        let home = support::state_root();
        let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
        let project_id = ProjectId::generate();
        let created_at = at("2026-10-10T09:00:00Z");
        store
            .create_project(&NewProject {
                id: project_id,
                name: name("Durable desk project"),
                root_path: name("/tmp/durable-desk-project"),
                created_at,
            })
            .expect("the project is created");

        let domain = bundled_operational_domain().expect("the bundled domain");
        let default = domain
            .team_definitions
            .first()
            .expect("a definition")
            .clone();
        let base_version = default.topology.version.get();
        let base = topology_revision(base_version, false);
        let successor = topology_revision(base_version + 1, true);
        for topology in [&base, &successor] {
            store
                .publish_topology_spec(project_id, topology, &default_stamp(), created_at)
                .expect("the topology publishes");
        }
        let mut value = serde_json::to_value(&default).expect("serializes");
        let containers: Vec<serde_json::Value> =
            serde_json::from_str(DESK_CONTAINERS).expect("containers");
        value["containers"]
            .as_array_mut()
            .expect("containers")
            .extend(containers);
        value["desks"] = serde_json::from_str(DESKS).expect("desks");
        value["version"] = serde_json::json!(default.version.get() + 1);
        value["topology"] =
            serde_json::to_value(snapshot_of(&successor)).expect("a topology snapshot");
        let definition: TeamDefinitionSpec =
            serde_json::from_value(value).expect("a desk definition");
        store
            .publish_team_definition(project_id, &definition, created_at)
            .expect("the desk definition publishes");

        let base = snapshot_of(&base);
        let root = TopologyNodeId::generate();
        store
            .create_topology_node(&NewSessionTopologyNode {
                id: root,
                project_id,
                mini_project_id: None,
                topology: base.clone(),
                kind: kind("PSW"),
                parent_id: None,
                task_id: None,
                created_at,
            })
            .expect("the project root is placed on the default revision");
        Self {
            home,
            store,
            project_id,
            base,
            desks: snapshot_of(&successor),
            definition,
            root,
            created_at,
        }
    }

    fn planned(&self, key: &str) -> StoredDesk {
        StoredDesk {
            project_id: self.project_id,
            desk_key: desk_key(key),
            topology_node_id: TopologyNodeId::generate(),
            workspace_node_id: TopologyNodeId::generate(),
            team_definition: TeamDefinitionSnapshot::from_revision(&self.definition)
                .expect("a snapshot"),
            created_at: self.created_at,
        }
    }

    fn node(
        &self,
        id: TopologyNodeId,
        kind_text: &str,
        parent: TopologyNodeId,
        topology: &TopologySnapshot,
        epic: Option<MiniProjectId>,
    ) -> Result<(), RepositoryError> {
        self.store
            .create_topology_node(&NewSessionTopologyNode {
                id,
                project_id: self.project_id,
                mini_project_id: epic,
                topology: topology.clone(),
                kind: kind(kind_text),
                parent_id: Some(parent),
                task_id: None,
                created_at: self.created_at,
            })
            .map(|_| ())
    }

    /// One whole desk: the row first, then its two nodes by the planned ids.
    fn place(&self, key: &str) -> StoredDesk {
        let desk = self.planned(key);
        self.store
            .create_desk(&desk)
            .expect("the desk row is written");
        self.node(desk.topology_node_id, "DESK", self.root, &self.desks, None)
            .expect("the desk hangs off the older project root");
        self.node(
            desk.workspace_node_id,
            "DWS",
            desk.topology_node_id,
            &self.desks,
            None,
        )
        .expect("the workspace sits inside its desk");
        desk
    }
}

#[test]
fn both_desks_hang_off_the_older_root_without_an_epic_and_survive_a_reopen() {
    let f = Fixture::build();
    let adam = f.place("adam");
    let review = f.place("pr-review");

    for desk in [&adam, &review] {
        for node_id in [desk.topology_node_id, desk.workspace_node_id] {
            let node = f
                .store
                .get_topology_node(f.project_id, node_id)
                .expect("reads")
                .expect("the node exists");
            assert_eq!(node.mini_project_id, None, "a desk node belongs to no epic");
            assert_eq!(node.task_id, None);
            assert_eq!(node.topology, f.desks, "placed under the desk revision");
        }
    }
    assert_ne!(
        f.base, f.desks,
        "positive control: the root is on an older revision"
    );

    let path = f.home.path().join("kontor.db");
    drop(f.store);
    let reopened = SqliteStore::open(&path).expect("the store reopens");
    assert_eq!(
        reopened
            .get_desk(f.project_id, &desk_key("adam"))
            .expect("reads"),
        Some(adam.clone())
    );
    for node_id in [review.topology_node_id, review.workspace_node_id] {
        assert_eq!(
            reopened
                .get_desk_by_node(f.project_id, node_id)
                .expect("reads"),
            Some(review.clone()),
            "either node finds its desk"
        );
    }
    assert_eq!(
        reopened
            .get_desk_by_node(f.project_id, f.root)
            .expect("reads"),
        None,
        "the project root is no desk's"
    );
    assert_eq!(
        reopened.list_desks(f.project_id).expect("lists"),
        vec![adam, review],
        "listed by key"
    );
}

#[test]
fn a_desk_row_is_unique_immutable_and_permanent() {
    let f = Fixture::build();
    let adam = f.place("adam");

    let mut again = f.planned("adam");
    assert!(matches!(
        f.store.create_desk(&again),
        Err(RepositoryError::Conflict {
            subject: "desk",
            ..
        })
    ));
    again.desk_key = desk_key("other");
    again.topology_node_id = adam.topology_node_id;
    assert!(
        matches!(
            f.store.create_desk(&again),
            Err(RepositoryError::Conflict {
                subject: "desk",
                ..
            })
        ),
        "one node realizes at most one desk"
    );
    let mut same = f.planned("same");
    same.workspace_node_id = same.topology_node_id;
    assert!(f.store.create_desk(&same).is_err(), "a desk is two nodes");

    let connection =
        rusqlite::Connection::open(f.home.path().join("kontor.db")).expect("a raw connection");
    let updated = connection.execute(
        "UPDATE desks SET team_definition_version = team_definition_version + 1",
        [],
    );
    assert!(
        updated
            .expect_err("a desk is immutable")
            .to_string()
            .contains("a desk keeps its key, nodes and Team Definition pin")
    );
    let deleted = connection.execute("DELETE FROM desks", []);
    assert!(
        deleted
            .expect_err("a desk is permanent")
            .to_string()
            .contains("a desk is never deleted")
    );
    assert_eq!(
        f.store
            .get_desk(f.project_id, &desk_key("adam"))
            .expect("reads"),
        Some(adam)
    );
}

#[test]
fn the_older_root_admits_only_an_unscoped_kind_it_never_declared_from_a_newer_revision() {
    let f = Fixture::build();

    // A kind the root's own revision declares keeps the epic rule: placed
    // unscoped from the newer revision, it is refused.
    assert!(
        matches!(
            f.node(TopologyNodeId::generate(), "ESW", f.root, &f.desks, None),
            Err(RepositoryError::Conflict { .. })
        ),
        "an epic workspace cannot ride the desk exception"
    );

    // A desk kind scoped to an epic is refused, even with the epic pinned to
    // the very revision that declares it.
    let epic = MiniProjectId::generate();
    f.store
        .create_mini_project(&NewMiniProject {
            id: epic,
            project_id: f.project_id,
            name: name("An epic"),
            created_at: f.created_at,
        })
        .expect("the epic is created");
    f.store
        .pin_mini_project_topology(&MiniProjectTopologySnapshot {
            project_id: f.project_id,
            mini_project_id: epic,
            topology: f.desks.clone(),
            pinned_at: f.created_at,
        })
        .expect("the epic pins the desk revision");
    assert!(
        matches!(
            f.node(
                TopologyNodeId::generate(),
                "DESK",
                f.root,
                &f.desks,
                Some(epic)
            ),
            Err(RepositoryError::Conflict { .. })
        ),
        "a desk never belongs to an epic"
    );

    // A desk kind from a revision *older* than the root is refused too: the
    // exception only ever looks forward along the lineage.
    let newer = topology_revision(f.desks.version.get() + 1, false);
    f.store
        .publish_topology_spec(f.project_id, &newer, &default_stamp(), f.created_at)
        .expect("a newer revision without desks publishes");
    let other = ProjectId::generate();
    f.store
        .create_project(&NewProject {
            id: other,
            name: name("Newer root project"),
            root_path: name("/tmp/newer-root-project"),
            created_at: f.created_at,
        })
        .expect("a second project");
    let desk_revision = topology_revision(f.desks.version.get(), true);
    for topology in [&desk_revision, &newer] {
        f.store
            .publish_topology_spec(other, topology, &default_stamp(), f.created_at)
            .expect("publishes");
    }
    let newer_root = TopologyNodeId::generate();
    f.store
        .create_topology_node(&NewSessionTopologyNode {
            id: newer_root,
            project_id: other,
            mini_project_id: None,
            topology: snapshot_of(&newer),
            kind: kind("PSW"),
            parent_id: None,
            task_id: None,
            created_at: f.created_at,
        })
        .expect("a root on the newer revision");
    assert!(
        matches!(
            f.store.create_topology_node(&NewSessionTopologyNode {
                id: TopologyNodeId::generate(),
                project_id: other,
                mini_project_id: None,
                topology: snapshot_of(&desk_revision),
                kind: kind("DESK"),
                parent_id: Some(newer_root),
                task_id: None,
                created_at: f.created_at,
            }),
            Err(RepositoryError::Conflict { .. })
        ),
        "a desk from an older revision than its root is refused"
    );

    // Positive control: the same desk placed forward along the lineage.
    f.place("adam");
}

#[test]
fn a_verified_snapshot_restore_keeps_every_desk() {
    let f = Fixture::build();
    let adam = f.place("adam");
    let outcome = create_snapshot(
        &f.home.path().join("kontor.db"),
        &f.home.path().join("snapshots"),
        f.created_at,
    )
    .expect("the snapshot is taken");
    let destination = f.home.path().join("restored").join("kontor.db");
    restore_snapshot(&outcome.snapshot, &destination, f.created_at).expect("restores");
    let restored = SqliteStore::open(&destination).expect("the restored store opens");
    assert_eq!(
        restored.list_desks(f.project_id).expect("lists"),
        vec![adam]
    );
}

#[test]
fn the_typed_export_carries_desks_and_no_older_generation_can_carry_or_conceal_them() {
    let f = Fixture::build();
    let adam = f.place("adam");
    let export = export_realm(&f.store, at("2026-10-10T10:00:00Z")).expect("the Realm exports");
    assert_eq!(export.records.desks.len(), 1);
    assert_eq!(export.records.desks[0].desk_key, adam.desk_key.as_str());
    assert_eq!(
        export.records.desks[0].workspace_node_id,
        adam.workspace_node_id.to_string()
    );
    let base = serde_json::to_value(&export).expect("serializes");
    KontorExportV1::parse(&serde_json::to_vec(&base).expect("bytes"))
        .expect("positive control: the current generation verifies");

    let rehash = |document: &mut serde_json::Value| {
        let mut records =
            serde_json::to_vec(document.get("records").expect("records")).expect("serializes");
        records.push(b'\n');
        document["records_hash"] = serde_json::json!(ContentHash::of(&records).to_string());
    };
    let refusal = |document: serde_json::Value| -> String {
        match KontorExportV1::parse(&serde_json::to_vec(&document).expect("bytes")) {
            Err(kontor_store::backup::BackupError::Verification { detail }) => detail.to_owned(),
            other => panic!("expected a verification refusal, got {other:?}"),
        }
    };

    let mut carrying = base.clone();
    carrying["schema_version"] = serde_json::json!(13);
    carrying["database_schema_version"] = serde_json::json!(130);
    rehash(&mut carrying);
    assert_eq!(
        refusal(carrying),
        "the legacy export generation carries desks it did not define"
    );

    let mut concealing = base.clone();
    concealing["schema_version"] = serde_json::json!(13);
    concealing["records"]["desks"] = serde_json::json!([]);
    concealing["continuity_summary"]["record_counts"]["desks"] = serde_json::json!(0);
    rehash(&mut concealing);
    assert_eq!(
        refusal(concealing),
        "the legacy export generation cannot prove desk completeness"
    );

    let mut understated = base;
    understated["continuity_summary"]["record_counts"]["desks"] = serde_json::json!(0);
    rehash(&mut understated);
    assert!(
        KontorExportV1::parse(&serde_json::to_vec(&understated).expect("bytes")).is_err(),
        "the continuity summary must disclose every desk"
    );
}
