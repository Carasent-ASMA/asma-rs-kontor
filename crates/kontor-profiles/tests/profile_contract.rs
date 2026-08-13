//! Profile packs: composition, arbitrary graphs, revision immutability, persona
//! safety and multi-discipline closure.
//!
//! The mutants this suite exists to kill:
//!
//! * accepting a forward phase cycle, or a rejection route that is not a strict
//!   ancestor;
//! * skipping a pinned cross-reference check, so a dangling role, skill, context,
//!   team or profile revision resolves anyway;
//! * consuming an artifact before the phase that produces it can run;
//! * letting a simulated persona evaluate or waive the gate it is under test for,
//!   through the profile *or* through the pinned team;
//! * treating a coding verdict as sufficient to close a profile that declares
//!   design, functionality-QA, design-QA and audit obligations;
//! * accepting a waived gate with no named, authorized, evidence-bearing waiver;
//! * rewriting a published revision instead of publishing the next one;
//! * branching generic behavior on a bundled seed id — proved both by running an
//!   unrelated pack through the same entry points and by scanning the shipped
//!   source for the seed vocabulary.

use std::collections::{BTreeMap, BTreeSet};

use kontor_core::DomainError;
use kontor_core::id::{
    AgentRunId, AggregateRevision, ArtifactKey, ContentHash, EventCursor, GateKey, PhaseKey,
    ProjectId, RoleKey, SCHEMA_VERSION, SpecVersion, TeamRunId, Timestamp, WorkProfileKey,
    parse_utc_timestamp,
};
use kontor_core::repository::AgentRun;
use kontor_core::spec::{
    ArtifactContentType, PersonaScenarioSpec, ResolvedWorkProfileSnapshot, TeamRunSnapshot,
    WorkProfileSpec,
};
use kontor_core::state::{
    DerivedRunState, DesiredRunState, GateState, ObservedRunState, RunLifecycle, RunProjection,
    TerminalEvidence, TerminalEvidenceSource, TerminalOutcome,
};
use kontor_profiles::pack::{
    GateWaiver, PackAvailability, PackCategoryKey, ProfilePackSpec, ResolvedProfileBundle,
    TaskTeamEvidence, certify_task_closure, parse_pack, resolve_profile, revise_persona_scenario,
    revise_work_profile, validate_pack,
};
use kontor_profiles::seeds::bundled_pack;
use kontor_teams::run::{TeamClosureCertificate, TeamRunLease, TeamRunSlots};
use kontor_teams::spec::TeamTemplateSpec;

const CUSTOM_A: &str = include_str!("fixtures/custom-pack-a.json");
const CUSTOM_B: &str = include_str!("fixtures/custom-pack-b.json");

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn at(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical UTC timestamp")
}

fn now() -> Timestamp {
    at("2026-08-10T09:00:00Z")
}

fn seeds() -> ProfilePackSpec {
    bundled_pack().expect("the bundled pack loads and validates")
}

fn custom_a() -> ProfilePackSpec {
    parse_pack(CUSTOM_A).expect("custom pack A loads and validates")
}

fn custom_b() -> ProfilePackSpec {
    parse_pack(CUSTOM_B).expect("custom pack B loads and validates")
}

/// Every phase of a profile, in a topological order of its forward edges.
///
/// Building the order from the edges rather than from the declaration order is
/// what makes the walk a real check: a profile whose phases are declared in a
/// scrambled order still walks correctly, and a cycle would make this loop.
fn topological_phases(profile: &WorkProfileSpec) -> Vec<PhaseKey> {
    let mut indegree: BTreeMap<&PhaseKey, usize> =
        profile.phases.iter().map(|phase| (&phase.id, 0)).collect();
    for edge in &profile.edges {
        if let Some(degree) = indegree.get_mut(&edge.to) {
            *degree += 1;
        }
    }
    let mut ready: Vec<&PhaseKey> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(phase, _)| *phase)
        .collect();
    let mut order = Vec::new();
    while let Some(phase) = ready.pop() {
        order.push(phase.clone());
        for edge in profile.edges.iter().filter(|edge| &edge.from == phase) {
            if let Some(degree) = indegree.get_mut(&edge.to) {
                *degree -= 1;
                if *degree == 0 {
                    ready.push(&edge.to);
                }
            }
        }
    }
    assert_eq!(
        order.len(),
        profile.phases.len(),
        "a validated profile has no forward cycle"
    );
    order
}

/// Everything a profile declares, satisfied in full.
fn fully_satisfied(
    profile: &WorkProfileSpec,
) -> (
    BTreeSet<PhaseKey>,
    BTreeMap<GateKey, GateState>,
    BTreeSet<ArtifactKey>,
) {
    (
        topological_phases(profile).into_iter().collect(),
        profile
            .gates
            .iter()
            .map(|gate| (gate.id.clone(), GateState::Passed))
            .collect(),
        profile
            .artifacts
            .iter()
            .map(|contract| contract.key.clone())
            .collect(),
    )
}

/// The runnable category whose profile declares the most gates.
///
/// Picking the profile *structurally* is the point: the multi-discipline rule
/// under test is a consequence of a profile's own declared obligations, so the
/// test must not find it by name either.
fn most_gated(pack: &ProfilePackSpec) -> (PackCategoryKey, WorkProfileSpec) {
    pack.manifest
        .iter()
        .filter(|entry| entry.availability == PackAvailability::Seeded)
        .filter_map(|entry| {
            let id = entry.profile.as_ref()?;
            let version = entry.profile_version?;
            let profile = pack.profile(id, version)?;
            Some((entry.category.clone(), profile.clone()))
        })
        .max_by_key(|(_, profile)| profile.gates.len())
        .expect("the pack advertises at least one runnable category")
}

