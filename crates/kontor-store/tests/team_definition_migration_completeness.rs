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
    AgentRunId, CanonicalDocument, CommandReceiptId, ContentHash, ExternalId, ExternalName,
    IdempotencyKey, MiniProjectId, ProjectId, RoleCode, RoleKey, RoleSlotId, RuntimeBindingId,
    RuntimeKindKey, SeatBindingId, SpecVersion, TaskId, TeamDefinitionMigrationId, TeamRunId,
    Timestamp, TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::receipt::{AggregateRef, CommandKind};
use kontor_core::repository::{
    CommandRepository, MigrationObjectKind, MiniProjectTeamDefinitionSnapshot,
    MiniProjectTopologySnapshot, NativePlacement, NewAgentRun, NewCommandIntent, NewMiniProject,
    NewNativeContainerBinding, NewProject, NewSeatBinding, NewSessionTopologyNode, NewTask,
    NewTeamDefinitionMigration, NewTeamDefinitionMigrationTarget, NewTeamRun, ProjectRepository,
    RunRepository, RuntimeBinding, SpecRepository, TeamDefinitionMigrationObservation,
    TeamDefinitionMigrationState, TeamDefinitionMigrationSubject,
    TeamDefinitionMigrationTargetState, TeamDefinitionRepository, TopologyRepository,
};
use kontor_core::spec::{
    CatalogRoleRef, Shareability, ShareabilityTier, TeamDefinitionSnapshot, TeamDefinitionSpec,
    TeamRunSnapshot, TeamTemplateRevision, TopologySnapshot,
};
use kontor_core::state::{NativeRuntimeIdentity, ObservedContainerKind, TaskState};
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
    /// A TSW node hosting one delivery seat whose native session is live.
    tsw: TopologyNodeId,
    /// That delivery seat's binding.
    delivery_seat: SeatBindingId,
    /// The task and team run the delivery seat serves.
    task: TaskId,
    team_run: TeamRunId,
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

    // A delivery seat: a TSW node, an active seat binding on it, and an agent
    // run whose runtime binding holds the live native session. Its identity
    // lives in `AgentRun.binding`, not in the container or consultation tables,
    // which is exactly why a census that reads only those misses it.
    let tsw = TopologyNodeId::generate();
    store
        .create_topology_node(&node(tsw, "TSW", Some(esw), Some(mini_project_id)))
        .expect("the task workspace node is created");
    store
        .bind_topology_node_container(&NewNativeContainerBinding {
            topology_node_id: tsw,
            project_id,
            container_binding_id: ExternalId::parse("bind_wks_tsw").expect("a binding id"),
            identity: identity("wks_tsw"),
            observed_kind: ObservedContainerKind::Workspace,
            canonical_cwd: Some(name("/tmp/kontor")),
            observed_at: created_at,
        })
        .expect("the task workspace takes a native container");

    let catalog = domain.role_catalogs.first().expect("a catalog").clone();
    store
        .publish_role_catalog(&catalog, &stamp(), created_at)
        .expect("the catalog publishes");
    let entry = catalog
        .role(&RoleCode::parse("AUD").expect("a role code"))
        .expect("the catalog has the delivery role");
    let delivery_seat = SeatBindingId::generate();
    let task = TaskId::generate();
    store
        .create_task(&NewTask {
            id: task,
            project_id,
            mini_project_id: Some(mini_project_id),
            title: name("Delivery task"),
            module: None,
            state: TaskState::Ready,
            created_at,
        })
        .expect("the task is created");
    let template = TeamTemplateRevision {
        template_id: kontor_core::id::TeamTemplateId::generate(),
        version: SpecVersion::FIRST,
        name: name("Delivery team"),
        definition: CanonicalDocument::from_serializable(&serde_json::json!({
            "schema_version": 1,
            "team": "delivery",
        }))
        .expect("a canonical team document"),
        role_authority: Vec::new(),
    };
    store
        .insert_team_template(project_id, &template)
        .expect("the template is stored");
    let team_run = TeamRunId::generate();
    store
        .create_team_run(&NewTeamRun {
            id: team_run,
            project_id,
            task_id: task,
            snapshot: TeamRunSnapshot::from_revision(&template, kontor_core::id::SCHEMA_VERSION),
            created_at,
        })
        .expect("the team run is created");
    let slot = RoleSlotId::parse("delivery.auditor").expect("a slot");
    store
        .create_seat_binding(&NewSeatBinding {
            id: delivery_seat,
            project_id,
            topology_node_id: tsw,
            role_slot_id: slot.clone(),
            role: CatalogRoleRef {
                catalog_id: catalog.catalog_id,
                catalog_revision: catalog.version,
                role_code: entry.role_code.clone(),
                standard_title: entry.standard_title.clone(),
                custom_display_name: None,
            },
            task_id: Some(task),
            team_run_id: Some(team_run),
            attach_deadline: at("2026-09-02T09:30:00Z"),
            parent_seat_binding_id: None,
            created_at,
        })
        .expect("the delivery seat is bound");
    let agent_run = AgentRunId::generate();
    store
        .create_agent_run(&NewAgentRun {
            id: agent_run,
            project_id,
            team_run_id: team_run,
            parent_agent_run_id: None,
            role: RoleKey::parse("delivery.auditor").expect("a logical role"),
            account_profile_id: None,
            binding: Some(RuntimeBinding {
                id: RuntimeBindingId::generate(),
                agent_run_id: agent_run,
                identity: identity("agent_delivery_aud"),
                bound_at: created_at,
            }),
            created_at,
        })
        .expect("the delivery seat's native session is bound");

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
        tsw,
        delivery_seat,
        task,
        team_run,
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
        container_target(w.tsw, "wks_tsw", "TSW • KBI-8062"),
        delivery_seat_target(w),
    ]
}

