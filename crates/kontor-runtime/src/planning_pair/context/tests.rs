//! Hypothetical pure fixtures: plain facts, as if a service had read them.
//! The daemon's loopback tests prove the trusted path that actually reads them.

use super::*;
use kontor_core::consultation::ConsultationRunState;
use kontor_core::id::{
    AdvisorRunId, AggregateRevision, BoundedText, IdempotencyKey, MiniProjectId,
    PlanningPairProfileId, ProjectId, RoleCatalogId, RoleKey, RoleSlotId, SeatBindingId,
    SpecVersion, TeamDefinitionId, TopologyNodeId, TopologySpecId,
};
use kontor_core::spec::{ModelRef, ModelRung, ProviderRef};

fn id(tail: &str) -> String {
    format!("01991c00-0000-7000-8000-0000000000{tail}")
}

fn at() -> kontor_core::id::Timestamp {
    "2026-10-02T10:00:00Z".parse().expect("an instant")
}

fn route(provider: &str, model: &str) -> ModelRung {
    ModelRung {
        provider: ProviderRef(provider.to_owned()),
        model: ModelRef(model.to_owned()),
        effort: None,
    }
}

fn pin() -> PlanningPairPin {
    PlanningPairPin {
        profile_id: PlanningPairProfileId::parse(&id("b1")).expect("a profile id"),
        version: SpecVersion::FIRST,
        definition_hash: ContentHash::of(b"planning pair document"),
    }
}

fn placement() -> PlanningPairMembers {
    let member = |slot: PlanningPairSlot, route: ModelRung, vendor: &str| PlanningPairMember {
        slot,
        binding_key: format!("pair/{}", slot.as_str()),
        route,
        vendor: vendor.to_owned(),
    };
    PlanningPairMembers::freeze(
        ContentHash::of(b"placement"),
        vec![
            member(
                PlanningPairSlot::SeatA,
                route("codex-work", "gpt-6.1-sol"),
                "openai",
            ),
            member(
                PlanningPairSlot::SeatB,
                route("claude-work", "claude-opus-5"),
                "anthropic",
            ),
        ],
    )
    .expect("two members on distinct vendors")
}

fn team_definition() -> TeamDefinitionSnapshot {
    TeamDefinitionSnapshot {
        definition_id: TeamDefinitionId::parse(&id("d2")).expect("a definition id"),
        version: SpecVersion::FIRST,
        canonical_hash: ContentHash::of(b"team definition"),
    }
}

fn topology() -> TopologySnapshot {
    TopologySnapshot {
        spec_id: TopologySpecId::parse(&id("c2")).expect("a topology id"),
        version: SpecVersion::FIRST,
        canonical_hash: ContentHash::of(b"topology"),
    }
}

/// A planning pair run frozen under [`pin`], [`placement`] and
/// [`team_definition`].
fn run() -> StoredConsultationRun {
    let pin = pin();
    let team = team_definition();
    StoredConsultationRun {
        id: ConsultationRunId::PlanningPair(PlanningPairRunId::parse(&id("a9")).expect("a run id")),
        project_id: ProjectId::parse(&id("f1")).expect("a project id"),
        mini_project_id: MiniProjectId::parse(&id("e1")).expect("an epic id"),
        profile_id: pin.profile_id.to_string(),
        profile_version: pin.version,
        definition_hash: pin.definition_hash,
        semantic_identity_hash: None,
        subject: None,
        topic: None,
        question: BoundedText::parse("Is this plan the smallest sound next step?")
            .expect("a question"),
        question_hash: ContentHash::of(b"question"),
        context: serde_json::json!({
            "placement_hash": placement().placement_hash().as_str(),
            "team_definition_id": team.definition_id.to_string(),
            "team_definition_version": team.version.get(),
            "team_definition_hash": team.canonical_hash.as_str(),
        }),
        context_hash: ContentHash::of(b"context"),
        caller_seat_binding_id: SeatBindingId::parse(&id("a1")).expect("a caller"),
        topology_node_id: TopologyNodeId::parse(&id("c1")).expect("a node"),
        invoke_key: IdempotencyKey::parse("pair-invoke-1").expect("a key"),
        invoke_intent_hash: ContentHash::of(b"intent"),
        state: ConsultationRunState::Running,
        round: 1,
        result: None,
        result_hash: None,
        revision: AggregateRevision::INITIAL,
        created_at: at(),
        updated_at: at(),
        settled_at: None,
    }
}

