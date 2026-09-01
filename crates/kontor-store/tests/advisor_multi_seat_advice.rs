//! Schema v78: one Advisor Session Workspace holds one *or more* independently
//! reporting seats, so advice is keyed by the seat that gave it.

use kontor_core::consultation::{ConsultationFamily, ConsultationRunId, ConsultationRunState};
use kontor_core::id::{
    AdvisorRunId, AggregateRevision, BoundedText, CanonicalDocument, ContentHash, ExternalId,
    ExternalName, IdempotencyKey, MiniProjectId, ProjectId, RoleCode, RoleKey, RoleSlotId,
    RuntimeKindKey, SeatBindingId, SpecVersion, Timestamp, TopologyKindKey, TopologyNodeId,
    parse_utc_timestamp,
};
use kontor_core::repository::{
    MiniProjectTopologySnapshot, NewMiniProject, NewProject, NewSeatBinding,
    NewSessionTopologyNode, ProjectRepository, StoredConsultationProfileRevision,
    StoredConsultationRun, StoredConsultationSeat, TopologyRepository,
};
use kontor_core::spec::{
    CatalogRoleRef, ModelRef, ModelRung, ProviderRef, Shareability, ShareabilityTier,
    TopologySnapshot,
};
use kontor_core::state::NativeRuntimeIdentity;
use kontor_profiles::bundled_operational_domain;
use kontor_store::SqliteStore;
use tempfile::TempDir;

const PROFILE: &str = "01991c00-0000-7000-8000-00000000009a";

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
    _home: TempDir,
    store: SqliteStore,
    project_id: ProjectId,
    run: StoredConsultationRun,
    seats: [SeatBindingId; 2],
}

/// One Advisor run with two attested, independently reporting seats.
fn world() -> World {
    let home = TempDir::new().expect("a temporary directory");
    let store = SqliteStore::open(&home.path().join("kontor.db")).expect("the store opens");
    let project_id = ProjectId::generate();
    let mini_project_id = MiniProjectId::generate();
    let created_at = at("2026-09-01T12:00:00Z");
    store
        .create_project(&NewProject {
            id: project_id,
            name: name("Advisor project"),
            root_path: name("/tmp/advisor-project"),
            created_at,
        })
        .expect("the project is created");
    store
        .create_mini_project(&NewMiniProject {
            id: mini_project_id,
            project_id,
            name: name("Advisor epic"),
            created_at,
        })
        .expect("the epic is created");

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
                custom_display_name: Some(name("Kontor LSA")),
            },
            task_id: None,
            team_run_id: None,
            attach_deadline: at("2026-09-01T12:10:00Z"),
            parent_seat_binding_id: None,
            created_at,
        })
        .expect("the caller seat is bound");

    let profile = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "seats": ["SA", "AUD"],
    }))
    .expect("a canonical profile");
    store
        .publish_consultation_profile_revision(&StoredConsultationProfileRevision {
            project_id,
            family: ConsultationFamily::Advisor,
            profile_id: PROFILE.to_owned(),
            version: SpecVersion::FIRST,
            name: name("Two-seat advisor"),
            definition: profile.json().to_owned(),
            definition_hash: profile.hash().clone(),
            published_at: created_at,
        })
        .expect("the advisor profile publishes");

    let asw = TopologyNodeId::generate();
    let run_id = ConsultationRunId::Advisor(AdvisorRunId::generate());
    let question = BoundedText::parse("Is the alias cleanup a naming prerequisite?")
        .expect("a bounded question");
    let context = serde_json::json!({ "schema_version": 1 });
    let context_hash = CanonicalDocument::from_serializable(&context)
        .expect("canonical context")
        .hash()
        .clone();
    let run = StoredConsultationRun {
        id: run_id,
        project_id,
        mini_project_id,
        topic: Some(name("Alias cleanup")),
        profile_id: PROFILE.to_owned(),
        profile_version: SpecVersion::FIRST,
        definition_hash: profile.hash().clone(),
        question_hash: ContentHash::of(question.as_str().as_bytes()),
        question,
        context,
        context_hash,
        caller_seat_binding_id: caller,
        topology_node_id: asw,
        invoke_key: IdempotencyKey::parse("invoke-two-seat-advisor").expect("a key"),
        invoke_intent_hash: ContentHash::of(b"invoke"),
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
        provider: ProviderRef("claude-work".to_owned()),
        model: ModelRef("claude-opus-5".to_owned()),
        effort: None,
    };
    let bindings: Vec<(SeatBindingId, &str, &str)> = vec![
        (SeatBindingId::generate(), "advisor-a", "SA"),
        (SeatBindingId::generate(), "advisor-b", "AUD"),
    ];
    let seat_bindings: Vec<NewSeatBinding> = bindings
        .iter()
        .map(|(id, slot, code)| {
            let entry = catalog
                .role(&RoleCode::parse(code).expect("a role code"))
                .expect("the catalog has the role");
            NewSeatBinding {
                id: *id,
                project_id,
                topology_node_id: asw,
                role_slot_id: RoleSlotId::parse(slot).expect("a slot"),
                role: CatalogRoleRef {
                    catalog_id: catalog.catalog_id,
                    catalog_revision: catalog.version,
                    role_code: entry.role_code.clone(),
                    standard_title: entry.standard_title.clone(),
                    custom_display_name: None,
                },
                task_id: None,
                team_run_id: None,
                attach_deadline: at("2026-09-01T12:10:00Z"),
                parent_seat_binding_id: None,
                created_at,
            }
        })
        .collect();
    let seats: Vec<StoredConsultationSeat> = bindings
        .iter()
        .map(|(id, slot, _)| StoredConsultationSeat {
            run_id,
            role_slot_id: RoleSlotId::parse(slot).expect("a slot"),
            committee_role: None,
            logical_role: RoleKey::parse("advisor").expect("a logical role"),
            seat_binding_id: *id,
            model_rung: rung.clone(),
            occupancy_generation: 1,
            native_identity: None,
            provider_session_id: None,
            observed_at: None,
        })
        .collect();
    store
        .create_consultation_run(
            &run,
            &node(asw, "ASW", Some(esw), Some(mini_project_id)),
            &seats.iter().zip(seat_bindings.iter()).collect::<Vec<_>>(),
        )
        .expect("the consultation and both seats are frozen");

    // Attest both seats: advice requires its own attested seat.
    for (index, seat) in seats.iter().enumerate() {
        let mut attested = seat.clone();
        attested.native_identity = Some(NativeRuntimeIdentity {
            runtime_kind: RuntimeKindKey::parse("paseo").expect("a runtime kind"),
            host: name("localhost"),
            generation: 1,
            native_id: ExternalId::parse(&format!("agent_advisor_{index}")).expect("a native id"),
        });
        attested.provider_session_id =
            Some(ExternalId::parse(&format!("session_{index}")).expect("a session id"));
        attested.observed_at = Some(created_at);
        store
            .bind_consultation_seat(project_id, &attested)
            .expect("the seat is attested");
    }

    let run = store
        .advance_consultation_run(
            project_id,
            run_id,
            AggregateRevision::INITIAL,
            ConsultationRunState::Running,
            None,
            at("2026-09-01T12:20:00Z"),
        )
        .expect("the run starts running");

    World {
        _home: home,
        store,
        project_id,
        run,
        seats: [bindings[0].0, bindings[1].0],
    }
}