/// The delivery seat target: a seat, named by its role code alone.
fn delivery_seat_target(w: &World) -> NewTeamDefinitionMigrationTarget {
    NewTeamDefinitionMigrationTarget {
        subject: TeamDefinitionMigrationSubject::Seat {
            topology_node_id: w.tsw,
            seat_binding_id: w.delivery_seat,
        },
        identity: identity("agent_delivery_aud"),
        desired: NativePlacement {
            title: name("AUD"),
            parent_native_id: Some(ExternalId::parse("wks_tsw").expect("a native id")),
            kind: MigrationObjectKind::Seat,
            canonical_cwd: None,
        },
    }
}

/// Read back every enumerated target of one migration exactly as previewed.
fn read_every_target_back(w: &World, id: TeamDefinitionMigrationId) {
    for target in complete_targets(w) {
        w.store
            .observe_team_definition_migration(
                w.project_id,
                id,
                &[TeamDefinitionMigrationObservation {
                    subject: target.subject,
                    identity: target.identity.clone(),
                    observed: Some(target.desired.clone()),
                    state: TeamDefinitionMigrationTargetState::Renamed,
                    observed_at: at("2026-09-02T11:00:00Z"),
                }],
                at("2026-09-02T11:00:00Z"),
            )
            .expect("each enumerated target reads back");
    }
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
        command_intent_hash: ContentHash::of(b"command-intent"),
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
    let mut seen: Vec<String> = census
        .iter()
        .map(|live| {
            let seat = match live.subject {
                TeamDefinitionMigrationSubject::Seat { .. } => "seat",
                TeamDefinitionMigrationSubject::Container { .. } => "container",
            };
            format!(
                "{}/{seat}/{}",
                live.node_kind.as_str(),
                live.identity.native_id.as_str()
            )
        })
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        vec![
            "ECP/container/wks_ecp".to_owned(),
            "ESW/container/wks_esw".to_owned(),
            "TSW/container/wks_tsw".to_owned(),
            // The delivery seat's identity lives in `AgentRun.binding`; a census
            // that reads only containers and consultation seats loses it.
            "TSW/seat/agent_delivery_aud".to_owned(),
        ],
        "every native-bearing family is a live subject the migration must cover"
    );
}