/// A real team closure certificate for `bundle`: every declared seat run once
/// and closed with evidence.
///
/// The certificate is obtained the only way it can be — from
/// `certify_team_closure` over a hydrated roster — so a task test cannot fake
/// the team half of closure any more than production can.
fn team_closure(bundle: &ResolvedProfileBundle) -> (TeamRunId, TeamClosureCertificate) {
    let revision = bundle.team.clone().expect("the profile pins a team");
    let team = TeamTemplateSpec::from_revision(&revision).expect("the team reads back");
    let snapshot = TeamRunSnapshot::from_revision(&revision, SCHEMA_VERSION);
    let team_run_id = TeamRunId::generate();

    let rows: Vec<AgentRun> = team
        .slots
        .iter()
        .map(|declared| closed_run(team_run_id, declared.id.as_role_key()))
        .collect();

    let lease = TeamRunLease::acquire(team_run_id).expect("the test is the only writer");
    let certificate = TeamRunSlots::hydrate(lease, &snapshot, &rows, &[])
        .expect("a complete roster hydrates")
        .certify_team_closure(&[])
        .expect("every declared seat closed");
    (team_run_id, certificate)
}

/// One closed attempt at one seat.
fn closed_run(team_run_id: TeamRunId, role: &RoleKey) -> AgentRun {
    let id = AgentRunId::generate();
    AgentRun {
        id,
        project_id: ProjectId::generate(),
        team_run_id,
        parent_agent_run_id: None,
        role: role.clone(),
        account_profile_id: None,
        binding: None,
        projection: RunProjection {
            lifecycle: RunLifecycle::Succeeded,
            desired: DesiredRunState::RunRequested,
            observed: ObservedRunState::Succeeded,
            derived: DerivedRunState::Terminal {
                outcome: TerminalOutcome::Succeeded,
            },
            last_confirmed_at: Some(now()),
            last_cursor: None,
        },
        terminal: Some(TerminalEvidence {
            outcome: TerminalOutcome::Succeeded,
            source: TerminalEvidenceSource::RuntimeObservation {
                cursor: EventCursor::parse(7).expect("a positive cursor"),
            },
            evidence_hash: ContentHash::of(id.to_string().as_bytes()),
            closed_at: now(),
        }),
        revision: AggregateRevision::INITIAL,
        created_at: now(),
        closed_at: Some(now()),
    }
}

// ---------------------------------------------------------------------------
// The bundled pack
// ---------------------------------------------------------------------------

#[test]
fn every_runnable_category_resolves_and_every_manifest_only_category_does_not() {
    let pack = seeds();
    validate_pack(&pack).expect("the bundled pack validates");

    let runnable = pack.runnable_categories().len();
    assert!(
        runnable >= 4,
        "the pack ships the seeded profile categories"
    );
    let advertised_only = pack.manifest.len() - runnable;
    assert!(
        advertised_only >= 10,
        "the pack advertises discovery vocabulary it does not claim to run"
    );

    for entry in &pack.manifest {
        let resolved = resolve_profile(&pack, &entry.category, now());
        match entry.availability {
            PackAvailability::Seeded => {
                let bundle = resolved.unwrap_or_else(|error| {
                    panic!("a seeded category must resolve: {error}");
                });
                bundle.verify().expect("the bundle verifies");
                assert_eq!(&bundle.category, &entry.category);
                assert!(
                    bundle.team.is_some(),
                    "every seeded profile pins a team it can actually run with"
                );
            }
            PackAvailability::ManifestOnly => {
                assert!(
                    resolved.is_err(),
                    "a manifest-only category must not resolve as a runnable profile"
                );
            }
        }
    }
}

#[test]
fn one_seed_team_declares_a_role_twice_through_two_slots() {
    let pack = seeds();
    let parallel = pack
        .teams
        .iter()
        .find(|team| {
            team.roles
                .iter()
                .any(|requirement| requirement.min_slots == 2 && requirement.max_slots == 2)
        })
        .expect("a seed team declares a role exactly twice");
    let repeated = parallel
        .roles
        .iter()
        .find(|requirement| requirement.min_slots == 2)
        .expect("the repeated requirement");

    let slots = parallel.slots_of(&repeated.role.role);
    assert_eq!(
        slots.len(),
        2,
        "the cardinality is met by two concrete slots"
    );
    assert_ne!(slots[0].id, slots[1].id, "with distinct slot ids");
}

#[test]
fn a_resolved_bundle_owns_every_document_it_pinned() {
    let pack = seeds();
    let (category, profile) = most_gated(&pack);
    let bundle = resolve_profile(&pack, &category, now()).expect("the category resolves");

    for reference in &profile.roles {
        assert!(
            bundle
                .roles
                .iter()
                .any(|definition| definition.role == reference.role
                    && definition.version == reference.version),
            "the bundle carries every role revision the profile pinned"
        );
    }
    let team = bundle.team.as_ref().expect("the profile pinned a team");
    for slot in &kontor_teams::spec::TeamTemplateSpec::from_revision(team)
        .expect("the pinned team revision reads back")
        .slots
    {
        for skill in &slot.skills {
            assert!(
                bundle
                    .skills
                    .iter()
                    .any(|definition| definition.skill == skill.skill
                        && definition.version == skill.version),
                "the bundle carries every skill revision a slot pinned"
            );
        }
    }
    bundle.verify().expect("the bundle verifies");
}

// ---------------------------------------------------------------------------
// Unrelated custom packs (AC-2)
// ---------------------------------------------------------------------------

#[test]
fn two_unrelated_packs_share_no_identifier_with_the_seeds_or_each_other() {
    let seeds = open_ids(&seeds());
    let alpha = open_ids(&custom_a());
    let omega = open_ids(&custom_b());

    assert!(
        seeds.is_disjoint(&alpha),
        "custom pack A reuses no seed identifier"
    );
    assert!(
        seeds.is_disjoint(&omega),
        "custom pack B reuses no seed identifier"
    );
    assert!(
        alpha.is_disjoint(&omega),
        "the two custom packs are unrelated to each other"
    );
}