fn advice(marker: &str) -> (serde_json::Value, ContentHash) {
    let document = CanonicalDocument::from_serializable(&serde_json::json!({
        "schema_version": 1,
        "finding": marker,
    }))
    .expect("canonical advice");
    (
        serde_json::from_str(document.json()).expect("advice value"),
        document.hash().clone(),
    )
}

#[test]
fn two_advisor_seats_each_record_their_own_advice_without_settling_the_run() {
    let w = world();
    let run_id = match w.run.id {
        ConsultationRunId::Advisor(id) => id,
        ConsultationRunId::Committee(_) => unreachable!("the fixture builds an Advisor run"),
    };

    let (first, first_hash) = advice("seat-a");
    let (run, inserted) = w
        .store
        .record_advisor_advice(
            w.project_id,
            run_id,
            w.seats[0],
            &first,
            &first_hash,
            w.run.revision,
            at("2026-09-01T13:00:00Z"),
        )
        .expect("the first seat reports");
    assert!(inserted);
    assert_eq!(
        run.state,
        ConsultationRunState::Running,
        "one seat reporting does not settle a run that has another seat to hear from"
    );
    assert!(run.result.is_none(), "no aggregate verdict exists yet");

    // The second seat is a different artifact, not a conflict.
    let (second, second_hash) = advice("seat-b");
    let (run, inserted) = w
        .store
        .record_advisor_advice(
            w.project_id,
            run_id,
            w.seats[1],
            &second,
            &second_hash,
            run.revision,
            at("2026-09-01T13:01:00Z"),
        )
        .expect("the second seat reports independently");
    assert!(inserted);
    assert_eq!(run.state, ConsultationRunState::Running);

    let recorded = w
        .store
        .list_advisor_advice(w.project_id, run_id)
        .expect("the advice lists");
    assert_eq!(recorded.len(), 2, "both seats' advice survives");
    let mut seats: Vec<_> = recorded.iter().map(|entry| entry.seat_binding_id).collect();
    seats.sort();
    let mut expected = w.seats.to_vec();
    expected.sort();
    assert_eq!(seats, expected);

    // "The advice of the run" is no longer a single artifact, and the single
    // getter refuses rather than silently reporting one of the two.
    assert!(
        w.store.get_advisor_advice(w.project_id, run_id).is_err(),
        "a multi-seat run has no single advice artifact"
    );
}

#[test]
fn advice_is_idempotent_per_exact_seat_and_immutable_per_seat() {
    let w = world();
    let run_id = match w.run.id {
        ConsultationRunId::Advisor(id) => id,
        ConsultationRunId::Committee(_) => unreachable!("the fixture builds an Advisor run"),
    };
    let (document, hash) = advice("seat-a");
    let (run, inserted) = w
        .store
        .record_advisor_advice(
            w.project_id,
            run_id,
            w.seats[0],
            &document,
            &hash,
            w.run.revision,
            at("2026-09-01T13:00:00Z"),
        )
        .expect("the seat reports");
    assert!(inserted);

    // The same seat submitting the same bytes is a replay: no second artifact,
    // and the run does not advance again.
    let (replayed, inserted) = w
        .store
        .record_advisor_advice(
            w.project_id,
            run_id,
            w.seats[0],
            &document,
            &hash,
            run.revision,
            at("2026-09-01T13:02:00Z"),
        )
        .expect("the same seat replays");
    assert!(!inserted, "a replay records nothing new");
    assert_eq!(replayed.revision, run.revision);
    assert_eq!(
        w.store
            .list_advisor_advice(w.project_id, run_id)
            .expect("the advice lists")
            .len(),
        1
    );

    // The same seat cannot change what it said.
    let (different, different_hash) = advice("seat-a-changed-its-mind");
    assert!(
        w.store
            .record_advisor_advice(
                w.project_id,
                run_id,
                w.seats[0],
                &different,
                &different_hash,
                run.revision,
                at("2026-09-01T13:03:00Z"),
            )
            .is_err(),
        "advice that was given cannot be edited"
    );
}
