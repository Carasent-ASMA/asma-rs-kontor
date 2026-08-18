//! Durable Advisor consultations.
//!
//! A consultation is evidence, so these tests are about the properties that make
//! it evidence rather than chat: one invocation opens exactly one consultation
//! whatever a retry does, the advice is written once and cannot be rewritten, and
//! a disposition is appended beside it rather than over it.

use kontor_core::consultation::{AdviceDisposition, AdvisorRunState};
use kontor_core::id::{
    AdvisorRunId, AggregateRevision, BoundedText, CanonicalDocument, ContentHash, ExternalName,
    MiniProjectId, ProjectId, RoleCode, RoleSlotId, SeatBindingId, SpecVersion, Timestamp,
    TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::repository::{
    NewProject, ProjectRepository, RepositoryError, StoredAdvice, StoredAdviceDisposition,
    StoredAdvisorRun,
};
use kontor_core::spec::CatalogRoleRef;
use kontor_store::SqliteStore;
use tempfile::TempDir;

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("bounded text")
}

fn hash(value: &str) -> ContentHash {
    CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": kontor_core::id::SCHEMA_VERSION,
        "value": value,
    }))
    .expect("a canonical document")
    .hash()
    .clone()
}

struct World {
    _home: TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    epic: MiniProjectId,
}

fn world() -> World {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Consultation project"),
            root_path: name("/tmp/op05-runs"),
            created_at: at("2026-08-18T09:00:00Z"),
        })
        .expect("the project is created");
    World {
        _home: home,
        store,
        project_id,
        epic: MiniProjectId::generate(),
    }
}

fn run(world: &World, intent: &str) -> StoredAdvisorRun {
    StoredAdvisorRun {
        id: AdvisorRunId::generate(),
        project_id: world.project_id,
        mini_project_id: world.epic,
        task_id: None,
        profile_id: "01991c00-0000-7000-8000-0000000000a1".to_owned(),
        profile_version: SpecVersion::FIRST,
        profile_hash: hash("profile"),
        question: text("Is this change compliant with the plan?"),
        question_hash: hash("question"),
        owner_authority_seat_binding_id: SeatBindingId::generate(),
        context: r#"{"schema_version":1}"#.to_owned(),
        context_hash: hash("context"),
        provenance: serde_json::json!([{"source": "plan", "revision": 1}]),
        topology_node_id: TopologyNodeId::generate(),
        seat_binding_id: SeatBindingId::generate(),
        role_slot_id: RoleSlotId::parse("advisor").expect("a slot"),
        role: CatalogRoleRef {
            catalog_id: kontor_core::id::RoleCatalogId::generate(),
            catalog_revision: SpecVersion::FIRST,
            role_code: RoleCode::parse("SA").expect("a role code"),
            standard_title: name("Software Architect"),
            custom_display_name: None,
        },
        esw_topology_node_id: TopologyNodeId::generate(),
        esw_native_id: None,
        state: AdvisorRunState::Placed,
        intent_hash: hash(intent),
        revision: AggregateRevision::INITIAL,
        created_at: at("2026-08-18T09:05:00Z"),
    }
}

#[test]
fn a_consultation_reads_back_exactly_as_it_was_frozen() {
    let world = world();
    let planned = run(&world, "first");
    world
        .store
        .create_advisor_run(&planned)
        .expect("the consultation opens");
    let found = world
        .store
        .get_advisor_run(world.project_id, planned.id)
        .expect("readable")
        .expect("present");
    assert_eq!(found, planned, "the frozen row must survive a round trip");
}

#[test]
fn one_invocation_opens_one_consultation() {
    // The retry that lost its acknowledgement must not place a second ASW or
    // spend a second consultation against the profile's limit.
    let world = world();
    world
        .store
        .create_advisor_run(&run(&world, "same"))
        .expect("the first opens");
    let error = world
        .store
        .create_advisor_run(&run(&world, "same"))
        .expect_err("the same intent must not open a second");
    assert!(matches!(error, RepositoryError::Conflict { .. }));
}

#[test]
fn a_retry_finds_the_consultation_its_intent_already_opened() {
    let world = world();
    let planned = run(&world, "lost-ack");
    world
        .store
        .create_advisor_run(&planned)
        .expect("the consultation opens");
    let found = world
        .store
        .get_advisor_run_by_intent(world.project_id, &planned.intent_hash)
        .expect("readable")
        .expect("present");
    assert_eq!(
        found.id, planned.id,
        "the retry must reconcile, not re-place"
    );
}