#[test]
fn a_migration_that_omits_a_live_delivery_seat_is_refused() {
    let w = world();
    let without_the_seat: Vec<_> = complete_targets(&w)
        .into_iter()
        .filter(|target| !matches!(target.subject, TeamDefinitionMigrationSubject::Seat { .. }))
        .collect();
    assert!(
        w.store
            .record_team_definition_migration(&migration(
                &w,
                "omits-delivery-seat",
                without_the_seat
            ))
            .is_err(),
        "a delivery seat bound through its agent run is a live native subject"
    );
    assert!(
        w.store
            .get_in_flight_team_definition_migration(w.project_id, w.mini_project_id)
            .expect("the read succeeds")
            .is_none(),
        "the refusal is pre-mutation"
    );
    w.store
        .record_team_definition_migration(&migration(&w, "covers-the-seat", complete_targets(&w)))
        .expect("the complete enumeration, including the delivery seat, is recorded");
}

#[test]
fn a_migration_must_match_the_live_census_in_both_subject_and_identity_directions() {
    let w = world();

    let mut wrong_identity = complete_targets(&w);
    wrong_identity
        .iter_mut()
        .find(|target| {
            target.subject
                == (TeamDefinitionMigrationSubject::Seat {
                    topology_node_id: w.tsw,
                    seat_binding_id: w.delivery_seat,
                })
        })
        .expect("the delivery seat is enumerated")
        .identity = identity("a_stale_predecessor_identity");
    assert!(
        w.store
            .record_team_definition_migration(&migration(
                &w,
                "wrong-live-native-identity",
                wrong_identity,
            ))
            .is_err(),
        "the right SeatBinding with the wrong native identity is not the live census"
    );

    let catalog = bundled_operational_domain()
        .expect("the bundled domain validates")
        .role_catalogs
        .first()
        .expect("a catalog")
        .clone();
    let role = catalog
        .role(&RoleCode::parse("SA").expect("a role code"))
        .expect("the catalog has SA");
    let stale_seat = SeatBindingId::generate();
    w.store
        .create_seat_binding(&NewSeatBinding {
            id: stale_seat,
            project_id: w.project_id,
            topology_node_id: w.tsw,
            role_slot_id: RoleSlotId::parse("delivery.stale").expect("a slot"),
            role: CatalogRoleRef {
                catalog_id: catalog.catalog_id,
                catalog_revision: catalog.version,
                role_code: role.role_code.clone(),
                standard_title: role.standard_title.clone(),
                custom_display_name: None,
            },
            task_id: Some(w.task),
            team_run_id: Some(w.team_run),
            attach_deadline: at("2026-09-02T10:45:00Z"),
            parent_seat_binding_id: None,
            created_at: at("2026-09-02T10:44:00Z"),
        })
        .expect("the logical seat exists without a native session");
    let mut extra_subject = complete_targets(&w);
    extra_subject.push(NewTeamDefinitionMigrationTarget {
        subject: TeamDefinitionMigrationSubject::Seat {
            topology_node_id: w.tsw,
            seat_binding_id: stale_seat,
        },
        identity: identity("a_native_that_is_not_live"),
        desired: NativePlacement {
            title: name("SA"),
            parent_native_id: Some(ExternalId::parse("wks_tsw").expect("a native id")),
            kind: MigrationObjectKind::Seat,
            canonical_cwd: None,
        },
    });
    assert!(
        w.store
            .record_team_definition_migration(&migration(
                &w,
                "extra-stale-native-subject",
                extra_subject,
            ))
            .is_err(),
        "an enumerated logical seat with no live native may not survive as a stale target"
    );
    assert!(
        w.store
            .get_in_flight_team_definition_migration(w.project_id, w.mini_project_id)
            .expect("the fence reads")
            .is_none(),
        "both census mismatches are refused before a migration is recorded"
    );
}

