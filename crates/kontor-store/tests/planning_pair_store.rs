//! Schema v122 (ASMA-8282): the `planning_pair` consultation family in the one
//! consultation registry, with its run-keyed placement, record and
//! contribution payload.
//!
//! These prove storage's own share of the contract: the shared semantic
//! identity index, compare-and-swap, immutable contributions, a terminal
//! disposition, and the family-conditioned states SQL itself refuses. The
//! container kind a governed pair is placed in is the service's explicit
//! Team Definition selection, so the fixture's node kind is incidental here.

mod support;

use kontor_core::consultation::{
    ConsultationContextPolicy, ConsultationFamily, ConsultationRunId, ConsultationRunState,
    ConsultationScope, ConsultationSubject, MemoryAccess,
};
use kontor_core::id::{
    AdvisorRunId, AggregateRevision, BoundedText, CanonicalDocument, ContentHash, CurrencyCode,
    ExternalName, IdempotencyKey, MiniProjectId, Money, PlanningPairProfileId, PlanningPairRunId,
    ProjectId, RoleCode, RoleKey, RoleSlotId, SCHEMA_VERSION, SeatBindingId, SpecVersion,
    Timestamp, TopologyKindKey, TopologyNodeId, parse_utc_timestamp,
};
use kontor_core::planning_pair::{
    ConsultationProtocol, PlanningPairActor, PlanningPairMember, PlanningPairMemberSpec,
    PlanningPairMembers, PlanningPairRecord, PlanningPairRound, PlanningPairRun, PlanningPairSlot,
    PlanningPairSpec, PlanningPairState, RecordedContribution,
};
use kontor_core::repository::{
    MiniProjectTopologySnapshot, NewMiniProject, NewProject, NewSeatBinding,
    NewSessionTopologyNode, ProjectRepository, RepositoryError, StoredConsultationProfileRevision,
    StoredConsultationRun, StoredConsultationSeat, StoredEpicRoster,
    StoredPlanningPairContribution, StoredPlanningPairPlacement, StoredPlanningPairRecord,
    TopologyRepository,
};
use kontor_core::spec::{
    BudgetBounds, CatalogRoleRef, ModelRef, ModelRung, ProviderRef, Shareability, ShareabilityTier,
    TopologySnapshot,
};
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use rusqlite::Connection;
use tempfile::TempDir;

const PROFILE: &str = "01991c00-0000-7000-8000-0000000000a1";

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

fn name(text: &str) -> ExternalName {
    ExternalName::parse(text).expect("a valid name")
}

fn text(value: &str) -> BoundedText {
    BoundedText::parse(value).expect("bounded text")
}

fn stamp() -> Shareability {
    Shareability::default_for(ShareabilityTier::ProjectKnowledge).expect("tier B classifies")
}

fn spec() -> PlanningPairSpec {
    let member = |slot| PlanningPairMemberSpec {
        slot,
        role_code: RoleCode::parse("SA").expect("a registered role code"),
        specialty: text("Independent planning perspective"),
        behavior: text("Read the frozen plan and give one finding; change nothing."),
        context: ConsultationContextPolicy {
            skills: Vec::new(),
            files: Vec::new(),
            memory: MemoryAccess::None,
        },
    };
    PlanningPairSpec {
        schema_version: SCHEMA_VERSION,
        protocol: ConsultationProtocol::PlanningPair,
        profile_id: PlanningPairProfileId::parse(PROFILE).expect("profile id"),
        version: SpecVersion::FIRST,
        name: name("Planning pair"),
        charter: text("Is this plan the smallest sound next step?"),
        container_kind: TopologyKindKey::parse("CSW").expect("kind"),
        members: vec![
            member(PlanningPairSlot::SeatA),
            member(PlanningPairSlot::SeatB),
        ],
        allowed_caller_roles: vec![RoleKey::parse("lead").expect("role")],
        allowed_scopes: vec![ConsultationScope::Epic],
        budget: BudgetBounds {
            max_tokens: 200_000,
            max_commands: 40,
            max_duration_seconds: 1_800,
            max_cost: Money {
                minor_units: 5_000,
                currency: CurrencyCode::parse("NOK").expect("currency"),
            },
        },
    }
}