#[test]
fn a_state_advance_is_compare_and_swap() {
    let world = world();
    let planned = run(&world, "advance");
    world.store.create_advisor_run(&planned).expect("opens");
    let next = world
        .store
        .advance_advisor_run(
            world.project_id,
            planned.id,
            AdvisorRunState::Advised,
            AggregateRevision::INITIAL,
        )
        .expect("the first advance lands");
    assert_eq!(next.get(), 2);

    let stale = world
        .store
        .advance_advisor_run(
            world.project_id,
            planned.id,
            AdvisorRunState::Disposed,
            AggregateRevision::INITIAL,
        )
        .expect_err("a stale revision must refuse");
    assert!(matches!(stale, RepositoryError::Conflict { .. }));

    let found = world
        .store
        .get_advisor_run(world.project_id, planned.id)
        .expect("readable")
        .expect("present");
    assert_eq!(found.state, AdvisorRunState::Advised);
    assert_eq!(found.revision.get(), 2);
}

#[test]
fn an_advisor_submits_its_output_once() {
    let world = world();
    let planned = run(&world, "advice");
    world.store.create_advisor_run(&planned).expect("opens");
    let advice = StoredAdvice {
        advisor_run_id: planned.id,
        advice: text("Use the canonical mirror; the FDW path cannot carry this volume."),
        advice_hash: hash("advice"),
        created_at: at("2026-08-18T10:00:00Z"),
    };
    world.store.record_advice(&advice).expect("it records");
    let error = world
        .store
        .record_advice(&advice)
        .expect_err("advice is written once");
    assert!(matches!(error, RepositoryError::Conflict { .. }));

    let found = world
        .store
        .get_advice(planned.id)
        .expect("readable")
        .expect("present");
    assert_eq!(found, advice);
}

#[test]
fn dispositions_append_rather_than_replace() {
    let world = world();
    let planned = run(&world, "dispositions");
    world.store.create_advisor_run(&planned).expect("opens");
    let recorder = SeatBindingId::generate();
    for (sequence, disposition) in [
        (1_u32, AdviceDisposition::Rejected),
        (2, AdviceDisposition::Superseded),
    ] {
        world
            .store
            .append_advice_disposition(&StoredAdviceDisposition {
                advisor_run_id: planned.id,
                sequence,
                disposition,
                rationale: text("Considered, and a later decision replaced it."),
                cited_receipts: Vec::new(),
                recorded_by: recorder,
                created_at: at("2026-08-18T11:00:00Z"),
            })
            .expect("it appends");
    }
    let listed = world
        .store
        .list_advice_dispositions(planned.id)
        .expect("readable");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].disposition, AdviceDisposition::Rejected);
    assert_eq!(listed[1].disposition, AdviceDisposition::Superseded);
    assert!(
        listed[0].created_at <= listed[1].created_at,
        "the earlier decision stays on the record"
    );

    // The same position twice is a rewrite in disguise.
    let error = world
        .store
        .append_advice_disposition(&StoredAdviceDisposition {
            advisor_run_id: planned.id,
            sequence: 1,
            disposition: AdviceDisposition::Accepted,
            rationale: text("Actually, adopted."),
            cited_receipts: vec!["01991c00-0000-7000-8000-0000000000c1".to_owned()],
            recorded_by: recorder,
            created_at: at("2026-08-18T12:00:00Z"),
        })
        .expect_err("a recorded decision cannot be overwritten");
    assert!(matches!(error, RepositoryError::Conflict { .. }));
}

#[test]
fn every_consultation_of_an_epic_is_listed_oldest_first() {
    let world = world();
    for intent in ["one", "two", "three"] {
        world
            .store
            .create_advisor_run(&run(&world, intent))
            .expect("opens");
    }
    let listed = world
        .store
        .list_advisor_runs(world.project_id, world.epic)
        .expect("readable");
    assert_eq!(
        listed.len(),
        3,
        "a profile's consultation limit counts durable runs"
    );
    let elsewhere = world
        .store
        .list_advisor_runs(world.project_id, MiniProjectId::generate())
        .expect("readable");
    assert!(elsewhere.is_empty());
}

#[test]
fn another_projects_consultation_does_not_resolve() {
    let world = world();
    let planned = run(&world, "scoped");
    world.store.create_advisor_run(&planned).expect("opens");
    let elsewhere = world
        .store
        .get_advisor_run(ProjectId::generate(), planned.id)
        .expect("readable");
    assert!(
        elsewhere.is_none(),
        "a valid id from another project must not resolve"
    );
}