#[test]
fn the_two_custom_packs_have_genuinely_different_graph_shapes() {
    let alpha = custom_a();
    let omega = custom_b();

    let branched = &alpha.profiles[0];
    let joins = branched
        .phases
        .iter()
        .filter(|phase| {
            branched
                .edges
                .iter()
                .filter(|edge| edge.to == phase.id)
                .count()
                > 1
        })
        .count();
    let forks = branched
        .phases
        .iter()
        .filter(|phase| {
            branched
                .edges
                .iter()
                .filter(|edge| edge.from == phase.id)
                .count()
                > 1
        })
        .count();
    assert!(joins >= 1 && forks >= 1, "pack A branches and joins");

    let linear = &omega.profiles[0];
    assert_eq!(linear.edges.len(), linear.phases.len() - 1);
    assert!(
        linear.phases.iter().all(|phase| {
            linear
                .edges
                .iter()
                .filter(|edge| edge.from == phase.id)
                .count()
                <= 1
        }),
        "pack B is linear"
    );
    assert!(!omega.personas.is_empty(), "pack B carries its own persona");
}

#[test]
fn an_unrelated_pack_walks_its_own_graph_and_closes_with_no_rust_change() {
    for pack in [custom_a(), custom_b()] {
        for category in pack.runnable_categories() {
            let bundle = resolve_profile(&pack, category, now()).expect("it resolves");
            bundle.verify().expect("it verifies");
            let profile = &bundle.profile.definition;

            // Walking the declared order proves the graph is usable, not merely
            // parseable: each phase's artifacts exist by the time it runs.
            let mut produced: BTreeSet<ArtifactKey> = BTreeSet::new();
            let mut completed: BTreeSet<PhaseKey> = BTreeSet::new();
            let mut gates: BTreeMap<GateKey, GateState> = BTreeMap::new();
            for phase in topological_phases(profile) {
                let declared = profile
                    .phases
                    .iter()
                    .find(|candidate| candidate.id == phase)
                    .expect("the phase is declared");
                for contract in profile
                    .artifacts
                    .iter()
                    .filter(|contract| contract.producer_phase == phase)
                {
                    produced.insert(contract.key.clone());
                }
                for required in &declared.required_artifacts {
                    assert!(
                        produced.contains(required),
                        "an artifact is required before anything produced it"
                    );
                }
                for gate in &declared.gates {
                    let spec = profile.gate(gate).expect("the gate is declared");
                    for evidence in &spec.required_evidence {
                        assert!(
                            produced.contains(evidence),
                            "gate evidence is demanded before it can exist"
                        );
                    }
                    gates.insert(gate.clone(), GateState::Passed);
                }
                completed.insert(phase);
            }

            let (team_run_id, certificate) = team_closure(&bundle);
            certify_task_closure(
                &bundle.profile,
                TaskTeamEvidence::Certified {
                    team_run_id,
                    certificate: &certificate,
                },
                &completed,
                &gates,
                &produced,
                &[],
            )
            .expect("an unrelated profile closes on its own declared obligations");
        }
    }
}

#[test]
fn renaming_every_identifier_in_a_custom_pack_changes_nothing_structural() {
    let original = custom_a();
    let renamed: ProfilePackSpec =
        parse_pack(&CUSTOM_A.replace("\"alpha-", "\"gamma-")).expect("the renamed pack validates");

    assert!(
        open_ids(&original).is_disjoint(&open_ids(&renamed)),
        "the rename replaced every identifier"
    );

    let before = resolve_profile(&original, original.runnable_categories()[0], now())
        .expect("the original resolves");
    let after = resolve_profile(&renamed, renamed.runnable_categories()[0], now())
        .expect("so does the copy");

    assert_eq!(
        before.profile.definition.phases.len(),
        after.profile.definition.phases.len(),
        "the same shape survives the rename"
    );
    assert_eq!(
        before.profile.definition.edges.len(),
        after.profile.definition.edges.len()
    );
    assert_ne!(
        before.bundle_hash, after.bundle_hash,
        "different names are different content"
    );
    after.verify().expect("the renamed bundle verifies");
}

/// Every open identifier a pack carries, as text.
fn open_ids(pack: &ProfilePackSpec) -> BTreeSet<String> {
    let mut ids: BTreeSet<String> = BTreeSet::new();
    ids.insert(pack.pack_id.to_string());
    for entry in &pack.manifest {
        ids.insert(entry.category.to_string());
    }
    for definition in &pack.roles {
        ids.insert(definition.role.to_string());
    }
    for definition in &pack.skills {
        ids.insert(definition.skill.to_string());
    }
    for definition in &pack.contexts {
        ids.insert(definition.template.to_string());
    }
    for profile in &pack.profiles {
        ids.insert(profile.id.to_string());
        ids.extend(profile.phases.iter().map(|phase| phase.id.to_string()));
        ids.extend(profile.gates.iter().map(|gate| gate.id.to_string()));
        ids.extend(
            profile
                .artifacts
                .iter()
                .map(|contract| contract.key.to_string()),
        );
        ids.insert(profile.runtime_routing.runtime_kind.to_string());
    }
    for team in &pack.teams {
        ids.extend(team.slots.iter().map(|slot| slot.id.to_string()));
    }
    for persona in &pack.personas {
        ids.insert(persona.scenario.persona.to_string());
        ids.insert(persona.scenario.actor_role.to_string());
    }
    ids
}