fn member(slot: PlanningPairSlot, account: &str, vendor: &str) -> PlanningPairMember {
    PlanningPairMember {
        slot,
        binding_key: "advisor/01a02d00-0000-7000-8000-00000000ad01".to_owned(),
        route: ModelRung {
            provider: ProviderRef(account.to_owned()),
            model: ModelRef(format!("{vendor}-flagship")),
            effort: None,
        },
        vendor: vendor.to_owned(),
    }
}

struct World {
    _home: TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    mini_project_id: MiniProjectId,
    topology: TopologySnapshot,
    esw: TopologyNodeId,
    caller: SeatBindingId,
    catalog: kontor_core::spec::RoleCatalogRevision,
}

fn world() -> World {
    let home = support::state_root();
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    let mini_project_id = MiniProjectId::generate();
    let created_at = at("2026-10-02T12:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Planning project"),
            root_path: name("/tmp/planning-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_mini_project(&NewMiniProject {
            id: mini_project_id,
            project_id,
            name: name("Planning epic"),
            created_at,
        })
        .expect("the epic is created");
    let domain = bundled_operational_domain().expect("the bundled domain validates");
    let topology_spec = domain.topology_specs.first().expect("a topology").clone();
    let catalog = domain.role_catalogs.first().expect("a catalog").clone();
    let canonical_hash = store
        .publish_topology_spec(project_id, &topology_spec, &stamp(), created_at)
        .expect("the topology publishes");
    let catalog_hash = store
        .publish_role_catalog(&catalog, &stamp(), created_at)
        .expect("the catalog publishes");
    // The epic's frozen roster is its role catalog selection.
    store
        .put_epic_roster(&StoredEpicRoster {
            project_id,
            mini_project_id,
            core_team_version: SpecVersion::FIRST,
            catalog_hash,
            seats: serde_json::json!([]),
            quick_session_id: None,
            revision: AggregateRevision::INITIAL,
            pinned_at: created_at,
        })
        .expect("the epic freezes its roster");
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
    let caller = SeatBindingId::generate();
    store
        .create_seat_binding(&NewSeatBinding {
            id: caller,
            project_id,
            topology_node_id: ecp,
            role_slot_id: RoleSlotId::parse("epic.lsa").expect("a slot"),
            role: role(&catalog, "LSA"),
            task_id: None,
            team_run_id: None,
            attach_deadline: at("2026-10-02T12:10:00Z"),
            parent_seat_binding_id: None,
            created_at,
        })
        .expect("the caller seat is bound");
    let document = spec().canonicalize().expect("a canonical document");
    store
        .publish_consultation_profile_revision(&StoredConsultationProfileRevision {
            project_id,
            family: ConsultationFamily::PlanningPair,
            profile_id: PROFILE.to_owned(),
            version: SpecVersion::FIRST,
            name: name("Planning pair"),
            definition: document.json().to_owned(),
            definition_hash: document.hash().clone(),
            published_at: created_at,
        })
        .expect("a planning pair document publishes into the shared catalog");
    World {
        _home: home,
        store,
        project_id,
        mini_project_id,
        topology,
        esw,
        caller,
        catalog,
    }
}

fn role(catalog: &kontor_core::spec::RoleCatalogRevision, code: &str) -> CatalogRoleRef {
    let entry = catalog
        .role(&RoleCode::parse(code).expect("a role code"))
        .expect("the catalog has the role");
    CatalogRoleRef {
        catalog_id: catalog.catalog_id,
        catalog_revision: catalog.version,
        role_code: entry.role_code.clone(),
        standard_title: entry.standard_title.clone(),
        custom_display_name: None,
    }
}

