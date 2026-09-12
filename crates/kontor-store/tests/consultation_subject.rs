//! Schema v94: the advised or debated subject of one consultation is durable.
//!
//! A consultation is invoked about one ticket or about the epic as a whole, and
//! its ASW/CSW container has to render that subject's confirmed Jira key. The
//! subject cannot live on `topology_nodes.task_id`: `ux_topology_node_task`
//! reserves that column for the one active delivery workspace per task, so a
//! consultation about a ticket would collide with the ticket's own TSW. It is
//! therefore frozen beside the run.
//!
//! The mutants this suite exists to kill:
//!
//! * discarding the requested ticket and rendering the containing epic;
//! * letting a consultation claim the delivery workspace's node task;
//! * moving a frozen subject after invocation;
//! * losing the subject across a restart or a backup/restore;
//! * inventing `epic` for a historical run whose subject was never recorded.

use kontor_core::consultation::{
    ConsultationFamily, ConsultationRunId, ConsultationRunState, ConsultationSubject,
};
use kontor_core::id::{
    AdvisorRunId, AggregateRevision, BoundedText, CanonicalDocument, ContentHash, ExternalName,
    IdempotencyKey, MiniProjectId, ProjectId, RoleCode, RoleSlotId, SeatBindingId, SpecVersion,
    TaskId, Timestamp, TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::repository::{
    MiniProjectTopologySnapshot, NewMiniProject, NewProject, NewSeatBinding,
    NewSessionTopologyNode, NewTask, ProjectRepository, StoredConsultationProfileRevision,
    StoredConsultationRun, TopologyRepository,
};
use kontor_core::spec::{CatalogRoleRef, Shareability, ShareabilityTier, TopologySnapshot};
use kontor_core::state::TaskState;
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use tempfile::TempDir;

const PROFILE: &str = "01991c00-0000-7000-8000-00000000009c";

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn stamp() -> Shareability {
    Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("tier B classifies")
}

struct World {
    home: TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    task_id: TaskId,
    /// A task in a *different* epic of the same project.
    sibling_task_id: TaskId,
    /// A task in a different project entirely.
    foreign_task_id: TaskId,
    topology: TopologySnapshot,
    esw: TopologyNodeId,
    caller: SeatBindingId,
    created_at: Timestamp,
}

/// One epic holding a task whose delivery workspace already owns the node-level
/// task, so every consultation below has to coexist with it.
fn world() -> World {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    let mini_project_id = MiniProjectId::generate();
    let created_at = at("2026-09-06T12:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Subject project"),
            root_path: name("/tmp/subject-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_mini_project(&NewMiniProject {
            id: mini_project_id,
            project_id,
            name: name("Subject epic"),
            created_at,
        })
        .expect("the epic is created");
    let task_id = TaskId::generate();
    store
        .create_task(&NewTask {
            id: task_id,
            project_id,
            mini_project_id: Some(mini_project_id),
            title: name("Implement the tokens"),
            module: None,
            state: TaskState::Ready,
            created_at,
        })
        .expect("the task is created");

    // A sibling epic in the same project, holding its own task. Its ticket is
    // reachable by id and satisfies the column's foreign key, so it is exactly
    // what a containment rule has to refuse.
    let sibling_epic_id = MiniProjectId::generate();
    store
        .create_mini_project(&NewMiniProject {
            id: sibling_epic_id,
            project_id,
            name: name("Sibling epic"),
            created_at,
        })
        .expect("the sibling epic is created");
    let sibling_task_id = TaskId::generate();
    store
        .create_task(&NewTask {
            id: sibling_task_id,
            project_id,
            mini_project_id: Some(sibling_epic_id),
            title: name("A sibling epic's task"),
            module: None,
            state: TaskState::Ready,
            created_at,
        })
        .expect("the sibling task is created");

    // And a task in another project entirely.
    let foreign_project_id = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: foreign_project_id,
            name: name("Foreign project"),
            root_path: name("/tmp/foreign-project"),
            created_at,
        })
        .expect("the foreign project is created");
    let foreign_epic_id = MiniProjectId::generate();
    store
        .create_mini_project(&NewMiniProject {
            id: foreign_epic_id,
            project_id: foreign_project_id,
            name: name("Foreign epic"),
            created_at,
        })
        .expect("the foreign epic is created");
    let foreign_task_id = TaskId::generate();
    store
        .create_task(&NewTask {
            id: foreign_task_id,
            project_id: foreign_project_id,
            mini_project_id: Some(foreign_epic_id),
            title: name("Another project's task"),
            module: None,
            state: TaskState::Ready,
            created_at,
        })
        .expect("the foreign task is created");

    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let topology_spec = domain.topology_specs.first().expect("a topology").clone();
    let catalog = domain.role_catalogs.first().expect("a catalog").clone();
    let canonical_hash = store
        .publish_topology_spec(project_id, &topology_spec, &stamp(), created_at)
        .expect("the topology publishes");
    store
        .publish_role_catalog(&catalog, &stamp(), created_at)
        .expect("the catalog publishes");
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

    let node = |id, kind: &str, parent, epic, task| NewSessionTopologyNode {
        id,
        project_id,
        mini_project_id: epic,
        topology: topology.clone(),
        kind: TopologyKindKey::parse(kind).expect("a kind"),
        parent_id: parent,
        task_id: task,
        created_at,
    };
    let root = TopologyNodeId::generate();
    store
        .create_topology_node(&node(root, "PSW", None, None, None))
        .expect("the project root is created");
    let esw = TopologyNodeId::generate();
    store
        .create_topology_node(&node(esw, "ESW", Some(root), Some(mini_project_id), None))
        .expect("the epic node is created");
    let ecp = TopologyNodeId::generate();
    store
        .create_topology_node(&node(ecp, "ECP", Some(esw), Some(mini_project_id), None))
        .expect("the ECP node is created");
    // The delivery workspace holds the node-level task. Every consultation
    // about that same task has to coexist with this row.
    store
        .create_topology_node(&node(
            TopologyNodeId::generate(),
            "TSW",
            Some(esw),
            Some(mini_project_id),
            Some(task_id),
        ))
        .expect("the delivery workspace is created");

    let entry = catalog
        .role(&RoleCode::parse("LSA").expect("a role code"))
        .expect("the catalog has LSA");
    let caller = SeatBindingId::generate();
    store
        .create_seat_binding(&NewSeatBinding {
            id: caller,
            project_id,
            topology_node_id: ecp,
            role_slot_id: RoleSlotId::parse("epic.lsa").expect("a slot"),
            role: CatalogRoleRef {
                catalog_id: catalog.catalog_id,
                catalog_revision: catalog.version,
                role_code: entry.role_code.clone(),
                standard_title: entry.standard_title.clone(),
                custom_display_name: None,
            },
            task_id: None,
            team_run_id: None,
            attach_deadline: at("2026-09-06T12:10:00Z"),
            parent_seat_binding_id: None,
            created_at,
        })
        .expect("the caller seat is bound");

    let profile = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "seats": ["SA"],
    }))
    .expect("a canonical profile");
    store
        .publish_consultation_profile_revision(&StoredConsultationProfileRevision {
            project_id,
            family: ConsultationFamily::Advisor,
            profile_id: PROFILE.to_owned(),
            version: SpecVersion::FIRST,
            name: name("Subject advisor"),
            definition: profile.json().to_owned(),
            definition_hash: profile.hash().clone(),
            published_at: created_at,
        })
        .expect("the advisor profile publishes");

    World {
        home,
        store,
        project_id,
        mini_project_id,
        task_id,
        sibling_task_id,
        foreign_task_id,
        topology,
        esw,
        caller,
        created_at,
    }
}