#[test]
fn no_shipped_source_file_compares_an_identifier_to_a_seed_name() {
    let vocabulary = open_ids(&seeds());
    let crate_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let sources = [
        crate_root.join("src"),
        crate_root
            .parent()
            .expect("the crate lives in the workspace")
            .join("kontor-teams")
            .join("src"),
    ];

    let mut offenders: Vec<String> = Vec::new();
    for directory in &sources {
        for entry in std::fs::read_dir(directory).expect("the source directory is readable") {
            let path = entry.expect("a readable directory entry").path();
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("the source file is readable");
            for id in &vocabulary {
                // A quoted occurrence is what a comparison or a match arm looks
                // like. Prose in a doc comment spells an id in backticks.
                if text.contains(&format!("\"{id}\"")) {
                    offenders.push(format!("{} mentions \"{id}\"", path.display()));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "production source must not name bundled seed data: {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// Structural validation (AC-3)
// ---------------------------------------------------------------------------

#[test]
fn structurally_invalid_profiles_are_refused() {
    type Case = (&'static str, fn(&mut ProfilePackSpec));
    let cases: &[Case] = &[
        ("a forward cycle", |pack| {
            let profile = &mut pack.profiles[0];
            let first = profile.phases[0].id.clone();
            let last = profile.phases[profile.phases.len() - 1].id.clone();
            profile.edges.push(kontor_core::spec::PhaseEdge {
                from: last,
                to: first,
                handoff_role: None,
            });
        }),
        ("a self edge", |pack| {
            let profile = &mut pack.profiles[0];
            profile.edges[0].to = profile.edges[0].from.clone();
        }),
        ("a duplicate edge", |pack| {
            let profile = &mut pack.profiles[0];
            let first = profile.edges[0].clone();
            profile.edges.push(first);
        }),
        ("a dangling edge", |pack| {
            let profile = &mut pack.profiles[0];
            profile.edges[0].to = PhaseKey::parse("zz.nowhere").expect("a phase key");
        }),
        ("a rejection route that is not an ancestor", |pack| {
            let profile = &mut pack.profiles[0];
            let entry = profile.entry_phase.clone();
            let last = profile.phases[profile.phases.len() - 1].id.clone();
            for phase in &mut profile.phases {
                if phase.id == entry {
                    phase.rejection_route = Some(last.clone());
                }
            }
        }),
        ("a gate rejecting to its own phase", |pack| {
            let profile = &mut pack.profiles[0];
            let gate = &mut profile.gates[0];
            gate.rejection_target = gate.phase.clone();
        }),
        ("an undeclared terminal phase", |pack| {
            let profile = &mut pack.profiles[0];
            profile.terminal_phases = vec![PhaseKey::parse("zz.nowhere").expect("a phase key")];
        }),
        ("no terminal phase at all", |pack| {
            pack.profiles[0].terminal_phases.clear();
        }),
        ("an entry phase with an incoming edge", |pack| {
            let profile = &mut pack.profiles[0];
            let entry = profile.entry_phase.clone();
            let other = profile
                .phases
                .iter()
                .map(|phase| phase.id.clone())
                .find(|id| id != &entry)
                .expect("a second phase");
            profile.edges.push(kontor_core::spec::PhaseEdge {
                from: other,
                to: entry,
                handoff_role: None,
            });
        }),
        ("a gate listed by the wrong phase", |pack| {
            let profile = &mut pack.profiles[0];
            let gate = profile.gates[0].id.clone();
            let owner = profile.gates[0].phase.clone();
            for phase in &mut profile.phases {
                if phase.id == owner {
                    phase.gates.retain(|listed| listed != &gate);
                } else if phase.gates.is_empty() {
                    phase.gates.push(gate.clone());
                }
            }
        }),
        ("an artifact consumed before it is produced", |pack| {
            let profile = &mut pack.profiles[0];
            let terminal = profile.terminal_phases[0].clone();
            let entry = profile.entry_phase.clone();
            let late = profile
                .artifacts
                .iter()
                .map(|contract| contract.key.clone())
                .find(|key| {
                    profile
                        .artifacts
                        .iter()
                        .any(|contract| &contract.key == key && contract.producer_phase == terminal)
                })
                .expect("the terminal phase produces something");
            for phase in &mut profile.phases {
                if phase.id == entry {
                    phase.required_artifacts.push(late.clone());
                }
            }
        }),
        ("a dangling role pin", |pack| {
            pack.profiles[0].roles[0].version = SpecVersion::parse(9).expect("v9");
        }),
        ("a dangling skill pin", |pack| {
            pack.profiles[0].skills[0].version = SpecVersion::parse(9).expect("v9");
        }),
        ("a dangling team pin", |pack| {
            pack.profiles[0].team_template = Some(kontor_core::spec::TeamTemplateRef {
                template_id: kontor_core::id::TeamTemplateId::generate(),
                version: SpecVersion::FIRST,
            });
        }),
        ("a handoff to a role no slot fills", |pack| {
            let unfilled = RoleKey::parse("zz.unseated").expect("a role key");
            let profile = &mut pack.profiles[0];
            profile.roles.push(kontor_core::spec::RoleRef {
                role: unfilled.clone(),
                version: SpecVersion::FIRST,
            });
            profile.edges[0].handoff_role = Some(unfilled);
        }),
        ("a gate evaluator the team does not authorize", |pack| {
            let stranger = RoleKey::parse("zz.stranger").expect("a role key");
            let profile = &mut pack.profiles[0];
            profile.roles.push(kontor_core::spec::RoleRef {
                role: stranger.clone(),
                version: SpecVersion::FIRST,
            });
            profile.gates[0].evaluator_roles = vec![stranger];
        }),
        ("a seeded category pinning nothing", |pack| {
            for entry in &mut pack.manifest {
                if entry.availability == PackAvailability::Seeded {
                    entry.profile = None;
                    entry.profile_version = None;
                }
            }
        }),
        ("a manifest-only category pinning a profile", |pack| {
            let profile = pack.profiles[0].id.clone();
            for entry in &mut pack.manifest {
                if entry.availability == PackAvailability::ManifestOnly {
                    entry.profile = Some(profile.clone());
                    entry.profile_version = Some(SpecVersion::FIRST);
                }
            }
        }),
        ("a duplicate profile revision", |pack| {
            let first = pack.profiles[0].clone();
            pack.profiles.push(first);
        }),
        ("a duplicate role definition", |pack| {
            let first = pack.roles[0].clone();
            pack.roles.push(first);
        }),
    ];

    for (label, mutate) in cases {
        let mut broken = custom_a();
        mutate(&mut broken);
        assert!(
            validate_pack(&broken).is_err(),
            "{label} must be refused, but the pack validated"
        );
        let category = broken.manifest[0].category.clone();
        assert!(
            resolve_profile(&broken, &category, now()).is_err(),
            "{label} must not produce a resolved bundle"
        );
    }
    validate_pack(&custom_a()).expect("the untouched pack still validates");
}

// ---------------------------------------------------------------------------
// Revisions leave history alone (AC-1)
// ---------------------------------------------------------------------------

#[test]
fn a_profile_revision_publishes_n_plus_one_and_leaves_n_untouched() {
    let pack = custom_a();
    let first = pack.profiles[0].clone();
    let before = first.canonicalize().expect("v1 canonicalizes");

    let second = revise_work_profile(&first, |profile| {
        profile.budget_defaults.max_tokens += 1;
    })
    .expect("a bounded edit revises");

    assert_eq!(second.id, first.id, "the logical id is preserved");
    assert_eq!(second.version, SpecVersion::parse(2).expect("v2"));
    assert_eq!(first.version, SpecVersion::FIRST, "v1 is not mutated");

    let after = first.canonicalize().expect("v1 still canonicalizes");
    assert_eq!(after.json(), before.json(), "v1's bytes are unchanged");
    assert_eq!(after.hash(), before.hash(), "and so is its digest");
    assert_ne!(
        second.canonicalize().expect("v2 canonicalizes").hash(),
        before.hash()
    );

    // A snapshot taken from v1 keeps describing v1 after v2 exists.
    let pinned = ResolvedWorkProfileSnapshot::resolve(&first, now()).expect("v1 resolves");
    let _ = second;
    pinned.verify().expect("the v1 snapshot still verifies");
    assert_eq!(pinned.definition.version, SpecVersion::FIRST);

    assert!(
        revise_work_profile(&first, |profile| {
            profile.id = WorkProfileKey::parse("zz.renamed").expect("a profile key");
        })
        .is_err(),
        "a revision may not rename the profile"
    );
    assert!(
        revise_work_profile(&first, |profile| {
            profile.version = SpecVersion::parse(7).expect("v7");
        })
        .is_err(),
        "a revision may not choose its own version"
    );
}

#[test]
fn a_persona_revision_publishes_n_plus_one_and_leaves_n_untouched() {
    let pack = custom_b();
    let first = pack.personas[0].scenario.clone();
    let before = first.canonicalize().expect("v1 canonicalizes");

    let second = revise_persona_scenario(&first, |scenario| {
        scenario
            .prohibited_actions
            .push(kontor_core::id::ExternalName::parse("Also never do this").expect("a name"));
    })
    .expect("a bounded edit revises");

    assert_eq!(second.scenario_id, first.scenario_id);
    assert_eq!(second.version, SpecVersion::parse(2).expect("v2"));
    assert_eq!(
        first.canonicalize().expect("v1 still canonicalizes").hash(),
        before.hash(),
        "publishing v2 does not rewrite v1"
    );

    assert!(
        revise_persona_scenario(&first, |scenario| {
            scenario.scenario_id = kontor_core::id::PersonaScenarioId::generate();
        })
        .is_err(),
        "a revision may not re-identify the scenario"
    );
}

// ---------------------------------------------------------------------------
// Persona safety
// ---------------------------------------------------------------------------

#[test]
fn a_simulated_persona_cannot_approve_its_own_gate() {
    let pack = seeds();
    let persona = pack.personas.first().expect("the pack ships a persona");
    let profile = pack
        .profile(&persona.profile, persona.profile_version)
        .expect("its profile is carried");
    let gate = profile
        .gate(&persona.scenario.gate_under_test)
        .expect("its gate is declared");

    assert!(
        !gate.evaluator_roles.contains(&persona.scenario.actor_role),
        "the actor may not evaluate its own gate"
    );
    assert!(
        !gate.waiver_roles.contains(&persona.scenario.actor_role),
        "nor waive it"
    );
    assert!(
        !persona
            .scenario
            .evaluator_roles
            .contains(&persona.scenario.actor_role),
        "and the evaluators are somebody else"
    );
    assert!(
        !persona.scenario.evaluator_roles.is_empty(),
        "an independent verifier exists"
    );
    assert!(
        persona.scenario.identity.seeded,
        "the identity is seeded fixture data"
    );
    assert_ne!(
        persona.scenario.environment.kind,
        kontor_core::spec::EnvironmentKind::Production,
        "and never runs against production"
    );

    // The pinned bundle applies the same rule through the existing core proof.
    let category = pack
        .manifest
        .iter()
        .find(|entry| entry.profile.as_ref() == Some(&persona.profile))
        .expect("the profile is advertised")
        .category
        .clone();
    let bundle = resolve_profile(&pack, &category, now()).expect("it resolves");
    bundle
        .freeze_persona(&persona.scenario)
        .expect("an independent scenario freezes onto the pinned profile");
}

#[test]
fn a_malformed_persona_scenario_is_refused() {
    type Case = (&'static str, fn(&mut PersonaScenarioSpec));
    let cases: &[Case] = &[
        ("the actor is its own evaluator", |scenario| {
            let actor = scenario.actor_role.clone();
            scenario.evaluator_roles = vec![actor];
        }),
        ("an evaluator the gate never authorized", |scenario| {
            scenario.evaluator_roles = vec![RoleKey::parse("zz.stranger").expect("a role key")];
        }),
        ("no independent evaluator at all", |scenario| {
            scenario.evaluator_roles.clear();
        }),
        ("a production environment", |scenario| {
            scenario.environment.kind = kontor_core::spec::EnvironmentKind::Production;
        }),
        ("an identity that is not seeded", |scenario| {
            scenario.identity.seeded = false;
        }),
        ("evidence the profile does not declare", |scenario| {
            scenario.required_evidence = vec![ArtifactKey::parse("zz.ghost").expect("a key")];
        }),
        ("a gate the pinned profile does not declare", |scenario| {
            scenario.gate_under_test = GateKey::parse("zz.ghost-gate").expect("a gate key");
        }),
        ("no prohibited actions", |scenario| {
            scenario.prohibited_actions.clear();
        }),
        ("steps numbered out of order", |scenario| {
            scenario.steps[0].order = 7;
        }),
    ];

    for (label, mutate) in cases {
        let mut pack = custom_b();
        mutate(&mut pack.personas[0].scenario);
        assert!(
            validate_pack(&pack).is_err(),
            "{label} must be refused, but the pack validated"
        );
    }
}

/// The actor-holds-authority rule, isolated from every other check.
///
/// Each case leaves the profile, the team and the reference catalog *exactly* as
/// they were and moves only `actor_role` onto a role the pinned gate already
/// trusts. Core's own `validate()` is satisfied — the actor still names an
/// independent evaluator — so the pack rule is the only thing that can refuse,
/// which is what stops this test from passing for the wrong reason.
#[test]
fn the_simulated_actor_may_not_hold_authority_over_its_own_gate() {
    // Through the gate's own evaluator list, on a profile that pins no team.
    //
    // Dropping the team pin is what isolates the rule: with a team in play the
    // seat-authority check would refuse this too, and the test could not tell
    // which of the two did it. The gate needs two evaluators so the scenario
    // still names an independent one and core's `validate()` stays satisfied.
    let mut pack = seeds();
    let scenario = pack.personas[0].scenario.clone();
    let target = pack.personas[0].profile.clone();
    let target_version = pack.personas[0].profile_version;
    for profile in &mut pack.profiles {
        if profile.id == target && profile.version == target_version {
            profile.team_template = None;
        }
    }
    let profile = pack
        .profile(&target, target_version)
        .expect("the pinned profile")
        .clone();
    let gate = profile
        .gate(&scenario.gate_under_test)
        .expect("the pinned gate");
    assert!(
        gate.evaluator_roles.len() >= 2,
        "the gate under test has an independent pair of evaluators"
    );
    let borrowed = gate
        .evaluator_roles
        .iter()
        .find(|role| !scenario.evaluator_roles.contains(role))
        .expect("one evaluator the scenario does not already name")
        .clone();
    validate_pack(&pack).expect("dropping the team pin leaves a valid pack");
    pack.personas[0].scenario.actor_role = borrowed;
    pack.personas[0]
        .scenario
        .validate()
        .expect("the scenario is still well formed on its own");
    assert!(
        validate_pack(&pack).is_err(),
        "an actor that evaluates its own gate must be refused"
    );

    // Through the gate's waiver list, again with the team pin dropped so only
    // the gate's own authority can refuse.
    let mut pack = custom_b();
    pack.profiles[0].team_template = None;
    let profile = pack.profiles[0].clone();
    let gate = profile
        .gate(&pack.personas[0].scenario.gate_under_test)
        .expect("the pinned gate");
    let waiver = gate
        .waiver_roles
        .first()
        .expect("the gate allows a waiver")
        .clone();
    validate_pack(&pack).expect("dropping the team pin leaves a valid pack");
    pack.personas[0].scenario.actor_role = waiver;
    pack.personas[0]
        .scenario
        .validate()
        .expect("the scenario is still well formed on its own");
    assert!(
        validate_pack(&pack).is_err(),
        "an actor that waives its own gate must be refused"
    );

    // And through the team, even when the gate itself names the actor nowhere.
    // The seat keeps authority the profile's gate list does not repeat, which is
    // legal for a shared template — but not for the persona acting as that role.
    let mut pack = custom_b();
    let gate_id = pack.personas[0].scenario.gate_under_test.clone();
    for gate in &mut pack.profiles[0].gates {
        if gate.id == gate_id {
            gate.waiver_allowed = false;
            gate.waiver_roles.clear();
        }
    }
    let seat_role = {
        let slot = pack.teams[0]
            .slots
            .iter_mut()
            .find(|slot| slot.may_waive.contains(&gate_id))
            .expect("a seat carries waiver authority for that gate");
        slot.may_waive.clear();
        slot.may_evaluate.push(gate_id.clone());
        slot.role.role.clone()
    };
    validate_pack(&pack).expect("the doctored pack is otherwise valid");
    let gate = pack.profiles[0].gate(&gate_id).expect("the gate");
    assert!(
        !gate.evaluator_roles.contains(&seat_role) && !gate.waiver_roles.contains(&seat_role),
        "the gate itself grants that role nothing"
    );
    pack.personas[0].scenario.actor_role = seat_role;
    pack.personas[0]
        .scenario
        .validate()
        .expect("the scenario is still well formed on its own");
    assert!(
        validate_pack(&pack).is_err(),
        "a seat that can decide the gate is authority just as much as a gate role is"
    );
}

// ---------------------------------------------------------------------------
// Task closure (AC-6, AC-7)
// ---------------------------------------------------------------------------

#[test]
fn task_closure_requires_every_declared_phase_gate_and_artifact() {
    let pack = seeds();
    let (category, profile) = most_gated(&pack);
    let bundle = resolve_profile(&pack, &category, now()).expect("it resolves");
    let (phases, gates, artifacts) = fully_satisfied(&profile);
    let (team_run_id, certificate) = team_closure(&bundle);
    let team = TaskTeamEvidence::Certified {
        team_run_id,
        certificate: &certificate,
    };

    certify_task_closure(&bundle.profile, team, &phases, &gates, &artifacts, &[])
        .expect("a fully satisfied profile closes");

    for phase in &phases {
        let mut short = phases.clone();
        short.remove(phase);
        assert!(
            certify_task_closure(&bundle.profile, team, &short, &gates, &artifacts, &[]).is_err(),
            "omitting a phase must refuse closure"
        );
    }
    for gate in gates.keys() {
        let mut short = gates.clone();
        short.remove(gate);
        assert!(
            certify_task_closure(&bundle.profile, team, &phases, &short, &artifacts, &[]).is_err(),
            "omitting a gate verdict must refuse closure"
        );
    }
    for artifact in &artifacts {
        let mut short = artifacts.clone();
        short.remove(artifact);
        assert!(
            certify_task_closure(&bundle.profile, team, &phases, &gates, &short, &[]).is_err(),
            "omitting an artifact must refuse closure"
        );
    }
}

#[test]
fn a_waived_gate_needs_named_authorized_evidence_bearing_authority() {
    let pack = seeds();
    // Structurally: the one runnable profile that allows any gate to be waived.
    let (category, profile) = pack
        .manifest
        .iter()
        .filter(|entry| entry.availability == PackAvailability::Seeded)
        .filter_map(|entry| {
            let profile = pack.profile(entry.profile.as_ref()?, entry.profile_version?)?;
            profile
                .gates
                .iter()
                .any(|gate| gate.waiver_allowed)
                .then(|| (entry.category.clone(), profile.clone()))
        })
        .next()
        .expect("a seed profile allows a waiver");
    let bundle = resolve_profile(&pack, &category, now()).expect("it resolves");
    let waivable = profile
        .gates
        .iter()
        .find(|gate| gate.waiver_allowed)
        .expect("the waivable gate");

    let (phases, mut gates, artifacts) = fully_satisfied(&profile);
    gates.insert(waivable.id.clone(), GateState::Waived);
    let (team_run_id, certificate) = team_closure(&bundle);
    let team = TaskTeamEvidence::Certified {
        team_run_id,
        certificate: &certificate,
    };

    assert!(
        certify_task_closure(&bundle.profile, team, &phases, &gates, &artifacts, &[]).is_err(),
        "a bare waived state is an assertion, not authority"
    );

    let honest = GateWaiver {
        gate: waivable.id.clone(),
        authorized_by: waivable.waiver_roles[0].clone(),
        evidence: waivable.required_evidence.clone(),
        recorded_at: now(),
    };
    certify_task_closure(
        &bundle.profile,
        team,
        &phases,
        &gates,
        &artifacts,
        std::slice::from_ref(&honest),
    )
    .expect("an authorized, evidenced waiver closes the gate");

    let unauthorized = GateWaiver {
        authorized_by: RoleKey::parse("zz.stranger").expect("a role key"),
        ..honest.clone()
    };
    assert!(
        certify_task_closure(
            &bundle.profile,
            team,
            &phases,
            &gates,
            &artifacts,
            &[unauthorized]
        )
        .is_err(),
        "an unauthorized waiver must not close the gate"
    );

    let evidence_free = GateWaiver {
        evidence: Vec::new(),
        ..honest.clone()
    };
    assert!(
        certify_task_closure(
            &bundle.profile,
            team,
            &phases,
            &gates,
            &artifacts,
            &[evidence_free]
        )
        .is_err(),
        "an evidence-free waiver must not close the gate"
    );

    let unwaivable = profile
        .gates
        .iter()
        .find(|gate| !gate.waiver_allowed)
        .expect("a gate the profile forbids waiving");
    let forbidden = GateWaiver {
        gate: unwaivable.id.clone(),
        ..honest
    };
    let mut forced = gates.clone();
    forced.insert(unwaivable.id.clone(), GateState::Waived);
    assert!(
        certify_task_closure(
            &bundle.profile,
            team,
            &phases,
            &forced,
            &artifacts,
            &[forbidden]
        )
        .is_err(),
        "a gate the profile forbids waiving must not be waived"
    );
}

/// Finding 4 regression: profile closure and team closure are both required.
///
/// A task whose profile prescribes a team cannot close on the profile's
/// obligations alone — every phase, gate and artifact can be satisfied while a
/// role slot still holds a live session, and that must not be a terminal task.
#[test]
fn a_task_needs_its_team_closure_as_well_as_its_profile_closure() {
    let pack = seeds();
    let (category, profile) = most_gated(&pack);
    let bundle = resolve_profile(&pack, &category, now()).expect("it resolves");
    assert!(
        bundle.team.is_some(),
        "the profile under test prescribes a team"
    );
    let (phases, gates, artifacts) = fully_satisfied(&profile);

    // Everything the profile asks for, and no team evidence at all.
    assert_eq!(
        certify_task_closure(
            &bundle.profile,
            TaskTeamEvidence::NoTeam,
            &phases,
            &gates,
            &artifacts,
            &[]
        )
        .expect_err("a team-bearing task may not close on its profile alone"),
        DomainError::MissingEvidence {
            subject: "task closure",
            rule: "a task whose profile prescribes a team must present that team's closure",
        }
    );

    // A certificate for some other team run proves nothing about this one.
    let (team_run_id, certificate) = team_closure(&bundle);
    assert!(
        certify_task_closure(
            &bundle.profile,
            TaskTeamEvidence::Certified {
                team_run_id: TeamRunId::generate(),
                certificate: &certificate,
            },
            &phases,
            &gates,
            &artifacts,
            &[]
        )
        .is_err(),
        "a certificate for another team run must not close this task"
    );

    // Both halves together do close it.
    certify_task_closure(
        &bundle.profile,
        TaskTeamEvidence::Certified {
            team_run_id,
            certificate: &certificate,
        },
        &phases,
        &gates,
        &artifacts,
        &[],
    )
    .expect("profile closure plus team closure closes the task");

    // And team evidence for a profile that prescribes no team is a mismatch,
    // not a free pass.
    let mut teamless = profile.clone();
    teamless.team_template = None;
    let snapshot =
        ResolvedWorkProfileSnapshot::resolve(&teamless, now()).expect("it still resolves");
    assert!(
        certify_task_closure(
            &snapshot,
            TaskTeamEvidence::Certified {
                team_run_id,
                certificate: &certificate,
            },
            &phases,
            &gates,
            &artifacts,
            &[]
        )
        .is_err(),
        "team evidence for a teamless profile is a mismatch"
    );
}

/// AC-7. Nothing here reads a profile id: the multi-discipline profile is the
/// one with the most gates, and the "coding verdict" is the gate whose evidence
/// is a code change.
#[test]
fn a_coding_verdict_alone_does_not_close_a_multi_discipline_profile() {
    let pack = seeds();
    let (category, profile) = most_gated(&pack);
    let bundle = resolve_profile(&pack, &category, now()).expect("it resolves");

    let code_artifacts: BTreeSet<&ArtifactKey> = profile
        .artifacts
        .iter()
        .filter(|contract| contract.content_type == ArtifactContentType::CodeChange)
        .map(|contract| &contract.key)
        .collect();
    assert!(
        !code_artifacts.is_empty(),
        "the profile under test involves code"
    );
    let coding_gates: Vec<&kontor_core::spec::GateSpec> = profile
        .gates
        .iter()
        .filter(|gate| {
            gate.required_evidence
                .iter()
                .any(|evidence| code_artifacts.contains(evidence))
        })
        .collect();
    assert_eq!(coding_gates.len(), 1, "exactly one gate judges the code");

    let other_gates = profile.gates.len() - coding_gates.len();
    assert!(
        other_gates >= 4,
        "the profile declares design, functionality-QA, design-QA and audit \
         obligations beyond the coding one"
    );

    // Everything the coding discipline can legitimately produce and decide.
    let coding_phases: BTreeSet<PhaseKey> = coding_gates
        .iter()
        .map(|gate| gate.phase.clone())
        .chain(
            code_artifacts
                .iter()
                .filter_map(|key| profile.artifacts.iter().find(|c| &&c.key == key))
                .map(|contract| contract.producer_phase.clone()),
        )
        .collect();
    let coding_artifacts: BTreeSet<ArtifactKey> = profile
        .artifacts
        .iter()
        .filter(|contract| coding_phases.contains(&contract.producer_phase))
        .map(|contract| contract.key.clone())
        .collect();
    let coding_verdicts: BTreeMap<GateKey, GateState> = coding_gates
        .iter()
        .map(|gate| (gate.id.clone(), GateState::Passed))
        .collect();

    let (team_run_id, certificate) = team_closure(&bundle);
    let team = TaskTeamEvidence::Certified {
        team_run_id,
        certificate: &certificate,
    };
    let refused = certify_task_closure(
        &bundle.profile,
        team,
        &coding_phases,
        &coding_verdicts,
        &coding_artifacts,
        &[],
    );
    assert!(
        refused.is_err(),
        "the coding verdict alone must not close a profile with independent \
         design, functionality and audit obligations"
    );

    // The obligations that remain are exactly the non-coding ones.
    let outstanding: Vec<&GateKey> = profile
        .gates
        .iter()
        .map(|gate| &gate.id)
        .filter(|id| !coding_verdicts.contains_key(id))
        .collect();
    assert_eq!(outstanding.len(), other_gates);

    let (phases, gates, artifacts) = fully_satisfied(&profile);
    certify_task_closure(&bundle.profile, team, &phases, &gates, &artifacts, &[])
        .expect("supplying every declared verdict and artifact closes it");
}

#[test]
fn the_multi_discipline_closure_rule_survives_renaming_the_profile() {
    let pack = seeds();
    let (_, profile) = most_gated(&pack);

    let mut renamed = profile.clone();
    renamed.id = WorkProfileKey::parse("zz.anonymous").expect("a profile key");
    // The rule under test is the profile's own obligation graph, so the team
    // half is taken out of the picture entirely rather than satisfied.
    renamed.team_template = None;
    let snapshot = ResolvedWorkProfileSnapshot::resolve(&renamed, now())
        .expect("the renamed profile still resolves");

    let one_gate: BTreeMap<GateKey, GateState> = profile
        .gates
        .iter()
        .take(1)
        .map(|gate| (gate.id.clone(), GateState::Passed))
        .collect();
    assert!(
        certify_task_closure(
            &snapshot,
            TaskTeamEvidence::NoTeam,
            &BTreeSet::new(),
            &one_gate,
            &BTreeSet::new(),
            &[]
        )
        .is_err(),
        "the rule is structural, so renaming the profile changes nothing"
    );

    let (phases, gates, artifacts) = fully_satisfied(&renamed);
    certify_task_closure(
        &snapshot,
        TaskTeamEvidence::NoTeam,
        &phases,
        &gates,
        &artifacts,
        &[],
    )
    .expect("and satisfying it still closes");
}

// ---------------------------------------------------------------------------
// Context-window seeds
// ---------------------------------------------------------------------------

/// The seed table is data the bundle freezes, and the bundle digest covers it.
///
/// Nothing here names a role: the assertion is that whatever the pack seeds is
/// what the bundle carries, so a deployment with entirely different roles takes
/// the identical path.
#[test]
fn a_bundle_freezes_the_seeds_for_exactly_the_roles_it_selected() {
    let pack = seeds();
    let category = pack.runnable_categories()[0].clone();
    let bundle = resolve_profile(&pack, &category, now()).expect("the category resolves");

    let selected: BTreeSet<&kontor_core::id::RoleKey> =
        bundle.roles.iter().map(|role| &role.role).collect();
    for seed in &bundle.context_policy.role_seeds {
        assert!(
            selected.contains(&seed.role),
            "a bundle only seeds roles it actually selected"
        );
    }
    // Every seeded role the pack declares for this bundle is carried across.
    let expected = pack
        .role_context_seeds
        .iter()
        .filter(|seed| selected.contains(&seed.role))
        .count();
    assert_eq!(bundle.context_policy.role_seeds.len(), expected);

    bundle.verify().expect("the bundle matches its own digest");
}

/// Changing a seed changes the bundle digest, so a run pinned to a bundle hash
/// cannot silently inherit a re-tuned context policy.
#[test]
fn editing_a_seed_changes_the_bundle_digest() {
    let pack = seeds();
    let category = pack.runnable_categories()[0].clone();
    let before = resolve_profile(&pack, &category, now()).expect("it resolves");
    assert!(
        !before.context_policy.role_seeds.is_empty(),
        "the bundled pack seeds at least one selected role"
    );

    let mut retuned = pack.clone();
    for seed in &mut retuned.role_context_seeds {
        seed.context_window.class = match seed.context_window.class {
            kontor_core::spec::ContextWindowClass::Lean => {
                kontor_core::spec::ContextWindowClass::Deep
            }
            _ => kontor_core::spec::ContextWindowClass::Lean,
        };
    }
    let after = resolve_profile(&retuned, &category, now()).expect("it still resolves");
    assert_ne!(before.bundle_hash, after.bundle_hash);
}

/// A pack that seeds an explicit-only class is refused at resolution, so the
/// rule cannot be evaded by writing it into deployment data.
#[test]
fn a_pack_cannot_seed_an_explicit_only_class() {
    let mut pack = seeds();
    let category = pack.runnable_categories()[0].clone();
    let selected = resolve_profile(&pack, &category, now())
        .expect("it resolves")
        .roles[0]
        .role
        .clone();

    for seed in &mut pack.role_context_seeds {
        if seed.role == selected {
            seed.context_window.class = kontor_core::spec::ContextWindowClass::Extended;
        }
    }
    assert!(
        resolve_profile(&pack, &category, now()).is_err(),
        "a seeded explicit-only class is refused"
    );
}