/// Bind one more delivery seat on the shared team run, with its own slot,
/// agent run and native session.
fn add_delivery_seat(w: &World, slot: &str, role: &str, native: &str) -> SeatBindingId {
    let catalog = bundled_operational_domain()
        .expect("the bundled domain validates")
        .role_catalogs
        .first()
        .expect("a catalog")
        .clone();
    let entry = catalog
        .role(&RoleCode::parse(role).expect("a role code"))
        .expect("the catalog has the role");
    let seat = SeatBindingId::generate();
    w.store
        .create_seat_binding(&NewSeatBinding {
            id: seat,
            project_id: w.project_id,
            topology_node_id: w.tsw,
            role_slot_id: RoleSlotId::parse(slot).expect("a slot"),
            role: CatalogRoleRef {
                catalog_id: catalog.catalog_id,
                catalog_revision: catalog.version,
                role_code: entry.role_code.clone(),
                standard_title: entry.standard_title.clone(),
                custom_display_name: None,
            },
            task_id: Some(w.task),
            team_run_id: Some(w.team_run),
            attach_deadline: at("2026-09-02T10:40:00Z"),
            parent_seat_binding_id: None,
            created_at: at("2026-09-02T10:40:00Z"),
        })
        .expect("the extra delivery seat is bound");
    let run = AgentRunId::generate();
    w.store
        .create_agent_run(&NewAgentRun {
            id: run,
            project_id: w.project_id,
            team_run_id: w.team_run,
            parent_agent_run_id: None,
            // The exact durable shared key: a run persists its slot address as
            // `role_key`, and the seat persists the same slot.
            role: RoleSlotId::parse(slot)
                .expect("a slot")
                .as_role_key()
                .clone(),
            account_profile_id: None,
            binding: Some(RuntimeBinding {
                id: RuntimeBindingId::generate(),
                agent_run_id: run,
                identity: identity(native),
                bound_at: at("2026-09-02T10:41:00Z"),
            }),
            created_at: at("2026-09-02T10:41:00Z"),
        })
        .expect("its native session is bound");
    seat
}

#[test]
fn an_ordinary_multi_seat_team_run_censuses_each_seat_exactly_once() {
    let w = world();
    // A second and third delivery seat on the same TeamRun, each with its own
    // slot, agent run and native session — the ordinary shape of a delivery
    // team, and the shape the live high-stakes TSW has.
    let reviewer = add_delivery_seat(&w, "delivery.reviewer", "SA", "agent_delivery_sa");
    let lead = add_delivery_seat(&w, "delivery.lead", "LSA", "agent_delivery_lsa");

    let census = w
        .store
        .list_live_native_subjects(w.project_id, w.mini_project_id)
        .expect("an ordinary multi-seat TeamRun censuses cleanly");
    let seats: Vec<(SeatBindingId, String)> = census
        .iter()
        .filter_map(|live| match live.subject {
            TeamDefinitionMigrationSubject::Seat {
                seat_binding_id, ..
            } => Some((seat_binding_id, live.identity.native_id.as_str().to_owned())),
            TeamDefinitionMigrationSubject::Container { .. } => None,
        })
        .collect();
    assert_eq!(
        seats.len(),
        3,
        "each delivery seat is enumerated exactly once, not paired with every \
         session on its team run: {seats:?}"
    );
    let mut paired: Vec<(SeatBindingId, String)> = seats.clone();
    paired.sort();
    let mut expected = vec![
        (w.delivery_seat, "agent_delivery_aud".to_owned()),
        (reviewer, "agent_delivery_sa".to_owned()),
        (lead, "agent_delivery_lsa".to_owned()),
    ];
    expected.sort();
    assert_eq!(
        paired, expected,
        "every seat keeps its own SeatBinding id and its own native session"
    );

    // And the whole epic still migrates: a normal multi-seat TeamRun is not a
    // reason to refuse.
    let mut targets = complete_targets(&w);
    for (seat, native, title) in [
        (reviewer, "agent_delivery_sa", "SA"),
        (lead, "agent_delivery_lsa", "LSA"),
    ] {
        targets.push(NewTeamDefinitionMigrationTarget {
            subject: TeamDefinitionMigrationSubject::Seat {
                topology_node_id: w.tsw,
                seat_binding_id: seat,
            },
            identity: identity(native),
            desired: NativePlacement {
                title: name(title),
                parent_native_id: Some(ExternalId::parse("wks_tsw").expect("a native id")),
                kind: MigrationObjectKind::Seat,
                canonical_cwd: None,
            },
        });
    }
    w.store
        .record_team_definition_migration(&migration(&w, "multi-seat-team-run", targets))
        .expect("a multi-seat TeamRun is migratable");
}