/// Invoke one Advisor consultation about `subject`, returning its run and node.
fn consult(
    world: &World,
    key: &str,
    subject: Option<ConsultationSubject>,
) -> (StoredConsultationRun, TopologyNodeId) {
    let asw = TopologyNodeId::generate();
    let question = BoundedText::parse("Which confirmed key names this?").expect("a question");
    let context = serde_json::json!({ "schema_version": 1 });
    let context_hash = CanonicalDocument::from_serializable(&context)
        .expect("canonical context")
        .hash()
        .clone();
    let run = StoredConsultationRun {
        id: ConsultationRunId::Advisor(AdvisorRunId::generate()),
        project_id: world.project_id,
        mini_project_id: world.mini_project_id,
        topic: Some(name("Naming review")),
        profile_id: PROFILE.to_owned(),
        profile_version: SpecVersion::FIRST,
        definition_hash: ContentHash::of(b"subject-advisor"),
        semantic_identity_hash: Some(ContentHash::of(key.as_bytes())),
        subject,
        question_hash: ContentHash::of(question.as_str().as_bytes()),
        question,
        context,
        context_hash,
        caller_seat_binding_id: world.caller,
        topology_node_id: asw,
        invoke_key: IdempotencyKey::parse(key).expect("a key"),
        invoke_intent_hash: ContentHash::of(key.as_bytes()),
        state: ConsultationRunState::Materializing,
        round: 1,
        result: None,
        result_hash: None,
        revision: AggregateRevision::INITIAL,
        created_at: world.created_at,
        updated_at: world.created_at,
        settled_at: None,
    };
    world
        .store
        .create_consultation_run(
            &run,
            &NewSessionTopologyNode {
                id: asw,
                project_id: world.project_id,
                mini_project_id: Some(world.mini_project_id),
                topology: world.topology.clone(),
                kind: TopologyKindKey::parse("ASW").expect("the advisor kind"),
                parent_id: Some(world.esw),
                // Reserved for the delivery workspace; the subject is on the run.
                task_id: None,
                created_at: world.created_at,
            },
            &[],
        )
        .expect("the consultation run is durable");
    (run, asw)
}