/// One frozen pair: its run, node, two member seats, placement and first
/// record, with the domain run that record restores to.
#[derive(Debug)]
struct Frozen {
    run: StoredConsultationRun,
    seats: [SeatBindingId; 2],
    pair: PlanningPairRun,
    record: PlanningPairRecord,
}

fn freeze(world: &World, key: &str, semantic: ContentHash) -> Result<Frozen, RepositoryError> {
    freeze_as(world, key, semantic, &role(&world.catalog, "SA"))
}

/// Freeze one pair whose two member seats hold `member_role`.
fn freeze_as(
    world: &World,
    key: &str,
    semantic: ContentHash,
    member_role: &CatalogRoleRef,
) -> Result<Frozen, RepositoryError> {
    let created_at = at("2026-10-02T12:30:00Z");
    let run_id = ConsultationRunId::PlanningPair(PlanningPairRunId::generate());
    let placement = CanonicalDocument::from_value(&serde_json::json!({
        "schema_version": 1,
        "protocol": "planning_pair@1",
        "selection": { "key": key },
    }))
    .expect("a canonical placement");
    let members = PlanningPairMembers::freeze(
        placement.hash().clone(),
        vec![
            member(PlanningPairSlot::SeatA, "claude-personal", "anthropic"),
            member(PlanningPairSlot::SeatB, "codex-work", "openai"),
        ],
    )
    .expect("two members on distinct vendors");
    let question = text("Should the migration land first?");
    let pair = PlanningPairRun::admit(&spec(), members, question.clone()).expect("admitted");
    let record = PlanningPairRecord::admitted(&pair);
    let context = serde_json::json!({ "schema_version": 1 });
    let context_hash = CanonicalDocument::from_serializable(&context)
        .expect("canonical context")
        .hash()
        .clone();
    let node_id = TopologyNodeId::generate();
    let run = StoredConsultationRun {
        id: run_id,
        project_id: world.project_id,
        mini_project_id: world.mini_project_id,
        topic: Some(name("Migration ordering")),
        profile_id: PROFILE.to_owned(),
        profile_version: SpecVersion::FIRST,
        definition_hash: pair.spec_hash().clone(),
        semantic_identity_hash: Some(semantic),
        subject: Some(ConsultationSubject::Epic),
        question_hash: ContentHash::of(question.as_str().as_bytes()),
        question,
        context,
        context_hash,
        caller_seat_binding_id: world.caller,
        topology_node_id: node_id,
        invoke_key: IdempotencyKey::parse(key).expect("a key"),
        invoke_intent_hash: ContentHash::of(key.as_bytes()),
        state: ConsultationRunState::Materializing,
        round: 1,
        result: None,
        result_hash: None,
        revision: AggregateRevision::INITIAL,
        created_at,
        updated_at: created_at,
        settled_at: None,
    };
    let rung = ModelRung {
        provider: ProviderRef("claude-personal".to_owned()),
        model: ModelRef("anthropic-flagship".to_owned()),
        effort: None,
    };
    let ids = [SeatBindingId::generate(), SeatBindingId::generate()];
    let bindings: Vec<NewSeatBinding> = PlanningPairSlot::ALL
        .iter()
        .zip(ids)
        .map(|(slot, id)| NewSeatBinding {
            id,
            project_id: world.project_id,
            topology_node_id: node_id,
            role_slot_id: RoleSlotId::parse(slot.as_str()).expect("a slot"),
            role: member_role.clone(),
            task_id: None,
            team_run_id: None,
            attach_deadline: at("2026-10-02T12:40:00Z"),
            parent_seat_binding_id: Some(world.caller),
            created_at,
        })
        .collect();
    let seats: Vec<StoredConsultationSeat> = PlanningPairSlot::ALL
        .iter()
        .zip(ids)
        .map(|(slot, id)| StoredConsultationSeat {
            run_id,
            role_slot_id: RoleSlotId::parse(slot.as_str()).expect("a slot"),
            committee_role: None,
            logical_role: RoleKey::parse("planning_pair_member").expect("a logical role"),
            seat_binding_id: id,
            model_rung: rung.clone(),
            occupancy_generation: 1,
            native_identity: None,
            provider_session_id: None,
            observed_at: None,
        })
        .collect();
    world.store.create_planning_pair_run(
        &run,
        &NewSessionTopologyNode {
            id: node_id,
            project_id: world.project_id,
            mini_project_id: Some(world.mini_project_id),
            topology: world.topology.clone(),
            kind: TopologyKindKey::parse("CSW").expect("a kind"),
            parent_id: Some(world.esw),
            task_id: None,
            created_at,
        },
        &seats.iter().zip(bindings.iter()).collect::<Vec<_>>(),
        &StoredPlanningPairPlacement {
            run_id,
            project_id: world.project_id,
            placement,
            created_at,
        },
        &stored_record(world, run_id, AggregateRevision::INITIAL, &pair, &record),
    )?;
    Ok(Frozen {
        run,
        seats: ids,
        pair,
        record,
    })
}

