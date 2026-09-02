//! Two release blockers on the epic upgrade path.
//!
//! P1-1: a migration must cover every live native-bearing subject of the epic,
//! and the target definition must declare every live node kind. Skipping a kind
//! silently would leave part of the epic rendering the old pin's names while
//! the epic claims the new one.
//!
//! P1-2: confirming commits the pin and the terminal state, and the command
//! receipt is written after it. A crash in that window must be recoverable from
//! the migration's own idempotency key, and the recovery must produce the
//! receipt without repeating a single native effect.

use kontor_core::id::{
    CanonicalDocument, CommandReceiptId, ExternalId, ExternalName, IdempotencyKey, MiniProjectId,
    ProjectId, RuntimeKindKey, SpecVersion, TeamDefinitionMigrationId, Timestamp, TopologyKindKey,
    TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CommandRepository, MigrationObjectKind, MiniProjectTeamDefinitionSnapshot,
    MiniProjectTopologySnapshot, NativePlacement, NewCommandIntent, NewMiniProject,
    NewNativeContainerBinding, NewProject, NewSessionTopologyNode, NewTeamDefinitionMigration,
    NewTeamDefinitionMigrationTarget, ProjectRepository, TeamDefinitionMigrationObservation,
    TeamDefinitionMigrationState, TeamDefinitionMigrationSubject,
    TeamDefinitionMigrationTargetState, TeamDefinitionRepository, TopologyRepository,
};
use kontor_core::spec::{
    Shareability, ShareabilityTier, TeamDefinitionSnapshot, TeamDefinitionSpec, TopologySnapshot,
};
use kontor_core::state::{NativeRuntimeIdentity, ObservedContainerKind};
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn stamp() -> Shareability {
    Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("tier B classifies")
}

fn identity(native: &str) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo").expect("a runtime kind"),
        host: name("localhost"),
        generation: 1,
        native_id: ExternalId::parse(native).expect("a native id"),
    }
}

fn snapshot(definition: &TeamDefinitionSpec) -> TeamDefinitionSnapshot {
    TeamDefinitionSnapshot::from_revision(definition).expect("a snapshot")
}

struct World {
    home: TempDir,
    database: std::path::PathBuf,
    store: SqliteStore,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    definition: TeamDefinitionSpec,
    second: TeamDefinitionSpec,
    esw: TopologyNodeId,
    ecp: TopologyNodeId,
}