/// Attempt one consultation, returning the store's answer rather than panicking.
fn try_consult(
    world: &World,
    key: &str,
    subject: Option<ConsultationSubject>,
) -> Result<(), kontor_core::repository::RepositoryError> {
    let asw = TopologyNodeId::generate();
    let question = BoundedText::parse("Which confirmed key names this?").expect("a question");
    let context = serde_json::json!({ "schema_version": 1 });
    let context_hash = CanonicalDocument::from_serializable(&context)
        .expect("canonical context")
        .hash()
        .clone();
    let run = StoredConsultationRun {
        id: ConsultationRunId::Advisor(AdvisorRunId::generate()),
        project_id: world.project_id,
        mini_project_id: world.mini_project_id,
        topic: Some(name("Naming review")),
        profile_id: PROFILE.to_owned(),
        profile_version: SpecVersion::FIRST,
        definition_hash: ContentHash::of(b"subject-advisor"),
        semantic_identity_hash: Some(ContentHash::of(key.as_bytes())),
        subject,
        question_hash: ContentHash::of(question.as_str().as_bytes()),
        question,
        context,
        context_hash,
        caller_seat_binding_id: world.caller,
        topology_node_id: asw,
        invoke_key: IdempotencyKey::parse(key).expect("a key"),
        invoke_intent_hash: ContentHash::of(key.as_bytes()),
        state: ConsultationRunState::Materializing,
        round: 1,
        result: None,
        result_hash: None,
        revision: AggregateRevision::INITIAL,
        created_at: world.created_at,
        updated_at: world.created_at,
        settled_at: None,
    };
    world.store.create_consultation_run(
        &run,
        &NewSessionTopologyNode {
            id: asw,
            project_id: world.project_id,
            mini_project_id: Some(world.mini_project_id),
            topology: world.topology.clone(),
            kind: TopologyKindKey::parse("ASW").expect("the advisor kind"),
            parent_id: Some(world.esw),
            task_id: None,
            created_at: world.created_at,
        },
        &[],
    )
}

#[test]
fn a_subject_from_another_project_is_refused() {
    let world = world();

    // The ticket exists, so the column's own foreign key is satisfied. It
    // belongs to another project, so it is not a subject this epic may name.
    let refused = try_consult(
        &world,
        "subject-cross-project",
        Some(ConsultationSubject::Task(world.foreign_task_id)),
    )
    .expect_err("a cross-project subject must be refused");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                subject: "consultation subject",
                ..
            }
        ),
        "unexpected refusal: {refused}"
    );
}