/// The frozen `seat-b` member seat at generation 2.
fn seat() -> StoredConsultationSeat {
    StoredConsultationSeat {
        run_id: run().id,
        role_slot_id: RoleSlotId::parse("seat-b").expect("a slot"),
        committee_role: None,
        logical_role: RoleKey::parse("sa").expect("a role"),
        seat_binding_id: SeatBindingId::parse(&id("a3")).expect("a member seat"),
        model_rung: route("claude-work", "claude-opus-5"),
        occupancy_generation: 2,
        native_identity: None,
        provider_session_id: None,
        observed_at: None,
    }
}

fn node() -> SessionTopologyNode {
    serde_json::from_value(serde_json::json!({
        "id": id("c3"),
        "project_id": id("f1"),
        "mini_project_id": id("e1"),
        "topology": topology(),
        "kind": "PPW",
        "lifecycle": "active",
        "placement": "bound",
        "revision": 1,
        "created_at": "2026-10-02T10:00:00Z",
        "updated_at": "2026-10-02T10:00:00Z",
    }))
    .expect("a member container")
}

fn catalog(roles: serde_json::Value) -> RoleCatalogRevision {
    serde_json::from_value(serde_json::json!({
        "schema_version": 1,
        "catalog_id": id("d1"),
        "version": 1,
        "name": "Standard roles",
        "roles": roles,
    }))
    .expect("a role catalog")
}

fn roles() -> serde_json::Value {
    serde_json::json!([{
        "role_code": "LSA",
        "standard_title": "Lead Solution Architect",
        "segment": "architecture",
        "responsibility_summary": "Owns the epic's architecture.",
        "lifecycle": "current",
    }])
}

fn provenance() -> FleetLaunchProvenance {
    FleetLaunchProvenance {
        policy_hash: ContentHash::of(b"activated policy"),
        source_bundle_hash: Some(ContentHash::of(b"bundle")),
        binding_key: "pair/seat-b".to_owned(),
        chain: "pair".to_owned(),
        step: 1,
        sub_step: 2,
        vendor: "anthropic".to_owned(),
        eligibility: Some(LaunchEligibility::default()),
    }
}

fn cwd() -> WorkspaceRoot {
    WorkspaceRoot::parse("/realm/pair").expect("a member cwd")
}

/// The `seat-b` context, derived through every stage from the fixtures.
fn context() -> PlanningPairLaunchContext {
    let (run, pin, members, seat) = (run(), pin(), placement(), seat());
    let member = frozen_member(
        &run,
        &pin,
        &members,
        members.placement_hash(),
        &seat,
        PlanningPairSlot::SeatB,
    )
    .expect("the frozen member");
    let team = member
        .require_team_definition(Some(team_definition()))
        .expect("the pinned team definition");
    require_container_topology(&topology(), &node()).expect("the pinned topology");
    member
        .into_context(
            team,
            &node(),
            &cwd(),
            &catalog(roles()),
            Some(&provenance()),
        )
        .expect("a context")
}

fn refusal_of(
    run: &StoredConsultationRun,
    pin: &PlanningPairPin,
    seat: &StoredConsultationSeat,
) -> Option<ContextRefusal> {
    let members = placement();
    frozen_member(
        run,
        pin,
        &members,
        members.placement_hash(),
        seat,
        PlanningPairSlot::SeatB,
    )
    .err()
}