/// An epic pinned to v1, with two *live* native containers: an ESW and an ECP.
fn world() -> World {
    let home = TempDir::new().expect("a temporary directory");
    let database = home.path().join("kontor.db");
    let store = SqliteStore::open(&database).expect("the store opens");
    let project_id = ProjectId::generate();
    let mini_project_id = MiniProjectId::generate();
    let created_at = at("2026-09-02T09:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Completeness project"),
            root_path: name("/tmp/completeness-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_mini_project(&NewMiniProject {
            id: mini_project_id,
            project_id,
            name: name("Completeness epic"),
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
        .expect("the definition's validator is bundled")
        .clone();
    let canonical_hash = store
        .publish_topology_spec(project_id, &topology_spec, &stamp(), created_at)
        .expect("the topology publishes");
    let topology = TopologySnapshot {
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
        .expect("the epic pins its topology");

    let node = |id, kind: &str, parent, epic| NewSessionTopologyNode {
        id,
        project_id,
        mini_project_id: epic,
        topology: topology.clone(),
        kind: TopologyKindKey::parse(kind).expect("a kind"),
        parent_id: parent,
        task_id: None,
        created_at,
    };
    let root = TopologyNodeId::generate();
    store
        .create_topology_node(&node(root, "PSW", None, None))
        .expect("the project root is created");
    let esw = TopologyNodeId::generate();
    store
        .create_topology_node(&node(esw, "ESW", Some(root), Some(mini_project_id)))
        .expect("the epic node is created");
    let ecp = TopologyNodeId::generate();
    store
        .create_topology_node(&node(ecp, "ECP", Some(esw), Some(mini_project_id)))
        .expect("the ECP node is created");

    // Both nodes actually hold native containers. This is what makes them
    // subjects a migration is obliged to cover.
    for (node_id, native, kind) in [
        (esw, "wks_esw", ObservedContainerKind::Workspace),
        (ecp, "wks_ecp", ObservedContainerKind::Workspace),
    ] {
        store
            .bind_topology_node_container(&NewNativeContainerBinding {
                topology_node_id: node_id,
                project_id,
                container_binding_id: ExternalId::parse(&format!("bind_{native}"))
                    .expect("a binding id"),
                identity: identity(native),
                observed_kind: kind,
                canonical_cwd: Some(name("/tmp/kontor")),
                observed_at: created_at,
            })
            .expect("the native container is bound");
    }

    store
        .publish_team_definition(project_id, &definition, created_at)
        .expect("v1 publishes");
    let mut second = definition.clone();
    second.version = SpecVersion::parse(2).expect("a valid version");
    store
        .publish_team_definition(project_id, &second, created_at)
        .expect("v2 publishes");
    store
        .pin_mini_project_team_definition(&MiniProjectTeamDefinitionSnapshot {
            project_id,
            mini_project_id,
            definition: snapshot(&definition),
            pinned_at: created_at,
        })
        .expect("the epic freezes v1");

    World {
        home,
        database,
        store,
        project_id,
        mini_project_id,
        definition,
        second,
        esw,
        ecp,
    }
}

fn container_target(
    node: TopologyNodeId,
    native: &str,
    title: &str,
) -> NewTeamDefinitionMigrationTarget {
    NewTeamDefinitionMigrationTarget {
        subject: TeamDefinitionMigrationSubject::Container {
            topology_node_id: node,
        },
        identity: identity(native),
        desired: NativePlacement {
            title: name(title),
            parent_native_id: Some(ExternalId::parse("wks_root").expect("a native id")),
            kind: MigrationObjectKind::WorkspaceContainer,
            canonical_cwd: Some(name("/tmp/kontor")),
        },
    }
}

fn complete_targets(w: &World) -> Vec<NewTeamDefinitionMigrationTarget> {
    vec![
        container_target(w.esw, "wks_esw", "ESW • KBI-8049"),
        container_target(w.ecp, "wks_ecp", "ECP • KBI-8049"),
    ]
}

fn migration(
    w: &World,
    key: &str,
    targets: Vec<NewTeamDefinitionMigrationTarget>,
) -> NewTeamDefinitionMigration {
    NewTeamDefinitionMigration {
        id: TeamDefinitionMigrationId::generate(),
        project_id: w.project_id,
        mini_project_id: w.mini_project_id,
        idempotency_key: IdempotencyKey::parse(key).expect("a key"),
        from: Some(snapshot(&w.definition)),
        to: snapshot(&w.second),
        targets,
        recorded_at: at("2026-09-02T10:00:00Z"),
    }
}

// ---------------------------------------------------------------------------
// P1-1 — the census must be complete
// ---------------------------------------------------------------------------

#[test]
fn the_census_lists_every_live_native_bearing_subject_of_the_epic() {
    let w = world();
    let census = w
        .store
        .list_live_native_subjects(w.project_id, w.mini_project_id)
        .expect("the census reads");
    let mut kinds: Vec<String> = census
        .iter()
        .map(|subject| subject.node_kind.as_str().to_owned())
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        vec!["ECP".to_owned(), "ESW".to_owned()],
        "both bound containers are live subjects the migration must cover"
    );
}

#[test]
fn a_migration_that_omits_a_live_native_subject_is_refused_before_any_mutation() {
    let w = world();
    // Only the ESW is enumerated; the live ECP container is silently skipped.
    let refused = w.store.record_team_definition_migration(&migration(
        &w,
        "omits-the-ecp",
        vec![container_target(w.esw, "wks_esw", "ESW • KBI-8049")],
    ));
    assert!(
        refused.is_err(),
        "a migration that leaves a live native subject unenumerated must be \
         refused before the first retitle"
    );
    // And nothing was recorded: the refusal is pre-mutation, not a rollback of
    // something the epic can already see.
    assert!(
        w.store
            .get_in_flight_team_definition_migration(w.project_id, w.mini_project_id)
            .expect("the read succeeds")
            .is_none(),
        "a refused migration leaves no in-flight intent behind"
    );
    assert_eq!(
        w.store
            .get_mini_project_team_definition(w.project_id, w.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        w.definition.version,
        "the epic still holds the pin its natives render"
    );

    // The complete enumeration is accepted, so the refusal is about the
    // omission rather than about the fixture.
    w.store
        .record_team_definition_migration(&migration(&w, "covers-everything", complete_targets(&w)))
        .expect("a complete migration is recorded");
}

#[test]
fn a_target_definition_that_does_not_declare_a_live_node_kind_is_refused() {
    let w = world();
    // Strip ECP from the target definition while an ECP container is live.
    let mut narrowed = w.second.clone();
    narrowed
        .containers
        .retain(|container| container.kind.as_str() != "ECP");
    narrowed.version = SpecVersion::parse(3).expect("a valid version");
    w.store
        .publish_team_definition(w.project_id, &narrowed, at("2026-09-02T09:30:00Z"))
        .expect("the narrowed definition is itself a legal publication");

    let mut request = migration(&w, "narrowed-target", complete_targets(&w));
    request.to = snapshot(&narrowed);
    assert!(
        w.store.record_team_definition_migration(&request).is_err(),
        "a definition that cannot name a live node kind must not become the \
         epic's pin"
    );
}

#[test]
fn confirmation_re_proves_parity_against_the_live_census() {
    let w = world();
    let recorded = w
        .store
        .record_team_definition_migration(&migration(&w, "parity-at-confirm", complete_targets(&w)))
        .expect("a complete migration is recorded");

    // A native container appears after the preview was taken.
    let tsw = TopologyNodeId::generate();
    w.store
        .create_topology_node(&NewSessionTopologyNode {
            id: tsw,
            project_id: w.project_id,
            mini_project_id: Some(w.mini_project_id),
            topology: w
                .store
                .get_mini_project_topology(w.project_id, w.mini_project_id)
                .expect("the read succeeds")
                .expect("the epic pins a topology")
                .topology,
            kind: TopologyKindKey::parse("TSW").expect("a kind"),
            parent_id: Some(w.esw),
            task_id: None,
            created_at: at("2026-09-02T10:30:00Z"),
        })
        .expect("a task workspace node appears");
    w.store
        .bind_topology_node_container(&NewNativeContainerBinding {
            topology_node_id: tsw,
            project_id: w.project_id,
            container_binding_id: ExternalId::parse("bind_wks_tsw").expect("a binding id"),
            identity: identity("wks_tsw"),
            observed_kind: ObservedContainerKind::Workspace,
            canonical_cwd: Some(name("/tmp/kontor")),
            observed_at: at("2026-09-02T10:31:00Z"),
        })
        .expect("it takes a native container");

    for (index, (node, native, title)) in [
        (w.esw, "wks_esw", "ESW • KBI-8049"),
        (w.ecp, "wks_ecp", "ECP • KBI-8049"),
    ]
    .into_iter()
    .enumerate()
    {
        let _ = index;
        w.store
            .observe_team_definition_migration(
                w.project_id,
                recorded.id,
                &[TeamDefinitionMigrationObservation {
                    subject: TeamDefinitionMigrationSubject::Container {
                        topology_node_id: node,
                    },
                    identity: identity(native),
                    observed: Some(NativePlacement {
                        title: name(title),
                        parent_native_id: Some(ExternalId::parse("wks_root").expect("a native id")),
                        kind: MigrationObjectKind::WorkspaceContainer,
                        canonical_cwd: Some(name("/tmp/kontor")),
                    }),
                    state: TeamDefinitionMigrationTargetState::Renamed,
                    observed_at: at("2026-09-02T11:00:00Z"),
                }],
                at("2026-09-02T11:00:00Z"),
            )
            .expect("each enumerated target reads back");
    }

    assert!(
        w.store
            .confirm_team_definition_migration(
                w.project_id,
                recorded.id,
                at("2026-09-02T11:05:00Z")
            )
            .is_err(),
        "confirmation must re-prove parity: a native that appeared after the \
         preview is not covered by this migration"
    );
    assert_eq!(
        w.store
            .get_mini_project_team_definition(w.project_id, w.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        w.definition.version,
        "the pin does not move on an incomplete migration"
    );
}

// ---------------------------------------------------------------------------
// P1-2 — the confirm/receipt crash window
// ---------------------------------------------------------------------------

#[test]
fn a_crash_between_the_pin_commit_and_the_receipt_recovers_from_the_same_key() {
    let w = world();
    let key = "recoverable-upgrade";
    let recorded = w
        .store
        .record_team_definition_migration(&migration(&w, key, complete_targets(&w)))
        .expect("the migration is recorded");
    for (node, native, title) in [
        (w.esw, "wks_esw", "ESW • KBI-8049"),
        (w.ecp, "wks_ecp", "ECP • KBI-8049"),
    ] {
        w.store
            .observe_team_definition_migration(
                w.project_id,
                recorded.id,
                &[TeamDefinitionMigrationObservation {
                    subject: TeamDefinitionMigrationSubject::Container {
                        topology_node_id: node,
                    },
                    identity: identity(native),
                    observed: Some(NativePlacement {
                        title: name(title),
                        parent_native_id: Some(ExternalId::parse("wks_root").expect("a native id")),
                        kind: MigrationObjectKind::WorkspaceContainer,
                        canonical_cwd: Some(name("/tmp/kontor")),
                    }),
                    state: TeamDefinitionMigrationTargetState::Renamed,
                    observed_at: at("2026-09-02T11:00:00Z"),
                }],
                at("2026-09-02T11:00:00Z"),
            )
            .expect("every target reads back");
    }
    let confirmed = w
        .store
        .confirm_team_definition_migration(w.project_id, recorded.id, at("2026-09-02T11:05:00Z"))
        .expect("the pin moves");
    assert_eq!(confirmed.state, TeamDefinitionMigrationState::Confirmed);
    assert!(
        confirmed.receipt_id.is_none(),
        "the receipt has not been written yet; this is the crash window"
    );

    // Crash here: the pin and the terminal state are committed, the receipt is
    // not. Reopen the store as a restarted process would.
    drop(w.store);
    let restarted = SqliteStore::open(&w.database).expect("the store reopens");

    // Recovery is keyed by the migration's own idempotency key, which is the
    // only thing a retrying caller still holds.
    let recovered = restarted
        .get_team_definition_migration_by_key(
            w.project_id,
            &IdempotencyKey::parse(key).expect("a key"),
        )
        .expect("the lookup succeeds")
        .expect("the migration is findable by its key after a restart");
    assert_eq!(
        recovered.id, recorded.id,
        "the same migration, not a new one"
    );
    assert_eq!(recovered.state, TeamDefinitionMigrationState::Confirmed);
    assert!(
        recovered.receipt_id.is_none(),
        "recovery can see that the receipt is exactly what is missing"
    );
    assert_eq!(
        restarted
            .get_mini_project_team_definition(w.project_id, w.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        w.second.version,
        "the pin committed before the crash and stays committed"
    );

    // Completing the receipt repeats no native effect: every target keeps the
    // exact observation it already had.
    let before: Vec<_> = recovered
        .targets
        .iter()
        .map(|target| (target.subject, target.identity.clone(), target.state))
        .collect();
    // Recovery writes the command receipt first, then binds it: the binding is
    // evidence that this exact recorded command is the one that moved the pin.
    let receipt = CommandReceiptId::generate();
    let mini_project_revision = restarted
        .get_mini_project(w.project_id, w.mini_project_id)
        .expect("the read succeeds")
        .expect("the epic exists")
        .revision;
    restarted
        .record_intent(&NewCommandIntent {
            project_id: w.project_id,
            receipt_id: receipt,
            idempotency_key: IdempotencyKey::parse("recoverable-upgrade-receipt").expect("a key"),
            kind: CommandKind::UpgradeTeamDefinition,
            target: AggregateRef::MiniProject {
                mini_project_id: w.mini_project_id,
            },
            target_revision: mini_project_revision,
            intent: CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
                "migration": "recoverable-upgrade",
            }))
            .expect("a canonical intent"),
            payload: CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
            }))
            .expect("a canonical payload"),
            desired: None,
            not_before: at("2026-09-02T11:09:00Z"),
            created_at: at("2026-09-02T11:09:00Z"),
        })
        .expect("recovery records the command it is completing");
    restarted
        .bind_team_definition_migration_receipt(
            w.project_id,
            recovered.id,
            receipt,
            at("2026-09-02T11:10:00Z"),
        )
        .expect("the receipt is produced by recovery");
    let settled = restarted
        .get_team_definition_migration_by_key(
            w.project_id,
            &IdempotencyKey::parse(key).expect("a key"),
        )
        .expect("the lookup succeeds")
        .expect("the migration is still findable");
    assert_eq!(settled.receipt_id, Some(receipt));
    let after: Vec<_> = settled
        .targets
        .iter()
        .map(|target| (target.subject, target.identity.clone(), target.state))
        .collect();
    assert_eq!(
        before, after,
        "recovery produced the receipt without touching a single native target"
    );

    // Binding the same receipt again is the replay of the same recovery.
    restarted
        .bind_team_definition_migration_receipt(
            w.project_id,
            recovered.id,
            receipt,
            at("2026-09-02T11:11:00Z"),
        )
        .expect("the same receipt binds idempotently");
    // A different receipt would claim this migration was commanded twice.
    let other_receipt = CommandReceiptId::generate();
    restarted
        .record_intent(&NewCommandIntent {
            project_id: w.project_id,
            receipt_id: other_receipt,
            idempotency_key: IdempotencyKey::parse("a-second-command").expect("a key"),
            kind: CommandKind::UpgradeTeamDefinition,
            target: AggregateRef::MiniProject {
                mini_project_id: w.mini_project_id,
            },
            target_revision: mini_project_revision,
            intent: CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
                "migration": "a-second-command",
            }))
            .expect("a canonical intent"),
            payload: CanonicalDocument::from_serializable(&serde_json::json!({
                "schema_version": 1,
            }))
            .expect("a canonical payload"),
            desired: None,
            not_before: at("2026-09-02T11:12:00Z"),
            created_at: at("2026-09-02T11:12:00Z"),
        })
        .expect("a second command is recordable in its own right");
    assert!(
        restarted
            .bind_team_definition_migration_receipt(
                w.project_id,
                recovered.id,
                other_receipt,
                at("2026-09-02T11:12:00Z"),
            )
            .is_err(),
        "one migration is commanded once"
    );
    drop(w.home);
}
