//! Schema v77: the immutable Team Definition, its selection and epic pin, the
//! resumable identity-preserving migration intent, and legacy-compatible
//! consultation topic storage.

use kontor_core::consultation::{ConsultationFamily, ConsultationRunId, ConsultationRunState};
use kontor_core::id::{
    AdvisorRunId, AggregateRevision, ContentHash, ExternalId, ExternalName, IdempotencyKey,
    MiniProjectId, ProjectId, RoleCode, RoleSlotId, RuntimeKindKey, SeatBindingId, SpecVersion,
    TeamDefinitionMigrationId, Timestamp, TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::naming::NativeNameValues;
use kontor_core::repository::{
    MigrationObjectKind, MiniProjectTeamDefinitionSnapshot, MiniProjectTopologySnapshot,
    NativePlacement, NewMiniProject, NewProject, NewSeatBinding, NewSessionTopologyNode,
    NewTeamDefinitionMigration, NewTeamDefinitionMigrationTarget, ProjectRepository,
    ProjectTeamDefinitionDefault, StoredConsultationProfileRevision, StoredConsultationRun,
    TeamDefinitionMigrationObservation, TeamDefinitionMigrationState,
    TeamDefinitionMigrationSubject, TeamDefinitionMigrationTargetState, TeamDefinitionRepository,
    TopologyRepository,
};
use kontor_core::spec::{
    CatalogRoleRef, Shareability, ShareabilityTier, TeamDefinitionSnapshot, TeamDefinitionSpec,
    TopologySnapshot,
};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use tempfile::TempDir;

/// The advisor profile the topic fixtures freeze against.
const ADVISOR_PROFILE: &str = "01991c00-0000-7000-8000-00000000008a";

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn default_stamp() -> Shareability {
    Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("tier B classifies")
}

/// A project with the bundled topology published, plus one epic.
struct Fixture {
    home: TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    topology: TopologySnapshot,
    definition: TeamDefinitionSpec,
    created_at: Timestamp,
    /// A real ESW node of this epic; migration targets must name owned nodes.
    esw: TopologyNodeId,
    /// A real ECP node of this epic, used for several seats on one node.
    ecp: TopologyNodeId,
}

fn fixture() -> Fixture {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    let mini_project_id = MiniProjectId::generate();
    let created_at = at("2026-09-01T12:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Team definition project"),
            root_path: name("/tmp/team-definition-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_mini_project(&NewMiniProject {
            id: mini_project_id,
            project_id,
            name: name("Team definition epic"),
            created_at,
        })
        .expect("the epic is created");

    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let definition = domain
        .team_definitions
        .first()
        .expect("a bundled team definition")
        .clone();
    // The exact revision this definition names as its validator, not simply the
    // first one the pack happens to declare.
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
    let topology = TopologySnapshot {
        spec_id: topology_spec.spec_id,
        version: topology_spec.version,
        canonical_hash,
    };
    // A real epic carries both pins: topology legalizes the nodes it places,
    // and the Team Definition names them.
    store
        .pin_mini_project_topology(&MiniProjectTopologySnapshot {
            project_id,
            mini_project_id,
            topology: topology.clone(),
            pinned_at: created_at,
        })
        .expect("the epic pins its topology validator");
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
    let esw = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: esw,
            project_id,
            mini_project_id: Some(mini_project_id),
            topology: topology.clone(),
            kind: TopologyKindKey::parse("ESW").expect("the epic kind"),
            parent_id: Some(root),
            task_id: None,
            created_at,
        })
        .expect("the epic node is created");
    let ecp = TopologyNodeId::generate();
    store
        .create_topology_node(&NewSessionTopologyNode {
            id: ecp,
            project_id,
            mini_project_id: Some(mini_project_id),
            topology: topology.clone(),
            kind: TopologyKindKey::parse("ECP").expect("the control-plane kind"),
            parent_id: Some(esw),
            task_id: None,
            created_at,
        })
        .expect("the ECP node is created");

    Fixture {
        home,
        store,
        project_id,
        mini_project_id,
        topology,
        definition,
        created_at,
        esw,
        ecp,
    }
}

/// The four-part native identity of one object under test.
fn identity(native: &str) -> NativeRuntimeIdentity {
    NativeRuntimeIdentity {
        runtime_kind: RuntimeKindKey::parse("paseo").expect("a runtime kind"),
        host: name("localhost"),
        generation: 1,
        native_id: ExternalId::parse(native).expect("a native id"),
    }
}

/// A workspace-container placement: the common case a retitle changes.
fn placement(title: &str) -> NativePlacement {
    NativePlacement {
        title: name(title),
        parent_native_id: Some(ExternalId::parse("wks_root").expect("a native id")),
        kind: MigrationObjectKind::WorkspaceContainer,
        canonical_cwd: Some(name("/tmp/kontor")),
    }
}

/// A native-root placement. A root has no container above it.
fn root_placement(title: &str) -> NativePlacement {
    NativePlacement {
        title: name(title),
        parent_native_id: None,
        kind: MigrationObjectKind::ProjectContainer,
        canonical_cwd: Some(name("/tmp/kontor")),
    }
}

/// A seat placement. A seat names the container it sits in and need not own a
/// working directory of its own.
fn seat_placement(title: &str, container: &str) -> NativePlacement {
    NativePlacement {
        title: name(title),
        parent_native_id: Some(ExternalId::parse(container).expect("a native id")),
        kind: MigrationObjectKind::Seat,
        canonical_cwd: None,
    }
}

/// The same definition at a later immutable revision.
fn next_revision(definition: &TeamDefinitionSpec, version: u32) -> TeamDefinitionSpec {
    let mut next = definition.clone();
    next.version = SpecVersion::parse(version).expect("a valid version");
    next
}

fn snapshot(definition: &TeamDefinitionSpec) -> TeamDefinitionSnapshot {
    TeamDefinitionSnapshot::from_revision(definition).expect("a snapshot")
}

// ---------------------------------------------------------------------------
// Publication
// ---------------------------------------------------------------------------