fn stored_record(
    world: &World,
    run_id: ConsultationRunId,
    revision: AggregateRevision,
    pair: &PlanningPairRun,
    record: &PlanningPairRecord,
) -> StoredPlanningPairRecord {
    StoredPlanningPairRecord {
        run_id,
        project_id: world.project_id,
        revision,
        phase: pair.state(),
        record: record.canonicalize().expect("a canonical record"),
        created_at: at("2026-10-02T12:45:00Z"),
    }
}

fn contribution(
    run_id: ConsultationRunId,
    slot: PlanningPairSlot,
    hash: &ContentHash,
    seat: SeatBindingId,
    revision: AggregateRevision,
) -> StoredPlanningPairContribution {
    StoredPlanningPairContribution {
        run_id,
        round: PlanningPairRound::Findings,
        slot,
        document_hash: hash.clone(),
        seat_binding_id: seat,
        occupancy_generation: 1,
        record_revision: revision,
        created_at: at("2026-10-02T12:45:00Z"),
    }
}

fn raw(world: &World) -> Connection {
    Connection::open(world._home.path().join("kontor.db")).expect("a raw connection")
}

fn conflict_rule(result: Result<impl std::fmt::Debug, RepositoryError>) -> &'static str {
    match result {
        Err(RepositoryError::Conflict { rule, .. }) => rule,
        other => panic!("expected a conflict, got {other:?}"),
    }
}

fn refused_by_sql(result: rusqlite::Result<usize>, needle: &str) {
    match result {
        Err(error) => assert!(error.to_string().contains(needle), "{error} lacks {needle}"),
        Ok(rows) => panic!("SQL accepted the write ({rows} rows) that `{needle}` forbids"),
    }
}

/// Record seat A's finding through storage the way the service does: the
/// domain transition, then the persistence owner's own input appended under
/// the address the transition returned.
fn record_seat_a(world: &World, frozen: &mut Frozen) -> StoredConsultationRun {
    let hash = frozen
        .pair
        .record_finding(
            PlanningPairActor::Member(PlanningPairSlot::SeatA),
            PlanningPairSlot::SeatA,
            text("Land the migration first."),
        )
        .expect("seat A's finding");
    frozen.record.findings.push(RecordedContribution {
        slot: PlanningPairSlot::SeatA,
        advice: text("Land the migration first."),
        document_hash: hash.clone(),
    });
    let next = AggregateRevision::INITIAL.next().expect("revision two");
    world
        .store
        .append_planning_pair_record(
            world.project_id,
            frozen.run.id,
            AggregateRevision::INITIAL,
            ConsultationRunState::Running,
            &stored_record(world, frozen.run.id, next, &frozen.pair, &frozen.record),
            Some(&contribution(
                frozen.run.id,
                PlanningPairSlot::SeatA,
                &hash,
                frozen.seats[0],
                next,
            )),
        )
        .expect("the transition is accepted")
}

