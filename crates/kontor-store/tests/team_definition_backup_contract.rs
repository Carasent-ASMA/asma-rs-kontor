//! Team Definition surfaces in the restart/export path: the typed Realm
//! export must carry every Team Definition surface, and a verified whole-file
//! snapshot restored into a fresh store must read back definitions, project
//! selection, epic pins and in-flight migration intents exactly.

use std::path::PathBuf;

use kontor_core::id::{
    ExternalId, ExternalName, IdempotencyKey, MiniProjectId, ProjectId, RuntimeKindKey,
    TeamDefinitionMigrationId, Timestamp, TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_core::repository::{
    MigrationObjectKind, MiniProjectTeamDefinitionSnapshot, MiniProjectTopologySnapshot,
    NativePlacement,
    NewMiniProject, NewProject, NewSessionTopologyNode,
    NewTeamDefinitionMigration, NewTeamDefinitionMigrationTarget,
    TeamDefinitionMigrationSubject, ProjectRepository,
    ProjectTeamDefinitionDefault, TeamDefinitionMigrationTargetState, TeamDefinitionRepository,
    TopologyRepository,
};
use kontor_core::spec::{
    Shareability, ShareabilityTier, TeamDefinitionSnapshot, TeamDefinitionSpec,
};
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use kontor_store::backup::{create_snapshot, export_realm, restore_snapshot};
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn default_stamp() -> Shareability {
    Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("tier B classifies")
}

/// One epic pinned to definition v1, with v2 selected as the project default
/// and one recorded migration intent that has not touched the runtime yet.
struct Fixture {
    home: TempDir,
    database: PathBuf,
    store: SqliteStore,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    definition: TeamDefinitionSpec,
    second: TeamDefinitionSpec,
    migration_id: TeamDefinitionMigrationId,
    native: NativeRuntimeIdentity,
    created_at: Timestamp,
}

fn fixture() -> Fixture {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the store opens");
    let project_id = ProjectId::generate();
    let mini_project_id = MiniProjectId::generate();
    let created_at = at("2026-09-01T12:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Team definition backup project"),
            root_path: name("/tmp/team-definition-backup-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_mini_project(&NewMiniProject {
            id: mini_project_id,
            project_id,
            name: name("Team definition backup epic"),
            created_at,
        })
        .expect("the epic is created");

    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let definition = domain
        .team_definitions
        .first()
        .expect("a bundled team definition")
        .clone();
    let topology_spec = domain
        .topology_specs
        .iter()
        .find(|topology| {
            topology.spec_id == definition.topology.spec_id
                && topology.version == definition.topology.version
        })
        .expect("the definition's topology validator is bundled")
        .clone();
    let canonical_hash = store
        .publish_topology_spec(project_id, &topology_spec, &default_stamp(), created_at)
        .expect("the topology is published");
    let topology = kontor_core::spec::TopologySnapshot {
        spec_id: topology_spec.spec_id,
        version: topology_spec.version,
        canonical_hash,
    };
    store
        .pin_mini_project_topology(&MiniProjectTopologySnapshot {
            project_id,
            mini_project_id,
            topology: topology.clone(),
            pinned_at: created_at,
        })
        .expect("the epic pins its topology validator");

    store
        .publish_team_definition(project_id, &definition, created_at)
        .expect("v1 publishes");
    let mut second = definition.clone();
    second.version = kontor_core::id::SpecVersion::parse(2).expect("a valid version");
    store
        .publish_team_definition(project_id, &second, created_at)
        .expect("v2 publishes");
    store
        .set_project_team_definition_default(&ProjectTeamDefinitionDefault {
            project_id,
            expected: None,
            definition: snapshot(&second),
            selected_at: created_at,
        })
        .expect("the project selects v2 for future epics");
    store
        .pin_mini_project_team_definition(&MiniProjectTeamDefinitionSnapshot {
            project_id,
            mini_project_id,
            definition: snapshot(&definition),
            pinned_at: created_at,
        })
        .expect("the epic freezes v1");

    let root = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: root,
            project_id,
            mini_project_id: None,
            topology: topology.clone(),
            kind: TopologyKindKey::parse("PSW").expect("the root kind"),
            parent_id: None,
            task_id: None,
            created_at,
        })
        .expect("the project root is created");
    let node = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: node,
            project_id,
            mini_project_id: Some(mini_project_id),
            topology: topology.clone(),
            kind: TopologyKindKey::parse("ESW").expect("the epic kind"),
            parent_id: Some(root),
            task_id: None,
            created_at,
        })
        .expect("the epic node is created");
    let native = NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo").expect("a runtime kind"),
        host: name("localhost"),
        generation: 1,
        native_id: ExternalId::parse("wks_epic_root").expect("a native id"),
    };
    let migration = store
        .record_team_definition_migration(&NewTeamDefinitionMigration {
            id: TeamDefinitionMigrationId::generate(),
            project_id,
            mini_project_id,
            idempotency_key: IdempotencyKey::parse("upgrade-team-definition-backup")
                .expect("a key"),
            from: Some(snapshot(&definition)),
            to: snapshot(&second),
            targets: vec![NewTeamDefinitionMigrationTarget {
                subject: TeamDefinitionMigrationSubject::Container {
                    topology_node_id: node,
                },
                identity: native.clone(),
                desired: NativePlacement {
                    title: name("ESW • KBI-8049"),
                    parent_native_id: Some(
                        ExternalId::parse("wks_root").expect("a native id"),
                    ),
                    kind: MigrationObjectKind::WorkspaceContainer,
                    canonical_cwd: None,
                },
            }],
            recorded_at: created_at,
        })
        .expect("the migration intent is recorded before any runtime effect");

    Fixture {
        home,
        database,
        store,
        project_id,
        mini_project_id,
        definition,
        second,
        migration_id: migration.id,
        native,
        created_at,
    }
}