#[test]
fn a_published_team_definition_revision_cannot_be_replaced_even_with_the_same_bytes() {
    let f = fixture();
    f.store
        .publish_team_definition(f.project_id, &f.definition, f.created_at)
        .expect("the definition is first published");
    assert!(
        f.store
            .publish_team_definition(f.project_id, &f.definition, f.created_at)
            .is_err(),
        "a published revision is append-only; idempotency belongs above the store"
    );
}

#[test]
fn a_definition_naming_an_unpublished_topology_revision_is_refused() {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    let created_at = at("2026-09-01T12:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Bare project"),
            root_path: name("/tmp/bare-project"),
            created_at,
        })
        .expect("the project is created");
    let definition = bundled_operational_domain()
        .expect("the bundled domain validates")
        .team_definitions
        .first()
        .expect("a bundled team definition")
        .clone();
    assert!(
        store
            .publish_team_definition(project_id, &definition, created_at)
            .is_err(),
        "a definition cannot cite a topology validator the project never published"
    );
}

#[test]
fn definitions_round_trip_byte_exactly_and_list_deterministically() {
    let f = fixture();
    let second = next_revision(&f.definition, 2);
    f.store
        .publish_team_definition(f.project_id, &f.definition, f.created_at)
        .expect("v1 publishes");
    f.store
        .publish_team_definition(f.project_id, &second, f.created_at)
        .expect("v2 publishes");

    let read = f
        .store
        .get_team_definition(
            f.project_id,
            f.definition.definition_id,
            f.definition.version,
        )
        .expect("the read succeeds")
        .expect("the revision is present");
    assert_eq!(read, f.definition, "the exact published bytes come back");

    let listed = f
        .store
        .list_team_definitions(f.project_id)
        .expect("the list succeeds");
    assert_eq!(
        listed
            .iter()
            .map(|definition| definition.version)
            .collect::<Vec<_>>(),
        vec![f.definition.version, second.version],
        "revisions list in deterministic identity/version order"
    );
}

// ---------------------------------------------------------------------------
// Selection and epic pin
// ---------------------------------------------------------------------------

#[test]
fn the_project_default_is_re_selectable_and_refuses_a_hash_that_was_never_published() {
    let f = fixture();
    f.store
        .publish_team_definition(f.project_id, &f.definition, f.created_at)
        .expect("v1 publishes");
    let second = next_revision(&f.definition, 2);
    f.store
        .publish_team_definition(f.project_id, &second, f.created_at)
        .expect("v2 publishes");

    f.store
        .set_project_team_definition_default(&ProjectTeamDefinitionDefault {
            project_id: f.project_id,
            expected: None,
            definition: snapshot(&f.definition),
            selected_at: f.created_at,
        })
        .expect("v1 is selected");
    f.store
        .set_project_team_definition_default(&ProjectTeamDefinitionDefault {
            project_id: f.project_id,
            expected: Some(snapshot(&f.definition)),
            definition: snapshot(&second),
            selected_at: at("2026-09-01T13:00:00Z"),
        })
        .expect("the default moves to v2");
    assert_eq!(
        f.store
            .get_project_team_definition_default(f.project_id)
            .expect("the read succeeds")
            .expect("a default is selected")
            .definition
            .version,
        second.version
    );

    let mut forged = snapshot(&f.definition);
    forged.canonical_hash = ContentHash::of(b"not these bytes");
    assert!(
        f.store
            .set_project_team_definition_default(&ProjectTeamDefinitionDefault {
                project_id: f.project_id,
                expected: Some(snapshot(&second)),
                definition: forged,
                selected_at: f.created_at,
            })
            .is_err(),
        "a selection must name the exact published bytes"
    );
}