#[test]
fn a_planning_pair_is_frozen_with_its_placement_and_first_record() {
    let world = world();
    let frozen = freeze(&world, "invoke-pair-1", ContentHash::of(b"identity-1")).expect("frozen");
    let stored = world
        .store
        .get_consultation_run(world.project_id, frozen.run.id)
        .expect("readable")
        .expect("present");
    assert_eq!(stored.id.family(), ConsultationFamily::PlanningPair);
    assert_eq!(stored.state, ConsultationRunState::Materializing);
    assert_eq!(
        world
            .store
            .get_consultation_run_by_semantic_identity(
                world.project_id,
                &ContentHash::of(b"identity-1")
            )
            .expect("readable")
            .map(|run| run.id),
        Some(frozen.run.id),
        "the shared semantic identity lookup finds the pair"
    );
    let placement = world
        .store
        .planning_pair_placement(world.project_id, frozen.run.id)
        .expect("readable")
        .expect("frozen");
    assert_eq!(
        placement.placement.hash(),
        frozen.pair.members().placement_hash()
    );
    let record = world
        .store
        .latest_planning_pair_record(world.project_id, frozen.run.id)
        .expect("readable")
        .expect("the first revision");
    assert_eq!(record.revision, AggregateRevision::INITIAL);
    assert_eq!(record.phase, PlanningPairState::AwaitingFindings);
    // Storage returns bytes; the domain alone reads them back.
    let restored =
        PlanningPairRun::restore(&spec(), record.record.deserialize().expect("a record"))
            .expect("restores through the domain transitions");
    assert_eq!(restored.state(), PlanningPairState::AwaitingFindings);
}

#[test]
fn a_transition_moves_the_run_and_its_record_together_under_compare_and_swap() {
    let world = world();
    let mut frozen =
        freeze(&world, "invoke-pair-2", ContentHash::of(b"identity-2")).expect("frozen");
    let moved = record_seat_a(&world, &mut frozen);
    let two = AggregateRevision::INITIAL.next().expect("revision two");
    assert_eq!(
        (moved.revision, moved.state),
        (two, ConsultationRunState::Running)
    );
    let latest = world
        .store
        .latest_planning_pair_record(world.project_id, frozen.run.id)
        .expect("readable")
        .expect("present");
    assert_eq!(latest.revision, two);
    let contributions = world
        .store
        .planning_pair_contributions(world.project_id, frozen.run.id)
        .expect("readable");
    assert_eq!(contributions.len(), 1);
    assert_eq!(contributions[0].seat_binding_id, frozen.seats[0]);
    assert_eq!(contributions[0].record_revision, two);

    // A writer that read revision one has lost the race.
    let three = two.next().expect("revision three");
    assert_eq!(
        conflict_rule(world.store.append_planning_pair_record(
            world.project_id,
            frozen.run.id,
            AggregateRevision::INITIAL,
            ConsultationRunState::Running,
            &stored_record(&world, frozen.run.id, two, &frozen.pair, &frozen.record),
            None,
        )),
        "the run moved since it was read"
    );
    // The record must describe exactly the next revision.
    assert_eq!(
        conflict_rule(world.store.append_planning_pair_record(
            world.project_id,
            frozen.run.id,
            two,
            ConsultationRunState::Running,
            &stored_record(&world, frozen.run.id, two, &frozen.pair, &frozen.record),
            None,
        )),
        "the record and contribution must describe the run's next revision"
    );
    // And storage itself refuses a record that skips the run's revision.
    refused_by_sql(
        raw(&world).execute(
            "INSERT INTO planning_pair_record_revisions
                 (run_id, project_id, revision, phase, record, record_hash, created_at)
             VALUES (?1, ?2, ?3, 'findings_released', '{}', ?4, '2026-10-02T13:00:00Z')",
            rusqlite::params![
                frozen.run.id.as_text(),
                world.project_id.to_string(),
                i64::try_from(three.get().saturating_add(1)).expect("fits"),
                ContentHash::of(b"forged").as_str(),
            ],
        ),
        "follows its run revision exactly",
    );
}