#[test]
fn a_subject_from_a_sibling_epic_in_the_same_project_is_refused() {
    let world = world();

    // Same project, same caller authority, different epic. The containing epic
    // is what bounds a consultation's subject, not the project.
    let refused = try_consult(
        &world,
        "subject-sibling-epic",
        Some(ConsultationSubject::Task(world.sibling_task_id)),
    )
    .expect_err("a sibling-epic subject must be refused");
    assert!(
        matches!(
            refused,
            kontor_core::repository::RepositoryError::Conflict {
                subject: "consultation subject",
                ..
            }
        ),
        "unexpected refusal: {refused}"
    );
}

#[test]
fn storage_refuses_an_uncontained_subject_even_without_the_repository_check() {
    let world = world();
    consult(
        &world,
        "subject-storage-guard",
        Some(ConsultationSubject::Epic),
    );
    let connection = rusqlite::Connection::open(world.home.path().join("kontor.db"))
        .expect("the database reopens");
    // The frozen-inputs trigger would refuse any subject change on its own, so
    // leaving it in place would let this test pass without containment being
    // enforced at all. Removing it is what makes the assertion below mean what
    // it says.
    connection
        .execute("DROP TRIGGER consultation_run_inputs_are_frozen", [])
        .expect("the fixture isolates the containment rule");

    // Written straight at the table, past the repository's own validation, to
    // prove the containment rule is in storage and not only in Rust.
    for (label, task_id) in [
        ("another project", world.foreign_task_id),
        ("a sibling epic", world.sibling_task_id),
    ] {
        let written = connection.execute(
            "UPDATE consultation_runs SET subject_kind = 'task', subject_task_id = ?1",
            rusqlite::params![task_id.to_string()],
        );
        assert!(
            written.is_err(),
            "storage must refuse a subject from {label}",
        );
    }

    // The contained ticket is still accepted, so the rule rejects by
    // containment rather than by refusing every task subject.
    connection
        .execute(
            "UPDATE consultation_runs SET subject_kind = 'task', subject_task_id = ?1",
            rusqlite::params![world.task_id.to_string()],
        )
        .expect("this epic's own task remains a valid subject");
}

/// The repository's one scope-relationship check now covers the subject too.
#[test]
fn the_run_node_and_subject_must_all_describe_one_scope() {
    let world = world();

    // The pre-existing halves of the relationship still hold.
    assert!(
        try_consult(&world, "subject-scope-ok", Some(ConsultationSubject::Epic)).is_ok(),
        "a well-formed epic-scoped run is admitted"
    );

    // And the subject is now part of the same relationship: same project, same
    // containing epic, or it is not this consultation's subject.
    for (label, task_id) in [
        ("another project", world.foreign_task_id),
        ("a sibling epic", world.sibling_task_id),
    ] {
        let refused = try_consult(
            &world,
            &format!("subject-scope-{}", label.replace(' ', "-")),
            Some(ConsultationSubject::Task(task_id)),
        )
        .expect_err("an uncontained subject breaks the scope relationship");
        assert!(
            matches!(
                refused,
                kontor_core::repository::RepositoryError::Conflict {
                    subject: "consultation subject",
                    ..
                }
            ),
            "{label}: unexpected refusal: {refused}"
        );
    }

    // The epic's own task completes the relationship.
    assert!(
        try_consult(
            &world,
            "subject-scope-contained",
            Some(ConsultationSubject::Task(world.task_id))
        )
        .is_ok(),
        "the containing epic's own task is a valid subject"
    );
}

#[test]
fn an_epic_and_a_task_subject_both_round_trip_through_the_store() {
    let world = world();
    let (epic_run, _) = consult(&world, "subject-epic", Some(ConsultationSubject::Epic));
    let (task_run, _) = consult(
        &world,
        "subject-task",
        Some(ConsultationSubject::Task(world.task_id)),
    );

    let read = |run: &StoredConsultationRun| {
        world
            .store
            .get_consultation_run(world.project_id, run.id)
            .expect("the run reads")
            .expect("the run exists")
            .subject
    };
    assert_eq!(read(&epic_run), Some(ConsultationSubject::Epic));
    assert_eq!(
        read(&task_run),
        Some(ConsultationSubject::Task(world.task_id)),
        "the exact advised ticket survives the write, not just the fact of one",
    );
}

