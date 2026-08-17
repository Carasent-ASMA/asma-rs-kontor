//! KON-OP-04 durable reconciliation keys, and what happens when a write fails.
//!
//! Both OP-04 commands that produce several effects write the row that
//! reconciles them *before* those effects. These tests are about the moment
//! that ordering exists for: a failure between the key and its effects. A key
//! recorded without what a resume needs to read is worse than no key at all,
//! because the row is keyed by its subject and nothing deletes it — the subject
//! is then stuck for good.

use kontor_core::id::{
    AggregateRevision, BoundedText, CanonicalDocument, ContentHash, ExternalName, MiniProjectId,
    ProjectId, QuickSessionId, RoleCode, RoleSlotId, SeatBindingId, SpecVersion, Timestamp,
    TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::repository::{
    NewProject, ProjectRepository, RepositoryError, SourceDisposition, StoredEpicRoster,
    StoredPromotion, StoredQuickSession,
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
}

fn world() -> World {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Operational project"),
            root_path: name("/tmp/op04-promotion"),
            created_at: at("2026-08-17T09:00:00Z"),
        })
        .expect("the project is created");
    World {
        _home: home,
        store,
        project_id,
    }
}

fn quick_session(project_id: ProjectId, intent: &str) -> StoredQuickSession {
    StoredQuickSession {
        id: QuickSessionId::generate(),
        project_id,
        role: CatalogRoleRef {
            catalog_id: kontor_core::id::RoleCatalogId::generate(),
            catalog_revision: SpecVersion::FIRST,
            role_code: RoleCode::parse("SA").expect("a role code"),
            standard_title: name("Software Architect"),
            custom_display_name: None,
        },
        role_slot_id: RoleSlotId::parse("sa").expect("a slot"),
        topology_node_id: TopologyNodeId::generate(),
        seat_binding_id: SeatBindingId::generate(),
        psw_topology_node_id: TopologyNodeId::generate(),
        psw_native_id: None,
        purpose: BoundedText::parse("Investigate something").expect("a purpose"),
        intent_hash: hash(intent),
        disposition: SourceDisposition::Idle,
        revision: AggregateRevision::INITIAL,
        created_at: at("2026-08-17T09:01:00Z"),
    }
}

fn roster(project_id: ProjectId, mini_project_id: MiniProjectId) -> StoredEpicRoster {
    StoredEpicRoster {
        project_id,
        mini_project_id,
        core_team_version: SpecVersion::FIRST,
        catalog_hash: hash("catalog"),
        seats: serde_json::json!([]),
        quick_session_id: None,
        revision: AggregateRevision::INITIAL,
        pinned_at: at("2026-08-17T09:02:00Z"),
    }
}

fn promotion(
    project_id: ProjectId,
    quick_session_id: QuickSessionId,
    mini_project_id: MiniProjectId,
) -> StoredPromotion {
    StoredPromotion {
        quick_session_id,
        project_id,
        mini_project_id,
        preview_hash: hash("preview"),
        source_disposition: SourceDisposition::Idle,
        handoff: None,
        handoff_hash: None,
        lsa_seat_binding_id: None,
        completed_at: None,
        created_at: at("2026-08-17T09:02:00Z"),
    }
}

/// Authorizing a promotion records both of the things a resume has to read.
///
/// The resume path reads the frozen roster before anything else. A promotion
/// row written without one leaves every retry dying on a roster that was never
/// written — and since the row is keyed by its source and nothing deletes it,
/// that Quick session can never be promoted again.
#[test]
fn authorizing_a_promotion_records_the_epic_and_its_roster_together() {
    let world = world();
    let session = quick_session(world.project_id, "together");
    world
        .store
        .create_quick_session(&session)
        .expect("the session is recorded");
    let epic_id = MiniProjectId::generate();

    world
        .store
        .begin_promotion(
            &promotion(world.project_id, session.id, epic_id),
            &roster(world.project_id, epic_id),
        )
        .expect("the promotion is authorized");

    // Both, or the resume has nothing to resume from.
    assert!(
        world
            .store
            .get_promotion(session.id)
            .expect("the promotion reads")
            .is_some(),
        "the promotion row is missing"
    );
    assert!(
        world
            .store
            .get_epic_roster(world.project_id, epic_id)
            .expect("the roster reads")
            .is_some(),
        "the epic was recorded as promoted with no roster to resume from"
    );
}

/// A promotion that cannot record its roster records nothing at all.
///
/// The two writes are one transaction precisely so this cannot half-happen. If
/// the promotion row survived a failed roster write, the source would be marked
/// promoted, the resume would find no roster, and no operation exists to clear
/// either — the source would be permanently unpromotable.
#[test]
fn a_promotion_that_cannot_freeze_its_roster_leaves_the_source_promotable() {
    let world = world();
    let session = quick_session(world.project_id, "rollback");
    world
        .store
        .create_quick_session(&session)
        .expect("the session is recorded");
    let epic_id = MiniProjectId::generate();
    // Occupy the roster's primary key, so the second write of the pair fails.
    world
        .store
        .put_epic_roster(&roster(world.project_id, epic_id))
        .expect("the conflicting roster is written");

    let refused = world.store.begin_promotion(
        &promotion(world.project_id, session.id, epic_id),
        &roster(world.project_id, epic_id),
    );
    assert!(refused.is_err(), "the conflicting write was accepted");

    assert!(
        world
            .store
            .get_promotion(session.id)
            .expect("the promotion reads")
            .is_none(),
        "a failed authorization left the source recorded as promoted, which nothing can undo"
    );
}

/// Two commands opening the same session: the loser is told, not crashed.
///
/// The row is written before the node and the seat, so a loser that is told
/// `Conflict` has placed nothing and can simply read the winner's session. A
/// generic backend error here would be indistinguishable from a real storage
/// failure, and the caller would retry into a second workspace.
#[test]
fn a_second_command_for_one_session_is_a_conflict_rather_than_a_backend_error() {
    let world = world();
    let first = quick_session(world.project_id, "raced");
    world
        .store
        .create_quick_session(&first)
        .expect("the first session is recorded");

    // Same intent, different minted ids: the second command's plan.
    let mut second = quick_session(world.project_id, "raced");
    second.intent_hash = first.intent_hash.clone();

    match world.store.create_quick_session(&second) {
        Err(RepositoryError::Conflict { .. }) => {}
        other => panic!("expected a conflict, got {other:?}"),
    }

    let found = world
        .store
        .get_quick_session_by_intent(world.project_id, &first.intent_hash)
        .expect("the session reads")
        .expect("one session for this intent");
    assert_eq!(
        found.id, first.id,
        "the loser's ids replaced the winner's session"
    );
}