#[test]
fn a_recorded_contribution_cannot_be_rewritten() {
    let world = world();
    let mut frozen =
        freeze(&world, "invoke-pair-3", ContentHash::of(b"identity-3")).expect("frozen");
    record_seat_a(&world, &mut frozen);
    let two = AggregateRevision::INITIAL.next().expect("revision two");
    let three = two.next().expect("revision three");
    // A second seat-A finding, even under a fresh revision, is refused.
    assert_eq!(
        conflict_rule(world.store.append_planning_pair_record(
            world.project_id,
            frozen.run.id,
            two,
            ConsultationRunState::Running,
            &stored_record(&world, frozen.run.id, three, &frozen.pair, &frozen.record),
            Some(&contribution(
                frozen.run.id,
                PlanningPairSlot::SeatA,
                &ContentHash::of(b"a softer finding"),
                frozen.seats[0],
                three,
            )),
        )),
        "a recorded finding or answer is immutable"
    );
    let connection = raw(&world);
    refused_by_sql(
        connection.execute(
            "UPDATE planning_pair_contributions SET document_hash = ?1 WHERE run_id = ?2",
            rusqlite::params![
                ContentHash::of(b"rewritten").as_str(),
                frozen.run.id.as_text()
            ],
        ),
        "a planning pair contribution is immutable",
    );
    refused_by_sql(
        connection.execute(
            "DELETE FROM planning_pair_contributions WHERE run_id = ?1",
            [frozen.run.id.as_text()],
        ),
        "cannot be withdrawn",
    );
    refused_by_sql(
        connection.execute(
            "UPDATE planning_pair_record_revisions SET record = '{}' WHERE run_id = ?1",
            [frozen.run.id.as_text()],
        ),
        "a planning pair record revision is immutable",
    );
    refused_by_sql(
        connection.execute(
            "UPDATE planning_pair_placements SET placement = '{}' WHERE run_id = ?1",
            [frozen.run.id.as_text()],
        ),
        "a frozen planning pair placement is immutable",
    );
}

#[test]
fn a_disposed_pair_is_terminal_in_storage() {
    let world = world();
    let frozen = freeze(&world, "invoke-pair-4", ContentHash::of(b"identity-4")).expect("frozen");
    let two = AggregateRevision::INITIAL.next().expect("revision two");
    let disposed = world
        .store
        .append_planning_pair_record(
            world.project_id,
            frozen.run.id,
            AggregateRevision::INITIAL,
            ConsultationRunState::Disposed,
            &StoredPlanningPairRecord {
                phase: PlanningPairState::Disposed,
                ..stored_record(&world, frozen.run.id, two, &frozen.pair, &frozen.record)
            },
            None,
        )
        .expect("storage accepts the disposed revision it is given");
    assert_eq!(disposed.state, ConsultationRunState::Disposed);
    assert!(
        disposed.settled_at.is_none(),
        "a disposition is not a settlement"
    );
    let three = two.next().expect("revision three");
    assert_eq!(
        conflict_rule(world.store.append_planning_pair_record(
            world.project_id,
            frozen.run.id,
            two,
            ConsultationRunState::Disposed,
            &stored_record(&world, frozen.run.id, three, &frozen.pair, &frozen.record),
            None,
        )),
        "the run moved since it was read"
    );
    let connection = raw(&world);
    refused_by_sql(
        connection.execute(
            "UPDATE consultation_runs SET updated_at = '2026-10-02T14:00:00Z' WHERE run_id = ?1",
            [frozen.run.id.as_text()],
        ),
        "a disposed planning pair is terminal",
    );
    refused_by_sql(
        connection.execute(
            "INSERT INTO planning_pair_record_revisions
                 (run_id, project_id, revision, phase, record, record_hash, created_at)
             VALUES (?1, ?2, 2, 'disposed', '{}', ?3, '2026-10-02T14:00:00Z')",
            rusqlite::params![
                frozen.run.id.as_text(),
                world.project_id.to_string(),
                ContentHash::of(b"after").as_str()
            ],
        ),
        "never a disposition",
    );
}