#[test]
fn a_member_is_held_to_its_frozen_run_pin_and_placement_in_order() {
    assert_eq!(refusal_of(&run(), &pin(), &seat()), None);

    let mut advisor = run();
    advisor.id = ConsultationRunId::Advisor(AdvisorRunId::generate());
    advisor.definition_hash = ContentHash::of(b"another document");
    assert_eq!(
        refusal_of(&advisor, &pin(), &seat()),
        Some(ContextRefusal::NotPlanningPair)
    );

    let mut other_profile = pin();
    other_profile.profile_id = PlanningPairProfileId::generate();
    let mut other_version = pin();
    other_version.version = other_version.version.next().expect("a version");
    let mut other_hash = pin();
    other_hash.definition_hash = ContentHash::of(b"another document");
    for other in [other_profile, other_version, other_hash] {
        let mut rerouted = seat();
        rerouted.model_rung = route("codex-work", "gpt-6.1-sol");
        assert_eq!(
            refusal_of(&run(), &other, &rerouted),
            Some(ContextRefusal::DocumentPinDiffers),
            "{other:?}"
        );
    }

    let mut rerouted = seat();
    rerouted.model_rung = route("claude-work", "claude-opus-4.8");
    rerouted.occupancy_generation = 0;
    assert_eq!(
        refusal_of(&run(), &pin(), &rerouted),
        Some(ContextRefusal::RouteDiffers)
    );

    let mut unoccupied = seat();
    unoccupied.occupancy_generation = 0;
    let mut elsewhere = run();
    elsewhere.context["placement_hash"] = serde_json::json!(ContentHash::of(b"other").as_str());
    assert_eq!(
        refusal_of(&elsewhere, &pin(), &unoccupied),
        Some(ContextRefusal::NoOccupancyGeneration)
    );
    assert_eq!(
        refusal_of(&elsewhere, &pin(), &seat()),
        Some(ContextRefusal::PlacementDiffers)
    );
    let mut unplaced = run();
    unplaced.context = serde_json::json!({});
    assert_eq!(
        refusal_of(&unplaced, &pin(), &seat()),
        Some(ContextRefusal::PlacementDiffers)
    );
}

#[test]
fn the_epics_pinned_team_definition_is_the_frozen_one() {
    let (run, pin, members, seat) = (run(), pin(), placement(), seat());
    let member = frozen_member(
        &run,
        &pin,
        &members,
        members.placement_hash(),
        &seat,
        PlanningPairSlot::SeatB,
    )
    .expect("the frozen member");
    assert_eq!(
        member.require_team_definition(Some(team_definition())),
        Ok(team_definition())
    );
    assert_eq!(
        member.require_team_definition(None),
        Err(ContextRefusal::NoPinnedTeamDefinition)
    );
    let mut other_id = team_definition();
    other_id.definition_id = TeamDefinitionId::generate();
    let mut other_version = team_definition();
    other_version.version = other_version.version.next().expect("a version");
    let mut other_hash = team_definition();
    other_hash.canonical_hash = ContentHash::of(b"another team definition");
    for other in [other_id, other_version, other_hash] {
        assert_eq!(
            member.require_team_definition(Some(other.clone())),
            Err(ContextRefusal::TeamDefinitionDiffers),
            "{other:?}"
        );
    }
}

#[test]
fn the_member_container_runs_the_epics_pinned_topology() {
    assert_eq!(require_container_topology(&topology(), &node()), Ok(()));
    let mut other_spec = topology();
    other_spec.spec_id = TopologySpecId::generate();
    let mut other_version = topology();
    other_version.version = other_version.version.next().expect("a version");
    let mut other_hash = topology();
    other_hash.canonical_hash = ContentHash::of(b"another topology");
    for other in [other_spec, other_version, other_hash] {
        assert_eq!(
            require_container_topology(&other, &node()),
            Err(ContextRefusal::TopologyDiffers),
            "{other:?}"
        );
    }
}

#[test]
fn the_context_is_built_from_exactly_the_facts_read() {
    let context = context();
    let seat = seat();
    let catalog = catalog(roles());
    assert_eq!(context.run_id.to_string(), id("a9"));
    assert_eq!(context.seat_binding_id, seat.seat_binding_id);
    assert_eq!(context.slot, PlanningPairSlot::SeatB);
    assert_eq!(context.occupancy_generation, 2);
    assert_eq!(context.profile, pin());
    assert_eq!(context.topology, node().topology);
    assert_eq!(context.team_definition, team_definition());
    assert_eq!(
        context.role_catalog,
        PlanningPairCatalogPin {
            catalog_id: catalog.catalog_id,
            version: catalog.version,
            canonical_hash: catalog.canonicalize().expect("a catalog").hash().clone(),
        }
    );
    assert_eq!(context.topology_node_id, node().id);
    assert_eq!(context.cwd, cwd());
    assert_eq!(context.route, seat.model_rung);
    assert_eq!(context.vendor, "anthropic");
    assert_eq!(&context.placement_hash, placement().placement_hash());
    assert_eq!(context.requested_fleet_provenance, provenance());
}