#[test]
fn a_consultation_about_a_task_never_claims_the_delivery_workspaces_node() {
    let world = world();
    let (run, asw) = consult(
        &world,
        "subject-coexist",
        Some(ConsultationSubject::Task(world.task_id)),
    );

    // The one-active-node-per-task invariant is untouched: the TSW keeps the
    // node task, and the ASW about the same task holds none.
    let node = world
        .store
        .get_topology_node(world.project_id, asw)
        .expect("the ASW reads")
        .expect("the ASW exists");
    assert_eq!(node.task_id, None);
    let delivery: Vec<_> = world
        .store
        .list_topology_nodes(world.project_id, Some(world.mini_project_id))
        .expect("the topology reads")
        .into_iter()
        .filter(|node| node.task_id == Some(world.task_id))
        .collect();
    assert_eq!(delivery.len(), 1, "exactly one node may own a task");
    assert_eq!(delivery[0].kind.as_str(), "TSW");

    // And the subject is still exactly recoverable from the run beside it.
    assert_eq!(
        world
            .store
            .get_consultation_run_by_topology_node(world.project_id, asw)
            .expect("the run reads")
            .expect("the node has a run")
            .subject,
        Some(ConsultationSubject::Task(world.task_id)),
    );
    assert_eq!(run.subject, Some(ConsultationSubject::Task(world.task_id)));
}

#[test]
fn a_frozen_subject_cannot_be_moved_after_invocation() {
    let world = world();
    let (run, _) = consult(
        &world,
        "subject-frozen",
        Some(ConsultationSubject::Task(world.task_id)),
    );

    // A consultation's subject is semantic input. Nothing may retarget it
    // later, and nothing may record one onto a run that never had it.
    let database = world.home.path().join("kontor.db");
    let connection = rusqlite::Connection::open(&database).expect("the database reopens");
    for (kind, task) in [
        ("epic", None),
        ("task", Some(TaskId::generate().to_string())),
    ] {
        let moved = connection.execute(
            "UPDATE consultation_runs SET subject_kind = ?1, subject_task_id = ?2
             WHERE run_id = ?3",
            rusqlite::params![kind, task, run.id.as_text()],
        );
        assert!(
            moved.is_err(),
            "a frozen subject must not move to {kind}/{task:?}"
        );
    }
    assert_eq!(
        world
            .store
            .get_consultation_run(world.project_id, run.id)
            .expect("the run reads")
            .expect("the run exists")
            .subject,
        Some(ConsultationSubject::Task(world.task_id)),
    );
}

/// Every subject state a realm can hold, and the identities they hang off.
fn recorded_subjects(world: &World) -> Vec<(ConsultationRunId, TopologyNodeId, &'static str)> {
    let (task_run, task_node) = consult(
        world,
        "subject-task-state",
        Some(ConsultationSubject::Task(world.task_id)),
    );
    let (epic_run, epic_node) =
        consult(world, "subject-epic-state", Some(ConsultationSubject::Epic));
    // A run written before v94 recorded a subject at all.
    let (legacy_run, legacy_node) = consult(world, "subject-legacy-state", None);
    vec![
        (task_run.id, task_node, "task"),
        (epic_run.id, epic_node, "epic"),
        (legacy_run.id, legacy_node, "absent"),
    ]
}