#[test]
fn a_live_delivery_session_with_no_seat_at_its_slot_fails_closed() {
    let w = world();
    // A bound agent run whose slot no active seat carries. It cannot be named
    // as a subject, and excluding it silently is the skip this census exists to
    // prevent, so the epic is unmigratable until seat and run agree.
    let orphan = AgentRunId::generate();
    w.store
        .create_agent_run(&NewAgentRun {
            id: orphan,
            project_id: w.project_id,
            team_run_id: w.team_run,
            parent_agent_run_id: None,
            role: RoleSlotId::parse("delivery.nobody")
                .expect("a slot")
                .as_role_key()
                .clone(),
            account_profile_id: None,
            binding: Some(RuntimeBinding {
                id: RuntimeBindingId::generate(),
                agent_run_id: orphan,
                identity: identity("agent_delivery_orphan"),
                bound_at: at("2026-09-02T10:41:00Z"),
            }),
            created_at: at("2026-09-02T10:41:00Z"),
        })
        .expect("the orphaned session is bound");

    assert!(
        w.store
            .list_live_native_subjects(w.project_id, w.mini_project_id)
            .is_err(),
        "a live delivery session with no seat at its slot fails closed"
    );
    assert!(
        w.store
            .record_team_definition_migration(&migration(
                &w,
                "orphaned-delivery-session",
                complete_targets(&w)
            ))
            .is_err(),
        "and no migration can be recorded while a live session cannot be named"
    );
}