#[test]
fn the_build_refuses_an_invalid_catalog_then_a_missing_provenance() {
    let (run, pin, members, seat) = (run(), pin(), placement(), seat());
    let member = frozen_member(
        &run,
        &pin,
        &members,
        members.placement_hash(),
        &seat,
        PlanningPairSlot::SeatB,
    )
    .expect("the frozen member");
    let build = |catalog: &RoleCatalogRevision, requested: Option<&FleetLaunchProvenance>| {
        member
            .into_context(team_definition(), &node(), &cwd(), catalog, requested)
            .err()
    };
    let empty = catalog(serde_json::json!([]));
    assert!(
        matches!(build(&empty, None), Some(ContextBuildError::Domain(_))),
        "the catalog is canonicalized before the provenance is required"
    );
    assert_eq!(
        build(&catalog(roles()), None),
        Some(ContextBuildError::Refused(
            ContextRefusal::NoFleetProvenance
        ))
    );
}

/// The daemon's `member_context_hash` document at `be9537aa`, copied
/// verbatim: the moved hash names exactly these canonical bytes.
fn baseline_document(context: &PlanningPairLaunchContext) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "planning_pair_run_id": context.run_id.to_string(),
        "seat_binding_id": context.seat_binding_id.to_string(),
        "slot": context.slot.as_str(),
        "occupancy_generation": context.occupancy_generation,
        "profile": [
            context.profile.profile_id.to_string(),
            context.profile.version.get(),
            context.profile.definition_hash.as_str(),
        ],
        "topology": [
            context.topology.spec_id.to_string(),
            context.topology.version.get(),
            context.topology.canonical_hash.as_str(),
        ],
        "team_definition": [
            context.team_definition.definition_id.to_string(),
            context.team_definition.version.get(),
            context.team_definition.canonical_hash.as_str(),
        ],
        "role_catalog": [
            context.role_catalog.catalog_id.to_string(),
            context.role_catalog.version.get(),
            context.role_catalog.canonical_hash.as_str(),
        ],
        "topology_node_id": context.topology_node_id.to_string(),
        "cwd": context.cwd.as_str(),
        "route": context.route,
        "vendor": context.vendor,
        "placement_hash": context.placement_hash.as_str(),
        "requested_fleet_provenance": context.requested_fleet_provenance,
    })
}

fn baseline_hash(context: &PlanningPairLaunchContext) -> ContentHash {
    CanonicalDocument::from_value(&baseline_document(context))
        .expect("a canonical document")
        .hash()
        .clone()
}

/// Every field reaches the hash, and the hash is the baseline's.
#[test]
fn the_frozen_hash_is_the_baseline_document_over_every_field() {
    let context = context();
    let frozen = context.frozen_hash().expect("a hash");
    assert_eq!(frozen, baseline_hash(&context));
    assert_eq!(
        frozen.as_str(),
        "4a6fcad2a061f25b45a069af834943fa9350016a1e11072cd5d93c9c3c07ce34",
        "the golden of the fixed fixtures"
    );

    let edits: [fn(&mut PlanningPairLaunchContext); 16] = [
        |c| c.run_id = PlanningPairRunId::generate(),
        |c| c.seat_binding_id = SeatBindingId::generate(),
        |c| c.slot = PlanningPairSlot::SeatA,
        |c| c.occupancy_generation += 1,
        |c| c.profile.definition_hash = ContentHash::of(b"x"),
        |c| c.topology.version = c.topology.version.next().expect("a version"),
        |c| c.team_definition.canonical_hash = ContentHash::of(b"x"),
        |c| c.role_catalog.catalog_id = RoleCatalogId::generate(),
        |c| c.topology_node_id = TopologyNodeId::generate(),
        |c| c.cwd = WorkspaceRoot::parse("/realm/other").expect("a cwd"),
        |c| c.route.model = ModelRef("claude-opus-4.8".to_owned()),
        |c| c.vendor = "openai".to_owned(),
        |c| c.placement_hash = ContentHash::of(b"x"),
        |c| c.requested_fleet_provenance.step += 1,
        |c| c.profile.profile_id = PlanningPairProfileId::generate(),
        |c| c.team_definition.definition_id = TeamDefinitionId::generate(),
    ];
    for (index, edit) in edits.into_iter().enumerate() {
        let mut changed = context.clone();
        edit(&mut changed);
        let hash = changed.frozen_hash().expect("a hash");
        assert_ne!(hash, frozen, "edit {index} reaches the hash");
        assert_eq!(
            hash,
            baseline_hash(&changed),
            "edit {index} stays the baseline"
        );
    }
}