#[test]
fn an_epic_keeps_its_frozen_definition_when_the_project_default_moves() {
    let f = fixture();
    f.store
        .publish_team_definition(f.project_id, &f.definition, f.created_at)
        .expect("v1 publishes");
    let second = next_revision(&f.definition, 2);
    f.store
        .publish_team_definition(f.project_id, &second, f.created_at)
        .expect("v2 publishes");
    f.store
        .pin_mini_project_team_definition(&MiniProjectTeamDefinitionSnapshot {
            project_id: f.project_id,
            mini_project_id: f.mini_project_id,
            definition: snapshot(&f.definition),
            pinned_at: f.created_at,
        })
        .expect("the epic freezes v1");

    // Selecting a newer project default is exactly the mutant this kills: an
    // epic reads its own frozen pin, never the project's current preference.
    f.store
        .set_project_team_definition_default(&ProjectTeamDefinitionDefault {
            project_id: f.project_id,
            expected: None,
            definition: snapshot(&second),
            selected_at: at("2026-09-01T13:00:00Z"),
        })
        .expect("the project default moves to v2");

    assert_eq!(
        f.store
            .get_mini_project_team_definition(f.project_id, f.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        f.definition.version,
        "the epic still reads the revision it froze"
    );
}

#[test]
fn a_second_pin_for_the_same_epic_is_refused() {
    let f = fixture();
    f.store
        .publish_team_definition(f.project_id, &f.definition, f.created_at)
        .expect("v1 publishes");
    let pin = MiniProjectTeamDefinitionSnapshot {
        project_id: f.project_id,
        mini_project_id: f.mini_project_id,
        definition: snapshot(&f.definition),
        pinned_at: f.created_at,
    };
    f.store
        .pin_mini_project_team_definition(&pin)
        .expect("the first pin is accepted");
    assert!(
        f.store.pin_mini_project_team_definition(&pin).is_err(),
        "an epic freezes its definition once; moving it is the migration authority"
    );
}

// ---------------------------------------------------------------------------
// Resumable identity-preserving migration
// ---------------------------------------------------------------------------

struct Migration {
    fixture: Fixture,
    second: TeamDefinitionSpec,
    node: TopologyNodeId,
    native: NativeRuntimeIdentity,
}

fn migration_fixture() -> Migration {
    let f = fixture();
    f.store
        .publish_team_definition(f.project_id, &f.definition, f.created_at)
        .expect("v1 publishes");
    let second = next_revision(&f.definition, 2);
    f.store
        .publish_team_definition(f.project_id, &second, f.created_at)
        .expect("v2 publishes");
    f.store
        .pin_mini_project_team_definition(&MiniProjectTeamDefinitionSnapshot {
            project_id: f.project_id,
            mini_project_id: f.mini_project_id,
            definition: snapshot(&f.definition),
            pinned_at: f.created_at,
        })
        .expect("the epic freezes v1");
    Migration {
        second,
        node: f.esw,
        native: identity("wks_epic_root"),
        fixture: f,
    }
}

fn new_migration(m: &Migration, key: &str) -> NewTeamDefinitionMigration {
    NewTeamDefinitionMigration {
        id: TeamDefinitionMigrationId::generate(),
        project_id: m.fixture.project_id,
        mini_project_id: m.fixture.mini_project_id,
        idempotency_key: IdempotencyKey::parse(key).expect("a key"),
        from: Some(snapshot(&m.fixture.definition)),
        to: snapshot(&m.second),
        targets: vec![NewTeamDefinitionMigrationTarget {
            subject: TeamDefinitionMigrationSubject::Container {
                topology_node_id: m.node,
            },
            identity: m.native.clone(),
            desired: placement("ESW • KBI-8049"),
        }],
        command_intent_hash: ContentHash::of(b"command-intent"),
        recorded_at: at("2026-09-01T14:00:00Z"),
    }
}

#[test]
fn a_migration_replays_under_its_key_instead_of_recording_a_rival() {
    let m = migration_fixture();
    let request = new_migration(&m, "migrate-kbi-8049");
    let first = m
        .fixture
        .store
        .record_team_definition_migration(&request)
        .expect("the intent is recorded");
    assert_eq!(first.state, TeamDefinitionMigrationState::Recorded);
    assert_eq!(first.targets.len(), 1);

    // A resumed apply presents the same key with a fresh intent id. It must get
    // the migration already in flight, not a second one racing it.
    let mut replay = new_migration(&m, "migrate-kbi-8049");
    replay.id = TeamDefinitionMigrationId::generate();
    let second = m
        .fixture
        .store
        .record_team_definition_migration(&replay)
        .expect("the replay resolves to the original");
    assert_eq!(second.id, first.id, "the same key is the same migration");
}

#[test]
fn an_epic_may_have_only_one_migration_in_flight() {
    let m = migration_fixture();
    m.fixture
        .store
        .record_team_definition_migration(&new_migration(&m, "migrate-once"))
        .expect("the first intent is recorded");
    assert!(
        m.fixture
            .store
            .record_team_definition_migration(&new_migration(&m, "migrate-twice"))
            .is_err(),
        "a second in-flight migration would interleave renames with the first"
    );
}

#[test]
fn the_fence_reports_the_in_flight_migration_and_clears_when_it_is_abandoned() {
    let m = migration_fixture();
    let recorded = m
        .fixture
        .store
        .record_team_definition_migration(&new_migration(&m, "migrate-then-abandon"))
        .expect("the intent is recorded");
    assert_eq!(
        m.fixture
            .store
            .get_in_flight_team_definition_migration(
                m.fixture.project_id,
                m.fixture.mini_project_id
            )
            .expect("the read succeeds")
            .expect("a migration fences the epic")
            .id,
        recorded.id
    );

    let failed = m
        .fixture
        .store
        .fail_team_definition_migration(
            m.fixture.project_id,
            recorded.id,
            at("2026-09-01T15:00:00Z"),
        )
        .expect("the migration is abandoned");
    assert_eq!(failed.state, TeamDefinitionMigrationState::Failed);
    assert!(
        m.fixture
            .store
            .get_in_flight_team_definition_migration(
                m.fixture.project_id,
                m.fixture.mini_project_id
            )
            .expect("the read succeeds")
            .is_none(),
        "an abandoned migration stops fencing the epic"
    );
    assert_eq!(
        m.fixture
            .store
            .get_mini_project_team_definition(m.fixture.project_id, m.fixture.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        m.fixture.definition.version,
        "an abandoned migration leaves the epic on the definition its natives render"
    );
}

#[test]
fn an_observation_whose_native_id_changed_is_refused() {
    let m = migration_fixture();
    let recorded = m
        .fixture
        .store
        .record_team_definition_migration(&new_migration(&m, "migrate-identity"))
        .expect("the intent is recorded");
    assert!(
        m.fixture
            .store
            .observe_team_definition_migration(
                m.fixture.project_id,
                recorded.id,
                &[TeamDefinitionMigrationObservation {
                    subject: TeamDefinitionMigrationSubject::Container {
                        topology_node_id: m.node,
                    },
                    identity: identity("wks_a_different_workspace"),
                    observed: Some(placement("ESW • KBI-8049")),
                    state: TeamDefinitionMigrationTargetState::Renamed,
                    observed_at: at("2026-09-01T15:00:00Z"),
                }],
                at("2026-09-01T15:00:00Z"),
            )
            .is_err(),
        "a readback carrying another native id did not observe this target"
    );
}

#[test]
fn the_pin_moves_only_after_every_target_reads_back_its_desired_title() {
    let m = migration_fixture();
    let recorded = m
        .fixture
        .store
        .record_team_definition_migration(&new_migration(&m, "migrate-readback"))
        .expect("the intent is recorded");

    // A rename that was asked for and not confirmed is recorded as exactly
    // that, and it is not enough to move the pin.
    let applying = m
        .fixture
        .store
        .observe_team_definition_migration(
            m.fixture.project_id,
            recorded.id,
            &[TeamDefinitionMigrationObservation {
                subject: TeamDefinitionMigrationSubject::Container {
                    topology_node_id: m.node,
                },
                identity: m.native.clone(),
                observed: None,
                state: TeamDefinitionMigrationTargetState::RenamePending,
                observed_at: at("2026-09-01T15:00:00Z"),
            }],
            at("2026-09-01T15:00:00Z"),
        )
        .expect("the pending observation is recorded");
    assert_eq!(applying.state, TeamDefinitionMigrationState::Applying);
    assert!(
        m.fixture
            .store
            .confirm_team_definition_migration(
                m.fixture.project_id,
                recorded.id,
                at("2026-09-01T15:01:00Z")
            )
            .is_err(),
        "a partial apply never becomes a confirmed pin"
    );
    assert_eq!(
        m.fixture
            .store
            .get_mini_project_team_definition(m.fixture.project_id, m.fixture.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        m.fixture.definition.version,
        "the epic still holds the old pin while the migration is in flight"
    );

    // The exact title reads back under the same native id, and only now does
    // the pin move.
    m.fixture
        .store
        .observe_team_definition_migration(
            m.fixture.project_id,
            recorded.id,
            &[TeamDefinitionMigrationObservation {
                subject: TeamDefinitionMigrationSubject::Container {
                    topology_node_id: m.node,
                },
                identity: m.native.clone(),
                observed: Some(placement("ESW • KBI-8049")),
                state: TeamDefinitionMigrationTargetState::Renamed,
                observed_at: at("2026-09-01T15:02:00Z"),
            }],
            at("2026-09-01T15:02:00Z"),
        )
        .expect("the confirmed readback is recorded");
    let confirmed = m
        .fixture
        .store
        .confirm_team_definition_migration(
            m.fixture.project_id,
            recorded.id,
            at("2026-09-01T15:03:00Z"),
        )
        .expect("every target read back, so the pin moves");
    assert_eq!(confirmed.state, TeamDefinitionMigrationState::Confirmed);
    assert_eq!(
        m.fixture
            .store
            .get_mini_project_team_definition(m.fixture.project_id, m.fixture.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        m.second.version,
        "the epic now holds the definition its natives render"
    );
    assert!(
        m.fixture
            .store
            .get_in_flight_team_definition_migration(
                m.fixture.project_id,
                m.fixture.mini_project_id
            )
            .expect("the read succeeds")
            .is_none(),
        "a confirmed migration stops fencing the epic"
    );
}

#[test]
fn a_recorded_migration_and_its_pin_survive_a_restart() {
    let m = migration_fixture();
    let recorded = m
        .fixture
        .store
        .record_team_definition_migration(&new_migration(&m, "migrate-restart"))
        .expect("the intent is recorded");
    let database = m.fixture.home.path().join("kontor.db");
    drop(m.fixture.store);

    let reopened = SqliteStore::open(&database).expect("the store reopens");
    let read = reopened
        .get_team_definition_migration(m.fixture.project_id, recorded.id)
        .expect("the read succeeds")
        .expect("the migration survived");
    assert_eq!(read.state, TeamDefinitionMigrationState::Recorded);
    assert_eq!(read.targets.len(), 1);
    assert_eq!(read.targets[0].identity, m.native);
    assert_eq!(read.targets[0].desired.title, name("ESW • KBI-8049"));
    assert_eq!(
        read.targets[0].state,
        TeamDefinitionMigrationTargetState::Pending
    );
    assert_eq!(
        read.from.expect("a prior pin").version,
        m.fixture.definition.version
    );
}

// ---------------------------------------------------------------------------
// Legacy-compatible consultation topic
// ---------------------------------------------------------------------------

/// Create one Advisor consultation, with or without an authoritative topic.
fn consultation_with_topic(f: &Fixture, topic: Option<ExternalName>) -> StoredConsultationRun {
    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let catalog = domain
        .role_catalogs
        .first()
        .expect("a role catalog")
        .clone();
    f.store
        .publish_role_catalog(&catalog, &default_stamp(), f.created_at)
        .expect("the catalog publishes");

    // The fixture already placed this epic's ESW and ECP; a second project
    // root would exceed the topology's declared cardinality.
    let esw = f.esw;
    let ecp = f.ecp;

    let entry = catalog
        .role(&RoleCode::parse("LSA").expect("a standard role code"))
        .expect("the catalog has the role");
    let caller = SeatBindingId::generate();
    f.store
        .create_seat_binding(&NewSeatBinding {
            id: caller,
            project_id: f.project_id,
            topology_node_id: ecp,
            role_slot_id: RoleSlotId::parse("epic.lsa").expect("a role slot"),
            role: CatalogRoleRef {
                catalog_id: catalog.catalog_id,
                catalog_revision: catalog.version,
                role_code: entry.role_code.clone(),
                standard_title: entry.standard_title.clone(),
                custom_display_name: Some(name("Kontor LSA")),
            },
            task_id: None,
            team_run_id: None,
            attach_deadline: at("2026-09-01T12:10:00Z"),
            parent_seat_binding_id: None,
            created_at: f.created_at,
        })
        .expect("the caller seat is bound");

    // The run's profile revision has to exist: a consultation cites the exact
    // published definition it was frozen against.
    let profile = kontor_core::id::CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "seats": ["SA"],
    }))
    .expect("a canonical advisor profile");
    f.store
        .publish_consultation_profile_revision(&StoredConsultationProfileRevision {
            project_id: f.project_id,
            family: ConsultationFamily::Advisor,
            profile_id: ADVISOR_PROFILE.to_owned(),
            version: SpecVersion::FIRST,
            name: name("Independent advisor"),
            definition: profile.json().to_owned(),
            definition_hash: profile.hash().clone(),
            published_at: f.created_at,
        })
        .expect("the advisor profile publishes");

    let question = kontor_core::id::BoundedText::parse(
        "Should the connector alias cleanup block the naming migration?",
    )
    .expect("a bounded question");
    let context = serde_json::json!({ "schema_version": 1 });
    let context_hash = kontor_core::id::CanonicalDocument::from_serializable(&context)
        .expect("canonical context")
        .hash()
        .clone();
    let asw = TopologyNodeId::generate();
    let run = StoredConsultationRun {
        id: ConsultationRunId::Advisor(AdvisorRunId::generate()),
        project_id: f.project_id,
        mini_project_id: f.mini_project_id,
        topic,
        profile_id: ADVISOR_PROFILE.to_owned(),
        profile_version: SpecVersion::FIRST,
        definition_hash: profile.hash().clone(),
        question_hash: ContentHash::of(question.as_str().as_bytes()),
        question,
        context,
        context_hash,
        caller_seat_binding_id: caller,
        topology_node_id: asw,
        invoke_key: IdempotencyKey::parse("invoke-advisor-1").expect("a key"),
        invoke_intent_hash: ContentHash::of(b"invoke-intent"),
        state: ConsultationRunState::Materializing,
        round: 1,
        result: None,
        result_hash: None,
        revision: AggregateRevision::INITIAL,
        created_at: f.created_at,
        updated_at: f.created_at,
        settled_at: None,
    };
    f.store
        .create_consultation_run(
            &run,
            &NewSessionTopologyNode {
                id: asw,
                project_id: f.project_id,
                mini_project_id: Some(f.mini_project_id),
                topology: f.topology.clone(),
                kind: TopologyKindKey::parse("ASW").expect("the advisor kind"),
                parent_id: Some(esw),
                task_id: None,
                created_at: f.created_at,
            },
            &[],
        )
        .expect("the consultation is frozen");
    run
}

#[test]
fn a_consultation_topic_round_trips_and_is_reachable_from_its_topology_node() {
    let f = fixture();
    let run = consultation_with_topic(&f, Some(name("Jira recovery")));

    let by_node = f
        .store
        .get_consultation_run_by_topology_node(f.project_id, run.topology_node_id)
        .expect("the read succeeds")
        .expect("the ASW node resolves to its consultation");
    assert_eq!(by_node.id, run.id);
    assert_eq!(
        by_node.topic.as_ref().map(ExternalName::as_str),
        Some("Jira recovery"),
        "the topic the ASW name renders comes back exactly"
    );
}

#[test]
fn a_legacy_consultation_without_a_topic_stays_readable_and_renders_nothing() {
    let f = fixture();
    let run = consultation_with_topic(&f, None);

    let read = f
        .store
        .get_consultation_run(f.project_id, run.id)
        .expect("the read succeeds")
        .expect("the legacy row is still readable");
    assert!(
        read.topic.is_none(),
        "a consultation recorded before topics has none, and none is inferred \
         from its question"
    );

    // Fail-closed: the ASW template cannot be rendered for this consultation.
    // Nothing substitutes the question, profile id or node UUID for the topic.
    let asw = f
        .definition
        .container(&TopologyKindKey::parse("ASW").expect("the advisor kind"))
        .expect("the definition configures ASW");
    assert!(
        asw.name_template
            .render(
                &f.definition.separator,
                &NativeNameValues::new()
                    .with_prefix("ASW")
                    .with_scope_item_code("KBI-8049"),
            )
            .is_err(),
        "an unmapped legacy consultation refuses to render before any mutation"
    );
}

// ---------------------------------------------------------------------------
// A node is not one native object
// ---------------------------------------------------------------------------

#[test]
fn several_seats_on_one_node_are_distinct_targets_that_survive_preview_and_confirmation() {
    let m = migration_fixture();
    // One ECP node carrying its own container and two control seats. Keying a
    // migration by the node would have collapsed all three into one row and
    // silently dropped two of them from the migration.
    let ecp = m.fixture.ecp;
    let container = TeamDefinitionMigrationSubject::Container {
        topology_node_id: ecp,
    };
    let lsa = TeamDefinitionMigrationSubject::Seat {
        topology_node_id: ecp,
        seat_binding_id: SeatBindingId::generate(),
    };
    let tpm = TeamDefinitionMigrationSubject::Seat {
        topology_node_id: ecp,
        seat_binding_id: SeatBindingId::generate(),
    };
    let subjects = [container, lsa, tpm];
    let identities = [
        identity("wks_ecp_container"),
        identity("agent_ecp_lsa"),
        identity("agent_ecp_tpm"),
    ];
    let desired = [
        placement("ECP • KBI-8049"),
        seat_placement("LSA", "wks_ecp_container"),
        seat_placement("TPM", "wks_ecp_container"),
    ];

    let recorded = m
        .fixture
        .store
        .record_team_definition_migration(&NewTeamDefinitionMigration {
            id: TeamDefinitionMigrationId::generate(),
            project_id: m.fixture.project_id,
            mini_project_id: m.fixture.mini_project_id,
            idempotency_key: IdempotencyKey::parse("migrate-ecp-and-its-seats").expect("a key"),
            from: Some(snapshot(&m.fixture.definition)),
            to: snapshot(&m.second),
            targets: subjects
                .iter()
                .zip(identities.iter())
                .zip(desired.iter())
                .map(
                    |((subject, identity), placement)| NewTeamDefinitionMigrationTarget {
                        subject: *subject,
                        identity: identity.clone(),
                        desired: placement.clone(),
                    },
                )
                .collect(),
            command_intent_hash: ContentHash::of(b"command-intent"),
            recorded_at: at("2026-09-01T14:00:00Z"),
        })
        .expect("all three targets are recorded");
    assert_eq!(
        recorded.targets.len(),
        3,
        "the container and both seats each persist as their own target"
    );
    for (subject, identity) in subjects.iter().zip(identities.iter()) {
        let target = recorded
            .targets
            .iter()
            .find(|target| &target.subject == subject)
            .expect("the subject persisted");
        assert_eq!(&target.identity, identity);
    }

    // Confirming needs every one of them, each under its own unchanged identity.
    m.fixture
        .store
        .observe_team_definition_migration(
            m.fixture.project_id,
            recorded.id,
            &[TeamDefinitionMigrationObservation {
                subject: container,
                identity: identities[0].clone(),
                observed: Some(desired[0].clone()),
                state: TeamDefinitionMigrationTargetState::Renamed,
                observed_at: at("2026-09-01T15:00:00Z"),
            }],
            at("2026-09-01T15:00:00Z"),
        )
        .expect("the container reads back");
    assert!(
        m.fixture
            .store
            .confirm_team_definition_migration(
                m.fixture.project_id,
                recorded.id,
                at("2026-09-01T15:01:00Z")
            )
            .is_err(),
        "the seats are still pending, so the pin does not move"
    );

    for index in 1..3 {
        m.fixture
            .store
            .observe_team_definition_migration(
                m.fixture.project_id,
                recorded.id,
                &[TeamDefinitionMigrationObservation {
                    subject: subjects[index],
                    identity: identities[index].clone(),
                    observed: Some(desired[index].clone()),
                    state: TeamDefinitionMigrationTargetState::Renamed,
                    observed_at: at("2026-09-01T15:02:00Z"),
                }],
                at("2026-09-01T15:02:00Z"),
            )
            .expect("each seat reads back on its own");
    }
    let confirmed = m
        .fixture
        .store
        .confirm_team_definition_migration(
            m.fixture.project_id,
            recorded.id,
            at("2026-09-01T15:03:00Z"),
        )
        .expect("every container and seat read back, so the pin moves");
    assert_eq!(confirmed.state, TeamDefinitionMigrationState::Confirmed);
    for (subject, identity) in subjects.iter().zip(identities.iter()) {
        let target = confirmed
            .targets
            .iter()
            .find(|target| &target.subject == subject)
            .expect("the subject survived confirmation");
        assert_eq!(
            target.state,
            TeamDefinitionMigrationTargetState::Renamed,
            "each seat is confirmed in its own right"
        );
        assert_eq!(
            &target.identity, identity,
            "identity is unchanged from preview through confirmation"
        );
    }
    assert_eq!(
        m.fixture
            .store
            .get_mini_project_team_definition(m.fixture.project_id, m.fixture.mini_project_id)
            .expect("the read succeeds")
            .expect("the epic is pinned")
            .definition
            .version,
        m.second.version
    );
}

#[test]
fn a_seat_target_and_its_container_do_not_share_a_key() {
    let node = TopologyNodeId::generate();
    let seat = SeatBindingId::generate();
    let container = TeamDefinitionMigrationSubject::Container {
        topology_node_id: node,
    };
    let seated = TeamDefinitionMigrationSubject::Seat {
        topology_node_id: node,
        seat_binding_id: seat,
    };
    assert_ne!(container.target_key(), seated.target_key());
    assert_eq!(container.topology_node_id(), seated.topology_node_id());
    assert_eq!(container.seat_binding_id(), None);
    assert_eq!(seated.seat_binding_id(), Some(seat));
}

#[test]
fn all_three_target_kinds_persist_and_confirm_under_their_own_identities() {
    let m = migration_fixture();
    // A native root, a workspace container and a seat. A seat is not a
    // workspace: recording it as one would put a false statement into the
    // evidence the retitle is supposed to prove.
    let root = TeamDefinitionMigrationSubject::Container {
        topology_node_id: m.node,
    };
    let workspace = TeamDefinitionMigrationSubject::Container {
        topology_node_id: m.fixture.ecp,
    };
    let seat = TeamDefinitionMigrationSubject::Seat {
        topology_node_id: m.fixture.ecp,
        seat_binding_id: SeatBindingId::generate(),
    };
    let subjects = [root, workspace, seat];
    let identities = [
        identity("wks_root"),
        identity("wks_ecp_container"),
        identity("agent_ecp_lsa"),
    ];
    let desired = [
        root_placement("ESW • KBI-8049"),
        placement("ECP • KBI-8049"),
        seat_placement("LSA", "wks_ecp_container"),
    ];

    let recorded = m
        .fixture
        .store
        .record_team_definition_migration(&NewTeamDefinitionMigration {
            id: TeamDefinitionMigrationId::generate(),
            project_id: m.fixture.project_id,
            mini_project_id: m.fixture.mini_project_id,
            idempotency_key: IdempotencyKey::parse("migrate-every-kind").expect("a key"),
            from: Some(snapshot(&m.fixture.definition)),
            to: snapshot(&m.second),
            targets: subjects
                .iter()
                .zip(identities.iter())
                .zip(desired.iter())
                .map(
                    |((subject, identity), placement)| NewTeamDefinitionMigrationTarget {
                        subject: *subject,
                        identity: identity.clone(),
                        desired: placement.clone(),
                    },
                )
                .collect(),
            command_intent_hash: ContentHash::of(b"command-intent"),
            recorded_at: at("2026-09-01T14:00:00Z"),
        })
        .expect("a root, a workspace and a seat are all recordable targets");

    for (index, subject) in subjects.iter().enumerate() {
        let target = recorded
            .targets
            .iter()
            .find(|target| &target.subject == subject)
            .expect("the subject persisted");
        assert_eq!(
            target.desired.kind, desired[index].kind,
            "each target keeps the kind it actually is"
        );
    }
    let seat_target = recorded
        .targets
        .iter()
        .find(|target| target.subject == seat)
        .expect("the seat persisted");
    assert_eq!(
        seat_target.desired.kind,
        MigrationObjectKind::Seat,
        "a seat is a seat, not a workspace"
    );
    assert_eq!(
        seat_target.desired.parent_native_id,
        Some(ExternalId::parse("wks_ecp_container").expect("a native id")),
        "a seat readback proves the container it sits in"
    );
    assert!(
        seat_target.desired.canonical_cwd.is_none(),
        "a seat need not own a working directory"
    );

    for (index, subject) in subjects.iter().enumerate() {
        m.fixture
            .store
            .observe_team_definition_migration(
                m.fixture.project_id,
                recorded.id,
                &[TeamDefinitionMigrationObservation {
                    subject: *subject,
                    identity: identities[index].clone(),
                    observed: Some(desired[index].clone()),
                    state: TeamDefinitionMigrationTargetState::Renamed,
                    observed_at: at("2026-09-01T15:00:00Z"),
                }],
                at("2026-09-01T15:00:00Z"),
            )
            .expect("each kind reads back under its own identity");
    }
    let confirmed = m
        .fixture
        .store
        .confirm_team_definition_migration(
            m.fixture.project_id,
            recorded.id,
            at("2026-09-01T15:03:00Z"),
        )
        .expect("all three kinds confirmed, so the pin moves");
    assert_eq!(confirmed.state, TeamDefinitionMigrationState::Confirmed);
}

#[test]
fn a_placement_must_describe_the_subject_it_is_recorded_against() {
    let m = migration_fixture();
    // A seat recorded as a container, and a container recorded as a seat, both
    // make the readback prove something other than what was asked.
    let mismatched = [
        (
            TeamDefinitionMigrationSubject::Seat {
                topology_node_id: m.fixture.ecp,
                seat_binding_id: SeatBindingId::generate(),
            },
            placement("LSA"),
        ),
        (
            TeamDefinitionMigrationSubject::Container {
                topology_node_id: m.fixture.ecp,
            },
            seat_placement("ECP • KBI-8049", "wks_root"),
        ),
    ];
    for (index, (subject, desired)) in mismatched.into_iter().enumerate() {
        assert!(
            m.fixture
                .store
                .record_team_definition_migration(&NewTeamDefinitionMigration {
                    id: TeamDefinitionMigrationId::generate(),
                    project_id: m.fixture.project_id,
                    mini_project_id: m.fixture.mini_project_id,
                    idempotency_key: IdempotencyKey::parse(&format!("mismatch-{index}"))
                        .expect("a key"),
                    from: Some(snapshot(&m.fixture.definition)),
                    to: snapshot(&m.second),
                    targets: vec![NewTeamDefinitionMigrationTarget {
                        subject,
                        identity: identity("wks_mismatch"),
                        desired,
                    }],
                    command_intent_hash: ContentHash::of(b"command-intent"),
                    recorded_at: at("2026-09-01T14:00:00Z"),
                })
                .is_err(),
            "the object kind must describe the subject it is about"
        );
    }
}

// ---------------------------------------------------------------------------
// Operator-supplied legacy topics
// ---------------------------------------------------------------------------

/// A fixture with one topicless consultation and one in-flight migration.
fn legacy_topic_fixture() -> (Migration, TopologyNodeId, TeamDefinitionMigrationId) {
    let m = migration_fixture();
    let run = consultation_with_topic(&m.fixture, None);
    let recorded = m
        .fixture
        .store
        .record_team_definition_migration(&new_migration(&m, "migrate-legacy-topics"))
        .expect("the intent is recorded");
    (m, run.topology_node_id, recorded.id)
}

#[test]
fn an_operator_supplied_topic_is_written_once_with_its_migration_provenance() {
    let (m, node, intent) = legacy_topic_fixture();
    let updated = m
        .fixture
        .store
        .supply_legacy_consultation_topic(
            m.fixture.project_id,
            node,
            intent,
            &name("Jira recovery"),
            at("2026-09-01T14:30:00Z"),
        )
        .expect("the operator's topic is accepted for a consultation that had none");
    assert_eq!(
        updated.topic.as_ref().map(ExternalName::as_str),
        Some("Jira recovery")
    );

    // Same value again is the replay of a migration step, not a new decision.
    let replayed = m
        .fixture
        .store
        .supply_legacy_consultation_topic(
            m.fixture.project_id,
            node,
            intent,
            &name("Jira recovery"),
            at("2026-09-01T14:31:00Z"),
        )
        .expect("supplying the same topic again is idempotent");
    assert_eq!(replayed.topic, updated.topic);

    // A different value is a conflict: the first has already been rendered
    // into a native title and treated as authoritative.
    assert!(
        m.fixture
            .store
            .supply_legacy_consultation_topic(
                m.fixture.project_id,
                node,
                intent,
                &name("Naming contract"),
                at("2026-09-01T14:32:00Z"),
            )
            .is_err(),
        "a consultation does not quietly change the topic its name was built from"
    );
}

#[test]
fn a_topic_cannot_be_supplied_across_a_project_boundary() {
    let (m, node, _) = legacy_topic_fixture();
    let other = fixture();
    other
        .store
        .publish_team_definition(other.project_id, &other.definition, other.created_at)
        .expect("the other project publishes its own definition");
    let second = next_revision(&other.definition, 2);
    other
        .store
        .publish_team_definition(other.project_id, &second, other.created_at)
        .expect("v2 publishes");
    other
        .store
        .pin_mini_project_team_definition(&MiniProjectTeamDefinitionSnapshot {
            project_id: other.project_id,
            mini_project_id: other.mini_project_id,
            definition: snapshot(&other.definition),
            pinned_at: other.created_at,
        })
        .expect("the other epic freezes v1");
    let foreign_intent = other
        .store
        .record_team_definition_migration(&NewTeamDefinitionMigration {
            id: TeamDefinitionMigrationId::generate(),
            project_id: other.project_id,
            mini_project_id: other.mini_project_id,
            idempotency_key: IdempotencyKey::parse("foreign-migration").expect("a key"),
            from: Some(snapshot(&other.definition)),
            to: snapshot(&second),
            targets: vec![NewTeamDefinitionMigrationTarget {
                subject: TeamDefinitionMigrationSubject::Container {
                    topology_node_id: other.esw,
                },
                identity: identity("wks_other_root"),
                desired: placement("ESW • KBI-9001"),
            }],
            command_intent_hash: ContentHash::of(b"command-intent"),
            recorded_at: other.created_at,
        })
        .expect("the other project records its own intent");

    // The other project's store cannot reach this project's consultation, even
    // knowing its node id.
    assert!(
        other
            .store
            .supply_legacy_consultation_topic(
                other.project_id,
                node,
                foreign_intent.id,
                &name("Jira recovery"),
                at("2026-09-01T14:30:00Z"),
            )
            .is_err(),
        "a node id from another project must not resolve a consultation"
    );

    // And this project's consultation cannot cite another project's migration.
    assert!(
        m.fixture
            .store
            .supply_legacy_consultation_topic(
                m.fixture.project_id,
                node,
                foreign_intent.id,
                &name("Jira recovery"),
                at("2026-09-01T14:30:00Z"),
            )
            .is_err(),
        "provenance must name a migration of this project"
    );
}

#[test]
fn a_settled_migration_can_no_longer_supply_a_topic() {
    let (m, node, intent) = legacy_topic_fixture();
    m.fixture
        .store
        .fail_team_definition_migration(m.fixture.project_id, intent, at("2026-09-01T14:40:00Z"))
        .expect("an untouched migration may still be abandoned");
    assert!(
        m.fixture
            .store
            .supply_legacy_consultation_topic(
                m.fixture.project_id,
                node,
                intent,
                &name("Jira recovery"),
                at("2026-09-01T14:41:00Z"),
            )
            .is_err(),
        "a topic is supplied before the new pin becomes current, not after the \
         migration has settled"
    );
}

// ---------------------------------------------------------------------------
// The audited negatives
// ---------------------------------------------------------------------------

#[test]
fn publication_composes_against_the_exact_topology_bytes_and_rules() {
    let f = fixture();
    // A forged topology hash: locally valid, but not these bytes.
    let mut forged = f.definition.clone();
    forged.topology.canonical_hash = ContentHash::of(b"not the published topology");
    assert!(
        f.store
            .publish_team_definition(f.project_id, &forged, f.created_at)
            .is_err(),
        "a definition may not cite topology bytes that were never published"
    );

    // A capability policy the topology validator does not permit.
    let mut escalated = f.definition.clone();
    escalated.containers[0].read_only = !escalated.containers[0].read_only;
    assert!(
        f.store
            .publish_team_definition(f.project_id, &escalated, f.created_at)
            .is_err(),
        "publication must refuse a policy the topology does not legalize"
    );

    // The unmodified definition still publishes, so the refusals above are
    // about the mutations rather than about the fixture.
    f.store
        .publish_team_definition(f.project_id, &f.definition, f.created_at)
        .expect("the bundled definition composes against its validator");
}

#[test]
fn a_success_state_cannot_be_claimed_without_the_exact_desired_placement() {
    let m = migration_fixture();
    let recorded = m
        .fixture
        .store
        .record_team_definition_migration(&new_migration(&m, "migrate-readback-proof"))
        .expect("the intent is recorded");
    let subject = TeamDefinitionMigrationSubject::Container {
        topology_node_id: m.node,
    };
    let mut wrong_title = placement("ESW • KBI-8049");
    wrong_title.title = name("ESW • WRONG");

    for (observed, why) in [
        (None, "a success state with no readback at all"),
        (
            Some(wrong_title),
            "a success state carrying a different title",
        ),
    ] {
        assert!(
            m.fixture
                .store
                .observe_team_definition_migration(
                    m.fixture.project_id,
                    recorded.id,
                    &[TeamDefinitionMigrationObservation {
                        subject,
                        identity: m.native.clone(),
                        observed,
                        state: TeamDefinitionMigrationTargetState::Renamed,
                        observed_at: at("2026-09-01T15:00:00Z"),
                    }],
                    at("2026-09-01T15:00:00Z"),
                )
                .is_err(),
            "{why} must not be recorded as success"
        );
    }
}

#[test]
fn a_migration_cannot_enumerate_a_node_of_another_project_or_epic() {
    let m = migration_fixture();
    let other = fixture();
    assert!(
        m.fixture
            .store
            .record_team_definition_migration(&NewTeamDefinitionMigration {
                id: TeamDefinitionMigrationId::generate(),
                project_id: m.fixture.project_id,
                mini_project_id: m.fixture.mini_project_id,
                idempotency_key: IdempotencyKey::parse("migrate-foreign-node").expect("a key"),
                from: Some(snapshot(&m.fixture.definition)),
                to: snapshot(&m.second),
                targets: vec![NewTeamDefinitionMigrationTarget {
                    // A node that exists, but belongs to another project.
                    subject: TeamDefinitionMigrationSubject::Container {
                        topology_node_id: other.esw,
                    },
                    identity: identity("wks_foreign"),
                    desired: placement("ESW • KBI-8049"),
                }],
                command_intent_hash: ContentHash::of(b"command-intent"),
                recorded_at: at("2026-09-01T14:00:00Z"),
            })
            .is_err(),
        "a migration must not retitle another project's natives"
    );
}

#[test]
fn a_migration_with_runtime_effects_stays_resumable_instead_of_going_terminal() {
    let m = migration_fixture();
    let recorded = m
        .fixture
        .store
        .record_team_definition_migration(&new_migration(&m, "migrate-no-terminal-failure"))
        .expect("the intent is recorded");
    m.fixture
        .store
        .observe_team_definition_migration(
            m.fixture.project_id,
            recorded.id,
            &[TeamDefinitionMigrationObservation {
                subject: TeamDefinitionMigrationSubject::Container {
                    topology_node_id: m.node,
                },
                identity: m.native.clone(),
                observed: None,
                state: TeamDefinitionMigrationTargetState::RenamePending,
                observed_at: at("2026-09-01T15:00:00Z"),
            }],
            at("2026-09-01T15:00:00Z"),
        )
        .expect("one target has been asked to rename");
    assert!(
        m.fixture
            .store
            .fail_team_definition_migration(
                m.fixture.project_id,
                recorded.id,
                at("2026-09-01T15:01:00Z")
            )
            .is_err(),
        "abandoning after a runtime effect would drop the fence while part of \
         the runtime already renders the new titles"
    );
    assert!(
        m.fixture
            .store
            .get_in_flight_team_definition_migration(
                m.fixture.project_id,
                m.fixture.mini_project_id
            )
            .expect("the read succeeds")
            .is_some(),
        "the epic stays fenced until the migration is resolved coherently"
    );
}

#[test]
fn one_key_cannot_stand_for_two_different_migrations() {
    let m = migration_fixture();
    m.fixture
        .store
        .record_team_definition_migration(&new_migration(&m, "migrate-fingerprint"))
        .expect("the intent is recorded");
    let mut different = new_migration(&m, "migrate-fingerprint");
    different.id = TeamDefinitionMigrationId::generate();
    different.targets[0].desired.title = name("ESW • SOMETHING ELSE");
    assert!(
        m.fixture
            .store
            .record_team_definition_migration(&different)
            .is_err(),
        "a key reused for a different request is a conflict, not a replay"
    );
}

#[test]
fn a_stale_selection_preview_cannot_overwrite_a_newer_choice() {
    let f = fixture();
    f.store
        .publish_team_definition(f.project_id, &f.definition, f.created_at)
        .expect("v1 publishes");
    let second = next_revision(&f.definition, 2);
    f.store
        .publish_team_definition(f.project_id, &second, f.created_at)
        .expect("v2 publishes");

    // Someone previews "there is no default yet", then an explicit selection
    // lands before they apply.
    f.store
        .set_project_team_definition_default(&ProjectTeamDefinitionDefault {
            project_id: f.project_id,
            expected: None,
            definition: snapshot(&second),
            selected_at: at("2026-09-01T13:00:00Z"),
        })
        .expect("the explicit selection lands first");
    assert!(
        f.store
            .set_project_team_definition_default(&ProjectTeamDefinitionDefault {
                project_id: f.project_id,
                expected: None,
                definition: snapshot(&f.definition),
                selected_at: at("2026-09-01T13:01:00Z"),
            })
            .is_err(),
        "a bootstrap that observed no default must not overwrite the choice \
         made after it looked"
    );
    assert_eq!(
        f.store
            .get_project_team_definition_default(f.project_id)
            .expect("the read succeeds")
            .expect("a default is selected")
            .definition
            .version,
        second.version
    );
}