#[test]
fn an_epic_team_run_with_no_seat_bindings_at_all_cannot_advance_the_pin() {
    let w = world();
    // The shape observed live: a delivery TeamRun in the epic whose bound
    // AgentRuns have no SeatBinding rows whatsoever. Matching on the slot alone
    // would enumerate nothing for them and let the pin move over four live
    // native sessions.
    let task = TaskId::generate();
    w.store
        .create_task(&NewTask {
            id: task,
            project_id: w.project_id,
            mini_project_id: Some(w.mini_project_id),
            title: name("Seatless delivery task"),
            module: None,
            state: TaskState::Ready,
            created_at: at("2026-09-02T10:35:00Z"),
        })
        .expect("the task is created");
    let template = TeamTemplateRevision {
        template_id: kontor_core::id::TeamTemplateId::generate(),
        version: SpecVersion::FIRST,
        name: name("Seatless team"),
        definition: CanonicalDocument::from_serializable(&serde_json::json!({
            "schema_version": 1,
            "team": "seatless",
        }))
        .expect("a canonical team document"),
        role_authority: Vec::new(),
    };
    w.store
        .insert_team_template(w.project_id, &template)
        .expect("the template is stored");
    let team_run = TeamRunId::generate();
    w.store
        .create_team_run(&NewTeamRun {
            id: team_run,
            project_id: w.project_id,
            task_id: task,
            snapshot: TeamRunSnapshot::from_revision(&template, kontor_core::id::SCHEMA_VERSION),
            created_at: at("2026-09-02T10:36:00Z"),
        })
        .expect("the team run is created");
    for slot in ["scope", "implement", "verify", "audit"] {
        let run = AgentRunId::generate();
        w.store
            .create_agent_run(&NewAgentRun {
                id: run,
                project_id: w.project_id,
                team_run_id: team_run,
                parent_agent_run_id: None,
                role: RoleSlotId::parse(slot)
                    .expect("a slot")
                    .as_role_key()
                    .clone(),
                account_profile_id: None,
                binding: Some(RuntimeBinding {
                    id: RuntimeBindingId::generate(),
                    agent_run_id: run,
                    identity: identity(&format!("agent_{slot}")),
                    bound_at: at("2026-09-02T10:37:00Z"),
                }),
                created_at: at("2026-09-02T10:37:00Z"),
            })
            .expect("the seatless delivery session is bound");
    }

    assert!(
        w.store
            .list_live_native_subjects(w.project_id, w.mini_project_id)
            .is_err(),
        "four bound sessions with no seat rows must fail the census closed"
    );
    assert!(
        w.store
            .record_team_definition_migration(&migration(
                &w,
                "seatless-team-run",
                complete_targets(&w)
            ))
            .is_err(),
        "record refuses while a live session cannot be enumerated"
    );
    assert_eq!(
        w.store
            .get_mini_project_team_definition(w.project_id, w.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        w.definition.version,
        "the pin cannot advance over unenumerable live sessions"
    );
}

#[test]
fn a_migration_recorded_before_a_seatless_session_appears_cannot_confirm() {
    let w = world();
    let recorded = w
        .store
        .record_team_definition_migration(&migration(
            &w,
            "seatless-appears-later",
            complete_targets(&w),
        ))
        .expect("a complete migration is recorded while the epic is coherent");
    read_every_target_back(&w, recorded.id);

    // A bound delivery session appears on the epic's existing team run with no
    // seat at its slot.
    let orphan = AgentRunId::generate();
    w.store
        .create_agent_run(&NewAgentRun {
            id: orphan,
            project_id: w.project_id,
            team_run_id: w.team_run,
            parent_agent_run_id: None,
            role: RoleSlotId::parse("delivery.unseated")
                .expect("a slot")
                .as_role_key()
                .clone(),
            account_profile_id: None,
            binding: Some(RuntimeBinding {
                id: RuntimeBindingId::generate(),
                agent_run_id: orphan,
                identity: identity("agent_delivery_unseated"),
                bound_at: at("2026-09-02T10:41:00Z"),
            }),
            created_at: at("2026-09-02T10:41:00Z"),
        })
        .expect("the seatless session is bound");

    assert!(
        w.store
            .confirm_team_definition_migration(
                w.project_id,
                recorded.id,
                at("2026-09-02T11:05:00Z")
            )
            .is_err(),
        "confirmation refuses while a live session cannot be enumerated"
    );
    assert_eq!(
        w.store
            .get_mini_project_team_definition(w.project_id, w.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        w.definition.version,
        "the pin does not move"
    );
}

#[test]
fn a_migration_that_omits_a_live_native_subject_is_refused_before_any_mutation() {
    let w = world();
    // Only the ESW is enumerated; every other live native is silently skipped.
    let refused = w.store.record_team_definition_migration(&migration(
        &w,
        "omits-the-rest",
        vec![container_target(w.esw, "wks_esw", "ESW • KBI-8049")],
    ));
    assert!(
        refused.is_err(),
        "a migration that leaves a live native subject unenumerated must be \
         refused before the first retitle"
    );
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
    w.store
        .record_team_definition_migration(&migration(&w, "covers-everything", complete_targets(&w)))
        .expect("a complete migration is recorded");
}

#[test]
fn a_target_definition_that_does_not_declare_a_live_node_kind_is_refused() {
    let w = world();
    // Strip TSW from the target definition while a TSW container and its
    // delivery seat are live.
    let mut narrowed = w.second.clone();
    narrowed
        .containers
        .retain(|container| container.kind.as_str() != "TSW");
    narrowed.version = SpecVersion::parse(3).expect("a valid version");
    w.store
        .publish_team_definition(w.project_id, &narrowed, at("2026-09-02T09:40:00Z"))
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
    read_every_target_back(&w, recorded.id);

    // A native container appears after the preview was taken.
    let asw = TopologyNodeId::generate();
    w.store
        .create_topology_node(&NewSessionTopologyNode {
            id: asw,
            project_id: w.project_id,
            mini_project_id: Some(w.mini_project_id),
            topology: w
                .store
                .get_mini_project_topology(w.project_id, w.mini_project_id)
                .expect("the read succeeds")
                .expect("the epic pins a topology")
                .topology,
            kind: TopologyKindKey::parse("ASW").expect("a kind"),
            parent_id: Some(w.esw),
            task_id: None,
            created_at: at("2026-09-02T10:30:00Z"),
        })
        .expect("an advisor workspace node appears");
    w.store
        .bind_topology_node_container(&NewNativeContainerBinding {
            topology_node_id: asw,
            project_id: w.project_id,
            container_binding_id: ExternalId::parse("bind_wks_asw").expect("a binding id"),
            identity: identity("wks_asw"),
            observed_kind: ObservedContainerKind::Workspace,
            canonical_cwd: Some(name("/tmp/kontor")),
            observed_at: at("2026-09-02T10:31:00Z"),
        })
        .expect("it takes a native container");

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
    read_every_target_back(&w, recorded.id);
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