#[test]
fn storage_refuses_an_unknown_family_and_a_state_outside_the_family() {
    let world = world();
    let frozen = freeze(&world, "invoke-pair-5", ContentHash::of(b"identity-5")).expect("frozen");
    let connection = raw(&world);
    refused_by_sql(
        connection.execute(
            "INSERT INTO consultation_profile_revisions
                 (project_id, family, profile_id, version, name, definition, definition_hash, created_at)
             VALUES (?1, 'jury', ?2, 1, 'Jury', '{}', ?3, '2026-10-02T12:00:00Z')",
            rusqlite::params![
                world.project_id.to_string(),
                PROFILE,
                ContentHash::of(b"jury").as_str()
            ],
        ),
        "CHECK constraint failed",
    );
    // A planning pair never awaits a Judge, settles or carries a result, and
    // never takes a second Committee round as a clarification allowance.
    for (assignment, case) in [
        ("state = 'awaiting_judge'", "awaiting_judge"),
        (
            "state = 'settled', settled_at = '2026-10-02T13:00:00Z'",
            "settled",
        ),
        ("result = '{}', result_hash = ?2", "a result"),
        ("round = 2", "a second round"),
    ] {
        let sql = format!("UPDATE consultation_runs SET {assignment} WHERE run_id = ?1");
        let result = if assignment.contains("?2") {
            connection.execute(
                &sql,
                rusqlite::params![frozen.run.id.as_text(), ContentHash::of(b"r").as_str()],
            )
        } else {
            connection.execute(&sql, [frozen.run.id.as_text()])
        };
        match result {
            Err(error) => assert!(
                error.to_string().contains("CHECK constraint failed"),
                "{case}: {error}"
            ),
            Ok(rows) => panic!("{case}: SQL accepted {rows} rows"),
        }
    }
    // And no Advisor or Committee row can be disposed. The control row is the
    // pair's own, copied under another family and state, so the only thing
    // that differs between the accepted and the refused insert is the state.
    // Foreign keys are off on this connection only: the copy names a node and
    // an Advisor profile that do not exist, and this proves the state CHECK
    // alone, not referential integrity.
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .expect("the probe disables foreign keys");
    let copy = |family: &str, state: &str, key: &str| {
        connection.execute(
            &format!(
                "INSERT INTO consultation_runs
                 SELECT ?2, project_id, mini_project_id, '{family}', profile_id,
                        profile_version, definition_hash, question, question_hash, context,
                        context_hash, caller_seat_binding_id, ?3, ?4, invoke_intent_hash,
                        '{state}', round, result, result_hash, revision, created_at,
                        updated_at, settled_at, topic, NULL, subject_kind, subject_task_id
                   FROM consultation_runs WHERE run_id = ?1"
            ),
            rusqlite::params![
                frozen.run.id.as_text(),
                AdvisorRunId::generate().to_string(),
                TopologyNodeId::generate().to_string(),
                key,
            ],
        )
    };
    for family in ["advisor", "committee"] {
        refused_by_sql(
            copy(family, "disposed", &format!("{family}-disposed")),
            "CHECK constraint failed",
        );
    }
    assert_eq!(
        copy("advisor", "running", "advisor-running").expect("the control row is valid"),
        1
    );
}

#[test]
fn a_planning_pair_shares_the_one_semantic_identity_index() {
    let world = world();
    freeze(&world, "invoke-pair-6", ContentHash::of(b"shared identity")).expect("the first pair");
    assert_eq!(
        conflict_rule(freeze(
            &world,
            "invoke-pair-7",
            ContentHash::of(b"shared identity")
        )),
        "a consultation run already owns this family, scope, profile and topic"
    );
}