/// Assert one reopened store still holds every subject against the exact run
/// and topology identities it was written with.
fn assert_subjects_intact(
    store: &SqliteStore,
    world: &World,
    recorded: &[(ConsultationRunId, TopologyNodeId, &'static str)],
) {
    for (run_id, node_id, state) in recorded {
        let run = store
            .get_consultation_run(world.project_id, *run_id)
            .expect("the run reads")
            .unwrap_or_else(|| panic!("the {state} run survived under its own id"));
        assert_eq!(
            run.topology_node_id, *node_id,
            "the {state} run kept the exact topology node it was invoked on",
        );
        let expected = match *state {
            "task" => Some(ConsultationSubject::Task(world.task_id)),
            "epic" => Some(ConsultationSubject::Epic),
            _ => None,
        };
        assert_eq!(
            run.subject, expected,
            "the {state} subject survived exactly"
        );

        // Reachable from the node too: this is the direction the renderer uses.
        let by_node = store
            .get_consultation_run_by_topology_node(world.project_id, *node_id)
            .expect("the run reads by node")
            .unwrap_or_else(|| panic!("the {state} node still resolves to its run"));
        assert_eq!(by_node.id, *run_id);
        assert_eq!(by_node.subject, expected);

        // And a consultation still never holds the delivery workspace's task.
        assert_eq!(
            store
                .get_topology_node(world.project_id, *node_id)
                .expect("the node reads")
                .expect("the node survived")
                .task_id,
            None,
        );
    }

    // The delivery workspace is still the sole owner of the node-level task.
    let delivery: Vec<_> = store
        .list_topology_nodes(world.project_id, Some(world.mini_project_id))
        .expect("the topology reads")
        .into_iter()
        .filter(|node| node.task_id == Some(world.task_id))
        .collect();
    assert_eq!(delivery.len(), 1);
    assert_eq!(delivery[0].kind.as_str(), "TSW");
}

#[test]
fn every_subject_state_survives_a_restart_under_its_own_identity() {
    let mut world = world();
    let recorded = recorded_subjects(&world);
    let database = world.home.path().join("kontor.db");
    // Close the writer exactly as a daemon shutdown would, then reopen.
    world.store = SqliteStore::open(&database).expect("the store reopens");

    // A fresh handle over the same file. The subject is durable state, not
    // something held in memory between invocation and rendering.
    assert_subjects_intact(&world.store, &world, &recorded);
}

#[test]
fn every_subject_state_survives_a_backup_and_restore_under_its_own_identity() {
    let world = world();
    let recorded = recorded_subjects(&world);

    let backups = world.home.path().join("backups");
    std::fs::create_dir_all(&backups).expect("a backup directory");
    let taken = kontor_store::backup::create_snapshot(
        &world.home.path().join("kontor.db"),
        &backups,
        at("2026-09-06T13:00:00Z"),
    )
    .expect("the snapshot is taken and verified");
    assert_eq!(
        taken.manifest.database_schema_version,
        kontor_store::SCHEMA_VERSION,
        "the snapshot carries the schema that recorded the subject",
    );

    // Recovery is the case that matters most: a realm restored from a backup
    // must still know which ticket each consultation was about, against the
    // same run and topology ids the originals carried.
    let restored = SqliteStore::open(&taken.snapshot).expect("the snapshot opens as a store");
    assert_subjects_intact(&restored, &world, &recorded);
}

#[test]
fn a_run_recorded_before_the_subject_existed_reads_back_absent() {
    let world = world();
    let (run, _) = consult(&world, "subject-historical", None);

    // No backfill invents `epic` for it. The absence is the honest state, and
    // it is what the renderer fails closed on.
    assert_eq!(
        world
            .store
            .get_consultation_run(world.project_id, run.id)
            .expect("the run reads")
            .expect("the run exists")
            .subject,
        None,
    );
}

#[test]
fn an_impossible_stored_pairing_is_refused_rather_than_guessed() {
    let task_id = TaskId::generate();
    assert_eq!(
        ConsultationSubject::from_stored(None, None).expect("an unrecorded subject"),
        None
    );
    assert_eq!(
        ConsultationSubject::from_stored(Some("epic"), None).expect("an epic subject"),
        Some(ConsultationSubject::Epic)
    );
    assert_eq!(
        ConsultationSubject::from_stored(Some("task"), Some(task_id)).expect("a task subject"),
        Some(ConsultationSubject::Task(task_id))
    );
    for (kind, task) in [
        (Some("task"), None),
        (Some("epic"), Some(task_id)),
        (None, Some(task_id)),
        (Some("ticket"), Some(task_id)),
    ] {
        assert!(
            ConsultationSubject::from_stored(kind, task).is_err(),
            "{kind:?}/{task:?} is not a subject this store may report",
        );
    }
}

#[test]
fn the_database_refuses_every_impossible_subject_pairing() {
    let world = world();
    let (run, _) = consult(&world, "subject-check", Some(ConsultationSubject::Epic));
    let database = world.home.path().join("kontor.db");
    let connection = rusqlite::Connection::open(&database).expect("the database reopens");
    // The frozen-input trigger governs *when* a subject may be written. This
    // asserts the column constraint that governs *what* may be written at all,
    // so it is removed here to isolate the one from the other.
    connection
        .execute("DROP TRIGGER consultation_run_inputs_are_frozen", [])
        .expect("the fixture isolates the column constraint");

    let task = world.task_id.to_string();
    let set = |kind: Option<&str>, subject_task: Option<&str>| {
        connection.execute(
            "UPDATE consultation_runs SET subject_kind = ?1, subject_task_id = ?2
             WHERE run_id = ?3",
            rusqlite::params![kind, subject_task, run.id.as_text()],
        )
    };

    // SQLite counts a CHECK that evaluates to NULL as a pass, so a constraint
    // written with `=` against these nullable columns would silently store the
    // rows it appears to forbid. `(NULL, <task>)` is the exact pairing that
    // slips through such a constraint: a ticket recorded against a run whose
    // subject is supposedly unrecorded, which the reader would then refuse to
    // interpret. Every impossible pairing has to fail at the column.
    for (kind, subject_task) in [
        (None, Some(task.as_str())),
        (Some("epic"), Some(task.as_str())),
        (Some("task"), None),
        (Some("ticket"), Some(task.as_str())),
        (Some("ticket"), None),
        (Some(""), None),
    ] {
        assert!(
            set(kind, subject_task).is_err(),
            "the schema must refuse subject_kind={kind:?} with subject_task_id={subject_task:?}",
        );
    }

    // And exactly the three legal pairings are storable.
    for (kind, subject_task) in [
        (None, None),
        (Some("epic"), None),
        (Some("task"), Some(task.as_str())),
    ] {
        assert!(
            set(kind, subject_task).is_ok(),
            "the schema must admit subject_kind={kind:?} with subject_task_id={subject_task:?}",
        );
    }

    // The reader agrees with the column: what survived is the task subject.
    assert_eq!(
        world
            .store
            .get_consultation_run(world.project_id, run.id)
            .expect("the run reads")
            .expect("the run exists")
            .subject,
        Some(ConsultationSubject::Task(world.task_id)),
    );
}

/// Supplemental boundary evidence, not the round-trip proof: the authoritative
/// recovery guarantees are the restart and backup/restore round-trips above.
/// The redacted *hand-out* document is a different artifact with a different
/// job, and consultation runs are deliberately not one of its shapes.
#[test]
fn the_redacted_export_carries_no_consultation_subject() {
    let world = world();
    let (run, _node) = consult(
        &world,
        "subject-export",
        Some(ConsultationSubject::Task(world.task_id)),
    );

    // Consultation runs are not an exported shape, so the ticket a realm was
    // advised about cannot leave in the redacted document. The export declares
    // typed columns rather than dumping tables, so this stays true only while
    // nobody adds them — which is exactly what this asserts.
    let document = kontor_store::backup::export_realm(&world.store, at("2026-09-06T14:00:00Z"))
        .expect("the realm exports");
    let bytes = document
        .canonical_bytes()
        .expect("the export document canonicalizes");
    let text = String::from_utf8(bytes).expect("the export is UTF-8");
    // The task itself is an exported shape, so its id legitimately appears.
    // What must not appear is the consultation, or the columns that say which
    // ticket it was about.
    for leaked in [
        "subject_kind".to_owned(),
        "subject_task_id".to_owned(),
        run.id.as_text().to_owned(),
    ] {
        assert!(
            !text.contains(&leaked),
            "the export must not carry `{leaked}`",
        );
    }
}