fn snapshot(definition: &TeamDefinitionSpec) -> TeamDefinitionSnapshot {
    TeamDefinitionSnapshot::from_revision(definition).expect("a snapshot")
}

#[test]
fn typed_export_carries_every_team_definition_surface() {
    let f = fixture();
    let export = export_realm(&f.store, at("2026-09-01T13:00:00Z")).expect("the Realm exports");
    let document = serde_json::to_string(&export).expect("the export serializes");

    assert!(
        document.contains("\"topology_specs\""),
        "positive control: the export does carry published topology revisions"
    );
    for surface in [
        "\"team_definitions\"",
        "\"project_team_definition_defaults\"",
        "\"mini_project_team_definition_snapshots\"",
        "\"team_definition_migration_intents\"",
        "\"team_definition_migration_targets\"",
    ] {
        assert!(
            document.contains(surface),
            "the typed export must carry {surface}; a typed export that drops a \
             Team Definition surface loses the naming authority on import"
        );
    }
}

#[test]
fn a_verified_snapshot_restore_preserves_the_team_definition_surfaces() {
    let f = fixture();
    let outcome = create_snapshot(&f.database, &f.home.path().join("snapshots"), f.created_at)
        .expect("the snapshot is taken");
    let destination = f.home.path().join("restored").join("kontor.db");
    restore_snapshot(&outcome.snapshot, &destination, f.created_at)
        .expect("the snapshot restores into a fresh destination");

    let restored = SqliteStore::open(&destination).expect("the restored store opens");
    let v1 = restored
        .get_team_definition(
            f.project_id,
            f.definition.definition_id,
            f.definition.version,
        )
        .expect("definitions read back")
        .expect("the published revision survives the restore");
    assert_eq!(
        v1.canonicalize().expect("canonical bytes").hash(),
        &f.definition
            .canonicalize()
            .expect("canonical bytes")
            .hash()
            .clone(),
        "the restored revision is the exact published bytes"
    );
    assert_eq!(
        restored
            .get_project_team_definition_default(f.project_id)
            .expect("the default reads back")
            .expect("the project selection survives the restore")
            .definition,
        snapshot(&f.second),
    );
    assert_eq!(
        restored
            .get_mini_project_team_definition(f.project_id, f.mini_project_id)
            .expect("the pin reads back")
            .expect("the epic pin survives the restore")
            .definition,
        snapshot(&f.definition),
        "the epic keeps the pin its natives render"
    );

    let migration = restored
        .get_team_definition_migration(f.project_id, f.migration_id)
        .expect("migrations read back")
        .expect("the recorded intent survives the restore");
    assert_eq!(migration.state.as_str(), "recorded");
    assert_eq!(migration.targets.len(), 1);
    assert_eq!(migration.targets[0].identity, f.native);
    assert_eq!(
        migration.targets[0].desired.title.as_str(),
        "ESW • KBI-8049"
    );
    assert_eq!(
        migration.targets[0].state,
        TeamDefinitionMigrationTargetState::Pending,
        "an unapplied target must not come back as a success it never had"
    );
    assert_eq!(
        restored
            .get_in_flight_team_definition_migration(f.project_id, f.mini_project_id)
            .expect("the fence reads back")
            .map(|in_flight| in_flight.id),
        Some(f.migration_id),
        "a restored epic is still fenced by its in-flight migration"
    );
}