#[test]
fn placement_and_records_belong_only_to_a_planning_pair() {
    let world = world();
    let frozen = freeze(&world, "invoke-pair-8", ContentHash::of(b"identity-8")).expect("frozen");
    let advisor_id = ConsultationRunId::Advisor(AdvisorRunId::generate());
    assert_eq!(
        conflict_rule(world.store.append_planning_pair_record(
            world.project_id,
            advisor_id,
            AggregateRevision::INITIAL,
            ConsultationRunState::Running,
            &stored_record(
                &world,
                advisor_id,
                AggregateRevision::INITIAL.next().expect("two"),
                &frozen.pair,
                &frozen.record
            ),
            None,
        )),
        "the record and contribution must describe the run's next revision"
    );
    refused_by_sql(
        raw(&world).execute(
            "INSERT INTO planning_pair_placements
                 (run_id, project_id, protocol, placement, placement_hash, created_at)
             VALUES (?1, ?2, 'committee@1', '{}', ?3, '2026-10-02T12:00:00Z')",
            rusqlite::params![
                frozen.run.id.as_text(),
                world.project_id.to_string(),
                ContentHash::of(b"x").as_str()
            ],
        ),
        "CHECK constraint failed",
    );
}

/// ASMA-8282 audit 6f: a member seat's role is an exact projection of the role
/// catalog its epic selected, proved inside the freezing transaction. Another
/// persisted catalog revision, a rewritten title, a code the catalog does not
/// declare and an epic with no frozen roster each write nothing.
#[test]
fn a_member_role_outside_the_epics_selected_catalog_is_refused_in_storage() {
    let world = world();
    let mut unselected = world.catalog.clone();
    unselected.version = SpecVersion::parse(2).expect("a later revision");
    world
        .store
        .publish_role_catalog(&unselected, &stamp(), at("2026-10-02T12:05:00Z"))
        .expect("an unselected revision is persisted beside the selected one");
    assert_eq!(
        conflict_rule(freeze_as(
            &world,
            "pp-store-unselected",
            ContentHash::of(b"unselected"),
            &role(&unselected, "SA"),
        )),
        "a member role names a role catalog the epic did not select"
    );
    let mut retitled = role(&world.catalog, "SA");
    retitled.standard_title = name("Self-appointed architect");
    let refused = freeze_as(
        &world,
        "pp-store-retitled",
        ContentHash::of(b"retitled"),
        &retitled,
    );
    assert!(
        matches!(
            &refused,
            Err(RepositoryError::Domain(kontor_core::DomainError::Invalid {
                rule: "standard title differs from the catalog",
                ..
            }))
        ),
        "{refused:?}"
    );
    let mut unknown = role(&world.catalog, "SA");
    unknown.role_code = RoleCode::parse("ZZZ").expect("a well-formed code");
    let refused = freeze_as(
        &world,
        "pp-store-unknown",
        ContentHash::of(b"unknown"),
        &unknown,
    );
    assert!(
        matches!(
            &refused,
            Err(RepositoryError::Domain(kontor_core::DomainError::Invalid {
                rule: "names a role code absent from the pinned catalog",
                ..
            }))
        ),
        "{refused:?}"
    );
    raw(&world)
        .execute(
            "DELETE FROM epic_rosters WHERE mini_project_id = ?1",
            [world.mini_project_id.to_string()],
        )
        .expect("the roster row is removed");
    assert_eq!(
        conflict_rule(freeze(
            &world,
            "pp-store-no-roster",
            ContentHash::of(b"no roster")
        )),
        "the epic has frozen no roster, so it has selected no role catalog"
    );
    let runs: i64 = raw(&world)
        .query_row(
            "SELECT count(*) FROM consultation_runs WHERE family = 'planning_pair'",
            [],
            |row| row.get(0),
        )
        .expect("the runs count");
    assert_eq!(runs, 0, "no refused freeze wrote a run");
}