fn receipt() -> serde_json::Value {
    serde_json::json!({"selection": {
        "provenance": {
            "policy_hash": ContentHash::of(b"activated policy").as_str(),
            "source_bundle_hash": ContentHash::of(b"bundle").as_str(),
        },
        "slots": [
            {"slot_id": "seat-a", "binding_key": "pair/seat-a", "chain": "pair",
             "selected": {"step": 1, "sub_step": 1, "vendor": "openai"}},
            {"slot_id": "seat-b", "binding_key": "pair/seat-b", "chain": "pair",
             "selected": {"step": 1, "sub_step": 2, "vendor": "anthropic"},
             "eligibility": {"unavailable_accounts": [], "excluded_vendors": []}},
        ],
    }})
}

#[test]
fn the_requested_provenance_is_read_from_the_frozen_receipt_per_slot() {
    assert_eq!(
        requested_fleet_provenance(&receipt(), PlanningPairSlot::SeatB),
        Some(provenance())
    );
    let seat_a =
        requested_fleet_provenance(&receipt(), PlanningPairSlot::SeatA).expect("the seat-a member");
    assert_eq!(
        (
            seat_a.binding_key.as_str(),
            seat_a.sub_step,
            seat_a.vendor.as_str()
        ),
        ("pair/seat-a", 1, "openai")
    );
    assert_eq!(seat_a.eligibility, Some(LaunchEligibility::default()));

    let mut listed = receipt();
    listed["selection"]["slots"][1]["eligibility"] = serde_json::json!({
        "unavailable_accounts": ["codex-b", 7, "codex-a"],
        "excluded_vendors": ["openai"],
    });
    let listed = requested_fleet_provenance(&listed, PlanningPairSlot::SeatB)
        .and_then(|provenance| provenance.eligibility)
        .expect("an eligibility");
    assert_eq!(
        listed.unavailable_accounts.into_iter().collect::<Vec<_>>(),
        ["codex-a", "codex-b"]
    );
    assert_eq!(
        listed.excluded_vendors.into_iter().collect::<Vec<_>>(),
        ["openai"]
    );

    let mut unbundled = receipt();
    unbundled["selection"]["provenance"]["source_bundle_hash"] = serde_json::json!("not a hash");
    assert_eq!(
        requested_fleet_provenance(&unbundled, PlanningPairSlot::SeatB)
            .map(|provenance| provenance.source_bundle_hash),
        Some(None),
        "an unreadable bundle hash is absent, not a refusal"
    );
}

#[test]
fn a_receipt_without_one_required_fact_names_no_provenance() {
    let edits: [fn(&mut serde_json::Value); 9] = [
        |r| r["selection"]["slots"] = serde_json::json!([r["selection"]["slots"][0].clone()]),
        |r| r["selection"]["provenance"] = serde_json::Value::Null,
        |r| r["selection"]["provenance"]["policy_hash"] = serde_json::json!("not a hash"),
        |r| r["selection"]["slots"][1]["binding_key"] = serde_json::Value::Null,
        |r| r["selection"]["slots"][1]["chain"] = serde_json::Value::Null,
        |r| r["selection"]["slots"][1]["selected"]["step"] = serde_json::json!(65_536),
        |r| r["selection"]["slots"][1]["selected"]["sub_step"] = serde_json::json!(-1),
        |r| r["selection"]["slots"][1]["selected"]["vendor"] = serde_json::Value::Null,
        |r| r["selection"]["slots"][1]["selected"] = serde_json::Value::Null,
    ];
    for (index, edit) in edits.into_iter().enumerate() {
        let mut broken = receipt();
        edit(&mut broken);
        assert_eq!(
            requested_fleet_provenance(&broken, PlanningPairSlot::SeatB),
            None,
            "edit {index}"
        );
    }
    assert_eq!(
        requested_fleet_provenance(&serde_json::json!({}), PlanningPairSlot::SeatB),
        None
    );
}
